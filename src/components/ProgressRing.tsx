import { cx } from "../lib/cx";

interface ProgressRingProps {
  size?: 16 | 20 | 32;
  /** Accessible label; omit when the surrounding text already explains the wait. */
  label?: string;
  className?: string;
}

/** Indeterminate Fluent ProgressRing. */
export function ProgressRing({ size = 16, label, className }: ProgressRingProps) {
  return (
    <svg
      className={cx("progress-ring", className)}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      role={label ? "progressbar" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <circle cx="8" cy="8" r="7" />
    </svg>
  );
}
