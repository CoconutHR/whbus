//! WGS-84 ⇄ GCJ-02 坐标转换。
//!
//! # 为什么这个模块是关键路径
//!
//! - 安卓系统定位（`tauri-plugin-geolocation` → `FusedLocationProvider`）返回 **WGS-84**
//! - 武汉公交 API（`nearBy` / `detailData` / `line`）返回 **GCJ-02**
//! - 两者在武汉的偏移实测约 **587 米**（见 `tests`）
//! - 而 `nearBy` 的搜索半径只有 **约 400 米**
//!
//! 偏移量 > 搜索半径，意味着**不做转换会直接查到错误的站点**，而不是"稍微不准"。
//!
//! # 约定
//!
//! App 内部**一律使用 GCJ-02**（`LngLat` 即 GCJ-02）。
//! 只有两个边界需要转换：
//! 1. 定位出口：WGS-84 → GCJ-02（`wgs84_to_gcj02`）
//! 2. 需要把坐标交给系统/其他 WGS-84 服务时：GCJ-02 → WGS-84（`gcj02_to_wgs84`）
//!
//! GCJ-02 是国测局加密坐标系，不存在精确的解析逆变换。
//! 本模块用不动点迭代求逆，单次迭代即可达到亚米级精度。

use serde::{Deserialize, Serialize};

/// 经纬度点。**语义约定：本项目的 `LngLat` 一律表示 GCJ-02 坐标。**
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LngLat {
    /// 经度（GCJ-02）
    pub lng: f64,
    /// 纬度（GCJ-02）
    pub lat: f64,
}

/// 供日志输出：`114.295109,30.492370` 这样的紧凑形式。
///
/// 6 位小数对城市尺度足够（约 0.1 米），且方便直接粘进高德坐标拾取器
/// 复核 —— 排查"周边查不到站点"这类问题时经常要这么做。
impl std::fmt::Display for LngLat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.6},{:.6}", self.lng, self.lat)
    }
}

impl LngLat {
    pub const fn new(lng: f64, lat: f64) -> Self {
        Self { lng, lat }
    }

    /// 坐标是否有效（排除 0,0 哨兵值 —— 部分线路终点站坐标为 0.0,0.0）
    ///
    /// 同时校验纬度/经度落在合法值域内，避免上游脏数据污染几何计算。
    pub fn is_valid(&self) -> bool {
        if self.lng == 0.0 && self.lat == 0.0 {
            return false;
        }
        self.lat.is_finite()
            && self.lng.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lng)
    }

    /// 是否落在中国大陆大致范围内（GCJ-02 偏移只在国境内生效）
    fn in_china_bbox(&self) -> bool {
        (72.004..=137.8347).contains(&self.lng) && (0.8293..=55.8271).contains(&self.lat)
    }

    /// 从字符串解析（API 的坐标字段是字符串类型）
    pub fn from_str_pair(lng: &str, lat: &str) -> Option<Self> {
        let lng: f64 = lng.trim().parse().ok()?;
        let lat: f64 = lat.trim().parse().ok()?;
        let p = Self { lng, lat };
        p.is_valid().then_some(p)
    }
}

/// 地球平均半径（米），与 haversine 公式配套
const EARTH_RADIUS_M: f64 = 6_372_792.0;

/// 转换为弧度
#[inline]
fn rad(d: f64) -> f64 {
    d * std::f64::consts::PI / 180.0
}

/// 两点球面距离（米），haversine 公式。
///
/// 用于步行距离估算与"车辆还有多远"计算。公交站间通常是百米量级，
/// haversine 在此尺度上的误差远小于 GPS 本身的漂移。
pub fn distance_m(a: LngLat, b: LngLat) -> f64 {
    let (lat1, lat2) = (rad(a.lat), rad(b.lat));
    let dlat = lat2 - lat1;
    let dlng = rad(b.lng - a.lng);
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlng / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().clamp(0.0, 1.0).asin()
}

// ---------------------------------------------------------------------------
// GCJ-02 正向变换（WGS-84 → GCJ-02）
// ---------------------------------------------------------------------------

fn transform_lat(x: f64, y: f64) -> f64 {
    let mut ret = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    ret += (20.0 * (6.0 * x * std::f64::consts::PI).sin()
        + 20.0 * (2.0 * x * std::f64::consts::PI).sin())
        * 2.0
        / 3.0;
    ret += (20.0 * (y * std::f64::consts::PI).sin()
        + 40.0 * (y / 3.0 * std::f64::consts::PI).sin())
        * 2.0
        / 3.0;
    ret += (160.0 * (y / 12.0 * std::f64::consts::PI).sin()
        + 320.0 * (y * std::f64::consts::PI / 30.0).sin())
        * 2.0
        / 3.0;
    ret
}

