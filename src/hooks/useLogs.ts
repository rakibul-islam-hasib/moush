import { useEffect, useState } from "react";
import { backend, type LogEntry } from "../api";
import { useToast } from "./useToast";

const INITIAL = 500;
const MAX_LINES = 2000;

const keyOf = (e: LogEntry) => `${e.ts}|${e.target}|${e.message}`;

/** Recent log lines (fetched on mount) followed by live lines. */
export function useLogs(): LogEntry[] {
  const toast = useToast();
  const [entries, setEntries] = useState<LogEntry[]>([]);

  useEffect(() => {
    let disposed = false;
    let loaded = false;
    let early: LogEntry[] = [];
    let unlisten: (() => void) | null = null;

    backend
      .onLog((entry) => {
        if (disposed) return;
        if (!loaded) {
          early.push(entry);
          return;
        }
        setEntries((all) => {
          const next = [...all, entry];
          return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
        });
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((e: unknown) => toast.error(e, "Live log is unavailable"));

    backend.api
      .getLogs(INITIAL)
      .then(
        (initial) => {
          if (disposed) return;
          const seen = new Set(initial.map(keyOf));
          const live = early.filter((e) => !seen.has(keyOf(e)));
          loaded = true;
          early = [];
          setEntries([...initial, ...live]);
        },
        (e: unknown) => {
          if (disposed) return;
          const live = early;
          loaded = true;
          early = [];
          setEntries(live);
          toast.error(e, "Couldn't load the log");
        },
      );

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [toast]);

  return entries;
}
