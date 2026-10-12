import { Button, Card, Chip, FreshnessChip, StateView } from "./ui";
import {
  IconAlert,
  IconBus,
  IconClock,
  IconInfo,
  IconTransfer,
  IconWalk,
} from "./icons";
import { MiniTimeline } from "./MiniTimeline";
import type { JourneyLeg, JourneyOption } from "../lib/types";
import { formatClock, formatDistance, formatMinutes, lineColorVar } from "../lib/utils";
import "./DecisionCard.css";

/**
 * 情景决策卡 —— 首页的头号卡片，也是整个 App 的核心价值所在。
 *
 * # 回答的问题
 *
 * 「我**最晚几点**必须出发，**预计几点**到家，路上怎么走。」
 *
 * # 设计要点
 *
 * 1. **到达时刻是主角**：34px 大字。用户关心的是"几点到家"，
 *    不是"有几条路线"。
 * 2. **最晚出发时刻紧随其后**：这是"要不要现在起身"的直接依据。
 * 3. 方案拆成步行 / 候车 / 乘车 / 换乘四类，用微型时间轴 + 分段数字表达。
 * 4. **可信度诚实标注**：只有一段是推测也要说出来，因为到达时刻是整条链
 *    累积的结果，任何一段不可靠都会影响结论。
 * 5. 给不出方案时**解释原因**，而不是只显示"无结果"——上游没有班次计划表，
 *    "没有在线车辆就真的算不出来"这件事必须让用户知道，否则会被当成 bug。
 */
export function DecisionCard({
  directionLabel,
  fromLabel,
  toLabel,
  plan,
  loading,
  error,
  computedAt,
  stale,
  onRefresh,
  onAccept,
  onOpenPlan,
  compact,
}: {
  /** 如"回家" / "去公司" */
  directionLabel: string;
  fromLabel: string;
  toLabel: string;
  plan: JourneyOption | null;
  loading: boolean;
  error: string | null;
  computedAt: Date | null;
  stale: boolean;
  onRefresh: () => void;
  onAccept: () => void;
  onOpenPlan?: () => void;
  /** 紧凑模式：只显示到达时刻与最晚出发 */
  compact?: boolean;
}) {
  const base = computedAt ?? new Date();

  return (
    <Card raised className="dc">
      {/* 顶行：方向 + 状态 */}
      <div className="dc__top">
        <span className="row gap-2">
          <IconClock size={15} className="t-2" />
          <span className="t-label t-2">
            {directionLabel} · 从{fromLabel}出发
          </span>
        </span>
        <span className="grow" />
        {loading ? <Chip tone="neutral">计算中</Chip> : null}
        {!loading && stale ? (
          <button type="button" className="dc__refresh" onClick={onRefresh}>
            刷新
          </button>
        ) : null}
        {!loading && plan ? <FreshnessChip confidence={worstConfidence(plan.legs)} /> : null}
      </div>

      {/* 主体 */}
      {loading && !plan ? (
        <div className="dc__loading">
          <div className="skeleton" style={{ height: 38, width: "52%", marginBottom: 10 }} />
          <div className="skeleton" style={{ height: 16, width: "76%" }} />
        </div>
      ) : error ? (
        <div className="dc__state">
          <StateView kind="offline" title="暂时算不出方案" detail={error} />
        </div>
      ) : !plan ? (
        <div className="dc__state">
          <StateView
            kind="info"
            title="暂时给不出方案"
            detail={
              <>
                可能是「{fromLabel}」或「{toLabel}」周边 400 米内没有查到公交站，
                <br />
                也可能相关线路当前没有在线车辆。
                <br />
                <br />
                上游<b>没有班次计划表</b>，所以没有在线车辆时确实无法预判下一班何时来。
                与其编一个时间让你误车，这里选择如实告诉你。
              </>
            }
            action={
              <div className="row gap-2" style={{ marginTop: "var(--space-2)" }}>
                <Button variant="ghost" size="sm" onClick={onRefresh}>
                  重新计算
                </Button>
                <Button variant="quiet" size="sm" onClick={onOpenPlan}>
                  手动规划
                </Button>
              </div>
            }
          />
        </div>
      ) : (
        <>
          {/* 到达时刻：视觉主角 */}
          <div className="dc__arrive">
            <span className="dc__arrive-time tnum">
              {formatClock(base, plan.arriveOffsetMin)}
            </span>
            <span className="dc__arrive-label t-label t-2">预计到达</span>
          </div>

          {/* 最晚出发 + 全程耗时 */}
          <div className="dc__meta">
            <span className="dc__depart">
              <span className="t-cap t-3">最晚出发</span>
              <span className="dc__depart-time tnum">
                {formatClock(base, Math.max(0, plan.latestDepartOffsetMin))}
              </span>
            </span>
            <span className="dc__sep" aria-hidden />
            <span className="dc__stat t-cap t-2">
              全程 {formatMinutes(plan.totalMinutes)}
            </span>
            <span className="dc__stat t-cap t-2">步行 {formatDistance(plan.walkMeters)}</span>
            <span className="dc__stat t-cap t-2">
              {plan.transfers === 0 ? "直达" : `换乘 ${plan.transfers} 次`}
            </span>
          </div>

          {/* 分段构成：步行 / 候车 / 乘车 / 换乘 */}
          {!compact ? <LegBreakdown legs={plan.legs} base={base} /> : null}

          {/* 乘坐的线路 */}
          <div className="dc__lines">
            {plan.legs
              .filter((l) => l.kind === "bus" && l.lineName)
              .map((l, i) => (
                <span key={i} className="row gap-2">
                  {i > 0 ? <IconTransfer size={13} className="t-3" /> : null}
                  <span className="line-badge">
                    <span
                      className="line-badge__dot"
                      style={{ background: lineColorVar(l.lineName as string) }}
                      aria-hidden
                    />
                    {l.lineName}
                  </span>
                  <span className="t-cap t-3">{l.stopCount} 站</span>
                </span>
              ))}
          </div>

          {/* 建议时刻的容错提示 */}
          {plan.latestDepartOffsetMin <= 0 ? (
            <div className="dc__warning">
              <IconAlert size={14} />
              <span>
                现在出发刚刚好，再晚可能赶不上这班车。
              </span>
            </div>
          ) : plan.latestDepartOffsetMin <= 5 ? (
            <div className="dc__warning">
              <IconAlert size={14} />
              <span>建议在 {Math.max(1, Math.round(plan.latestDepartOffsetMin))} 分钟内出发。</span>
            </div>
          ) : null}

          {/* CTA */}
          <div className="dc__actions">
            <Button variant="accent" block onClick={onAccept}>
              开始导航
            </Button>
            <Button variant="quiet" size="sm" onClick={onOpenPlan}>
              其他方案
            </Button>
          </div>
        </>
      )}

      {computedAt ? (
        <div className="dc__foot t-cap t-3">
          <IconInfo size={11} />
          {stale ? "结果可能已过期，建议刷新" : "基于实时车辆位置计算"} ·{" "}
          {computedAt.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}
        </div>
      ) : null}
    </Card>
  );
}

