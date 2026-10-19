import { memo, useId, useMemo, useState, type FormEvent, type KeyboardEvent } from "react";
import { backend, type SharedTextView, type TransferView } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { Icon } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { Page } from "../components/Page";
import { ProgressBar } from "../components/ProgressBar";
import { Select, type SelectOption } from "../components/Select";
import { noTargetReason, TargetList, targetArg } from "../components/SendFiles";
import { CardGroup, EmptyState } from "../components/SettingsCard";
import { TextArea } from "../components/TextBox";
import { useAction } from "../hooks/useAction";
import { useApp } from "../hooks/useApp";
import { useNow } from "../hooks/useNow";
import { useToast } from "../hooks/useToast";
import { cx } from "../lib/cx";
import { formatEta, formatSpeed, isFinished, peersFor, transferPercent, transferState, transferTitle } from "../lib/features";
import { formatBytes, formatRelative, plural } from "../lib/format";
import type { Navigate } from "../lib/navigation";

const MAX_SHARE_CHARS = 65_536;
const ALL = "__all__";

// ---------------------------------------------------------------- send files

function SendPanel({ onNavigate }: { onNavigate: Navigate }) {
  const { snapshot, settings } = useApp();
  const toast = useToast();
  const { busy, run } = useAction();
  const { available, unavailable } = peersFor(snapshot, settings, "files");
  const [choice, setChoice] = useState<string | null>(null);
  const target = available.find((p) => p.id === choice)?.id ?? available[0]?.id ?? null;
  const reason = noTargetReason(snapshot, settings);
  const isClient = snapshot.role === "client";
  const name = useId();

  const pick = async () => {
    const ok = await run(
      "pick",
      async () => {
        const paths = await backend.api.pickFiles();
        if (paths.length === 0) return;
        await backend.api.sendFiles(targetArg(snapshot, target), paths);
        toast.notify("success", `Offered ${plural(paths.length, "file")} to ${available.find((p) => p.id === target)?.name ?? "the other computer"}.`);
      },
      "Couldn't send the files",
    );
    return ok;
  };

  return (
    <CardGroup title="Send files">
      <div className={cx("send-panel", reason && "is-blocked")}>
        <div className="drop-zone">
          <span className="drop-zone-icon">
            <Icon name="dropFiles" size={32} />
          </span>
          <span className="drop-zone-title">Drag files onto this window</span>
          <span className="drop-zone-text">{reason ?? "Or choose them yourself. They go only to the computer you pick."}</span>
          <Button variant="accent" icon="upload" busy={busy === "pick"} disabled={reason !== null || target === null} onClick={() => void pick()}>
            Send files…
          </Button>
          {reason && !settings.transfer.enabled && (
            <Button variant="subtle" size="small" onClick={() => onNavigate("features", "transfer")}>
              File transfer settings
            </Button>
          )}
        </div>
        {(available.length > 1 || unavailable.length > 0) && !isClient ? (
          <div className="send-targets">
            <span className="send-targets-title" id={`${name}-label`}>
              Send to
            </span>
            <TargetList name={name} available={available} unavailable={unavailable} value={target} onChange={setChoice} />
          </div>
        ) : (
          available.length === 1 && (
            <p className="send-to">
              <Icon name={isClient ? "desktop" : "laptop"} /> Files go to <strong>{available[0].name}</strong>
            </p>
          )
        )}
      </div>
    </CardGroup>
  );
}

// ---------------------------------------------------------------- transfer list

interface RowProps {
  t: TransferView;
  now: number;
  busy: string | null;
  run: (key: string, fn: () => Promise<unknown>, errorTitle?: string) => Promise<boolean>;
}

