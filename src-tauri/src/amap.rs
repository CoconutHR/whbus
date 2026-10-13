//! 高德地图 Web 服务 API 客户端。
//!
//! # 为什么需要它
//!
//! 公交接口 **只匹配站名子串**，不认地址、小区名、写字楼名：
//! `"常青花园"` ✅ / `"楚天雅苑"` ❌ / `"洪山科创大厦"` ❌。
//! 这意味着"设置家/公司"这个功能靠公交接口根本做不了。
//!
//! 高德补上了两块能力：
//! 1. **地理编码 / POI 搜索**：地点名 → 坐标
//! 2. **步行路径规划**：真实的步行距离与耗时，替代"直线距离 × 绕路系数"的估算
//!
//! # 坐标系天然对齐
//!
//! 高德 Web API 返回的也是 **GCJ-02**，与公交接口一致，
//! 因此不需要任何转换——这也是选高德而非 OSM/MapLibre 的现实理由
//! （后两者是 WGS-84，混用会引入约 587 米的偏移，正是本项目的头号坑）。
//!
//! # Key 的处理
//!
//! **Key 不写进代码、不进版本库。** 由用户在设置页粘贴、本地保存。
//! 理由有两条：
//! - 本仓库是公开的，硬编码的 Key 会被爬虫抓走并刷爆配额
//! - 别人 clone 这个项目时应该用自己的配额
//!
//! 调用方（命令层）负责把 Key 传进来；本模块只做协议。

use std::time::Duration;

use serde::Deserialize;

use crate::gcj::LngLat;
use crate::{AppError, Result};

/// 地理编码 / POI 搜索域名
const REST_BASE: &str = "https://restapi.amap.com";

/// 步行速度上限校验：高德返回的 duration 单位是**秒**（已实测确认：
/// 5154 米 / 4123 秒 = 4.5 km/h，正是步行速度；且等同于 17 个 step 的 duration 之和）
const SECS_PER_MIN: f64 = 60.0;

// ---------------------------------------------------------------------------
// 响应信封
// ---------------------------------------------------------------------------

/// 高德 Web 服务的通用信封。
///
/// 注意：成功判定字段是字符串 `"1"`（不是布尔），且 v3/v4 的字段名略有差异
/// （`info`/`infocode` vs `errcode`/`errmsg`），这里统一兼容。
///
/// `bound` 是必需的：`T` 只出现在 `Option<T>` 里，derive 生成的默认约束
/// 会绑定到 `&'de str` 的生命周期，与 `serde_json::from_str` 配合时报错。
/// 显式声明为 `DeserializeOwned` 即可。
#[derive(Debug, Deserialize)]
#[serde(bound = "T: serde::de::DeserializeOwned")]
struct AmapEnvelope<T> {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    info: Option<String>,
    #[serde(default)]
    infocode: Option<String>,
    #[serde(default)]
    errcode: Option<i64>,
    #[serde(default)]
    errmsg: Option<String>,
    #[serde(default)]
    data: Option<T>,

    // ---- v3 的字段直接平铺在顶层 ----
    #[serde(default)]
    geocodes: Option<Vec<GeocodeItem>>,
    #[serde(default)]
    pois: Option<Vec<PoiItem>>,
}

impl<T> AmapEnvelope<T> {
    /// 校验信封并返回 body。
    fn into_body(self) -> Result<AmapEnvelope<T>> {
        // v4 风格：status 缺失但有 errcode
        if let Some(code) = self.errcode {
            if code != 0 {
                return Err(AppError::Api {
                    code: format!("amap/{code}"),
                    message: self.errmsg.unwrap_or_else(|| "高德接口报错".into()),
                });
            }
            return Ok(self);
        }

        match self.status.as_deref() {
            Some("1") => Ok(self),
            Some(_) => Err(AppError::Api {
                code: self.infocode.clone().unwrap_or_default(),
                message: self.info.clone().unwrap_or_else(|| "高德接口报错".into()),
            }),
            // 两个都没有：结构不符合预期
            None => Err(AppError::Shape(
                "高德响应缺少 status 字段".into(),
            )),
        }
    }
}

/// 步行路径专用的信封。
///
/// # 为什么不能复用 `AmapEnvelope<WalkingBody>`
///
/// v3 路径规划把结果放在**顶层 `route`**，而不是 `data` 里：
///
/// ```json
/// { "status":"1", "info":"ok", "route": { "paths": [ … ] } }
/// ```
///
/// 这与 v4 的 `{ "errcode":0, "data": { "paths": [ … ] } }` 完全不同。
/// 早期实现照着 v4 写，既用错了端点也解错了字段，线上直接报
/// `10002 SERVICE_NOT_AVAILABLE`。
#[derive(Debug, Deserialize)]
struct WalkingEnvelope {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    info: Option<String>,
    #[serde(default)]
    infocode: Option<String>,
    #[serde(default)]
    errcode: Option<i64>,
    #[serde(default)]
    errmsg: Option<String>,
    #[serde(default)]
    route: Option<WalkingRouteBody>,
}

