import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { InfoBar, type Severity } from "../components/InfoBar";
import { errorMessage } from "../lib/errors";

export interface ToastAction {
  label: string;
  onClick: () => void;
}

interface Toast {
  id: number;
  severity: Severity;
  message: string;
  title?: string;
  action?: ToastAction;
}

export interface ToastApi {
  notify: (severity: Severity, message: string, title?: string, action?: ToastAction) => void;
  /** Shows a rejected backend call (or any thrown value) as an error toast. */
  error: (e: unknown, title?: string) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => setToasts((all) => all.filter((t) => t.id !== id)), []);

  const notify = useCallback((severity: Severity, message: string, title?: string, action?: ToastAction) => {
    const id = nextId.current++;
    // Identical messages (e.g. from polling) are shown once.
    setToasts((all) =>
      all.some((t) => t.message === message && t.severity === severity)
        ? all
        : [...all.slice(-3), { id, severity, message, title, action }],
    );
  }, []);

  const error = useCallback((e: unknown, title?: string) => notify("error", errorMessage(e), title), [notify]);
  const value = useMemo<ToastApi>(() => ({ notify, error }), [notify, error]);

  return (
    <ToastContext value={value}>
      {children}
      <div className="toast-region" aria-label="Notifications" role="region">
        {toasts.map((t) => (
          <ToastItem key={t.id} toast={t} onDismiss={dismiss} />
        ))}
      </div>
    </ToastContext>
  );
}

function ToastItem({ toast, onDismiss }: { toast: Toast; onDismiss: (id: number) => void }) {
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    if (paused) return;
    const ms = toast.severity === "error" ? 10_000 : 5_000;
    const timer = window.setTimeout(() => onDismiss(toast.id), ms);
    return () => window.clearTimeout(timer);
  }, [paused, toast, onDismiss]);

  return (
    <div
      className="toast"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
    >
      <InfoBar
        severity={toast.severity}
        title={toast.title}
        live
        onClose={() => onDismiss(toast.id)}
        action={
          toast.action && (
            <button
              type="button"
              className="btn btn-standard btn-small"
              onClick={() => {
                toast.action?.onClick();
                onDismiss(toast.id);
              }}
            >
              {toast.action.label}
            </button>
          )
        }
      >
        {toast.message}
      </InfoBar>
    </div>
  );
}

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used inside <ToastProvider>");
  return ctx;
}
