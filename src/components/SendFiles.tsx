import { useId, useRef, useState } from "react";
import { backend, type AppSnapshot, type FeaturePeer, type Settings } from "../api";
import { useToast } from "../hooks/useToast";
import { cx } from "../lib/cx";
import { peersFor } from "../lib/features";
import { plural } from "../lib/format";
import { Button } from "./Button";
import { Dialog, useClosable } from "./Dialog";
import { Icon } from "./Icon";

/** peerId for api.sendFiles / api.shareText: the client role always addresses its host (null). */
export function targetArg(snapshot: AppSnapshot, peerId: string | null): string | null {
  return snapshot.role === "client" ? null : peerId;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** Why nothing can receive files right now, or null when at least one peer can. */
export function noTargetReason(snapshot: AppSnapshot, settings: Settings): string | null {
  if (!settings.transfer.enabled) return "File transfer is turned off. Turn it on in Features › File transfer.";
  if (snapshot.features.peers.length === 0) {
    return snapshot.role === "client" ? "Connect to a host to send files." : "No computers are connected yet.";
  }
  const { available, unavailable } = peersFor(snapshot, settings, "files");
  if (available.length > 0) return null;
  return unavailable.length === 1 ? unavailable[0].reason : "None of the connected computers accepts files right now.";
}

/** Full-window overlay while files are dragged over Moush. */
export function DropOverlay({ snapshot, settings }: { snapshot: AppSnapshot; settings: Settings }) {
  const reason = noTargetReason(snapshot, settings);
  const { available } = peersFor(snapshot, settings, "files");
  const title = reason
    ? "Can't send files right now"
    : available.length === 1
      ? `Drop to send to ${available[0].name}`
      : "Drop to send files";
  const detail = reason ?? (available.length === 1 ? "The files go straight to that computer." : "You'll choose the computer next.");
  return (
    <div className={cx("drop-overlay", reason && "is-blocked")} role="status" aria-live="assertive">
      <div className="drop-overlay-frame">
        <span className="drop-overlay-icon">
          <Icon name={reason ? "block" : "dropFiles"} size={32} />
        </span>
        <span className="drop-overlay-title">{title}</span>
        <span className="drop-overlay-text">{detail}</span>
      </div>
    </div>
  );
}

interface SendDialogProps {
  snapshot: AppSnapshot;
  settings: Settings;
  paths: string[];
  onSent: () => void;
  onClose: () => void;
}

/** Asks which computer gets the files when several can receive them. */
export function SendTargetDialog({ snapshot, settings, paths, onSent, onClose }: SendDialogProps) {
  const toast = useToast();
  const { available, unavailable } = peersFor(snapshot, settings, "files");
  const [choice, setChoice] = useState<string | null>(available[0]?.id ?? null);
  const [busy, setBusy] = useState(false);
  const { closing, close } = useClosable(onClose);
  const sendRef = useRef<HTMLButtonElement>(null);
  const name = useId();

  const send = async () => {
    if (!choice) return;
    setBusy(true);
    try {
      await backend.api.sendFiles(targetArg(snapshot, choice), paths);
      onSent();
      close();
    } catch (e) {
      toast.error(e, "Couldn't send the files");
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={`Send ${plural(paths.length, "file")} to…`}
      closing={closing}
      onDismiss={busy ? undefined : close}
      initialFocus={sendRef}
      footer={
        <>
          <Button ref={sendRef} variant="accent" icon="send" busy={busy} disabled={!choice} onClick={() => void send()}>
            Send
          </Button>
          <Button disabled={busy} onClick={close}>
            Cancel
          </Button>
        </>
      }
    >
      <p className="dialog-text text-secondary">{paths.map(fileName).slice(0, 3).join(", ") + (paths.length > 3 ? ` and ${paths.length - 3} more` : "")}</p>
      <TargetList name={name} available={available} unavailable={unavailable} value={choice} onChange={setChoice} />
    </Dialog>
  );
}

interface TargetListProps {
  name: string;
  available: FeaturePeer[];
  unavailable: { peer: FeaturePeer; reason: string }[];
  value: string | null;
  onChange: (id: string) => void;
  /** Extra first option, e.g. "All connected computers". */
  allOption?: { id: string; label: string };
}

/** Radio list of computers; unavailable ones are shown disabled with the reason. */
export function TargetList({ name, available, unavailable, value, onChange, allOption }: TargetListProps) {
  return (
    <div className="target-list" role="radiogroup" aria-label="Computer">
      {allOption && (
        <label className={cx("target", value === allOption.id && "is-selected")}>
          <input type="radio" name={name} checked={value === allOption.id} onChange={() => onChange(allOption.id)} />
          <Icon name="devices" size={20} />
          <span className="target-text">
            <span className="target-name">{allOption.label}</span>
            <span className="target-desc">{plural(available.length, "computer")}</span>
          </span>
        </label>
      )}
      {available.map((p) => (
        <label key={p.id} className={cx("target", value === p.id && "is-selected")}>
          <input type="radio" name={name} checked={value === p.id} onChange={() => onChange(p.id)} />
          <Icon name="laptop" size={20} />
          <span className="target-text">
            <span className="target-name">{p.name}</span>
            <span className="target-desc">Connected</span>
          </span>
        </label>
      ))}
      {unavailable.map(({ peer, reason }) => (
        <label key={peer.id} className="target is-disabled">
          <input type="radio" name={name} disabled checked={false} onChange={() => undefined} />
          <Icon name="block" size={20} />
          <span className="target-text">
            <span className="target-name">{peer.name}</span>
            <span className="target-desc">{reason}</span>
          </span>
        </label>
      ))}
    </div>
  );
}
