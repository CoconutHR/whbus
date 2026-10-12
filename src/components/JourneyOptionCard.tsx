import { Button, Card, Chip, FreshnessChip } from "./ui";
import {
  IconBus,
  IconChevron,
  IconClock,
  IconTransfer,
  IconWalk,
} from "./icons";
import type { JourneyLeg, JourneyOption } from "../lib/types";
import { formatClock, formatDistance, lineColorVar } from "../lib/utils";
import { formatMinutes } from "../lib/utils";
import "./JourneyOptionCard.css";

/**
 * 出行方案卡片。
 *
 * # 设计要点（来自已定稿规格）
 *
 * 1. **结论前置**：最上面一行直接给"最晚 X 出发 → Y 到达"，
 *    而不是让用户自己把各段加起来。
 * 2. **标签给决策依据**：最快 / 少步行 / 更晚出发 / 少换乘。
 *    用户要的是"为什么选它"，不是一个排序列表。
 * 3. **每段都带可信度**：步行时间是确定值，候车时间是概率——
 *    UI 必须能区分，否则用户会把推测当成实测。
 */
export function JourneyOptionCard({
  option,
  baseTime,
  rank,
  onSelect,
  expanded,
}: {
  option: JourneyOption;
  /** 计算绝对时刻的基准（点"规划"的那一刻） */
  baseTime: Date;
  rank: number;
  onSelect?: () => void;
  expanded?: boolean;
}) {
  const departOffset = option.latestDepartOffsetMin;
  const arriveOffset = option.arriveOffsetMin;

  // 最差的一段可信度决定整卡片的可信度提示
  const worst = worstConfidence(option.legs);
  const inferred = option.legs.some(
    (l) => l.kind === "bus" && l.confidence === "inferred",
  );

  const busLegs = option.legs.filter((l) => l.kind === "bus");

  return (
    <Card raised={rank === 0} className="joc">
      {/* 顶行：标签 */}
      <div className="joc__tags">
        {option.tags.map((t) => (
          <Chip key={t} tone={t === "最快" ? "brand" : "neutral"}>
            {t}
          </Chip>
        ))}
        <span className="grow" />
        {inferred ? <Chip tone="warn">含推测</Chip> : null}
        <FreshnessChip confidence={worst} />
      </div>

      {/* 结论行：最晚出发 → 到达 */}
      <div className="joc__headline">
        <div className="joc__time">
          <span className="joc__time-label t-cap t-3">
            {departOffset > 0.5 ? "最晚出发" : "现在出发"}
          </span>
          <span className="joc__clock tnum">
            {formatClock(baseTime, Math.max(0, departOffset))}
          </span>
        </div>
        <div className="joc__arrow" aria-hidden>
          <IconChevron size={16} />
        </div>
        <div className="joc__time">
          <span className="joc__time-label t-cap t-3">预计到达</span>
          <span className="joc__clock joc__clock--arrive tnum">
            {formatClock(baseTime, arriveOffset)}
          </span>
        </div>
      </div>

      {/* 摘要行 */}
      <div className="joc__summary t-label t-2">
        <span className="row gap-1">
          <IconClock size={13} />
          全程 {formatMinutes(option.totalMinutes)}
        </span>
        <span className="row gap-1">
          <IconWalk size={13} />
          步行 {formatDistance(option.walkMeters)}
        </span>
        <span className="row gap-1">
          <IconTransfer size={13} />
          {option.transfers === 0 ? "直达" : `换乘 ${option.transfers} 次`}
        </span>
      </div>

      {/* 线路序列 */}
      <div className="joc__lines">
        {busLegs.map((l, i) => (
          <span key={i} className="row gap-2">
            {i > 0 ? <IconTransfer size={13} className="t-3" /> : null}
            <span className="line-badge">
              <span
                className="line-badge__dot"
                style={{ background: lineColorVar(l.lineName ?? "?") }}
                aria-hidden
              />
              {l.lineName}
            </span>
            <span className="t-cap t-3">
              {l.stopCount} 站
            </span>
          </span>
        ))}
      </div>

      {/* 展开的时间轴 */}
      {expanded ? <LegTimeline legs={option.legs} baseTime={baseTime} /> : null}

      {onSelect ? (
        <div className="joc__action">
          <Button variant="ghost" size="sm" onClick={onSelect}>
            {expanded ? "收起详情" : "查看每一段"}
          </Button>
        </div>
      ) : null}
    </Card>
  );
}

/** 逐段时间轴 */
function LegTimeline({ legs, baseTime }: { legs: JourneyLeg[]; baseTime: Date }) {
  return (
    <ol className="legs">
      {legs.map((l, i) => (
        <li key={i} className={`legs__item legs__item--${l.kind}`}>
          <div className="legs__rail" aria-hidden>
            <span className={`legs__dot legs__dot--${l.kind}`} />
            {i < legs.length - 1 ? <span className="legs__line" /> : null}
          </div>

          <div className="legs__body">
            <div className="row gap-2">
              <span className="t-label">{legTitle(l)}</span>
              <span className="grow" />
              <span className="t-cap t-3 tnum">
                {formatClock(baseTime, l.startOffsetMin)}
              </span>
            </div>

            {l.kind === "bus" && l.lineName ? (
              <div className="row gap-2" style={{ marginTop: 2 }}>
                <span className="line-badge">
                  <span
                    className="line-badge__dot"
                    style={{ background: lineColorVar(l.lineName) }}
                    aria-hidden
                  />
                  {l.lineName}
                </span>
                <span className="t-cap t-2 truncate">
                  {l.fromName} → {l.toName}
                </span>
              </div>
            ) : (
              <div className="t-cap t-3 truncate" style={{ marginTop: 2 }}>
                {l.fromName === l.toName
                  ? l.fromName
                  : `${l.fromName} → ${l.toName}`}
              </div>
            )}

            <div className="t-cap t-3" style={{ marginTop: 2 }}>
              {formatMinutes(l.minutes)}
              {l.meters != null && l.kind === "walk" ? ` · ${formatDistance(l.meters)}` : ""}
              {l.stopCount != null && l.kind === "bus" ? ` · ${l.stopCount} 站` : ""}
              {l.kind === "bus" && l.confidence !== "live" ? " · 时间含推测" : ""}
            </div>
          </div>
        </li>
      ))}
    </ol>
  );
}

function legTitle(l: JourneyLeg): string {
  switch (l.kind) {
    case "walk":
      return "步行";
    case "wait":
      return l.vehicle ? `等 ${l.lineName}` : "候车";
    case "bus":
      return "乘车";
    case "transfer":
      return "换乘步行";
  }
}

/**
 * 取方案中最差的一段可信度。
 *
 * 用"最差"而不是"平均"：只要有一段是推测，整个方案的到达时刻就不可靠，
 * 用户需要知道这一点。
 */
function worstConfidence(legs: JourneyLeg[]): "live" | "delayed" | "inferred" | "stale" {
  const order = { live: 0, delayed: 1, inferred: 2, stale: 3 } as const;
  let worst: keyof typeof order = "live";
  for (const l of legs) {
    if (order[l.confidence] > order[worst]) worst = l.confidence;
  }
  return worst;
}

/** 线路图标（供外部复用） */
export { IconBus };
