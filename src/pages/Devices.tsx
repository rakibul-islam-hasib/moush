import { memo, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { backend, type AppSnapshot, type ClientSnapshot, type DevicePolicy, type FeaturePeer, type HostSnapshot, type TrustedDevice } from "../api";
import { Badge, StatusDot } from "../components/Badge";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Expander, ExpanderRow } from "../components/Expander";
import { Icon, type IconName } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { Page } from "../components/Page";
import { ProgressRing } from "../components/ProgressRing";
import { CardGroup, EmptyState, LinkCard, SettingsCard } from "../components/SettingsCard";
import { Tabs, type TabItem } from "../components/Tabs";
import { TextBox } from "../components/TextBox";
import { ToggleSwitch } from "../components/ToggleSwitch";
import { useAction } from "../hooks/useAction";
import { useSettings, useSnapshot } from "../hooks/useApp";
import { useNow } from "../hooks/useNow";
import { useToast } from "../hooks/useToast";
import { cx } from "../lib/cx";
import { formatMs, formatRelative } from "../lib/format";
import type { Navigate } from "../lib/navigation";
import { clientStatus } from "../lib/status";
import { LayoutTab } from "./Layout";

type DevicesTabId = "devices" | "layout";

const TABS: readonly TabItem<DevicesTabId>[] = [
  { id: "devices", label: "Devices", icon: "devices" },
  { id: "layout", label: "Screen layout", icon: "layout" },
];

// ---------------------------------------------------------------- shared pieces

const CAPS: { key: keyof Omit<FeaturePeer, "id" | "name">; label: string; icon: IconName }[] = [
  { key: "clipboard", label: "Clipboard", icon: "clipboard" },
  { key: "files", label: "Files", icon: "transfer" },
  { key: "textShare", label: "Text", icon: "note" },
];

/** What both computers currently allow (effective capabilities); off is stated in text. */
function CapChips({ peer }: { peer: FeaturePeer | undefined }) {
  if (!peer) return null;
  return (
    <span className="cap-chips">
      {CAPS.map((c) => (
        <span key={c.key} className={cx("cap-chip", !peer[c.key] && "is-off")}>
          <Icon name={c.icon} size={16} />
          {c.label}
          {!peer[c.key] && <span className="cap-chip-off"> off</span>}
        </span>
      ))}
    </span>
  );
}

function OnlineBadge({ online }: { online: boolean }) {
  return (
    <span className={cx("online-badge", online && "is-online")}>
      <StatusDot tone={online ? "success" : "neutral"} />
      {online ? "Online" : "Offline"}
    </span>
  );
}

const POLICY_ROWS: { key: keyof DevicePolicy; title: string; desc: (name: string) => string; icon: IconName }[] = [
  { key: "clipboard", title: "Clipboard sync", desc: (n) => `Share copied text and images with ${n}.`, icon: "clipboard" },
  { key: "files", title: "File transfer", desc: (n) => `Send files to ${n} and accept files from it.`, icon: "transfer" },
  { key: "textShare", title: "Shared text and links", desc: (n) => `Exchange texts, links and snippets with ${n}.`, icon: "note" },
];

