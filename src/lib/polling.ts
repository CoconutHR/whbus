/**
 * 实时数据轮询。
 *
 * # 为什么间隔要自适应
 *
 * 上游建议请求间隔 ≥ 10 秒，但通勤场景下"车还有几站"决定了用户是否要跑。
 * 固定间隔要么浪费电、要么在最需要的时候不够新鲜。策略：
 *
 * | 车辆距你 | 间隔 | 理由 |
 * |---|---|---|
 * | ≤ 2 站 | 30s | 马上要决策 |
 * | 3-5 站 | 60s | 正常 |
 * | > 5 站 或 无车 | 120s | 不值得耗电 |
 * | 已收班 | 停止轮询 | 不会有新数据 |
 *
 * 另外：页面切到后台（`visibilitychange`）自动暂停，回到前台立即拉一次。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { getLineVehicles } from "./api";
import type { LineRealtime } from "./types";

export interface PollState<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
  /** 上次成功更新的时刻 */
  updatedAt: number | null;
  refresh: () => void;
}

function intervalFor(data: LineRealtime | null): number | null {
  if (!data) return 60_000;
  // 已收班：不再轮询，省电
  if (!data.inService) return null;
  // 该方向没有任何在线车辆：低频试探，等首班车出现
  if (data.vehicles.length === 0) return 120_000;

  const nearest = data.arrivals.reduce<number | null>(
    (min, a) => (min == null || a.stopsAway < min ? a.stopsAway : min),
    null,
  );
  if (nearest == null) return 120_000;
  if (nearest <= 2) return 30_000;
  if (nearest <= 5) return 60_000;
  return 120_000;
}

/**
 * 轮询一条线路的实时状态。
 *
 * `fn` 由调用方注入，便于复用同一套策略拉取不同数据。
 */
export function usePolling<T>(
  fn: () => Promise<T>,
  computeInterval: (data: T | null) => number | null,
  deps: unknown[] = [],
): PollState<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);

  const dataRef = useRef<T | null>(null);
  const timerRef = useRef<number | null>(null);
  const aliveRef = useRef(true);
  const fnRef = useRef(fn);
  fnRef.current = fn;
  const intervalRef = useRef(computeInterval);
  intervalRef.current = computeInterval;

  const clear = () => {
    if (timerRef.current != null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  };

  const tick = useCallback(async () => {
    if (!aliveRef.current) return;
    try {
      const next = await fnRef.current();
      if (!aliveRef.current) return;
      dataRef.current = next;
      setData(next);
      setUpdatedAt(Date.now());
      setError(null);
    } catch (e) {
      if (!aliveRef.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (aliveRef.current) setLoading(false);
    }

    // 用最新数据决定下一次间隔
    clear();
    const ms = intervalRef.current(dataRef.current);
    if (ms != null && aliveRef.current && !document.hidden) {
      timerRef.current = window.setTimeout(() => void tick(), ms);
    }
  }, []);

  const manualRefresh = useCallback(() => {
    clear();
    setLoading(true);
    void tick();
  }, [tick]);

  useEffect(() => {
    aliveRef.current = true;
    setLoading(true);
    void tick();

    const onVisibility = () => {
      if (document.hidden) {
        // 切到后台：暂停，省电
        clear();
      } else {
        // 回到前台：立即拉一次，用户不该看到过期数据
        void tick();
      }
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      aliveRef.current = false;
      clear();
      document.removeEventListener("visibilitychange", onVisibility);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, ...deps]);

  return { data, loading, error, updatedAt, refresh: manualRefresh };
}

/** 线路实时状态轮询（内置本文件顶部描述的间隔策略） */
export function useLineRealtime(lineId: string): PollState<LineRealtime> {
  return usePolling<LineRealtime>(
    () => getLineVehicles(lineId),
    (d) => intervalFor(d),
    [lineId],
  );
}
