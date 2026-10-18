import { useEffect, useState } from "react";
import { backend, type Diagnostics } from "../api";
import { errorMessage } from "../lib/errors";

/**
 * Polls diagnostics once per second while the component is mounted and the
 * window is visible. Stops when hidden or unmounted.
 */
export function useDiagnostics(): { data: Diagnostics | null; error: string | null } {
  const [data, setData] = useState<Diagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let timer: number | undefined;
    let cancelled = false;
    let inFlight = false;

    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const d = await backend.api.getDiagnostics();
        if (!cancelled) {
          setData(d);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(errorMessage(e));
      } finally {
        inFlight = false;
      }
    };
    const start = () => {
      if (timer !== undefined) return;
      void tick();
      timer = window.setInterval(() => void tick(), 1000);
    };
    const stop = () => {
      if (timer === undefined) return;
      window.clearInterval(timer);
      timer = undefined;
    };
    const onVisibility = () => (document.visibilityState === "visible" ? start() : stop());

    onVisibility();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      cancelled = true;
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  return { data, error };
}
