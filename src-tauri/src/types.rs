//! 领域模型与错误类型。
//!
//! 字段命名对前端友好（camelCase），并在 Rust 侧完成所有类型修正：
//! 公交 API 把 `latitude` / `longitude` / `gpsSpeed` / `busOrder` / `arrived`
//! 全部当**字符串**返回，前端不应该关心这层。

use serde::{Deserialize, Deserializer, Serialize};

use crate::gcj::LngLat;

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

/// 应用错误。
///
/// 必须实现 `Serialize`：Tauri 的 IPC 要求命令的 `Err` 类型可序列化，
/// 否则 `#[tauri::command]` 直接编译不过。
///
/// 序列化成 `{ kind, message }` 而非裸字符串，前端就能按 `kind` 做分支
/// （例如密钥失效 vs 网络故障要显示完全不同的引导），同时保留
/// `Display` 的中文可读文案。
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    #[error("网络请求失败：{0}")]
    Http(String),

    #[error("接口返回错误 {code}：{message}")]
    Api { code: String, message: String },

    #[error("加密层错误：{0}")]
    Crypto(String),

    #[error("响应格式不符合预期：{0}")]
    Shape(String),

    #[error("缺少必填参数：{0}")]
    BadRequest(String),

    #[error("该功能需要有效的坐标：{0}")]
    BadCoord(String),

    #[error("内部错误：{0}")]
    Internal(String),
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Http(e.to_string())
    }
}

impl From<crate::sm4crypto::CryptoError> for AppError {
    fn from(e: crate::sm4crypto::CryptoError) -> Self {
        AppError::Crypto(e.to_string())
    }
}

impl AppError {
    /// 是否为"密钥失效"——需要重新取 `/sec/list` 后重试一次
    pub fn is_stale_key(&self) -> bool {
        matches!(self, AppError::Api { code, .. } if code == "500001")
    }
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

// ---------------------------------------------------------------------------
// 反序列化工具
// ---------------------------------------------------------------------------

/// 把"字符串形式的数字"解析为 `f64`。空串 / 非法值 / null 都退化为 `None`。
///
/// 公交 API 把几乎所有数值字段都当字符串返回，而且是**带引号的字符串**，
/// 这让 `serde` 的默认行为失效。这里统一做兼容：既接受 `"30.48"` 也接受 `30.48`。
fn de_opt_f64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    use serde::de::Error as _;
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.parse::<f64>().map_err(D::Error::custom)?)
            }
        }
        Some(other) => {
            return Err(D::Error::custom(format!("期望数字，得到 {other}")));
        }
    })
}

/// 同 `de_opt_f64`，但解析为 `i64`。
fn de_opt_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(de_opt_f64(d)?.map(|f| f.round() as i64))
}

/// 前端要用的坐标点（camelCase，且明确标注是 GCJ-02）
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoPoint {
    pub lng: f64,
    pub lat: f64,
}

impl From<LngLat> for GeoPoint {
    fn from(p: LngLat) -> Self {
        Self {
            lng: p.lng,
            lat: p.lat,
        }
    }
}

impl From<GeoPoint> for LngLat {
    fn from(p: GeoPoint) -> Self {
        LngLat::new(p.lng, p.lat)
    }
}

// ---------------------------------------------------------------------------
// /sec/list
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct SecListData {
    pub version: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// /bus/search  —— 双模式：线路号 或 站名
// ---------------------------------------------------------------------------

/// 线路方向。`lineId` 形如 `420100-687-0`，末位 `0`/`1` 表示方向。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineSummary {
    pub line_id: String,
    pub line_name: String,
    pub start_stop: String,
    pub end_stop: String,
    pub early_hour: String,
    pub last_hour: String,
    pub price: String,
    /// 运营状态标记；实测同一线路双向可能不同
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub run_status: Option<i64>,
}

impl LineSummary {
    /// 方向后缀：`-0` / `-1`
    pub fn direction_suffix(&self) -> Option<&str> {
        self.line_id.rsplit('-').next()
    }

