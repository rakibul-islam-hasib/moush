import { memo, type ReactNode } from "react";
import { backend, type AppSnapshot, type ClientSnapshot, type FeaturesSnapshot, type HostSnapshot, type Settings } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { Icon, type IconName } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { Page } from "../components/Page";
import { ProgressBar } from "../components/ProgressBar";
import { CardGroup } from "../components/SettingsCard";
import { useAction } from "../hooks/useAction";
import { useApp, useSettings } from "../hooks/useApp";
import { useNow } from "../hooks/useNow";
import { cx } from "../lib/cx";
import { CLIP_KIND_LABEL, transferPercent, transferState, transferTitle } from "../lib/features";
import { formatMs, formatRelative, plural } from "../lib/format";
import type { Navigate } from "../lib/navigation";
import { clientStatus, cursorOwnerName, hostStatus, type Tone } from "../lib/status";

// ---------------------------------------------------------------- connection hero

type LinkState = "connected" | "waiting" | "reconnecting" | "error" | "off";

const LINK_TONE: Record<LinkState, Tone> = {
  connected: "success",
  waiting: "accent",
  reconnecting: "caution",
  error: "critical",
  off: "neutral",
};

/** Two computers and the link between them; the link animates with the connection state. */
function ConnectionArt({ state, local, remote, count }: { state: LinkState; local: IconName; remote: IconName; count: number }) {
  return (
    <div className={cx("conn", `conn-${state}`)} aria-hidden="true">
      <span className="conn-node conn-local">
        <Icon name={local} size={24} />
      </span>
      <span className="conn-link">
        <svg viewBox="0 0 120 12" preserveAspectRatio="none">
          <path className="conn-track" d="M2 6H118" />
          <path className="conn-flow" d="M2 6H118" />
        </svg>
        {state === "error" && (
          <span className="conn-break">
            <Icon name="dismiss" size={16} />
          </span>
        )}
      </span>
      <span className="conn-node conn-remote">
        <Icon name={remote} size={24} />
        {count > 1 && <span className="conn-count">{count}</span>}
      </span>
    </div>
  );
}

interface HeroProps {
  state: LinkState;
  title: string;
  detail: string;
  local: IconName;
  remote: IconName;
  count: number;
  busy?: boolean;
  actions: ReactNode;
  children?: ReactNode;
}

function ConnectionHero({ state, title, detail, local, remote, count, busy, actions, children }: HeroProps) {
  const tone = LINK_TONE[state];
  return (
    <section className={cx("hero", `tone-${tone}`)} aria-label="Connection status">
      <div className="hero-text" aria-busy={busy || undefined}>
        <span className={cx("hero-state", `tone-${tone}`)}>
          <span className="hero-state-dot" aria-hidden="true" />
          {STATE_LABEL[state]}
        </span>
        <h2 className="hero-title">{title}</h2>
        <p className="hero-detail">{detail}</p>
        {children}
        <div className="hero-actions">{actions}</div>
      </div>
      <ConnectionArt state={state} local={local} remote={remote} count={count} />
    </section>
  );
}

const STATE_LABEL: Record<LinkState, string> = {
  connected: "Connected",
  waiting: "Waiting",
  reconnecting: "Reconnecting",
  error: "Problem",
  off: "Off",
};

function hostLinkState(host: HostSnapshot): LinkState {
  if (host.error) return "error";
  if (!host.running) return "off";
  return host.clients.length > 0 ? "connected" : "waiting";
}

function clientLinkState(c: ClientSnapshot): LinkState {
  switch (c.state) {
    case "connected":
      return "connected";
    case "connecting":
    case "pairing":
      return "waiting";
    case "reconnecting":
      return "reconnecting";
    case "error":
      return "error";
    case "idle":
      return "off";
  }
}

function HostHero({ host }: { host: HostSnapshot }) {
  const { busy, run } = useAction();
  const status = hostStatus(host);
  const state = hostLinkState(host);
  return (
    <ConnectionHero
      state={state}
      title={status.title}
      detail={status.detail}
      local="desktop"
      remote="laptop"
      count={host.clients.length}
      actions={
        host.running ? (
          <Button icon="stop" busy={busy === "toggle"} onClick={() => void run("toggle", () => backend.api.hostStop())}>
            Stop sharing
          </Button>
        ) : (
          <Button variant="accent" icon="play" busy={busy === "toggle"} onClick={() => void run("toggle", () => backend.api.hostStart())}>
            Start sharing
          </Button>
        )
      }
    />
  );
}

