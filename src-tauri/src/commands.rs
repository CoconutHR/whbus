//! Tauri 命令层 —— WebView 与 Rust 后端之间的唯一接口。
//!
//! 设计原则：
//! - 前端**不感知**坐标系来源：所有出参坐标一律是 GCJ-02
//! - 前端**不感知**加密与密钥轮换
//! - 每个命令都返回 `Result<T, String>`，错误信息面向用户可读

use serde::Serialize;
use tauri::State;

use crate::api::Api;
use crate::gcj::{LngLat, wgs84_to_gcj02, gcj02_to_wgs84};
use crate::transit;
use crate::types::*;
use crate::{AppError, Result};

/// 应用级共享状态
pub struct AppState {
    pub api: Api,
    /// 线路站点表长期缓存：`line_id -> LineDetail`
    ///
    /// 站点表几乎不变，而换乘规划需要反复读取多条线路的站点表。
    /// 不做这个缓存，一次规划会打出几十个请求。
    pub line_cache: tokio::sync::RwLock<std::collections::HashMap<String, LineDetail>>,
    /// 速度模型：按 `line_id` 累积历史行驶样本，让 ETA 越用越准
    pub speed_models: tokio::sync::RwLock<std::collections::HashMap<String, transit::SpeedModel>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            api: Api::new(),
            line_cache: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            speed_models: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// 取线路站点表，命中缓存则不发请求。
    async fn line_detail_cached(&self, line_id: &str) -> Result<LineDetail> {
        if let Some(hit) = self.line_cache.read().await.get(line_id) {
            return Ok(hit.clone());
        }
        let fresh = self.api.line_detail(line_id).await?;
        self.line_cache
            .write()
            .await
            .insert(line_id.to_string(), fresh.clone());
        Ok(fresh)
    }

