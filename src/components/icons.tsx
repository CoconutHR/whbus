/**
 * 图标系统 —— 手写内联 SVG。
 *
 * 为什么不用 Phosphor / Lucide 等图标库：
 * 1. 安卓包体敏感，整套图标库只用到十来个图标不划算
 * 2. 统一 24×24 viewBox + 1.75 描边 + round 线帽，风格自洽
 * 3. 零依赖，不引入额外的 tree-shaking 风险
 *
 * 规范：禁止用 emoji 作结构图标；图标尺寸只用 sm/md/lg 三档令牌。
 */

import type { SVGProps } from "react";

export type IconSize = "sm" | "md" | "lg";

const SIZE_PX: Record<IconSize, number> = {
  sm: 16,
  md: 20,
  lg: 24,
};

interface IconProps extends Omit<SVGProps<SVGSVGElement>, "children"> {
  size?: IconSize | number;
  /** 无障碍标签；不传则视为纯装饰（aria-hidden） */
  label?: string;
}

function sizeToPx(size: IconSize | number | undefined): number {
  if (size == null) return SIZE_PX.md;
  return typeof size === "number" ? size : SIZE_PX[size];
}

/** 所有图标的共同外壳 */
function Svg({ size, label, children, ...rest }: IconProps & { children: React.ReactNode }) {
  const px = sizeToPx(size);
  return (
    <svg
      width={px}
      height={px}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      focusable="false"
      {...rest}
    >
      {children}
    </svg>
  );
}

/* ---------------------------------------------------------------- 导航类 */

/** 首页 */
export const IconHome = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 10.6 12 3.8l8.5 6.8V20a1 1 0 0 1-1 1h-5v-6h-5v6h-5a1 1 0 0 1-1-1z" />
  </Svg>
);

/** 规划（路线分叉） */
export const IconRoute = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="6" cy="19" r="2.4" />
    <circle cx="18" cy="5" r="2.4" />
    <path d="M8.4 19h5.1a3 3 0 0 0 0-6H10a3 3 0 0 1 0-6h5.6" />
  </Svg>
);

/** 线路（公交） */
export const IconBus = (p: IconProps) => (
  <Svg {...p}>
    <rect x="4" y="3.5" width="16" height="14" rx="2.5" />
    <path d="M4 10.5h16" />
    <path d="M7.5 17.5V20M16.5 17.5V20" />
    <circle cx="8" cy="14" r="0.9" fill="currentColor" stroke="none" />
    <circle cx="16" cy="14" r="0.9" fill="currentColor" stroke="none" />
  </Svg>
);

/** 我的（设置） */
export const IconSettings = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M12 2.8v2.1M12 19.1v2.1M4.5 7.5l1.8 1.1M17.7 15.4l1.8 1.1M4.5 16.5l1.8-1.1M17.7 8.6l1.8-1.1" />
  </Svg>
);

/* ---------------------------------------------------------------- 功能类 */

/** 定位 */
export const IconLocate = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3.2" />
    <circle cx="12" cy="12" r="7.6" />
    <path d="M12 1.8v2.6M12 19.6v2.6M1.8 12h2.6M19.6 12h2.6" />
  </Svg>
);

/** 站牌 */
export const IconStop = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 21.2v-6.1" />
    <circle cx="12" cy="7.4" r="5.2" />
    <path d="M9.6 7.4h4.8" />
  </Svg>
);

/** 时钟 */
export const IconClock = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.6" />
    <path d="M12 7.4V12l3 1.9" />
  </Svg>
);

/** 收藏（实心/空心由 filled 控制） */
export const IconStar = ({ filled, ...p }: IconProps & { filled?: boolean }) => (
  <Svg {...p} fill={filled ? "currentColor" : "none"}>
    <path d="m12 3.6 2.6 5.3 5.8.8-4.2 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8L3.6 9.7l5.8-.8z" />
  </Svg>
);

/** 步行 */
export const IconWalk = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="13" cy="4.2" r="1.8" />
    <path d="M11.4 21.5l1.5-5.2-2-2.1.9-4.2 3.1 1.3 2.3 3.1" />
    <path d="M11.8 10 8.6 12.2l-1.3 4.3" />
  </Svg>
);

