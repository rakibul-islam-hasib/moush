import { memo, useEffect, useState } from "react";
import {
  backend,
  type ClipboardEntry,
  type ClipboardSettings,
  type KeyboardSettings,
  type NotificationSettings,
  type Settings,
  type TransferSettings,
} from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Expander, ExpanderRow } from "../components/Expander";
import { Icon, type IconName } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { Page } from "../components/Page";
import { ProgressRing } from "../components/ProgressRing";
import { Select, type SelectOption } from "../components/Select";
import { EmptyState } from "../components/SettingsCard";
import { ShortcutEditor } from "../components/ShortcutEditor";
import { Slider } from "../components/Slider";
import { ToggleSwitch } from "../components/ToggleSwitch";
import { useSettings, useSnapshot } from "../hooks/useApp";
import { useClipboardHistory } from "../hooks/useClipboardHistory";
import { useNow } from "../hooks/useNow";
import { useToast } from "../hooks/useToast";
import { CLIP_KIND_LABEL } from "../lib/features";
import { formatBytes, formatRelative, plural } from "../lib/format";
import type { FeatureAnchor, Navigate } from "../lib/navigation";

type Section = FeatureAnchor;

/** Setters for one nested settings section; `debounce` for sliders and text. */
function useSection() {
  const { settings, update } = useSettings();
  return {
    settings,
    clipboard: (p: Partial<ClipboardSettings>, debounce = false) => update((s) => ({ ...s, clipboard: { ...s.clipboard, ...p } }), { debounce }),
    keyboard: (p: Partial<KeyboardSettings>, debounce = false) => update((s) => ({ ...s, keyboard: { ...s.keyboard, ...p } }), { debounce }),
    transfer: (p: Partial<TransferSettings>, debounce = false) => update((s) => ({ ...s, transfer: { ...s.transfer, ...p } }), { debounce }),
    notifications: (p: Partial<NotificationSettings>) => update((s) => ({ ...s, notifications: { ...s.notifications, ...p } })),
    root: (p: Partial<Settings>, debounce = false) => update((s) => ({ ...s, ...p }), { debounce }),
  };
}

function Toggle({ checked, onChange, ids, disabled }: { checked: boolean; onChange: (v: boolean) => void; ids: { label: string; desc: string }; disabled?: boolean }) {
  return <ToggleSwitch checked={checked} onChange={onChange} labelledBy={ids.label} describedBy={ids.desc} disabled={disabled} />;
}

function SubHeading({ children }: { children: string }) {
  return <h4 className="expander-subheading">{children}</h4>;
}

// ---------------------------------------------------------------- clipboard

const SIZE_PRESETS = [256, 1024, 5 * 1024, 10 * 1024, 25 * 1024, 64 * 1024];

function sizeLabel(kb: number): string {
  return kb < 1024 ? `${kb} KB` : `${Math.round((kb / 1024) * 10) / 10} MB`;
}

