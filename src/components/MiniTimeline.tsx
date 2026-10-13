/**
 * 微型线路时间轴 —— 本 App 的签名视觉。
 *
 * 复用三处：首页线路卡 / 线路详情 / 方案时间轴。
 *
 * # 编码规则（全部来自设计规格）
 *
 * | 元素 | 真实 GPS | 推测位置 |
 * |---|---|---|
 * | 已驶过段 | 实线、不透明、品牌色 | 同左（历史是确定的） |
 * | 未驶过段 | 实线、不透明、淡色 | **虚线 + 半透明** |
 * | 车辆点 | 实心圆 + 脉冲光环 | **虚线圆环 + 中心小点 + "推测"角标** |
 *
 * 关键无障碍要求：**不能只靠颜色区分**。已过站用实心灰、未来站用空心描边、
 * 当前位置用实心品牌色 + 脉冲，三者在灰度下依然可辨。
 * 脉冲动画遵守 `prefers-reduced-motion`（在 global.css 里统一降级为静态圆环）。
 */

import { useId } from "react";

import type { PositionConfidence } from "../lib/types";
import { cx } from "../lib/utils";
import "./MiniTimeline.css";

export interface TimelineStop {
  stopNo: number;
  stopName: string;
  /** 累计里程（米） */
  at: number;
}

export interface TimelineVehicle {
  /** 沿线里程（米） */
  along: number;
  confidence: PositionConfidence;
  label?: string;
}

