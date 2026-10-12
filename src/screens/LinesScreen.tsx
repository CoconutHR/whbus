import { useEffect, useRef, useState } from "react";

import { Card, Chip, StateView } from "../components/ui";
import { LineRealtimeCard } from "../components/LineRealtimeCard";
import { PageTitle, Screen, SectionTitle } from "../components/Screen";
import { IconBus, IconMoon, IconSearch, IconStar, IconStop } from "../components/icons";
import { LineDetailView } from "./LineDetailView";
import { searchLines, searchStops } from "../lib/api";
import { useAppStore } from "../lib/store";
import type { LineSummary, StopSummary } from "../lib/types";
import { lineColorVar, nowHHMM } from "../lib/utils";
import "./LinesScreen.css";

/**
 * 线路页 —— 搜索与追踪。
 *
 * # 上游搜索的两条硬约束（都已实测）
 *
 * 1. **只匹配站名子串**，不认地址/小区名/写字楼名。
 *    `"常青花园"` ✅ / `"楚天雅苑"` ❌
 * 2. **收班后接口整体返回空**（凌晨连 `"715"` 都搜不到）。
 *    这是服务端时段行为，不是故障——必须给用户明确解释，
 *    否则会被当成 App 坏了。
 *
 * 两次搜索并行发出，按返回内容自动切换展示模式，省掉用户一次选择。
 */
