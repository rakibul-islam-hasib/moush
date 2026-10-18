import type { ComponentProps } from "react";
import { cx } from "../lib/cx";
import { Icon, type IconName } from "./Icon";
import { ProgressRing } from "./ProgressRing";

interface ButtonProps extends ComponentProps<"button"> {
  variant?: "standard" | "accent" | "subtle";
  icon?: IconName;
  /** Shows a ProgressRing in place of the icon and disables the button. */
  busy?: boolean;
}

export function Button({
  variant = "standard",
  icon,
  busy = false,
  className,
  disabled,
  type = "button",
  children,
  ...rest
}: ButtonProps) {
  const iconOnly = children === undefined || children === null;
  return (
    <button
      type={type}
      className={cx("btn", `btn-${variant}`, iconOnly && "btn-icon", className)}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      {...rest}
    >
      {busy ? <ProgressRing size={16} /> : icon ? <Icon name={icon} /> : null}
      {!iconOnly && <span>{children}</span>}
    </button>
  );
}
