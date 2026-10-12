import { useEffect, useState } from "react";

import { TabBar, type TabKey } from "./components/TabBar";
import { HomeScreen } from "./screens/HomeScreen";
import { PlanScreen } from "./screens/PlanScreen";
import { LinesScreen } from "./screens/LinesScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
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

  useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  /**
   * 四个 Tab 始终保持挂载（仅切换可见性）。
   *
   * 这是刻意的：安卓用户期望"切走再切回"时滚动位置与表单内容都还在。
   * 卸载重挂会丢失列表滚动位置，也是评审里的严重级问题。
   */
  return (
    <div className="app">
      <main className="app__body">
        <Pane active={tab === "home"}>
          <HomeScreen />
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
      </main>

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
