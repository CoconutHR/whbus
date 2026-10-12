/** 轻量工具函数集合。刻意不引入 lodash 等依赖——安卓包体越小越好。 */

/** 条件类名拼接 */
export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

/** 把分钟数格式化为"约 6 分钟" / "约 1 小时 5 分" */
export function formatMinutes(min: number): string {
  if (!Number.isFinite(min)) return "—";
  if (min < 1) return "不到 1 分钟";
  if (min < 60) return `${Math.round(min)} 分钟`;
  const h = Math.floor(min / 60);
  const m = Math.round(min % 60);
  return m === 0 ? `${h} 小时` : `${h} 小时 ${m} 分`;
}

/**
 * ETA 区间格式化。
 *
 * 刻意保留区间而非取中点：没有班次计划表时 ETA 本质是概率，
 * 给一个假装精确的数字会误导用户。区间宽度本身就是"数据可信度"的表达。
 */
export function formatEtaRange(low: number, high: number): string {
  if (!Number.isFinite(low) || !Number.isFinite(high)) return "—";
  const l = Math.max(0, Math.round(low));
  const h = Math.max(l, Math.round(high));
  if (h === 0) return "即将到站";
  if (l === h) return `约 ${h} 分钟`;
  // 区间很窄时不必显示两个数字
  if (h - l <= 1) return `约 ${h} 分钟`;
  return `约 ${l}-${h} 分钟`;
}

/** 把"分钟偏移"格式化为时钟时刻，如 18:42 */
export function formatClock(base: Date, offsetMin: number): string {
  const d = new Date(base.getTime() + offsetMin * 60_000);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** 当前时间的 HH:MM */
export function nowHHMM(d = new Date()): string {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** 数据新鲜度的文字表达 */
export function formatAge(ageSeconds: number | null | undefined): string {
  if (ageSeconds == null) return "时间未知";
  if (ageSeconds < 20) return "刚刚更新";
  if (ageSeconds < 60) return `${ageSeconds} 秒前`;
  const min = Math.floor(ageSeconds / 60);
  if (min < 60) return `${min} 分钟前`;
  return `${Math.floor(min / 60)} 小时前`;
}

/** 距离格式化：米 / 公里 */
export function formatDistance(meters: number): string {
  if (!Number.isFinite(meters)) return "—";
  if (meters < 1000) return `${Math.round(meters)} 米`;
  return `${(meters / 1000).toFixed(1)} 公里`;
}

/** 限制数值范围 */
export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

/**
 * 由线路号稳定映射到一个线路色变量名。
 *
 * 上游不提供品牌色，我们也不假装有——用哈希稳定分配，
 * 保证同一条线路在任何页面颜色一致。
 */
export function lineColorVar(lineName: string): string {
  let h = 0;
  for (let i = 0; i < lineName.length; i++) {
    h = (h * 31 + lineName.charCodeAt(i)) >>> 0;
  }
  return `var(--line-${(h % 8) + 1})`;
}