const TransferRow = memo(function TransferRow({ t, now, busy, run }: RowProps) {
  const state = transferState(t);
  const pct = transferPercent(t);
  const title = transferTitle(t);
  const send = t.direction === "send";
  const eta = formatEta(t);
  const key = (a: string) => `${t.id}:${a}`;
  const act = (a: string, fn: () => Promise<unknown>, err: string) => void run(key(a), fn, err);

  let detail: string;
  switch (t.state) {
    case "active":
      detail = `${formatBytes(t.doneBytes)} of ${formatBytes(t.totalBytes)} · ${t.bytesPerSec > 0 ? formatSpeed(t.bytesPerSec) : "starting…"}${eta ? ` · ${eta}` : ""}`;
      break;
    case "pending":
      detail = send ? `${formatBytes(t.totalBytes)} · waiting for ${t.peerName} to accept` : `${formatBytes(t.totalBytes)} · ${t.peerName} wants to send this`;
      break;
    case "done":
      detail = `${formatBytes(t.totalBytes)} · ${formatRelative(t.finishedAt ?? t.startedAt, now)}${t.destination ? ` · saved in ${t.destination}` : ""}`;
      break;
    case "failed":
      detail = `${t.error ?? "Something went wrong."} · ${formatBytes(t.doneBytes)} of ${formatBytes(t.totalBytes)} sent`;
      break;
    default:
      detail = `${formatBytes(t.totalBytes)} · ${formatRelative(t.finishedAt ?? t.startedAt, now)}`;
  }

  return (
    <li className={cx("transfer", `is-${t.state}`, t.state === "pending" && !send && "needs-answer")}>
      <span className={cx("transfer-icon", send ? "is-send" : "is-receive")}>
        <Icon name={send ? "upload" : "download"} size={20} />
      </span>
      <div className="transfer-main">
        <div className="transfer-head">
          <span className="transfer-title" title={t.files.map((f) => f.name).join("\n")}>
            {title}
          </span>
          <Badge tone={state.tone}>{state.label}</Badge>
        </div>
        <span className="transfer-peer">
          {send ? "To" : "From"} {t.peerName}
          {t.files.length > 1 && ` · ${plural(t.files.length, "file")}`}
        </span>
        {(t.state === "active" || (t.state === "failed" && t.doneBytes > 0)) && (
          <ProgressBar
            value={pct}
            active={t.state === "active"}
            tone={t.state === "failed" ? "critical" : "accent"}
            label={`${send ? "Sending" : "Receiving"} ${title}`}
            valueText={`${Math.round(pct)} percent, ${formatBytes(t.doneBytes)} of ${formatBytes(t.totalBytes)}`}
          />
        )}
        <span className="transfer-detail tabular">{detail}</span>
      </div>
      <div className="transfer-actions">
        {t.state === "pending" && !send && (
          <>
            <Button variant="accent" icon="checkmark" busy={busy === key("accept")} onClick={() => act("accept", () => backend.api.respondTransfer(t.id, true), "Couldn't accept")}>
              Accept
            </Button>
            <Button icon="dismiss" busy={busy === key("decline")} onClick={() => act("decline", () => backend.api.respondTransfer(t.id, false), "Couldn't decline")}>
              Decline
            </Button>
          </>
        )}
        {(t.state === "active" || (t.state === "pending" && send)) && (
          <Button icon="dismiss" busy={busy === key("cancel")} aria-label={`Cancel ${title}`} onClick={() => act("cancel", () => backend.api.cancelTransfer(t.id), "Couldn't cancel")}>
            Cancel
          </Button>
        )}
        {t.state === "done" && t.destination && (
          <Button icon="folder" busy={busy === key("open")} onClick={() => act("open", () => backend.api.openTransferLocation(t.id), "Couldn't open the folder")}>
            Show in folder
          </Button>
        )}
        {t.canRetry && (t.state === "failed" || t.state === "cancelled") && (
          <Button icon="refresh" busy={busy === key("retry")} aria-label={`Retry ${title}`} onClick={() => act("retry", () => backend.api.retryTransfer(t.id), "Couldn't retry")}>
            Retry
          </Button>
        )}
      </div>
    </li>
  );
});

