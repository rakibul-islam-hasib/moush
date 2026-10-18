import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import { Icon, type IconName } from "./Icon";

export type Severity = "informational" | "success" | "warning" | "error";

const ICON: Record<Severity, IconName> = {
  informational: "info",
  success: "successCircle",
  warning: "warning",
  error: "errorCircle",
};

interface InfoBarProps {
  severity: Severity;
  title?: string;
  children?: ReactNode;
  action?: ReactNode;
  onClose?: () => void;
  /** Announce to assistive technology when it appears. */
  live?: boolean;
  className?: string;
}

export function InfoBar({ severity, title, children, action, onClose, live = false, className }: InfoBarProps) {
  return (
    <div
      className={cx("infobar", `infobar-${severity}`, className)}
      role={live ? (severity === "error" ? "alert" : "status") : undefined}
    >
      <Icon name={ICON[severity]} className="infobar-icon" />
      <div className="infobar-content">
        {title && <span className="infobar-title">{title}</span>}
        {children !== undefined && <span className="infobar-message">{children}</span>}
        {action && <div className="infobar-action">{action}</div>}
      </div>
      {onClose && (
        <button type="button" className="btn btn-subtle btn-icon infobar-close" aria-label="Close" onClick={onClose}>
          <Icon name="dismiss" />
        </button>
      )}
    </div>
  );
}
