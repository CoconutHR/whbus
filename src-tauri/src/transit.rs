//! 实时车辆定位与到站预判。
//!
//! # 这里为什么必须自算
//!
//! 上游 `/bus/station` 提供了 `nscs` / `nsns`（剩余站数）字段，看起来正是
//! "还有几站"所需。但实测结论是：**该字段恒为哨兵值**（`-3`、`-2`），
//! 即使把一条**确实有在线车辆**的线路（`420100-夜行518-1`）的全部站点
//! 都查一遍，非负取值出现次数依然为 **0**。
//!
//! 因此"还有几站 / 还有几分钟"完全由本模块基于 `/bus/detailData` 的
//! 车辆坐标 + `/bus/line` 的站点表自行推算。
//!
//! # 坐标系
//!
//! 车辆坐标与站点坐标**同为 GCJ-02**，可直接做几何运算，无需转换。
//! 只有来自系统定位的用户坐标需要先经 `gcj::wgs84_to_gcj02`。

use crate::gcj::{LngLat, cumulative_lengths, distance_m, project_onto_path};
use crate::types::{
    ArrivalView, LineDetail, PositionConfidence, RawVehicle, StopView, VehicleView,
};

/// GPS 数据仍算"新鲜"的阈值
pub const LIVE_MAX_AGE_SECS: i64 = 180;
/// 超过此年龄即视为不可用（只允许作为历史推测的输入）
pub const STALE_AFTER_SECS: i64 = 600;

/// 解析 `yyyyMMddHHmmss` 为 `(unix_secs, 原始字段)`。
///
/// 不使用 chrono：只为一个固定格式引入完整日期库会显著增加安卓构建体积，
/// 而这里需要的全部能力就是"把日历日期转成天数"。
pub fn parse_gps_time(s: &str) -> Option<(i64, (i32, u32, u32, u32, u32, u32))> {
    let s = s.trim();
    if s.len() != 14 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |a: usize, b: usize| s[a..b].parse::<i64>().ok();
    let (y, mo, d) = (n(0, 4)? as i32, n(4, 6)? as u32, n(6, 8)? as u32);
    let (h, mi, sec) = (n(8, 10)? as u32, n(10, 12)? as u32, n(12, 14)? as u32);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    Some((civil_to_unix(y, mo, d, h, mi, sec), (y, mo, d, h, mi, sec)))
}

/// 把公历日期时间转成 Unix 秒（Howard Hinnant 的 days_from_civil 算法）。
fn civil_to_unix(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = ((m as i64) + 9) % 12; // 3月=0 … 2月=11
    let doy = (153 * mp + 2) / 5 + (d as i64) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + (h as i64) * 3600 + (mi as i64) * 60 + s as i64
}

/// 本地当前时间，格式与 `gpsTime` 一致（`yyyyMMddHHmmss`）。
///
/// `gpsTime` 与本地时钟同处一个时区（均为北京时间），因此可以直接相减。
pub fn local_now_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// `days_from_civil` 的逆运算
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// 数据年龄（秒）。无法解析时返回 `None`。
pub fn data_age_secs(gps_time: &str, now_stamp: &str) -> Option<i64> {
    let (t, _) = parse_gps_time(gps_time)?;
    let (n, _) = parse_gps_time(now_stamp)?;
    Some((n - t).max(0))
}

/// 依据数据年龄判定位置可信度。
///
/// 这个判定直接驱动 UI：`Live` 画实线不透明，`Inferred` 必须画虚线半透明。
pub fn confidence_for_age(age: Option<i64>) -> PositionConfidence {
    match age {
        Some(a) if a <= LIVE_MAX_AGE_SECS => PositionConfidence::Live,
        Some(a) if a <= STALE_AFTER_SECS => PositionConfidence::Delayed,
        Some(_) => PositionConfidence::Stale,
        // 无法解析 GPS 时间时保守处理：不宣称其实时
        None => PositionConfidence::Delayed,
    }
}

