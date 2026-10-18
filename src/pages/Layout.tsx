import { useEffect, useId, useMemo, useState } from "react";
import { backend, type AppSnapshot, type HostSnapshot } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { InfoBar } from "../components/InfoBar";
import { LayoutCanvas, type CanvasClient, type MoveRequest } from "../components/LayoutCanvas";
import { Page } from "../components/Page";
import { CardGroup, ListCard, ListRow, SettingsCard } from "../components/SettingsCard";
import { useAction } from "../hooks/useAction";
import { useApp } from "../hooks/useApp";
import { useToast } from "../hooks/useToast";
import { formatResolution, formatScale } from "../lib/format";
import { resolveCollisions, snapPosition, touchesAny, type Point, type Rect } from "../lib/layoutGeometry";

type Edits = Record<string, Point>;

function HostLayoutEditor({ host, deviceName }: { host: HostSnapshot; deviceName: string }) {
  const toast = useToast();
  const { busy, run } = useAction();
  const [edits, setEdits] = useState<Edits>({});
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const helpId = useId();

  // Drop edits that the backend now reports (after a save) or whose device vanished.
  useEffect(() => {
    setEdits((prev) => {
      let changed = false;
      const next: Edits = {};
      for (const [id, p] of Object.entries(prev)) {
        const saved = host.layout.find((c) => c.clientId === id);
        if (!saved || (saved.x === p.x && saved.y === p.y)) {
          changed = true;
          continue;
        }
        next[id] = p;
      }
      return changed ? next : prev;
    });
  }, [host.layout]);

  const clients: CanvasClient[] = useMemo(
    () =>
      host.layout.map((c) => {
        const e = edits[c.clientId];
        const rect: Rect = { x: e?.x ?? c.x, y: e?.y ?? c.y, width: c.width, height: c.height };
        return {
          id: c.clientId,
          name: c.name,
          online: c.online,
          active: host.activeTarget === c.clientId,
          rect,
          monitors: c.monitors,
          resolutionWidth: c.resolutionWidth,
          resolutionHeight: c.resolutionHeight,
          scale: c.scale,
          // Computed locally so it follows unsaved edits.
          adjacent: touchesAny(rect, host.monitors),
        };
      }),
    [host.layout, host.monitors, host.activeTarget, edits],
  );

  const dirty = host.layout.some((c) => {
    const e = edits[c.clientId];
    return e !== undefined && (e.x !== c.x || e.y !== c.y);
  });
  const selected = clients.find((c) => c.id === selectedId) ?? null;
  const unreachable = clients.filter((c) => !c.adjacent);

  const move = ({ id, position, snapThreshold }: MoveRequest) => {
    const me = clients.find((c) => c.id === id);
    if (!me) return;
    const others: Rect[] = [...host.monitors, ...clients.filter((c) => c.id !== id).map((c) => c.rect)];
    let p: Point = position;
    if (snapThreshold !== null) p = snapPosition({ ...me.rect, ...p }, others, snapThreshold);
    p = resolveCollisions({ ...me.rect, ...p }, others, { x: me.rect.x, y: me.rect.y });
    const rounded = { x: Math.round(p.x), y: Math.round(p.y) };
    if (rounded.x === me.rect.x && rounded.y === me.rect.y) return;
    setEdits((prev) => ({ ...prev, [id]: rounded }));
  };

  const save = async () => {
    const placements = clients.map((c) => ({ clientId: c.id, x: Math.round(c.rect.x), y: Math.round(c.rect.y) }));
    const ok = await run("save", () => backend.api.hostSetLayout(placements), "Couldn't save the layout");
    if (ok) toast.notify("success", "Layout saved.");
  };

  return (
    <Page
      title="Display layout"
      actions={
        <>
          {dirty && <Badge tone="caution">Unsaved changes</Badge>}
          <Button icon="undo" disabled={!dirty || busy === "save"} onClick={() => setEdits({})}>
            Revert
          </Button>
          <Button variant="accent" icon="save" disabled={!dirty} busy={busy === "save"} onClick={() => void save()}>
            Save
          </Button>
        </>
      }
    >
      {unreachable.map((c) => (
        <InfoBar key={c.id} severity="warning" title={`The cursor can't reach ${c.name}`}>
          Drag it so one of its edges touches a screen of this computer.
        </InfoBar>
      ))}

      <div className="layout-editor">
        <LayoutCanvas
          monitors={host.monitors}
          clients={clients}
          selectedId={selectedId}
          onSelect={setSelectedId}
          onMove={move}
          label={`Display layout of ${deviceName} and its devices`}
          describedBy={helpId}
        />
        <div className="layout-legend">
          <span id={helpId}>Drag a device, or select it and use the arrow keys (hold Shift for bigger steps).</span>
          <span className="legend-item">
            <span className="legend-swatch legend-edge" aria-hidden="true" />
            Cursor crosses here
          </span>
          <span className="legend-item">
            <span className="legend-swatch legend-offline" aria-hidden="true" />
            Offline
          </span>
        </div>
      </div>

      {clients.length === 0 ? (
        <InfoBar severity="informational" title="No devices yet">
          On another computer, install Moush, choose Client and connect to {deviceName}. It will appear here so you can
          place it next to your screens.
        </InfoBar>
      ) : selected ? (
        <SettingsCard
          icon="laptop"
          title={
            <span className="title-with-badge">
              {selected.name}
              <Badge tone={selected.online ? "success" : "neutral"}>{selected.online ? "Online" : "Offline"}</Badge>
              {selected.active && <Badge tone="accent">Has the cursor</Badge>}
            </span>
          }
          description={`${formatResolution(selected.resolutionWidth, selected.resolutionHeight)} at ${formatScale(selected.scale)} · Position ${Math.round(selected.rect.x)}, ${Math.round(selected.rect.y)}`}
        >
          <Button
            icon="target"
            disabled={!selected.online || !host.running}
            busy={busy === "test"}
            title={selected.online ? "Move the shared cursor to this device" : "The device must be online to test"}
            onClick={() => void run("test", () => backend.api.hostTestSwitch(selected.id))}
          >
            Test
          </Button>
        </SettingsCard>
      ) : (
        <SettingsCard icon="cursor" title="Select a device" description="Select a device in the layout to see its details or test the switch." />
      )}

      <CardGroup title="How the layout works">
        <ListCard>
          <ListRow
            icon="desktop"
            title="Your screens"
            description="The arrangement of this computer's screens comes from Windows Settings › System › Display. After changing it there, select Detect displays."
          >
            <Button icon="refresh" busy={busy === "monitors"} onClick={() => void run("monitors", () => backend.api.refreshMonitors())}>
              Detect displays
            </Button>
          </ListRow>
          <ListRow
            icon="mouse"
            title="Moving between your own screens"
            description="Crossing between this computer's screens works as usual — input stays on this computer."
          />
          <ListRow
            icon="link"
            title="Moving into another device"
            description="Crossing a highlighted edge into a device sends your mouse and keyboard to that computer. Press Ctrl+Alt+Esc to bring the cursor back."
          />
        </ListCard>
      </CardGroup>
    </Page>
  );
}

