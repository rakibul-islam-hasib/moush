import type { ReactNode } from "react";
import { backend, type AppSnapshot, type ClientSnapshot, type HostSnapshot } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { Icon, type IconName } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { Page } from "../components/Page";
import { CardGroup, LinkCard, ListCard, ListRow } from "../components/SettingsCard";
import { useAction } from "../hooks/useAction";
import { useApp } from "../hooks/useApp";
import { useNow } from "../hooks/useNow";
import { cx } from "../lib/cx";
import { formatMs, formatRelative } from "../lib/format";
import type { Navigate } from "../lib/navigation";
import { clientStatus, cursorOwnerName, hostStatus, type Tone } from "../lib/status";

const TONE_ICON: Record<Tone, IconName> = {
  neutral: "mouse",
  accent: "link",
  success: "successCircle",
  caution: "warning",
  critical: "errorCircle",
};

interface StatusHeroProps {
  tone: Tone;
  title: string;
  detail: string;
  busy?: boolean;
  children?: ReactNode;
  actions: ReactNode;
}

function StatusHero({ tone, title, detail, busy, children, actions }: StatusHeroProps) {
  return (
    <section className="status-hero" aria-label="Status">
      <span className={cx("status-hero-icon", `tone-${tone}`)}>
        <Icon name={TONE_ICON[tone]} size={32} />
      </span>
      <div className="status-hero-text" aria-busy={busy || undefined}>
        <h2 className="status-hero-title">{title}</h2>
        <p className="status-hero-detail">{detail}</p>
        {children}
      </div>
      <div className="status-hero-actions">{actions}</div>
    </section>
  );
}

function HotkeyHints({ scrollLock }: { scrollLock: boolean }) {
  return (
    <CardGroup title="Shortcuts">
      <ListCard>
        <ListRow
          icon="keyboard"
          title={
            <>
              <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Esc</kbd>
            </>
          }
          description="Returns the cursor to the host"
        />
        {scrollLock && (
          <ListRow icon="lock" title={<kbd>Scroll Lock</kbd>} description="Locks the cursor to the current computer" />
        )}
      </ListCard>
    </CardGroup>
  );
}

function HostHome({ host, onNavigate }: { host: HostSnapshot; onNavigate: Navigate }) {
  const { busy, run } = useAction();
  const now = useNow(30_000);
  const status = hostStatus(host);
  const owner = cursorOwnerName(host);

  return (
    <>
      <StatusHero
        tone={status.tone}
        title={status.title}
        detail={status.detail}
        actions={
          host.running ? (
            <Button icon="stop" busy={busy === "toggle"} onClick={() => void run("toggle", () => backend.api.hostStop())}>
              Stop sharing
            </Button>
          ) : (
            <Button
              variant="accent"
              icon="play"
              busy={busy === "toggle"}
              onClick={() => void run("toggle", () => backend.api.hostStart())}
            >
              Start sharing
            </Button>
          )
        }
      >
        {host.running && (
          <ul className="status-facts">
            <li>
              <Icon name="cursor" />
              {owner ? (
                <span>
                  The cursor is on <strong>{owner}</strong>
                </span>
              ) : (
                <span>The cursor is on this computer</span>
              )}
            </li>
            {host.locked && (
              <li>
                <Icon name="lock" />
                <span>Cursor locked to {owner ?? "this computer"} — press Scroll Lock to unlock</span>
              </li>
            )}
          </ul>
        )}
      </StatusHero>

      {host.error && (
        <InfoBar severity="error" title="Sharing problem">
          {host.error}
        </InfoBar>
      )}
      {host.running && !host.captureActive && (
        <InfoBar severity="warning" title="Mouse and keyboard aren't being captured">
          Moush couldn't install its input hooks, so the cursor can't move to other computers. Try stopping and starting
          sharing.
        </InfoBar>
      )}

      <CardGroup title="Connected devices">
        {host.clients.length === 0 ? (
          <ListCard>
            <ListRow
              icon="devices"
              title={host.running ? "No devices connected" : "Sharing is off"}
              description={
                host.running
                  ? "On another computer, install Moush, choose Client and connect to this computer."
                  : "Start sharing to let other computers connect."
              }
            />
          </ListCard>
        ) : (
          <ListCard label="Connected devices">
            {host.clients.map((c) => (
              <ListRow
                key={c.id}
                icon="laptop"
                title={
                  <span className="title-with-badge">
                    {c.name}
                    {c.active && <Badge tone="accent">Has the cursor</Badge>}
                  </span>
                }
                description={`${c.address} · Connected ${formatRelative(c.connectedAt, now)}`}
              >
                <span className="metric" title="Round-trip time">
                  {formatMs(c.rttMs)}
                </span>
              </ListRow>
            ))}
          </ListCard>
        )}
      </CardGroup>

      <CardGroup title="Quick links">
        <LinkCard
          icon="layout"
          title="Display layout"
          description="Place your devices next to this computer's screens"
          onClick={() => onNavigate("layout")}
        />
        <LinkCard icon="devices" title="Devices" description="Connected and trusted devices" onClick={() => onNavigate("devices")} />
        <LinkCard icon="settings" title="Settings" description="Startup, network and pointer options" onClick={() => onNavigate("settings")} />
      </CardGroup>
    </>
  );
}