    /// 取某条线路的速度模型快照
    async fn speed_model(&self, line_id: &str) -> transit::SpeedModel {
        self.speed_models
            .read()
            .await
            .get(line_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 密钥 / 诊断
// ---------------------------------------------------------------------------

/// 当前密钥状态。设置页用它显示"数据通道是否正常"。
#[tauri::command]
pub async fn get_api_state(state: State<'_, AppState>) -> Result<ApiState> {
    Ok(state.api.state().await)
}

// ---------------------------------------------------------------------------
// 搜索
// ---------------------------------------------------------------------------

/// 线路号检索。
///
/// 上游是**模糊匹配**：搜 `"72"` 会返回整个 72x 系列，因此这里按 `lineName`
/// 精确过滤优先。站名检索走 `search_stops`。
#[tauri::command]
pub async fn search_lines(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<Vec<LineSummary>> {
    state.api.search_exact(&keyword).await
}

/// 站名检索。
///
/// ⚠️ 实测：上游**只匹配站名子串**，不认地址、小区名、写字楼名。
/// `"常青花园"` ✅ / `"楚天雅苑"` ❌。地址 → 坐标请走高德 Web API。
///
/// ⚠️ 另实测：**收班后该接口整体返回空**（凌晨连 `"715"` 都搜不到），
/// 这是服务端时段行为而非故障，前端应据此走缓存降级路径。
#[tauri::command]
pub async fn search_stops(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<Vec<StopSummary>> {
    Ok(state.api.search(&keyword).await?.stops)
}

// ---------------------------------------------------------------------------
// 线路
// ---------------------------------------------------------------------------

/// 线路详情（含完整站点表）。结果在 Rust 侧长期缓存。
#[tauri::command]
pub async fn get_line_detail(
    state: State<'_, AppState>,
    line_id: String,
) -> Result<LineDetail> {
    state.line_detail_cached(&line_id).await
}

/// 线路实时状态：车辆位置 + 到站预判 + 首末班。
///
/// 这是首页卡片与线路详情页的主数据源。**一次调用只覆盖一个方向**。
#[tauri::command]
pub async fn get_line_vehicles(
    state: State<'_, AppState>,
    line_id: String,
) -> Result<LineRealtime> {
    let detail = state.line_detail_cached(&line_id).await?;
    let raw = state.api.line_vehicles(&line_id).await?;

    // gpsTime 与本地时钟同为北京时间，可直接比较；缺失时以本地时刻兜底
    let now = transit::local_now_stamp();

    let vehicles: Vec<VehicleView> = raw
        .line_bus_dto_list
        .iter()
        .map(|v| transit::position_vehicle(v, &detail, &now))
        .collect();

    let model = state.speed_model(&line_id).await;
    let arrivals = transit::arrivals_for_stops(&detail, &vehicles, &model);

    // 收班判定：依据首末班时间而非"有没有车"。
    // 两者语义不同——白天也可能暂时没车，深夜则是正常收班。
    let hhmm = format!("{}:{}", &now[8..10], &now[10..12]);
    let summary = LineSummary {
        line_id: detail.base.line_id.clone(),
        line_name: detail.base.line_name.clone(),
        start_stop: detail.base.start_stop.clone(),
        end_stop: detail.base.end_stop.clone(),
        early_hour: detail.base.early_hour.clone(),
        last_hour: detail.base.last_hour.clone(),
        price: detail.base.price.clone(),
        run_status: detail.base.line_type,
    };

    Ok(LineRealtime {
        line_id: detail.base.line_id.clone(),
        line_name: detail.base.line_name.clone(),
        start_stop: detail.base.start_stop.clone(),
        end_stop: detail.base.end_stop.clone(),
        has_live_vehicle: !vehicles.is_empty(),
        in_service: summary.is_in_service(&hhmm),
        early_hour: detail.base.early_hour.clone(),
        last_hour: detail.base.last_hour.clone(),
        stops: detail.stops,
        cumulative_meters: detail.cumulative_meters,
        total_meters: detail.total_meters,
        vehicles,
        arrivals,
    })
}

// ---------------------------------------------------------------------------
// 站点
// ---------------------------------------------------------------------------

/// 周边站点。
///
/// `point` 必须是 **GCJ-02**。来自系统定位的 WGS-84 坐标请先经
/// `wgs84_to_gcj02` 转换——武汉地区两者相差约 587m，
/// 而本接口搜索半径只有约 400m，不转换会查到完全错误的站点。
#[tauri::command]
pub async fn get_nearby_stops(
    state: State<'_, AppState>,
    point: GeoPoint,
) -> Result<Vec<NearbyStop>> {
    state.api.nearby(LngLat::from(point)).await
}

/// 站点各线路到站信息（原始形态，供调试与降级路径使用）。
#[tauri::command]
pub async fn get_stop_arrivals(
    state: State<'_, AppState>,
    stop_id: String,
) -> Result<serde_json::Value> {
    let data = state.api.stop_arrivals(&stop_id).await?;
    serde_json::to_value(data).map_err(|e| AppError::Internal(e.to_string()))
}

// ---------------------------------------------------------------------------
// 坐标转换
// ---------------------------------------------------------------------------

/// WGS-84 → GCJ-02（**定位出口必用**）
///
/// 前端的 `wgs84ToGcj02()` 调用的就是这个命令。
#[tauri::command]
pub fn wgs84_to_gcj02_cmd(point: GeoPoint) -> GeoPoint {
    wgs84_to_gcj02(LngLat::from(point)).into()
}

/// GCJ-02 → WGS-84
#[tauri::command]
pub fn gcj02_to_wgs84_cmd(point: GeoPoint) -> GeoPoint {
    gcj02_to_wgs84(LngLat::from(point)).into()
}

// ---------------------------------------------------------------------------
// 规划
// ---------------------------------------------------------------------------

/// 出行规划。起终点均为 **GCJ-02**。
#[tauri::command]
pub async fn plan_journey(
    _state: State<'_, AppState>,
    _query: JourneyQuery,
) -> Result<Vec<JourneyOption>> {
    // TODO(下一轮)：接入 journey::plan，需要先完成
    //   1) 候选站点枚举（nearBy + 本地站点索引）
    //   2) 站名 → lineId 的本地索引（避免反复打 search）
    //   3) 请求预算控制（站点表走 line_cache）
    Err(AppError::Internal(
        "规划功能尚在实现中（需先完成候选站枚举与本地线路索引）".into(),
    ))
}

/// 把 `Result` 序列化为前端友好的形态
#[allow(dead_code)]
pub fn to_wire<T: Serialize>(value: T) -> std::result::Result<T, String> {
    Ok(value)
}
