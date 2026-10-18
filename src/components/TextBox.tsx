import type { ComponentProps } from "react";
import { cx } from "../lib/cx";

interface TextBoxProps extends ComponentProps<"input"> {
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

interface TextAreaProps extends ComponentProps<"textarea"> {
  invalid?: boolean;
}

export function TextArea({ invalid = false, className, ...rest }: TextAreaProps) {
  return <textarea className={cx("textbox", "textarea", className)} aria-invalid={invalid || undefined} spellCheck={false} {...rest} />;
}
