import { useCallback, useEffect, useState } from "react";
import { backend, type ClipboardEntry } from "../api";
import { errorMessage } from "../lib/errors";

/**
 * The clipboard history (memory only on the backend). Refetched whenever
 * features.clipboard.historyRevision changes, and only while `enabled`.
 */
export function useClipboardHistory(revision: number, enabled: boolean) {
  const [entries, setEntries] = useState<ClipboardEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    backend.api
      .getClipboardHistory()
      .then((list) => {
        if (cancelled) return;
        setEntries(list);
        setError(null);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(errorMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, [revision, enabled, nonce]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  /** Optimistic local removal (the next revision brings the authoritative list). */
  const removeLocal = useCallback((id: string) => setEntries((list) => list?.filter((e) => e.id !== id) ?? list), []);

  return { entries, error, reload, removeLocal, setEntries };
}
