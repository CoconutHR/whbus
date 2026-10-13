import { MiniTimeline, type TimelineVehicle } from "./MiniTimeline";
import { Chip, FreshnessChip } from "./ui";
import { IconBus, IconMoon } from "./icons";
import { useLineRealtime } from "../lib/polling";
import { formatAge, formatEtaRange, headwayText, lineColorVar, motionBadge } from "../lib/utils";
import "./LineRealtimeCard.css";

/**
 * 首页的线路实时卡片。
 *
 * 信息层级（从上到下最重要）：
 * 1. **还有多久** —— 用户打开 App 唯一想知道的事
 * 2. 线路号与方向 —— 确认没看错车
 * 3. 车辆在时间轴上的位置 —— 比数字更有实感
 * 4. 第二班 —— 用来判断"赶不上要不要等"
 *
 * 左侧 3px 色条是线路色的唯一大面积用法；线路号本身用中性色，
 * 这样黄色线路也不会出现"白字配黄底"。
 */
export function LineRealtimeCard({
  lineId,
  lineName,
  startStop,
  endStop,
  onClick,
  boardStopId,
}: {
  lineId: string;
  lineName: string;
  startStop: string;
  endStop: string;
  onClick?: () => void;
  /** 用户指定的上车站；不传则取整条线路上最快到的那班 */
  boardStopId?: string | null;
}) {
  const { data, loading, error } = useLineRealtime(lineId);

  const color = lineColorVar(lineName);

  // 选定"关注站"：优先用户设置的，否则用最快到站的那班对应的站
  const focus =
    (boardStopId && data?.arrivals.find((a) => a.stopId === boardStopId)) ||
    data?.arrivals[0] ||
    null;

  const next = data?.arrivals.filter((a) => a.stopId !== focus?.stopId)[0] ?? null;

  const timelineVehicles: TimelineVehicle[] =
    data?.vehicles
      .filter((v) => v.alongM != null && !v.arrived)
      .map((v) => ({ along: v.alongM as number, confidence: v.confidence })) ?? [];

  const focusAt = focus
    ? (data?.cumulativeMeters[data.stops.findIndex((s) => s.stopId === focus.stopId)] ?? undefined)
    : undefined;

  const hasInferred = data?.vehicles.some((v) => v.confidence === "inferred") ?? false;

  return (
    <button type="button" className="lrc" onClick={onClick}>
      <span className="lrc__bar" style={{ background: color }} aria-hidden />

      <span className="lrc__body">
        {/* 第一行：线路号 + 方向 + 状态 */}
        <span className="lrc__row1">
          <span className="lrc__no">{lineName}</span>
          <span className="t-cap t-2 truncate">
            往 {endStop}
          </span>
          <span className="grow" />
          {loading && !data ? (
            <span className="t-cap t-3">加载中</span>
          ) : error ? (
            <span className="t-cap" style={{ color: "var(--danger)" }}>
              读取失败
            </span>
          ) : data ? (
            <FreshnessChip
              confidence={
                focus?.confidence ??
                (data.vehicles[0]?.confidence ?? "stale")
              }
              ageSeconds={data.vehicles[0]?.ageSeconds ?? null}
            />
          ) : null}
        </span>

        {/* 第二行：核心数字 */}
        {data && !data.inService ? (
          <span className="lrc__row2">
            <span className="row gap-2" style={{ color: "var(--text-2)" }}>
              <IconMoon size={16} />
              <span className="t-head">今日已收班</span>
              <span className="t-cap t-3">
                运营 {data.earlyHour}–{data.lastHour}
              </span>
            </span>
          </span>
        ) : data && !data.hasLiveVehicle ? (
          <span className="lrc__row2">
            <span className="row gap-2" style={{ color: "var(--text-2)" }}>
              <IconBus size={16} />
              <span className="t-head">该方向暂无在线车辆</span>
            </span>
          </span>
        ) : focus ? (
          <span className="lrc__row2">
            <span className="lrc__eta tnum">{formatEtaRange(focus.etaMinLow, focus.etaMinHigh)}</span>
            <span className="t-label t-2">
              {focus.stopsAway === 0 ? "即将进站" : `还有 ${focus.stopsAway} 站`}
              {next ? ` · 下一班 ${Math.round(next.etaMinLow)} 分` : ""}
            </span>
            {/*
              运动状态是判断"这个 ETA 靠不靠谱"的关键：
              堵在路口的车即使 ETA 接近，也可能比预期晚很多
            */}
            {(() => {
              const v = data?.vehicles.find((x) => x.busId === focus.busId);
              const mb = motionBadge(v?.motion);
              return mb && mb.notable ? <Chip tone={mb.tone}>{mb.label}</Chip> : null;
            })()}
          </span>
        ) : (
          <span className="lrc__row2">
            <span className="t-head t-2">暂无到站预判</span>
          </span>
        )}

        {/* 第三行：微型时间轴 */}
        {data && data.stops.length > 1 ? (
          <span className="lrc__timeline">
            <MiniTimeline
              stops={data.stops.map((s, i) => ({
                stopNo: s.stopNo,
                stopName: s.stopName,
                at: data.cumulativeMeters[i] ?? 0,
              }))}
              totalMeters={data.totalMeters}
              vehicles={timelineVehicles}
              {...(focusAt != null ? { userAt: focusAt } : {})}
              variant="compact"
            />
            <span className="lrc__legend t-cap t-3">
              {startStop} → {endStop}
              {hasInferred ? " · 含推测位置" : ""}
            </span>
            {/*
              发车间隔由在线车辆的班次号反推，回答"错过这班要等多久"。
              待发班次来自上游的 plans 字段（收班时段为空，届时整行不显示）。
            */}
            {headwayText(data.headwayMin, data.pendingDepartures) ? (
              <span className="lrc__legend t-cap t-3">
                {headwayText(data.headwayMin, data.pendingDepartures)}
              </span>
            ) : null}
          </span>
        ) : null}

        {error && !data ? (
          <span className="t-cap" style={{ color: "var(--danger)" }}>
            {error}
          </span>
        ) : null}

        {data && data.vehicles[0] ? (
          <span className="t-cap t-3">更新于 {formatAge(data.vehicles[0].ageSeconds)}</span>
        ) : null}
      </span>
    </button>
  );
}
