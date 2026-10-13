/**
 * 应用状态。刻意用 localStorage 而非 tauri-plugin-store：
 * 需要持久化的只有收藏与常用地点这类小数据，localStorage 零依赖、同步读取，
 * 能避免首屏出现"加载中"闪烁。
 */

import { create } from "zustand";

import type { FavoriteLine, GeoPoint, NearbyStop, SavedPlace } from "./types";

const LS_FAVS = "whbus.favorites";
const LS_PLACES = "whbus.places";
const LS_RECENT = "whbus.recentPlaces";

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 隐私模式下可能失败，静默忽略——不该因为存储失败而崩 UI */
  }
}

interface AppStore {
  /* ---- 收藏线路 ---- */
  favorites: FavoriteLine[];
  isFavorite: (lineId: string) => boolean;
  toggleFavorite: (line: FavoriteLine) => void;
  setFavoriteBoardStop: (
    lineId: string,
    stopId: string | null,
    stopName: string | null,
  ) => void;

  /* ---- 常用地点 ---- */
  places: Partial<Record<"home" | "work" | "other", SavedPlace>>;
  setPlace: (place: SavedPlace) => void;
  clearPlace: (kind: "home" | "work" | "other") => void;

  /* ---- 最近去过的地点（用于规划页快捷入口）---- */
  recentPlaces: SavedPlace[];
  pushRecentPlace: (p: SavedPlace) => void;

  /* ---- 上次定位结果（跨 Tab 共享，避免重复定位）---- */
  lastFix:
    | (GeoPoint & {
        accuracy: number;
        at: number;
        /** 定位来源。必须持久化，否则重新加载后无法如实说明坐标是怎么来的 */
        source: "plugin" | "webview" | "simulated";
        /** 用户拒绝了授权 */
        denied?: boolean;
      })
    | null;
  lastNearby: NearbyStop[];
  setFix: (
    fix:
      | (GeoPoint & {
          accuracy: number;
          at: number;
          source: "plugin" | "webview" | "simulated";
          denied?: boolean;
        })
      | null,
    nearby: NearbyStop[],
  ) => void;

  /* ---- 采纳的出行方案（追踪模式）---- */
  tracking: Tracking | null;
  setTracking: (t: Tracking | null) => void;

  /* ---- 当前打开的线路详情（全 App 单一来源）---- */
  /**
   * 线路详情是全屏覆盖层，不占 Tab。
   *
   * 放在 store 而不是各屏局部 state，是因为追踪条（App 层）与首页
   * （HomeScreen 层）都需要打开它。两处各存一份会导致"从追踪条点开的
   * 详情页"和"从首页点开的"互相不可见。
   */
  openLineId: string | null;
  setOpenLineId: (id: string | null) => void;

  /* ---- 触觉反馈 ---- */
  haptic: (style: "light" | "medium" | "heavy") => Promise<void>;
}

/**
 * 已采纳的出行方案。
 *
 * 采纳后 App 进入"追踪模式"：Tab 栏上方出现常驻细条。
 * 这是 App 的**第二状态**，不是第四层页面——用户采纳完必须知道下一步看哪，
 * 否则会迷失在四个 Tab 里。
 */
export interface Tracking {
  /** 如"回家" */
  label: string;
  /** 终点名，如"家" */
  toLabel: string;
  /** 相对采纳时刻的预计到达分钟数 */
  arriveOffsetMin: number;
  acceptedAt: number;
  /**
   * 方案涉及的线路。
   *
   * ⚠️ 必须同时存 `id`，不能只存线路号。
   * 早期只存了名字（`lineNames`），点追踪条时只能切到「线路」Tab
   * 而打不开任何详情页 —— 用户看到的就是一片空白。
   * 而 `LineDetailView` 需要的是 `420100-687-0` 这种**带方向的 id**，
   * 光有线路号查不出数据。
   */
  lines: Array<{ id: string; name: string }>;
  /** 已到达（用户手动结束或时间已过） */
  done?: boolean;
}

export const useAppStore = create<AppStore>((set, get) => ({
  favorites: load<FavoriteLine[]>(LS_FAVS, []),

  isFavorite: (lineId) => get().favorites.some((f) => f.lineId === lineId),

  toggleFavorite: (line) => {
    const cur = get().favorites;
    const exists = cur.some((f) => f.lineId === line.lineId);
    const next = exists
      ? cur.filter((f) => f.lineId !== line.lineId)
      : [...cur, line];
    save(LS_FAVS, next);
    set({ favorites: next });
  },

  setFavoriteBoardStop: (lineId, stopId, stopName) => {
    const next = get().favorites.map((f) =>
      f.lineId === lineId
        ? { ...f, boardStopId: stopId, boardStopName: stopName }
        : f,
    );
    save(LS_FAVS, next);
    set({ favorites: next });
  },

  places: load<Partial<Record<"home" | "work" | "other", SavedPlace>>>(LS_PLACES, {}),
  setPlace: (place) => {
    const next = { ...get().places, [place.kind]: place };
    save(LS_PLACES, next);
    set({ places: next });
  },
  clearPlace: (kind) => {
    const next = { ...get().places };
    delete next[kind];
    save(LS_PLACES, next);
    set({ places: next });
  },

  recentPlaces: load<SavedPlace[]>(LS_RECENT, []),
  pushRecentPlace: (p) => {
    const dedup = get().recentPlaces.filter(
      (r) => !(Math.abs(r.point.lng - p.point.lng) < 1e-5 && Math.abs(r.point.lat - p.point.lat) < 1e-5),
    );
    const next = [p, ...dedup].slice(0, 8);
    save(LS_RECENT, next);
    set({ recentPlaces: next });
  },

  lastFix: null,
  lastNearby: [],
  setFix: (fix, nearby) => set({ lastFix: fix, lastNearby: nearby }),

  tracking: null,
  setTracking: (t) => set({ tracking: t }),

  openLineId: null,
  setOpenLineId: (id) => set({ openLineId: id }),

  /**
   * 触觉反馈。
   *
   * 只在三个时刻使用：采纳方案、车辆即将到站、收藏成功。
   * 桌面端没有触觉，静默忽略（不报错、不弹提示）。
   */
  haptic: async (style) => {
    if (typeof navigator === "undefined" || !/Android|iPhone|iPad|iPod/i.test(navigator.userAgent)) {
      return;
    }
    try {
      const h = await import("@tauri-apps/plugin-haptics");
      switch (style) {
        case "light":
          await h.impactFeedback("light");
          break;
        case "medium":
          await h.impactFeedback("medium");
          break;
        case "heavy":
          await h.impactFeedback("heavy");
          break;
      }
    } catch {
      /* 设备不支持触觉时忽略 */
    }
  },
}));

/** 单独抽出以便将来换存储实现 */
/**
 * 判断一个坐标是否落在某个已保存地点附近。
 *
 * 用于"智能展示"：站在公司附近就推"回家"的方案。
 * 阈值取 250m —— 比 GPS 漂移大、比城市街区小，是可靠的分界。
 */
export function nearPlace(
  point: GeoPoint,
  place: SavedPlace | undefined,
  thresholdM = 250,
): boolean {
  if (!place) return false;
  const R = 6_372_792;
  const rad = (d: number) => (d * Math.PI) / 180;
  const dLat = rad(place.point.lat - point.lat);
  const dLng = rad(place.point.lng - point.lng);
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(rad(point.lat)) * Math.cos(rad(place.point.lat)) * Math.sin(dLng / 2) ** 2;
  const meters = 2 * R * Math.asin(Math.sqrt(Math.min(1, a)));
  return meters <= thresholdM;
}