function ClipboardCard({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const { settings, clipboard } = useSection();
  const { snapshot } = useSnapshot();
  const c = settings.clipboard;
  const off = !c.enabled;
  const sizeOptions: SelectOption<string>[] = [...new Set([...SIZE_PRESETS, c.maxSizeKb])].sort((a, b) => a - b).map((kb) => ({ value: String(kb), label: sizeLabel(kb) }));
  const peers = snapshot.features.peers.filter((p) => p.clipboard).length;

  return (
    <Expander
      id="feature-clipboard"
      icon="clipboard"
      title="Clipboard sync"
      description={c.enabled ? `Copy on one computer, paste on the other${peers > 0 ? ` · active with ${plural(peers, "computer")}` : ""}` : "Off — copies stay on each computer"}
      open={open}
      onOpenChange={onOpenChange}
      headerControl={(ids) => <Toggle checked={c.enabled} onChange={(v) => clipboard({ enabled: v })} ids={ids} />}
    >
      {snapshot.features.clipboard.error && (
        <InfoBar severity="warning" title="Clipboard problem" className="expander-infobar">
          {snapshot.features.clipboard.error}
        </InfoBar>
      )}
      <SubHeading>What to share</SubHeading>
      <ExpanderRow title="Plain text" description="Text you copy in any app" disabled={off}>
        {(ids) => <Toggle checked={c.text} onChange={(v) => clipboard({ text: v })} ids={ids} disabled={off} />}
      </ExpanderRow>
      <ExpanderRow title="Formatting" description="Keep fonts, links and tables (HTML and RTF) along with the text" disabled={off || !c.text}>
        {(ids) => <Toggle checked={c.richText} onChange={(v) => clipboard({ richText: v })} ids={ids} disabled={off || !c.text} />}
      </ExpanderRow>
      <ExpanderRow title="Images" description="Screenshots and copied pictures" disabled={off}>
        {(ids) => <Toggle checked={c.images} onChange={(v) => clipboard({ images: v })} ids={ids} disabled={off} />}
      </ExpanderRow>
      <ExpanderRow title="Size limit" description="Larger copies stay on the computer where they were made" disabled={off}>
        {(ids) => (
          <Select
            value={String(c.maxSizeKb)}
            options={sizeOptions}
            onChange={(v) => clipboard({ maxSizeKb: Number(v) })}
            labelledBy={ids.label}
            describedBy={ids.desc}
          />
        )}
      </ExpanderRow>
      <SubHeading>Privacy</SubHeading>
      <ExpanderRow title="Skip private content" description="Don't share copies that apps such as password managers mark as private" disabled={off}>
        {(ids) => <Toggle checked={c.skipSensitive} onChange={(v) => clipboard({ skipSensitive: v })} ids={ids} disabled={off} />}
      </ExpanderRow>
      <ExpanderRow title="Keep a history" description="Recent copies stay in memory so you can paste them again; never saved to disk">
        {(ids) => <Toggle checked={c.history} onChange={(v) => clipboard({ history: v })} ids={ids} />}
      </ExpanderRow>
      <ExpanderRow title="History length" description="How many copies to remember" disabled={!c.history}>
        {(ids) => (
          <Slider
            value={c.historyLimit}
            min={1}
            max={200}
            step={1}
            format={(v) => plural(v, "item")}
            onChange={(v) => clipboard({ historyLimit: v }, true)}
            labelledBy={ids.label}
            describedBy={ids.desc}
            disabled={!c.history}
          />
        )}
      </ExpanderRow>
    </Expander>
  );
}

const KIND_ICON: Record<ClipboardEntry["kind"], IconName> = { text: "text", rich: "document", image: "image" };
const ORIGIN_LABEL: Record<ClipboardEntry["origin"], string> = { local: "Copied here", sent: "Copied here and shared", received: "Received" };

const HistoryItem = memo(function HistoryItem({ entry, now, onCopy, onDelete }: { entry: ClipboardEntry; now: number; onCopy: (e: ClipboardEntry) => void; onDelete: (e: ClipboardEntry) => void }) {
  return (
    <li className="history-item">
      <span className="history-kind" title={CLIP_KIND_LABEL[entry.kind]}>
        <Icon name={KIND_ICON[entry.kind]} size={20} />
      </span>
      <div className="history-text">
        <span className={entry.kind === "image" ? "history-preview is-image" : "history-preview"}>{entry.preview || "(empty)"}</span>
        <span className="history-meta">
          {CLIP_KIND_LABEL[entry.kind]} · {ORIGIN_LABEL[entry.origin]}
          {entry.origin === "received" ? ` from ${entry.source}` : ""} · {formatBytes(entry.bytes)} · {formatRelative(entry.at, now)}
        </span>
      </div>
      <div className="history-actions">
        <Button variant="subtle" icon="copy" onClick={() => onCopy(entry)} aria-label={`Copy "${entry.preview.slice(0, 40)}" to the clipboard`} title="Copy to the clipboard" />
        <Button variant="subtle" icon="delete" onClick={() => onDelete(entry)} aria-label={`Delete "${entry.preview.slice(0, 40)}" from the history`} title="Delete" />
      </div>
    </li>
  );
});

function HistoryCard({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const { settings, clipboard } = useSection();
  const { snapshot } = useSnapshot();
  const toast = useToast();
  const now = useNow(30_000, open);
  const status = snapshot.features.clipboard;
  const { entries, error, removeLocal } = useClipboardHistory(status.historyRevision, open && settings.clipboard.history);
  const [confirmClear, setConfirmClear] = useState(false);

  const copy = (e: ClipboardEntry) =>
    backend.api.copyClipboardEntry(e.id).then(
      () => toast.notify("success", "Copied to the clipboard."),
      (err: unknown) => toast.error(err, "Couldn't copy"),
    );
  const remove = (e: ClipboardEntry) => {
    removeLocal(e.id);
    backend.api.deleteClipboardEntry(e.id).catch((err: unknown) => toast.error(err, "Couldn't delete the item"));
  };

  return (
    <>
    <Expander
      id="feature-history"
      icon="history"
      title="Clipboard history"
      description={settings.clipboard.history ? `${plural(status.historyCount, "item")} · kept in memory only` : "Off"}
      open={open}
      onOpenChange={onOpenChange}
      headerControl={
        <Button size="small" icon="delete" disabled={status.historyCount === 0} onClick={() => setConfirmClear(true)}>
          Clear all
        </Button>
      }
    >
      {!settings.clipboard.history ? (
        <EmptyState icon="history" title="History is off" action={<Button onClick={() => clipboard({ history: true })}>Turn on history</Button>}>
          Turn it on to paste earlier copies again. The history is never written to disk.
        </EmptyState>
      ) : error ? (
        <InfoBar severity="error" title="Couldn't load the history" className="expander-infobar">
          {error}
        </InfoBar>
      ) : entries === null ? (
        <div className="loading-row expander-loading">
          <ProgressRing size={16} />
          <span className="text-secondary">Loading…</span>
        </div>
      ) : entries.length === 0 ? (
        <EmptyState icon="clipboard" title="Nothing here yet">
          Text and images you copy on any connected computer show up here.
        </EmptyState>
      ) : (
        <ul className="history-list" aria-label="Clipboard history">
          {entries.map((e) => (
            <HistoryItem key={e.id} entry={e} now={now} onCopy={copy} onDelete={remove} />
          ))}
        </ul>
      )}
    </Expander>
      {/* Outside the expander: its body is inert while collapsed. */}
      {confirmClear && (
        <ConfirmDialog
          title="Clear the clipboard history?"
          confirmLabel="Clear all"
          destructive
          onConfirm={() => backend.api.clearClipboardHistory()}
          onClose={() => setConfirmClear(false)}
        >
          {plural(status.historyCount, "item")} will be removed from this computer's history. What's on the clipboard right now stays.
        </ConfirmDialog>
      )}
    </>
  );
}

// ---------------------------------------------------------------- keyboard & switching

const MODIFIERS: readonly SelectOption<KeyboardSettings["edgeSwitchModifier"]>[] = [
  { value: "none", label: "Always switch" },
  { value: "ctrl", label: "Only while Ctrl is held" },
  { value: "shift", label: "Only while Shift is held" },
  { value: "alt", label: "Only while Alt is held" },
];

function KeyboardCard({ open, onOpenChange, isHost, hostName }: { open: boolean; onOpenChange: (v: boolean) => void; isHost: boolean; hostName: string | null }) {
  const { settings, keyboard, root } = useSection();
  const k = settings.keyboard;

  if (!isHost) {
    return (
      <Expander id="feature-keyboard" icon="keyboard" title="Keyboard and switching" description="Focus when the cursor arrives" open={open} onOpenChange={onOpenChange}>
        <ExpanderRow
          title="Focus the window under the cursor"
          description={
            <>
              When the shared cursor arrives on this computer, bring the window under it to the front so typing goes there. Applies right
              away.
            </>
          }
        >
          {(ids) => <Toggle checked={k.focusOnEnter} onChange={(v) => keyboard({ focusOnEnter: v })} ids={ids} />}
        </ExpanderRow>
        <InfoBar severity="informational" className="expander-infobar">
          Keyboard forwarding, shortcuts, edge switching and pointer speed are set in Moush on the host{hostName ? ` (${hostName})` : ""}.
        </InfoBar>
      </Expander>
    );
  }

  const noKeys = !k.forwardKeyboard;
  return (
    <Expander
      id="feature-keyboard"
      icon="keyboard"
      title="Keyboard and switching"
      description={k.forwardKeyboard ? "Typing follows the cursor to the other computer" : "Only the mouse moves; typing stays here"}
      open={open}
      onOpenChange={onOpenChange}
    >
      <SubHeading>Keyboard</SubHeading>
      <ExpanderRow title="Forward the keyboard" description="Typing goes to the computer that has the cursor. Off: only the mouse moves.">
        {(ids) => <Toggle checked={k.forwardKeyboard} onChange={(v) => keyboard({ forwardKeyboard: v })} ids={ids} />}
      </ExpanderRow>
      <ExpanderRow title="Send system shortcuts" description="Alt+Tab, Win, Alt+F4 and Ctrl+Esc act on the other computer while it has the cursor" disabled={noKeys}>
        {(ids) => <Toggle checked={k.forwardSystemShortcuts} onChange={(v) => keyboard({ forwardSystemShortcuts: v })} ids={ids} disabled={noKeys} />}
      </ExpanderRow>
      <ExpanderRow
        title="Keep these shortcuts on this computer"
        description="They always act here, wherever the cursor is"
        disabled={noKeys}
        footer={<ShortcutEditor value={k.localShortcuts} onChange={(next) => keyboard({ localShortcuts: next })} disabled={noKeys} />}
      />

      <SubHeading>Switching computers</SubHeading>
      <ExpanderRow
        title="Switch with hotkeys"
        description={
          <>
            <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Shift</kbd>+arrow jumps to the neighbouring computer; +<kbd>Home</kbd> comes back
          </>
        }
      >
        {(ids) => <Toggle checked={k.switchHotkeys} onChange={(v) => keyboard({ switchHotkeys: v })} ids={ids} />}
      </ExpanderRow>
      <ExpanderRow title="Switch at screen edges" description="Hold a key to cross into another computer, so the cursor doesn't slip over by accident">
        {(ids) => <Select value={k.edgeSwitchModifier} options={MODIFIERS} onChange={(v) => keyboard({ edgeSwitchModifier: v })} labelledBy={ids.label} describedBy={ids.desc} />}
      </ExpanderRow>
      <ExpanderRow title="Switch delay" description="How long the cursor rests at an edge before it moves to another computer">
        {(ids) => (
          <Slider value={settings.switchDelayMs} min={0} max={1000} step={10} format={(v) => `${v} ms`} onChange={(v) => root({ switchDelayMs: v }, true)} labelledBy={ids.label} describedBy={ids.desc} />
        )}
      </ExpanderRow>
      <ExpanderRow title="Corner dead zone" description="Don't switch this close to a screen corner">
        {(ids) => (
          <Slider value={settings.cornerDeadZonePx} min={0} max={200} step={5} format={(v) => `${v} px`} onChange={(v) => root({ cornerDeadZonePx: v }, true)} labelledBy={ids.label} describedBy={ids.desc} />
        )}
      </ExpanderRow>
      <ExpanderRow title="Scroll Lock locks the cursor" description="Press Scroll Lock to keep the cursor on the current computer">
        {(ids) => <Toggle checked={settings.scrollLockToggle} onChange={(v) => root({ scrollLockToggle: v })} ids={ids} />}
      </ExpanderRow>

      <SubHeading>Pointer</SubHeading>
      <ExpanderRow title="Pointer speed on other computers" description="Faster or slower cursor while it's on a client">
        {(ids) => (
          <Slider
            value={settings.clientPointerSpeed}
            min={0.25}
            max={4}
            step={0.05}
            format={(v) => `${v.toFixed(2)}×`}
            onChange={(v) => root({ clientPointerSpeed: Math.round(v * 100) / 100 }, true)}
            labelledBy={ids.label}
            describedBy={ids.desc}
          />
        )}
      </ExpanderRow>
    </Expander>
  );
}

// ---------------------------------------------------------------- file transfer

const OVERWRITE: readonly SelectOption<TransferSettings["overwrite"]>[] = [
  { value: "rename", label: "Keep both (rename)" },
  { value: "overwrite", label: "Replace the old file" },
  { value: "skip", label: "Skip the new file" },
];

export const SPEEDS: readonly SelectOption<TransferSettings["speed"]>[] = [
  { value: "balanced", label: "Balanced" },
  { value: "fast", label: "Fast" },
];

function useDefaultDownloadDir(): string | null {
  const [dir, setDir] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    backend.api.defaultDownloadDir().then(
      (d) => !cancelled && setDir(d),
      () => !cancelled && setDir(null),
    );
    return () => {
      cancelled = true;
    };
  }, []);
  return dir;
}

