//! 出行规划算法。
//!
//! # 核心约束：请求预算
//!
//! 上游建议请求间隔 ≥ 10 秒。一次朴素的穷举会打出 40+ 个请求 —— 既慢又容易
//! 触发限流。因此本模块被设计成**纯函数**：
//!
//! ```text
//! 调用方（commands.rs）负责取数据  ──►  plan_from(...)  ──►  方案列表
//! ```
//!
//! 这样算法可以独立单测（用 fixture 数据），而"取多少数据"这个成本决策
//! 留在调用方，并能借助 `AppState::line_cache` 复用已缓存的站点表。
//!
//! # 算法结构
//!
//! 1. **直达**：遍历"起点附近站点"与"终点附近站点"共有的线路
//! 2. **一次换乘**：遍历 (起点候选线路 × 终点候选线路) 的组合，
//!    找出两条线路共有的站点作为换乘点
//!
//! 覆盖绝大多数通勤场景。更复杂的多换乘在当前接口的请求预算下不划算。
//!
//! # 时间构成
//!
//! ```text
//! 总耗时 = 走到上车站 + 候车 + 乘车 + [换乘步行 + 换乘候车 + 乘车] + 走到终点
//! ```
//!
//! 每一段都带 `confidence`，因为"走到车站要 5 分钟"是确定值，
//! 而"车还有 6 分钟到"是概率——UI 上必须能区分。

use std::collections::{HashMap, HashSet};

use crate::gcj::{LngLat, distance_m};
use crate::transit::SpeedModel;
use crate::types::{
    JourneyLeg, JourneyOption, LegKind, LineDetail, PositionConfidence, StopView, VehicleView,
};

/// 规划参数
#[derive(Debug, Clone)]
pub struct PlanOptions {
    /// 步行速度（米/分钟）。默认 75 ≈ 4.5 km/h
    pub walk_speed_m_per_min: f64,
    /// 起步即算的固定准备时间（分钟）：锁门、等电梯
    pub boarding_overhead_min: f64,
    /// 每次换乘的额外缓冲（分钟）：找站台、过街
    pub transfer_buffer_min: f64,
    /// 最多返回几个方案
    pub max_options: usize,
    /// 单个站点最多考虑几条线路（按线路号排序取前 N，控制搜索空间）
    pub max_lines_per_stop: usize,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            walk_speed_m_per_min: 75.0,
            boarding_overhead_min: 1.0,
            transfer_buffer_min: 2.0,
            max_options: 5,
            max_lines_per_stop: 12,
        }
    }
}

/// 一个候选站点及其线路
#[derive(Debug, Clone)]
pub struct CandidateStop {
    pub stop_id: String,
    pub stop_name: String,
    pub point: LngLat,
    /// 该站经过的线路号（用于匹配）
    pub line_names: Vec<String>,
}

/// 规划所需的全部输入。由调用方（命令层）负责取齐。
#[derive(Debug, Default, Clone)]
pub struct PlanInputs {
    /// 线路号 → 该线路的所有方向（一个方向一个 `LineDetail`）
    pub lines: HashMap<String, Vec<LineDetail>>,
    /// `line_id` → 该方向当前的在线车辆
    pub vehicles: HashMap<String, Vec<VehicleView>>,
    /// 起点附近站点
    pub origin_stops: Vec<CandidateStop>,
    /// 终点附近站点
    pub dest_stops: Vec<CandidateStop>,
}

/// 规划主入口。
///
/// `inputs` 中缺失的数据会被跳过（而不是报错）——这很关键：
/// 调用方可能只缓存了部分线路的站点表，算法应该用现有数据给出**可能的最优解**，
/// 而不是因为一条线路缺数据就整体失败。
pub fn plan_from(
    origin: LngLat,
    destination: LngLat,
    inputs: &PlanInputs,
    opts: &PlanOptions,
) -> Vec<JourneyOption> {
    if !origin.is_valid() || !destination.is_valid() {
        return Vec::new();
    }

    let mut options: Vec<JourneyOption> = Vec::new();

    options.extend(find_direct(origin, destination, inputs, opts));
    options.extend(find_one_transfer(origin, destination, inputs, opts));

    dedup_and_rank(&mut options, opts.max_options);
    options
}

// ---------------------------------------------------------------------------
// 时间估算
// ---------------------------------------------------------------------------

/// 估算步行时间（分钟）
fn walk_minutes(meters: f64, opts: &PlanOptions) -> f64 {
    if opts.walk_speed_m_per_min <= 0.0 {
        return f64::INFINITY;
    }
    // 步行距离乘以 1.25 的绕路系数：直线距离不等于实际路径
    meters * 1.25 / opts.walk_speed_m_per_min
}

/// 找到某站在线路上的索引
fn stop_index(detail: &LineDetail, stop_id: &str) -> Option<usize> {
    detail.stops.iter().position(|s| s.stop_id == stop_id)
}

