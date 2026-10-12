import type { ReactNode } from "react";

import "./Screen.css";

/** 页面容器：统一处理安全区、滚动与固定头部的内边距补偿 */
export function Screen({
  children,
  header,
}: {
  children: ReactNode;
  header?: ReactNode;
}) {
  return (
    <div className="screen">
      {header ? <div className="screen__header">{header}</div> : null}
      <div className="screen__scroll scroll">{children}</div>
    </div>
  );
}

/** 页面标题栏（非吸顶，随内容滚动） */
export function PageTitle({
  title,
  subtitle,
  action,
}: {
  title: string;
  subtitle?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="page-title">
      <div className="grow">
        <h1 className="t-title page-title__h">{title}</h1>
        {subtitle ? <div className="t-label t-2">{subtitle}</div> : null}
      </div>
      {action}
    </div>
  );
}

/** 区块标题 */
export function SectionTitle({
  children,
  action,
}: {
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="section-title">
      <span className="t-label t-2">{children}</span>
      {action}
    </div>
  );
}
