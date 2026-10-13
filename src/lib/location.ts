/**
 * 定位。
 *
 * # 坐标系陷阱（本项目的头号坑）
 *
 * 所有定位来源返回的都是 **WGS-84**，而公交 API 用 **GCJ-02**。
 * 武汉地区两者相差约 **587 米**，而 `nearBy` 的搜索半径只有约 **400 米**。
 * 偏移量大于搜索半径 ⇒ 不转换会查到**完全错误的站点**。
 *
 * 因此本模块的 `lng`/`lat` **始终是 GCJ-02**，转换在 Rust 侧完成。
 *
 * # 三条定位路径（按优先级降级）
 *
 * | 顺序 | 来源 | 依赖 | 适用 |
 * |---|---|---|---|
 * | 1 | `tauri-plugin-geolocation` | **Google Play Services** | 有 GMS 的设备，最快最准 |
 * | 2 | WebView `navigator.geolocation` | 安卓系统定位 | **无 GMS 的国产 ROM（华为等）** |
 * | 3 | 模拟位置 | 无 | 桌面开发 / 用户拒绝授权 |
 *
 * 之所以必须有第 2 条：官方插件的安卓实现里写死了
 * ```kotlin
 * if (GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(context) == SUCCESS) {
 *     // 用 FusedLocationProvider 定位
 * } else {
 *     errorCallback("Google Play Services unavailable.")   // 直接失败，不降级
 * }
 * ```
 * 也就是说 **GMS 不可用时它直接报错，不会退回安卓原生 LocationManager**。
 * 华为设备、以及禁用了 GMS 的国产 ROM 都会撞上这一点。
 * `navigator.geolocation` 走的是系统定位，与 GMS 无关，正好补上这个缺口。
 */

import { useCallback, useState } from "react";

import { getNearbyStops, isTauri, wgs84ToGcj02 } from "./api";
import { useAppStore } from "./store";
import type { GeoPoint, NearbyStop } from "./types";

/** 开发用模拟位置：武汉常青花园片区（已经是 GCJ-02） */
const SIMULATED: GeoPoint = { lng: 114.242448, lat: 30.636765 };

/** 定位来源。UI 上要如实告诉用户当前用的是哪一条。 */
export type FixSource = "plugin" | "webview" | "simulated";

