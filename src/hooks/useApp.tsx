import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { backend, type AppSnapshot, type Settings } from "../api";
import { Button } from "../components/Button";
import { InfoBar } from "../components/InfoBar";
import { Logo } from "../components/Logo";
import { ProgressRing } from "../components/ProgressRing";
import { errorMessage } from "../lib/errors";
import { share } from "../lib/share";
import { useToast } from "./useToast";

export interface UpdateOptions {
  /** Wait for a short pause before saving (text fields, sliders). */
  debounce?: boolean;
}

export interface SettingsApi {
  /** Current settings: the optimistic draft while a save is pending, else the backend's value. */
  settings: Settings;
  /**
   * Edits settings. The change shows at once (optimistic) and is saved through
   * api.updateSettings; the validated value the backend returns replaces the draft.
   * On failure the draft rolls back to the last saved settings and an error toast shows.
   */
  update: (mutate: (s: Settings) => Settings, opts?: UpdateOptions) => void;
  /** Restores defaults through api.resetSettings. Rejects with the backend's message. */
  reset: () => Promise<void>;
}

interface AppContextValue {
  snapshot: AppSnapshot;
  /** When the current snapshot arrived (unix ms); anchors relative values like retryInMs. */
  snapshotAt: number;
  setSnapshot: (s: AppSnapshot) => void;
}

const AppContext = createContext<AppContextValue | null>(null);
const SettingsContext = createContext<SettingsApi | null>(null);

interface Stamped {
  value: AppSnapshot;
  at: number;
}

const DEBOUNCE_MS = 400;

/** Loads the snapshot + settings, subscribes to snapshot events and shares them. */
export function AppProvider({ children }: { children: ReactNode }) {
  const toast = useToast();
  const [snapshot, setSnapshotState] = useState<Stamped | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  /** Last value the backend confirmed. */
  const confirmed = useRef<Settings | null>(null);
  /** What the UI shows (confirmed + unsaved edits). */
  const draft = useRef<Settings | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const saving = useRef(false);
  const again = useRef(false);

  const accept = useCallback((s: AppSnapshot) => {
    setSnapshotState((prev) => ({ value: share(prev?.value, s), at: Date.now() }));
  }, []);

  useEffect(() => {
    let disposed = false;
    let gotEvent = false;
    let unlisten: (() => void) | null = null;

    backend
      .onSnapshot((s) => {
        if (disposed) return;
        gotEvent = true;
        accept(s);
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((e: unknown) => toast.error(e, "Live updates are unavailable"));

    Promise.all([backend.api.getSnapshot(), backend.api.getSettings()])
      .then(([snap, st]) => {
        if (disposed) return;
        // A pushed snapshot is newer than the one we asked for.
        if (!gotEvent) accept(snap);
        confirmed.current = st;
        draft.current = st;
        setSettings(st);
        setLoadError(null);
      })
      .catch((e: unknown) => {
        if (!disposed) setLoadError(errorMessage(e));
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [attempt, toast, accept]);

  // The backend updates lastHostAddress itself after a successful connection and
  // there is no settings event, so re-read it whenever the client connects.
  const clientState = snapshot?.value.client?.state;
  useEffect(() => {
    if (clientState !== "connected") return;
    let cancelled = false;
    backend.api
      .getSettings()
      .then((st) => {
        if (cancelled) return;
        confirmed.current = confirmed.current ? { ...confirmed.current, lastHostAddress: st.lastHostAddress } : st;
        const next = draft.current ? { ...draft.current, lastHostAddress: st.lastHostAddress } : st;
        draft.current = next;
        setSettings(next);
      })
      .catch((e: unknown) => {
        if (!cancelled) toast.error(e, "Couldn't refresh settings");
      });
    return () => {
      cancelled = true;
    };
  }, [clientState, toast]);

  /**
   * Saves the latest draft. Saves never overlap: edits made while a save is in flight
   * are sent right after it, so the backend always ends with the newest settings.
   */
  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
    if (saving.current) {
      again.current = true;
      return;
    }
    saving.current = true;
    try {
      do {
        again.current = false;
        const sent = draft.current;
        if (!sent) return;
        // The UI never edits lastHostAddress; always send the backend's latest value.
        const body: Settings = { ...sent, lastHostAddress: confirmed.current?.lastHostAddress ?? sent.lastHostAddress };
        try {
          const saved = await backend.api.updateSettings(body);
          confirmed.current = saved;
          if (draft.current === sent && timer.current === undefined) {
            draft.current = saved;
            setSettings(saved);
          }
        } catch (e) {
          toast.error(e, "Couldn't save settings");
          window.clearTimeout(timer.current);
          timer.current = undefined;
          again.current = false;
          const previous = confirmed.current ?? sent;
          draft.current = previous;
          setSettings(previous);
        }
      } while (again.current);
    } finally {
      saving.current = false;
    }
  }, [toast]);

  const update = useCallback(
    (mutate: (s: Settings) => Settings, opts?: UpdateOptions) => {
      const base = draft.current;
      if (!base) return;
      const next = mutate(base);
      if (next === base) return;
      draft.current = next;
      setSettings(next);
      window.clearTimeout(timer.current);
      if (opts?.debounce) {
        timer.current = window.setTimeout(() => {
          timer.current = undefined;
          void flush();
        }, DEBOUNCE_MS);
      } else {
        timer.current = undefined;
        void flush();
      }
    },
    [flush],
  );

  const reset = useCallback(async () => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
    const saved = await backend.api.resetSettings();
    confirmed.current = saved;
    draft.current = saved;
    setSettings(saved);
  }, []);

  // Don't lose a debounced edit when the window is hidden or closed to the tray.
  useEffect(() => {
    const onHide = () => {
      if (timer.current !== undefined) void flush();
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") onHide();
    };
    window.addEventListener("pagehide", onHide);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("pagehide", onHide);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [flush]);

  const value = useMemo<AppContextValue | null>(
    () => (snapshot ? { snapshot: snapshot.value, snapshotAt: snapshot.at, setSnapshot: accept } : null),
    [snapshot, accept],
  );
  const settingsValue = useMemo<SettingsApi | null>(() => (settings ? { settings, update, reset } : null), [settings, update, reset]);

  if (!value || !settingsValue) {
    return (
      <div className="splash">
        {loadError ? (
          <div className="splash-error">
            <InfoBar severity="error" title="Moush couldn't start" live>
              {loadError}
            </InfoBar>
            <Button variant="accent" icon="refresh" onClick={() => setAttempt((n) => n + 1)}>
              Try again
            </Button>
          </div>
        ) : (
          <>
            <Logo size={48} />
            <span className="splash-text">
              <ProgressRing size={16} />
              Starting Moush…
            </span>
          </>
        )}
      </div>
    );
  }

  return (
    <AppContext value={value}>
      <SettingsContext value={settingsValue}>{children}</SettingsContext>
    </AppContext>
  );
}

/** Snapshot + settings. Components that only need settings should use useSettings(). */
export function useApp(): AppContextValue & SettingsApi {
  const ctx = useContext(AppContext);
  const s = useContext(SettingsContext);
  if (!ctx || !s) throw new Error("useApp must be used inside <AppProvider>");
  return { ...ctx, ...s };
}

export function useSnapshot(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useSnapshot must be used inside <AppProvider>");
  return ctx;
}

export function useSettings(): SettingsApi {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error("useSettings must be used inside <AppProvider>");
  return ctx;
}