function ClientHero({ client, snapshotAt, lastHost, onNavigate }: { client: ClientSnapshot; snapshotAt: number; lastHost: string | null; onNavigate: Navigate }) {
  const { busy, run } = useAction();
  const now = useNow(500, client.state === "reconnecting" && client.retryInMs !== null);
  const retryLeft = client.retryInMs !== null ? Math.max(0, snapshotAt + client.retryInMs - now) : null;
  const status = clientStatus(client, retryLeft);
  const inProgress = client.state === "connecting" || client.state === "pairing" || client.state === "reconnecting";

  const actions =
    client.state === "connected" ? (
      <Button icon="unlink" busy={busy === "conn"} onClick={() => void run("conn", () => backend.api.clientDisconnect())}>
        Disconnect
      </Button>
    ) : inProgress ? (
      <Button icon="dismiss" busy={busy === "conn"} onClick={() => void run("conn", () => backend.api.clientDisconnect())}>
        Stop connecting
      </Button>
    ) : (
      <>
        {lastHost && (
          <Button variant="accent" icon="link" busy={busy === "conn"} onClick={() => void run("conn", () => backend.api.clientConnect(lastHost, null))}>
            {client.state === "error" ? "Try again" : `Connect to ${lastHost}`}
          </Button>
        )}
        <Button variant={lastHost ? "standard" : "accent"} icon="devices" onClick={() => onNavigate("devices")}>
          Choose a host
        </Button>
      </>
    );

  return (
    <ConnectionHero
      state={clientLinkState(client)}
      title={status.title}
      detail={status.detail}
      local="laptop"
      remote="desktop"
      count={1}
      busy={inProgress}
      actions={actions}
    />
  );
}

// ---------------------------------------------------------------- quick toggles

interface QuickToggleProps {
  icon: IconName;
  label: string;
  checked: boolean;
  detail: string;
  onChange: (on: boolean) => void;
}

function QuickToggle({ icon, label, checked, detail, onChange }: QuickToggleProps) {
  return (
    <button type="button" role="switch" aria-checked={checked} className={cx("quick-tile", checked && "is-on")} onClick={() => onChange(!checked)}>
      <span className="quick-tile-icon">
        <Icon name={icon} size={20} />
      </span>
      <span className="quick-tile-text">
        <span className="quick-tile-label">{label}</span>
        <span className="quick-tile-state">
          {checked ? "On" : "Off"} · {detail}
        </span>
      </span>
      <span className="quick-tile-switch" aria-hidden="true">
        <span className="toggle-knob" />
      </span>
    </button>
  );
}

const QuickToggles = memo(function QuickToggles({ settings, isHost, onNavigate }: { settings: Settings; isHost: boolean; onNavigate: Navigate }) {
  const { update } = useSettings();
  const c = settings.clipboard;
  const clipKinds = [c.text && "text", c.images && "images"].filter(Boolean).join(" and ") || "nothing selected";
  return (
    <CardGroup
      title="Quick settings"
      actions={
        <Button variant="subtle" size="small" icon="sliders" onClick={() => onNavigate("features")}>
          All features
        </Button>
      }
    >
      <div className="quick-grid">
        <QuickToggle
          icon="clipboard"
          label="Clipboard sync"
          checked={c.enabled}
          detail={c.enabled ? `Shares ${clipKinds}` : "Copies stay on each computer"}
          onChange={(on) => update((s) => ({ ...s, clipboard: { ...s.clipboard, enabled: on } }))}
        />
        {isHost ? (
          <QuickToggle
            icon="keyboard"
            label="Keyboard forwarding"
            checked={settings.keyboard.forwardKeyboard}
            detail={settings.keyboard.forwardKeyboard ? "Typing follows the cursor" : "Only the mouse moves"}
            onChange={(on) => update((s) => ({ ...s, keyboard: { ...s.keyboard, forwardKeyboard: on } }))}
          />
        ) : (
          <QuickToggle
            icon="bell"
            label="Notifications"
            checked={settings.notifications.enabled}
            detail={settings.notifications.enabled ? "Windows notifications" : "Silent"}
            onChange={(on) => update((s) => ({ ...s, notifications: { ...s.notifications, enabled: on } }))}
          />
        )}
        <QuickToggle
          icon="transfer"
          label="File transfer"
          checked={settings.transfer.enabled}
          detail={settings.transfer.enabled ? (settings.transfer.askBeforeReceiving ? "Asks before receiving" : "Accepts automatically") : "Can't send or receive"}
          onChange={(on) => update((s) => ({ ...s, transfer: { ...s.transfer, enabled: on } }))}
        />
      </div>
    </CardGroup>
  );
});

