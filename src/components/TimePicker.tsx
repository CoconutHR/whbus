import { useEffect, useMemo, useState } from "react";
import { cx } from "../lib/utils";
import { IconClock, IconClose } from "./icons";
import { Button } from "./ui";
import "./TimePicker.css";

/**
 * 主题化的时间选择器。
 *
 * # 为什么不用 `<input type="time">`
 *
 * 安卓上它会拉起系统的时间轮盘（Material TimePickerDialog），
 * 与 App 的主题完全脱节：浅色主题下弹出一个深色轮盘、
 * 圆角与配色都不一样，看起来像"另一个 App 的控件"。
 * 而且 WebView 里这个原生控件的样式**无法用 CSS 覆盖**。
 *
 * 所以自己做一个底部抽屉：复用 App 的卡片、圆角、按钮与配色令牌。
 *
 * # 两种模式
 *
 * - `depart`：选择**出发**时刻 —— 主交互，默认"现在出发"
 * - `arrive`：选择**最晚到达**时刻 —— 可有可无，所以给"不限制"
 */

/** 把"相对现在的分钟数"格式化成 HH:MM */
export function formatOffsetClock(offsetMin: number): string {
  const d = new Date(Date.now() + offsetMin * 60_000);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/** 把 HH:MM 转成"相对现在的分钟数"，已过今天的时刻视为明天 */
export function clockToOffset(hhmm: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(hhmm.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const mi = Number(m[2]);
  if (h > 23 || mi > 59) return null;

  const now = new Date();
  const target = new Date(now);
  target.setHours(h, mi, 0, 0);
  // 已过（或正好等于）当前时刻 → 视为明天的班次
  if (target.getTime() <= now.getTime()) target.setDate(target.getDate() + 1);
  return Math.round((target.getTime() - now.getTime()) / 60_000);
}

/**
 * 生成"以当前时刻为起点、每 step 分钟一档"的时刻表。
 *
 * 从**已过去的整点**开始，这样用户能看到刚才的时刻也能往后选，
 * 而不是只能选未来 —— 想记录"我通常 7:50 出门"时更自然。
 */
export function timeOptions(step = 15, count = 28): Array<{ label: string; offset: number }> {
  const now = new Date();
  // 向下取整到 step 的整数倍
  const base = new Date(now);
  base.setMinutes(Math.floor(now.getMinutes() / step) * step, 0, 0);

  const out: Array<{ label: string; offset: number }> = [];
  for (let i = 0; i < count; i++) {
    const d = new Date(base.getTime() + i * step * 60_000);
    const hh = String(d.getHours()).padStart(2, "0");
    const mm = String(d.getMinutes()).padStart(2, "0");
    out.push({
      label: `${hh}:${mm}`,
      offset: Math.round((d.getTime() - now.getTime()) / 60_000),
    });
  }
  return out;
}

export interface TimePickerProps {
  /** 当前值（相对现在的分钟偏移）；null 表示未设置 */
  value: number | null;
  onChange: (offsetMin: number | null) => void;
  mode: "depart" | "arrive";
  /** 触发按钮上的前置文字，如"出发" */
  label: string;
  /** 未设置时显示的占位文字 */
  placeholder: string;
}

export function TimePicker({ value, onChange, mode, label, placeholder }: TimePickerProps) {
  const [open, setOpen] = useState(false);
  // 每次打开时重算，避免选项停留在打开前的时刻
  const [opts, setOpts] = useState(() => timeOptions());
  // 未选过时默认停在"约半小时后"那一档，比停在当前时刻更有用
  const [pending, setPending] = useState<number | null>(value);

  useEffect(() => {
    if (open) {
      setOpts(timeOptions());
      setPending(value);
    }
  }, [open, value]);

  // Esc 关闭：桌面预览与带键盘的安卓设备都用得上
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  const quick = useMemo(() => {
    if (mode === "arrive") {
      return [
        { label: "+30 分", offset: 30 },
        { label: "+1 小时", offset: 60 },
        { label: "+1.5 小时", offset: 90 },
        { label: "+2 小时", offset: 120 },
      ];
    }
    return [
      { label: "现在", offset: 0 },
      { label: "+15 分", offset: 15 },
      { label: "+30 分", offset: 30 },
      { label: "+1 小时", offset: 60 },
    ];
  }, [mode]);

  const display = useMemo(() => {
    if (value == null) return null;
    // 接近 0 时直接说"现在"，比显示一个会过期的 HH:MM 更稳
    if (mode === "depart" && value <= 1) return "现在出发";
    return formatOffsetClock(value);
  }, [value, mode]);

  const isTomorrow = useMemo(() => {
    if (pending == null) return false;
    const d = new Date(Date.now() + pending * 60_000);
    return d.getDate() !== new Date().getDate();
  }, [pending]);

  return (
    <>
      <button
        type="button"
        className={cx("timepick__trigger", value != null && "is-set")}
        onClick={() => setOpen(true)}
        aria-haspopup="dialog"
      >
        <IconClock size={16} />
        <span className="tnum">{display ?? placeholder}</span>
      </button>

      {open ? (
        <div
          className="timepick__scrim"
          role="presentation"
          onClick={() => setOpen(false)}
        >
          <div
            className="timepick__sheet"
            role="dialog"
            aria-modal="true"
            aria-label={`选择${label}`}
            // 阻止冒泡，避免点内容区把抽屉关掉
            onClick={(e) => e.stopPropagation()}
          >
            <div className="timepick__head">
              <span className="t-head">{label}</span>
              <CloseButtonPlain onClick={() => setOpen(false)} />
            </div>

            <div className="timepick__quick">
              {quick.map((q) => (
                <button
                  key={q.label}
                  type="button"
                  className={cx(
                    "timepick__chip",
                    pending != null && Math.abs(pending - q.offset) < 2 && "is-on",
                  )}
                  onClick={() => setPending(q.offset)}
                >
                  {q.label}
                </button>
              ))}
              {mode === "arrive" ? (
                <button
                  type="button"
                  className={cx("timepick__chip", pending == null && "is-on")}
                  onClick={() => {
                    onChange(null);
                    setOpen(false);
                  }}
                >
                  不限制
                </button>
              ) : null}
            </div>

            <div className="timepick__grid" role="listbox" aria-label={`${label}时刻`}>
              {opts.map((o) => (
                <button
                  key={o.label}
                  type="button"
                  role="option"
                  aria-selected={pending != null && Math.abs(pending - o.offset) < 1}
                  className={cx(
                    "timepick__cell tnum",
                    pending != null && Math.abs(pending - o.offset) < 1 && "is-on",
                  )}
                  onClick={() => setPending(o.offset)}
                >
                  {o.label}
                </button>
              ))}
            </div>

            <div className="timepick__foot">
              <span className="t-cap t-3">
                {pending == null
                  ? "未选择时刻"
                  : isTomorrow
                    ? `明天 ${formatOffsetClock(pending)} 出发`
                    : `今天 ${formatOffsetClock(pending)} 出发`}
              </span>
              <span className="row gap-2">
                <Button variant="ghost" onClick={() => setOpen(false)}>
                  取消
                </Button>
                <Button
                  variant="primary"
                  onClick={() => {
                    onChange(pending);
                    setOpen(false);
                  }}
                >
                  确定
                </Button>
              </span>
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}

/** 仅供本组件使用的极简关闭按钮（避免与 ui.tsx 循环依赖） */
function CloseButtonPlain({ onClick }: { onClick: () => void }) {
  return (
    <button type="button" className="timepick__close" onClick={onClick} aria-label="关闭">
      <IconClose size={18} />
    </button>
  );
}