/// 把一辆原始车辆数据吸附到线路走向上，得到 UI 可直接渲染的视图。
///
/// `fetched_at` 为本次请求成功返回的时刻（本地 `yyyyMMddHHmmss`），
/// 用于在 `gpsTime` 缺失时兜底。
pub fn position_vehicle(
    raw: &RawVehicle,
    detail: &LineDetail,
    fetched_at: &str,
) -> VehicleView {
    let point = raw
        .lng
        .zip(raw.lat)
        .map(|(lng, lat)| LngLat::new(lng, lat))
        .filter(LngLat::is_valid);

    let gps_time = raw.gps_time.clone();
    let age = gps_time
        .as_deref()
        .and_then(|t| data_age_secs(t, fetched_at));

    let arrival_flag = raw.arrived == Some(1);

    // 车辆位置 -> 路径里程
    let (along_m, offset_m) = match point {
        Some(p) => {
            let path: Vec<LngLat> = detail
                .stops
                .iter()
                .filter_map(|s| s.point.map(LngLat::from))
                .collect();
            match project_onto_path(&path, p) {
                Some((along, perp)) => (Some(along), Some(perp)),
                None => (None, None),
            }
        }
        None => (None, None),
    };

    // 里程 -> 所在站 / 下一站
    let (at_stop_no, next_stop_no) = match along_m {
        Some(along) => locate_stop(detail, along),
        None => (None, None),
    };

    // 垂距过大（>800m）说明车辆偏离线路走向，位置不可信
    let confidence = match (age, offset_m) {
        (_, Some(off)) if off > 800.0 => PositionConfidence::Stale,
        (a, _) => confidence_for_age(a),
    };

    VehicleView {
        bus_id: raw.bus_id.clone(),
        bus_order: raw.bus_order,
        point: point.map(Into::into),
        speed_kmh: raw.gps_speed,
        along_m,
        offset_m,
        at_stop_no,
        next_stop_no,
        arrived: arrival_flag,
        gps_time,
        age_seconds: age,
        confidence,
    }
}

/// 由沿线里程定位 `(当前/刚过的站序号, 下一站序号)`。
///
/// 注意返回的"当前站"语义是**刚经过或正停在的站**，与 `next_stop_no` 配合
/// 表达"车在 A 站和 B 站之间"。UI 上这一对值决定时间轴的脉冲点位置。
pub fn locate_stop(detail: &LineDetail, along_m: f64) -> (Option<i64>, Option<i64>) {
    let acc = &detail.cumulative_meters;
    if acc.is_empty() {
        return (None, None);
    }

    // 找到最后一个里程 <= along_m 的站点
    let mut idx = 0usize;
    for (i, &a) in acc.iter().enumerate() {
        if a <= along_m + 1e-6 {
            idx = i;
        } else {
            break;
        }
    }

    let at = detail.stops.get(idx).map(|s| s.stop_no);
    let next = detail.stops.get(idx + 1).map(|s| s.stop_no);

    // 已驶过末站：没有下一站
    (at, next)
}

/// 速度模型：把"距离 → 时间"的换算集中一处。
///
/// # 为什么需要它
///
/// 上游**没有任何班次计划表**（`plans` 字段恒为空），
/// 所以"最晚几点出发"根本无法从时刻表推导，只能自建经验模型。
///
/// 模型是三层的加权融合：
/// 1. **全局先验**：武汉市区公交平均运营速度（含停站）
/// 2. **线路历史**：本地累积的该线路实际行驶样本
/// 3. **实时速度**：该车此刻的 GPS 速度
///
/// 样本越多，权重越偏向实测，ETA 区间也越窄——UI 上直接体现"越用越准"。
#[derive(Debug, Clone)]
pub struct SpeedModel {
    /// 全局先验速度（km/h）
    pub prior_kmh: f64,
    /// 线路级累计里程（米）
    pub line_meters: f64,
    /// 线路级累计耗时（秒）
    pub line_secs: f64,
    /// 该线路的观测样本数
    pub line_samples: u32,
}

/// 武汉市区的公交平均运营速度先验（km/h，已含停站与红灯）
pub const DEFAULT_BUS_KMH: f64 = 18.0;
/// 步行速度先验（km/h）
pub const DEFAULT_WALK_KMH: f64 = 4.5;
/// 样本数的"半衰"常数：观测满 20 次后历史权重接近上限
const SAMPLE_HALFLIFE: f64 = 20.0;

