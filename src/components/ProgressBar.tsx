import type { CSSProperties } from "react";
import { cx } from "../lib/cx";

interface ProgressBarProps {
  /** 0..100; null = indeterminate. */
  value: number | null;
  /** Accessible name. */
  label: string;
  /** Human-readable value, e.g. "38 %, 560 MB of 1.4 GB". */
  valueText?: string;
  /** Moving highlight while data flows. */
  active?: boolean;
  tone?: "accent" | "success" | "caution" | "critical" | "neutral";
  className?: string;
}

export function ProgressBar({ value, label, valueText, active = false, tone = "accent", className }: ProgressBarProps) {
  const determinate = value !== null;
  const pct = determinate ? Math.max(0, Math.min(100, value)) : 0;
  const style = { "--pct": `${pct}%` } as CSSProperties;
  return (
    <div
      className={cx("progress-bar", `progress-${tone}`, !determinate && "is-indeterminate", active && "is-active", className)}
      role="progressbar"
      aria-label={label}
      aria-valuemin={determinate ? 0 : undefined}
      aria-valuemax={determinate ? 100 : undefined}
      aria-valuenow={determinate ? Math.round(pct) : undefined}
      aria-valuetext={valueText}
      style={style}
    >
      <span className="progress-bar-fill" />
    </div>
  );
}
