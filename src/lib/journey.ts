/**
 * 出行规划的数据获取。
 *
 * # 为什么缓存与节流都要自己做
 *
 * 规划是全 App **最耗请求**的接口：首次需要逐条拉取线路站点表，
 * 而上游要求请求间隔 ≥10 秒。如果首页每次挂载/切回都重新规划，
 * 会反复触发 20+ 个请求。
 *
 * 因此这里的策略是：
 * - 同一对起终点在 `TTL` 内复用结果（默认 3 分钟）
 * - 切换 Tab 回来不重算，只有真的过期才重算
 * - 暴露 `refresh()` 供用户手动触发
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { planJourney } from "./api";
import type { GeoPoint, JourneyOption } from "./types";

/** 结果的有效期：实时公交变化快，3 分钟是个平衡点 */
const TTL_MS = 3 * 60 * 1000;

interface CacheEntry {
  key: string;
  options: JourneyOption[];
  at: number;
}

/** 模块级缓存：跨组件、跨 Tab 切换共享 */
let cache: CacheEntry | null = null;

function keyOf(o: GeoPoint, d: GeoPoint): string {
  // 约 10 米精度：走路挪几步不该触发重新规划
  const r = (n: number) => n.toFixed(4);
  return `${r(o.lng)},${r(o.lat)}|${r(d.lng)},${r(d.lat)}`;
}

export interface UseJourneyPlan {
  options: JourneyOption[] | null;
  /** 推荐方案（总耗时最短） */
  best: JourneyOption | null;
  loading: boolean;
  error: string | null;
  /** 结果生成时刻，用于显示"X 分钟前计算" */
  computedAt: Date | null;
  /** 结果是否已过期（过期后 UI 应提示可刷新） */
  stale: boolean;
  refresh: () => void;
}

/**
 * 为给定起终点做规划。
 *
 * `origin` / `destination` 为 `null` 时不发起任何请求。
 */
export function useJourneyPlan(
  origin: GeoPoint | null,
  destination: GeoPoint | null,
  enabled = true,
): UseJourneyPlan {
  const [options, setOptions] = useState<JourneyOption[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [computedAt, setComputedAt] = useState<Date | null>(null);
  const [stale, setStale] = useState(false);

  const alive = useRef(true);
  const reqId = useRef(0);

  const key =
    origin && destination ? keyOf(origin, destination) : null;

  const run = useCallback(
    async (force: boolean) => {
      if (!origin || !destination || !key) return;

      // 命中未过期的缓存 → 直接复用，不发请求
      if (!force && cache && cache.key === key) {
        setOptions(cache.options);
        setComputedAt(new Date(cache.at));
        setStale(Date.now() - cache.at > TTL_MS);
        setError(null);
        return;
      }

      const id = ++reqId.current;
      setLoading(true);
      setError(null);

      try {
        const r = await planJourney({ origin, destination });
        if (!alive.current || id !== reqId.current) return;
        cache = { key, options: r, at: Date.now() };
        setOptions(r);
        setComputedAt(new Date(cache.at));
        setStale(false);
      } catch (e) {
        if (!alive.current || id !== reqId.current) return;
        setError(e instanceof Error ? e.message : String(e));
        setOptions(null);
      } finally {
        if (alive.current && id === reqId.current) setLoading(false);
      }
    },
    // origin/destination 是对象，用 key 作为依赖更稳定
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key],
  );

  useEffect(() => {
    alive.current = true;
    if (!enabled || !key) {
      setOptions(null);
      setComputedAt(null);
      setStale(false);
      return;
    }
    void run(false);

    // 过期后在下次可见时重算（不轮询，省电与省请求）
    const onVisible = () => {
      if (!document.hidden && cache && Date.now() - cache.at > TTL_MS) {
        void run(false);
      }
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      alive.current = false;
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [key, enabled, run]);

  const refresh = useCallback(() => {
    void run(true);
  }, [run]);

  return {
    options,
    best: options && options.length > 0 ? (options[0] ?? null) : null,
    loading,
    error,
    computedAt,
    stale,
    refresh,
  };
}

/** 供设置页/调试用：清空规划缓存 */
export function clearPlanCache() {
  cache = null;
}