/** A paired device with its permissions. Policy changes are optimistic and roll back on error. */
const PairedDevice = memo(function PairedDevice({ device, peer, onForget }: { device: TrustedDevice; peer: FeaturePeer | undefined; onForget: (d: TrustedDevice) => void }) {
  const toast = useToast();
  const now = useNow(30_000);
  const [pending, setPending] = useState<DevicePolicy | null>(null);
  const policy = pending ?? device.policy;

  // The snapshot caught up: drop the optimistic value.
  useEffect(() => {
    if (pending && CAPS.every((c) => pending[c.key] === device.policy[c.key])) setPending(null);
  }, [device.policy, pending]);

  const set = (key: keyof DevicePolicy, value: boolean) => {
    const next = { ...policy, [key]: value };
    setPending(next);
    backend.api.setDevicePolicy(device.id, next).catch((e: unknown) => {
      setPending(null);
      toast.error(e, `Couldn't change the permissions for ${device.name}`);
    });
  };

  const allowed = CAPS.filter((c) => policy[c.key]).map((c) => c.label.toLowerCase());
  return (
    <Expander
      icon={device.role === "host" ? "desktop" : "laptop"}
      title={
        <span className="title-with-badge">
          {device.name}
          <OnlineBadge online={device.online} />
        </span>
      }
      description={
        <>
          <span className="mono">{device.fingerprint}</span>
          {" · "}
          {device.online ? "Connected now" : device.lastSeen ? `Last seen ${formatRelative(device.lastSeen, now)}` : "Never seen"}
          {" · "}
          {allowed.length === 3 ? "All features allowed" : allowed.length === 0 ? "Features blocked" : `Allows ${allowed.join(", ")}`}
        </>
      }
    >
      {POLICY_ROWS.map((row) => (
        <ExpanderRow key={row.key} title={row.title} description={row.desc(device.name)}>
          {(ids) => <ToggleSwitch checked={policy[row.key]} onChange={(v) => set(row.key, v)} labelledBy={ids.label} describedBy={ids.desc} />}
        </ExpanderRow>
      ))}
      {device.online && peer && (
        <ExpanderRow title="Working right now" description="What both computers allow at the moment (each side can turn features off).">
          <CapChips peer={peer} />
        </ExpanderRow>
      )}
      <ExpanderRow
        title="Forget this device"
        description={`${device.name} will need a new pairing code to connect again.${device.lastAddress ? ` Last address ${device.lastAddress}.` : ""}`}
      >
        <Button variant="danger" icon="delete" onClick={() => onForget(device)} aria-label={`Forget ${device.name}`}>
          Forget
        </Button>
      </ExpanderRow>
    </Expander>
  );
});

function PairedList({ snapshot, title, emptyText }: { snapshot: AppSnapshot; title: string; emptyText: string }) {
  const [forgetting, setForgetting] = useState<TrustedDevice | null>(null);
  const devices = snapshot.trusted;
  const peers = snapshot.features.peers;
  return (
    <CardGroup title={title} description="Paired devices connect without asking again. Open a device to choose what it may do with this computer.">
      {devices.length === 0 ? (
        <div className="settings-card">
          <EmptyState icon="shield" title="No paired devices">
            {emptyText}
          </EmptyState>
        </div>
      ) : (
        devices.map((d) => <PairedDevice key={d.id} device={d} peer={peers.find((p) => p.id === d.id)} onForget={setForgetting} />)
      )}
      {forgetting && (
        <ConfirmDialog
          title={`Forget ${forgetting.name}?`}
          confirmLabel="Forget"
          destructive
          onConfirm={() => backend.api.forgetDevice(forgetting.id)}
          onClose={() => setForgetting(null)}
        >
          {forgetting.name} will have to be paired again, with a verification code on both computers, before it can connect.
        </ConfirmDialog>
      )}
    </CardGroup>
  );
}

function ThisComputer({ snapshot, onNavigate }: { snapshot: AppSnapshot; onNavigate: Navigate }) {
  return (
    <CardGroup title="This computer">
      <SettingsCard
        icon={snapshot.role === "client" ? "laptop" : "desktop"}
        title={snapshot.deviceName}
        description={
          <>
            Fingerprint <span className="mono">{snapshot.fingerprint}</span> · Compare it with the code shown when pairing
          </>
        }
      >
        <Badge tone="accent">{snapshot.role === "client" ? "Client" : "Host"}</Badge>
      </SettingsCard>
      <LinkCard icon="pulse" title="Connection diagnostics" description="Latency test, firewall, network adapters and the log" onClick={() => onNavigate("diagnostics")} />
    </CardGroup>
  );
}

// ---------------------------------------------------------------- host

function HostDevices({ snapshot, host, onNavigate }: { snapshot: AppSnapshot; host: HostSnapshot; onNavigate: Navigate }) {
  const { busy, run } = useAction();
  const now = useNow(30_000);
  const peers = snapshot.features.peers;
  return (
    <>
      <CardGroup title="Connected computers">
        <ul className="list-card" aria-label="Connected computers">
          {host.clients.length === 0 ? (
            <li className="list-row">
              <EmptyState icon="devices" title={host.running ? "No computers connected" : "Sharing is off"}>
                {host.running
                  ? `On another computer, install Moush, choose Client and connect to ${snapshot.deviceName}.`
                  : "Start sharing on the Home page so other computers can connect."}
              </EmptyState>
            </li>
          ) : (
            host.clients.map((c) => (
              <li key={c.id} className="list-row device-row">
                <Icon name="laptop" size={20} className="settings-card-icon" />
                <div className="settings-card-text">
                  <span className="settings-card-title title-with-badge">
                    {c.name}
                    {c.active && <Badge tone="accent">Has the cursor</Badge>}
                  </span>
                  <span className="settings-card-desc">
                    {c.address} · Round trip <span className="tabular">{formatMs(c.rttMs)}</span> · Connected {formatRelative(c.connectedAt, now)}
                  </span>
                  <CapChips peer={peers.find((p) => p.id === c.id)} />
                </div>
                <div className="settings-card-control">
                  <Button icon="unlink" busy={busy === c.id} aria-label={`Disconnect ${c.name}`} onClick={() => void run(c.id, () => backend.api.hostDisconnectClient(c.id))}>
                    Disconnect
                  </Button>
                </div>
              </li>
            ))
          )}
        </ul>
      </CardGroup>
      <PairedList snapshot={snapshot} title="Paired devices" emptyText="When a client pairs with this computer, it appears here." />
      <ThisComputer snapshot={snapshot} onNavigate={onNavigate} />
    </>
  );
}

