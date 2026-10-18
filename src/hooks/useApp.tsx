import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { backend, type AppSnapshot, type Settings } from "../api";
import { Button } from "../components/Button";
import { InfoBar } from "../components/InfoBar";
import { ProgressRing } from "../components/ProgressRing";
import { errorMessage } from "../lib/errors";
import { useToast } from "./useToast";

export interface SaveResult {
  ok: boolean;
  /** Authoritative settings after the attempt (saved, or reverted on failure). */
  settings: Settings;
}

interface AppContextValue {
  snapshot: AppSnapshot;
  /** When the current snapshot arrived (unix ms); anchors relative values like retryInMs. */
  snapshotAt: number;
  settings: Settings;
  setSnapshot: (s: AppSnapshot) => void;
  saveSettings: (next: Settings) => Promise<SaveResult>;
}

const AppContext = createContext<AppContextValue | null>(null);

interface Stamped {
  value: AppSnapshot;
  at: number;
}

/** Loads the snapshot + settings, subscribes to snapshot events and shares them. */
export function AppProvider({ children }: { children: ReactNode }) {
  const toast = useToast();
  const [snapshot, setSnapshotState] = useState<Stamped | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const confirmed = useRef<Settings | null>(null);
  const saveSeq = useRef(0);

  useEffect(() => {
    let disposed = false;
    let gotEvent = false;
    let unlisten: (() => void) | null = null;

    backend
      .onSnapshot((s) => {
        if (disposed) return;
        gotEvent = true;
        setSnapshotState({ value: s, at: Date.now() });
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
        if (!gotEvent) setSnapshotState({ value: snap, at: Date.now() });
        confirmed.current = st;
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
  }, [attempt, toast]);

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
        confirmed.current = st;
        setSettings((prev) => (prev ? { ...prev, lastHostAddress: st.lastHostAddress } : st));
      })
      .catch((e: unknown) => {
        if (!cancelled) toast.error(e, "Couldn't refresh settings");
      });
    return () => {
      cancelled = true;
    };
  }, [clientState, toast]);

  const setSnapshot = useCallback((s: AppSnapshot) => setSnapshotState({ value: s, at: Date.now() }), []);

  const saveSettings = useCallback(
    async (draft: Settings): Promise<SaveResult> => {
      // The UI never edits lastHostAddress; always send the backend's latest value.
      const next: Settings = { ...draft, lastHostAddress: confirmed.current?.lastHostAddress ?? draft.lastHostAddress };
      const seq = ++saveSeq.current;
      setSettings(next); // optimistic: e.g. the theme applies immediately
      try {
        const saved = await backend.api.updateSettings(next);
        confirmed.current = saved;
        if (seq === saveSeq.current) setSettings(saved);
        return { ok: true, settings: saved };
      } catch (e) {
        toast.error(e, "Couldn't save settings");
        const previous = confirmed.current ?? next;
        if (seq === saveSeq.current) setSettings(previous);
        return { ok: false, settings: previous };
      }
    },
    [toast],
  );

  const value = useMemo<AppContextValue | null>(
    () =>
      snapshot && settings
        ? { snapshot: snapshot.value, snapshotAt: snapshot.at, settings, setSnapshot, saveSettings }
        : null,
    [snapshot, settings, setSnapshot, saveSettings],
  );

  if (!value) {
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
            <ProgressRing size={32} label="Loading Moush" />
            <span className="text-secondary">Starting Moush…</span>
          </>
        )}
      </div>
    );
  }

  return <AppContext value={value}>{children}</AppContext>;
}

export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used inside <AppProvider>");
  return ctx;
}
