//! 武汉公交 `gps-resapi` 客户端。
//!
//! # 为什么所有网络请求都在 Rust 侧
//!
//! 1. **密钥隔离**：SM4 密钥每日轮换，绝不能进入 WebView
//! 2. **无 CORS**：WebView 直连第三方 API 会被同源策略拦住
//! 3. **统一限流**：文档要求轮询间隔 ≥ 10s，限流器集中一处才不会失控
//! 4. **统一退避**：上游偶发超时（实测 `curl rc=28`），需要重试策略
//!
//! # 请求预算
//!
//! 这个 API 的请求数是本项目的隐藏瓶颈：一次换乘规划的朴素穷举
//! 可能打出 40+ 个请求。因此 `Api` 内置了：
//! - 密钥按版本缓存（不必每次调用前都取 `/sec/list`）
//! - 最小请求间隔（默认 10s，符合上游建议）
//!
//! 站点表（`/bus/line`）几乎不变，调用方应自行长期缓存。

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::gcj::{LngLat, distance_m};
use crate::sm4crypto::SecretMaterial;
use crate::types::*;
use crate::{AppError, Result};

/// 服务地址
pub const BASE_URL: &str = "https://customized.wuhanbus.com/gps-resapi";
/// 城市码（武汉，GB/T 2260）
pub const CITY_CODE: &str = "420100";
/// 渠道号 —— **必填**，缺失会得到 199999 / 999999
pub const CHANNEL_ID: &str = "23000003";
/// `/sec/list` 的固定 appId
pub const XIAOMA_APP_ID: &str = "A0CADFE6F3B914A2";
/// 上游建议的轮询间隔下限
pub const MIN_REQUEST_GAP: Duration = Duration::from_secs(10);

/// 响应信封
#[derive(Debug, serde::Deserialize)]
struct Envelope {
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    msg: Option<Msg>,
    #[serde(default)]
    success: bool,
}

#[derive(Debug, serde::Deserialize)]
struct Msg {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl Envelope {
    fn into_result(self) -> Result<Option<Value>> {
        if self.success {
            return Ok(self.data);
        }
        let (code, message) = self
            .msg
            .map(|m| (m.code.unwrap_or_default(), m.message.unwrap_or_default()))
            .unwrap_or_default();
        Err(AppError::Api { code, message })
    }
}

/// 公交 API 客户端。线程安全，可放入 Tauri 的 managed state 共享。
pub struct Api {
    http: reqwest::Client,
    base: String,
    /// 当日密钥缓存（按 `version` 自动失效，无需手动清理）
    secret: Arc<Mutex<Option<SecretMaterial>>>,
    /// 限流：上一次请求的时刻
    last_request: Arc<Mutex<Option<Instant>>>,
    /// 最小请求间隔
    min_gap: Duration,
}

impl Api {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            // 上游偶发 TLS 读超时，让连接池不复用太久
            .pool_idle_timeout(Duration::from_secs(30))
            .user_agent("whbus/0.1 (+https://github.com/CoconutHR/whbus)")
            .build()
            .expect("构建 HTTP 客户端失败");

        Self {
            http,
            base: BASE_URL.to_string(),
            secret: Arc::new(Mutex::new(None)),
            last_request: Arc::new(Mutex::new(None)),
            min_gap: MIN_REQUEST_GAP,
        }
    }

    #[cfg(test)]
    pub fn with_base(base: impl Into<String>, min_gap: Duration) -> Self {
        let mut api = Self::new();
        api.base = base.into();
        api.min_gap = min_gap;
        api
    }

    // -----------------------------------------------------------------------
    // 传输层
    // -----------------------------------------------------------------------

    /// 限流：保证两次请求之间至少间隔 `min_gap`。
    ///
    /// 文档明确建议轮询间隔 ≥ 10s。这是对上游服务的基本尊重，
    /// 也避免我们的 IP 被限流。
    async fn throttle(&self) {
        let mut guard = self.last_request.lock().await;
        if let Some(prev) = *guard {
            let elapsed = prev.elapsed();
            if elapsed < self.min_gap {
                let wait = self.min_gap - elapsed;
                drop(guard);
                tokio::time::sleep(wait).await;
                guard = self.last_request.lock().await;
            }
        }
        *guard = Some(Instant::now());
    }

    /// 发送一次 POST，返回响应信封。
    async fn post_once(&self, path: &str, payload: &Value) -> Result<Envelope> {
        self.throttle().await;

        let url = format!("{}{}", self.base, path);
        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("channelId", CHANNEL_ID)
            .header("cityCode", CITY_CODE)
            .json(payload)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(AppError::Shape(format!(
                "HTTP {status}：{}",
                text.chars().take(200).collect::<String>()
            )));
        }