// ---------------------------------------------------------------- client

const SCAN_INTERVAL_MS = 4000;

function ManualConnect({ disabled }: { disabled: boolean }) {
  const { settings } = useSettings();
  const { busy, run } = useAction();
  const [address, setAddress] = useState(settings.lastHostAddress ?? "");
  const [error, setError] = useState<string | null>(null);
  const inputId = useId();
  const errorId = useId();

  const validate = (text: string): string | null => {
    const value = text.trim();
    if (!value) return "Enter the host's IP address or computer name.";
    if (/\s/.test(value)) return "The address can't contain spaces.";
    const port = value.match(/:(\d*)$/);
    if (port) {
      const n = Number(port[1]);
      if (!port[1] || !Number.isInteger(n) || n < 1 || n > 65535) return "The port after ':' must be a number from 1 to 65535.";
    }
    return null;
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const problem = validate(address);
    setError(problem);
    if (!problem) void run("connect", () => backend.api.clientConnect(address.trim(), null));
  };

  return (
    <form className="settings-card manual-connect" onSubmit={submit} noValidate>
      <div className="settings-card-row">
        <Icon name="link" size={20} className="settings-card-icon" />
        <div className="settings-card-text">
          <label className="settings-card-title" htmlFor={inputId}>
            Connect by address
          </label>
          <span className="settings-card-desc">IP address or computer name, optionally with a port, e.g. 192.168.1.20 or STUDIO-PC:24800</span>
        </div>
      </div>
      <div className="manual-connect-row">
        <TextBox
          id={inputId}
          value={address}
          placeholder="192.168.1.20"
          invalid={error !== null}
          aria-describedby={error ? errorId : undefined}
          onChange={(e) => {
            setAddress(e.target.value);
            if (error) setError(null);
          }}
        />
        <Button type="submit" variant="accent" busy={busy === "connect"} disabled={disabled}>
          Connect
        </Button>
      </div>
      {error && (
        <p className="field-error" id={errorId} role="alert">
          {error}
        </p>
      )}
    </form>
  );
}

