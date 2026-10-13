import { useState } from "react";

import { Button, Card, Notice, SkeletonCard, StateView } from "../components/ui";
import { PageTitle, Screen, SectionTitle } from "../components/Screen";
import { JourneyOptionCard } from "../components/JourneyOptionCard";
import { TimePicker } from "../components/TimePicker";
import {
  IconClock,
  IconStop,
  IconHomePin,
  IconInfo,
  IconRoute,
  IconSwap,
  IconWork,
} from "../components/icons";
import { PlaceSearch } from "./PlaceSearch";
import { planJourney } from "../lib/api";
import { useAppStore } from "../lib/store";
import { useLocation } from "../lib/location";
import type { JourneyOption } from "../lib/types";
import "./PlanScreen.css";

/**
 * 规划页 —— 双目的地与最晚出发时间。
 *
 * # 设计要点（来自已定稿规格）
 *
 * 1. **结论前置**：每张方案卡最上面一行直接给"最晚 X 出发 → Y 到达"，
 *    而不是让用户自己把各段加起来。
 * 2. **方案给差异维度**（最快 / 少步行 / 更晚出发），不是一个排序列表。
 * 3. ETA 用**区间**。上游没有班次计划表，预测本质是概率。
 *
 * # 请求预算提示
 *
 * 首次规划要并发拉取候选线路的站点表与实时车辆位置，约 10-20 秒；
 * 站点表会长期缓存，之后规划几乎是瞬时的。UI 上要**明确告知**这一点，
 * 否则用户会以为卡死了。
 */
