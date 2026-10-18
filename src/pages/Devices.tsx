import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { backend, type AppSnapshot, type ClientSnapshot, type HostSnapshot, type TrustedDevice } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Icon } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { Page } from "../components/Page";
import { ProgressRing } from "../components/ProgressRing";
import { CardGroup, ListCard, ListRow, SettingsCard } from "../components/SettingsCard";
import { TextBox } from "../components/TextBox";
import { useAction } from "../hooks/useAction";
import { useApp } from "../hooks/useApp";
import { useNow } from "../hooks/useNow";
import { useToast } from "../hooks/useToast";
import { formatMs, formatRelative } from "../lib/format";
import { clientStatus } from "../lib/status";

function ThisDevice({ snapshot }: { snapshot: AppSnapshot }) {
  return (
    <CardGroup title="This computer">
      <SettingsCard
        icon={snapshot.role === "client" ? "laptop" : "desktop"}
        title={snapshot.deviceName}
        description={
          <>
            Fingerprint <span className="mono">{snapshot.fingerprint}</span>
          </>
        }
      >
        <Badge tone="accent">{snapshot.role === "client" ? "Client" : "Host"}</Badge>
      </SettingsCard>
    </CardGroup>
  );
}

function TrustedList({ devices, title, emptyText }: { devices: TrustedDevice[]; title: string; emptyText: string }) {
  const now = useNow(30_000);
  const [forgetting, setForgetting] = useState<TrustedDevice | null>(null);
  return (
    <CardGroup title={title} description="Paired devices connect without asking again. Forget a device to require pairing next time.">
      <ListCard label={title}>
        {devices.length === 0 ? (
          <ListRow icon="shield" title="No trusted devices" description={emptyText} />
        ) : (
          devices.map((d) => (
            <ListRow
              key={d.id}
              icon={d.role === "host" ? "desktop" : "laptop"}
              title={d.name}
              description={
                <>
                  <span className="mono">{d.fingerprint}</span>
                  {" · "}
                  {d.lastSeen ? `Last seen ${formatRelative(d.lastSeen, now)}` : "Never seen"}
                  {d.lastAddress && ` · ${d.lastAddress}`}
                </>
              }
            >
              <Button onClick={() => setForgetting(d)} aria-label={`Forget ${d.name}`}>
                Forget
              </Button>
            </ListRow>
          ))
        )}
      </ListCard>
      {forgetting && (
        <ConfirmDialog
          title={`Forget ${forgetting.name}?`}
          confirmLabel="Forget"
          onConfirm={() => backend.api.forgetDevice(forgetting.id)}
          onClose={() => setForgetting(null)}
        >
          {forgetting.name} will have to be paired again, with a verification code on both computers, before it can connect.
        </ConfirmDialog>
      )}
    </CardGroup>
  );
}

function HostDevices({ snapshot, host }: { snapshot: AppSnapshot; host: HostSnapshot }) {
  const { busy, run } = useAction();
  const now = useNow(30_000);
  return (
    <>
      <CardGroup title="Connected devices">
        <ListCard label="Connected devices">
          {host.clients.length === 0 ? (
            <ListRow
              icon="devices"
              title="No devices connected"
              description={
                host.running
                  ? `On another computer, install Moush, choose Client and connect to ${snapshot.deviceName}.`
                  : "Sharing is off. Start sharing on the Home page so devices can connect."
              }
            />
          ) : (
            host.clients.map((c) => (
              <ListRow
                key={c.id}
                icon="laptop"
                title={
                  <span className="title-with-badge">
                    {c.name}
                    {c.active && <Badge tone="accent">Has the cursor</Badge>}
                  </span>
                }
                description={`${c.address} · Round trip ${formatMs(c.rttMs)} · Connected ${formatRelative(c.connectedAt, now)}`}
              >
                <Button
                  icon="unlink"
                  busy={busy === c.id}
                  aria-label={`Disconnect ${c.name}`}
                  onClick={() => void run(c.id, () => backend.api.hostDisconnectClient(c.id))}
                >
                  Disconnect
                </Button>
              </ListRow>
            ))
          )}
        </ListCard>
      </CardGroup>
      <TrustedList
        devices={snapshot.trusted}
        title="Trusted devices"
        emptyText="When a client pairs with this computer, it appears here."
      />
    </>
  );
}

const SCAN_INTERVAL_MS = 4000;

function ManualConnect({ disabled }: { disabled: boolean }) {
  const { settings } = useApp();
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

function ClientDevices({ snapshot, client, snapshotAt }: { snapshot: AppSnapshot; client: ClientSnapshot; snapshotAt: number }) {
  const { settings } = useApp();
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
          <InfoBar severity="informational">
            Automatic search is turned off in Settings. Select Search again to look once, or connect by address below.
          </InfoBar>
        )}
        <ListCard label="Hosts on this network">
          {snapshot.discovered.length === 0 ? (
            <li className="list-row">
              {scanning ? <ProgressRing size={20} /> : <Icon name="wifi" size={20} className="settings-card-icon" />}
              <div className="settings-card-text">
                <span className="settings-card-title">{scanning ? "Searching for hosts…" : "No hosts found yet"}</span>
                <span className="settings-card-desc">
                  Make sure Moush is running as Host on the other computer, both computers are on the same network, and
                  Windows Firewall allows Moush on Private networks.
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
                      {h.trusted && !isCurrent && <Badge>Trusted</Badge>}
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
        </ListCard>
        <ManualConnect disabled={busyConnecting} />
      </CardGroup>

      <TrustedList
        devices={snapshot.trusted}
        title="Trusted hosts"
        emptyText="After this computer pairs with a host, it appears here."
      />
    </>
  );
}

export function DevicesPage() {
  const { snapshot, snapshotAt } = useApp();
  return (
    <Page title="Devices">
      {snapshot.role === "host" && snapshot.host && <HostDevices snapshot={snapshot} host={snapshot.host} />}
      {snapshot.role === "client" && snapshot.client && (
        <ClientDevices snapshot={snapshot} client={snapshot.client} snapshotAt={snapshotAt} />
      )}
      <ThisDevice snapshot={snapshot} />
    </Page>
  );
}