/** 步行 → 候车 → 乘车 → 换乘的分段构成 */
function LegBreakdown({ legs, base }: { legs: JourneyLeg[]; base: Date }) {
  const walk = legs.filter((l) => l.kind === "walk");
  const waits = legs.filter((l) => l.kind === "wait");
  const rides = legs.filter((l) => l.kind === "bus");
  const transfers = legs.filter((l) => l.kind === "transfer");

  const sum = (ls: JourneyLeg[]) => ls.reduce((a, b) => a + b.minutes, 0);

  const parts: Array<{ label: string; minutes: number; kind: string }> = [];
  if (walk.length) parts.push({ label: "步行", minutes: sum(walk), kind: "walk" });
  if (waits.length) parts.push({ label: "候车", minutes: sum(waits), kind: "wait" });
  if (rides.length) parts.push({ label: "乘车", minutes: sum(rides), kind: "bus" });
  if (transfers.length) parts.push({ label: "换乘", minutes: sum(transfers), kind: "transfer" });

  const total = parts.reduce((a, b) => a + b.minutes, 0) || 1;

  return (
    <div className="dc__breakdown">
      {/* 比例条：一眼看出时间花在哪 */}
      <div className="dc__bar" aria-hidden>
        {parts.map((p, i) => (
          <span
            key={i}
            className={`dc__bar-seg dc__bar-seg--${p.kind}`}
            style={{ flexGrow: Math.max(0.02, p.minutes / total) }}
          />
        ))}
      </div>

      <div className="dc__parts">
        {parts.map((p, i) => (
          <span key={i} className="dc__part">
            {p.kind === "walk" ? (
              <IconWalk size={12} />
            ) : p.kind === "bus" ? (
              <IconBus size={12} />
            ) : p.kind === "transfer" ? (
              <IconTransfer size={12} />
            ) : (
              <IconClock size={12} />
            )}
            <span className="t-cap">
              {p.label} {Math.round(p.minutes)} 分
            </span>
          </span>
        ))}
        <span className="grow" />
        <span className="t-cap t-3 tnum">
          {formatClock(base, 0)} → {formatClock(base, total)}
        </span>
      </div>

      {/* 微型时间轴：车辆位置与你的位置 */}
      <div className="dc__timeline">
        <MiniTimeline
          stops={buildTimelineStops(legs)}
          totalMeters={totalMeters(legs)}
          vehicles={buildTimelineVehicles(legs)}
          variant="compact"
        />
      </div>
    </div>
  );
}

/** 把方案里的乘车站点串成时间轴刻度 */
function buildTimelineStops(legs: JourneyLeg[]) {
  const out: Array<{ stopNo: number; stopName: string; at: number }> = [];
  let acc = 0;
  for (const l of legs) {
    if (l.kind === "bus" && l.meters) {
      acc += l.meters;
      out.push({
        stopNo: l.boardStopNo ?? out.length + 1,
        stopName: l.fromName,
        at: acc - l.meters,
      });
      out.push({
        stopNo: l.alightStopNo ?? out.length + 2,
        stopName: l.toName,
        at: acc,
      });
    }
  }
  return out.length >= 2
    ? out
    : [
        { stopNo: 1, stopName: "起点", at: 0 },
        { stopNo: 2, stopName: "终点", at: 1000 },
      ];
}

function totalMeters(legs: JourneyLeg[]): number {
  const last = buildTimelineStops(legs).at(-1);
  return last ? last.at : 1000;
}

/** 方案里已知的车辆位置 */
function buildTimelineVehicles(legs: JourneyLeg[]) {
  return legs
    .filter((l) => l.kind === "bus" && l.vehicle?.alongM != null && !l.vehicle.arrived)
    .map((l) => ({
      along: l.vehicle!.alongM as number,
      confidence: l.vehicle!.confidence,
    }));
}

/**
 * 取方案中最差的一段可信度。
 *
 * 用"最差"而不是"平均"：到达时刻是整条链累积的结果，
 * 只要有一段是推测，结论的可靠性就会下降，用户需要知道。
 */
function worstConfidence(legs: JourneyLeg[]): "live" | "delayed" | "inferred" | "stale" {
  const order = { live: 0, delayed: 1, inferred: 2, stale: 3 } as const;
  let worst: keyof typeof order = "live";
  for (const l of legs) {
    if (order[l.confidence] > order[worst]) worst = l.confidence;
  }
  return worst;
}
