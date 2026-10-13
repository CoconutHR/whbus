import { useMemo, useState } from "react";

import { MiniTimeline, TimelineLegend } from "../components/MiniTimeline";
import { Button, Card, Chip, FreshnessChip, LineBadge, Segmented, StateView } from "../components/ui";
import {
  IconBack,
  IconBus,
  IconClock,
  IconMoon,
  IconRefresh,
  IconStar,
  IconSubway,
} from "../components/icons";
import { useLineRealtime } from "../lib/polling";
import { useAppStore } from "../lib/store";
import type { ArrivalView } from "../lib/types";
import { formatAge, formatDistance, formatEtaRange, headwayText, lineColorVar, motionBadge } from "../lib/utils";
import "./LineDetailView.css";

/**
 * 线路详情 —— 实时追踪。
 *
 * 已过站**不删除**，而是降饱和 + 缩小字号：用户需要确认"车确实已经过我这站了"
 * 才能安心，隐藏已过站会让人怀疑数据是否准确。
 */
export function LineDetailView({
  lineId,
  onBack,
}: {
  lineId: string;
  onBack: () => void;
}) {
  const { data, loading, error, updatedAt, refresh } = useLineRealtime(lineId);
  const { isFavorite, toggleFavorite } = useAppStore();
  const [view, setView] = useState<"timeline" | "map">("timeline");

  const fav = isFavorite(lineId);
  const color = lineColorVar(data?.lineName ?? lineId);

  // 每个站的到站预判，便于 O(1) 查表
  const arrivalByStop = useMemo(() => {
    const m = new Map<string, ArrivalView>();
    data?.arrivals.forEach((a) => m.set(a.stopId, a));
    return m;
  }, [data]);

  const timelineVehicles = useMemo(
    () =>
      data?.vehicles
        .filter((v) => v.alongM != null && !v.arrived)
        .map((v) => ({ along: v.alongM as number, confidence: v.confidence })) ?? [],
    [data],
  );

  const hasInferred = data?.vehicles.some((v) => v.confidence === "inferred") ?? false;
  const freshest = data?.vehicles[0] ?? null;

  return (
    <div className="ldv">
      {/* 吸顶头部（毛玻璃） */}
      <header className="ldv__header glass">
        <button type="button" className="ldv__back" onClick={onBack} aria-label="返回">
          <IconBack size={20} />
        </button>

        <div className="grow" style={{ minWidth: 0 }}>
          <div className="row gap-2">
            <LineBadge lineName={data?.lineName ?? "…"} color={color} size="lg" />
            <span className="t-cap t-2 truncate">
              {data ? `往 ${data.endStop}` : ""}
            </span>
          </div>
        </div>

        <button
          type="button"
          className="ldv__fav"
          aria-label={fav ? "取消收藏" : "收藏线路"}
          aria-pressed={fav}
          onClick={() =>
            data &&
            toggleFavorite({
              lineId: data.lineId,
              lineName: data.lineName,
              startStop: data.startStop,
              endStop: data.endStop,
              addedAt: Date.now(),
            })
          }
        >
          <IconStar size={20} filled={fav} />
        </button>
      </header>

      <div className="ldv__scroll scroll">
        {loading && !data ? (
          <div className="ldv__pad">
            <Card>
              <div className="skeleton" style={{ height: 34, width: "55%", marginBottom: 12 }} />
              <div className="skeleton" style={{ height: 16, width: "80%", marginBottom: 10 }} />
              <div className="skeleton" style={{ height: 26, width: "100%" }} />
            </Card>
          </div>
        ) : error && !data ? (
          <div className="ldv__pad">
            <Card>
              <StateView
                kind="offline"
                title="读取线路数据失败"
                detail={error}
                action={<Button onClick={refresh}>重试</Button>}
              />
            </Card>
          </div>
        ) : data ? (
          <>
            {/* 概览 */}
            <div className="ldv__pad">
              <Card>
                {!data.inService ? (
                  <div className="row gap-3">
                    <IconMoon size={22} />
                    <div className="grow">
                      <div className="t-head">今日已收班</div>
                      <div className="t-label t-2">
                        运营时间 {data.earlyHour}–{data.lastHour} · 明早 {data.earlyHour} 首班
                      </div>
                    </div>
                  </div>
                ) : !data.hasLiveVehicle ? (
                  <div className="row gap-3">
                    <IconBus size={22} />
                    <div className="grow">
                      <div className="t-head">该方向暂无在线车辆</div>
                      <div className="t-label t-2">
                        车辆可能尚未发车或已回场。这不是接口故障，稍后会自动更新。
                      </div>
                    </div>
                  </div>
                ) : (
                  <>
                    <div className="row gap-2" style={{ marginBottom: "var(--space-2)" }}>
                      <span className="t-label t-2">最近一班</span>
                      {freshest ? (
                        <FreshnessChip
                          confidence={freshest.confidence}
                          ageSeconds={freshest.ageSeconds}
                        />
                      ) : null}
                      <span className="grow" />
                      <span className="t-cap t-3">{formatAge(freshest?.ageSeconds)}</span>
                    </div>

                    {data.arrivals[0] ? (
                      <div className="row" style={{ alignItems: "baseline", gap: "var(--space-3)" }}>
                        <span className="ldv__big tnum">
                          {formatEtaRange(data.arrivals[0].etaMinLow, data.arrivals[0].etaMinHigh)}
                        </span>
                        <span className="t-label t-2">
                          {data.arrivals[0].stopName} 到站
                        </span>
                      </div>
                    ) : null}

                    <div className="t-cap t-3" style={{ marginTop: "var(--space-2)" }}>
                      在线 {data.vehicles.length} 辆 · 全程 {data.stops.length} 站 ·{" "}
                      {formatDistance(data.totalMeters)}
                    </div>

                    {/*
                      发车间隔由在线车辆的班次号反推；待发班次来自上游的 plans 字段。
                      这两项回答"错过这班要等多久"——上游不给完整时刻表，
                      这是唯一的推算途径。收班时 plans 为空，整块自然消失。
                    */}
                    {headwayText(data.headwayMin, data.pendingDepartures) ? (
                      <div className="ldv__headway t-cap t-2">
                        <IconClock size={12} />
                        {headwayText(data.headwayMin, data.pendingDepartures)}
                      </div>
                    ) : null}
                  </>
                )}

                <div style={{ marginTop: "var(--space-3)" }}>
                  <Segmented
                    value={view}
                    ariaLabel="视图切换"
                    onChange={setView}
                    options={[
                      { value: "timeline", label: "时间轴" },
                      { value: "map", label: "地图" },
                    ]}
                  />
                </div>
              </Card>
            </div>

            {view === "map" ? (
              <div className="ldv__pad">
                <Card>
                  <StateView
                    kind="info"
                    title="地图视图规划中"
                    detail="线路走向折线需要额外的路径数据（当前接口只提供站点坐标）。时间轴视图已经能表达车辆位置与到站顺序。"
                    action={
                      <Button variant="ghost" onClick={() => setView("timeline")}>
                        回到时间轴
                      </Button>
                    }
                  />
                </Card>
              </div>
            ) : (
              <>
                {/* 全线时间轴 */}
                {data.stops.length > 1 ? (
                  <div className="ldv__pad">
                    <Card>
                      <MiniTimeline
                        stops={data.stops.map((s, i) => ({
                          stopNo: s.stopNo,
                          stopName: s.stopName,
                          at: data.cumulativeMeters[i] ?? 0,
                        }))}
                        totalMeters={data.totalMeters}
                        vehicles={timelineVehicles}
                        variant="detail"
                      />
                      <TimelineLegend inferred={hasInferred} />
                      {hasInferred ? (
                        <div className="t-cap t-3" style={{ marginTop: "var(--space-2)" }}>
                          虚线为根据历史行驶速度推算的位置，不是实测 GPS。
                        </div>
                      ) : null}
                    </Card>
                  </div>
                ) : null}

                {/* 途经站点列表 */}
                <div className="ldv__pad">
                  <div className="section-title">
                    <span className="t-label t-2">途经 {data.stops.length} 站</span>
                    <button
                      type="button"
                      className="t-cap"
                      style={{ color: "var(--brand)", minHeight: 32 }}
                      onClick={refresh}
                    >
                      <IconRefresh size={13} /> 刷新
                    </button>
                  </div>

                  <div className="ldv__stops">
                    {data.stops.map((s, i) => {
                      const arrival = arrivalByStop.get(s.stopId);
                      const arrived = !arrival; // 车已过此站（或该站之后无车）
                      const vehiclesHere = data.vehicles.filter(
                        (v) => v.atStopNo === s.stopNo && !v.arrived,
                      );

                      return (
                        <div
                          key={s.stopId}
                          className={`ldv__stop${arrived ? " ldv__stop--passed" : ""}`}
                        >
                          {/* 左侧轨道 */}
                          <div className="ldv__rail" aria-hidden>
                            <span className={`ldv__dot${arrived ? " ldv__dot--passed" : ""}`} />
                            {i < data.stops.length - 1 ? <span className="ldv__line" /> : null}
                          </div>

                          <div className="ldv__stop-body">
                            <div className="row gap-2">
                              <span className="t-head truncate">{s.stopName}</span>
                              {s.subwayLines.length > 0 ? (
                                <span className="ldv__subway" title="地铁换乘">
                                  <IconSubway size={12} />
                                  {s.subwayLines.join("/")}
                                </span>
                              ) : null}
                            </div>

                            <div className="row gap-2" style={{ marginTop: 2 }}>
                              {vehiclesHere.length > 0 ? (
                                <Chip tone="brand">
                                  <IconBus size={12} />
                                  车辆在此
                                </Chip>
                              ) : null}
                              {arrival && data.inService && data.hasLiveVehicle ? (
                                <span className="t-label t-3 tnum">
                                  {formatEtaRange(arrival.etaMinLow, arrival.etaMinHigh)}
                                  {arrival.stopsAway > 0 ? ` · 还有 ${arrival.stopsAway} 站` : ""}
                                </span>
                              ) : arrived && data.inService && data.hasLiveVehicle ? (
                                <span className="t-cap t-3">已过</span>
                              ) : null}
                              {/*
                                到达该站的车的运动状态：停站/堵车都会让 ETA 不可靠，
                                用户需要据此决定要不要现在出门
                              */}
                              {(() => {
                                if (!arrival) return null;
                                const v = data.vehicles.find((x) => x.busId === arrival.busId);
                                const mb = motionBadge(v?.motion);
                                return mb && mb.notable ? (
                                  <Chip tone={mb.tone}>{mb.label}</Chip>
                                ) : null;
                              })()}
                            </div>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              </>
            )}

            <div className="ldv__foot t-cap t-3">
              <IconClock size={12} />
              {updatedAt
                ? `上次更新 ${new Date(updatedAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`
                : ""}
              {" · 车辆位置每 30-120 秒自动刷新"}
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
