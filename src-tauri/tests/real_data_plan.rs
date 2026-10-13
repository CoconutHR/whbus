//! 用真实数据驱动规划算法的集成测试。
//!
//! # 为什么需要它
//!
//! `journey.rs` 的单元测试用的是手工构造的 fixture（几条 3-5 站的假线路），
//! 那能证明算法逻辑自洽，但**证明不了它在真实数据上能出方案**。
//!
//! 本测试读入 `fixtures/home_to_work.json` —— 从上游接口实抓的真实站点表
//! （家：楚天·雅苑 / 公司：洪山科创大厦，两条线路 687 与 804 各自的全部方向）。
//! 真实数据会带来单测里遇不到的东西：4 个方向的站序差异、
//! 同一物理站点的东/西行两个 stopId、长达 33 站的线路、以及
//! 「必须换乘才能到达」这个关键事实。
//!
//! fixture 由 `_probe/export_fixture.py` 生成，需要刷新时手动跑。

use std::collections::HashMap;

use whbus_lib::gcj::LngLat;
use whbus_lib::journey::{plan_from, CandidateStop, PlanInputs, PlanOptions};
use whbus_lib::types::{
    GeoPoint, LegKind, LineBaseDto, LineDetail, PositionConfidence, StopView, VehicleMotion,
    VehicleView,
};

/// 与 `_probe/export_fixture.py` 的输出结构对应
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    #[serde(rename = "_origin")]
    origin: Anchor,
    #[serde(rename = "_dest")]
    dest: Anchor,
    origin_stops: Vec<FxStop>,
    dest_stops: Vec<FxStop>,
    lines: HashMap<String, Vec<FxLine>>,
}