fn transform_lng(x: f64, y: f64) -> f64 {
    let mut ret = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    ret += (20.0 * (6.0 * x * std::f64::consts::PI).sin()
        + 20.0 * (2.0 * x * std::f64::consts::PI).sin())
        * 2.0
        / 3.0;
    ret += (20.0 * (x * std::f64::consts::PI).sin()
        + 40.0 * (x / 3.0 * std::f64::consts::PI).sin())
        * 2.0
        / 3.0;
    ret += (150.0 * (x / 12.0 * std::f64::consts::PI).sin()
        + 300.0 * (x / 30.0 * std::f64::consts::PI).sin())
        * 2.0
        / 3.0;
    ret
}

/// WGS-84 → GCJ-02。
///
/// 输入落在国境之外时原样返回（GCJ-02 偏移只在境内定义）。
/// 这也是自动化测试中的预期行为，便于在境外环境下跑单测。
pub fn wgs84_to_gcj02(p: LngLat) -> LngLat {
    if !p.in_china_bbox() {
        return p;
    }
    // 克拉索夫斯基椭球参数
    const A: f64 = 6_378_245.0;
    const EE: f64 = 0.006_693_421_622_965_943;

    let dlat = transform_lat(p.lng - 105.0, p.lat - 35.0);
    let dlng = transform_lng(p.lng - 105.0, p.lat - 35.0);

    let rad_lat = rad(p.lat);
    let mut magic = rad_lat.sin();
    magic = 1.0 - EE * magic * magic;
    let sqrt_magic = magic.sqrt();

    let dlat = (dlat * 180.0) / ((A * (1.0 - EE)) / (magic * sqrt_magic) * std::f64::consts::PI);
    let dlng = (dlng * 180.0) / (A / sqrt_magic * rad_lat.cos() * std::f64::consts::PI);

    LngLat::new(p.lng + dlng, p.lat + dlat)
}

/// GCJ-02 → WGS-84，用不动点迭代求逆。
///
/// 迭代式：`w_{n+1} = w_n + (g - F(w_n))`，其中 `F = wgs84_to_gcj02`。
/// 由于偏移场在局部近似常数，2 次迭代即可收敛到厘米级；这里取 3 次留余量。
///
/// **注意**：仅在确需输出 WGS-84 时调用。App 内部请保持 GCJ-02。
pub fn gcj02_to_wgs84(p: LngLat) -> LngLat {
    if !p.in_china_bbox() {
        return p;
    }
    let mut guess = p;
    for _ in 0..3 {
        let projected = wgs84_to_gcj02(guess);
        guess = LngLat::new(
            guess.lng + (p.lng - projected.lng),
            guess.lat + (p.lat - projected.lat),
        );
    }
    guess
}

// ---------------------------------------------------------------------------
// 路径几何 —— 用于"车辆在线路上走到哪了"的时间轴渲染
// ---------------------------------------------------------------------------

/// 折线累计长度（米）。返回 `acc[i]` = 从起点到第 `i` 个顶点的累计距离。
///
/// 站点表只给坐标、不给走向折线，因此本项目用**相邻站点直线连接**
/// 作为线路几何。这是近似，但足以支撑"车在 A 站和 B 站之间、
/// 更靠近 B"这类判断，也正是 UI 上时间轴要表达的信息。
pub fn cumulative_lengths(points: &[LngLat]) -> Vec<f64> {
    let mut acc = Vec::with_capacity(points.len());
    let mut total = 0.0;
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            total += distance_m(points[i - 1], *p);
        }
        acc.push(total);
    }
    acc
}

/// 把点投影到折线上，返回 `(投影点的沿线里程 m, 距折线的垂距 m)`。
///
/// 用于把一辆车的实时坐标吸附到它所在的线路走向上——GPS 漂移时
/// 直接按最近站点插值会跳变，投影到折线再取里程则平滑得多。
pub fn project_onto_path(points: &[LngLat], p: LngLat) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let acc = cumulative_lengths(points);
    let mut best: Option<(f64, f64)> = None;

    for i in 0..points.len() - 1 {
        let (a, b) = (points[i], points[i + 1]);
        // 以 a 为原点做局部平面近似（百米尺度下误差可忽略）
        let lat_ref = rad((a.lat + b.lat) / 2.0);
        const M_PER_DEG_LAT: f64 = 111_320.0;
        let m_per_deg_lng = M_PER_DEG_LAT * lat_ref.cos();

        let bx = (b.lng - a.lng) * m_per_deg_lng;
        let by = (b.lat - a.lat) * M_PER_DEG_LAT;
        let px = (p.lng - a.lng) * m_per_deg_lng;
        let py = (p.lat - a.lat) * M_PER_DEG_LAT;

        let seg_len_sq = bx * bx + by * by;
        let t = if seg_len_sq <= f64::EPSILON {
            0.0
        } else {
            ((px * bx + py * by) / seg_len_sq).clamp(0.0, 1.0)
        };

        let (cx, cy) = (t * bx, t * by);
        let perp = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
        let seg_len = seg_len_sq.sqrt();
        let along = acc[i] + t * seg_len;

        if best.map_or(true, |(_, bp)| perp < bp) {
            best = Some((along, perp));
        }
    }
    best
}