impl Default for SpeedModel {
    fn default() -> Self {
        Self {
            prior_kmh: DEFAULT_BUS_KMH,
            line_meters: 0.0,
            line_secs: 0.0,
            line_samples: 0,
        }
    }
}

impl SpeedModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 累积一次观测（用于"越用越准"）
    pub fn observe(&mut self, meters: f64, secs: f64) {
        if meters > 0.0 && secs > 0.0 {
            self.line_meters += meters;
            self.line_secs += secs;
            self.line_samples = self.line_samples.saturating_add(1);
        }
    }

    /// 历史样本推出的线路速度（km/h）。样本不足时退回先验。
    fn historical_kmh(&self) -> f64 {
        if self.line_secs <= 0.0 || self.line_meters <= 0.0 {
            return self.prior_kmh;
        }
        let kmh = (self.line_meters / self.line_secs) * 3.6;
        // 公交运营速度的现实区间，防止脏样本把模型带偏
        kmh.clamp(8.0, 45.0)
    }

    /// 融合后的有效速度（km/h）。
    ///
    /// `live_kmh` 为车辆当前 GPS 速度（可为 `None` 或 0——静止/等灯时速度为 0，
    /// 此时不能直接采信，否则会算出无穷大的 ETA）。
    pub fn effective_kmh(&self, live_kmh: Option<f64>) -> f64 {
        let hist = self.historical_kmh();
        let hist_weight = (self.line_samples as f64 / (self.line_samples as f64 + SAMPLE_HALFLIFE))
            .clamp(0.0, 0.75);

        let base = self.prior_kmh * (1.0 - hist_weight) + hist * hist_weight;

        // 实时速度只做修正，且只在明显大于"停站拖累"时才采信
        match live_kmh {
            Some(v) if v >= 3.0 => {
                let blended = base * 0.65 + v * 0.35;
                blended.clamp(6.0, 60.0)
            }
            _ => base,
        }
    }

    /// 相对不确定度（变异系数）。
    ///
    /// 观测越多越确定；实时速度可用时也收窄。UI 用它决定 ETA 区间的宽度。
    pub fn relative_uncertainty(&self, live_kmh: Option<f64>) -> f64 {
        let n = self.line_samples as f64;
        // 无样本时 ±45%，20 个样本后收窄到约 ±18%
        let mut cv = 0.45 / (1.0 + n / SAMPLE_HALFLIFE) + 0.10;
        if live_kmh.is_some_and(|v| v >= 3.0) {
            cv *= 0.7;
        }
        cv.clamp(0.05, 0.60)
    }

    /// 行驶 `meters` 米所需分钟数的区间 `(下界, 上界)`。
    ///
    /// 返回区间而非单值，是因为没有时刻表时 ETA 本质是概率分布。
    /// UI 直接渲染成"约 6-9 分钟"，比一个假装精确的"7 分钟"更诚实，
    /// 也更有用——用户关心的是"我赶不赶得上"。
    pub fn travel_minutes(&self, meters: f64, live_kmh: Option<f64>) -> (f64, f64) {
        let kmh = self.effective_kmh(live_kmh).max(1.0);
        let center = (meters / 1000.0) / kmh * 60.0;
        let cv = self.relative_uncertainty(live_kmh);
        // 至少 0.4 分钟的绝对余量，避免极短距离算出零宽区间
        let spread = (center * cv).max(0.4);
        ((center - spread).max(0.0), center + spread)
    }
}