/** 换乘（双向箭头） */
export const IconTransfer = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 8.5h13M13.4 5.2 16.8 8.5l-3.4 3.3" />
    <path d="M20 15.5H7M10.6 12.2 7.2 15.5l3.4 3.3" />
  </Svg>
);

/** 刷新 */
export const IconRefresh = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20.3 12a8.3 8.3 0 1 1-2.5-5.9" />
    <path d="M20.6 4.2v4.6h-4.6" />
  </Svg>
);

/** 预警（三角感叹号） */
export const IconAlert = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 3.9 21 19.6H3z" />
    <path d="M12 10.2v4M12 16.9v.1" />
  </Svg>
);

/** 右箭头（列表项指示） */
export const IconChevron = (p: IconProps) => (
  <Svg {...p}>
    <path d="m9.5 5.5 6.5 6.5-6.5 6.5" />
  </Svg>
);

/** 返回 */
export const IconBack = (p: IconProps) => (
  <Svg {...p}>
    <path d="M19 12H5M11.5 18.5 5 12l6.5-6.5" />
  </Svg>
);

/** 关闭 */
export const IconClose = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 6l12 12M18 6 6 18" />
  </Svg>
);

/** 搜索 */
export const IconSearch = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="10.8" cy="10.8" r="6.6" />
    <path d="m15.6 15.6 4.4 4.4" />
  </Svg>
);

/** 交换（起终点互换） */
export const IconSwap = (p: IconProps) => (
  <Svg {...p}>
    <path d="M7.5 4.2v15.6M4.2 16.6l3.3 3.2 3.3-3.2" />
    <path d="M16.5 19.8V4.2M13.2 7.4l3.3-3.2 3.3 3.2" />
  </Svg>
);

/** 家 */
export const IconHomePin = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.8 10.4 12 3.9l8.2 6.5" />
    <path d="M5.6 11.6v8.5h12.8v-8.5" />
    <path d="M10 20.1v-4.6h4v4.6" />
  </Svg>
);

/** 公司 / 楼宇 */
export const IconWork = (p: IconProps) => (
  <Svg {...p}>
    <rect x="4" y="3.8" width="10" height="16.4" rx="1.5" />
    <path d="M14 9.2h5.2a1 1 0 0 1 1 1v10h-6.2" />
    <path d="M7.2 7.6h3.6M7.2 11.2h3.6M7.2 14.8h3.6" />
  </Svg>
);

/** 地铁（换乘徽标用） */
export const IconSubway = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.6" />
    <path d="M8.4 15.6V8.8l3.6 3.6 3.6-3.6v6.8" />
  </Svg>
);

/** 信息 / 数据状态 */
export const IconInfo = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.6" />
    <path d="M12 11v5.2M12 7.9v.1" />
  </Svg>
);

/** 无数据 / 空状态 */
export const IconEmpty = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3.6" y="5.4" width="16.8" height="13.2" rx="2.4" />
    <path d="M3.6 10.2h16.8M8.4 14.4h7.2" />
  </Svg>
);

/** 断网 / 错误 */
export const IconOffline = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 8.6a12.5 12.5 0 0 1 12.2-2.4M20.5 8.6a12.4 12.4 0 0 0-4.2-2.5" />
    <path d="M6.6 12.2a8.6 8.6 0 0 1 3.3-1.3M17.4 12.2a8.5 8.5 0 0 0-2.9-1.2" />
    <path d="M9.7 15.6a4.6 4.6 0 0 1 4.6 0" />
    <path d="M12 19v.1M3 3l18 18" />
  </Svg>
);

/** 夜间 / 已收班 */
export const IconMoon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20.4 14.2A8.6 8.6 0 0 1 9.8 3.6a8.8 8.8 0 1 0 10.6 10.6z" />
  </Svg>
);

/** 推测位置（虚线云） */
export const IconInferred = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.4" strokeDasharray="4 3.4" />
    <circle cx="12" cy="12" r="2.2" fill="currentColor" stroke="none" />
  </Svg>
);