function TransferList() {
  const { snapshot } = useApp();
  const { busy, run } = useAction();
  const now = useNow(30_000);
  const transfers = snapshot.features.transfers;
  const finished = transfers.filter((t) => isFinished(t.state)).length;
  // Requests that need an answer first, then the rest in the backend's order (newest first).
  const { waiting, ordered } = useMemo(() => {
    const w = transfers.filter((t) => t.state === "pending" && t.direction === "receive");
    return { waiting: w, ordered: [...w, ...transfers.filter((t) => !w.includes(t))] };
  }, [transfers]);

  return (
    <CardGroup
      title="Transfers"
      description={transfers.length > 0 ? `${plural(transfers.length, "transfer")} · the newest 50 are kept until Moush quits` : undefined}
      actions={
        <Button variant="subtle" icon="delete" disabled={finished === 0} busy={busy === "clear"} onClick={() => void run("clear", () => backend.api.clearFinishedTransfers(), "Couldn't clear")}>
          Clear finished
        </Button>
      }
    >
      {waiting.length > 0 && (
        <InfoBar severity="warning" title={waiting.length === 1 ? "A computer wants to send you files" : `${waiting.length} transfers are waiting for you`}>
          Accept to save them in your download folder, or decline.
        </InfoBar>
      )}
      {transfers.length === 0 ? (
        <div className="settings-card">
          <EmptyState icon="transfer" title="No transfers yet">
            Files you send or receive show up here with their progress.
          </EmptyState>
        </div>
      ) : (
        <ul className="list-card transfer-list" aria-label="Transfers">
          {ordered.map((t) => (
            <TransferRow key={t.id} t={t} now={now} busy={busy?.startsWith(t.id) ? busy : null} run={run} />
          ))}
        </ul>
      )}
    </CardGroup>
  );
}

// ---------------------------------------------------------------- shared text

function ShareComposer() {
  const { snapshot, settings } = useApp();
  const toast = useToast();
  const { busy, run } = useAction();
  const [text, setText] = useState("");
  const [choice, setChoice] = useState<string>(ALL);
  const inputId = useId();
  const countId = useId();
  const { available, unavailable } = peersFor(snapshot, settings, "textShare");
  const isClient = snapshot.role === "client";

  const options: SelectOption<string>[] = useMemo(
    () => [...(available.length > 1 ? [{ value: ALL, label: "All connected computers" }] : []), ...available.map((p) => ({ value: p.id, label: p.name }))],
    [available],
  );
  const target = options.some((o) => o.value === choice) ? choice : (options[0]?.value ?? null);
  const trimmed = text.trim();
  const tooLong = trimmed.length > MAX_SHARE_CHARS;
  const isLink = /^https?:\/\/\S+$/i.test(trimmed);
  const blocked = available.length === 0;
  const reason =
    snapshot.features.peers.length === 0
      ? isClient
        ? "Connect to a host to share text."
        : "No computers are connected yet."
      : blocked
        ? (unavailable[0]?.reason ?? "Text sharing is turned off for the connected computers.")
        : null;

  const send = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!trimmed || tooLong || target === null) return;
    const peerId = target === ALL ? null : target;
    const ok = await run("share", () => backend.api.shareText(targetArg(snapshot, peerId), trimmed), "Couldn't share");
    if (ok) {
      setText("");
      toast.notify("success", `Shared ${isLink ? "the link" : "the text"} with ${target === ALL ? "all connected computers" : (available.find((p) => p.id === target)?.name ?? "the host")}.`);
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void send();
  };

  return (
    <form className="settings-card composer" onSubmit={(e) => void send(e)} noValidate>
      <label htmlFor={inputId} className="composer-label">
        <Icon name={isLink ? "link" : "note"} size={20} />
        Share a text, link or snippet
      </label>
      <TextArea
        id={inputId}
        value={text}
        rows={3}
        placeholder={reason ?? "Paste or type here. Links open with one click on the other computer."}
        disabled={blocked}
        invalid={tooLong}
        aria-describedby={countId}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKeyDown}
      />
      <div className="composer-footer">
        <span id={countId} className={cx("composer-count", tooLong && "is-error")}>
          {tooLong ? `Too long: ${trimmed.length.toLocaleString()} of ${MAX_SHARE_CHARS.toLocaleString()} characters` : isLink ? "Link" : `${trimmed.length.toLocaleString()} characters`}
          {!blocked && " · Ctrl+Enter to send"}
        </span>
        <span className="composer-actions">
          {!isClient && options.length > 1 && target !== null && <Select value={target} options={options} onChange={setChoice} label="Share with" />}
          {(isClient || options.length === 1) && available[0] && <span className="composer-to">To {available[0].name}</span>}
          <Button type="submit" variant="accent" icon="send" busy={busy === "share"} disabled={blocked || !trimmed || tooLong}>
            Share
          </Button>
        </span>
      </div>
    </form>
  );
}

