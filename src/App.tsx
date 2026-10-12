import { useEffect, useState } from "react";

import { TabBar, type TabKey } from "./components/TabBar";
import { TrackingBar } from "./components/TrackingBar";
import { BrowserModeBanner } from "./components/ui";
import { HomeScreen } from "./screens/HomeScreen";
import { PlanScreen } from "./screens/PlanScreen";
import { LinesScreen } from "./screens/LinesScreen";
import { LineDetailView } from "./screens/LineDetailView";
import { SettingsScreen } from "./screens/SettingsScreen";
import { isTauri } from "./lib/api";
import { useAppStore } from "./lib/store";
import "./App.css";

export type ThemeMode = "system" | "light" | "dark";

const THEME_KEY = "whbus.theme";

/** 把主题模式应用到 <html data-theme> */
function applyTheme(mode: ThemeMode) {
  const root = document.documentElement;
  if (mode === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", mode);
  }
}

export default function App() {
  const [tab, setTab] = useState<TabKey>("home");
  const [theme, setTheme] = useState<ThemeMode>(
    () => (localStorage.getItem(THEME_KEY) as ThemeMode | null) ?? "system",
  );
  const { openLineId, setOpenLineId } = useAppStore();

  useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  /**
   * 接管安卓返回键。
   *
   * 规则（也是移动端评审里"严重"级的要求）：
   * 1. 有覆盖层（线路详情）→ 关掉覆盖层
   * 2. 不在首页 Tab → 回首页 Tab
   * 3. 已在首页 → 交给系统处理（退出）
   *
   * **绝不能在任何子页面直接退出 App。**
   */
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        unlisten = await win.onCloseRequested((e) => {
          if (openLineId) {
            e.preventDefault();
            setOpenLineId(null);
          } else if (tab !== "home") {
            e.preventDefault();
            setTab("home");
          }
        });
      } catch {
        // 桌面浏览器里没有这个 API，忽略
      }
    })();

    return () => unlisten?.();
  }, [openLineId, tab, setOpenLineId]);

  /**
   * 四个 Tab 始终保持挂载（仅切换可见性）。
   *
   * 这是刻意的：安卓用户期望"切走再切回"时滚动位置与表单内容都还在。
   * 卸载重挂会丢失列表滚动位置，也是评审里的严重级问题。
   */
  return (
    <div className="app">
      {/* 浏览器预览时一次性说明，避免每页各自抛技术错误 */}
      {!isTauri() ? <BrowserModeBanner /> : null}

      <main className="app__body">
        <Pane active={tab === "home"}>
          {/* 首页的决策卡可以跳到规划页看其他方案 */}
          <HomeScreen onGoToPlan={() => setTab("plan")} />
        </Pane>
        <Pane active={tab === "plan"}>
          <PlanScreen />
        </Pane>
        <Pane active={tab === "lines"}>
          <LinesScreen />
        </Pane>
        <Pane active={tab === "settings"}>
          <SettingsScreen theme={theme} onThemeChange={setTheme} />
        </Pane>

        {/*
          线路详情是全屏覆盖层，盖住所有 Tab 但**不盖 Tab 栏**。
          这样用户随时能切走，不会被困在一个页面里（模态逃生）。
        */}
        {openLineId ? (
          <div className="app__overlay">
            <LineDetailView lineId={openLineId} onBack={() => setOpenLineId(null)} />
          </div>
        ) : null}
      </main>

      {/* 追踪条位于 Tab 栏正上方：属于会话状态，优先级高于切页 */}
      <TrackingBar
        onOpen={(lineName) => {
          // 追踪条给的是线路号，需要换成 lineId 才能打开详情。
          // 这里不额外请求：线路页自己会解析，所以先切到线路页更稳。
          void lineName;
          setTab("lines");
        }}
      />

      <TabBar active={tab} onChange={setTab} />
    </div>
  );
}

function Pane({
  active,
  children,
}: {
  active: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="app__pane" hidden={!active} aria-hidden={!active}>
      {children}
    </div>
  );
}
