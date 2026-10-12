import { useState } from "react";

import { Button, Card, Chip, Notice, StateView } from "../components/ui";
import { PageTitle, Screen } from "../components/Screen";
import {
  IconClock,
  IconHomePin,
  IconInfo,
  IconRoute,
  IconSwap,
  IconWork,
} from "../components/icons";
import { useAppStore } from "../lib/store";
import { useLocation } from "../lib/location";
import "./PlanScreen.css";

/**
 * 规划页 —— 双目的地与最晚出发时间。
 *
 * # 设计要点（来自已定稿规格）
 *
 * 1. **结论前置**：最上面一行直接给"最晚 X 出发，Y 到达"，
 *    而不是先列一堆方案让用户自己算。
 * 2. **方案必须给出差异维度**（最快 / 少步行 / 更晚出发），
 *    而不是一个排序列表——用户要的是决策依据，不是一个答案。
 * 3. ETA 用**区间**表达。没有班次计划表时预测本质是概率，
 *    给一个假装精确的数字是误导。
 *
 * # 当前状态
 *
 * 规划算法（`journey.rs`）尚在实现中。本页先把交互骨架与地点设置做完，
 * 地点数据是规划的必要输入。
 */
export function PlanScreen() {
  const { places, setPlace, clearPlace } = useAppStore();
  const { fix, nearby, refresh, loading } = useLocation();
  const [swapped, setSwapped] = useState(false);

  const from = swapped ? places.work : places.home;
  const to = swapped ? places.home : places.work;

  /** 把当前位置保存为家或公司，并顺带记录最近的几个站点 */
  const saveCurrentAs = (kind: "home" | "work", label: string) => {
    if (!fix) return;
    setPlace({
      kind,
      label,
      point: { lng: fix.lng, lat: fix.lat },
      source: "gps",
      ...(fix.simulated ? { address: "模拟位置（桌面开发）" } : {}),
      nearestStops: nearby.slice(0, 3),
    });
  };

  const bothSet = Boolean(places.home && places.work);

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
            <div className="plan__od-field">
              <div className="t-cap t-3">从</div>
              <div className="t-head truncate">
                {from ? from.label : <span className="t-3">未设置</span>}
              </div>
            </div>
            <div className="plan__od-field plan__od-field--last">
              <div className="t-cap t-3">到</div>
              <div className="t-head truncate">
                {to ? to.label : <span className="t-3">未设置</span>}
              </div>
            </div>
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

      {/* ---------- 地点设置（规划的必要输入） ---------- */}
      {!bothSet ? (
        <div style={{ marginTop: "var(--space-3)" }}>
          <Card>
            <div className="card__head">
              <span className="card__title">先设置家和公司</span>
            </div>
            <p className="t-label t-2" style={{ margin: "0 0 var(--space-3)" }}>
              这两个地点决定了「下班推回家、上班推去公司」的判断。
              设置后无需每次输入。
            </p>

            <div className="stack">
              <PlaceRow
                icon={<IconHomePin size={18} />}
                title="家"
                place={places.home}
                onUseCurrent={() => saveCurrentAs("home", "家")}
                onClear={() => clearPlace("home")}
                canUseCurrent={Boolean(fix)}
                locating={loading}
                onLocate={() => void refresh()}
              />
              <PlaceRow
                icon={<IconWork size={18} />}
                title="公司"
                place={places.work}
                onUseCurrent={() => saveCurrentAs("work", "公司")}
                onClear={() => clearPlace("work")}
                canUseCurrent={Boolean(fix)}
                locating={loading}
                onLocate={() => void refresh()}
              />
            </div>

            <div style={{ marginTop: "var(--space-3)" }}>
              <Notice tone="info">
                <IconInfo size={14} />
                <span>
                  「用当前位置」需要站在家或公司附近时点击。
                  <br />
                  按地址/小区名搜索需要接入高德 Web API（已在计划中）。
                </span>
              </Notice>
            </div>
          </Card>
        </div>
      ) : (
        <div style={{ marginTop: "var(--space-3)" }}>
          <div className="stack">
            <PlaceRow
              compact
              icon={<IconHomePin size={16} />}
              title="家"
              place={places.home}
              onUseCurrent={() => saveCurrentAs("home", "家")}
              onClear={() => clearPlace("home")}
              canUseCurrent={Boolean(fix)}
              locating={loading}
              onLocate={() => void refresh()}
            />
            <PlaceRow
              compact
              icon={<IconWork size={16} />}
              title="公司"
              place={places.work}
              onUseCurrent={() => saveCurrentAs("work", "公司")}
              onClear={() => clearPlace("work")}
              canUseCurrent={Boolean(fix)}
              locating={loading}
              onLocate={() => void refresh()}
            />
          </div>
        </div>
      )}

      {/* ---------- 到达时间约束 ---------- */}
      <div style={{ marginTop: "var(--space-3)" }}>
        <Card>
          <div className="row gap-3">
            <IconClock size={20} className="t-2" />
            <div className="grow">
              <div className="t-head">最晚到达时间</div>
              <div className="t-cap t-3">用于反推「最晚几点必须出发」</div>
            </div>
            <Chip tone="neutral">未设置</Chip>
          </div>
        </Card>
      </div>

      {/* ---------- 方案区 ---------- */}
      <div style={{ marginTop: "var(--space-4)" }}>
        <Card>
          <StateView
            kind="info"
            title={bothSet ? "规划算法正在实现" : "设置好家与公司后即可规划"}
            detail={
              bothSet ? (
                <>
                  已实现：实时车辆定位、到站预判（ETA 区间）、坐标转换、站点表缓存。
                  <br />
                  待实现：候选站点枚举与一次换乘搜索。
                  <br />
                  <br />
                  之所以要先做这些基础设施：上游接口
                  <b>没有班次计划表</b>（<code>plans</code> 恒为空），
                  「最晚几点出发」只能靠自建的行驶时间模型推算。
                </>
              ) : (
                "规划需要起点和终点两个坐标。用「当前位置」站在相应地点点击即可。"
              )
            }
            action={
              <div className="row gap-2" style={{ marginTop: "var(--space-2)" }}>
                <Button variant="ghost" onClick={() => void refresh()} disabled={loading}>
                  {loading ? "定位中…" : "刷新定位"}
                </Button>
              </div>
            }
          />
        </Card>
      </div>

      <div className="plan__foot t-cap t-3">
        <IconRoute size={12} /> 规划结果会标注每段的数据可信度（实时 / 推测）
      </div>
    </Screen>
  );
}

/** 单个地点设置行 */
function PlaceRow({
  icon,
  title,
  place,
  onUseCurrent,
  onClear,
  canUseCurrent,
  locating,
  onLocate,
  compact,
}: {
  icon: React.ReactNode;
  title: string;
  place?: { label: string; point: { lng: number; lat: number }; address?: string } | undefined;
  onUseCurrent: () => void;
  onClear: () => void;
  canUseCurrent: boolean;
  locating: boolean;
  onLocate: () => void;
  compact?: boolean;
}) {
  return (
    <div className={`place-row${compact ? " place-row--compact" : ""}`}>
      <span className="place-row__icon">{icon}</span>
      <div className="grow" style={{ minWidth: 0 }}>
        <div className="t-head">{title}</div>
        {place ? (
          <div className="t-cap t-3 truncate">
            {place.point.lng.toFixed(5)}, {place.point.lat.toFixed(5)}
            {place.address ? ` · ${place.address}` : ""}
          </div>
        ) : (
          <div className="t-cap t-3">未设置</div>
        )}
      </div>
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