// ---------------------------------------------------------------- active computer & activity

function ActiveComputer({ snapshot }: { snapshot: AppSnapshot }) {
  let name: string;
  let detail: string;
  let here = false;
  let locked = false;
  if (snapshot.host) {
    const owner = snapshot.host.running ? cursorOwnerName(snapshot.host) : null;
    here = owner === null;
    name = owner ?? snapshot.deviceName;
    locked = snapshot.host.locked;
    const rtt = snapshot.host.clients.find((c) => c.active)?.rttMs ?? null;
    detail = !snapshot.host.running
      ? "Sharing is off, so the mouse and keyboard stay here."
      : here
        ? "Move the cursor past a screen edge to use another computer."
        : `Round trip ${formatMs(rtt)}. Press Ctrl+Alt+Esc to come back.`;
  } else {
    const c = snapshot.client;
    const connected = c?.state === "connected";
    here = !!c?.active;
    name = here ? snapshot.deviceName : connected ? (c?.hostName ?? "the host") : snapshot.deviceName;
    detail = !connected ? "Not connected to a host." : here ? "You're controlling this computer from the host." : `Round trip ${formatMs(c?.rttMs)}.`;
  }
  return (
    <div className="active-card">
      <span className={cx("active-card-icon", here && "is-here")}>
        <Icon name="cursor" size={20} />
      </span>
      <div className="active-card-text">
        <span className="active-card-label">Has the mouse and keyboard</span>
        <span className="active-card-name">
          {name}
          {here && <Badge tone="accent">This computer</Badge>}
          {locked && (
            <Badge tone="caution">
              <Icon name="lock" size={16} /> Locked
            </Badge>
          )}
        </span>
        <span className="active-card-detail">{detail}</span>
      </div>
    </div>
  );
}

interface ActivityRowProps {
  icon: IconName;
  title: string;
  detail: ReactNode;
  extra?: ReactNode;
  onClick: () => void;
}

function ActivityRow({ icon, title, detail, extra, onClick }: ActivityRowProps) {
  return (
    <li>
      <button type="button" className="activity-row" onClick={onClick}>
        <span className="activity-icon">
          <Icon name={icon} size={20} />
        </span>
        <span className="activity-text">
          <span className="activity-title">{title}</span>
          <span className="activity-detail">{detail}</span>
          {extra}
        </span>
        <Icon name="chevronRight" className="settings-card-chevron" />
      </button>
    </li>
  );
}

