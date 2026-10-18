import { useState } from "react";
import { backend, type PairingRequest } from "../api";
import { useNow } from "../hooks/useNow";
import { errorMessage } from "../lib/errors";
import { formatCode, formatCountdown, plural } from "../lib/format";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";
import { InfoBar } from "./InfoBar";
import { ProgressRing } from "./ProgressRing";

/** Shown whenever the snapshot has pending pairing requests; handles the oldest first. */
export function PairingDialog({ requests }: { requests: PairingRequest[] }) {
  const request = requests[0];
  if (!request) return null;
  return <PairingRequestDialog key={request.requestId} request={request} more={requests.length - 1} />;
}

function PairingRequestDialog({ request, more }: { request: PairingRequest; more: number }) {
  const now = useNow(1000);
  const [busy, setBusy] = useState<"allow" | "reject" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const remaining = request.expiresAt - now;
  const expired = remaining <= 0;
  const peer = request.peerName;

  const respond = async (accept: boolean) => {
    setBusy(accept ? "allow" : "reject");
    setError(null);
    try {
      await backend.api.respondPairing(request.requestId, accept);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const title = request.needsMyApproval
    ? request.peerRole === "client"
      ? `Allow ${peer} to connect?`
      : `Pair with ${peer}?`
    : `Waiting for ${peer}`;

  const footer = request.needsMyApproval ? (
    <>
      <Button variant="accent" icon="checkmark" busy={busy === "allow"} disabled={busy !== null || expired} onClick={() => void respond(true)}>
        Allow
      </Button>
      <Button icon="dismiss" busy={busy === "reject"} disabled={busy !== null} onClick={() => void respond(false)}>
        Reject
      </Button>
    </>
  ) : (
    <>
      <span className="pairing-waiting" role="status">
        <ProgressRing size={16} />
        Waiting for approval on {peer}…
      </span>
      <Button busy={busy === "reject"} disabled={busy !== null} onClick={() => void respond(false)}>
        Cancel
      </Button>
    </>
  );

  return (
    <Dialog title={title} footer={footer} className="pairing-dialog">
      <div className="pairing-peer">
        <Icon name={request.peerRole === "host" ? "desktop" : "laptop"} size={32} />
        <div>
          <div className="pairing-peer-name">{peer}</div>
          <div className="text-secondary">
            {request.peerRole === "host" ? "Host" : "Client"} · {request.peerAddress}
          </div>
        </div>
      </div>
      <div className="pairing-code-block">
        <span className="pairing-code-label" id={`code-${request.requestId}`}>
          Verification code
        </span>
        <span className="pairing-code" aria-labelledby={`code-${request.requestId}`} aria-live="off">
          {formatCode(request.code)}
        </span>
      </div>
      <p className="dialog-text">
        Make sure the same code is shown on <strong>{peer}</strong>.{" "}
        {request.needsMyApproval
          ? "If the codes don't match, select Reject."
          : "Approve the request there to finish pairing."}
      </p>
      <p className={expired ? "pairing-expiry is-expired" : "pairing-expiry"}>
        {expired ? "This request has expired." : `Expires in ${formatCountdown(remaining)}`}
        {more > 0 && ` · ${plural(more, "more request")} waiting`}
      </p>
      {error && (
        <InfoBar severity="error" live>
          {error}
        </InfoBar>
      )}
    </Dialog>
  );
}
