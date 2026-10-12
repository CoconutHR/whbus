/**
 * 定位。
 *
 * # 坐标系陷阱（本项目的头号坑）
 *
 * `tauri-plugin-geolocation` 返回 **WGS-84**，而公交 API 用 **GCJ-02**。
 * 武汉地区两者相差约 **587 米**，而 `nearBy` 的搜索半径只有约 **400 米**。
 * 偏移量大于搜索半径 ⇒ 不转换会查到**完全错误的站点**。
 *
 * 因此本模块的 `point` 字段**始终是 GCJ-02**，转换在 Rust 侧完成。
 *
 * # 桌面端
 *
 * geolocation 插件是 mobile-only：桌面端 `getCurrentPosition()` 会静默返回
 * `0,0` 而不报错。开发时必须识别这种情况并改用模拟位置，否则会拿到
 * 几内亚湾的坐标去查武汉公交。
 */

import { useCallback, useState } from "react";

import { getNearbyStops, isTauri, wgs84ToGcj02 } from "./api";
import { useAppStore } from "./store";
import type { GeoPoint, NearbyStop } from "./types";

/** 开发用模拟位置：武汉常青花园片区（已经是 GCJ-02） */
const SIMULATED: GeoPoint = { lng: 114.242448, lat: 30.636765 };

export interface Fix {
  /** **GCJ-02** 经度 */
  lng: number;
  /** **GCJ-02** 纬度 */
  lat: number;
  /** 定位精度（米） */
  accuracy: number;
  /** 取到定位的时刻 */
  at: number;
  /** 是否来自模拟（桌面开发或无权限时的兜底） */
  simulated: boolean;
}

function isTauriMobile(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
}

/** 判断坐标是否明显是无效的 0,0（桌面插件的静默返回值） */
function isNullIsland(p: GeoPoint): boolean {
  return Math.abs(p.lng) < 0.5 && Math.abs(p.lat) < 0.5;
}

export interface UseLocation {
  fix: Fix | null;
  nearby: NearbyStop[];
  loading: boolean;
  error: string | null;
  /** 权限被拒时为 true —— UI 应给出"手动选站"的入口 */
  denied: boolean;
  refresh: () => Promise<Fix | null>;
}

/**
 * 取当前位置并顺带查周边站点。
 *
 * 桌面环境自动降级到模拟位置，让 UI 开发不必依赖真机。
 */
export function useLocation(): UseLocation {
  const { lastFix, lastNearby, setFix } = useAppStore();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [denied, setDenied] = useState(false);

  // 跨 Tab 共享上次定位结果，避免每次切页都重新定位。
  // `simulated` 必须一并持久化：早期版本在这里写死 false，
  // 结果桌面端重新加载页面后，模拟坐标会被当成"已定位"展示，
  // 用户看到的精度与来源都是错的。
  const fix: Fix | null = lastFix;

  const refresh = useCallback(async (): Promise<Fix | null> => {
    setLoading(true);
    setError(null);

    let gcj: GeoPoint | null = null;
    let accuracy = 0;
    let simulated = false;

    // 两条降级路径都落到模拟位置：
    // 1. 不在 Tauri 里（浏览器预览）—— 插件根本不存在
    // 2. 非移动端（桌面 Tauri）—— geolocation 插件是 mobile-only，
    //    桌面端会**静默返回 0,0** 而不报错，所以必须先判平台
    if (!isTauri() || !isTauriMobile()) {
      gcj = SIMULATED;
      accuracy = 25;
      simulated = true;
    } else {
      try {
        const geo = await import("@tauri-apps/plugin-geolocation");
        let perm = await geo.checkPermissions();
        if (perm.location === "prompt" || perm.location === "prompt-with-rationale") {
          perm = await geo.requestPermissions(["location"]);
        }
        if (perm.location !== "granted") {
          setDenied(true);
          setError("定位权限被拒绝");
          setLoading(false);
          return null;
        }

        const pos = await geo.getCurrentPosition();
        const wgs: GeoPoint = {
          lng: pos.coords.longitude,
          lat: pos.coords.latitude,
        };

        if (isNullIsland(wgs)) {
          // 插件静默失败（例如无 Google 服务的设备）
          gcj = SIMULATED;
          accuracy = 50;
          simulated = true;
          setError("系统定位不可用，正在使用模拟位置");
        } else {
          // ⚠️ 关键一步：WGS-84 → GCJ-02。少了它，武汉地区会偏 587 米
          gcj = await wgs84ToGcj02(wgs);
          accuracy = pos.coords.accuracy ?? 0;
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        setLoading(false);
        return null;
      }
    }

    if (!gcj) {
      setLoading(false);
      return null;
    }

    const nextFix: Fix = { lng: gcj.lng, lat: gcj.lat, accuracy, at: Date.now(), simulated };

    // 顺带查周边站点。失败不该让整个定位失败——坐标本身仍可用。
    let nearby: NearbyStop[] = [];
    try {
      nearby = await getNearbyStops(gcj);
    } catch (e) {
      // 浏览器预览模式下后端本来就不存在，App 顶部已有统一的横幅说明，
      // 这里再报一次只会造成同一条信息重复三遍。
      if (isTauri()) {
        setError(e instanceof Error ? e.message : String(e));
      }
    }

    setDenied(false);
    setFix({ ...gcj, accuracy, at: nextFix.at, simulated }, nearby);
    setLoading(false);
    return nextFix;
  }, [setFix]);

  return { fix, nearby: lastNearby, loading, error, denied, refresh };
}
