import { useEffect, useState } from "react";

import { IconBus, IconChevron, IconClose } from "./icons";
import { useAppStore } from "../lib/store";
import { formatClock, formatMinutes, lineColorVar } from "../lib/utils";
import "./TrackingBar.css";

/**
 * 追踪条 —— 采纳方案后的常驻状态。
 *
 * # 为什么必须有它
 *
 * 用户在决策卡上点了「开始导航」，如果界面毫无变化，他会不知道下一步看哪。
 * 安卓用户不会去猜"是不是要去线路页"，所以采纳后必须给出**明确的持续反馈**。
 *
 * 这是 App 的第二状态（不是第四层页面）：出现在 Tab 栏正上方，
 * 细而低干扰，可点开看具体车辆，可一键结束。
 *
 * 放在 Tab 栏之上而不是之下，是因为它属于"当前会话状态"，
 * 优先级高于"切换页面"；同时它不能盖住 Tab 栏，否则导航就废了。
 */
export function TrackingBar({ onOpen }: { onOpen?: (lineName: string) => void }) {
  const { tracking, setTracking } = useAppStore();
  const [, forceTick] = useState(0);

  // 每分钟刷新一次剩余时间显示。
  // 用 tick 而不是 setInterval 去改 tracking 本身，避免污染持久化状态。
  useEffect(() => {
    if (!tracking) return;
    const t = window.setInterval(() => forceTick((n) => n + 1), 30_000);
    return () => window.clearInterval(t);
  }, [tracking]);

  if (!tracking) return null;

  const acceptedAt = new Date(tracking.acceptedAt);
  // 相对**采纳时刻**的到达时间；随着时间推移剩余分钟数会减少
  const elapsedMin = (Date.now() - tracking.acceptedAt) / 60_000;
  const remainMin = tracking.arriveOffsetMin - elapsedMin;
  const arriveAt = formatClock(acceptedAt, tracking.arriveOffsetMin);

  // 超过预计到达时间 5 分钟就认为结束了，避免出现"还剩 -12 分钟"
  const finished = remainMin < -5;

  return (
    <div className="trackbar glass" role="status" aria-live="polite">
      <span className="trackbar__icon" aria-hidden>
        <IconBus size={15} />
      </span>

      <button
        type="button"
        className="trackbar__body"
        onClick={() => {
          const first = tracking.lineNames[0];
          if (first && onOpen) onOpen(first);
        }}
      >
        <span className="trackbar__text">
          <span className="t-label">
            {finished
              ? `已到达${tracking.toLabel}附近`
              : `追踪中 · ${tracking.label} · ${formatMinutes(Math.max(0, remainMin))}`}
          </span>
          <span className="t-cap t-3">
            {finished ? `计划 ${arriveAt} 到达` : `预计 ${arriveAt} 到达${tracking.toLabel}`}
          </span>
        </span>

        {tracking.lineNames.length > 0 ? (
          <span className="trackbar__lines">
            {tracking.lineNames.map((n) => (
              <span key={n} className="line-badge">
                <span
                  className="line-badge__dot"
                  style={{ background: lineColorVar(n) }}
                  aria-hidden
                />
                {n}
              </span>
            ))}
          </span>
        ) : null}

        {onOpen && tracking.lineNames.length > 0 ? (
          <IconChevron size={14} className="t-3" />
        ) : null}
      </button>

      <button
        type="button"
        className="trackbar__close"
        onClick={() => setTracking(null)}
        aria-label={finished ? "完成" : "结束追踪"}
      >
        <IconClose size={16} />
      </button>
    </div>
  );
}