        serde_json::from_str(&text).map_err(|e| {
            AppError::Shape(format!(
                "无法解析响应信封：{e}；原文前 200 字符：{}",
                text.chars().take(200).collect::<String>()
            ))
        })
    }

    /// 明文接口：响应 `data` 直接是 JSON。
    async fn call_plain<T: DeserializeOwned>(&self, path: &str, payload: Value) -> Result<T> {
        let env = self.post_once(path, &payload).await?;
        let data = env.into_result()?;
        match data {
            Some(v) => serde_json::from_value(v).map_err(|e| {
                AppError::Shape(format!("{path} 的 data 结构与预期不符：{e}"))
            }),
            None => Err(AppError::Shape(format!("{path} 未返回 data"))),
        }
    }

    /// 加密接口：响应 `data` 是 hex 密文。
    ///
    /// 内部处理"密钥失效"（500001）——自动重新取一次密钥并重试一次。
    async fn call_encrypted<T: DeserializeOwned>(
        &self,
        path: &str,
        mut payload: Value,
    ) -> Result<T> {
        let mut retried = false;

        loop {
            let secret = self.secret_material().await?;
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("secretVersion".into(), json!(secret.version));
                obj.insert("cityCode".into(), json!(CITY_CODE));
            }

            let env = self.post_once(path, &payload).await;
            match env {
                Ok(env) => match env.into_result() {
                    Ok(Some(Value::String(cipher))) => {
                        return crate::sm4crypto::decrypt_data::<T>(&cipher, &secret.key)
                            .map_err(AppError::from);
                    }
                    // 极少数接口可能直接返回明文对象，兼容一下
                    Ok(Some(v)) => {
                        return serde_json::from_value(v).map_err(|e| {
                            AppError::Shape(format!("{path} 的 data 结构与预期不符：{e}"))
                        });
                    }
                    Ok(None) => return Err(AppError::Shape(format!("{path} 未返回 data"))),
                    Err(e) if e.is_stale_key() && !retried => {
                        retried = true;
                        log::warn!("密钥已失效，重新获取后重试 {path}");
                        self.invalidate_secret().await;
                        continue;
                    }
                    Err(e) => return Err(e),
                },
                Err(e) if !retried => {
                    // 上游偶发 TLS 读超时，退避后重试一次
                    retried = true;
                    log::warn!("{path} 请求失败，1 秒后重试：{e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    // -----------------------------------------------------------------------
    // 密钥
    // -----------------------------------------------------------------------

    /// 取当日密钥素材，按 `version` 缓存。
    pub async fn secret_material(&self) -> Result<SecretMaterial> {
        {
            let guard = self.secret.lock().await;
            if let Some(m) = guard.as_ref() {
                return Ok(m.clone());
            }
        }
        let fresh = self.fetch_secret().await?;
        let mut guard = self.secret.lock().await;
        *guard = Some(fresh.clone());
        Ok(fresh)
    }

    async fn invalidate_secret(&self) {
        let mut guard = self.secret.lock().await;
        *guard = None;
    }

    async fn fetch_secret(&self) -> Result<SecretMaterial> {
        let data: SecListData = self
            .call_plain(
                "/sec/list",
                json!({ "cityCode": CITY_CODE, "xiaomaAppId": XIAOMA_APP_ID }),
            )
            .await?;
        SecretMaterial::derive(data.version, &data.content).map_err(AppError::from)
    }

    /// 供前端诊断用
    pub async fn state(&self) -> ApiState {
        let guard = self.secret.lock().await;
        ApiState {
            secret_version: guard.as_ref().map(|m| m.version.clone()),
            key_ready: guard.is_some(),
            server_hint: None,
        }
    }

    // -----------------------------------------------------------------------
    // 业务接口
    // -----------------------------------------------------------------------

    /// 线路号检索。**模糊匹配**——搜 `"72"` 会返回 `72`/`720`/`721`/… 整个系列。
    ///
    /// 需要精确匹配时由调用方按 `lineName` 过滤。
    pub async fn search(&self, keyword: &str) -> Result<SearchResult> {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            return Err(AppError::BadRequest("搜索关键词不能为空".into()));
        }

        let data: SearchData = self
            .call_plain(
                "/bus/search",
                json!({ "keyword": keyword, "cityCode": CITY_CODE }),
            )
            .await?;

        Ok(SearchResult {
            lines: data.line_detail_list,
            stops: data.stop_detail_list,
        })
    }

    /// 线路号精确检索：先模糊搜，再按 `lineName` 过滤。
    pub async fn search_exact(&self, line_no: &str) -> Result<Vec<LineSummary>> {
        let all = self.search(line_no).await?;
        let want = line_no.trim();
        let exact: Vec<LineSummary> = all
            .lines
            .iter()
            .filter(|l| l.line_name.trim() == want)
            .cloned()
            .collect();

        // 精确命中为空时退回模糊结果——用户搜 "夜行518" 这类带前缀的线路号时
        // 站名/线路名的写法可能与输入不完全一致
        if exact.is_empty() {
            Ok(all.lines)
        } else {
            Ok(exact)
        }
    }

    /// 线路详情 + 完整站点表。**结果应长期缓存**——站点表几乎不变。
    pub async fn line_detail(&self, line_id: &str) -> Result<LineDetail> {
        if line_id.trim().is_empty() {
            return Err(AppError::BadRequest("lineId 不能为空".into()));
        }
        // 注意参数名是 `lineCode`，不是 `lineId`
        let data: LineDetailData = self
            .call_encrypted("/bus/line", json!({ "lineCode": line_id }))
            .await?;

        Ok(build_line_detail(data))
    }

    /// 实时车辆位置。**注意只返回该 `lineId` 对应方向的车**，查双向要调两次。
    pub async fn line_vehicles(&self, line_id: &str) -> Result<DetailData> {
        if line_id.trim().is_empty() {
            return Err(AppError::BadRequest("lineId 不能为空".into()));
        }
        // 注意这里的参数名是 `lineId`
        self.call_encrypted("/bus/detailData", json!({ "lineId": line_id }))
            .await
    }

    /// 站点各线路到站信息。
    ///
    /// ⚠️ 其中的 `nscs` / `nsns`（剩余站数）实测恒为哨兵值，本项目不使用。
    pub async fn stop_arrivals(&self, stop_id: &str) -> Result<StopArrivalsData> {
        if stop_id.trim().is_empty() {
            return Err(AppError::BadRequest("stopId 不能为空".into()));
        }
        // 注意参数名是 `destinationId`
        self.call_encrypted("/bus/station", json!({ "destinationId": stop_id }))
            .await
    }

    /// 周边站点。
    ///
    /// 实测行为（与文档不符，以实测为准）：
    /// - 半径约 **400 米**，固定返回 **5 个站**
    /// - 会同时返回同一物理站点的东行/西行两个 stopId
    /// - `distance` 字段**单位是米而非文档所述的公里**，本方法一律丢弃它、
    ///   用 haversine 本地重算
    pub async fn nearby(&self, center: LngLat) -> Result<Vec<NearbyStop>> {
        if !center.is_valid() {
            return Err(AppError::BadCoord(format!(
                "({}, {})",
                center.lng, center.lat
            )));
        }
        // 注意经度在前、纬度在后，且都是字符串
        let raw: Vec<NearbyStopRaw> = self
            .call_encrypted(
                "/bus/nearBy",
                json!({
                    "longitude": center.lng.to_string(),
                    "latitude": center.lat.to_string(),
                }),
            )
            .await?;

        let mut out: Vec<NearbyStop> = raw
            .into_iter()
            .map(|s| {
                let point = s
                    .lng
                    .zip(s.lat)
                    .map(|(lng, lat)| LngLat::new(lng, lat))
                    .filter(LngLat::is_valid);

                let distance_m = point.map(|p| distance_m(center, p)).unwrap_or(f64::MAX);

                let mut line_names: Vec<String> = Vec::new();
                let mut line_ids: Vec<String> = Vec::new();
                for l in &s.line_list {
                    if let Some(ln) = l.ln.clone() {
                        line_names.push(ln);
                    }
                    for id in l.direction_ids() {
                        line_ids.push(id.to_string());
                    }
                }
                line_names.sort();
                line_names.dedup();
                line_ids.sort();
                line_ids.dedup();

                NearbyStop {
                    stop_id: s.stop_id,
                    stop_name: s.stop_name,
                    point: point.map(Into::into),
                    distance_m,
                    direction: s.direction,
                    line_names,
                    line_ids,
                }
            })
            .collect();

        // 按本地重算的距离升序，最相关的站在最前
        out.sort_by(|a, b| a.distance_m.partial_cmp(&b.distance_m).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }
}

impl Default for Api {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 派生视图
// ---------------------------------------------------------------------------

/// 由原始站点表构造前端视图，并计算沿线路的累计里程。
pub fn build_line_detail(data: LineDetailData) -> LineDetail {
    let mut stops: Vec<StopDetail> = data.stop_detail_list;
    stops.sort_by_key(|s| s.stop_no);

    let views: Vec<StopView> = stops
        .iter()
        .map(|s| StopView {
            stop_no: s.stop_no,
            stop_id: s.stop_id.clone(),
            stop_name: s.stop_name.clone(),
            point: s.point().map(Into::into),
            subway_lines: s.subway_lines(),
        })
        .collect();

    // 累计里程：站点表只给坐标不给走向折线，用相邻站直线连接近似。
    // 缺失坐标的站点沿用上一个有效里程（不打断时间轴）。
    let mut acc = Vec::with_capacity(views.len());
    let mut total = 0.0;
    let mut prev: Option<LngLat> = None;
    for (i, s) in stops.iter().enumerate() {
        let p = s.point();
        if i > 0 {
            if let (Some(a), Some(b)) = (prev, p) {
                total += distance_m(a, b);
            }
        }
        if p.is_some() {
            prev = p;
        }
        acc.push(total);
    }

    LineDetail {
        base: data.line_detail_base_dto,
        stops: views,
        cumulative_meters: acc,
        total_meters: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(json_str: &str) -> Envelope {
        serde_json::from_str(json_str).unwrap()
    }

    #[test]
    fn envelope_success_returns_data() {
        let e = envelope(r#"{"data":{"a":1},"msg":{"code":"20000"},"success":true}"#);
        assert!(e.into_result().unwrap().is_some());
    }

    #[test]
    fn envelope_failure_surfaces_code_and_message() {
        let e = envelope(
            r#"{"msg":{"code":"500001","message":"密钥已失效，请更新"},"success":false}"#,
        );
        let err = e.into_result().unwrap_err();
        assert!(err.is_stale_key(), "500001 必须被识别为密钥失效以触发重试");
        assert!(err.to_string().contains("密钥已失效"));
    }

    #[test]
    fn other_error_codes_are_not_stale_key() {
        let e = envelope(r#"{"msg":{"code":"-1","message":"线路不存在：X"},"success":false}"#);
        assert!(!e.into_result().unwrap_err().is_stale_key());
    }

    #[test]
    fn builds_cumulative_mileage_from_stop_table() {
        let data: LineDetailData = serde_json::from_str(
            r#"{
              "lineDetailBaseDto": {
                "lineId":"420100-715-0","lineName":"715",
                "startStop":"民族大道冯徐村","endStop":"友谊大道沙湖",
                "earlyHour":"05:30","lastHour":"21:00","price":"2"
              },
              "stopDetailList": [
                {"stopNo":1,"stopId":"A","stopName":"起点","latitude":"30.465516","longitude":"114.391605"},
                {"stopNo":2,"stopId":"B","stopName":"第二站","latitude":"30.467013","longitude":"114.397088"},
                {"stopNo":3,"stopId":"C","stopName":"终点","latitude":"0.0","longitude":"0.0"}
              ]
            }"#,
        )
        .unwrap();

        let detail = build_line_detail(data);
        assert_eq!(detail.stops.len(), 3);
        assert_eq!(detail.cumulative_meters.len(), 3);
        assert_eq!(detail.cumulative_meters[0], 0.0);
        assert!(detail.cumulative_meters[1] > 500.0, "1→2 站约 540m");
        // 终点站坐标缺失，里程不应倒退，也不应 panic
        assert_eq!(
            detail.cumulative_meters[2], detail.cumulative_meters[1],
            "缺失坐标的站点应沿用上一段里程"
        );
        assert!(detail.stops[2].point.is_none());
        assert_eq!(detail.total_meters, detail.cumulative_meters[2]);
    }

    #[test]
    fn sorts_unordered_stop_table() {
        let data: LineDetailData = serde_json::from_str(
            r#"{
              "lineDetailBaseDto": {"lineId":"L","lineName":"1","startStop":"a","endStop":"b",
                                    "earlyHour":"06:00","lastHour":"20:00","price":"2"},
              "stopDetailList": [
                {"stopNo":2,"stopId":"B","stopName":"二","latitude":"30.5","longitude":"114.3"},
                {"stopNo":1,"stopId":"A","stopName":"一","latitude":"30.4","longitude":"114.2"}
              ]
            }"#,
        )
        .unwrap();
        let detail = build_line_detail(data);
        assert_eq!(detail.stops[0].stop_no, 1, "站点表应按 stopNo 升序");
        assert_eq!(detail.stops[1].stop_no, 2);
    }
}
