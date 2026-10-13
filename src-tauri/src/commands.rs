//! Tauri 命令层 —— WebView 与 Rust 后端之间的唯一接口。
//!
//! 设计原则：
//! - 前端**不感知**坐标系来源：所有出参坐标一律是 GCJ-02
//! - 前端**不感知**加密与密钥轮换
//! - 每个命令都返回 `Result<T, String>`，错误信息面向用户可读

use serde::Serialize;
use tauri::State;

use crate::api::Api;
use crate::gcj::{gcj02_to_wgs84, wgs84_to_gcj02, LngLat};
use crate::journey;
use crate::transit;
use crate::types::*;
use crate::{AppError, Result};

/// 应用级共享状态
pub struct AppState {
    pub api: Api,
    /// 高德 Web 服务客户端（地址搜索 / 步行路径）
    pub amap: crate::amap::Amap,
    /// 高德 Key。
    ///
    /// **刻意不放进前端、也不写进代码**：本仓库是公开的，
    /// 硬编码的 Key 会被爬虫抓走并刷爆配额；而且别人 clone 项目时
    /// 应该用自己的配额。由用户在设置页粘贴，这里缓存并提供给调用方。
    pub amap_key: std::sync::Mutex<Option<String>>,
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
            amap: crate::amap::Amap::new(),
            amap_key: std::sync::Mutex::new(None),
            line_cache: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            speed_models: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// 取当前高德 Key 的副本
    fn amap_key(&self) -> Option<String> {
        self.amap_key
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .filter(|k| !k.trim().is_empty())
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

    // 尚未发班的班次：解析成"距现在多少分钟"后按时间升序，
    // 这样 UI 能直接显示"下一班 X 分钟后发车"。
    let mut pending: Vec<(f64, String)> = raw
        .plans
        .iter()
        .filter_map(|p| {
            let t = p.trim();
            transit::minutes_until_departure(t, &hhmm).map(|m| (m, t.to_string()))
        })
        .collect();
    pending.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // 同一时刻可能重复出现（实测 715-0 有 ['10:25','10:25',...]），去重后更干净
    pending.dedup_by(|a, b| a.1 == b.1);

    Ok(LineRealtime {
        line_id: detail.base.line_id.clone(),
        line_name: detail.base.line_name.clone(),
        start_stop: detail.base.start_stop.clone(),
        end_stop: detail.base.end_stop.clone(),
        has_live_vehicle: !vehicles.is_empty(),
        in_service: summary.is_in_service(&hhmm),
        early_hour: detail.base.early_hour.clone(),
        last_hour: detail.base.last_hour.clone(),
        // 由在线车辆的班次号反推发车间隔：回答"错过这班要等多久"
        headway_min: transit::estimate_headway_min(&vehicles),
        pending_departures: pending.into_iter().map(|(_, t)| t).collect(),
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
///
/// # 请求预算
///
/// 这是全 App 最耗请求的接口。上游建议间隔 ≥10 秒，因此这里严格限制：
///
/// | 步骤 | 上限 | 说明 |
/// |---|---|---|
/// | 起终点周边站点 | 2 | `nearBy` 半径约 400m，固定返回 5 站 |
/// | 直达候选线路站点表 | 6 | 命中缓存则 0 请求 |
/// | 换乘候选线路站点表 | 8 | **仅在没有直达方案时才做** |
///
/// 站点表（`/bus/line`）在 `line_cache` 里长期缓存，所以**第二次规划几乎零请求**。
/// 如果你常坐的线路都在缓存里，规划是瞬时的。
///
/// 首次规划需要按 10 秒/请求节流，可能耗时较久。这是上游限流要求，
/// 不是可以绕过的实现问题。
#[tauri::command]
pub async fn plan_journey(
    state: State<'_, AppState>,
    query: JourneyQuery,
) -> Result<Vec<JourneyOption>> {
    let origin = LngLat::from(query.origin);
    let destination = LngLat::from(query.destination);
    if !origin.is_valid() {
        return Err(AppError::BadCoord(format!(
            "起点 ({}, {})",
            origin.lng, origin.lat
        )));
    }
    if !destination.is_valid() {
        return Err(AppError::BadCoord(format!(
            "终点 ({}, {})",
            destination.lng, destination.lat
        )));
    }

    const MAX_DIRECT_LINES: usize = 6;
    const MAX_TRANSFER_LINES: usize = 8;

    // ---- 1. 两侧周边站点（**并发**两个请求）----
    //
    // 早期是串行 await 两次，各自还要等一个全局 10 秒节流器，单这一步就 20 秒。
    // 现在 /nearBy 有独立分组（2 秒间隔）且可并发，这一步降到一次往返的量级。
    let (origin_nearby, dest_nearby) =
        futures::future::try_join(state.api.nearby(origin), state.api.nearby(destination))
            .await?;

    let to_candidates = |stops: &[NearbyStop]| -> Vec<journey::CandidateStop> {
        stops
            .iter()
            .filter_map(|s| {
                let p = s.point?;
                Some(journey::CandidateStop {
                    stop_id: s.stop_id.clone(),
                    stop_name: s.stop_name.clone(),
                    point: LngLat::from(p),
                    line_names: s.line_names.clone(),
                })
            })
            .collect()
    };

    let origin_stops = to_candidates(&origin_nearby);
    let dest_stops = to_candidates(&dest_nearby);

    log::info!(
        "规划开始：起点 {} 周边 {} 站 / 终点 {} 周边 {} 站 / 出发偏移 {:?} 分 / 到达约束 {:?} 分",
        origin,
        origin_stops.len(),
        destination,
        dest_stops.len(),
        query.depart_offset_min.or(query.depart_after_min),
        query.arrive_before_min
    );

    if origin_stops.is_empty() {
        log::warn!("规划失败：起点周边 400 米内没有公交站（{origin}）");
        return Err(AppError::BadCoord(
            "起点周边 400 米内没有查到公交站".into(),
        ));
    }
    if dest_stops.is_empty() {
        log::warn!("规划失败：终点周边 400 米内没有公交站（{destination}）");
        return Err(AppError::BadCoord(
            "终点周边 400 米内没有查到公交站".into(),
        ));
    }

    // ---- 2. 收集两侧线路号 ----
    let collect = |stops: &[journey::CandidateStop]| -> Vec<String> {
        let mut v: Vec<String> = stops
            .iter()
            .flat_map(|s| s.line_names.iter().cloned())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        v.sort();
        v
    };
    let origin_lines = collect(&origin_stops);
    let dest_lines = collect(&dest_stops);

    let dest_set: std::collections::HashSet<&String> = dest_lines.iter().collect();
    let shared: Vec<String> = origin_lines
        .iter()
        .filter(|l| dest_set.contains(l))
        .take(MAX_DIRECT_LINES)
        .cloned()
        .collect();

    log::info!(
        "候选线路：起点侧 {:?} / 终点侧 {:?} / 共有（直达）{:?}",
        origin_lines,
        dest_lines,
        shared
    );

    let mut inputs = journey::PlanInputs {
        origin_stops,
        dest_stops,
        ..Default::default()
    };

    // ---- 3. 直达候选的站点表（**并发**，命中缓存则不发请求）----
    let direct_names: Vec<String> = shared.clone();
    let direct_names_len = direct_names.len();
    let direct_loaded = futures::future::join_all(
        direct_names
            .iter()
            .map(|name| load_line_pair(&state, name)),
    )
    .await;
    for (name, dirs) in direct_names.into_iter().zip(direct_loaded) {
        if dirs.is_empty() {
            // 站点表拉不到就没法算里程与站序，这条线路等于废掉。
            // 这是"给不出方案"最常见的原因之一，必须记下来。
            log::warn!("直达候选线路 {name} 的站点表为空，已跳过");
        } else {
            inputs.lines.insert(name, dirs);
        }
    }
    log::info!(
        "直达阶段：可用线路 {} 条（候选 {} 条）",
        inputs.lines.len(),
        direct_names_len
    );

    // ---- 4. 换乘候选 —— 仅在没有直达方案时才做，且严格限量 ----
    if shared.is_empty() {
        log::info!("无直达线路，转入一次换乘搜索");
        let mut extra: Vec<String> = origin_lines
            .iter()
            .chain(dest_lines.iter())
            .filter(|l| !shared.contains(l))
            .take(MAX_TRANSFER_LINES)
            .cloned()
            .collect();
        extra.dedup();

        let extra_loaded = futures::future::join_all(
            extra.iter().map(|name| load_line_pair(&state, name)),
        )
        .await;
        for (name, dirs) in extra.into_iter().zip(extra_loaded) {
            if !dirs.is_empty() {
                inputs.lines.insert(name, dirs);
            }
        }
    }

    // ---- 5. 取在线车辆（**并发**，每条方向一个请求）----
    let all_line_ids: Vec<String> = inputs
        .lines
        .values()
        .flatten()
        .map(|d| d.base.line_id.clone())
        .filter(|id| !inputs.vehicles.contains_key(id))
        .collect();

    let fetched = futures::future::join_all(all_line_ids.iter().map(|line_id| {
        let api = &state.api;
        async move {
            match api.line_vehicles(line_id).await {
                Ok(v) => Some((line_id.clone(), v)),
                // 单条线路取车辆失败不该让整个规划失败
                Err(e) => {
                    log::warn!("取 {line_id} 在线车辆失败：{e}");
                    None
                }
            }
        }
    }))
    .await;

    let now = transit::local_now_stamp();
    for (line_id, raw) in fetched.into_iter().flatten() {
        let Some(detail) = inputs
            .lines
            .values()
            .flatten()
            .find(|d| d.base.line_id == line_id)
        else {
            continue;
        };
        let views: Vec<VehicleView> = raw
            .line_bus_dto_list
            .iter()
            .map(|v| transit::position_vehicle(v, detail, &now))
            .collect();
        inputs.vehicles.insert(line_id, views);
    }

    // 出发时刻：取 `departOffsetMin`（新字段）或 `departAfterMin`（旧字段名）。
    // 两者同义，保留旧名是为了兼容已装机的版本。
    let depart_offset = query
        .depart_offset_min
        .or(query.depart_after_min)
        .unwrap_or(0)
        .max(0) as f64;

    let plan_opts = journey::PlanOptions {
        walk_speed_m_per_min: query.walk_speed_m_per_min.unwrap_or(75.0),
        max_options: query.max_options.unwrap_or(5),
        depart_offset_min: depart_offset,
        ..Default::default()
    };

    let mut options = journey::plan_from(origin, destination, &inputs, &plan_opts);

    if options.is_empty() {
        // 这一条最重要：用户看到"暂时给不出方案"时，日志里要有足够线索
        log::warn!(
            "规划无结果：候选线路 {} 条已加载站点表、{} 条已取到实时车辆；             可能原因是候选车均已驶过上车站，或线路不在运营时段",
            inputs.lines.len(),
            inputs.vehicles.len()
        );
    } else {
        log::info!(
            "规划完成：{} 个方案，最优 {} 分钟（{} 段，换乘 {} 次）",
            options.len(),
            options[0].total_minutes as i64,
            options[0].legs.len(),
            options[0].transfers
        );
    }

    // ---- 6. 若有"最晚到达"约束，据此给每个方案算出可接受的出发上限 ----
    //
    // 注意语义：`depart_offset_min` 是用户**想几点走**，
    // 而 `arrive_before_min` 是用户**必须几点前到**。
    // 两者同时给时以后者为准来反推出发上限，但时间轴仍以
    // `depart_offset_min` 为基准 —— 这样"出发 18:30、到达 19:10、
    // 最晚 19:00 前到（已超）"是一致可读的，不会出现出发晚于到达。
    if let Some(deadline) = query.arrive_before_min {
        let deadline = deadline as f64;
        for o in &mut options {
            // 该方案最晚可以几点出发（相对现在）
            o.latest_depart_offset_min = journey::latest_departure(o, deadline);
            o.tags.push(if o.arrive_offset_min <= deadline + 0.5 {
                "赶得上".into()
            } else {
                "会迟到".into()
            });
        }
        // 赶得上的排前面；同组内按到达时刻升序
        options.sort_by(|a, b| {
            let ka = a.arrive_offset_min <= deadline + 0.5;
            let kb = b.arrive_offset_min <= deadline + 0.5;
            kb.cmp(&ka)
                .then(
                    a.arrive_offset_min
                        .partial_cmp(&b.arrive_offset_min)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
    }

    Ok(options)
}

/// 载入一条线路号的**所有方向**，站点表走长期缓存。
///
/// 一个方向一个 `LineDetail`：`/bus/detailData` 只返回单个方向的车辆，
/// 站点表也是按方向给的。规划时需要两个方向都在，才能判断乘车方向。
///
/// 返回 `Vec` 而不是直接写进 `PlanInputs`，是为了让调用方能用
/// `join_all` **并发**拉取多条线路——早期是串行 await，每条线路要等
/// 一次检索 + 两次站点表，八条线路就串成几十秒。
async fn load_line_pair(state: &AppState, line_name: &str) -> Vec<LineDetail> {
    // 先用 search 拿双向 lineId（明文接口）
    let summaries = match state.api.search_exact(line_name).await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("查询线路 {line_name} 失败：{e}");
            return Vec::new();
        }
    };

    // 两个方向的站点表也并发拉取
    let details = futures::future::join_all(
        summaries
            .iter()
            .take(2)
            .map(|s| state.line_detail_cached(&s.line_id)),
    )
    .await;

    details
        .into_iter()
        .filter_map(|r| match r {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn!("取站点表失败：{e}");
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 高德：地址搜索与步行路径
// ---------------------------------------------------------------------------

/// 高德 Key 是否已配置。**不返回 Key 本身**，只返回状态与掩码。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmapKeyStatus {
    pub configured: bool,
    /// 仅供确认是否填对，形如 `c269…b9f`
    pub masked: Option<String>,
}

#[tauri::command]
pub fn get_amap_key_status(state: State<'_, AppState>) -> AmapKeyStatus {
    match state.amap_key() {
        Some(k) if k.len() > 10 => AmapKeyStatus {
            configured: true,
            masked: Some(format!("{}…{}", &k[..4], &k[k.len() - 4..])),
        },
        Some(_) => AmapKeyStatus {
            configured: true,
            masked: Some("****".into()),
        },
        None => AmapKeyStatus {
            configured: false,
            masked: None,
        },
    }
}

/// 设置高德 Key（本地保存，不进入版本库）。
///
/// 传空字符串表示清除。
#[tauri::command]
pub async fn set_amap_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<AmapKeyStatus> {
    let trimmed = key.trim().to_string();

    if trimmed.is_empty() {
        if let Ok(mut g) = state.amap_key.lock() {
            *g = None;
        }
        let _ = crate::keyfile::remove(&app);
        return Ok(AmapKeyStatus {
            configured: false,
            masked: None,
        });
    }

    // 先落盘再更新内存：写盘失败时不要留下"看起来配好了"的状态
    crate::keyfile::save(&app, &trimmed)?;
    if let Ok(mut g) = state.amap_key.lock() {
        *g = Some(trimmed.clone());
    }

    Ok(AmapKeyStatus {
        configured: true,
        masked: Some(if trimmed.len() > 10 {
            format!("{}…{}", &trimmed[..4], &trimmed[trimmed.len() - 4..])
        } else {
            "****".into()
        }),
    })
}

/// 按关键词搜索地点。
///
/// 这是解决"公交接口搜不到小区名/写字楼名"的核心能力：
/// `"楚天雅苑"` / `"洪山科创大厦"` 这类输入，公交接口返回空，高德能定位。
///
/// 返回的坐标是 **GCJ-02**，可直接喂给 `get_nearby_stops` 求最近站点。
#[tauri::command]
pub async fn search_place(
    state: State<'_, AppState>,
    keyword: String,
    city: Option<String>,
) -> Result<Vec<crate::amap::Place>> {
    let key = state.amap_key().ok_or_else(|| {
        AppError::BadRequest(
            "尚未配置高德 Key。请到「我的 → 高德地图」填入后即可按地址搜索。".into(),
        )
    })?;
    state
        .amap
        .search_place(&key, &keyword, city.as_deref())
        .await
}

/// 步行路径规划。用于把"直线距离 × 绕路系数"的估算换成真实路径。
#[tauri::command]
pub async fn walking_route(
    state: State<'_, AppState>,
    from: GeoPoint,
    to: GeoPoint,
) -> Result<crate::amap::WalkingRoute> {
    let key = state.amap_key().ok_or_else(|| {
        AppError::BadRequest("尚未配置高德 Key".into())
    })?;
    state
        .amap
        .walking(&key, LngLat::from(from), LngLat::from(to))
        .await
}

/// 坐标 → 地址。用于把"当前位置"显示成人能读懂的地名。
#[tauri::command]
pub async fn reverse_geocode(
    state: State<'_, AppState>,
    point: GeoPoint,
) -> Result<Option<crate::amap::Place>> {
    let key = state.amap_key().ok_or_else(|| {
        AppError::BadRequest("尚未配置高德 Key".into())
    })?;
    state.amap.regeocode(&key, LngLat::from(point)).await
}

// ---------------------------------------------------------------------------
// 日志
// ---------------------------------------------------------------------------

/// 读取最近的日志（最新的在最后）。
///
/// UI 用来展示"刚才那次失败到底卡在哪一步"。默认取 300 条，
/// 足以覆盖一次完整的规划流程而不至于让 WebView 传输太多文本。
#[tauri::command]
pub fn get_logs(limit: Option<usize>) -> Vec<crate::applog::LogEntry> {
    let n = limit.unwrap_or(300).min(crate::applog::CAPACITY_PUBLIC);
    crate::applog::recent(n)
}

/// 清空内存中的日志缓冲（磁盘文件保留，便于对照历史）
#[tauri::command]
pub fn clear_logs() {
    crate::applog::clear();
    log::info!("用户清空了日志缓冲");
}

/// 把最近日志导出成文本文件，返回路径。
///
/// 返回路径而不是内容：日志可能有几百 KB，走 IPC 传字符串不划算，
/// 而且导出后用户可以直接用系统的"分享"功能发出来。
#[tauri::command]
pub fn export_logs() -> Result<String> {
    match crate::applog::export() {
        Ok(p) => {
            log::info!("日志已导出到 {p:?}");
            Ok(p.to_string_lossy().to_string())
        }
        Err(e) => Err(AppError::Internal(format!("导出日志失败：{e}"))),
    }
}

// ---------------------------------------------------------------------------
// 更新检查
// ---------------------------------------------------------------------------

/// GitHub Release 的最新版本信息（前端据此展示与跳转）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    pub published_at: String,
    pub notes: String,
    /// 全部资产名（前端按架构挑 APK）
    pub assets: Vec<String>,
}

/// 把 `v0.2.0` / `0.2.0` / `v0.2` 解析成可比较的数字序列。
///
/// 只取数字段，忽略 `-beta` 之类的预发布后缀 —— 当前项目不发预发布版，
/// 为它写完整语义化版本比较不划算。
fn parse_version(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .take(3)
        .map(|seg| {
            // 段内只取前导数字：`0` 与 `0rc1` 都算 0
            let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// `a` 是否比 `b` 新。缺失的段按 0 处理，所以 `0.2` 与 `0.2.0` 相等。
fn is_newer(a: &str, b: &str) -> bool {
    let (va, vb) = (parse_version(a), parse_version(b));
    for i in 0..va.len().max(vb.len()) {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 检查 GitHub 上是否有更新版本。
///
/// 返回 `None` 表示**已是最新**；返回 `Some` 表示有新版本。
/// 网络异常会走 `Err`，由前端显示为"检查失败"——
/// **绝不能把失败说成"已是最新"**，那是在骗用户。
#[tauri::command]
pub async fn check_update(
    state: State<'_, AppState>,
    current: String,
) -> Result<Option<ReleaseInfo>> {
    log::info!("检查更新：当前版本 {current}");

    let url = "https://api.github.com/repos/CoconutHR/whbus/releases/latest";
    let resp = state
        .api
        .http()
        .get(url)
        .header("Accept", "application/vnd.github+json")
        // GitHub API 对匿名请求限流较严，带 UA 是必需的礼貌
        .header("User-Agent", "whbus-app")
        .send()
        .await
        .map_err(|e| AppError::Http(format!("访问 GitHub 失败：{e}")))?;

    let status = resp.status();
    if !status.is_success() {
        log::warn!("检查更新失败：GitHub 返回 {status}");
        return Err(AppError::Http(format!(
            "GitHub 返回 {status}（可能是网络受限或触发限流）"
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Http(format!("解析 Release 信息失败：{e}")))?;

    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if tag.is_empty() {
        return Err(AppError::Http("Release 信息缺少 tag_name".into()));
    }

    if !is_newer(&tag, &current) {
        log::info!("已是最新版本（远端 {tag}，本地 {current}）");
        return Ok(None);
    }

    let published_at = body
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // 更新说明只取前 12 行：Release body 可能很长，UI 里也只展示摘要
    let notes = body
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .lines()
        .take(12)
        .collect::<Vec<_>>()
        .join("\n");

    let assets = body
        .get("assets")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    log::info!("发现新版本 {tag}，资产 {} 个", assets.len());
    Ok(Some(ReleaseInfo {
        version: tag,
        published_at,
        notes,
        assets,
    }))
}

// ---------------------------------------------------------------------------
// 预加载
// ---------------------------------------------------------------------------

/// 预热若干条线路的站点表与实时车辆，降低首次规划的等待。
///
/// # 为什么值得做
///
/// 一次规划要拉 8-16 条线路的站点表（每条 = 一次检索 + 两次站点表）。
/// 实测冷启动约 10-20 秒，而站点表当天不会变、命中缓存后是 0 请求。
/// 于是"用户打开 App 的那几秒"是白送的预热窗口 —— 用它把常用线路
/// 先拉进缓存，用户真正点「开始规划」时往往直接命中。
///
/// # 为什么要限量
///
/// 上游限流按端点分组（实时 2 秒 / 配置 1.5 秒）。一口气预加载十几条
/// 线路既会拖慢首屏，也可能触发服务端风控。所以这里：
/// - 最多 max_lines 条（默认 6）
/// - 分批而非一次性并发，给限流器留出节奏
/// - 全部失败都不算错（预加载本就是尽力而为）
#[tauri::command]
pub async fn preload_lines(
    state: State<'_, AppState>,
    line_names: Vec<String>,
    max_lines: Option<usize>,
) -> Result<usize> {
    let cap = max_lines.unwrap_or(6).min(20);
    let mut names: Vec<String> = line_names
        .into_iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    names.dedup();
    names.truncate(cap);

    if names.is_empty() {
        return Ok(0);
    }
    log::info!("开始预加载 {} 条线路：{:?}", names.len(), names);

    // 每批 3 条：与限流器的并发上限（6）留出余量，
    // 避免预加载把用户随后那次真实规划挤在后面排队。
    let mut loaded = 0usize;
    for chunk in names.chunks(3) {
        let results = futures::future::join_all(
            chunk.iter().map(|name| load_line_pair(&state, name)),
        )
        .await;
        for (name, dirs) in chunk.iter().zip(results) {
            if dirs.is_empty() {
                log::debug!("预加载 {name} 无结果（可能不在运营时段或站名不精确）");
            } else {
                loaded += dirs.len();
            }
        }
        // 批次之间稍作停顿，给上游限流器留节奏
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    log::info!("预加载完成：{loaded} 个方向已进缓存");
    Ok(loaded)
}
