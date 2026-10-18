import { useCallback, useState } from "react";
import { useToast } from "./useToast";

/**
 * Runs backend calls with a busy flag (keyed, so a list can show which row is
 * working) and reports rejections as error toasts.
 */
export function useAction() {
  const toast = useToast();
  const [busy, setBusy] = useState<string | null>(null);

  const run = useCallback(
    async (key: string, fn: () => Promise<unknown>, errorTitle?: string): Promise<boolean> => {
      setBusy(key);
      try {
        await fn();
        return true;
      } catch (e) {
        toast.error(e, errorTitle);
        return false;
      } finally {
        setBusy((current) => (current === key ? null : current));
      }
    },
    [toast],
  );

  return { busy, run };
}
