import type { InputHTMLAttributes } from "react";
import { cx } from "../lib/cx";

interface TextBoxProps extends InputHTMLAttributes<HTMLInputElement> {
  invalid?: boolean;
}

export function TextBox({ invalid = false, className, type = "text", ...rest }: TextBoxProps) {
  return (
    <input
      type={type}
      className={cx("textbox", className)}
      aria-invalid={invalid || undefined}
      spellCheck={false}
      autoComplete="off"
      {...rest}
    />
  );
}