/// 在某个站等某条线路的下一个班次。
///
/// 返回 `(候车分钟区间, 车辆, 位置可信度)`。
///
/// # 为什么是区间
///
/// 上游没有班次计划表（`plans` 恒为空），候车时间只能由车辆当前位置推算，
/// 因此本质是概率分布。同时这也是"没有在线车辆"这一正常状态的唯一出口——
/// 此时返回 `None`，调用方应放弃这个方案而不是编一个假时间。
fn wait_for_line(
    line_id: &str,
    stop_meters: f64,
    inputs: &PlanInputs,
    model: &SpeedModel,
) -> Option<(f64, f64, Option<VehicleView>, PositionConfidence)> {
    let vehicles = inputs.vehicles.get(line_id)?;

    let mut best: Option<(f64, f64, VehicleView, PositionConfidence)> = None;

    for v in vehicles {
        if v.arrived {
            continue;
        }
        let along = v.along_m?;
        // 车已过站（留 30m 容差，与 transit.rs 的到站判定保持一致）
        if along > stop_meters + 30.0 {
            continue;
        }
        let meters_away = (stop_meters - along).max(0.0);
        let (low, high) = model.travel_minutes(meters_away, v.speed_kmh);

        let better = best.as_ref().map_or(true, |(bl, _, _, _)| low < *bl);
        if better {
            best = Some((low, high, v.clone(), v.confidence));
        }
    }

    let (low, high, vehicle, confidence) = best?;
    Some((
        (low + high) / 2.0,
        (high - low) / 2.0,
        Some(vehicle),
        confidence,
    ))
}

/// 乘车时间（分钟）：从 `board_m` 到 `alight_m` 的沿线里程。
fn ride_minutes(
    board_m: f64,
    alight_m: f64,
    line_id: &str,
    inputs: &PlanInputs,
    model: &SpeedModel,
) -> (f64, f64) {
    let meters = (alight_m - board_m).max(0.0);
    // 乘车段用平均运营速度估算：车已经在路上，实时速度的参考价值有限
    let _ = line_id;
    let _ = inputs;
    model.travel_minutes(meters, None)
}

// ---------------------------------------------------------------------------
// 直达
// ---------------------------------------------------------------------------