export interface Fix {
  /** **GCJ-02** 经度 */
  lng: number;
  /** **GCJ-02** 纬度 */
  lat: number;
  /** 定位精度（米） */
  accuracy: number;
  /** 取到定位的时刻 */
  at: number;
  /** 定位来源 */
  source: FixSource;
  /** 用户拒绝了定位授权 */
  denied?: boolean;
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

function isTauriMobile(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
}

/** 判断坐标是否明显是无效的 0,0（桌面插件的静默返回值） */
function isNullIsland(p: GeoPoint): boolean {
  return Math.abs(p.lng) < 0.5 && Math.abs(p.lat) < 0.5;
}

/** WGS-84 的原始定位结果 */
interface RawFix {
  wgs: GeoPoint;
  accuracy: number;
}

/* ------------------------------------------------------------------ */
/* 路径 1：Tauri 官方插件（依赖 Google Play Services）                  */
/* ------------------------------------------------------------------ */

async function viaTauriPlugin(): Promise<RawFix> {
  const geo = await import("@tauri-apps/plugin-geolocation");

  let perm = await geo.checkPermissions();
  if (perm.location === "prompt" || perm.location === "prompt-with-rationale") {
    perm = await geo.requestPermissions(["location"]);
  }
  if (perm.location !== "granted") {
    const e = new Error("定位权限被拒绝");
    (e as Error & { denied?: boolean }).denied = true;
    throw e;
  }

  const pos = await geo.getCurrentPosition({
    enableHighAccuracy: true,
    timeout: 12_000,
    maximumAge: 30_000,
  });
  const wgs: GeoPoint = {
    lng: pos.coords.longitude,
    lat: pos.coords.latitude,
  };
  if (isNullIsland(wgs)) {
    // 插件静默失败（无 GMS 时也可能走到这里）
    throw new Error("插件返回了无效坐标 (0,0)");
  }
  return { wgs, accuracy: pos.coords.accuracy ?? 0 };
}

/* ------------------------------------------------------------------ */
/* 路径 2：WebView 的 navigator.geolocation（走安卓系统定位，与 GMS 无关）*/
/* ------------------------------------------------------------------ */

function viaWebViewGeolocation(): Promise<RawFix> {
  return new Promise((resolve, reject) => {
    const g = typeof navigator !== "undefined" ? navigator.geolocation : undefined;
    if (!g || typeof g.getCurrentPosition !== "function") {
      reject(new Error("此环境不提供 navigator.geolocation"));
      return;
    }

    // 先尝试高精度，失败或不支持时退回低精度
    const attempt = (highAccuracy: boolean) => {
      g.getCurrentPosition(
        (pos) => {
          const wgs: GeoPoint = {
            lng: pos.coords.longitude,
            lat: pos.coords.latitude,
          };
          if (isNullIsland(wgs)) {
            reject(new Error("WebView 定位返回了无效坐标"));
            return;
          }
          resolve({ wgs, accuracy: pos.coords.accuracy ?? 0 });
        },
        (err) => {
          // 高精度失败（常见于室内）时用低精度再试一次
          if (highAccuracy) {
            attempt(false);
            return;
          }
          const e = new Error(
            err.code === err.PERMISSION_DENIED
              ? "定位权限被拒绝"
              : err.code === err.TIMEOUT
                ? "定位超时（室内可能取不到信号）"
                : "系统定位不可用",
          );
          if (err.code === err.PERMISSION_DENIED) {
            (e as Error & { denied?: boolean }).denied = true;
          }
          reject(e);
        },
        {
          enableHighAccuracy: highAccuracy,
          timeout: highAccuracy ? 12_000 : 15_000,
          maximumAge: 30_000,
        },
      );
    };

    attempt(true);
  });
}

/* ------------------------------------------------------------------ */
/* 主流程                                                              */
/* ------------------------------------------------------------------ */

export function useLocation(): UseLocation {
  const { lastFix, lastNearby, setFix } = useAppStore();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [denied, setDenied] = useState(false);

  // 跨 Tab 共享上次定位结果，避免每次切页都重新定位。
  // `source` 必须一并持久化：早期版本在这里写死，导致桌面端重新加载后
  // 模拟坐标被当成"已定位"展示，精度与来源都是错的。
  const fix: Fix | null = lastFix;

  const refresh = useCallback(async (): Promise<Fix | null> => {
    setLoading(true);
    setError(null);

    let raw: RawFix | null = null;
    let source: FixSource = "simulated";
    const failures: string[] = [];
    let wasDenied = false;

    if (!isTauri() || !isTauriMobile()) {
      // 桌面 / 浏览器：两条真机路径都不可用，直接用模拟位置
      raw = null;
      source = "simulated";
    } else {
      // ---- 路径 1：官方插件 ----
      try {
        raw = await viaTauriPlugin();
        source = "plugin";
      } catch (e) {
        const err = e as Error & { denied?: boolean };
        if (err.denied) wasDenied = true;
        failures.push(`插件：${err.message}`);
        console.warn("通过官方插件定位失败，尝试 WebView 定位：", err.message);
      }

      // ---- 路径 2：WebView navigator.geolocation ----
      // 这一条是给无 GMS 设备准备的（华为等国产 ROM）。
      if (!raw) {
        try {
          raw = await viaWebViewGeolocation();
          source = "webview";
        } catch (e) {
          const err = e as Error & { denied?: boolean };
          if (err.denied) wasDenied = true;
          failures.push(`系统定位：${err.message}`);
          console.warn("WebView 定位也失败：", err.message);
        }
      }

      // ---- 路径 3：模拟位置兜底 ----
      if (!raw) {
        raw = { wgs: SIMULATED, accuracy: 100 };
        source = "simulated";
      }
    }

    if (!raw) {
      raw = { wgs: SIMULATED, accuracy: 100 };
      source = "simulated";
    }

    // ⚠️ 关键一步：WGS-84 → GCJ-02。少了它，武汉地区会偏 587 米。
    //
    // 注意 source === "simulated" 时 SIMULATED 本身就是 GCJ-02，
    // 不能重复转换（那会再偏一次）。
    let gcj: GeoPoint;
    if (source === "simulated") {
      gcj = SIMULATED;
    } else {
      try {
        gcj = await wgs84ToGcj02(raw.wgs);
      } catch (e) {
        // 坐标转换依赖 Rust 后端；万一不可用，宁可报错也不要用错坐标
        setError(e instanceof Error ? e.message : String(e));
        setLoading(false);
        return null;
      }
    }

    const nextFix: Fix = {
      lng: gcj.lng,
      lat: gcj.lat,
      accuracy: raw.accuracy,
      at: Date.now(),
      source,
      ...(wasDenied ? { denied: true } : {}),
    };

    // 顺带查周边站点。失败不该让整个定位失败——坐标本身仍可用。
    let nearby: NearbyStop[] = [];
    try {
      nearby = await getNearbyStops(gcj);
    } catch (e) {
      // 浏览器预览模式下后端本来就不存在，App 顶部已有统一横幅说明，
      // 这里再报一次只会造成同一条信息重复三遍。
      if (isTauri()) {
        setError(e instanceof Error ? e.message : String(e));
      }
    }

    setDenied(wasDenied);
    setFix(
      {
        ...gcj,
        accuracy: nextFix.accuracy,
        at: nextFix.at,
        source: nextFix.source,
        denied: wasDenied,
      },
      nearby,
    );

    // 走到模拟位置时给出可读的原因，而不是静默降级
    if (source === "simulated" && failures.length > 0) {
      setError(
        `未能获取真实定位（${failures.join("；")}）。` +
          "当前显示的是模拟位置；也可以到「规划」页按地址搜索直接设置家与公司。",
      );
    }

    setLoading(false);
    return nextFix;
  }, [setFix]);

  return { fix, nearby: lastNearby, loading, error, denied, refresh };
}

/** 供 UI 展示定位来源的中文说明 */
export function sourceLabel(source: FixSource | undefined): string {
  switch (source) {
    case "plugin":
      return "系统定位";
    case "webview":
      return "系统定位（兼容模式）";
    case "simulated":
      return "模拟位置";
    default:
      return "未定位";
  }
}
