import { useCallback, useEffect, useRef, useState } from "react";
import { backend, type Role, type Settings, type Theme } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Page } from "../components/Page";
import { Select, type SelectOption } from "../components/Select";
import { CardGroup, SettingsCard } from "../components/SettingsCard";
import { Slider } from "../components/Slider";
import { TextBox } from "../components/TextBox";
import { ToggleSwitch } from "../components/ToggleSwitch";
import { useAction } from "../hooks/useAction";
import { useApp } from "../hooks/useApp";
import type { Navigate } from "../lib/navigation";

const DEBOUNCE_MS = 400;

const THEMES: readonly SelectOption<Theme>[] = [
  { value: "system", label: "Use system setting" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

function validatePort(text: string): string | null {
  if (!/^\d+$/.test(text.trim())) return "Enter a whole number between 1024 and 65535.";
  const n = Number(text);
  return n >= 1024 && n <= 65535 ? null : "Enter a whole number between 1024 and 65535.";
}

function validateName(text: string): string | null {
  const v = text.trim();
  if (!v) return "Enter a name for this computer.";
  if (v.length > 63) return "Use 63 characters or fewer.";
  return null;
}

/**
 * Local draft of the settings. Toggles and selects save immediately; text and
 * sliders save after a short pause. Invalid text never reaches the draft.
 */
function useSettingsDraft() {
  const { settings, saveSettings } = useApp();
  const [draft, setDraft] = useState<Settings>(settings);
  const draftRef = useRef(draft);
  const timer = useRef<number | undefined>(undefined);
  const pending = useRef<Settings | null>(null);

  const commit = useCallback(
    async (next: Settings) => {
      pending.current = null;
      const result = await saveSettings(next);
      // On failure, roll back to what the backend has.
      if (!result.ok && draftRef.current === next) {
        draftRef.current = result.settings;
        setDraft(result.settings);
      }
    },
    [saveSettings],
  );

  const update = useCallback(
    (patch: Partial<Settings>, debounce = false) => {
      const next = { ...draftRef.current, ...patch };
      draftRef.current = next;
      setDraft(next);
      window.clearTimeout(timer.current);
      if (debounce) {
        pending.current = next;
        timer.current = window.setTimeout(() => void commit(next), DEBOUNCE_MS);
      } else {
        void commit(next);
      }
    },
    [commit],
  );

  // Flush a pending save when leaving the page.
  useEffect(
    () => () => {
      window.clearTimeout(timer.current);
      if (pending.current) void commit(pending.current);
    },
    [commit],
  );

  return { draft, update };
}

export function SettingsPage({ onNavigate }: { onNavigate: Navigate }) {
  const { snapshot, setSnapshot } = useApp();
  const { draft, update } = useSettingsDraft();
  const { busy, run } = useAction();
  const [nameText, setNameText] = useState(draft.deviceName);
  const [portText, setPortText] = useState(String(draft.port));
  const [confirmRole, setConfirmRole] = useState(false);
  const nameError = validateName(nameText);
  const portError = validatePort(portText);
  const role = snapshot.role;
  const otherRole: Role = role === "client" ? "host" : "client";
  const isHost = role !== "client";

  return (
    <Page title="Settings">
      <CardGroup title="This computer">
        <SettingsCard
          icon="edit"
          title="Device name"
          description="How this computer appears to your other computers"
          footer={nameError && <p className="field-error" id="name-error">{nameError}</p>}
        >
          {(ids) => (
            <TextBox
              className="textbox-medium"
              value={nameText}
              maxLength={63}
              invalid={nameError !== null}
              aria-labelledby={ids.label}
              aria-describedby={nameError ? "name-error" : ids.desc}
              onChange={(e) => {
                setNameText(e.target.value);
                if (!validateName(e.target.value)) update({ deviceName: e.target.value.trim() }, true);
              }}
            />
          )}
        </SettingsCard>
        <SettingsCard
          icon={isHost ? "desktop" : "laptop"}
          title="Role"
          description={
            isHost
              ? "Host — this computer shares its mouse and keyboard"
              : "Client — this computer is controlled by a host"
          }
        >
          <Badge tone="accent">{isHost ? "Host" : "Client"}</Badge>
          <Button onClick={() => setConfirmRole(true)}>Change role</Button>
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Appearance">
        <SettingsCard icon="theme" title="Theme" description="Choose light, dark, or follow Windows">
          {(ids) => (
            <Select value={draft.theme} options={THEMES} onChange={(theme) => update({ theme })} labelledBy={ids.label} describedBy={ids.desc} />
          )}
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Startup">
        <SettingsCard icon="power" title="Launch at startup" description="Start Moush when you sign in to Windows">
          {(ids) => (
            <ToggleSwitch checked={draft.launchAtStartup} onChange={(v) => update({ launchAtStartup: v })} labelledBy={ids.label} describedBy={ids.desc} />
          )}
        </SettingsCard>
        <SettingsCard icon="window" title="Start minimized" description="Open in the notification area instead of showing this window">
          {(ids) => (
            <ToggleSwitch checked={draft.startMinimized} onChange={(v) => update({ startMinimized: v })} labelledBy={ids.label} describedBy={ids.desc} />
          )}
        </SettingsCard>
        <SettingsCard icon="window" title="Close to the notification area" description="Closing the window keeps Moush running in the background">
          {(ids) => (
            <ToggleSwitch checked={draft.closeToTray} onChange={(v) => update({ closeToTray: v })} labelledBy={ids.label} describedBy={ids.desc} />
          )}
        </SettingsCard>
        <SettingsCard
          icon="play"
          title={isHost ? "Start sharing automatically" : "Reconnect automatically"}
          description={isHost ? "Start sharing as soon as Moush opens" : "Connect to the last host as soon as Moush opens"}
        >
          {(ids) => (
            <ToggleSwitch checked={draft.autoStart} onChange={(v) => update({ autoStart: v })} labelledBy={ids.label} describedBy={ids.desc} />
          )}
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Network">
        <SettingsCard
          icon="plug"
          title="Port"
          description={isHost ? "The port this computer listens on (default 24800)" : "The port used when an address has no port (default 24800)"}
          footer={portError && <p className="field-error" id="port-error">{portError}</p>}
        >
          {(ids) => (
            <TextBox
              className="textbox-small"
              inputMode="numeric"
              value={portText}
              invalid={portError !== null}
              aria-labelledby={ids.label}
              aria-describedby={portError ? "port-error" : ids.desc}
              onChange={(e) => {
                setPortText(e.target.value);
                if (!validatePort(e.target.value)) update({ port: Number(e.target.value) }, true);
              }}
            />
          )}
        </SettingsCard>
        <SettingsCard
          icon="wifi"
          title={isHost ? "Let other computers find this one" : "Find hosts automatically"}
          description={isHost ? "Answer searches from Moush clients on this network" : "Search this network for Moush hosts"}
        >
          {(ids) => (
            <ToggleSwitch checked={draft.discoveryEnabled} onChange={(v) => update({ discoveryEnabled: v })} labelledBy={ids.label} describedBy={ids.desc} />
          )}
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Mouse and keyboard">
        <SettingsCard icon="timer" title="Switch delay" description="How long the cursor must rest at a screen edge before it moves to another computer">
          {(ids) => (
            <Slider
              value={draft.switchDelayMs}
              min={0}
              max={1000}
              step={10}
              format={(v) => `${v} ms`}
              onChange={(v) => update({ switchDelayMs: v }, true)}
              labelledBy={ids.label}
              describedBy={ids.desc}
            />
          )}
        </SettingsCard>
        <SettingsCard icon="corner" title="Corner dead zone" description="Don't switch computers this close to a screen corner">
          {(ids) => (
            <Slider
              value={draft.cornerDeadZonePx}
              min={0}
              max={200}
              step={5}
              format={(v) => `${v} px`}
              onChange={(v) => update({ cornerDeadZonePx: v }, true)}
              labelledBy={ids.label}
              describedBy={ids.desc}
            />
          )}
        </SettingsCard>
        <SettingsCard icon="speed" title="Pointer speed on other computers" description="Makes the cursor faster or slower while it's on a client">
          {(ids) => (
            <Slider
              value={draft.clientPointerSpeed}
              min={0.25}
              max={4}
              step={0.05}
              format={(v) => `${v.toFixed(2)}×`}
              onChange={(v) => update({ clientPointerSpeed: Math.round(v * 100) / 100 }, true)}
              labelledBy={ids.label}
              describedBy={ids.desc}
            />
          )}
        </SettingsCard>
        <SettingsCard icon="lock" title="Scroll Lock locks the cursor" description="Press Scroll Lock to keep the cursor on the current computer">
          {(ids) => (
            <ToggleSwitch checked={draft.scrollLockToggle} onChange={(v) => update({ scrollLockToggle: v })} labelledBy={ids.label} describedBy={ids.desc} />
          )}
        </SettingsCard>
      </CardGroup>

      <CardGroup title="About">
        <SettingsCard
          icon="info"
          title={`Moush ${snapshot.version}`}
          description={
            <>
              Fingerprint <span className="mono">{snapshot.fingerprint}</span> · Device ID <span className="mono">{snapshot.deviceId}</span>
            </>
          }
        />
        <SettingsCard icon="power" title="Quit Moush" description="Stops sharing and closes the app, including the notification area icon">
          <Button busy={busy === "quit"} onClick={() => void run("quit", () => backend.api.quitApp())}>
            Quit
          </Button>
        </SettingsCard>
      </CardGroup>

      {confirmRole && (
        <ConfirmDialog
          title={otherRole === "client" ? "Make this computer a client?" : "Make this computer the host?"}
          confirmLabel={otherRole === "client" ? "Switch to client" : "Switch to host"}
          onConfirm={async () => {
            const snap = await backend.api.setRole(otherRole);
            setSnapshot(snap);
            onNavigate(otherRole === "client" ? "devices" : "home");
          }}
          onClose={() => setConfirmRole(false)}
        >
          {otherRole === "client"
            ? "Sharing will stop and connected devices will be disconnected. This computer will then be controlled by a host."
            : "This computer will disconnect from its host and start sharing its own mouse and keyboard instead."}
        </ConfirmDialog>
      )}
    </Page>
  );
}