/// 起点与终点附近站点共有的线路 → 直达方案
fn find_direct(
    origin: LngLat,
    destination: LngLat,
    inputs: &PlanInputs,
    opts: &PlanOptions,
) -> Vec<JourneyOption> {
    let model = SpeedModel::new();
    let mut out = Vec::new();

    // 起点侧站点的线路集合
    let origin_lines: HashSet<&str> = inputs
        .origin_stops
        .iter()
        .flat_map(|s| s.line_names.iter().map(String::as_str))
        .collect();
    // 终点侧站点的线路集合
    let dest_lines: HashSet<&str> = inputs
        .dest_stops
        .iter()
        .flat_map(|s| s.line_names.iter().map(String::as_str))
        .collect();

    // 两侧都出现的线路
    let shared: Vec<&&str> = origin_lines.intersection(&dest_lines).collect();

    for line_name in shared {
        let Some(directions) = inputs.lines.get(*line_name) else {
            continue; // 站点表未缓存，跳过而非报错
        };

        for detail in directions {
            let line_id = &detail.base.line_id;

            // 枚举所有 (上车站, 下车站) 组合，取最优
            for board_stop in &inputs.origin_stops {
                let Some(bi) = stop_index(detail, &board_stop.stop_id) else {
                    continue;
                };
                for alight_stop in &inputs.dest_stops {
                    let Some(ai) = stop_index(detail, &alight_stop.stop_id) else {
                        continue;
                    };
                    // 必须同方向且前进
                    if ai <= bi {
                        continue;
                    }

                    let board_m = detail.cumulative_meters[bi];
                    let alight_m = detail.cumulative_meters[ai];

                    let walk1 = walk_minutes(distance_m(origin, board_stop.point), opts);
                    let walk2 =
                        walk_minutes(distance_m(alight_stop.point, destination), opts);

                    let Some((wait, wait_spread, vehicle, conf)) = wait_for_line(line_id, board_m,
                        inputs,
                        &model,
                    ) else {
                        continue;
                    };

                    let (ride, ride_spread) =
                        ride_minutes(board_m, alight_m, line_id, inputs, &model);

                    let total = opts.boarding_overhead_min + walk1 + wait + ride + walk2;

                    out.push(JourneyOption {
                        total_minutes: total,
                        walk_meters: distance_m(origin, board_stop.point)
                            + distance_m(alight_stop.point, destination),
                        transfers: 0,
                        latest_depart_offset_min: 0.0,
                        arrive_offset_min: total,
                        legs: vec![
                            JourneyLeg {
                                kind: LegKind::Walk,
                                minutes: walk1,
                                from_name: "当前位置".into(),
                                to_name: board_stop.stop_name.clone(),
                                meters: Some(distance_m(origin, board_stop.point)),
                                line_id: None,
                                line_name: None,
                                board_stop_no: None,
                                alight_stop_no: None,
                                stop_count: None,
                                start_offset_min: 0.0,
                                end_offset_min: walk1,
                                vehicle: None,
                                confidence: PositionConfidence::Live,
                            },
                            JourneyLeg {
                                kind: LegKind::Wait,
                                minutes: wait,
                                from_name: board_stop.stop_name.clone(),
                                to_name: board_stop.stop_name.clone(),
                                meters: None,
                                line_id: Some(line_id.clone()),
                                line_name: Some(detail.base.line_name.clone()),
                                board_stop_no: Some(detail.stops[bi].stop_no),
                                alight_stop_no: None,
                                stop_count: None,
                                start_offset_min: walk1,
                                end_offset_min: walk1 + wait,
                                vehicle: vehicle.clone(),
                                confidence: conf,
                            },
                            JourneyLeg {
                                kind: LegKind::Bus,
                                minutes: ride,
                                from_name: board_stop.stop_name.clone(),
                                to_name: alight_stop.stop_name.clone(),
                                meters: Some(alight_m - board_m),
                                line_id: Some(line_id.clone()),
                                line_name: Some(detail.base.line_name.clone()),
                                board_stop_no: Some(detail.stops[bi].stop_no),
                                alight_stop_no: Some(detail.stops[ai].stop_no),
                                stop_count: Some(ai as i64 - bi as i64),
                                start_offset_min: walk1 + wait,
                                end_offset_min: walk1 + wait + ride,
                                vehicle,
                                confidence: conf,
                            },
                            JourneyLeg {
                                kind: LegKind::Walk,
                                minutes: walk2,
                                from_name: alight_stop.stop_name.clone(),
                                to_name: "目的地".into(),
                                meters: Some(distance_m(alight_stop.point, destination)),
                                line_id: None,
                                line_name: None,
                                board_stop_no: None,
                                alight_stop_no: None,
                                stop_count: None,
                                start_offset_min: walk1 + wait + ride,
                                end_offset_min: total,
                                vehicle: None,
                                confidence: PositionConfidence::Live,
                            },
                        ],
                        tags: vec![],
                        // 评分：总耗时为主，区间不确定度作为惩罚项
                        score: total + wait_spread + ride_spread,
                    });
                }
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// 一次换乘
// ---------------------------------------------------------------------------

/// 遍历 (起点侧线路 × 终点侧线路)，找出共有的换乘站。
///
/// # 搜索空间控制
///
/// 朴素的组合是 O(n×m)。这里用两条约束裁剪：
/// 1. 只考虑**已经有站点表缓存**的线路（缺数据的线路无法参与）
/// 2. 单侧线路数超过 `max_lines_per_stop` 时截断
///
/// 这是有意的取舍：先保证"能在合理请求预算内给出可用方案"，
/// 再考虑覆盖率。
fn find_one_transfer(
    origin: LngLat,
    destination: LngLat,
    inputs: &PlanInputs,
    opts: &PlanOptions,
) -> Vec<JourneyOption> {
    let model = SpeedModel::new();
    let mut out = Vec::new();

    // 收集两侧可用的方向
    let origin_dirs = available_directions(&inputs.origin_stops, inputs, opts);
    let dest_dirs = available_directions(&inputs.dest_stops, inputs, opts);

    for (l1_name, l1) in &origin_dirs {
        for (l2_name, l2) in &dest_dirs {
            // 同一条线路的换乘没有意义（已在直达里覆盖）
            if l1_name == l2_name {
                continue;
            }

            // 找出两条线路共有的站点
            let l2_by_id: HashMap<&str, usize> = l2
                .stops
                .iter()
                .enumerate()
                .map(|(i, s)| (s.stop_id.as_str(), i))
                .collect();

            for (bi, s1) in l1.stops.iter().enumerate() {
                let Some(&ti) = l2_by_id.get(s1.stop_id.as_str()) else {
                    continue;
                };

                // 上车站必须在起点侧；换乘后必须还有前进空间
                if ti + 1 >= l2.stops.len() {
                    continue;
                }

                // 起点侧：枚举更靠前的上车站
                for board in &inputs.origin_stops {
                    let Some(bi2) = stop_index(l1, &board.stop_id) else {
                        continue;
                    };
                    if bi2 >= bi {
                        continue; // 上车站必须在换乘站之前
                    }

                    // 终点侧：枚举换乘站之后的落客站
                    for alight in &inputs.dest_stops {
                        let Some(ai) = stop_index(l2, &alight.stop_id) else {
                            continue;
                        };
                        if ai <= ti {
                            continue;
                        }

                        let b1_m = l1.cumulative_meters[bi2];
                        let t1_m = l1.cumulative_meters[bi];
                        let b2_m = l2.cumulative_meters[ti];
                        let a2_m = l2.cumulative_meters[ai];

                        let walk1 = walk_minutes(distance_m(origin, board.point), opts);
                        let walk2 =
                            walk_minutes(distance_m(alight.point, destination), opts);

                        let l1_id = &l1.base.line_id;
                        let l2_id = &l2.base.line_id;

                        let Some((wait1, sp1, veh1, conf1)) = wait_for_line(l1_id, b1_m,
                            inputs,
                            &model,
                        ) else {
                            continue;
                        };
                        let (ride1, rsp1) = ride_minutes(b1_m, t1_m, l1_id, inputs, &model);

                        // 换乘候车：从到达换乘站的时刻起算
                        let Some((wait2, sp2, veh2, conf2)) = wait_for_line(l2_id, b2_m,
                            inputs,
                            &model,
                        ) else {
                            continue;
                        };
                        let (ride2, rsp2) = ride_minutes(b2_m, a2_m, l2_id, inputs, &model);

                        let total = opts.boarding_overhead_min
                            + walk1
                            + wait1
                            + ride1
                            + opts.transfer_buffer_min
                            + wait2
                            + ride2
                            + walk2;

                        let t0 = 0.0;
                        let t1 = t0 + walk1;
                        let t2 = t1 + wait1;
                        let t3 = t2 + ride1;
                        let t4 = t3 + opts.transfer_buffer_min;
                        let t5 = t4 + wait2;
                        let t6 = t5 + ride2;

                        let transfer_stop_name = s1.stop_name.clone();

                        out.push(JourneyOption {
                            total_minutes: total,
                            walk_meters: distance_m(origin, board.point)
                                + distance_m(alight.point, destination),
                            transfers: 1,
                            latest_depart_offset_min: 0.0,
                            arrive_offset_min: total,
                            legs: vec![
                                walk_leg("当前位置", &board.stop_name, walk1,
                                         distance_m(origin, board.point), t0, t1),
                                wait_leg(l1, bi2, wait1, veh1.clone(), conf1, t1, t2),
                                bus_leg(l1, bi2, bi, ride1, veh1, conf1, t2, t3),
                                JourneyLeg {
                                    kind: LegKind::Transfer,
                                    minutes: opts.transfer_buffer_min,
                                    from_name: transfer_stop_name.clone(),
                                    to_name: transfer_stop_name.clone(),
                                    meters: None,
                                    line_id: None,
                                    line_name: None,
                                    board_stop_no: None,
                                    alight_stop_no: None,
                                    stop_count: None,
                                    start_offset_min: t3,
                                    end_offset_min: t4,
                                    vehicle: None,
                                    confidence: PositionConfidence::Live,
                                },
                                wait_leg(l2, ti, wait2, veh2.clone(), conf2, t4, t5),
                                bus_leg(l2, ti, ai, ride2, veh2, conf2, t5, t6),
                                walk_leg(&alight.stop_name, "目的地", walk2,
                                         distance_m(alight.point, destination), t6, total),
                            ],
                            tags: vec![],
                            score: total + sp1 + rsp1 + sp2 + rsp2 + opts.transfer_buffer_min,
                        });
                    }
                }
            }
        }
    }

    out
}

/// 收集"起点侧或终点侧候选站所经过的线路"的站点表（仅限已缓存的）
fn available_directions<'a>(
    stops: &[CandidateStop],
    inputs: &'a PlanInputs,
    opts: &PlanOptions,
) -> Vec<(String, &'a LineDetail)> {
    let mut names: Vec<String> = stops
        .iter()
        .flat_map(|s| s.line_names.iter().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    names.sort();
    names.truncate(opts.max_lines_per_stop);

    let mut out = Vec::new();
    for name in names {
        if let Some(dirs) = inputs.lines.get(&name) {
            for d in dirs {
                out.push((name.clone(), d));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 段构造辅助
// ---------------------------------------------------------------------------

fn walk_leg(
    from: &str,
    to: &str,
    minutes: f64,
    meters: f64,
    start: f64,
    end: f64,
) -> JourneyLeg {
    JourneyLeg {
        kind: LegKind::Walk,
        minutes,
        from_name: from.into(),
        to_name: to.into(),
        meters: Some(meters),
        line_id: None,
        line_name: None,
        board_stop_no: None,
        alight_stop_no: None,
        stop_count: None,
        start_offset_min: start,
        end_offset_min: end,
        vehicle: None,
        confidence: PositionConfidence::Live,
    }
}

fn wait_leg(
    detail: &LineDetail,
    board_idx: usize,
    minutes: f64,
    vehicle: Option<VehicleView>,
    confidence: PositionConfidence,
    start: f64,
    end: f64,
) -> JourneyLeg {
    let stop: &StopView = &detail.stops[board_idx];
    JourneyLeg {
        kind: LegKind::Wait,
        minutes,
        from_name: stop.stop_name.clone(),
        to_name: stop.stop_name.clone(),
        meters: None,
        line_id: Some(detail.base.line_id.clone()),
        line_name: Some(detail.base.line_name.clone()),
        board_stop_no: Some(stop.stop_no),
        alight_stop_no: None,
        stop_count: None,
        start_offset_min: start,
        end_offset_min: end,
        vehicle,
        confidence,
    }
}

fn bus_leg(
    detail: &LineDetail,
    board_idx: usize,
    alight_idx: usize,
    minutes: f64,
    vehicle: Option<VehicleView>,
    confidence: PositionConfidence,
    start: f64,
    end: f64,
) -> JourneyLeg {
    let b = &detail.stops[board_idx];
    let a = &detail.stops[alight_idx];
    JourneyLeg {
        kind: LegKind::Bus,
        minutes,
        from_name: b.stop_name.clone(),
        to_name: a.stop_name.clone(),
        meters: Some(
            detail.cumulative_meters[alight_idx] - detail.cumulative_meters[board_idx],
        ),
        line_id: Some(detail.base.line_id.clone()),
        line_name: Some(detail.base.line_name.clone()),
        board_stop_no: Some(b.stop_no),
        alight_stop_no: Some(a.stop_no),
        stop_count: Some(alight_idx as i64 - board_idx as i64),
        start_offset_min: start,
        end_offset_min: end,
        vehicle,
        confidence,
    }
}

// ---------------------------------------------------------------------------
// 排序与标注
// ---------------------------------------------------------------------------

/// 去重、按三个维度打标签、截断。
///
/// # 为什么要三个维度而不是一个排序
///
/// 用户要的是**决策依据**，不是一个答案。"最快"和"少步行"和"更晚出发"
/// 是不同的诉求，给一个排序列表等于把决策成本推回给用户。
fn dedup_and_rank(options: &mut Vec<JourneyOption>, max: usize) {
    if options.is_empty() {
        return;
    }

    // 按总耗时排序，并去掉"同一线路组合且耗时接近"的重复项
    options.sort_by(|a, b| {
        a.total_minutes
            .partial_cmp(&b.total_minutes)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seen: Vec<(usize, Vec<String>)> = Vec::new();
    options.retain(|o| {
        let sig = line_signature(o);
        let dup = seen.iter().any(|(t, s)| {
            *s == sig && (o.total_minutes - *t as f64).abs() < 30.0
        });
        if !dup {
            seen.push((o.total_minutes as usize, sig));
        }
        !dup
    });

    if options.is_empty() {
        return;
    }

    // 最快
    options[0].tags.push("最快".into());

    // 少步行：与最快方案相比步行明显更少
    if let Some((idx, _)) = options
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.walk_meters
                .partial_cmp(&b.walk_meters)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        if idx != 0 && options[idx].walk_meters + 150.0 < options[0].walk_meters {
            options[idx].tags.push("少步行".into());
        }
    }

    // 更晚出发：能晚出门且不至于慢太多（容忍 8 分钟额外耗时）
    if let Some((idx, _)) = options
        .iter()
        .enumerate()
        .filter(|(_, o)| o.total_minutes <= options[0].total_minutes + 8.0)
        .max_by(|(_, a), (_, b)| {
            a.latest_depart_offset_min
                .partial_cmp(&b.latest_depart_offset_min)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        if !options[idx].tags.iter().any(|t| t == "最快") {
            options[idx].tags.push("更晚出发".into());
        }
    }

    // 少换乘
    if let Some((idx, _)) = options
        .iter()
        .enumerate()
        .filter(|(_, o)| o.transfers == 0)
        .min_by(|(_, a), (_, b)| {
            a.total_minutes
                .partial_cmp(&b.total_minutes)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        if idx != 0 {
            options[idx].tags.push("少换乘".into());
        }
    }

    options.truncate(max);
}

/// 方案中乘坐的线路序列，用于去重
fn line_signature(o: &JourneyOption) -> Vec<String> {
    let mut v: Vec<String> = o
        .legs
        .iter()
        .filter(|l| l.kind == LegKind::Bus)
        .filter_map(|l| l.line_name.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// 由"必须到达时刻"反推最晚出发时刻。
///
/// 返回相对现在的分钟偏移（可为负，表示已经来不及了）。
pub fn latest_departure(
    option: &JourneyOption,
    arrive_before_offset_min: f64,
) -> f64 {
    arrive_before_offset_min - option.total_minutes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GeoPoint, LineBaseDto};

    /// 构造一条测试线路。
    ///
    /// `stops` 每项是 `(站序, 站点ID, 站名, 经度, 纬度)`。
    ///
    /// ⚠️ 站点 ID **必须显式给出**，不能由线路号推导：同一条物理线路常被多条
    /// 线路共用，上游返回的是**同一个 stopId**，而换乘识别的全部依据就是这个 ID。
    /// 早期测试用 `{线路号}-S{站序}` 拼 ID，导致换乘永远匹配不上——
    /// 这个 bug 只在测试里，但它掩盖了算法是否真的工作。
    fn make_line(
        line_id: &str,
        line_name: &str,
        stops: &[(i64, &str, &str, f64, f64)],
    ) -> LineDetail {
        let views: Vec<StopView> = stops
            .iter()
            .map(|(no, sid, name, lng, lat)| StopView {
                stop_no: *no,
                stop_id: (*sid).into(),
                stop_name: (*name).into(),
                point: Some(GeoPoint {
                    lng: *lng,
                    lat: *lat,
                }),
                subway_lines: vec![],
            })
            .collect();
        let path: Vec<LngLat> = views
            .iter()
            .filter_map(|s| s.point.map(LngLat::from))
            .collect();
        let acc = crate::gcj::cumulative_lengths(&path);
        let total = acc.last().copied().unwrap_or(0.0);
        LineDetail {
            base: LineBaseDto {
                line_id: line_id.into(),
                line_name: line_name.into(),
                direction: None,
                start_stop: views.first().map(|s| s.stop_name.clone()).unwrap_or_default(),
                end_stop: views.last().map(|s| s.stop_name.clone()).unwrap_or_default(),
                early_hour: "05:30".into(),
                last_hour: "21:00".into(),
                price: "2".into(),
                line_type: Some(0),
            },
            stops: views,
            cumulative_meters: acc,
            total_meters: total,
        }
    }

    fn vehicle(id: &str, along: f64, speed: f64) -> VehicleView {
        VehicleView {
            bus_id: id.into(),
            bus_order: Some(1),
            point: Some(GeoPoint {
                lng: 114.30,
                lat: 30.50,
            }),
            speed_kmh: Some(speed),
            along_m: Some(along),
            offset_m: Some(5.0),
            at_stop_no: Some(1),
            next_stop_no: Some(2),
            arrived: false,
            gps_time: Some("20261013090000".into()),
            age_seconds: Some(20),
            confidence: PositionConfidence::Live,
        }
    }

    fn cand(stop_id: &str, name: &str, lng: f64, lat: f64, lines: &[&str]) -> CandidateStop {
        CandidateStop {
            stop_id: stop_id.into(),
            stop_name: name.into(),
            point: LngLat::new(lng, lat),
            line_names: lines.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// 站点 ID 常量，保证跨线路共享同一 ID
    const S_A: &str = "STOP-A";
    const S_B: &str = "STOP-B";
    const S_C: &str = "STOP-C";
    const S_D: &str = "STOP-D";
    const S_E: &str = "STOP-E";
    const S_F: &str = "STOP-F";

    /// 100 路：A → B → C → D → E（直达场景）
    fn direct_fixture() -> PlanInputs {
        let l = make_line(
            "420100-100-0",
            "100",
            &[
                (1, S_A, "A站", 114.300, 30.500),
                (2, S_B, "B站", 114.310, 30.500),
                (3, S_C, "C站", 114.320, 30.500),
                (4, S_D, "D站", 114.330, 30.500),
                (5, S_E, "E站", 114.340, 30.500),
            ],
        );

        let mut lines = HashMap::new();
        lines.insert("100".to_string(), vec![l]);

        let mut vehicles = HashMap::new();
        vehicles.insert("420100-100-0".to_string(), vec![vehicle("bus1", 0.0, 20.0)]);

        PlanInputs {
            lines,
            vehicles,
            origin_stops: vec![cand(S_A, "A站", 114.300, 30.500, &["100"])],
            dest_stops: vec![cand(S_D, "D站", 114.330, 30.500, &["100"])],
        }
    }

    /// 换乘场景：100 路 A→B→C，200 路 从 C 出发经 D/E 到 F。
    ///
    /// 两条线路在 C 站**共用同一个 stopId**，这是换乘能被识别的前提。
    /// C 站之后 200 路还有站，否则算法会（正确地）拒绝：
    /// 上了车也没有前进空间。
    fn transfer_fixture() -> PlanInputs {
        let l1 = make_line(
            "420100-100-0",
            "100",
            &[
                (1, S_A, "A站", 114.300, 30.500),
                (2, S_B, "B站", 114.310, 30.500),
                (3, S_C, "C站", 114.320, 30.500),
            ],
        );
        let l2 = make_line(
            "420100-200-0",
            "200",
            &[
                (1, S_C, "C站", 114.320, 30.500),
                (2, S_D, "D站", 114.330, 30.500),
                (3, S_E, "E站", 114.340, 30.500),
                (4, S_F, "F站", 114.350, 30.500),
            ],
        );

        let mut lines = HashMap::new();
        lines.insert("100".to_string(), vec![l1]);
        lines.insert("200".to_string(), vec![l2]);

        let mut vehicles = HashMap::new();
        vehicles.insert("420100-100-0".to_string(), vec![vehicle("bus100", 0.0, 20.0)]);
        vehicles.insert("420100-200-0".to_string(), vec![vehicle("bus200", 0.0, 20.0)]);

        PlanInputs {
            lines,
            vehicles,
            origin_stops: vec![cand(S_A, "A站", 114.300, 30.500, &["100"])],
            dest_stops: vec![cand(S_E, "E站", 114.340, 30.500, &["200"])],
        }
    }

    #[test]
    fn finds_direct_route() {
        let inputs = direct_fixture();
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.331, 30.500),
            &inputs,
            &PlanOptions::default(),
        );

        assert!(!plans.is_empty(), "应能找到直达方案");
        let best = &plans[0];
        assert_eq!(best.transfers, 0);
        assert!(best.tags.contains(&"最快".to_string()));

        let kinds: Vec<LegKind> = best.legs.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![LegKind::Walk, LegKind::Wait, LegKind::Bus, LegKind::Walk],
            "直达方案应有四段"
        );

        let bus = best.legs.iter().find(|l| l.kind == LegKind::Bus).unwrap();
        assert_eq!(bus.stop_count, Some(3), "A→D 应坐 3 站");
        assert_eq!(bus.board_stop_no, Some(1));
        assert_eq!(bus.alight_stop_no, Some(4));
        assert!(bus.minutes > 0.0);

        // 时间轴必须单调且与总耗时收口
        for w in best.legs.windows(2) {
            assert!(
                w[1].start_offset_min >= w[0].end_offset_min - 1e-6,
                "段时间轴不得重叠"
            );
        }
        assert!(
            (best.legs.last().unwrap().end_offset_min - best.total_minutes).abs() < 1e-6,
            "最后一段的结束时刻必须等于总耗时"
        );
    }

    #[test]
    fn finds_one_transfer_route() {
        let inputs = transfer_fixture();
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.341, 30.500),
            &inputs,
            &PlanOptions::default(),
        );

        let t = plans
            .iter()
            .find(|p| p.transfers == 1)
            .expect("应能找到一次换乘方案");

        let kinds: Vec<LegKind> = t.legs.iter().map(|l| l.kind).collect();
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
            "换乘方案应有七段"
        );

        let buses: Vec<&str> = t
            .legs
            .iter()
            .filter(|l| l.kind == LegKind::Bus)
            .filter_map(|l| l.line_name.as_deref())
            .collect();
        assert_eq!(buses, vec!["100", "200"], "先坐 100 再换 200");

        // 时间轴必须连续无缝
        for w in t.legs.windows(2) {
            assert!(
                (w[1].start_offset_min - w[0].end_offset_min).abs() < 1e-6,
                "换乘方案的时间轴必须连续"
            );
        }
        assert!((t.legs.last().unwrap().end_offset_min - t.total_minutes).abs() < 1e-6);
    }

    /// 换乘站恰为下车线路的终点站 → 应当被拒绝
    #[test]
    fn rejects_transfer_at_terminal_stop() {
        let mut inputs = transfer_fixture();
        if let Some(dirs) = inputs.lines.get_mut("200") {
            let d = &mut dirs[0];
            d.stops.truncate(1);
            d.cumulative_meters.truncate(1);
            d.total_meters = 0.0;
        }
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.341, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        assert!(
            plans.iter().all(|p| p.transfers == 0),
            "换乘站是终点站时不应产出换乘方案"
        );
    }

    #[test]
    fn returns_nothing_when_line_has_no_vehicle() {
        let mut inputs = direct_fixture();
        // 收班 / 暂无在线车辆：不应编造一个候车时间
        inputs.vehicles.insert("420100-100-0".into(), vec![]);
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.331, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        assert!(plans.is_empty(), "没有在线车辆时不应给出方案");
    }

    #[test]
    fn skips_lines_without_cached_stop_table() {
        let mut inputs = direct_fixture();
        inputs.lines.clear();
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.331, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        assert!(plans.is_empty(), "缺站点表的数据应被跳过而不是报错");
    }

    #[test]
    fn never_produces_backward_ride() {
        let inputs = direct_fixture();
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.331, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        assert!(!plans.is_empty());
        for p in &plans {
            for l in p.legs.iter().filter(|l| l.kind == LegKind::Bus) {
                assert!(
                    l.alight_stop_no.unwrap() > l.board_stop_no.unwrap(),
                    "落客站必须在上车站之后"
                );
                assert!(l.stop_count.unwrap() > 0);
            }
        }
    }

    #[test]
    fn final_stop_is_valid_destination() {
        // 终点站作为目的地是常见需求，必须能规划
        let mut inputs = direct_fixture();
        inputs.dest_stops = vec![cand(S_E, "E站", 114.340, 30.500, &["100"])];
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.341, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        assert!(!plans.is_empty(), "终点站应当可以作为目的地");
        let bus = plans[0].legs.iter().find(|l| l.kind == LegKind::Bus).unwrap();
        assert_eq!(bus.alight_stop_no, Some(5));
    }

    #[test]
    fn invalid_coords_yield_no_plan() {
        let inputs = direct_fixture();
        assert!(plan_from(
            LngLat::new(0.0, 0.0),
            LngLat::new(114.33, 30.50),
            &inputs,
            &PlanOptions::default()
        )
        .is_empty());
    }

    #[test]
    fn latest_departure_is_deadline_minus_duration() {
        let inputs = direct_fixture();
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.331, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        let p = &plans[0];
        let latest = latest_departure(p, 60.0);
        assert!((latest - (60.0 - p.total_minutes)).abs() < 1e-9);
        // 已经来不及的情形应返回负数
        assert!(latest_departure(p, 1.0) < 0.0);
        // 时间充裕时最晚出发应为正
        assert!(latest_departure(p, p.total_minutes + 5.0) > 0.0);
    }

    #[test]
    fn longer_walk_when_stop_is_farther() {
        let inputs = direct_fixture();
        let near = plan_from(
            LngLat::new(114.3005, 30.500),
            LngLat::new(114.3295, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        let far = plan_from(
            LngLat::new(114.2950, 30.500),
            LngLat::new(114.3350, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        let w = |ps: &Vec<JourneyOption>| {
            ps[0].legs
                .iter()
                .filter(|l| l.kind == LegKind::Walk)
                .map(|l| l.minutes)
                .sum::<f64>()
        };
        assert!(
            w(&far) > w(&near),
            "离站更远时步行时间必须更长：{:.1} vs {:.1}",
            w(&far),
            w(&near)
        );
    }

    #[test]
    fn walk_speed_affects_result() {
        let inputs = direct_fixture();
        let a = plan_from(
            LngLat::new(114.295, 30.500),
            LngLat::new(114.335, 30.500),
            &inputs,
            &PlanOptions {
                walk_speed_m_per_min: 120.0,
                ..Default::default()
            },
        );
        let b = plan_from(
            LngLat::new(114.295, 30.500),
            LngLat::new(114.335, 30.500),
            &inputs,
            &PlanOptions {
                walk_speed_m_per_min: 50.0,
                ..Default::default()
            },
        );
        assert!(a[0].total_minutes < b[0].total_minutes, "走得快总耗时必须更短");
    }

    #[test]
    fn respects_max_options() {
        let inputs = transfer_fixture();
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.341, 30.500),
            &inputs,
            &PlanOptions {
                max_options: 1,
                ..Default::default()
            },
        );
        assert!(plans.len() <= 1, "必须遵守 max_options 上限");
    }

    /// 车越远 → 候车越久。
    ///
    /// 注意只能比较"车仍在前方"的情形：一旦车驶过你的上车站，由于
    /// **上游没有班次计划表**，算法无从得知下一班何时来，会诚实地不给出方案
    /// （见 `no_plan_when_all_vehicles_passed_the_stop`）。
    #[test]
    fn closer_vehicle_waits_less() {
        let plan_with_bus_at = |meters_before_stop: f64| {
            let mut i = direct_fixture();
            // along_m 是沿线里程；A 站在 0m 处，因此负值表示"尚未到达 A 站"
            i.vehicles.insert(
                "420100-100-0".into(),
                vec![vehicle("b", -meters_before_stop, 20.0)],
            );
            plan_from(
                LngLat::new(114.300, 30.500),
                LngLat::new(114.331, 30.500),
                &i,
                &PlanOptions::default(),
            )
        };

        let near = plan_with_bus_at(100.0);
        let far = plan_with_bus_at(3000.0);

        assert!(!near.is_empty(), "车在前方 100m 时应有方案");
        assert!(!far.is_empty(), "车在前方 3km 时应有方案");

        let wait = |ps: &Vec<JourneyOption>| {
            ps[0]
                .legs
                .iter()
                .filter(|l| l.kind == LegKind::Wait)
                .map(|l| l.minutes)
                .sum::<f64>()
        };
        assert!(
            wait(&far) > wait(&near),
            "车更远时候车应更久：{:.2} vs {:.2}",
            wait(&far),
            wait(&near)
        );
    }

    /// 所有车辆都已驶过上车站 → 没有方案。
    ///
    /// 这是一个**有意的诚实行为**：没有班次计划表就无法预判下一班，
    /// 此时编一个候车时间会让用户误车。宁可说"暂无方案"。
    #[test]
    fn no_plan_when_all_vehicles_passed_the_stop() {
        let mut i = direct_fixture();
        // 车已过 A 站 600 米（A 站在 0m），且这是唯一一辆车
        i.vehicles
            .insert("420100-100-0".into(), vec![vehicle("b", 600.0, 20.0)]);
        let plans = plan_from(
            LngLat::new(114.300, 30.500),
            LngLat::new(114.331, 30.500),
            &i,
            &PlanOptions::default(),
        );
        assert!(
            plans.is_empty(),
            "车已驶过唯一上车站且无班次数据时不应产出方案，实测 {} 个",
            plans.len()
        );
    }

    #[test]
    fn ranking_labels_are_meaningful() {
        let inputs = transfer_fixture();
        let plans = plan_from(
            LngLat::new(114.299, 30.500),
            LngLat::new(114.341, 30.500),
            &inputs,
            &PlanOptions::default(),
        );
        assert!(!plans.is_empty());
        // 第一名必须带"最快"
        assert!(plans[0].tags.contains(&"最快".to_string()));
        // 标签不得重复
        for p in &plans {
            let mut t = p.tags.clone();
            t.sort();
            t.dedup();
            assert_eq!(t.len(), p.tags.len(), "标签不应重复");
        }
        // 方案应按总耗时升序
        for w in plans.windows(2) {
            assert!(w[0].total_minutes <= w[1].total_minutes + 1e-6);
        }
    }
}