function TransferCard({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const { settings, transfer } = useSection();
  const toast = useToast();
  const defaultDir = useDefaultDownloadDir();
  const [picking, setPicking] = useState(false);
  const t = settings.transfer;
  const off = !t.enabled;

  const pick = async () => {
    setPicking(true);
    try {
      const dir = await backend.api.pickFolder();
      if (dir) transfer({ downloadDir: dir });
    } catch (e) {
      toast.error(e, "Couldn't open the folder picker");
    } finally {
      setPicking(false);
    }
  };

  return (
    <Expander
      id="feature-transfer"
      icon="transfer"
      title="File transfer"
      description={t.enabled ? "Send files by dragging them onto Moush or from the Transfers page" : "Off — files can't be sent or received"}
      open={open}
      onOpenChange={onOpenChange}
      headerControl={(ids) => <Toggle checked={t.enabled} onChange={(v) => transfer({ enabled: v })} ids={ids} />}
    >
      <ExpanderRow
        title="Save received files in"
        description={
          t.downloadDir ? (
            <>
              <span className="mono path">{t.downloadDir}</span>
              {defaultDir && <span className="path-default"> · Default: {defaultDir}</span>}
            </>
          ) : (
            <>
              Default: <span className="mono path">{defaultDir ?? "Downloads\\Moush"}</span>
            </>
          )
        }
        disabled={off}
      >
        <Button icon="folder" busy={picking} disabled={off} onClick={() => void pick()}>
          Change…
        </Button>
        {t.downloadDir && (
          <Button variant="subtle" icon="reset" disabled={off} onClick={() => transfer({ downloadDir: "" })}>
            Use default
          </Button>
        )}
      </ExpanderRow>
      <ExpanderRow title="Ask before receiving" description="Off: files from paired devices that allow file transfer are accepted automatically" disabled={off}>
        {(ids) => <Toggle checked={t.askBeforeReceiving} onChange={(v) => transfer({ askBeforeReceiving: v })} ids={ids} disabled={off} />}
      </ExpanderRow>
      <ExpanderRow title="If a file with the same name exists" description="What to do with received files whose name is already taken" disabled={off}>
        {(ids) => <Select value={t.overwrite} options={OVERWRITE} onChange={(v) => transfer({ overwrite: v })} labelledBy={ids.label} describedBy={ids.desc} />}
      </ExpanderRow>
      <ExpanderRow
        title="Transfer speed"
        description={t.speed === "fast" ? "Fast: more data in flight; the mouse may feel slightly less smooth during big transfers" : "Balanced: keeps the mouse and keyboard smoothest"}
        disabled={off}
      >
        {(ids) => <Select value={t.speed} options={SPEEDS} onChange={(v) => transfer({ speed: v })} labelledBy={ids.label} describedBy={ids.desc} />}
      </ExpanderRow>
      <ExpanderRow title="Open the folder when done" description="Shows received files in Explorer. Moush never opens or runs the files themselves." disabled={off}>
        {(ids) => <Toggle checked={t.openFolderWhenDone} onChange={(v) => transfer({ openFolderWhenDone: v })} ids={ids} disabled={off} />}
      </ExpanderRow>
    </Expander>
  );
}

// ---------------------------------------------------------------- notifications

function NotificationsCard({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const { settings, notifications } = useSection();
  const n = settings.notifications;
  const off = !n.enabled;
  const rows: { key: keyof Omit<NotificationSettings, "enabled">; title: string; desc: string }[] = [
    { key: "connections", title: "Connections", desc: "A computer connected or disconnected" },
    { key: "transfers", title: "File transfers", desc: "Incoming requests and finished transfers" },
    { key: "sharedText", title: "Shared text and links", desc: "Something was shared with this computer" },
    { key: "clipboard", title: "Every clipboard sync", desc: "Can be noisy; off by default" },
  ];
  return (
    <Expander
      id="feature-notifications"
      icon="bell"
      title="Notifications"
      description={n.enabled ? "Windows notifications for what happens in the background" : "Off — Moush stays quiet"}
      open={open}
      onOpenChange={onOpenChange}
      headerControl={(ids) => <Toggle checked={n.enabled} onChange={(v) => notifications({ enabled: v })} ids={ids} />}
    >
      {rows.map((r) => (
        <ExpanderRow key={r.key} title={r.title} description={r.desc} disabled={off}>
          {(ids) => <Toggle checked={n[r.key]} onChange={(v) => notifications({ [r.key]: v })} ids={ids} disabled={off} />}
        </ExpanderRow>
      ))}
    </Expander>
  );
}

// ---------------------------------------------------------------- page

const ALL: Section[] = ["clipboard", "history", "keyboard", "transfer", "notifications"];

export function FeaturesPage({ anchor, expandAll = false }: { onNavigate: Navigate; anchor?: FeatureAnchor; expandAll?: boolean }) {
  const { snapshot } = useSnapshot();
  const { settings } = useSettings();
  const [open, setOpen] = useState<Set<Section>>(() => new Set(expandAll ? ALL : anchor ? [anchor] : []));
  const isHost = snapshot.role === "host";

  // Opening the page from a link: expand that card and bring it into view.
  useEffect(() => {
    if (!anchor) return;
    setOpen((prev) => (prev.has(anchor) ? prev : new Set(prev).add(anchor)));
    const id = window.setTimeout(() => document.getElementById(`feature-${anchor}`)?.scrollIntoView({ block: "start", behavior: "smooth" }), 60);
    return () => window.clearTimeout(id);
  }, [anchor]);

  const prop = (s: Section) => ({
    open: open.has(s),
    onOpenChange: (v: boolean) =>
      setOpen((prev) => {
        const next = new Set(prev);
        if (v) next.add(s);
        else next.delete(s);
        return next;
      }),
  });

  const on = [settings.clipboard.enabled, settings.transfer.enabled, settings.notifications.enabled].filter(Boolean).length;
  return (
    <Page title="Features" subtitle="Choose what your computers share besides the mouse and keyboard">
      <div className="feature-summary" aria-label="Summary">
        <SummaryPill icon="clipboard" label="Clipboard" on={settings.clipboard.enabled} />
        <SummaryPill icon="transfer" label="Files" on={settings.transfer.enabled} />
        {isHost && <SummaryPill icon="keyboard" label="Keyboard" on={settings.keyboard.forwardKeyboard} />}
        <SummaryPill icon="bell" label="Notifications" on={settings.notifications.enabled} />
        <span className="caption text-secondary">{on === 3 ? "Everything is on" : `${on} of 3 features on`}</span>
      </div>
      <section className="card-group" aria-label="Sharing">
        <ClipboardCard {...prop("clipboard")} />
        <HistoryCard {...prop("history")} />
        <TransferCard {...prop("transfer")} />
      </section>
      <section className="card-group" aria-label="Input">
        <KeyboardCard {...prop("keyboard")} isHost={isHost} hostName={snapshot.client?.hostName ?? null} />
      </section>
      <section className="card-group" aria-label="Notifications">
        <NotificationsCard {...prop("notifications")} />
      </section>
    </Page>
  );
}

function SummaryPill({ icon, label, on }: { icon: IconName; label: string; on: boolean }) {
  return (
    <Badge tone={on ? "accent" : "neutral"}>
      <Icon name={icon} size={16} />
      {label} {on ? "on" : "off"}
    </Badge>
  );
}