export function PlanScreen() {
  const { places, setPlace, clearPlace } = useAppStore();
  const { fix, nearby, refresh, loading: locating } = useLocation();

  const [swapped, setSwapped] = useState(false);
  const [editing, setEditing] = useState<null | "home" | "work">(null);

  const [planning, setPlanning] = useState(false);
  const [options, setOptions] = useState<JourneyOption[] | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<number | null>(0);
  const [planTime, setPlanTime] = useState<Date>(new Date());

  /**
   * 出发时刻（相对现在的分钟数）。null = 现在出发。
   *
   * 与 `arriveBefore` 是**两个独立**的概念：
   * 前者决定时间轴起点，后者是到达约束。
   */
  const [departOffset, setDepartOffset] = useState<number | null>(null);
  /** 最晚到达（相对现在的分钟数）。null = 不限制 */
  const [arriveBefore, setArriveBefore] = useState<number | null>(null);

  const from = swapped ? places.work : places.home;
  const to = swapped ? places.home : places.work;
  const bothSet = Boolean(places.home && places.work);

  /** 把当前位置保存为家或公司，并顺带记录最近的几个站点 */
  const saveCurrentAs = (kind: "home" | "work", label: string) => {
    if (!fix) return;
    setPlace({
      kind,
      label,
      point: { lng: fix.lng, lat: fix.lat },
      source: "gps",
      ...(fix.source === "simulated" ? { address: "模拟位置（未获取到真实定位）" } : {}),
      nearestStops: nearby.slice(0, 3),
    });
  };

  const run = async () => {
    if (!from || !to) return;
    setPlanning(true);
    setPlanError(null);
    setOptions(null);
    setPlanTime(new Date());
    try {
      const r = await planJourney({
        origin: from.point,
        destination: to.point,
        // 两个时刻各自传递，后端不再"反推出发"——
        // 反推是导致"出发晚于到达"的根源
        ...(departOffset != null && departOffset > 0
          ? { departOffsetMin: departOffset }
          : {}),
        ...(arriveBefore != null ? { arriveBeforeMin: arriveBefore } : {}),
      });
      setOptions(r);
      setExpanded(0);
    } catch (e) {
      setPlanError(e instanceof Error ? e.message : String(e));
    } finally {
      setPlanning(false);
    }
  };

  if (editing) {
    return (
      <PlaceSearch
        kind={editing}
        title={editing === "home" ? "家" : "公司"}
        onDone={() => setEditing(null)}
      />
    );
  }

  return (
    <Screen header={<PageTitle title="规划" subtitle="预判最晚出发时间，比较最少耗时方案" />}>
      {/* ---------- 起点 / 终点 ---------- */}
      <Card>
        <div className="plan__od">
          <div className="plan__od-rail" aria-hidden>
            <span className="plan__od-dot plan__od-dot--from" />
            <span className="plan__od-line" />
            <span className="plan__od-dot plan__od-dot--to" />
          </div>

          <div className="plan__od-fields">
            <button
              type="button"
              className="plan__od-field"
              onClick={() => setEditing(swapped ? "work" : "home")}
            >
              <div className="t-cap t-3">从</div>
              <div className="t-head truncate">
                {from ? from.label : <span className="t-3">点此设置</span>}
              </div>
            </button>
            <button
              type="button"
              className="plan__od-field plan__od-field--last"
              onClick={() => setEditing(swapped ? "home" : "work")}
            >
              <div className="t-cap t-3">到</div>
              <div className="t-head truncate">
                {to ? to.label : <span className="t-3">点此设置</span>}
              </div>
            </button>
          </div>

          <button
            type="button"
            className="plan__swap"
            onClick={() => setSwapped((v) => !v)}
            aria-label="交换起终点"
          >
            <IconSwap size={18} />
          </button>
        </div>
      </Card>

      {/* ---------- 地点未设置：引导 ---------- */}
      {!bothSet ? (
        <div style={{ marginTop: "var(--space-3)" }}>
          <Card>
            <div className="card__head">
              <span className="card__title">先设置家和公司</span>
            </div>
            <p className="t-label t-2" style={{ margin: "0 0 var(--space-3)" }}>
              这两个地点决定了「下班推回家、上班推去公司」的判断，设置后无需每次输入。
            </p>
            <div className="stack">
              <PlaceRow
                icon={<IconHomePin size={18} />}
                title="家"
                place={places.home}
                onEdit={() => setEditing("home")}
                onUseCurrent={() => saveCurrentAs("home", "家")}
                onClear={() => clearPlace("home")}
                canUseCurrent={Boolean(fix)}
                locating={locating}
                onLocate={() => void refresh()}
              />
              <PlaceRow
                icon={<IconWork size={18} />}
                title="公司"
                place={places.work}
                onEdit={() => setEditing("work")}
                onUseCurrent={() => saveCurrentAs("work", "公司")}
                onClear={() => clearPlace("work")}
                canUseCurrent={Boolean(fix)}
                locating={locating}
                onLocate={() => void refresh()}
              />
            </div>
            <div style={{ marginTop: "var(--space-3)" }}>
              <Notice tone="info">
                <IconInfo size={14} />
                <span>
                  按地址搜索需要配置高德 Key（设置页可填）。
                  也可以用当前位置——站在家或公司附近时点「就用这里」。
                </span>
              </Notice>
            </div>
          </Card>
        </div>
      ) : null}

      {/* ---------- 出发时刻与到达约束 ---------- */}
      {bothSet ? (
        <div style={{ marginTop: "var(--space-3)" }}>
          <Card>
            {/*
              两项分开是刻意的：
              - **出发时刻**决定时间轴从哪里开始，是主交互
              - **最晚到达**是约束，用来反推"最晚几点得走"
              早期只有后者，且实现是用 `deadline - 总耗时` 反推出发，
              却仍把"从现在起算的耗时"当到达时刻 —— 于是出现
              **出发时间比到达时间还晚**。现在两者各自独立、语义清晰。
            */}
            <div className="row gap-3">
              <IconClock size={20} className="t-2" />
              <div className="grow">
                <div className="t-head">出发时间</div>
                <div className="t-cap t-3">默认现在出发；可预估未来的行程</div>
              </div>
              <TimePicker
                mode="depart"
                label="出发时间"
                placeholder="现在出发"
                value={departOffset}
                onChange={setDepartOffset}
              />
            </div>

            <div className="plan__divider" />

            <div className="row gap-3">
              <IconStop size={20} className="t-2" />
              <div className="grow">
                <div className="t-head">最晚到达</div>
                <div className="t-cap t-3">不限制则只按出发时间算</div>
              </div>
              <TimePicker
                mode="arrive"
                label="最晚到达时间"
                placeholder="不限制"
                value={arriveBefore}
                onChange={setArriveBefore}
              />
            </div>
          </Card>

          <div style={{ marginTop: "var(--space-3)" }}>
            <Button block variant="accent" onClick={() => void run()} disabled={planning}>
              {planning ? "规划中…" : "开始规划"}
            </Button>
          </div>
        </div>
      ) : null}

      {/* ---------- 规划中：必须解释为什么慢 ---------- */}
      {planning ? (
        <div className="stack" style={{ marginTop: "var(--space-4)" }}>
          <SkeletonCard lines={3} />
          <Notice tone="info">
            <IconInfo size={14} />
            <span>
              正在并发拉取候选线路的站点表与实时车辆位置，首次约需 10-20 秒。
              这些数据会长期缓存，<b>之后再规划几乎是瞬时的</b>。
            </span>
          </Notice>
        </div>
      ) : null}

      {planError ? (
        <div style={{ marginTop: "var(--space-4)" }}>
          <Card>
            <StateView
              kind="offline"
              title="规划失败"
              detail={planError}
              action={
                <Button variant="ghost" onClick={() => void run()}>
                  重试
                </Button>
              }
            />
          </Card>
        </div>
      ) : null}

      {/* ---------- 方案列表 ---------- */}
      {options && !planning ? (
        <section>
          <SectionTitle
            action={
              options.length > 0 ? (
                <span className="t-cap t-3">
                  {new Date(planTime).toLocaleTimeString("zh-CN", {
                    hour: "2-digit",
                    minute: "2-digit",
                  })}{" "}
                  查询
                </span>
              ) : null
            }
          >
            {options.length > 0 ? `${options.length} 个方案` : "方案"}
          </SectionTitle>

          {options.length === 0 ? (
            <Card>
              <StateView
                kind="info"
                title="暂时给不出方案"
                detail={
                  <>
                    可能的原因：
                    <br />
                    · 起终点 400 米内没有查到公交站
                    <br />
                    · 相关线路当前没有在线车辆（收班或已回场）
                    <br />
                    <br />
                    上游<b>没有班次计划表</b>，所以没有在线车辆时无法预判下一班何时来。
                    与其编一个时间让你误车，这里选择如实告诉你。
                  </>
                }
                action={
                  <Button variant="ghost" onClick={() => void run()}>
                    重新规划
                  </Button>
                }
              />
            </Card>
          ) : (
            <div className="stack">
              {options.map((o, i) => (
                <JourneyOptionCard
                  key={i}
                  option={o}
                  baseTime={planTime}
                  rank={i}
                  expanded={expanded === i}
                  onSelect={() => setExpanded(expanded === i ? null : i)}
                />
              ))}
            </div>
          )}
        </section>
      ) : null}

      {/* ---------- 未开始规划时的提示 ---------- */}
      {bothSet && !options && !planning && !planError ? (
        <div style={{ marginTop: "var(--space-4)" }}>
          <Card>
            <StateView
              kind="info"
              title="点「开始规划」查看方案"
              detail={
                <>
                  会给出「最晚几点出发」与若干条可比方案（最快 / 少步行 / 更晚出发）。
                  <br />
                  每段都会标注数据可信度——实时 GPS 用实线，历史推测用虚线。
                </>
              }
            />
          </Card>
        </div>
      ) : null}

      {!bothSet ? (
        <div className="plan__foot t-cap t-3">
          <IconRoute size={12} /> 规划需要起点和终点两个坐标
        </div>
      ) : null}
    </Screen>
  );
}