const RecentActivity = memo(function RecentActivity({ features, onNavigate }: { features: FeaturesSnapshot; onNavigate: Navigate }) {
  const now = useNow(30_000);
  const clip = features.clipboard;
  const transfer = features.transfers[0];
  const text = features.sharedTexts[0];

  const clipDetail = clip.error
    ? clip.error
    : !clip.enabled
      ? "Clipboard sync is off."
      : clip.lastSyncAt && clip.lastDirection
        ? `${clip.lastDirection === "received" ? "Received" : "Sent"} ${clip.lastKind ? CLIP_KIND_LABEL[clip.lastKind].toLowerCase() : "a copy"} ${clip.lastDirection === "received" ? "from" : "to"} ${clip.lastPeer ?? "another computer"} · ${formatRelative(clip.lastSyncAt, now)}`
        : "Nothing synchronized yet. Copy something to share it.";

  return (
    <CardGroup title="Recent activity">
      <ul className="list-card activity-list">
        <ActivityRow icon="clipboard" title="Clipboard" detail={clipDetail} onClick={() => onNavigate("features", "history")} />
        {transfer ? (
          <ActivityRow
            icon={transfer.direction === "send" ? "upload" : "download"}
            title={transferTitle(transfer)}
            detail={`${transferState(transfer).label} · ${transfer.direction === "send" ? "to" : "from"} ${transfer.peerName} · ${formatRelative(transfer.finishedAt ?? transfer.startedAt, now)}`}
            extra={
              transfer.state === "active" && (
                <ProgressBar value={transferPercent(transfer)} label={`Progress of ${transferTitle(transfer)}`} active className="activity-progress" />
              )
            }
            onClick={() => onNavigate("transfers")}
          />
        ) : (
          <ActivityRow icon="transfer" title="File transfers" detail="No files sent or received yet." onClick={() => onNavigate("transfers")} />
        )}
        {text && (
          <ActivityRow
            icon={text.isLink ? "link" : "note"}
            title={text.direction === "received" ? `${text.isLink ? "Link" : "Text"} from ${text.peerName}` : `${text.isLink ? "Link" : "Text"} sent to ${text.peerName}`}
            detail={`${text.preview.slice(0, 90)}${text.preview.length > 90 ? "…" : ""} · ${formatRelative(text.at, now)}`}
            onClick={() => onNavigate("transfers")}
          />
        )}
      </ul>
    </CardGroup>
  );
});

function Shortcuts({ settings, isHost }: { settings: Settings; isHost: boolean }) {
  if (!isHost) return null;
  return (
    <CardGroup title="Shortcuts">
      <div className="shortcut-strip">
        <span className="shortcut-item">
          <span className="kbd-group">
            <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Esc</kbd>
          </span>
          Bring the cursor back
        </span>
        {settings.keyboard.switchHotkeys && (
          <span className="shortcut-item">
            <span className="kbd-group">
              <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>→</kbd>
            </span>
            Jump to the next computer
          </span>
        )}
        {settings.scrollLockToggle && (
          <span className="shortcut-item">
            <span className="kbd-group">
              <kbd>Scroll Lock</kbd>
            </span>
            Lock the cursor
          </span>
        )}
      </div>
    </CardGroup>
  );
}

// ---------------------------------------------------------------- page

export function HomePage({ onNavigate }: { onNavigate: Navigate }) {
  const { snapshot, snapshotAt, settings } = useApp();
  const isHost = snapshot.role === "host";
  const host = snapshot.host;
  const client = snapshot.client;

  return (
    <Page title="Home" subtitle={`${snapshot.deviceName} · ${isHost ? "Host" : "Client"}`}>
      {isHost && host ? (
        <HostHero host={host} />
      ) : client ? (
        <ClientHero client={client} snapshotAt={snapshotAt} lastHost={settings.lastHostAddress} onNavigate={onNavigate} />
      ) : (
        <InfoBar severity="informational">Waiting for the background service to report its status…</InfoBar>
      )}

      {host?.error && (
        <InfoBar severity="error" title="Sharing problem">
          {host.error}
        </InfoBar>
      )}
      {host?.running && !host.captureActive && (
        <InfoBar severity="warning" title="Mouse and keyboard aren't being captured">
          Moush couldn't install its input hooks, so the cursor can't move to other computers. Try stopping and starting sharing.
        </InfoBar>
      )}

      <div className="home-grid">
        <ActiveComputer snapshot={snapshot} />
        {isHost && host && (
          <button type="button" className="mini-stat" onClick={() => onNavigate("devices")}>
            <span className="mini-stat-value">{host.clients.length}</span>
            <span className="mini-stat-label">{plural(host.clients.length, "computer").replace(/^\d+ /, "")} connected</span>
            <Icon name="chevronRight" className="mini-stat-chevron" />
          </button>
        )}
      </div>

      <QuickToggles settings={settings} isHost={isHost} onNavigate={onNavigate} />
      <RecentActivity features={snapshot.features} onNavigate={onNavigate} />
      <Shortcuts settings={settings} isHost={isHost} />
    </Page>
  );
}
