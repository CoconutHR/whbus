/**
 * Rust 后端调用封装。
 *
 * 前端不直接发任何网络请求：
 * - 公交 API 的 SM4 密钥每日轮换，绝不能进入 WebView
 * - 第三方 API 无 CORS 头，WebView 直连必然被拦
 * - 限流与退避集中在 Rust 侧才好控制
 *
 * 因此这里只做 `invoke` 的薄封装 + 错误信息规整。
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  ApiState,
  GeoPoint,
  JourneyOption,
  JourneyQuery,
  LineDetail,
  LineRealtime,
  LineSummary,
  NearbyStop,
  StopSummary,
  LogEntry,
} from "./types";

/**
 * 是否运行在 Tauri WebView 里。
 *
 * Tauri 只在自己的 WebView 中注入 `window.__TAURI_INTERNALS__`。
 * 用浏览器直开 `pnpm dev` 时它不存在，所有 `invoke` 都会抛出
 * `TypeError: Cannot read properties of undefined (reading 'invoke')` ——
 * 这个信息对开发者毫无帮助，所以这里主动识别并给出可操作的提示。
 *
 * 注意：UI 本身在浏览器里是能正常渲染的（定位走模拟位置、
 * 数据走降级路径），因此这里**不阻止渲染**，只把错误说清楚。
 */
export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ !==
      "undefined"
  );
}

/** 浏览器预览模式下的统一提示 */
export const BROWSER_MODE_HINT =
  "当前在浏览器预览模式：界面可以正常查看，但公交数据需要 Tauri 后端。" +
  "请用 `pnpm tauri android dev`（真机）或直接安装 APK 体验完整功能。";

/** 判断一个错误是否由"不在 Tauri 里"引起 */
function isMissingTauri(e: unknown): boolean {
  const s = e instanceof Error ? e.message : String(e);
  return s.includes("invoke") && (s.includes("undefined") || s.includes("__TAURI"));
}

/** 后端返回的错误都是字符串（Rust 侧 `AppError` 已实现 `Display`） */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error(BROWSER_MODE_HINT);
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    if (isMissingTauri(e)) {
      throw new Error(BROWSER_MODE_HINT);
    }
    // 把 Rust 的错误变成可读的 Error，便于 UI 统一处理
    throw new Error(typeof e === "string" ? e : String(e));
  }
}

/* ----------------------------------------------------------- 密钥 / 诊断 */

export const getApiState = () => call<ApiState>("get_api_state");

/* ----------------------------------------------------------------- 搜索 */

/**
 * 线路号检索。
 *
 * ⚠️ 上游是模糊匹配：搜 "72" 会返回整个 72x 系列。
 * ⚠️ 实测：**收班后该接口整体返回空**（凌晨连 "715" 都搜不到），
 *    这是服务端时段行为而非故障——UI 需要据此走缓存降级路径。
 */
export const searchLines = (keyword: string) =>
  call<LineSummary[]>("search_lines", { keyword });

/**
 * 站名检索。
 *
 * ⚠️ 只匹配**站名子串**，不认地址、小区名、写字楼名。
 *    如"常青花园" ✅ / "楚天雅苑" ❌。地址转坐标请走高德 Web API。
 */
export const searchStops = (keyword: string) =>
  call<StopSummary[]>("search_stops", { keyword });

/* ----------------------------------------------------------------- 线路 */

export const getLineDetail = (lineId: string) =>
  call<LineDetail>("get_line_detail", { lineId });

export const getLineVehicles = (lineId: string) =>
  call<LineRealtime>("get_line_vehicles", { lineId });

/* ----------------------------------------------------------------- 站点 */

/** `point` 必须是 **GCJ-02** */
export const getNearbyStops = (point: GeoPoint) =>
  call<NearbyStop[]>("get_nearby_stops", { point });

export const getStopArrivals = (stopId: string) =>
  call<unknown>("get_stop_arrivals", { stopId });

/* --------------------------------------------------------------- 坐标转换 */

/** WGS-84 → GCJ-02。**定位出口必用** —— 不转换会有约 587 米偏移 */
export const wgs84ToGcj02 = (point: GeoPoint) =>
  call<GeoPoint>("wgs84_to_gcj02_cmd", { point });

export const gcj02ToWgs84 = (point: GeoPoint) =>
  call<GeoPoint>("gcj02_to_wgs84_cmd", { point });

/* ----------------------------------------------------------------- 规划 */

export const planJourney = (query: JourneyQuery) =>
  call<JourneyOption[]>("plan_journey", { query });

/* ------------------------------------------------- 高德：地址搜索与步行 */

export interface AmapKeyStatus {
  configured: boolean;
  /** 形如 `c269…b9f`，仅用于确认填对了 */
  masked: string | null;
}

export interface Place {
  name: string;
  address: string;
  /** **GCJ-02**，可直接喂给 getNearbyStops */
  point: GeoPoint;
  district: string;
  /** POI 类型，如"商务住宅;住宅区;住宅小区" */
  kind: string | null;
}

export interface WalkingRoute {
  meters: number;
  minutes: number;
}

export const getAmapKeyStatus = () => call<AmapKeyStatus>("get_amap_key_status");

/** 设置高德 Key。传空字符串表示清除。Key 保存在应用私有目录，不进版本库。 */
export const setAmapKey = (key: string) =>
  call<AmapKeyStatus>("set_amap_key", { key });

/**
 * 按关键词搜索地点。
 *
 * 这是解决"公交接口搜不到小区名/写字楼名"的核心能力：
 * `楚天雅苑` / `洪山科创大厦` 这类输入公交接口返回空，高德能定位。
 */
export const searchPlace = (keyword: string, city?: string) =>
  call<Place[]>("search_place", { keyword, city: city ?? null });

/** 步行路径规划（真实路径，替代直线距离估算） */
export const walkingRoute = (from: GeoPoint, to: GeoPoint) =>
  call<WalkingRoute>("walking_route", { from, to });

/** 坐标 → 地址 */
export const reverseGeocode = (point: GeoPoint) =>
  call<Place | null>("reverse_geocode", { point });

// ---------------------------------------------------------------------------
// 日志
// ---------------------------------------------------------------------------

/** 读取最近的应用日志（最新的在最后） */
export const getLogs = (limit?: number) =>
  call<LogEntry[]>("get_logs", { limit: limit ?? null });

/** 清空内存中的日志缓冲（磁盘文件保留） */
export const clearLogs = () => call<void>("clear_logs");

/**
 * 把日志导出成文本文件，返回文件路径。
 *
 * 返回路径而不是内容：日志可能几百 KB，走 IPC 传字符串不划算；
 * 拿到路径后可以直接交给系统的分享功能。
 */
export const exportLogs = () => call<string>("export_logs");

/**
 * 预热若干条线路的站点表（打开 App 时后台调用）。
 *
 * 返回成功载入的方向数；**失败不代表功能异常**，
 * 只是没有预热、首次规划仍需等一次冷启动。
 */
export const preloadLines = (lineNames: string[], maxLines?: number) =>
  call<number>("preload_lines", { lineNames, maxLines: maxLines ?? null });