function ClientDisplays({ snapshot }: { snapshot: AppSnapshot }) {
  const monitors = snapshot.client?.monitors ?? [];
  return (
    <Page title="Displays">
      <InfoBar severity="informational" title="The layout is set up on the host">
        On the host computer, open Moush › Layout and drag {snapshot.deviceName} next to its screens.
      </InfoBar>
      <div className="layout-editor">
        <LayoutCanvas monitors={monitors} label={`Screens of ${snapshot.deviceName}`} />
      </div>
      <CardGroup title="Screens on this computer">
        <ListCard>
          {monitors.map((m) => (
            <ListRow
              key={m.id}
              icon="desktop"
              title={
                <span className="title-with-badge">
                  {m.name}
                  {m.primary && <Badge>Primary</Badge>}
                </span>
              }
              description={`${formatResolution(m.width, m.height)} at ${formatScale(m.scale)} · Position ${m.x}, ${m.y}`}
            />
          ))}
        </ListCard>
      </CardGroup>
    </Page>
  );
}

export function LayoutPage() {
  const { snapshot } = useApp();
  if (snapshot.role === "host" && snapshot.host) {
    return <HostLayoutEditor host={snapshot.host} deviceName={snapshot.deviceName} />;
  }
  return <ClientDisplays snapshot={snapshot} />;
}