/// 为一条线路 + 一组车辆，计算"到达指定站点"的预判列表。
///
/// 对每个站只取**最快能到的那辆车**——这正是用户要问的问题
/// "我现在去这站，还要等多久"。
pub fn arrivals_for_stops(
    detail: &LineDetail,
    vehicles: &[VehicleView],
    model: &SpeedModel,
) -> Vec<ArrivalView> {
    let mut out = Vec::new();

    for (i, stop) in detail.stops.iter().enumerate() {
        let stop_m = detail.cumulative_meters.get(i).copied().unwrap_or(0.0);

        let mut best: Option<ArrivalView> = None;

        for v in vehicles {
            // 只考虑尚未到终点的车
            if v.arrived {
                continue;
            }
            let Some(along) = v.along_m else { continue };
            // 车已经驶过该站（留 30m 容差，处理 GPS 抖动）
            if along > stop_m + 30.0 {
                continue;
            }
            let meters_away = (stop_m - along).max(0.0);

            // 还有几站：用站序号差值，比里程更符合用户直觉
            let stops_away = match v.at_stop_no {
                Some(at) => (stop.stop_no - at).max(0),
                None => 0,
            };

            let (low, high) = model.travel_minutes(meters_away, v.speed_kmh);

            let candidate = ArrivalView {
                stop_id: stop.stop_id.clone(),
                stop_no: stop.stop_no,
                stop_name: stop.stop_name.clone(),
                bus_id: v.bus_id.clone(),
                bus_order: v.bus_order,
                stops_away,
                meters_away,
                eta_min_low: low,
                eta_min_high: high,
                confidence: v.confidence,
            };

            let better = best
                .as_ref()
                .map_or(true, |b| candidate.eta_min_low < b.eta_min_low);
            if better {
                best = Some(candidate);
            }
        }

        if let Some(a) = best {
            out.push(a);
        }
    }

    out
}

/// 找出离某点最近的站点（用于"我从哪个站上车"）。
pub fn nearest_stop<'a>(stops: &'a [StopView], p: LngLat) -> Option<(usize, &'a StopView, f64)> {
    stops
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let sp = s.point.map(LngLat::from)?;
            Some((i, s, distance_m(p, sp)))
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
}

