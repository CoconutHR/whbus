import { useCallback } from "react";

import {
  IconBus,
  IconHome,
  IconRoute,
  IconSettings,
  type IconProps,
} from "./icons";
import "./TabBar.css";

export type TabKey = "home" | "plan" | "lines" | "settings";

interface TabDef {
  key: TabKey;
  label: string;
  Icon: (p: IconProps) => React.ReactElement;
}

/**
 * 四个 Tab —— 底部导航上限是 5 项，4 项正好覆盖完整闭环：
 * 首页（零操作获取信息）/ 规划（主动查询）/ 线路（搜索与追踪）/ 我的（配置）
 */
const TABS: TabDef[] = [
  { key: "home", label: "首页", Icon: IconHome },
  { key: "plan", label: "规划", Icon: IconRoute },
  { key: "lines", label: "线路", Icon: IconBus },
  { key: "settings", label: "我的", Icon: IconSettings },
];

export function TabBar({
  active,
  onChange,
}: {
  active: TabKey;
  onChange: (k: TabKey) => void;
}) {
  return (
    <nav className="tabbar glass" role="tablist" aria-label="主导航">
      {TABS.map(({ key, label, Icon }) => {
        const selected = key === active;
        return (
          <button
            key={key}
            type="button"
            role="tab"
            className="tabbar__item"
            aria-selected={selected}
            onClick={() => onChange(key)}
          >
            <span className="tabbar__icon">
              {/* 激活态加粗描边：颜色之外的第二区分维度，不依赖色觉也能分辨 */}
              <Icon size={24} strokeWidth={selected ? 2.25 : 1.75} />
            </span>
            <span className="tabbar__label">{label}</span>
          </button>
        );
      })}
    </nav>
  );
}

/** 供外部触觉反馈使用：切换 Tab 时的轻触 */
export function useTabChange(onChange: (k: TabKey) => void) {
  return useCallback(
    (k: TabKey) => {
      onChange(k);
    },
    [onChange],
  );
}
