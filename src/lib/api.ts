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
} from "./types";

/** 后端返回的错误都是字符串（Rust 侧 `AppError` 已实现 `Display`） */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
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
