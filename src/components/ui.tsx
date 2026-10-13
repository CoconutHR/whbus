/** 通用 UI 原语。刻意保持极小集合——每个都对应设计规格里的一个明确角色。 */

import type { ReactNode } from "react";

import { cx } from "../lib/utils";
import { IconAlert, IconClose, IconEmpty, IconInfo, IconOffline } from "./icons";
import "./ui.css";

/* ---------------------------------------------------------------- 卡片 */

export function Card({
  children,
  /** 唯一的浮起卡片：首页决策卡。别处不要用 */
  raised,
  className,
}: {
  children: ReactNode;
  raised?: boolean;
  className?: string;
}) {
  return (
    <div className={cx("card", raised && "card--raised", className)}>{children}</div>
  );
}

/* ---------------------------------------------------------------- 按钮 */

export function Button({
  children,
  variant = "primary",
  block,
  size,
  onClick,
  disabled,
  type = "button",
  ariaLabel,
}: {
  children: ReactNode;
  variant?: "primary" | "accent" | "ghost" | "quiet";
  block?: boolean;
  size?: "sm";
  onClick?: () => void;
  disabled?: boolean;
  type?: "button" | "submit";
  ariaLabel?: string;
}) {
  return (
    <button
      type={type}
      className={cx("btn", `btn--${variant}`, block && "btn--block", size === "sm" && "btn--sm")}
      onClick={onClick}
      disabled={disabled}
      aria-label={ariaLabel}
    >
      {children}
    </button>
  );
}

/* ---------------------------------------------------------------- 标签 */

export function Chip({
  children,
  tone = "neutral",
  className,
}: {
  children: ReactNode;
  tone?: "neutral" | "brand" | "accent" | "live" | "warn" | "danger";
  className?: string;
}) {
  return <span className={cx("chip", `chip--${tone}`, className)}>{children}</span>;
}

/**
 * 线路号徽标。
 *
 * 上游不提供线路品牌色，我们也不假装有——用哈希稳定分配一个色点，
 * 保证同一条线路在任何页面颜色一致。色点只作辅助，线路号本身用中性色，
 * 这样黄色线路也不会出现"白字配黄底"的不可读组合。
 */
export function LineBadge({
  lineName,
  color,
  size = "md",
}: {
  lineName: string;
  color: string;
  size?: "md" | "lg";
}) {
  return (
    <span
      className="line-badge"
      style={size === "lg" ? { fontSize: "var(--t-head-size)", padding: "3px 10px" } : undefined}
    >
      <span className="line-badge__dot" style={{ background: color }} aria-hidden />
      {lineName}
    </span>
  );
}

/* ---------------------------------------------------------------- 提示条 */

export function Notice({
  children,
  tone = "info",
}: {
  children: ReactNode;
  tone?: "info" | "warn" | "danger";
}) {
  const Icon = tone === "warn" || tone === "danger" ? IconAlert : IconInfo;
  return (
    <div className={cx("notice", `notice--${tone}`)} role={tone === "info" ? undefined : "status"}>
      <Icon size={14} />
      <span className="grow">{children}</span>
    </div>
  );
}

/* ---------------------------------------------------------------- 分段控件 */

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (v: T) => void;
  ariaLabel?: string;
}) {
  return (
    <div className="segmented" role="tablist" aria-label={ariaLabel}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="tab"
          className="segmented__item"
          aria-selected={o.value === value}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/* ---------------------------------------------------------------- 状态页 */

/**
 * 空 / 错误 / 无数据状态。
 *
 * 这类 App 最常见的失败就是只做 happy path。每个列表都必须能表达
 * 这几种状态，且文案要可操作（告诉用户下一步做什么）。
 */
export function StateView({
  kind = "empty",
  title,
  detail,
  action,
}: {
  kind?: "empty" | "offline" | "info";
  title: string;
  detail?: ReactNode;
  action?: ReactNode;
}) {
  const Icon = kind === "offline" ? IconOffline : kind === "info" ? IconInfo : IconEmpty;
  return (
    <div className="state">
      <Icon size={40} className="state__icon" />
      <div className="state__title">{title}</div>
      {detail ? <div className="t-label">{detail}</div> : null}
      {action}
    </div>
  );
}

/** 骨架屏：形状必须与真实卡片一致，不能用转圈代替 */
export function SkeletonCard({ lines = 2 }: { lines?: number }) {
  return (
    <Card>
      <div className="skeleton" style={{ height: 18, width: "42%", marginBottom: 12 }} />
      <div className="skeleton" style={{ height: 34, width: "58%", marginBottom: 12 }} />
      {Array.from({ length: lines }).map((_, i) => (
        <div
          key={i}
          className="skeleton"
          style={{ height: 14, width: i === lines - 1 ? "64%" : "86%", marginBottom: 8 }}
        />
      ))}
    </Card>
  );
}

/* ---------------------------------------------------------- 数据新鲜度 */

/**
 * 数据新鲜度徽标。
 *
 * 这是建立信任的关键设计：每个实时数字都必须诚实标注它的来源与年龄。
 * 收班/无车时显示"暂无实时数据"而不是留空或显示 0。
 */
export function FreshnessChip({
  confidence,
  ageSeconds,
}: {
  confidence: "live" | "delayed" | "inferred" | "stale";
  ageSeconds?: number | null;
}) {
  if (confidence === "live") {
    return (
      <Chip tone="live">
        <span
          className="live-pulse"
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: "currentColor",
            display: "inline-block",
          }}
        />
        实时
      </Chip>
    );
  }
  if (confidence === "inferred") {
    return <Chip tone="warn">推测</Chip>;
  }
  if (confidence === "delayed") {
    const label =
      ageSeconds != null && ageSeconds >= 60
        ? `延迟 ${Math.floor(ageSeconds / 60)} 分`
        : "略有延迟";
    return <Chip tone="warn">{label}</Chip>;
  }
  return <Chip tone="neutral">数据过期</Chip>;
}

/** 关闭按钮（覆盖层用）。评审要求：模态必须有明确的关闭动作 */
export function CloseButton({ onClick }: { onClick: () => void }) {
  return (
    // 视觉 36px，`.close-btn::after` 用伪元素把触摸热区扩到 60px ——
    // 触摸目标要够大，但视觉尺寸与布局不该跟着变大
    <button type="button" className="close-btn" onClick={onClick} aria-label="关闭">
      <IconClose size={18} />
    </button>
  );
}

/**
 * 浏览器预览模式的提示条。
 *
 * 用浏览器直开 `pnpm dev` 时 UI 能正常渲染，但所有后端调用都不可用。
 * 与其让每个页面各自报一堆看不懂的错，不如在顶部一次性说清楚。
 */
export function BrowserModeBanner() {
  return (
    <div className="browser-banner">
      <IconInfo size={13} />
      <span>
        <b>浏览器预览模式</b>：界面可看，公交数据需要 Tauri 后端。
        用 <code>pnpm tauri android dev</code> 或安装 APK 体验完整功能。
      </span>
    </div>
  );
}