function ClientHome({ client, snapshotAt, onNavigate }: { client: ClientSnapshot; snapshotAt: number; onNavigate: Navigate }) {
  const { settings } = useApp();
  const { busy, run } = useAction();
  const now = useNow(500, client.state === "reconnecting" && client.retryInMs !== null);
  const retryLeft = client.retryInMs !== null ? Math.max(0, snapshotAt + client.retryInMs - now) : null;
  const status = clientStatus(client, retryLeft);
  const lastHost = settings.lastHostAddress;
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
          <Button
            variant="accent"
            icon="link"
            busy={busy === "conn"}
            onClick={() => void run("conn", () => backend.api.clientConnect(lastHost, null))}
          >
            {client.state === "error" ? "Try again" : `Connect to ${lastHost}`}
          </Button>
        )}
        <Button variant={lastHost ? "standard" : "accent"} icon="devices" onClick={() => onNavigate("devices")}>
          Choose a host
        </Button>
      </>
    );

  return (
    <>
      <StatusHero tone={status.tone} title={status.title} detail={status.detail} busy={inProgress} actions={actions}>
        {client.state === "connected" && (
          <ul className="status-facts">
            <li>
              <Icon name="cursor" />
              <span>{client.active ? "You're using this computer now" : `The cursor is on ${client.hostName ?? "the host"}`}</span>
            </li>
          </ul>
        )}
      </StatusHero>

      {client.state === "error" && client.lastError && (
        <InfoBar severity="error" title="Connection problem">
          {client.lastError}
        </InfoBar>
      )}

      <CardGroup title="Host">
        <ListCard>
          {client.state === "connected" ? (
            <ListRow
              icon="desktop"
              title={
                <span className="title-with-badge">
                  {client.hostName ?? client.target}
                  {client.active && <Badge tone="accent">Cursor here</Badge>}
                </span>
              }
              description={client.hostAddress ?? undefined}
            >
              <span className="metric" title="Round-trip time">
                {formatMs(client.rttMs)}
              </span>
            </ListRow>
          ) : (
            <ListRow
              icon="desktop"
              title="Not connected"
              description="Connect to the computer that has the mouse and keyboard you want to use."
            />
          )}
        </ListCard>
      </CardGroup>

      <CardGroup title="Quick links">
        <LinkCard icon="devices" title="Connect to a host" description="Find hosts on your network or enter an address" onClick={() => onNavigate("devices")} />
        <LinkCard icon="layout" title="Displays" description="This computer's screens" onClick={() => onNavigate("layout")} />
        <LinkCard icon="settings" title="Settings" description="Startup and pointer options" onClick={() => onNavigate("settings")} />
      </CardGroup>
    </>
  );
}

export function HomePage({ onNavigate }: { onNavigate: Navigate }) {
  const { snapshot, snapshotAt, settings } = useApp();
  return (
    <Page title="Home">
      <HomeBody snapshot={snapshot} snapshotAt={snapshotAt} onNavigate={onNavigate} />
      <HotkeyHints scrollLock={settings.scrollLockToggle} />
    </Page>
  );
}

function HomeBody({ snapshot, snapshotAt, onNavigate }: { snapshot: AppSnapshot; snapshotAt: number; onNavigate: Navigate }) {
  if (snapshot.role === "host" && snapshot.host) return <HostHome host={snapshot.host} onNavigate={onNavigate} />;
  if (snapshot.role === "client" && snapshot.client) {
    return <ClientHome client={snapshot.client} snapshotAt={snapshotAt} onNavigate={onNavigate} />;
  }
  return <InfoBar severity="informational">Waiting for the background service to report its status…</InfoBar>;
}