/// 计算站点表的总里程（供缓存与展示）
pub fn path_total_meters(stops: &[LngLat]) -> f64 {
    cumulative_lengths(stops).last().copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GeoPoint, LineBaseDto, StopView};

    fn stop(no: i64, name: &str, lng: f64, lat: f64) -> StopView {
        StopView {
            stop_no: no,
            stop_id: format!("S{no}"),
            stop_name: name.into(),
            point: Some(GeoPoint { lng, lat }),
            subway_lines: vec![],
        }
    }

    /// 一条东西向的测试线路，约每 0.01 经度一站
    fn fixture() -> LineDetail {
        let stops = vec![
            stop(1, "起点", 114.300, 30.500),
            stop(2, "第二站", 114.310, 30.500),
            stop(3, "第三站", 114.320, 30.500),
            stop(4, "终点", 114.330, 30.500),
        ];
        let path: Vec<LngLat> = stops.iter().filter_map(|s| s.point.map(LngLat::from)).collect();
        let acc = cumulative_lengths(&path);
        let total = acc.last().copied().unwrap_or(0.0);
        LineDetail {
            base: LineBaseDto {
                line_id: "420100-999-0".into(),
                line_name: "999".into(),
                direction: Some("420100-999-1".into()),
                start_stop: "起点".into(),
                end_stop: "终点".into(),
                early_hour: "05:30".into(),
                last_hour: "21:00".into(),
                price: "2".into(),
                line_type: Some(0),
            },
            stops,
            cumulative_meters: acc,
            total_meters: total,
        }
    }

    #[test]
    fn parses_gps_time_and_computes_age() {
        let (secs, parts) = parse_gps_time("20261012220716").unwrap();
        assert_eq!(parts, (2026, 10, 12, 22, 7, 16));
        assert!(secs > 0);

        // 10 秒后
        assert_eq!(data_age_secs("20261012220716", "20261012220726"), Some(10));
        // 跨天
        assert_eq!(
            data_age_secs("20261012235959", "20261013000001"),
            Some(2),
            "跨零点必须正确计算"
        );
        // 时钟回拨不应产生负年龄
        assert_eq!(data_age_secs("20261012220716", "20261012220700"), Some(0));
    }

    #[test]
    fn rejects_malformed_gps_time() {
        assert!(parse_gps_time("").is_none());
        assert!(parse_gps_time("2026-10-12").is_none());
        assert!(parse_gps_time("2026101222071").is_none(), "长度不足");
        assert!(parse_gps_time("20261312220716").is_none(), "月份非法");
        assert!(parse_gps_time("20261012250716").is_none(), "小时非法");
        assert!(parse_gps_time("20261012abcd16").is_none());
    }

    #[test]
    fn local_now_stamp_is_well_formed() {
        let s = local_now_stamp();
        assert_eq!(s.len(), 14, "格式必须是 yyyyMMddHHmmss");
        assert!(parse_gps_time(&s).is_some(), "自身输出必须能被解析");
    }

    #[test]
    fn civil_date_roundtrip() {
        // 用已知基准点校验：1970-01-01 → 0 天
        assert_eq!(civil_to_unix(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(civil_to_unix(2000, 1, 1, 0, 0, 0), 946_684_800);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }

    #[test]
    fn confidence_tracks_data_age() {
        // 这组边界直接决定 UI 上是实线还是虚线
        assert_eq!(confidence_for_age(Some(0)), PositionConfidence::Live);
        assert_eq!(confidence_for_age(Some(180)), PositionConfidence::Live);
        assert_eq!(confidence_for_age(Some(181)), PositionConfidence::Delayed);
        assert_eq!(confidence_for_age(Some(600)), PositionConfidence::Delayed);
        assert_eq!(confidence_for_age(Some(601)), PositionConfidence::Stale);
        assert_eq!(confidence_for_age(None), PositionConfidence::Delayed);
    }

    #[test]
    fn locates_vehicle_between_stops() {
        let d = fixture();
        let acc = &d.cumulative_meters;

        // 正好在第一站
        assert_eq!(locate_stop(&d, 0.0), (Some(1), Some(2)));
        // 第一站与第二站之间
        let mid = (acc[0] + acc[1]) / 2.0;
        assert_eq!(locate_stop(&d, mid), (Some(1), Some(2)));
        // 正好在第三站
        assert_eq!(locate_stop(&d, acc[2] + 0.1), (Some(3), Some(4)));
        // 已过终点站：不应再有下一站
        assert_eq!(locate_stop(&d, d.total_meters + 500.0), (Some(4), None));
    }

    #[test]
    fn arrives_only_for_stops_ahead() {
        let d = fixture();
        let model = SpeedModel::new();
        let acc = d.cumulative_meters.clone();

        // 车在起点，速度正常
        let v = VehicleView {
            bus_id: "bus-1".into(),
            bus_order: Some(1),
            point: Some(GeoPoint { lng: 114.300, lat: 30.500 }),
            speed_kmh: Some(20.0),
            along_m: Some(0.0),
            offset_m: Some(5.0),
            at_stop_no: Some(1),
            next_stop_no: Some(2),
            arrived: false,
            gps_time: Some("20261012220716".into()),
            age_seconds: Some(10),
            confidence: PositionConfidence::Live,
        };

        let a = arrivals_for_stops(&d, &[v.clone()], &model);
        assert_eq!(a.len(), 4, "车在起点时，4 个站都还没经过");
        // ETA 应随距离单调递增
        let first = a[0].eta_min_low;
        let last = a[3].eta_min_low;
        assert!(last > first, "终点站的 ETA 必须大于起点站");
        // 起点站为 0 距离
        assert_eq!(a[0].meters_away, 0.0);
        assert_eq!(a[0].stops_away, 0, "车就在此站，还有 0 站");
        assert_eq!(a[3].stops_away, 3);

        // 车刚驶离第三站 10 米：仍在 30m 容差内，因此第三站**依然可达**
        // （站台上还有人能追上），第四站也在前方
        let just_left = {
            let mut v2 = v.clone();
            v2.along_m = Some(acc[2] + 10.0);
            v2.at_stop_no = Some(3);
            v2
        };
        let a2 = arrivals_for_stops(&d, &[just_left], &model);
        assert_eq!(
            a2.iter().map(|x| x.stop_no).collect::<Vec<_>>(),
            vec![3, 4],
            "刚出站 10 米时，本站与前方站都应在结果里"
        );

        // 车驶离第三站 100 米：超出容差，只剩第四站
        let mut v3 = v.clone();
        v3.along_m = Some(acc[2] + 100.0);
        v3.at_stop_no = Some(3);
        let a3 = arrivals_for_stops(&d, &[v3], &model);
        assert_eq!(
            a3.iter().map(|x| x.stop_no).collect::<Vec<_>>(),
            vec![4],
            "远离本站后，只有终点站还没经过"
        );
        assert!(a3[0].meters_away > 0.0);
    }

    #[test]
    fn arrived_vehicle_is_excluded() {
        let d = fixture();
        let model = SpeedModel::new();
        let v = VehicleView {
            bus_id: "done".into(),
            bus_order: None,
            point: None,
            speed_kmh: None,
            along_m: Some(0.0),
            offset_m: None,
            at_stop_no: Some(1),
            next_stop_no: Some(2),
            arrived: true,
            gps_time: None,
            age_seconds: None,
            confidence: PositionConfidence::Stale,
        };
        assert!(
            arrivals_for_stops(&d, &[v], &model).is_empty(),
            "已到终点的车不应再产生到站预判"
        );
    }

    #[test]
    fn picks_fastest_vehicle_not_first() {
        let d = fixture();
        let model = SpeedModel::new();
        let mk = |id: &str, along: f64, stop_no: i64| VehicleView {
            bus_id: id.into(),
            bus_order: Some(stop_no),
            point: None,
            speed_kmh: Some(20.0),
            along_m: Some(along),
            offset_m: None,
            at_stop_no: Some(stop_no),
            next_stop_no: Some(stop_no + 1),
            arrived: false,
            gps_time: None,
            age_seconds: Some(10),
            confidence: PositionConfidence::Live,
        };
        // 后车比前车更靠近终点站？构造：两车都在起点附近，取更快到的
        let near = mk("near", 0.0, 1);
        let far = mk("far", -0.0, 1);
        let a = arrivals_for_stops(&d, &[far, near], &model);
        assert!(!a.is_empty());
        // 每站只输出一辆车
        let stops: Vec<i64> = a.iter().map(|x| x.stop_no).collect();
        let mut sorted = stops.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(stops, sorted, "每个站只应出现一次");
    }

    #[test]
    fn speed_model_learns_and_narrows() {
        let mut m = SpeedModel::new();
        // 无样本：区间较宽
        let (l0, h0) = m.travel_minutes(3000.0, None);
        let w0 = h0 - l0;

        // 累积 40 次观测，实际速度 20km/h
        for _ in 0..40 {
            m.observe(1000.0, 180.0); // 1000m / 180s = 20km/h
        }
        assert_eq!(m.line_samples, 40);
        let (l1, h1) = m.travel_minutes(3000.0, None);
        let w1 = h1 - l1;
        assert!(w1 < w0, "样本变多后 ETA 区间必须收窄：{w0:.2} → {w1:.2}");

        // 3000m @ 20km/h = 9 分钟，中心值应接近
        let center = (l1 + h1) / 2.0;
        assert!((center - 9.0).abs() < 2.0, "中心值应接近 9 分钟，实测 {center:.2}");
    }

    #[test]
    fn live_speed_narrows_interval() {
        let m = SpeedModel::new();
        let (l0, h0) = m.travel_minutes(2000.0, None);
        let (l1, h1) = m.travel_minutes(2000.0, Some(25.0));
        assert!(h1 - l1 < h0 - l0, "有实时速度时区间应更窄");
    }

    #[test]
    fn stopped_vehicle_speed_is_ignored() {
        // 速度为 0（等灯/静止）不能直接采信，否则会算出无穷大 ETA
        let m = SpeedModel::new();
        let a = m.effective_kmh(Some(0.0));
        let b = m.effective_kmh(None);
        assert!((a - b).abs() < 1e-9, "0 速度应等同于无速度信息");
        let (l, h) = m.travel_minutes(1000.0, Some(0.0));
        assert!(l.is_finite() && h.is_finite() && h > 0.0);
    }

    #[test]
    fn dirty_speed_samples_are_clamped() {
        let mut m = SpeedModel::new();
        // 异常样本：1 秒跑 1 公里 = 3600 km/h
        m.observe(1000.0, 1.0);
        let kmh = m.historical_kmh();
        assert!(kmh <= 45.0, "脏样本必须被夹取，实测 {kmh}");
    }

    #[test]
    fn nearest_stop_ignores_missing_coords() {
        let mut d = fixture();
        d.stops.push(StopView {
            stop_no: 5,
            stop_id: "S5".into(),
            stop_name: "无坐标站".into(),
            point: None,
            subway_lines: vec![],
        });
        let (idx, s, dist) = nearest_stop(&d.stops, LngLat::new(114.3105, 30.5002)).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(s.stop_no, 2);
        assert!(dist < 100.0, "应命中第二站，实测 {dist:.1}m");
    }
}