const SharedItem = memo(function SharedItem({ item, now, run, busy }: { item: SharedTextView; now: number; busy: string | null; run: RowProps["run"] }) {
  const received = item.direction === "received";
  const key = (a: string) => `${item.id}:${a}`;
  const toast = useToast();
  return (
    <li className={cx("shared", received ? "is-received" : "is-sent")}>
      <span className="shared-icon">
        <Icon name={item.isLink ? "globe" : "note"} size={20} />
      </span>
      <div className="shared-main">
        <span className="shared-meta">
          {received ? `From ${item.peerName}` : `Sent to ${item.peerName}`} · {formatRelative(item.at, now)}
          {item.chars > item.preview.length && ` · ${item.chars.toLocaleString()} characters`}
        </span>
        <span className={cx("shared-text selectable", item.isLink && "is-link")}>{item.preview}</span>
      </div>
      <div className="shared-actions">
        <Button
          variant="subtle"
          icon="copy"
          busy={busy === key("copy")}
          title="Copy"
          aria-label="Copy to the clipboard"
          onClick={() =>
            void run(key("copy"), async () => {
              await backend.api.copySharedText(item.id);
              toast.notify("success", "Copied to the clipboard.");
            }, "Couldn't copy")
          }
        />
        {item.isLink && received && (
          <Button icon="openExternal" busy={busy === key("open")} onClick={() => void run(key("open"), () => backend.api.openSharedLink(item.id), "Couldn't open the link")}>
            Open link
          </Button>
        )}
        <Button variant="subtle" icon="dismiss" title="Dismiss" aria-label="Dismiss" busy={busy === key("dismiss")} onClick={() => void run(key("dismiss"), () => backend.api.dismissSharedText(item.id), "Couldn't dismiss")} />
      </div>
    </li>
  );
});

function SharedList() {
  const { snapshot } = useApp();
  const { busy, run } = useAction();
  const now = useNow(30_000);
  const items = snapshot.features.sharedTexts;
  return (
    <CardGroup title="Shared text and links">
      <ShareComposer />
      {items.length === 0 ? (
        <div className="settings-card">
          <EmptyState icon="note" title="Nothing shared yet">
            Texts and links shared between your computers appear here until Moush quits.
          </EmptyState>
        </div>
      ) : (
        <ul className="list-card shared-list" aria-label="Shared text and links">
          {items.map((s) => (
            <SharedItem key={s.id} item={s} now={now} busy={busy?.startsWith(s.id) ? busy : null} run={run} />
          ))}
        </ul>
      )}
    </CardGroup>
  );
}

// ---------------------------------------------------------------- page

export function TransfersPage({ onNavigate }: { onNavigate: Navigate }) {
  const { snapshot } = useApp();
  const active = snapshot.features.transfers.filter((t) => t.state === "active");
  const speed = active.reduce((n, t) => n + t.bytesPerSec, 0);
  return (
    <Page
      title="Transfers"
      subtitle={active.length > 0 ? `${plural(active.length, "transfer")} in progress · ${formatSpeed(speed)}` : "Send files and text between your computers"}
    >
      <SendPanel onNavigate={onNavigate} />
      <TransferList />
      <SharedList />
    </Page>
  );
}
