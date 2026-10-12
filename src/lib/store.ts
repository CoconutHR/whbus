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
  lastFix: (GeoPoint & { accuracy: number; at: number }) | null;
  lastNearby: NearbyStop[];
  setFix: (
    fix: (GeoPoint & { accuracy: number; at: number }) | null,
    nearby: NearbyStop[],
  ) => void;
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