export function LinesScreen() {
  const [keyword, setKeyword] = useState("");
  const [lines, setLines] = useState<LineSummary[]>([]);
  const [stops, setStops] = useState<StopSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [openLine, setOpenLine] = useState<string | null>(null);
  const [searched, setSearched] = useState(false);

  const { favorites } = useAppStore();
  const inputRef = useRef<HTMLInputElement>(null);
  const reqId = useRef(0);

  const run = async (kw: string) => {
    const q = kw.trim();
    if (!q) {
      setLines([]);
      setStops([]);
      setSearched(false);
      return;
    }
    const id = ++reqId.current;
    setLoading(true);
    setError(null);
    try {
      // 并行发两个请求：线路号模式与站名模式互斥，但用户不该为此做选择
      const [ls, ss] = await Promise.allSettled([searchLines(q), searchStops(q)]);
      if (id !== reqId.current) return; // 丢弃过期响应

      const errs: string[] = [];
      if (ls.status === "fulfilled") setLines(ls.value);
      else {
        setLines([]);
        errs.push(String(ls.reason));
      }
      if (ss.status === "fulfilled") setStops(ss.value);
      else {
        setStops([]);
        errs.push(String(ss.reason));
      }
      setError(errs.length === 2 ? errs[0] ?? "搜索失败" : null);
    } finally {
      if (id === reqId.current) {
        setLoading(false);
        setSearched(true);
      }
    }
  };

  // 输入防抖 350ms
  useEffect(() => {
    const t = window.setTimeout(() => void run(keyword), 350);
    return () => window.clearTimeout(t);
  }, [keyword]);

  if (openLine) {
    return <LineDetailView lineId={openLine} onBack={() => setOpenLine(null)} />;
  }

  const empty = searched && !loading && lines.length === 0 && stops.length === 0;
  const hhmm = nowHHMM();
  const isNight = hhmm >= "22:00" || hhmm < "05:00";

  return (
    <Screen header={<PageTitle title="线路" subtitle="搜索线路号或站名，查看实时车辆" />}>
      {/* 搜索框 */}
      <div className="lines__search">
        <IconSearch size={18} className="lines__search-icon" />
        <input
          ref={inputRef}
          className="lines__input"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          placeholder="线路号或站名，如 715 / 常青花园"
          /* 线路号是纯数字，默认弹数字键盘能显著提速 */
          inputMode="search"
          enterKeyHint="search"
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          aria-label="搜索线路或站点"
        />
        {keyword ? (
          <button
            type="button"
            className="lines__clear"
            onClick={() => {
              setKeyword("");
              inputRef.current?.focus();
            }}
            aria-label="清空"
          >
            ×
          </button>
        ) : null}
      </div>

      {/* 收藏线路置顶 */}
      {!keyword && favorites.length > 0 ? (
        <section>
          <SectionTitle>
            <span className="row gap-1">
              <IconStar size={13} filled /> 我的收藏
            </span>
          </SectionTitle>
          <div className="stack">
            {favorites.map((f) => (
              <LineRealtimeCard
                key={f.lineId}
                lineId={f.lineId}
                lineName={f.lineName}
                startStop={f.startStop}
                endStop={f.endStop}
                boardStopId={f.boardStopId}
                onClick={() => setOpenLine(f.lineId)}
              />
            ))}
          </div>
        </section>
      ) : null}

      {/* 搜索结果 */}
      {loading ? (
        <div className="stack" style={{ marginTop: "var(--space-4)" }}>
          <Card>
            <div className="skeleton" style={{ height: 18, width: "40%", marginBottom: 10 }} />
            <div className="skeleton" style={{ height: 14, width: "72%" }} />
          </Card>
        </div>
      ) : null}

      {error ? (
        <div style={{ marginTop: "var(--space-4)" }}>
          <Card>
            <StateView kind="offline" title="搜索失败" detail={error} />
          </Card>
        </div>
      ) : null}

      {empty && !error ? (
        <div style={{ marginTop: "var(--space-4)" }}>
          <Card>
            <StateView
              kind={isNight ? "info" : "empty"}
              title={isNight ? "现在可能已收班" : "没有找到匹配的线路或站点"}
              detail={
                isNight ? (
                  <>
                    上游接口在收班时段会返回空结果，这是服务端行为而非故障。
                    <br />
                    明早 5:30 之后重试即可。已收藏的线路不受影响。
                  </>
                ) : (
                  <>
                    搜索只匹配**站名**，不认小区名和写字楼名。
                    <br />
                    试试输入线路号（如 <b>715</b>）或完整站名（如 <b>常青花园</b>）。
                  </>
                )
              }
            />
          </Card>
        </div>
      ) : null}

      {stops.length > 0 ? (
        <section>
          <SectionTitle>{`站点 · ${stops.length}`}</SectionTitle>
          <div className="stack">
            {stops.slice(0, 12).map((s) => (
              <div key={s.stopId} className="lines__stop">
                <span className="lines__stop-icon">
                  <IconStop size={16} />
                </span>
                <span className="list__grow">
                  <span className="t-head truncate">{s.stopName}</span>
                </span>
              </div>
            ))}
            {stops.length > 12 ? (
              <div className="t-cap t-3" style={{ textAlign: "center", padding: "var(--space-2)" }}>
                还有 {stops.length - 12} 个站点，请输入更完整的站名
              </div>
            ) : null}
          </div>
        </section>
      ) : null}

      {lines.length > 0 ? (
        <section>
          <SectionTitle>{`线路 · ${lines.length}`}</SectionTitle>
          <div className="stack">
            {lines.map((l) => (
              <button
                key={l.lineId}
                type="button"
                className="lines__line"
                onClick={() => setOpenLine(l.lineId)}
              >
                <span
                  className="lines__line-bar"
                  style={{ background: lineColorVar(l.lineName) }}
                  aria-hidden
                />
                <span className="list__grow">
                  <span className="row gap-2">
                    <span className="lines__line-no">{l.lineName}</span>
                    <Chip tone="neutral">{l.price} 元</Chip>
                  </span>
                  <span className="t-label t-2 truncate" style={{ display: "block", marginTop: 2 }}>
                    {l.startStop} → {l.endStop}
                  </span>
                  <span className="t-cap t-3">
                    运营 {l.earlyHour}–{l.lastHour}
                  </span>
                </span>
                <IconBus size={18} className="t-3" />
              </button>
            ))}
          </div>
        </section>
      ) : null}

      {/* 空态引导 */}
      {!keyword && favorites.length === 0 ? (
        <div style={{ marginTop: "var(--space-6)" }}>
          <Card>
            <StateView
              kind="info"
              title="先搜一条常坐的线路"
              detail={
                <>
                  搜索只匹配站名与线路号。地址与小区名搜不到——
                  <br />
                  那是公交接口的限制，不是本 App 的问题。
                </>
              }
            />
          </Card>
        </div>
      ) : null}

      {isNight && (lines.length > 0 || stops.length > 0) ? (
        <div className="row gap-2 t-cap t-3" style={{ justifyContent: "center", paddingTop: "var(--space-4)" }}>
          <IconMoon size={12} /> 夜间时段，实时车辆数据可能为空
        </div>
      ) : null}
    </Screen>
  );
}
