import { useRef, useState, type ReactNode } from "react";
import { errorMessage } from "../lib/errors";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { InfoBar } from "./InfoBar";

interface ConfirmDialogProps {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  /** The action; the dialog closes when it resolves and shows the error inline when it rejects. */
  onConfirm: () => Promise<unknown>;
  onClose: () => void;
}

export function ConfirmDialog({ title, children, confirmLabel, cancelLabel = "Cancel", onConfirm, onClose }: ConfirmDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await onConfirm();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={title}
      onDismiss={busy ? undefined : onClose}
      initialFocus={cancelRef}
      footer={
        <>
          <Button variant="accent" busy={busy} onClick={() => void confirm()}>
            {confirmLabel}
          </Button>
          <Button ref={cancelRef} disabled={busy} onClick={onClose}>
            {cancelLabel}
          </Button>
        </>
      }
    >
      <div className="dialog-text">{children}</div>
      {error && (
        <InfoBar severity="error" live>
          {error}
        </InfoBar>
      )}
    </Dialog>
  );
}
