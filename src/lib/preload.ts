/**
 * 启动预加载。
 *
 * # 目的是什么
 *
 * 一次规划要拉 8-16 条线路的站点表，冷启动实测 10-20 秒。
 * 而站点表当天不变、命中缓存后 **0 请求**。用户打开 App 到真正点
 * 「开始规划」之间通常有好几秒，这段时间是白送的预热窗口。
 *
 * # 预加载哪些线路
 *
 * 按"用户最可能用到"排序，且控制在 6 条以内（后端也会再截断）：
 *
 * 1. **收藏的线路** —— 用户明确表达过关注
 * 2. **家 / 公司附近站点已知的线路** —— 这些存在 `places[].nearestStops`
 *    里，是保存地点时顺手记下的，不需要新请求
 *
 * # 为什么放在 `requestIdleCallback` 之后
 *
 * 预加载会占用上游限流额度。如果它和首屏的"附近站点"查询抢带宽，
 * 用户会觉得 App 变慢了 —— 而预加载的全部意义就是让它更快。
 * 所以等首屏稳定、浏览器空闲时再开始。
 */

import { preloadLines } from "./api";
import type { FavoriteLine, SavedPlace } from "./types";

/** 最多预加载几条线路（后端亦有限量，这里是第一道闸） */
const MAX_PRELOAD = 6;

export interface PreloadSources {
  favorites: FavoriteLine[];
  /** store 里 places 是 `Partial<Record<kind, SavedPlace>>`，这里收成数组 */
  places: SavedPlace[];
}

/** 按优先级收集要预加载的线路号（去重、限量） */
export function collectPreloadLines({ favorites, places }: PreloadSources): string[] {
  const out: string[] = [];
  const seen = new Set<string>();

  const push = (name: string | undefined | null) => {
    const n = (name ?? "").trim();
    if (!n || seen.has(n)) return;
    seen.add(n);
    out.push(n);
  };

  // 1) 收藏优先 —— 用户明确表达过关注
  for (const f of favorites) push(f.lineName);

  // 2) 家/公司附近站点的线路 —— 保存地点时顺手记下的，不需要新请求
  for (const p of places) {
    for (const s of p.nearestStops ?? []) {
      for (const n of s.lineNames ?? []) push(n);
    }
  }

  return out.slice(0, MAX_PRELOAD);
}

/**
 * 启动预加载。**永不抛错**，也永不阻塞首屏 ——
 * 失败只是回到"没有预热"，不该影响任何正常功能。
 */
export function startPreload(sources: PreloadSources): void {
  const lines = collectPreloadLines(sources);
  if (lines.length === 0) return;

  const run = () => {
    void preloadLines(lines, MAX_PRELOAD).catch(() => {
      // 预加载是尽力而为：断网、收班、上游抽风都不该冒泡到 UI
    });
  };

  // 空闲时再跑；不支持 requestIdleCallback 的环境（旧 WebView）退化为延时
  const ric = (
    window as Window & {
      requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => number;
    }
  ).requestIdleCallback;

  if (typeof ric === "function") {
    ric(run, { timeout: 3000 });
  } else {
    window.setTimeout(run, 1200);
  }
}
