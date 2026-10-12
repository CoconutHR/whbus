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
  const clipId = useId().replace(/:/g, "");
  const w = 320;
  const h = variant === "detail" ? 34 : 26;
  const pad = 8;
  const usable = w - pad * 2;
  const total = totalMeters > 0 ? totalMeters : 1;

  const x = (m: number) => pad + (Math.max(0, Math.min(m, total)) / total) * usable;
  const y = h / 2;

  // 每条线路最多标 12 个站点刻度，避免小屏糊成一片
  const step = Math.max(1, Math.ceil(stops.length / 12));

  return (
    <div className={cx("timeline", className)}>
      <svg
        viewBox={`0 0 ${w} ${h}`}
        className="timeline__svg"
        preserveAspectRatio="none"
        aria-hidden
      >
        <defs>
          <clipPath id={`clip-${clipId}`}>
            <rect x="0" y="0" width={w} height={h} />
          </clipPath>
        </defs>

        <g clipPath={`url(#clip-${clipId})`}>
          {/* 背景轨：整条线路的"未驶过"状态 */}
          <line
            x1={pad}
            y1={y}
            x2={pad + usable}
            y2={y}
            stroke="var(--border-strong)"
            strokeWidth={2.5}
            strokeLinecap="round"
          />

          {/* 站点刻度 */}
          {stops
            .filter((_, i) => i % step === 0)
            .map((s) => (
              <circle
                key={s.stopNo}
                cx={x(s.at)}
                cy={y}
                r={variant === "detail" ? 3.2 : 2.6}
                fill="var(--surface)"
                stroke="var(--border-strong)"
                strokeWidth={1.6}
              />
            ))}

          {/* 用户位置：竖向刻度线，比圆点更醒目且不与车辆混淆 */}
          {userAt != null ? (
            <line
              x1={x(userAt)}
              y1={y - (variant === "detail" ? 11 : 8)}
              x2={x(userAt)}
              y2={y + (variant === "detail" ? 11 : 8)}
              stroke="var(--brand)"
              strokeWidth={2}
              strokeLinecap="round"
              opacity={0.85}
            />
          ) : null}

          {/* 车辆 */}
          {vehicles.map((v, i) => {
            const vx = x(v.along);
            const live = v.confidence === "live" || v.confidence === "delayed";
            const inferred = v.confidence === "inferred";

            // 已驶过段：从起点到车辆，实线不透明（历史是确定的）
            const passed = (
              <line
                key={`p-${i}`}
                x1={pad}
                y1={y}
                x2={vx}
                y2={y}
                stroke={
                  live ? "var(--pos-live-color)" : inferred ? "var(--pos-inferred-color)" : "var(--pos-stale-color)"
                }
                strokeWidth={2.5}
                strokeLinecap="round"
                opacity={live ? 1 : inferred ? 0.75 : 0.5}
              />
            );

            return (
              <g key={`v-${i}`}>
                {passed}

                {/* 车辆点 */}
                {live ? (
                  <>
                    <circle
                      cx={vx}
                      cy={y}
                      r={4}
                      fill="var(--pos-live-color)"
                      className="timeline__pulse"
                    />
                    <circle
                      cx={vx}
                      cy={y}
                      r={4.6}
                      fill="var(--surface)"
                      stroke="var(--pos-live-color)"
                      strokeWidth={2.4}
                    />
                  </>
                ) : (
                  /* 推测位置：虚线圆环 + 中心实心小点 —— 形状本身就传达不确定 */
                  <>
                    <circle
                      cx={vx}
                      cy={y}
                      r={5}
                      fill="var(--surface)"
                      stroke={inferred ? "var(--pos-inferred-color)" : "var(--pos-stale-color)"}
                      strokeWidth={2}
                      strokeDasharray={inferred ? "3 2.6" : "2 3"}
                      opacity={inferred ? 0.9 : 0.6}
                    />
                    <circle
                      cx={vx}
                      cy={y}
                      r={1.6}
                      fill={inferred ? "var(--pos-inferred-color)" : "var(--pos-stale-color)"}
                      opacity={inferred ? 0.9 : 0.6}
                    />
                  </>
                )}
              </g>
            );
          })}
        </g>
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