export function MiniTimeline({
  stops,
  totalMeters,
  vehicles = [],
  userAt,
  /** 紧凑模式用于列表卡；展开模式用于详情页 */
  variant = "compact",
  onStopClick,
  className,
}: {
  stops: TimelineStop[];
  totalMeters: number;
  vehicles?: TimelineVehicle[];
  /** 用户所在站点的里程（米） */
  userAt?: number;
  variant?: "compact" | "detail";
  onStopClick?: (stopNo: number) => void;
  className?: string;
}) {
  // useId 给 SVG 内的 marker 生成唯一 id，避免同页多个时间轴互相覆盖
  const markerId = useId().replace(/:/g, "");

  const w = 320;
  const h = variant === "detail" ? 34 : 26;
  const pad = 10;
  const rail = w - pad * 2;
  const total = totalMeters > 0 ? totalMeters : 1;
  const y = h / 2;

  const x = (m: number) => pad + (Math.max(0, Math.min(m, total)) / total) * rail;

  // 每条线路最多标 12 个站点刻度，避免小屏糊成一片
  const step = Math.max(1, Math.ceil(stops.length / 12));

  return (
    <div className={cx("timeline", className)}>
      {/*
        渲染要点（这几条都是修过 bug 的，别改回去）：

        1. `vector-effect="non-scaling-stroke"` 是必需的。
           viewBox 宽固定 320，而实际渲染宽度随卡片变化（约 340-380px）；
           配合 preserveAspectRatio="none" 会产生**非等比缩放**，
           把描边横向拉粗、把圆点拉成椭圆 —— 看上去就是"一段一段的短横线"。
           加上这个属性后描边宽度不再随缩放变化。

        2. 站点刻度改为 r=2 + 细边（1.25）。
           早期是 r=2.6 + 1.6 描边，在 26px 高的紧凑模式里
           圆点会被描边吃掉大半，糊成一串短横。

        3. 车辆点带一圈底色，压在轨道上也看得清。

        4. 轨道末端加箭头，明确"往哪个方向开" —— 光一条横线看不出方向。
      */}
      <svg
        viewBox={`0 0 ${w} ${h}`}
        className="timeline__svg"
        preserveAspectRatio="none"
        aria-hidden
      >
        <defs>
          <marker
            id={`arrow-${markerId}`}
            viewBox="0 0 6 6"
            refX="5"
            refY="3"
            markerWidth="5"
            markerHeight="5"
            orient="auto"
          >
            <path d="M0.5 1 L5 3 L0.5 5 z" fill="var(--text-3)" />
          </marker>
        </defs>

        {/* ---- 第 1 层：整条线路的底轨 ---- */}
        <line
          x1={pad}
          y1={y}
          x2={pad + rail}
          y2={y}
          stroke="var(--border-strong)"
          strokeWidth={3}
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
          markerEnd={`url(#arrow-${markerId})`}
        />

        {/* ---- 第 2 层：站点刻度 ---- */}
        {stops
          .filter((_, i) => i % step === 0)
          .map((s) => (
            <circle
              key={s.stopNo}
              cx={x(s.at)}
              cy={y}
              r={2}
              fill="var(--surface)"
              stroke="var(--border-strong)"
              strokeWidth={1.25}
              vectorEffect="non-scaling-stroke"
            />
          ))}

        {/* ---- 第 3 层：用户位置（竖向刻度，不与车辆混淆）---- */}
        {userAt != null ? (
          <line
            x1={x(userAt)}
            y1={y - (variant === "detail" ? 10 : 7)}
            x2={x(userAt)}
            y2={y + (variant === "detail" ? 10 : 7)}
            stroke="var(--brand)"
            strokeWidth={2}
            strokeLinecap="round"
            vectorEffect="non-scaling-stroke"
          />
        ) : null}

        {/* ---- 第 4 层：车辆 ---- */}
        {vehicles.map((v, i) => {
          const vx = x(v.along);
          const live = v.confidence === "live" || v.confidence === "delayed";
          const inferred = v.confidence === "inferred";
          const color = live
            ? "var(--pos-live-color)"
            : inferred
              ? "var(--pos-inferred-color)"
              : "var(--pos-stale-color)";

          return (
            <g key={`v-${i}`}>
              {/* 已驶过段：从起点到车辆，颜色随可信度 */}
              <line
                x1={pad}
                y1={y}
                x2={vx}
                y2={y}
                stroke={color}
                strokeWidth={3}
                strokeLinecap="round"
                opacity={live ? 1 : inferred ? 0.75 : 0.5}
                vectorEffect="non-scaling-stroke"
              />

              {live ? (
                <>
                  {/* 脉冲光环：只对真实位置显示 */}
                  <circle cx={vx} cy={y} r={4} fill={color} className="timeline__pulse" />
                  {/* 底色外圈，保证压在轨道上也看得清 */}
                  <circle cx={vx} cy={y} r={5} fill="var(--surface)" />
                  <circle cx={vx} cy={y} r={3.4} fill={color} />
                </>
              ) : (
                <>
                  {/* 推测位置：虚线圆环 + 中心点，**形状**本身就传达不确定 */}
                  <circle cx={vx} cy={y} r={5} fill="var(--surface)" />
                  <circle
                    cx={vx}
                    cy={y}
                    r={3.6}
                    fill="none"
                    stroke={color}
                    strokeWidth={1.75}
                    strokeDasharray={inferred ? "3 2.4" : "2 2.8"}
                    opacity={inferred ? 0.9 : 0.6}
                    vectorEffect="non-scaling-stroke"
                  />
                  <circle cx={vx} cy={y} r={1.3} fill={color} opacity={inferred ? 0.9 : 0.6} />
                </>
              )}
            </g>
          );
        })}
      </svg>

      {/* 站点点击热区（详情模式才可交互） */}
      {variant === "detail" && onStopClick ? (
        <div className="timeline__hit" aria-hidden>
          {stops.map((s) => (
            <button
              key={s.stopNo}
              type="button"
              className="timeline__hit-item"
              style={{ left: `${(x(s.at) / w) * 100}%` }}
              onClick={() => onStopClick(s.stopNo)}
              tabIndex={-1}
              aria-label={s.stopName}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

/**
 * 时间轴图例。
 *
 * 虚实线的含义必须显式说明——否则用户会把推测位置当成真实位置，
 * 这是信任问题，比视觉美观重要得多。
 */
export function TimelineLegend({ inferred = false }: { inferred?: boolean }) {
  return (
    <div className="timeline-legend t-cap t-3">
      <span className="timeline-legend__item">
        <svg width="18" height="8" aria-hidden>
          <line x1="0" y1="4" x2="18" y2="4" stroke="var(--pos-live-color)" strokeWidth="2.5" />
        </svg>
        实时位置
      </span>
      {inferred ? (
        <span className="timeline-legend__item">
          <svg width="18" height="8" aria-hidden>
            <line
              x1="0"
              y1="4"
              x2="18"
              y2="4"
              stroke="var(--pos-inferred-color)"
              strokeWidth="2"
              strokeDasharray="4 3"
            />
          </svg>
          推测位置
        </span>
      ) : null}
      <span className="timeline-legend__item">
        <svg width="8" height="10" aria-hidden>
          <line x1="4" y1="0" x2="4" y2="10" stroke="var(--brand)" strokeWidth="2" />
        </svg>
        你的位置
      </span>
    </div>
  );
}
