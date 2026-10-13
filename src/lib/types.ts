/**
 * 与 Rust 侧 `types.rs` 一一对应的类型定义。
 *
 * 重要约定：**所有坐标（GeoPoint）都是 GCJ-02**。
 * 来自系统定位的 WGS-84 坐标必须先经 `wgs84ToGcj02()` 转换才能送入。
 * 武汉地区两者相差约 587 米，而 nearby 的搜索半径只有约 400 米。
 */

export interface GeoPoint {
  lng: number;
  lat: number;
}

/**
 * 位置可信度 —— 直接驱动 UI 上"实线 vs 虚线"的表达。
 *
 * - `live`      真实 GPS，< 3 分钟        → 实线、不透明
 * - `delayed`   真实 GPS，3-10 分钟       → 实线、降饱和
 * - `inferred`  由历史速度推测            → **虚线、半透明、必须带"推测"角标**
 * - `stale`     过旧或缺失，位置不可用    → 点线、极淡
 */
export type PositionConfidence = "live" | "delayed" | "inferred" | "stale";

export interface LineSummary {
  lineId: string;
  lineName: string;
  startStop: string;
  endStop: string;
  earlyHour: string;
  lastHour: string;
  price: string;
  runStatus: number | null;
}

export interface StopSummary {
  stopId: string;
  stopName: string;
  lat: number | null;
  lng: number | null;
}

export interface StopView {
  stopNo: number;
  stopId: string;
  stopName: string;
  /** 缺失或为 0,0 时为 null —— 上游数据里有这种脏值 */
  point: GeoPoint | null;
  /** 地铁换乘线路号 */
  subwayLines: string[];
}

export interface LineDetail {
  base: {
    lineId: string;
    lineName: string;
    direction: string | null;
    startStop: string;
    endStop: string;
    earlyHour: string;
    lastHour: string;
    price: string;
    lineType: number | null;
  };
  stops: StopView[];
  /** 相邻站点累计里程（米），长度与 stops 一致 */
  cumulativeMeters: number[];
  totalMeters: number;
}

/**
 * 车辆的行驶动力学状态。
 *
 * 上游只给一个 `gpsSpeed`，为 0 时既可能是**停在站台上下客**，
 * 也可能是**堵在路上等红灯**——两者对 ETA 的含义完全不同。
 * 后端依据"车辆与最近站点的距离"来区分。
 */
export type VehicleMotion = "moving" | "atStop" | "stopped" | "arrived" | "unknown";

export interface VehicleView {
  busId: string;
  /** 班次序号，仅用于排序与区分，**不是车牌** */
  busOrder: number | null;
  point: GeoPoint | null;
  speedKmh: number | null;
  /** 已吸附到线路走向上的里程（米） */
  alongM: number | null;
  /** 距线路走向的垂距（米）。过大说明 GPS 漂移 */
  offsetM: number | null;
  atStopNo: number | null;
  nextStopNo: number | null;
  arrived: boolean;
  gpsTime: string | null;
  ageSeconds: number | null;
  confidence: PositionConfidence;
  /** 行驶动力学：行驶中 / 停站 / 堵车 */
  motion: VehicleMotion;
  /** 距最近站点的距离（米），用于区分停站与堵车 */
  distanceToStopM: number | null;
}

export interface ArrivalView {
  stopId: string;
  stopNo: number;
  stopName: string;
  busId: string;
  busOrder: number | null;
  /** 距该站还有几站 */
  stopsAway: number;
  /** 沿线路剩余距离（米） */
  metersAway: number;
  etaMinLow: number;
  etaMinHigh: number;
  confidence: PositionConfidence;
}

export interface LineRealtime {
  lineId: string;
  lineName: string;
  startStop: string;
  endStop: string;
  stops: StopView[];
  cumulativeMeters: number[];
  totalMeters: number;
  vehicles: VehicleView[];
  arrivals: ArrivalView[];
  /** 是否有在线车辆。false **不等于错误** —— 收班/回场都是正常状态 */
  hasLiveVehicle: boolean;
  /** 是否处于运营时段内（依据首末班时间判定的本地结论） */
  inService: boolean;
  earlyHour: string;
  lastHour: string;
  /**
   * 尚未发班的班次（预计发车时刻 `HH:MM`，已升序）。
   *
   * 来自 `/bus/detailData` 的 `plans` 字段。注意它**在收班时段恒为空**，
   * 因此 UI 要把"没有待发班次"和"已收班"分成两种表达。
   */
  pendingDepartures: string[];
  /**
   * 由在线车辆班次号反推的**发车间隔**（分钟）。
   *
   * 用于回答"错过这班要等多久"。上游不提供完整时刻表，这是唯一的推算途径。
   */
  headwayMin: number | null;
}

export interface NearbyStop {
  stopId: string;
  stopName: string;
  point: GeoPoint | null;
  /** 本地重算的直线距离（米）。**不使用接口的 distance 字段**（实测单位是米而非文档说的公里） */
  distanceM: number;
  direction: string | null;
  lineNames: string[];
  lineIds: string[];
}

export interface ApiState {
  secretVersion: string | null;
  keyReady: boolean;
  serverHint: string | null;
  /** 累计请求数（本次运行内） */
  requestCount: number;
  /** 累计网络往返耗时（毫秒，不含人为限流等待） */
  requestMillis: number;
  /** 平均每次往返耗时（毫秒） */
  avgMillis: number;
}

export type LegKind = "walk" | "bus" | "wait" | "transfer";

export interface JourneyLeg {
  kind: LegKind;
  minutes: number;
  fromName: string;
  toName: string;
  meters: number | null;
  lineId: string | null;
  lineName: string | null;
  boardStopNo: number | null;
  alightStopNo: number | null;
  stopCount: number | null;
  startOffsetMin: number;
  endOffsetMin: number;
  vehicle: VehicleView | null;
  confidence: PositionConfidence;
}

export interface JourneyOption {
  totalMinutes: number;
  walkMeters: number;
  transfers: number;
  latestDepartOffsetMin: number;
  arriveOffsetMin: number;
  legs: JourneyLeg[];
  tags: string[];
  score: number;
}

export interface JourneyQuery {
  origin: GeoPoint;
  destination: GeoPoint;
  departAfterMin?: number;
  arriveBeforeMin?: number;
  walkSpeedMPerMin?: number;
  maxOptions?: number;
}

/** 收藏的线路 */
export interface FavoriteLine {
  lineId: string;
  lineName: string;
  startStop: string;
  endStop: string;
  /** 用户指定的上车站（可选，留空则用最近站） */
  boardStopId?: string | null;
  boardStopName?: string | null;
  addedAt: number;
}

/** 常用地点 */
export interface SavedPlace {
  kind: "home" | "work" | "other";
  label: string;
  point: GeoPoint;
  /** 取点方式，用于 UI 上提示可信度 */
  source: "gps" | "amap" | "manual";
  /** 高德返回的地址文本 */
  address?: string;
  /** 离家/公司最近的站点（由 nearBy 推断） */
  nearestStops?: NearbyStop[];
}

/**
 * 一条应用日志。
 *
 * 用于排查"暂时给不出方案"这类问题 —— 后端在规划的关键分支上
 * 都会写日志（候选线路数、站点表是否拉到、有无共同线路等），
 * 设置页可以直接查看与导出。
 */
export interface LogEntry {
  /** 毫秒时间戳 */
  ts: number;
  /** ERROR / WARN / INFO / DEBUG / TRACE */
  level: string;
  /** 模块，如 whbus_lib::commands */
  target: string;
  message: string;
}