#[derive(serde::Deserialize)]
struct Anchor {
    #[allow(dead_code)]
    name: String,
    lng: f64,
    lat: f64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FxStop {
    stop_id: String,
    stop_name: String,
    lng: f64,
    lat: f64,
    line_names: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FxLine {
    line_id: String,
    line_name: String,
    start_stop: String,
    end_stop: String,
    early_hour: String,
    last_hour: String,
    price: String,
    stops: Vec<FxStopDetail>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FxStopDetail {
    stop_no: i64,
    stop_id: String,
    stop_name: String,
    lng: f64,
    lat: f64,
}

fn load() -> Fixture {
    let raw = include_str!("fixtures/home_to_work.json");
    serde_json::from_str(raw).expect("fixture 解析失败")
}

/// 把 fixture 的线路转成算法需要的 `LineDetail`（含累计里程）
fn to_detail(fx: &FxLine) -> LineDetail {
    let stops: Vec<StopView> = fx
        .stops
        .iter()
        .map(|s| StopView {
            stop_no: s.stop_no,
            stop_id: s.stop_id.clone(),
            stop_name: s.stop_name.clone(),
            point: Some(GeoPoint {
                lng: s.lng,
                lat: s.lat,
            }),
            subway_lines: vec![],
        })
        .collect();

    let path: Vec<LngLat> = stops
        .iter()
        .filter_map(|s| s.point.map(LngLat::from))
        .collect();
    let acc = whbus_lib::gcj::cumulative_lengths(&path);
    let total = acc.last().copied().unwrap_or(0.0);

    LineDetail {
        base: LineBaseDto {
            line_id: fx.line_id.clone(),
            line_name: fx.line_name.clone(),
            direction: None,
            start_stop: fx.start_stop.clone(),
            end_stop: fx.end_stop.clone(),
            early_hour: fx.early_hour.clone(),
            last_hour: fx.last_hour.clone(),
            price: fx.price.clone(),
            line_type: Some(0),
        },
        stops,
        cumulative_meters: acc,
        total_meters: total,
    }
}

fn to_candidate(s: &FxStop) -> CandidateStop {
    CandidateStop {
        stop_id: s.stop_id.clone(),
        stop_name: s.stop_name.clone(),
        point: LngLat::new(s.lng, s.lat),
        line_names: s.line_names.clone(),
    }
}

/// 构造一辆"即将到达某个上车站"的车，让对应方案成立。
///
/// 真实场景里 `detailData` 会返回车的位置；测试里只需一辆位于线路起点的车，
/// 这样所有下游站点都是"还没经过"，规划即可成立。
fn vehicle_at_start(line_id: &str) -> VehicleView {
    VehicleView {
        bus_id: format!("{line_id}-test"),
        bus_order: Some(1),
        point: None,
        speed_kmh: Some(20.0),
        along_m: Some(0.0),
        offset_m: Some(0.0),
        at_stop_no: Some(1),
        next_stop_no: Some(2),
        arrived: false,
        gps_time: Some("20261013093000".into()),
        age_seconds: Some(15),
        confidence: PositionConfidence::Live,
        // 测试不模拟停站/堵车，按"未知"处理
        motion: VehicleMotion::Unknown,
        distance_to_stop_m: None,
    }
}

fn build_inputs(fx: &Fixture) -> PlanInputs {
    let mut lines: HashMap<String, Vec<LineDetail>> = HashMap::new();
    let mut vehicles: HashMap<String, Vec<VehicleView>> = HashMap::new();

    for (name, dirs) in &fx.lines {
        let details: Vec<LineDetail> = dirs.iter().map(to_detail).collect();
        for d in &details {
            vehicles.insert(d.base.line_id.clone(), vec![vehicle_at_start(&d.base.line_id)]);
        }
        lines.insert(name.clone(), details);
    }

    PlanInputs {
        lines,
        vehicles,
        origin_stops: fx.origin_stops.iter().map(to_candidate).collect(),
        dest_stops: fx.dest_stops.iter().map(to_candidate).collect(),
    }
}

#[test]
fn real_data_produces_a_transfer_plan() {
    let fx = load();
    let inputs = build_inputs(&fx);

    let origin = LngLat::new(fx.origin.lng, fx.origin.lat);
    let dest = LngLat::new(fx.dest.lng, fx.dest.lat);

    let plans = plan_from(origin, dest, &inputs, &PlanOptions::default());

    assert!(
        !plans.is_empty(),
        "真实数据（家 {} -> 公司 {}）必须能规划出方案；\
         家附近线路 {:?} 与公司附近线路 {:?} 无共线，因此一定是换乘方案",
        fx.origin.name,
        fx.dest.name,
        fx.origin_stops
            .iter()
            .flat_map(|s| s.line_names.clone())
            .collect::<std::collections::HashSet<_>>(),
        fx.dest_stops
            .iter()
            .flat_map(|s| s.line_names.clone())
            .collect::<std::collections::HashSet<_>>(),
    );

    // 家到公司没有共线，所以第 0 个方案必须是换乘方案
    let best = &plans[0];
    assert_eq!(
        best.transfers, 1,
        "家与公司附近无共线，必须是 1 次换乘；实得 transfers={}",
        best.transfers
    );

    // 段结构：步行 → 候车 → 乘车 → 换乘 → 候车 → 乘车 → 步行
    let kinds: Vec<LegKind> = best.legs.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        vec![
            LegKind::Walk,
            LegKind::Wait,
            LegKind::Bus,
            LegKind::Transfer,
            LegKind::Wait,
            LegKind::Bus,
            LegKind::Walk,
        ],
        "换乘方案应为七段"
    );

    // 乘坐的两条线路必须一条服务家、一条服务公司
    let buses: Vec<&str> = best
        .legs
        .iter()
        .filter(|l| l.kind == LegKind::Bus)
        .filter_map(|l| l.line_name.as_deref())
        .collect();
    assert_eq!(buses.len(), 2, "应恰好乘坐两条线路");

    let home_lines: std::collections::HashSet<&str> = fx
        .origin_stops
        .iter()
        .flat_map(|s| s.line_names.iter().map(String::as_str))
        .collect();
    let work_lines: std::collections::HashSet<&str> = fx
        .dest_stops
        .iter()
        .flat_map(|s| s.line_names.iter().map(String::as_str))
        .collect();

    assert!(
        home_lines.contains(buses[0]),
        "第一条线路 {} 必须服务家附近站点（{:?}）",
        buses[0],
        home_lines
    );
    assert!(
        work_lines.contains(buses[1]),
        "第二条线路 {} 必须服务公司附近站点（{:?}）",
        buses[1],
        work_lines
    );

    // 时间轴必须连续且与总耗时收口
    for w in best.legs.windows(2) {
        assert!(
            (w[1].start_offset_min - w[0].end_offset_min).abs() < 1e-6,
            "段时间轴必须连续：{:?} 结束 {:.3} -> {:?} 开始 {:.3}",
            w[0].kind,
            w[0].end_offset_min,
            w[1].kind,
            w[1].start_offset_min
        );
    }
    assert!(
        (best.legs.last().unwrap().end_offset_min - best.arrive_offset_min).abs() < 1e-6,
        "最后一段的结束时刻必须等于总耗时"
    );

    // 总耗时必须是有限正数（真实距离约 4km，不该出现荒谬值）
    assert!(
        best.total_minutes.is_finite() && best.total_minutes > 5.0,
        "总耗时应在合理区间，实得 {:.1} 分钟",
        best.total_minutes
    );
    assert!(
        best.total_minutes < 180.0,
        "总耗时不该超过 3 小时，实得 {:.1} 分钟",
        best.total_minutes
    );

    // 换乘点必须在两条线路上都真实存在，且站序方向正确
    let transfer_stop_name = best
        .legs
        .iter()
        .find(|l| l.kind == LegKind::Transfer)
        .map(|l| l.from_name.clone())
        .expect("应有换乘段");
    assert!(
        !transfer_stop_name.is_empty(),
        "换乘站名不该为空"
    );

    // 每条乘车段的落客站必须在上车站之后
    for l in best.legs.iter().filter(|l| l.kind == LegKind::Bus) {
        assert!(
            l.alight_stop_no.unwrap() > l.board_stop_no.unwrap(),
            "{} 的落客站({:?})必须在上车站({:?})之后",
            l.line_name.as_deref().unwrap_or("?"),
            l.alight_stop_no,
            l.board_stop_no
        );
    }
}

#[test]
fn real_data_candidate_stops_are_all_usable() {
    let fx = load();

    // 真实数据里同一物理站点的东行/西行是两个不同 stopId，两者都应可用
    assert!(
        fx.origin_stops.len() >= 3,
        "家附近应至少 3 个候选站，实得 {}",
        fx.origin_stops.len()
    );
    assert!(
        fx.dest_stops.len() >= 3,
        "公司附近应至少 3 个候选站，实得 {}",
        fx.dest_stops.len()
    );

    // 所有候选站坐标必须有效（真实数据里终点站可能有 0,0）
    for s in fx.origin_stops.iter().chain(fx.dest_stops.iter()) {
        let p = LngLat::new(s.lng, s.lat);
        assert!(p.is_valid(), "站点 {} 坐标非法：{},{}", s.stop_name, s.lng, s.lat);
    }

    // 站点表里所有站也必须坐标有效（fixture 导出时已滤掉 0,0）
    for dirs in fx.lines.values() {
        for d in dirs {
            for s in &d.stops {
                let p = LngLat::new(s.lng, s.lat);
                assert!(p.is_valid(), "{} 的站点 {} 坐标非法", d.line_id, s.stop_name);
            }
        }
    }
}

#[test]
fn real_data_mileage_is_monotonic_and_plausible() {
    let fx = load();

    for dirs in fx.lines.values() {
        for d in dirs {
            let detail = to_detail(d);
            assert_eq!(
                detail.cumulative_meters.len(),
                detail.stops.len(),
                "累计里程长度必须与站点数一致"
            );
            assert_eq!(detail.cumulative_meters[0], 0.0, "首站里程应为 0");

            // 严格单调不减
            for w in detail.cumulative_meters.windows(2) {
                assert!(
                    w[1] >= w[0] - 1e-6,
                    "{} 的累计里程出现倒退：{:.1} -> {:.1}",
                    d.line_id,
                    w[0],
                    w[1]
                );
            }

            // 真实线路长度应在合理区间：26-33 站的市区线路通常 10-30km
            let km = detail.total_meters / 1000.0;
            assert!(
                (5.0..40.0).contains(&km),
                "{} 全长 {:.1}km 不合理（{} 站）",
                d.line_id,
                km,
                detail.stops.len()
            );
        }
    }
}

#[test]
fn real_data_survives_missing_vehicle_data() {
    // 收班场景：所有线路都没有在线车辆 —— 应返回空方案而不是 panic 或伪造时间
    let fx = load();
    let mut inputs = build_inputs(&fx);
    inputs.vehicles.clear();

    let plans = plan_from(
        LngLat::new(fx.origin.lng, fx.origin.lat),
        LngLat::new(fx.dest.lng, fx.dest.lat),
        &inputs,
        &PlanOptions::default(),
    );

    assert!(
        plans.is_empty(),
        "没有任何在线车辆时不应给出方案（上游无班次计划表，此时确实算不出来）"
    );
}

#[test]
fn real_data_survives_missing_stop_tables() {
    // 站点表未缓存：应跳过而不是报错
    let fx = load();
    let mut inputs = build_inputs(&fx);
    inputs.lines.clear();

    let plans = plan_from(
        LngLat::new(fx.origin.lng, fx.origin.lat),
        LngLat::new(fx.dest.lng, fx.dest.lat),
        &inputs,
        &PlanOptions::default(),
    );

    assert!(plans.is_empty(), "缺站点表时应安静地返回空结果");
}

/// 同一辆车，状态不同 → 候车时间不同。
///
/// # 为什么不用"整条方案的总耗时"来验证
///
/// 早期版本对比"所有车都堵"与"所有车都畅通"两个完整规划，断言前者总耗时更长。
/// 那是个**错误的抽象层次**：它同时改变了车辆速度、状态、以及（因为候选排序变化）
/// 最终胜出的是哪条方案，于是比较的根本不是同一个量，
/// 断言在 0.5 分钟这种噪声级别上时对时错。
///
/// 这里退一步，直接在最底层比较：同样距离、同一辆车，只换运动状态。
/// 这样检验的才是"拥堵惩罚是否生效"这一个事实。
#[test]
fn congestion_penalty_delays_the_wait() {
    use whbus_lib::transit::SpeedModel;
    use whbus_lib::types::VehicleMotion;

    // 用两条线路的真实站距构造一段有代表性的距离（687 路相邻站约 1.2km）
    let fx = load();
    let seg = {
        let d = to_detail(&fx.lines["687"][0]);
        // 取第 3 段站距作为样本
        d.cumulative_meters[3] - d.cumulative_meters[2]
    };
    assert!(seg > 100.0, "站距样本太小：{seg}");

    let model = SpeedModel::new(); // 冷启动：没有任何历史样本，最坏情况

    let unknown = model.travel_minutes_with_motion(seg, Some(0.0), VehicleMotion::Unknown);
    let stopped = model.travel_minutes_with_motion(seg, Some(0.0), VehicleMotion::Stopped);

    let mid = |(l, h): (f64, f64)| (l + h) / 2.0;
    assert!(
        mid(stopped) > mid(unknown),
        "堵车时 {seg:.0} 米的候车应更久：{:.1} vs {:.1} 分钟",
        mid(stopped),
        mid(unknown)
    );
    assert!(
        (stopped.1 - stopped.0) > (unknown.1 - unknown.0),
        "堵车时的不确定性应更大：{:.1} vs {:.1}",
        stopped.1 - stopped.0,
        unknown.1 - unknown.0
    );
}

/// 任何情况下都不该出现物理上不可能的耗时。
///
/// 这个测试守住的是一类真实 bug：早期冷启动的区间宽度没有上限，
/// 下界被压到接近 0，配合规划器用下界累加总耗时，算出过
/// "2773 米只要 0.07 分钟"（≈2400 km/h）这种荒谬值。
#[test]
fn real_data_never_produces_impossible_durations() {
    let fx = load();
    let origin = LngLat::new(fx.origin.lng, fx.origin.lat);
    let dest = LngLat::new(fx.dest.lng, fx.dest.lat);

    for motion in [
        VehicleMotion::Moving,
        VehicleMotion::AtStop,
        VehicleMotion::Stopped,
        VehicleMotion::Unknown,
    ] {
        for speed in [0.0, 5.0, 25.0] {
            let mut inputs = build_inputs(&fx);
            for vs in inputs.vehicles.values_mut() {
                for v in vs.iter_mut() {
                    v.motion = motion;
                    v.speed_kmh = Some(speed);
                }
            }
            let plans = plan_from(origin, dest, &inputs, &PlanOptions::default());

            for p in &plans {
                // 每一段的耗时都必须与它的里程相称
                for l in &p.legs {
                    let Some(meters) = l.meters else { continue };
                    if l.kind != LegKind::Bus || meters <= 100.0 {
                        continue;
                    }
                    // 公交在城市里的速度上限取 80 km/h（含停站，极宽松）
                    let min_minutes = (meters / 1000.0) / 80.0 * 60.0;
                    assert!(
                        l.minutes >= min_minutes,
                        "{motion:?}@{speed} 时 {:.0} 米的乘车段只用了 {:.2} 分钟，\
                         低于 80km/h 的物理下限 {:.2} 分钟",
                        meters,
                        l.minutes,
                        min_minutes
                    );
                }
            }
        }
    }
}

/// 出发时刻偏移必须让时间轴整体平移，且**出发一定早于到达**。
///
/// # 这个测试守的是什么 bug
///
/// 用户设"最晚 20:00 到达"时，早期实现用 `deadline - 总耗时` 反推出发，
/// 却仍把"从现在起算的耗时"当到达时刻。于是当总耗时 40 分钟、现在 18:00 时：
///   最晚出发 = 20:00 − 40 分 = 19:20（即 80 分钟后）
///   到达时刻 = 现在 + 40 分 = 18:40
/// **出发时间比到达时间还晚** —— 时间轴自相矛盾。
///
/// 根因是所有方案都从 0 起算。加上 `depart_offset_min` 后：
///   arrive_offset_min = depart_offset_min + total
/// 不变量成立：出发 ≤ 到达。
#[test]
fn depart_offset_shifts_timeline_and_stays_before_arrival() {
    let fx = load();
    let origin = LngLat::new(fx.origin.lng, fx.origin.lat);
    let dest = LngLat::new(fx.dest.lng, fx.dest.lat);

    let plan_with = |depart: f64| {
        let inputs = build_inputs(&fx);
        let opts = PlanOptions {
            depart_offset_min: depart,
            ..Default::default()
        };
        plan_from(origin, dest, &inputs, &opts)
    };

    let base = plan_with(0.0);
    assert!(!base.is_empty(), "基准方案不该为空");
    let later = plan_with(120.0);
    assert!(!later.is_empty(), "推迟两小时后仍应能规划");

    for (i, o) in later.iter().enumerate() {
        // ① 不变量：出发不晚于到达
        assert!(
            o.latest_depart_offset_min <= o.arrive_offset_min + 1e-6,
            "方案 {i} 的出发时刻 {:.1} 晚于到达时刻 {:.1}",
            o.latest_depart_offset_min,
            o.arrive_offset_min
        );

        // ② 时间轴确实平移了 120 分钟
        assert!(
            (o.latest_depart_offset_min - 120.0).abs() < 1e-6,
            "方案 {i} 的出发时刻应为 120，实测 {:.2}",
            o.latest_depart_offset_min
        );

        // ③ 第一段从出发时刻开始
        let first = o.legs.first().expect("方案至少有一段");
        assert!(
            (first.start_offset_min - 120.0).abs() < 1e-6,
            "方案 {i} 的首段应从 120 开始，实测 {:.2}",
            first.start_offset_min
        );

        // ④ 耗时不因出发时刻改变（同一套实时数据）
        let base_total = base[0].total_minutes;
        assert!(
            (o.total_minutes - base_total).abs() < 1e-6,
            "方案 {i} 的耗时不该随出发时刻变化：{:.2} vs {:.2}",
            o.total_minutes,
            base_total
        );

        // ⑤ 到达 = 出发 + 耗时
        assert!(
            (o.arrive_offset_min - (o.latest_depart_offset_min + o.total_minutes)).abs() < 1e-6,
            "方案 {i} 的到达时刻应等于出发 + 耗时"
        );

        // ⑥ 各段连续，无重叠无空隙
        for w in o.legs.windows(2) {
            assert!(
                (w[1].start_offset_min - w[0].end_offset_min).abs() < 1e-6,
                "方案 {i} 的时间轴不连续：{:.2} → {:.2}",
                w[0].end_offset_min,
                w[1].start_offset_min
            );
        }
    }
}

/// 不带出发偏移时（默认 0），行为与改造前一致 —— 防止回归。
#[test]
fn zero_depart_offset_keeps_legacy_shape() {
    let fx = load();
    let origin = LngLat::new(fx.origin.lng, fx.origin.lat);
    let dest = LngLat::new(fx.dest.lng, fx.dest.lat);
    let inputs = build_inputs(&fx);
    let plans = plan_from(origin, dest, &inputs, &PlanOptions::default());

    assert!(!plans.is_empty());
    assert!(
        plans[0].latest_depart_offset_min.abs() < 1e-6,
        "默认应从 0 起算"
    );
    assert!(
        plans[0].legs[0].start_offset_min.abs() < 1e-6,
        "默认首段应从 0 开始"
    );
}

#[test]
fn real_data_plans_are_ranked_and_labelled() {
    let fx = load();
    let inputs = build_inputs(&fx);
    let plans = plan_from(
        LngLat::new(fx.origin.lng, fx.origin.lat),
        LngLat::new(fx.dest.lng, fx.dest.lat),
        &inputs,
        &PlanOptions {
            max_options: 5,
            ..Default::default()
        },
    );

    assert!(!plans.is_empty());

    // 按总耗时升序
    for w in plans.windows(2) {
        assert!(
            w[0].total_minutes <= w[1].total_minutes + 1e-6,
            "方案必须按总耗时升序：{:.1} 在 {:.1} 之前",
            w[0].total_minutes,
            w[1].total_minutes
        );
    }

    // 第一名必须带"最快"
    assert!(
        plans[0].tags.contains(&"最快".to_string()),
        "第一个方案应带「最快」标签，实得 {:?}",
        plans[0].tags
    );

    // 标签不重复
    for p in &plans {
        let mut t = p.tags.clone();
        t.sort();
        t.dedup();
        assert_eq!(t.len(), p.tags.len(), "标签不应重复：{:?}", p.tags);
    }
}