/// 沿折线在给定里程处插值出坐标。与 `project_onto_path` 配对使用。
pub fn point_at_distance(points: &[LngLat], target_m: f64) -> Option<LngLat> {
    if points.is_empty() {
        return None;
    }
    if points.len() == 1 {
        return Some(points[0]);
    }
    let acc = cumulative_lengths(points);
    let total = *acc.last()?;
    if target_m <= 0.0 {
        return Some(points[0]);
    }
    if target_m >= total {
        return Some(points[points.len() - 1]);
    }

    for i in 0..points.len() - 1 {
        if acc[i] <= target_m && target_m <= acc[i + 1] {
            let seg = acc[i + 1] - acc[i];
            let t = if seg <= f64::EPSILON {
                0.0
            } else {
                (target_m - acc[i]) / seg
            };
            let (a, b) = (points[i], points[i + 1]);
            return Some(LngLat::new(
                a.lng + (b.lng - a.lng) * t,
                a.lat + (b.lat - a.lat) * t,
            ));
        }
    }
    Some(points[points.len() - 1])
}

/// 按步行速度估算时间（分钟）。`speed_m_per_min` 默认步行约 75 m/min（4.5 km/h）。
pub fn walk_minutes(meters: f64, speed_m_per_min: f64) -> f64 {
    if speed_m_per_min <= 0.0 {
        return f64::INFINITY;
    }
    meters / speed_m_per_min
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 武汉地区的偏移量必须显著大于 nearBy 的 400m 搜索半径，
    /// 否则"必须转换坐标"这个结论就不成立。
    #[test]
    fn wuhan_offset_exceeds_nearby_radius() {
        let cases = [
            (30.639206, 114.237014), // 常青花园
            (30.497100, 114.313574), // 南湖 / 丁字桥
            (30.513000, 114.343000), // 武汉理工马房山
            (30.617000, 114.253000), // 汉口火车站
        ];
        for (lat, lng) in cases {
            let wgs = LngLat::new(lng, lat);
            let gcj = wgs84_to_gcj02(wgs);
            let off = distance_m(wgs, gcj);
            assert!(
                (560.0..620.0).contains(&off),
                "武汉偏移应在 560-620m，实测 {off:.0}m @ ({lat},{lng})"
            );
            assert!(
                off > 400.0,
                "偏移必须大于 nearBy 的 400m 半径，否则坐标不转换也不会查错"
            );
        }
    }

    #[test]
    fn roundtrip_is_submeter() {
        let wgs = LngLat::new(114.343000, 30.513000);
        let back = gcj02_to_wgs84(wgs84_to_gcj02(wgs));
        let err = distance_m(wgs, back);
        assert!(err < 0.5, "往返误差应小于 0.5m，实测 {err:.3}m");
    }

    #[test]
    fn outside_china_is_identity() {
        let tokyo = LngLat::new(139.6917, 35.6895);
        assert_eq!(wgs84_to_gcj02(tokyo), tokyo);
    }

    /// 0,0 是上游的哨兵值，必须被判为无效，不能进入几何计算
    #[test]
    fn zero_coord_is_invalid() {
        assert!(!LngLat::new(0.0, 0.0).is_valid());
        assert!(LngLat::new(114.3, 30.5).is_valid());
    }

    #[test]
    fn parse_string_pair() {
        // API 的坐标字段是字符串
        let p = LngLat::from_str_pair("114.32599", "30.482331").unwrap();
        assert!((p.lng - 114.32599).abs() < 1e-9);
        assert!(LngLat::from_str_pair("0.0", "0.0").is_none());
        assert!(LngLat::from_str_pair("", "30.5").is_none());
    }

    #[test]
    fn projection_finds_midpoint() {
        // 一条东西向直线，测投影里程
        let path = [
            LngLat::new(114.300, 30.500),
            LngLat::new(114.310, 30.500),
            LngLat::new(114.320, 30.500),
        ];
        let acc = cumulative_lengths(&path);
        assert_eq!(acc.len(), 3);
        assert!((acc[0]).abs() < 1e-9);

        // 中点附近
        let mid = LngLat::new(114.310, 30.500);
        let (along, perp) = project_onto_path(&path, mid).unwrap();
        assert!(perp < 1.0, "垂距应接近 0，实测 {perp:.2}m");
        assert!(
            (along - acc[1]).abs() < 5.0,
            "里程应落在第二个顶点附近，实测 {along:.1} vs {:.1}",
            acc[1]
        );
    }

    #[test]
    fn interpolate_along_path() {
        let path = [LngLat::new(114.300, 30.500), LngLat::new(114.310, 30.500)];
        let total = cumulative_lengths(&path)[1];
        let mid = point_at_distance(&path, total / 2.0).unwrap();
        assert!((mid.lng - 114.305).abs() < 1e-4, "中点经度应约 114.305");
        // 越界应夹取到端点
        assert_eq!(point_at_distance(&path, -10.0).unwrap(), path[0]);
        assert_eq!(point_at_distance(&path, total * 2.0).unwrap(), path[1]);
    }
}