/** 单个地点行 */
function PlaceRow({
  icon,
  title,
  place,
  onEdit,
  onUseCurrent,
  onClear,
  canUseCurrent,
  locating,
  onLocate,
}: {
  icon: React.ReactNode;
  title: string;
  place?: { label: string; point: { lng: number; lat: number }; address?: string } | undefined;
  onEdit: () => void;
  onUseCurrent: () => void;
  onClear: () => void;
  canUseCurrent: boolean;
  locating: boolean;
  onLocate: () => void;
}) {
  return (
    <div className="place-row">
      <span className="place-row__icon">{icon}</span>
      <button type="button" className="grow place-row__main" onClick={onEdit}>
        <span className="t-head" style={{ display: "block", textAlign: "left" }}>
          {place ? place.label : title}
        </span>
        <span className="t-cap t-3 truncate" style={{ display: "block", textAlign: "left" }}>
          {place
            ? `${place.point.lng.toFixed(5)}, ${place.point.lat.toFixed(5)}${
                place.address ? ` · ${place.address}` : ""
              }`
            : "未设置 · 点此按地址搜索"}
        </span>
      </button>
      <div className="row gap-2">
        {place ? (
          <Button variant="quiet" size="sm" onClick={onClear}>
            清除
          </Button>
        ) : canUseCurrent ? (
          <Button variant="ghost" size="sm" onClick={onUseCurrent}>
            用当前位置
          </Button>
        ) : (
          <Button variant="ghost" size="sm" onClick={onLocate} disabled={locating}>
            {locating ? "定位中" : "先定位"}
          </Button>
        )}
      </div>
    </div>
  );
}