function ClientDevices({ snapshot, client, snapshotAt, onNavigate }: { snapshot: AppSnapshot; client: ClientSnapshot; snapshotAt: number; onNavigate: Navigate }) {
  const { settings } = useSettings();
  const toast = useToast();
  const { busy, run } = useAction();
  const now = useNow(1000);
  const [scanning, setScanning] = useState(false);
  const scanTimer = useRef<number | undefined>(undefined);
  const idle = client.state === "idle" || client.state === "error";
  const discoveryOn = settings.discoveryEnabled;

  const scan = useRef(async () => {
    setScanning(true);
    window.clearTimeout(scanTimer.current);
    scanTimer.current = window.setTimeout(() => setScanning(false), 1500);
    try {
      await backend.api.discoverHosts();
    } catch (e) {
      toast.error(e, "Couldn't search for hosts");
    }
  });

  // Scan once when the page opens…
  useEffect(() => {
    if (discoveryOn) void scan.current();
    return () => window.clearTimeout(scanTimer.current);
  }, [discoveryOn]);

  // …and keep scanning while there's nothing connected.
  useEffect(() => {
    if (!idle || !discoveryOn) return;
    const id = window.setInterval(() => void scan.current(), SCAN_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [idle, discoveryOn]);

  const retryLeft = client.retryInMs !== null ? Math.max(0, snapshotAt + client.retryInMs - now) : null;
  const status = clientStatus(client, retryLeft);
  const busyConnecting = !idle;

  return (
    <>
      {!idle && (
        <InfoBar
          severity={client.state === "connected" ? "success" : client.state === "reconnecting" ? "warning" : "informational"}
          title={status.title}
          action={
            <Button icon={client.state === "connected" ? "unlink" : "dismiss"} busy={busy === "disconnect"} onClick={() => void run("disconnect", () => backend.api.clientDisconnect())}>
              {client.state === "connected" ? "Disconnect" : "Stop connecting"}
            </Button>
          }
        >
          {status.detail}
        </InfoBar>
      )}
      {client.state === "error" && (
        <InfoBar severity="error" title={status.title}>
          {status.detail}
        </InfoBar>
      )}

      <CardGroup
        title="Hosts on this network"
        actions={
          <Button icon="refresh" busy={scanning} onClick={() => void scan.current()}>
            {scanning ? "Searching…" : "Search again"}
          </Button>
        }
      >
        {!discoveryOn && (
          <InfoBar severity="informational">Automatic search is turned off in Settings. Select Search again to look once, or connect by address below.</InfoBar>
        )}
        <ul className="list-card" aria-label="Hosts on this network">
          {snapshot.discovered.length === 0 ? (
            <li className="list-row">
              {scanning ? <ProgressRing size={20} /> : <Icon name="wifi" size={20} className="settings-card-icon" />}
              <div className="settings-card-text">
                <span className="settings-card-title">{scanning ? "Searching for hosts…" : "No hosts found yet"}</span>
                <span className="settings-card-desc">
                  Make sure Moush is running as Host on the other computer, both computers are on the same network, and Windows Firewall allows Moush on
                  Private networks.
                </span>
              </div>
            </li>
          ) : (
            snapshot.discovered.map((h) => {
              const isCurrent = client.hostId === h.id && client.state === "connected";
              return (
                <li key={h.id} className="list-row">
                  <Icon name="desktop" size={20} className="settings-card-icon" />
                  <div className="settings-card-text">
                    <span className="settings-card-title title-with-badge">
                      {h.name}
                      {isCurrent && <Badge tone="success">Connected</Badge>}
                      {h.trusted && !isCurrent && <Badge>Paired</Badge>}
                    </span>
                    <span className="settings-card-desc">
                      {h.address} · Seen {formatRelative(h.lastSeen, now)}
                    </span>
                  </div>
                  <div className="settings-card-control">
                    <Button
                      variant={isCurrent ? "standard" : "accent"}
                      icon="link"
                      busy={busy === h.id}
                      disabled={busyConnecting}
                      aria-label={`Connect to ${h.name}`}
                      onClick={() => void run(h.id, () => backend.api.clientConnect(null, h.id))}
                    >
                      {isCurrent ? "Connected" : "Connect"}
                    </Button>
                  </div>
                </li>
              );
            })
          )}
        </ul>
        <ManualConnect disabled={busyConnecting} />
      </CardGroup>

      <PairedList snapshot={snapshot} title="Paired hosts" emptyText="After this computer pairs with a host, it appears here." />
      <ThisComputer snapshot={snapshot} onNavigate={onNavigate} />
    </>
  );
}

// ---------------------------------------------------------------- page

export function DevicesPage({ tab, onNavigate }: { tab: DevicesTabId; onNavigate: Navigate }) {
  const { snapshot, snapshotAt } = useSnapshot();
  const online = snapshot.trusted.filter((t) => t.online).length;
  return (
    <Page title="Devices" subtitle={`${online} of ${snapshot.trusted.length} paired ${snapshot.trusted.length === 1 ? "device" : "devices"} online`}>
      <Tabs items={TABS} value={tab} onChange={(t) => onNavigate(t === "layout" ? "layout" : "devices")} label="Devices sections" idBase="devices" />
      <div className="tab-panel" id="devices-panel" role="tabpanel" aria-labelledby={`devices-tab-${tab}`} key={tab}>
        {tab === "layout" ? (
          <LayoutTab />
        ) : snapshot.role === "host" && snapshot.host ? (
          <HostDevices snapshot={snapshot} host={snapshot.host} onNavigate={onNavigate} />
        ) : snapshot.role === "client" && snapshot.client ? (
          <ClientDevices snapshot={snapshot} client={snapshot.client} snapshotAt={snapshotAt} onNavigate={onNavigate} />
        ) : null}
      </div>
    </Page>
  );
}