    /// 该方向是否正在运营（依据首末班时间与当前时刻）
    ///
    /// 用于夜间模式：收班后不再显示倒计时，改显示"今日已收班 / 明早首班"。
    pub fn is_in_service(&self, now_hhmm: &str) -> bool {
        let (Some(first), Some(last)) = (parse_hhmm(&self.early_hour), parse_hhmm(&self.last_hour))
        else {
            // 时间字段缺失时不武断判收班，交给实时数据说话
            return true;
        };
        let Some(now) = parse_hhmm(now_hhmm) else {
            return true;
        };
        if first <= last {
            (first..=last).contains(&now)
        } else {
            // 跨零点运营（如夜行线路 22:00-01:00）
            now >= first || now <= last
        }
    }
}

/// 解析 `HH:MM` 为分钟数。容忍 `5:30` 这类缺前导零的写法（实测存在）。
pub fn parse_hhmm(s: &str) -> Option<i32> {
    let s = s.trim();
    let (h, m) = s.split_once(':')?;
    let h: i32 = h.trim().parse().ok()?;
    let m: i32 = m.trim().parse().ok()?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 60 + m)
}

/// `/bus/search` 命中的站点（站名模式）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopSummary {
    pub stop_id: String,
    pub stop_name: String,
    #[serde(rename = "latitude", deserialize_with = "de_opt_f64", default)]
    pub lat: Option<f64>,
    #[serde(rename = "longitude", deserialize_with = "de_opt_f64", default)]
    pub lng: Option<f64>,
}

impl StopSummary {
    pub fn point(&self) -> Option<LngLat> {
        let (lng, lat) = (self.lng?, self.lat?);
        let p = LngLat::new(lng, lat);
        p.is_valid().then_some(p)
    }
}

/// `/bus/search` 的原始响应体（明文，无需密钥）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchData {
    #[serde(default)]
    pub line_detail_list: Vec<LineSummary>,
    /// 站名模式命中项。文档称恒为空，实测传站名时**有数据**。
    #[serde(default)]
    pub stop_detail_list: Vec<StopSummary>,
}

/// 给前端的统一搜索结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub lines: Vec<LineSummary>,
    pub stops: Vec<StopSummary>,
}