impl WalkingEnvelope {
    fn into_route(self) -> Result<WalkingRouteBody> {
        if let Some(code) = self.errcode {
            if code != 0 {
                return Err(AppError::Api {
                    code: format!("amap/{code}"),
                    message: self.errmsg.unwrap_or_else(|| "高德接口报错".into()),
                });
            }
        } else {
            match self.status.as_deref() {
                Some("1") => {}
                Some(_) => {
                    return Err(AppError::Api {
                        code: self.infocode.clone().unwrap_or_default(),
                        message: self.info.clone().unwrap_or_else(|| "高德接口报错".into()),
                    })
                }
                None => return Err(AppError::Shape("高德响应缺少 status 字段".into())),
            }
        }

        self.route
            .ok_or_else(|| AppError::Shape("高德未返回步行路径".into()))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GeocodeItem {
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    formatted_address: Option<String>,
    #[serde(default)]
    province: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    district: Option<String>,
    #[serde(default)]
    level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PoiItem {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    cityname: Option<String>,
    #[serde(default)]
    adname: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WalkingRouteBody {
    #[serde(default)]
    paths: Option<Vec<WalkingPath>>,
}

#[derive(Debug, Clone, Deserialize)]
struct WalkingPath {
    #[serde(default)]
    distance: Option<serde_json::Value>,
    #[serde(default)]
    duration: Option<serde_json::Value>,
}

/// 把可能是字符串或数字的字段解析为 `f64`
fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    match v? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 解析高德的 `"114.32599,30.482331"`（**经度在前**）
fn parse_location(s: &str) -> Option<LngLat> {
    let (lng, lat) = s.split_once(',')?;
    let p = LngLat::new(lng.trim().parse().ok()?, lat.trim().parse().ok()?);
    p.is_valid().then_some(p)
}

// ---------------------------------------------------------------------------
// 对外结果类型
// ---------------------------------------------------------------------------

/// 一个地理编码 / POI 结果
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    /// 完整地址
    pub address: String,
    /// **GCJ-02**，可直接喂给公交接口
    pub point: crate::types::GeoPoint,
    /// 省 / 市 / 区
    pub district: String,
    /// POI 类型，用于 UI 上区分"住宅小区/商务楼宇/地铁站"
    pub kind: Option<String>,
}

/// 步行路径结果
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalkingRoute {
    pub meters: f64,
    pub minutes: f64,
}

// ---------------------------------------------------------------------------
// 客户端
// ---------------------------------------------------------------------------

pub struct Amap {
    http: reqwest::Client,
    base: String,
}

impl Amap {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .connect_timeout(Duration::from_secs(6))
            .user_agent("whbus/0.1 (+https://github.com/CoconutHR/whbus)")
            .build()
            .expect("构建高德 HTTP 客户端失败");
        Self {
            http,
            base: REST_BASE.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base(base: impl Into<String>) -> Self {
        let mut a = Self::new();
        a.base = base.into();
        a
    }

    fn check_key(key: &str) -> Result<()> {
        if key.trim().is_empty() {
            return Err(AppError::BadRequest(
                "尚未配置高德 Key。请到「我的 → 高德 Key」填入，之后即可按地址搜索地点。".into(),
            ));
        }
        Ok(())
    }

    /// 按关键词搜索地点（POI 搜索）。
    ///
    /// 这是解决"搜不到楚天雅苑/洪山科创大厦"的核心接口。
    /// `city` 限定城市可显著提升准确度（武汉 = `420100`）。
    pub async fn search_place(
        &self,
        key: &str,
        keyword: &str,
        city: Option<&str>,
    ) -> Result<Vec<Place>> {
        Self::check_key(key)?;
        let keyword = keyword.trim();
        if keyword.is_empty() {
            return Err(AppError::BadRequest("搜索关键词不能为空".into()));
        }

        let mut url = format!(
            "{}/v3/place/text?key={}&keywords={}&offset=10&page=1&extensions=base",
            self.base,
            urlencoding(key),
            urlencoding(keyword)
        );
        if let Some(c) = city {
            url.push_str(&format!("&city={}&citylimit=true", urlencoding(c)));
        }

        let env: AmapEnvelope<serde_json::Value> =
            self.get_json::<AmapEnvelope<serde_json::Value>>(&url).await?.into_body()?;

        let pois = env.pois.unwrap_or_default();
        let out: Vec<Place> = pois
            .into_iter()
            .filter_map(|p| {
                let point = parse_location(p.location.as_deref()?)?;
                Some(Place {
                    name: p.name.unwrap_or_default(),
                    address: p.address.unwrap_or_default(),
                    point: point.into(),
                    district: format!(
                        "{}{}",
                        p.cityname.unwrap_or_default(),
                        p.adname.unwrap_or_default()
                    ),
                    kind: p.r#type,
                })
            })
            .collect();

        Ok(out)
    }

    /// 地址 → 坐标（结构化地理编码）。
    ///
    /// 比 POI 搜索更严格，适合"完整地址"输入。
    pub async fn geocode(
        &self,
        key: &str,
        address: &str,
        city: Option<&str>,
    ) -> Result<Vec<Place>> {
        Self::check_key(key)?;
        let address = address.trim();
        if address.is_empty() {
            return Err(AppError::BadRequest("地址不能为空".into()));
        }

        let mut url = format!(
            "{}/v3/geocode/geo?key={}&address={}",
            self.base,
            urlencoding(key),
            urlencoding(address)
        );
        if let Some(c) = city {
            url.push_str(&format!("&city={}", urlencoding(c)));
        }

        let env: AmapEnvelope<serde_json::Value> =
            self.get_json::<AmapEnvelope<serde_json::Value>>(&url).await?.into_body()?;

        let out: Vec<Place> = env
            .geocodes
            .unwrap_or_default()
            .into_iter()
            .filter_map(|g| {
                let point = parse_location(g.location.as_deref()?)?;
                Some(Place {
                    name: g.formatted_address.clone().unwrap_or_else(|| address.to_string()),
                    address: g.formatted_address.unwrap_or_default(),
                    point: point.into(),
                    district: format!(
                        "{}{}{}",
                        g.province.unwrap_or_default(),
                        g.city.unwrap_or_default(),
                        g.district.unwrap_or_default()
                    ),
                    kind: g.level,
                })
            })
            .collect();

        Ok(out)
    }

    /// 坐标 → 地址（逆地理编码）。
    pub async fn regeocode(&self, key: &str, p: LngLat) -> Result<Option<Place>> {
        Self::check_key(key)?;
        let url = format!(
            "{}/v3/geocode/regeo?key={}&location={},{}&extensions=base",
            self.base,
            urlencoding(key),
            p.lng,
            p.lat
        );

        let env: AmapEnvelope<serde_json::Value> =
            self.get_json::<AmapEnvelope<serde_json::Value>>(&url).await?.into_body()?;

        // regeocode 的结果结构特殊，直接取 formatted_address
        let addr = env
            .data
            .as_ref()
            .and_then(|d| d.get("regeocode"))
            .and_then(|r| r.get("formatted_address"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        Ok(addr.map(|a| Place {
            name: a.clone(),
            address: a,
            point: p.into(),
            district: String::new(),
            kind: None,
        }))
    }

    /// 步行路径规划。
    ///
    /// 用于替换"直线距离 × 1.25 绕路系数"的估算——真实路径会让步行段
    /// 的时间明显更准，而步行段恰恰是通勤里最容易被低估的部分。
    ///
    /// # 端点是 v3，不是 v4（实测结论）
    ///
    /// `/v4/direction/walking` 会返回 `errcode=10002 SERVICE_NOT_AVAILABLE`，
    /// 但 `/v3/direction/walking` 与 `/v5/direction/walking` 都正常。
    /// v4 需要额外开通，**不要用**。
    ///
    /// # 响应结构与 `data.paths` 无关
    ///
    /// v3 的结构是 `route.paths[]`，不是 v4 的 `data.paths[]`。
    /// `distance` / `duration` 都是**字符串**，`duration` 单位是**秒**。
    pub async fn walking(&self, key: &str, from: LngLat, to: LngLat) -> Result<WalkingRoute> {
        Self::check_key(key)?;
        if !from.is_valid() || !to.is_valid() {
            return Err(AppError::BadCoord("步行路径的起点或终点非法".into()));
        }

        let url = format!(
            "{}/v3/direction/walking?key={}&origin={},{}&destination={},{}",
            self.base,
            urlencoding(key),
            from.lng,
            from.lat,
            to.lng,
            to.lat
        );

        let env: WalkingEnvelope = self.get_json(&url).await?;
        let route = env.into_route()?;

        let path = route
            .paths
            .and_then(|p| p.into_iter().next())
            .ok_or_else(|| AppError::Shape("高德未返回步行路径".into()))?;

        let meters = num(path.distance.as_ref()).unwrap_or(0.0);
        let secs = num(path.duration.as_ref()).unwrap_or(0.0);

        if meters <= 0.0 {
            return Err(AppError::Shape("高德返回的步行距离为 0".into()));
        }

        Ok(WalkingRoute {
            meters,
            minutes: secs / SECS_PER_MIN,
        })
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T> {
        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(AppError::Shape(format!(
                "高德 HTTP {status}：{}",
                text.chars().take(200).collect::<String>()
            )));
        }
        serde_json::from_str(&text).map_err(|e| {
            AppError::Shape(format!(
                "无法解析高德响应：{e}；原文前 200 字符：{}",
                text.chars().take(200).collect::<String>()
            ))
        })
    }
}

impl Default for Amap {
    fn default() -> Self {
        Self::new()
    }
}

/// 最小化的 URL 百分号编码。
///
/// 不引入 `urlencoding`/`percent-encoding` 依赖：这里只需要处理
/// Key、关键词、地址三类参数，规则简单且可测。
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_handles_chinese_and_reserved() {
        // 中文必须被编码（高德会按 UTF-8 百分号解码）
        let enc = urlencoding("楚天雅苑");
        assert!(!enc.contains('楚'), "中文不应原样出现");
        assert!(enc.starts_with("%E6%A5%9A"), "应为 UTF-8 百分号编码：{enc}");

        // key 是十六进制，应原样保留
        assert_eq!(urlencoding("c269c5c9af5359359617b497a4765b9f"),
                   "c269c5c9af5359359617b497a4765b9f");

        // 空格与 & 必须编码，否则会破坏查询串
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("a&b"), "a%26b");
        assert_eq!(urlencoding("a=b"), "a%3Db");
    }

    #[test]
    fn parses_amap_location_lng_first() {
        // 高德是"经度,纬度"，与公交接口的字段顺序一致但含义相反于常见约定
        let p = parse_location("114.32599,30.482331").unwrap();
        assert!((p.lng - 114.32599).abs() < 1e-9);
        assert!((p.lat - 30.482331).abs() < 1e-9);

        assert!(parse_location("bad").is_none());
        assert!(parse_location("0,0").is_none(), "0,0 必须被拒");
    }

    #[test]
    fn envelope_accepts_v3_success() {
        let env: AmapEnvelope<serde_json::Value> =
            serde_json::from_str(r#"{"status":"1","info":"OK","count":"2","pois":[]}"#).unwrap();
        assert!(env.into_body().is_ok());
    }

    #[test]
    fn envelope_rejects_v3_failure_with_message() {
        let env: AmapEnvelope<serde_json::Value> = serde_json::from_str(
            r#"{"status":"0","info":"INVALID_USER_KEY","infocode":"10001"}"#,
        )
        .unwrap();
        let err = env.into_body().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("INVALID_USER_KEY"), "错误信息必须透出，实测：{msg}");
        assert!(msg.contains("10001"));
    }

    #[test]
    fn envelope_accepts_v4_success() {
        let env: AmapEnvelope<serde_json::Value> =
            serde_json::from_str(r#"{"errcode":0,"data":{"paths":[]}}"#).unwrap();
        assert!(env.into_body().is_ok());
    }

    #[test]
    fn envelope_rejects_v4_failure() {
        let env: AmapEnvelope<serde_json::Value> =
            serde_json::from_str(r#"{"errcode":20000,"errmsg":"ENGINE_RESPONSE_DATA_ERROR"}"#)
                .unwrap();
        let err = env.into_body().unwrap_err();
        assert!(err.to_string().contains("ENGINE_RESPONSE_DATA_ERROR"));
        assert!(err.to_string().contains("20000"));
    }

    #[test]
    fn envelope_rejects_missing_status() {
        let env: AmapEnvelope<serde_json::Value> =
            serde_json::from_str(r#"{"foo":1}"#).unwrap();
        assert!(matches!(env.into_body(), Err(AppError::Shape(_))));
    }

    #[test]
    fn num_accepts_string_and_number() {
        // 高德在不同接口里对数值的编码方式不一致，两种都要能解析
        let s: serde_json::Value = serde_json::json!("1234");
        let n: serde_json::Value = serde_json::json!(1234);
        assert_eq!(num(Some(&s)), Some(1234.0));
        assert_eq!(num(Some(&n)), Some(1234.0));
        assert_eq!(num(None), None);
    }

    #[test]
    fn empty_key_is_rejected_with_actionable_message() {
        let e = Amap::check_key("  ").unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("高德 Key"), "错误信息要告诉用户去配置：{msg}");
    }

    /// 回归：v3 步行响应的真实结构是**顶层 `route.paths[]`**，
    /// 既不是 v4 的 `data.paths[]`，也不是 `data.route.paths[]`。
    ///
    /// 这段 JSON 是从真实响应里摘出来的（家 → 公司，5154 米 / 4123 秒）。
    /// 早期实现用了 `/v4/direction/walking` 并解析 `data.paths`，
    /// 线上直接 `errcode=10002 SERVICE_NOT_AVAILABLE`。
    #[test]
    fn parses_real_v3_walking_response() {
        let raw = r#"{
          "status":"1","info":"ok","infocode":"10000","count":"1",
          "route":{
            "origin":"114.295109,30.492370",
            "destination":"114.334903,30.502804",
            "paths":[{
              "distance":"5154",
              "duration":"4123",
              "steps":[{"instruction":"向北步行20米左转","distance":"20","duration":"16"}]
            }]
          }
        }"#;

        let env: WalkingEnvelope = serde_json::from_str(raw).unwrap();
        let route = env.into_route().unwrap();
        let path = route
            .paths
            .and_then(|p| p.into_iter().next())
            .expect("必须能从顶层 route.paths 取到路径");

        let meters = num(path.distance.as_ref()).unwrap();
        let secs = num(path.duration.as_ref()).unwrap();
        assert_eq!(meters, 5154.0);
        assert_eq!(secs, 4123.0);

        // duration 是秒：5154m / 4123s = 1.25 m/s ≈ 4.5 km/h，正是步行速度。
        // 若误当作毫秒，会得到 0.07 分钟这种荒谬值。
        let minutes = secs / SECS_PER_MIN;
        assert!(
            (minutes - 68.7).abs() < 0.5,
            "应为约 68.7 分钟，实测 {minutes:.1}"
        );
        let kmh = (meters / 1000.0) / (secs / 3600.0);
        assert!(
            (3.5..5.5).contains(&kmh),
            "换算出的速度必须落在步行区间，实测 {kmh:.1} km/h"
        );
    }

    /// 回归：v4 端点会返回 SERVICE_NOT_AVAILABLE，必须被识别成错误而不是空结果
    #[test]
    fn v4_service_unavailable_is_surfaced_as_error() {
        let raw = r#"{"errcode":10002,"errmsg":"SERVICE_NOT_AVAILABLE"}"#;
        let env: WalkingEnvelope = serde_json::from_str(raw).unwrap();
        let err = env.into_route().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("SERVICE_NOT_AVAILABLE"), "错误必须透出：{msg}");
        assert!(msg.contains("10002"));
    }

    /// v3 失败响应（status=0）也要能被识别
    #[test]
    fn v3_walking_failure_is_surfaced() {
        let raw = r#"{"status":"0","info":"INVALID_USER_KEY","infocode":"10001"}"#;
        let env: WalkingEnvelope = serde_json::from_str(raw).unwrap();
        let err = env.into_route().unwrap_err();
        assert!(err.to_string().contains("INVALID_USER_KEY"));
    }

    /// 解析真实 POI 搜索结果（楚天·雅苑 / 洪山科创大厦）
    #[test]
    fn parses_real_poi_response() {
        let raw = r#"{
          "status":"1","info":"OK","infocode":"10000","count":"98",
          "pois":[
            {"name":"楚天·雅苑","location":"114.295109,30.492370",
             "address":"文治街","adname":"洪山区","cityname":"武汉市",
             "type":"商务住宅;住宅区;住宅小区"},
            {"name":"洪山科创大厦","location":"114.334903,30.502804",
             "address":"文治街","adname":"洪山区","cityname":"武汉市",
             "type":"商务住宅;楼宇;商务写字楼"}
          ]
        }"#;

        let env: AmapEnvelope<serde_json::Value> = serde_json::from_str(raw).unwrap();
        let env = env.into_body().unwrap();
        let pois = env.pois.unwrap();
        assert_eq!(pois.len(), 2);

        let home = parse_location(pois[0].location.as_deref().unwrap()).unwrap();
        assert!((home.lng - 114.295109).abs() < 1e-6);
        assert!((home.lat - 30.492370).abs() < 1e-6);

        let work = parse_location(pois[1].location.as_deref().unwrap()).unwrap();
        // 两点直线距离应在 3-5 km（真实约 3.9km）
        let d = crate::gcj::distance_m(home, work);
        assert!(
            (3000.0..5000.0).contains(&d),
            "家到公司直线距离应在 3-5km，实测 {:.0}m",
            d
        );
    }
}
