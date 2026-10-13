import { useEffect, useMemo } from "react";

import { DecisionCard } from "../components/DecisionCard";
import { LineRealtimeCard } from "../components/LineRealtimeCard";
import { PageTitle, Screen, SectionTitle } from "../components/Screen";
import { Button, Card, Notice, SkeletonCard, StateView } from "../components/ui";
import {
  IconAlert,
  IconChevron,
  IconClock,
  IconLocate,
  IconRefresh,
  IconStop,
  IconWalk,
} from "../components/icons";
import { useAppStore, nearPlace } from "../lib/store";
import { sourceLabel, useLocation } from "../lib/location";
import { useJourneyPlan } from "../lib/journey";
import type { GeoPoint } from "../lib/types";
import { formatDistance, nowHHMM } from "../lib/utils";
import "./HomeScreen.css";

const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

export function HomeScreen({ onGoToPlan }: { onGoToPlan?: () => void }) {
  const { fix, nearby, loading, error, denied, refresh } = useLocation();
  const { places, favorites, setTracking, haptic, setOpenLineId } = useAppStore();

  const now = new Date();
  const clock = nowHHMM(now);
  const weekday = WEEKDAYS[now.getDay()];

  // 首次进入自动定位一次
  useEffect(() => {
    if (!fix && !loading) void refresh();
    // 只在没有定位结果时跑一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /**
   * 情景判断 —— "智能展示"。
   *
   * 只做**基于位置**的判断（站在公司附近 → 推回家）。
   * 刻意不做纯时间猜测：定位漂移时推错方向比不推更糟。
   * 时间窗与历史采纳记录等加权项留到有真实使用数据后再接入。
   */
  const context = useMemo(() => {
    if (!fix) return null;
    const { home, work } = places;
    // 两个地点都设好才有意义——只知道一个方向就无从判断
    if (!home || !work) return null;
    const here: GeoPoint = { lng: fix.lng, lat: fix.lat };
    if (nearPlace(here, work)) {
      return { label: "回家", from: work, to: home, fromLabel: "公司" };
    }
    if (nearPlace(here, home)) {
      return { label: "去公司", from: home, to: work, fromLabel: "家" };
    }
    return null;
  }, [fix, places]);

  // 情景成立时才发起规划（否则纯属浪费请求）
  const plan = useJourneyPlan(
    context ? context.from.point : null,
    context ? context.to.point : null,
    Boolean(context),
  );

  // 最近的站点（附近的第一个）
  const nearest = nearby[0] ?? null;

  return (
    <Screen
      header={
        <PageTitle
          title={`${clock} · ${weekday}`}
          subtitle={
            fix ? (
              <span className="row gap-1">
                <IconLocate size={13} />
                {sourceLabel(fix.source)} · 精度 {Math.round(fix.accuracy)} 米
              </span>
            ) : loading ? (
              "正在定位…"
            ) : (
              "尚未定位"
            )
          }
          action={
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void refresh()}
              ariaLabel="刷新定位"
            >
              <IconRefresh size={16} />
            </Button>
          }
        />
      }
    >
      <div className="stack-lg">
        {/* ---------- 定位失败：给出可操作的下一步 ---------- */}
        {denied ? (
          <Card>
            <StateView
              kind="info"
              title="没有定位权限"
              detail="开启定位后即可看到附近站点的实时车辆。也可以在下方手动选择站点。"
              action={
                <Button variant="ghost" onClick={() => void refresh()}>
                  重新申请权限
                </Button>
              }
            />
          </Card>
        ) : null}

        {error && !denied ? (
          <Notice tone="warn">
            <IconAlert size={14} /> {error}
          </Notice>
        ) : null}

        {/* ---------- 情景决策卡：本 App 的核心价值 ---------- */}
        {context ? (
          <DecisionCard
            directionLabel={context.label}
            fromLabel={context.fromLabel}
            toLabel={context.label === "回家" ? "家" : "公司"}
            plan={plan.best}
            loading={plan.loading}
            error={plan.error}
            computedAt={plan.computedAt}
            stale={plan.stale}
            onRefresh={plan.refresh}
            onAccept={() => {
              if (!plan.best) return;
              // 采纳 → 进入追踪模式：Tab 栏上方会出现常驻细条，
              // 避免用户"采纳完就不知道下一步看哪"。
              //
              // 必须从**乘车段**里取 lineId 与线路号：只存线路号的话，
              // 追踪条点开时无法打开详情页（LineDetailView 要的是带方向的 id）。
              const lines = plan.best.legs
                .filter((l) => l.kind === "bus" && l.lineId && l.lineName)
                .map((l) => ({ id: l.lineId as string, name: l.lineName as string }));
              if (lines.length === 0) return;

              setTracking({
                label: context.label,
                toLabel: context.label === "回家" ? "家" : "公司",
                arriveOffsetMin: plan.best.arriveOffsetMin,
                acceptedAt: Date.now(),
                lines,
              });
              void haptic("light");
            }}
            {...(onGoToPlan ? { onOpenPlan: onGoToPlan } : {})}
          />
        ) : null}

        {/* ---------- 地点未设置：引导去规划页设置 ---------- */}
        {!context && (places.home || places.work) && fix ? (
          <Card>
            <StateView
              kind="info"
              title="不在家或公司附近"
              detail={
                <>
                  到了家或公司附近会自动显示「几点出发、几点到家」的建议。
                  <br />
                  还没设置两个地点的话，到「规划」页设置即可。
                </>
              }
              {...(onGoToPlan
                ? {
                    action: (
                      <Button variant="ghost" onClick={onGoToPlan}>
                        去设置
                      </Button>
                    ),
                  }
                : {})}
            />
          </Card>
        ) : null}

        {/* ---------- 最近站点 ---------- */}
        <section>
          <SectionTitle
            action={
              nearest ? (
                <span className="t-cap t-3 tnum">
                  {fix ? `距你 ${formatDistance(nearest.distanceM)}` : ""}
                </span>
              ) : null
            }
          >
            {nearest ? `最近站点 · ${shortStopName(nearest.stopName)}` : "附近站点"}
          </SectionTitle>

          {loading && nearby.length === 0 ? (
            <SkeletonCard lines={2} />
          ) : nearby.length === 0 ? (
            <Card>
              <StateView
                kind="empty"
                title="附近没有查到站点"
                detail={
                  fix
                    ? "公交接口的周边搜索半径约 400 米，你当前位置附近可能确实没有站点。"
                    : "先完成定位，或到「线路」页手动搜索站点。"
                }
                action={
                  <Button variant="ghost" onClick={() => void refresh()}>
                    重新定位
                  </Button>
                }
              />
            </Card>
          ) : (
            <div className="stack">
              {nearby.slice(0, 4).map((s) => (
                <button
                  key={s.stopId}
                  type="button"
                  className="stop-row"
                  onClick={() => s.lineIds[0] && setOpenLineId(s.lineIds[0])}
                >
                  <span className="stop-row__icon">
                    <IconStop size={18} />
                  </span>
                  <span className="list__grow">
                    <span className="row gap-2">
                      <span className="t-head truncate">{shortStopName(s.stopName)}</span>
                      {directionTag(s.stopName) ? (
                        <span className="t-cap t-3">{directionTag(s.stopName)}</span>
                      ) : null}
                    </span>
                    <span className="t-label t-2 row gap-2" style={{ marginTop: 2 }}>
                      <IconWalk size={13} />
                      {formatDistance(s.distanceM)}
                      {s.lineNames.length > 0 ? (
                        <span className="t-3">
                          · {s.lineNames.slice(0, 5).join(" / ")}
                          {s.lineNames.length > 5 ? " …" : ""}
                        </span>
                      ) : null}
                    </span>
                  </span>
                  <IconChevron size={16} className="t-3" />
                </button>
              ))}
            </div>
          )}
        </section>

        {/* ---------- 收藏线路 ---------- */}
        <section>
          <SectionTitle>{`收藏线路${favorites.length ? ` · ${favorites.length}` : ""}`}</SectionTitle>
          {favorites.length === 0 ? (
            <Card>
              <StateView
                kind="empty"
                title="还没有收藏线路"
                detail="到「线路」页搜索常坐的公交，收藏后会在这里显示实时等待时间。"
              />
            </Card>
          ) : (
            <div className="stack">
              {favorites.map((f) => (
                <LineRealtimeCard
                  key={f.lineId}
                  lineId={f.lineId}
                  lineName={f.lineName}
                  startStop={f.startStop}
                  endStop={f.endStop}
                  onClick={() => setOpenLineId(f.lineId)}
                />
              ))}
            </div>
          )}
        </section>

        <div className="home__footnote t-cap t-3">
          <IconClock size={12} /> 数据来自武汉公交实时接口，收班时段无在线车辆属正常现象
        </div>
      </div>
    </Screen>
  );
}

/** 站点名去掉方向括号，方向单独作为小字标签展示（信息密度更高且不重复） */
function shortStopName(name: string): string {
  return name.replace(/[（(](东行|西行|南行|北行)[)）]\s*$/, "");
}

function directionTag(name: string): string | null {
  const m = name.match(/[（(](东行|西行|南行|北行)[)）]\s*$/);
  return m?.[1] ?? null;
}