// ---------------------------------------------------------------------------
// /bus/line —— 线路详情 + 完整站点表
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineBaseDto {
    pub line_id: String,
    pub line_name: String,
    /// **反方向**的 lineId
    #[serde(default)]
    pub direction: Option<String>,
    pub start_stop: String,
    pub end_stop: String,
    #[serde(default)]
    pub early_hour: String,
    #[serde(default)]
    pub last_hour: String,
    #[serde(default)]
    pub price: String,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub line_type: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInfo {
    #[serde(default)]
    pub stop_name: Option<String>,
    /// 地铁线路号，如 "8"
    #[serde(default)]
    pub subway_line_no: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopDetail {
    pub stop_no: i64,
    pub stop_id: String,
    pub stop_name: String,
    #[serde(rename = "latitude", deserialize_with = "de_opt_f64", default)]
    pub lat: Option<f64>,
    #[serde(rename = "longitude", deserialize_with = "de_opt_f64", default)]
    pub lng: Option<f64>,
    /// 地铁换乘信息；可能不存在
    #[serde(default)]
    pub transfer_infos: Option<Vec<TransferInfo>>,
}

impl StopDetail {
    /// 终点站坐标可能是 `0.0,0.0`，必须判空
    pub fn point(&self) -> Option<LngLat> {
        let (lng, lat) = (self.lng?, self.lat?);
        let p = LngLat::new(lng, lat);
        p.is_valid().then_some(p)
    }

    /// 地铁换乘线路号（去重、排序），用于 UI 上的地铁徽标
    pub fn subway_lines(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .transfer_infos
            .as_ref()
            .map(|list| {
                list.iter()
                    .filter_map(|t| t.subway_line_no.clone())
                    .filter(|s| !s.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v.dedup();
        v
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineDetailData {
    pub line_detail_base_dto: LineBaseDto,
    #[serde(default)]
    pub stop_detail_list: Vec<StopDetail>,
}

/// 给前端的线路详情（含派生几何）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineDetail {
    pub base: LineBaseDto,
    pub stops: Vec<StopView>,
    /// 相邻站点累计里程（米），长度与 `stops` 一致。时间轴按此比例铺开。
    pub cumulative_meters: Vec<f64>,
    pub total_meters: f64,
}

/// 站点在前端的形态：坐标统一转成 `GeoPoint`，便于前端零转换使用
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopView {
    pub stop_no: i64,
    pub stop_id: String,
    pub stop_name: String,
    /// 缺失或为 0,0 时为 `None`——上游数据里有这种脏值
    pub point: Option<GeoPoint>,
    /// 地铁换乘线路号
    pub subway_lines: Vec<String>,
}

// ---------------------------------------------------------------------------
// /bus/detailData —— 实时车辆（核心接口）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawVehicle {
    pub bus_id: String,
    #[serde(default)]
    pub bus_no: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub bus_order: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub bus_type: Option<i64>,
    /// 速度，约 km/h。0 表示静止或等灯
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub gps_speed: Option<f64>,
    /// GPS 时间戳，格式 `yyyyMMddHHmmss`
    #[serde(default)]
    pub gps_time: Option<String>,
    #[serde(rename = "latitude", deserialize_with = "de_opt_f64", default)]
    pub lat: Option<f64>,
    #[serde(rename = "longitude", deserialize_with = "de_opt_f64", default)]
    pub lng: Option<f64>,
    /// `"0"` / `"1"`，是否已到达终点
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub arrived: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailData {
    /// 空数组表示该方向**确实没有在线车辆**（未发车/已回场），不是接口故障
    #[serde(default)]
    pub line_bus_dto_list: Vec<RawVehicle>,
    /// 实测恒为空，不要依赖
    #[serde(default)]
    pub plans: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// /bus/station —— 站点到站信息
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopDto {
    #[serde(default)]
    pub stop_id: Option<String>,
    #[serde(default)]
    pub stop_name: Option<String>,
    #[serde(rename = "latitude", deserialize_with = "de_opt_f64", default)]
    pub lat: Option<f64>,
    #[serde(rename = "longitude", deserialize_with = "de_opt_f64", default)]
    pub lng: Option<f64>,
}

/// 站点在某条线路上的到站信息。字段使用前缀缩写：
/// `n*` = 去程/上行，`p*` = 回程/下行。
///
/// **重要**：`nscs` / `nsns`（剩余站数）实测**恒为 -3 或 -2 等哨兵值**，
/// 即便该线路确有在线车辆时也如此。本项目**不使用这两个字段**，
/// "还有几站"一律由 `/bus/detailData` 的坐标自行推算。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StopLineInfo {
    /// 线路号
    #[serde(default)]
    pub ln: Option<String>,
    /// 该线路在此站的站序
    #[serde(default)]
    pub sn: Option<i64>,
    /// 去程 / 回程的 lineId，可直接喂给 `/bus/detailData`
    #[serde(default)]
    pub nli: Option<String>,
    #[serde(default)]
    pub pli: Option<String>,
    #[serde(default)]
    pub nesn: Option<String>,
    #[serde(default)]
    pub pesn: Option<String>,
    #[serde(default)]
    pub net: Option<String>,
    #[serde(default)]
    pub nlt: Option<String>,
    /// 剩余站数——**不可信**，保留仅为诊断
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub nscs: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub nsns: Option<i64>,
    /// 即将到站车辆数组，实测恒为空
    #[serde(default)]
    pub nbl: Option<Vec<serde_json::Value>>,
}

impl StopLineInfo {
    /// 该站该线路可用的方向（lineId 列表）
    pub fn direction_ids(&self) -> Vec<&str> {
        [self.nli.as_deref(), self.pli.as_deref()]
            .into_iter()
            .flatten()
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StopArrivalsData {
    #[serde(default)]
    pub stop_dto: Option<StopDto>,
    #[serde(default)]
    pub line_list: Vec<StopLineInfo>,
}

// ---------------------------------------------------------------------------
// /bus/nearBy —— 周边站点
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyStopRaw {
    pub stop_id: String,
    pub stop_name: String,
    #[serde(rename = "latitude", deserialize_with = "de_opt_f64", default)]
    pub lat: Option<f64>,
    #[serde(rename = "longitude", deserialize_with = "de_opt_f64", default)]
    pub lng: Option<f64>,
    /// ⚠️ 文档称单位是 km，**实测是米**（`0.1961` 表示就在原点，
    /// `259.6372` 表示 259m）。本项目一律丢弃该字段、自行用 haversine 计算。
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub distance: Option<f64>,
    /// 站点方向标记，如 `FX-DX`
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub line_list: Vec<StopLineInfo>,
}

/// 周边站点（前端形态，距离由本地重算）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyStop {
    pub stop_id: String,
    pub stop_name: String,
    pub point: Option<GeoPoint>,
    /// 本地重算的直线距离（米）。**不使用接口的 `distance`**
    pub distance_m: f64,
    pub direction: Option<String>,
    /// 经过该站的线路号
    pub line_names: Vec<String>,
    /// 经过该站的全部 lineId（双向都可能出现）
    pub line_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// 前端聚合视图
// ---------------------------------------------------------------------------

/// 线路实时状态。用于首页卡片与线路详情页。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRealtime {
    pub line_id: String,
    pub line_name: String,
    pub start_stop: String,
    pub end_stop: String,
    pub stops: Vec<StopView>,
    pub cumulative_meters: Vec<f64>,
    pub total_meters: f64,
    pub vehicles: Vec<VehicleView>,
    /// 该方向的到站预判（对"你所在的站"而言）
    pub arrivals: Vec<ArrivalView>,
    /// 是否有任何在线车辆。`false` 不等于是错误——收班/回场都是正常状态
    pub has_live_vehicle: bool,
    /// 是否处于运营时段内（依据首末班时间）
    pub in_service: bool,
    pub early_hour: String,
    pub last_hour: String,
}

/// 一辆车在 UI 上的形态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VehicleView {
    pub bus_id: String,
    /// 班次序号，仅用于排序与区分，不是车牌
    pub bus_order: Option<i64>,
    pub point: Option<GeoPoint>,
    pub speed_kmh: Option<f64>,
    /// 已吸附到线路走向上的里程（米）
    pub along_m: Option<f64>,
    /// 距线路走向的垂距（米）。过大说明 GPS 漂移或数据异常
    pub offset_m: Option<f64>,
    /// 车辆当前所在（或刚驶离）的站点序号
    pub at_stop_no: Option<i64>,
    /// 下一站序号
    pub next_stop_no: Option<i64>,
    /// 是否已到终点
    pub arrived: bool,
    /// GPS 时间戳原样保留，前端据此显示数据新鲜度
    pub gps_time: Option<String>,
    /// 数据年龄（秒）。用于"数据延迟"提示
    pub age_seconds: Option<i64>,
    /// 位置可信度：真实 GPS / 历史推测
    pub confidence: PositionConfidence,
}

/// 位置可信度——直接驱动 UI 上"实线 vs 虚线"的表达
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PositionConfidence {
    /// 真实 GPS 且新鲜（< 3 分钟）
    Live,
    /// 真实 GPS 但数据偏旧（3-10 分钟）——UI 用降饱和表达
    Delayed,
    /// 由历史行驶速度推测——UI 必须用虚线 + "推测"角标
    Inferred,
    /// 数据过旧或缺失，位置不可用
    Stale,
}

/// 到站预判
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrivalView {
    pub stop_id: String,
    pub stop_no: i64,
    pub stop_name: String,
    pub bus_id: String,
    pub bus_order: Option<i64>,
    /// 距该站还有几站
    pub stops_away: i64,
    /// 沿线路剩余距离（米）——比站数更精确
    pub meters_away: f64,
    /// 预计到达所需分钟数的**区间下界**
    pub eta_min_low: f64,
    /// 预计到达所需分钟数的**区间上界**
    ///
    /// 用区间而非单值表达，是因为没有班次计划表、ETA 本质是概率。
    /// 历史样本越多，区间越窄——UI 上直接体现"越用越准"。
    pub eta_min_high: f64,
    pub confidence: PositionConfidence,
}

// ---------------------------------------------------------------------------
// 出行规划
// ---------------------------------------------------------------------------

/// 规划请求
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyQuery {
    pub origin: GeoPoint,
    pub destination: GeoPoint,
    /// 最早可出发时间（分钟偏移，0 = 现在）
    #[serde(default)]
    pub depart_after_min: Option<i64>,
    /// 必须在此刻之前到达（分钟偏移，可选）
    #[serde(default)]
    pub arrive_before_min: Option<i64>,
    /// 步行速度 m/min，缺省 75（≈4.5 km/h）
    #[serde(default)]
    pub walk_speed_m_per_min: Option<f64>,
    /// 最多返回几个方案
    #[serde(default)]
    pub max_options: Option<usize>,
}

/// 一个出行方案
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyOption {
    /// 总耗时（分钟，含步行与候车）
    pub total_minutes: f64,
    /// 需要步行的总距离（米）
    pub walk_meters: f64,
    /// 换乘次数
    pub transfers: usize,
    /// 最晚出发时刻（相对现在的分钟偏移）
    pub latest_depart_offset_min: f64,
    /// 预计到达时刻（相对现在的分钟偏移）
    pub arrive_offset_min: f64,
    pub legs: Vec<JourneyLeg>,
    /// 该方案的标签，如"最快""少步行""更晚出发"
    pub tags: Vec<String>,
    /// 排序用的原始评分（越小越好）
    pub score: f64,
}

/// 方案中的一段
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyLeg {
    pub kind: LegKind,
    /// 该段耗时（分钟）
    pub minutes: f64,
    /// 起点名（步行段为"当前位置"等）
    pub from_name: String,
    /// 终点名
    pub to_name: String,
    /// 步行距离（米），仅步行段有值
    pub meters: Option<f64>,
    /// 乘坐的线路
    pub line_id: Option<String>,
    pub line_name: Option<String>,
    /// 上车站序号
    pub board_stop_no: Option<i64>,
    /// 下车站序号
    pub alight_stop_no: Option<i64>,
    /// 乘坐站数
    pub stop_count: Option<i64>,
    /// 该段的预计区间（分钟，相对现在）
    pub start_offset_min: f64,
    pub end_offset_min: f64,
    /// 该段所属车辆的实时位置（若能关联上）
    pub vehicle: Option<VehicleView>,
    /// 该段数据的可信度
    pub confidence: PositionConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LegKind {
    Walk,
    Bus,
    Wait,
    Transfer,
}

// ---------------------------------------------------------------------------
// 密钥状态（暴露给前端做诊断）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiState {
    /// 当前密钥版本（yyyyMMdd）
    pub secret_version: Option<String>,
    /// 密钥是否已就绪
    pub key_ready: bool,
    /// 服务端时间（由最近一次成功请求推断），用于校准本地时钟
    pub server_hint: Option<String>,
    /// 累计发出的请求数（本次运行内）
    pub request_count: u64,
    /// 累计网络往返耗时（毫秒，不含人为限流等待）
    pub request_millis: u64,
    /// 平均每次往返耗时（毫秒）
    pub avg_millis: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_numbers() {
        // 上游把数值当字符串返回，这是最常见的一类坑
        let v: RawVehicle = serde_json::from_str(
            r#"{"busId":"abc","busOrder":"30","gpsSpeed":"7",
                "gpsTime":"20261012220716","latitude":"30.55105293747686",
                "longitude":"114.30096087131294","arrived":"1"}"#,
        )
        .unwrap();
        assert_eq!(v.bus_order, Some(30));
        assert_eq!(v.gps_speed, Some(7.0));
        assert_eq!(v.arrived, Some(1));
        assert!((v.lat.unwrap() - 30.55105293747686).abs() < 1e-12);
    }

    #[test]
    fn tolerates_missing_and_empty_numeric_fields() {
        let v: RawVehicle =
            serde_json::from_str(r#"{"busId":"abc","gpsSpeed":"","latitude":null}"#).unwrap();
        assert_eq!(v.gps_speed, None);
        assert_eq!(v.lat, None);
        assert_eq!(v.bus_order, None);
    }

    #[test]
    fn empty_vehicle_list_is_not_an_error() {
        // 收班/回场时返回空数组，是正常状态而非故障
        let d: DetailData = serde_json::from_str(r#"{"lineBusDtoList":[],"plans":[]}"#).unwrap();
        assert!(d.line_bus_dto_list.is_empty());
    }

    #[test]
    fn stop_detail_rejects_zero_coords() {
        let s: StopDetail = serde_json::from_str(
            r#"{"stopNo":31,"stopId":"X","stopName":"终点站",
                "latitude":"0.0","longitude":"0.0"}"#,
        )
        .unwrap();
        assert!(s.point().is_none(), "0,0 哨兵值必须被视为无坐标");

        let ok: StopDetail = serde_json::from_str(
            r#"{"stopNo":1,"stopId":"Y","stopName":"起点站",
                "latitude":"30.465516","longitude":"114.391605"}"#,
        )
        .unwrap();
        assert!(ok.point().is_some());
    }

    #[test]
    fn extracts_subway_transfer_lines() {
        let s: StopDetail = serde_json::from_str(
            r#"{"stopNo":30,"stopId":"X","stopName":"中山路螃蟹岬",
                "latitude":"30.549872","longitude":"114.317911",
                "transferInfos":[{"subwayLineNo":"2"},{"subwayLineNo":"7"},
                                 {"subwayLineNo":"2"},{}]}"#,
        )
        .unwrap();
        assert_eq!(s.subway_lines(), vec!["2", "7"], "应去重并排序");
    }

    #[test]
    fn service_window_handles_padded_and_unpadded_hours() {
        // 715 路实测返回 "5:30-21:00"，缺前导零
        let l: LineSummary = serde_json::from_str(
            r#"{"lineId":"420100-715-0","lineName":"715","startStop":"A","endStop":"B",
                "earlyHour":"05:30","lastHour":"21:00","price":"2","runStatus":0}"#,
        )
        .unwrap();
        assert!(l.is_in_service("12:00"));
        assert!(l.is_in_service("05:30"), "首班时刻应算在运营内");
        assert!(l.is_in_service("21:00"), "末班时刻应算在运营内");
        assert!(!l.is_in_service("23:30"), "深夜应判为收班");
        assert!(!l.is_in_service("04:00"), "凌晨应判为收班");
        assert_eq!(l.direction_suffix(), Some("0"));
    }

    #[test]
    fn service_window_handles_overnight_lines() {
        // 夜行线路跨零点
        let l: LineSummary = serde_json::from_str(
            r#"{"lineId":"420100-夜行518-1","lineName":"夜行518","startStop":"A","endStop":"B",
                "earlyHour":"22:00","lastHour":"01:00","price":"2","runStatus":0}"#,
        )
        .unwrap();
        assert!(l.is_in_service("23:30"));
        assert!(l.is_in_service("00:30"), "跨零点后仍应在运营");
        assert!(!l.is_in_service("12:00"), "白天应判为不在运营");
    }

    #[test]
    fn parses_hhmm_variants() {
        assert_eq!(parse_hhmm("05:30"), Some(330));
        assert_eq!(parse_hhmm("5:30"), Some(330));
        assert_eq!(parse_hhmm(" 21:00 "), Some(1260));
        assert_eq!(parse_hhmm("25:00"), None);
        assert_eq!(parse_hhmm("abc"), None);
        assert_eq!(parse_hhmm(""), None);
    }

    #[test]
    fn stop_line_info_collects_both_directions() {
        let s: StopLineInfo = serde_json::from_str(
            r#"{"ln":"331","sn":1,"nli":"420100-331-0","pli":"420100-331-1",
                "nscs":-3,"nsns":-3,"nbl":[]}"#,
        )
        .unwrap();
        assert_eq!(s.direction_ids(), vec!["420100-331-0", "420100-331-1"]);
        // nscs 是哨兵值，本模块不暴露给业务逻辑
        assert_eq!(s.nscs, Some(-3));
    }
}
