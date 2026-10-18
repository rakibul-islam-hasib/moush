import { useEffect, useId, useRef, type ReactNode, type RefObject } from "react";
import { cx } from "../lib/cx";

interface DialogProps {
  title: string;
  children: ReactNode;
  footer: ReactNode;
  /** Called on Escape. When omitted the dialog can't be dismissed with Escape. */
  onDismiss?: () => void;
  /** Element to focus when the dialog opens (defaults to the dialog content). */
  initialFocus?: RefObject<HTMLElement | null>;
  className?: string;
}

/** Windows 11 ContentDialog on top of the native modal <dialog> (focus trap, inert page, smoke layer). */
export function Dialog({ title, children, footer, onDismiss, initialFocus, className }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const unmounting = useRef(false);
  const dismissRef = useRef(onDismiss);
  const titleId = useId();

  useEffect(() => {
    dismissRef.current = onDismiss;
  });

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    unmounting.current = false;
    if (!dialog.open) dialog.showModal();
    (initialFocus?.current ?? bodyRef.current)?.focus();
    return () => {
      unmounting.current = true;
      dialog.close();
    };
  }, [initialFocus]);

  return (
    <dialog
      ref={ref}
      className={cx("dialog", className)}
      aria-labelledby={titleId}
      onCancel={(e) => {
        e.preventDefault();
        dismissRef.current?.();
      }}
      onClose={() => {
        // The browser may force-close on repeated Escape; keep it open while we're mounted.
        const dialog = ref.current;
        if (unmounting.current || !dialog || dialog.open) return;
        if (dismissRef.current) dismissRef.current();
        else dialog.showModal();
      }}
    >
      <div className="dialog-body" ref={bodyRef} tabIndex={-1}>
        <h2 id={titleId} className="dialog-title">
          {title}
        </h2>
        {children}
      </div>
      <div className="dialog-footer">{footer}</div>
    </dialog>
  );
}
