import { useEffect, useRef, useState } from "react";
import { backend, type AppearanceSettings, type Role, type Settings, type Theme } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Icon } from "../components/Icon";
import { Logo } from "../components/Logo";
import { Page } from "../components/Page";
import { Select, type SelectOption } from "../components/Select";
import { ApplyHint, CardGroup, LinkCard, SettingsCard } from "../components/SettingsCard";
import { TextBox } from "../components/TextBox";
import { ToggleSwitch } from "../components/ToggleSwitch";
import { useAction } from "../hooks/useAction";
import { useApp } from "../hooks/useApp";
import { useToast } from "../hooks/useToast";
import { cx } from "../lib/cx";
import type { Navigate } from "../lib/navigation";
import { SPEEDS } from "./Features";

const MOTION: readonly SelectOption<AppearanceSettings["motion"]>[] = [
  { value: "system", label: "Follow Windows" },
  { value: "reduce", label: "Reduce" },
  { value: "full", label: "Full" },
];

const THEMES: { value: Theme; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "Windows default" },
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
 * Text bound to a setting: saves after a pause while valid, shows the error inline
 * while invalid, and follows the saved value (e.g. after a reset) when not focused.
 */
function useSettingText(value: string, validate: (t: string) => string | null, save: (t: string) => void) {
  const [text, setText] = useState(value);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (document.activeElement !== ref.current) setText(value);
  }, [value]);
  const error = validate(text);
  const onChange = (next: string) => {
    setText(next);
    if (!validate(next)) save(next);
  };
  const onBlur = () => {
    if (!validate(text)) setText(value);
  };
  return { text, error, onChange, onBlur, ref };
}

/** Theme choice as preview cards (radio group). */
function ThemePicker({ value, onChange, labelledBy }: { value: Theme; onChange: (t: Theme) => void; labelledBy: string }) {
  return (
    <div className="theme-picker" role="radiogroup" aria-labelledby={labelledBy}>
      {THEMES.map((t) => (
        <label key={t.value} className={cx("theme-option", value === t.value && "is-selected")}>
          <input type="radio" name="theme" value={t.value} checked={value === t.value} onChange={() => onChange(t.value)} className="sr-only" />
          <span className={`theme-preview theme-preview-${t.value}`} aria-hidden="true">
            <span className="tp-nav" />
            <span className="tp-card tp-card-1" />
            <span className="tp-card tp-card-2" />
            <span className="tp-accent" />
          </span>
          <span className="theme-option-label">
            <span className="theme-radio" aria-hidden="true" />
            {t.label}
          </span>
        </label>
      ))}
    </div>
  );
}

export function SettingsPage({ onNavigate }: { onNavigate: Navigate }) {
  const { snapshot, setSnapshot, settings, update, reset } = useApp();
  const toast = useToast();
  const { busy, run } = useAction();
  const [confirmRole, setConfirmRole] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);
  const role = snapshot.role;
  const otherRole: Role = role === "client" ? "host" : "client";
  const isHost = role !== "client";
  const sharing = isHost && !!snapshot.host?.running;
  const set = (patch: Partial<Settings>, debounce = false) => update((s) => ({ ...s, ...patch }), { debounce });
  const setAppearance = (patch: Partial<AppearanceSettings>) => update((s) => ({ ...s, appearance: { ...s.appearance, ...patch } }));
  const restartHint = sharing ? <ApplyHint>Restarts sharing briefly — connected computers reconnect</ApplyHint> : null;

  const name = useSettingText(settings.deviceName, validateName, (t) => set({ deviceName: t.trim() }, true));
  const port = useSettingText(String(settings.port), validatePort, (t) => set({ port: Number(t) }, true));
  const toggle = (key: "launchAtStartup" | "startMinimized" | "closeToTray" | "autoStart" | "discoveryEnabled") => (ids: { label: string; desc: string }) => (
    <ToggleSwitch checked={settings[key]} onChange={(v) => set({ [key]: v })} labelledBy={ids.label} describedBy={ids.desc} />
  );

  return (
    <Page title="Settings" subtitle="Personalise Moush and how it starts">
      <CardGroup title="This computer">
        <SettingsCard
          icon="edit"
          title="Device name"
          description="How this computer appears on your other computers"
          hint={restartHint}
          footer={name.error && <p className="field-error" id="name-error">{name.error}</p>}
        >
          {(ids) => (
            <TextBox
              ref={name.ref}
              className="textbox-medium"
              value={name.text}
              maxLength={63}
              invalid={name.error !== null}
              aria-labelledby={ids.label}
              aria-describedby={name.error ? "name-error" : ids.desc}
              onChange={(e) => name.onChange(e.target.value)}
              onBlur={name.onBlur}
            />
          )}
        </SettingsCard>
        <SettingsCard
          icon={isHost ? "desktop" : "laptop"}
          title="Role"
          description={isHost ? "Host — this computer shares its mouse and keyboard" : "Client — this computer is controlled by a host"}
        >
          <Badge tone="accent">{isHost ? "Host" : "Client"}</Badge>
          <Button onClick={() => setConfirmRole(true)}>Change role</Button>
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Appearance">
        <SettingsCard icon="theme" title="Theme" description="Light, dark, or follow Windows" className="settings-card-stacked">
          {(ids) => <ThemePicker value={settings.theme} onChange={(theme) => set({ theme })} labelledBy={ids.label} />}
        </SettingsCard>
        <SettingsCard
          icon="layers"
          title="Transparency effects"
          description={
            snapshot.platform.windows11
              ? "Let the desktop colour show softly through the window (Mica)"
              : "Needs Windows 11. On this computer the window is always opaque."
          }
          disabled={!snapshot.platform.windows11}
        >
          {(ids) => (
            <ToggleSwitch
              checked={settings.appearance.transparency}
              onChange={(v) => setAppearance({ transparency: v })}
              labelledBy={ids.label}
              describedBy={ids.desc}
              disabled={!snapshot.platform.windows11}
            />
          )}
        </SettingsCard>
        <SettingsCard icon="motion" title="Animations" description="Reduce removes movement such as sliding pages and pulsing connection lines">
          {(ids) => <Select value={settings.appearance.motion} options={MOTION} onChange={(motion) => setAppearance({ motion })} labelledBy={ids.label} describedBy={ids.desc} />}
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Startup">
        <SettingsCard icon="power" title="Launch at startup" description="Start Moush when you sign in to Windows">
          {toggle("launchAtStartup")}
        </SettingsCard>
        <SettingsCard icon="window" title="Start minimised" description="Open in the notification area instead of showing this window">
          {toggle("startMinimized")}
        </SettingsCard>
        <SettingsCard icon="window" title="Close to the notification area" description="Closing the window keeps Moush running in the background">
          {toggle("closeToTray")}
        </SettingsCard>
        <SettingsCard
          icon="play"
          title={isHost ? "Start sharing automatically" : "Reconnect automatically"}
          description={isHost ? "Start sharing as soon as Moush opens" : "Connect to the last host as soon as Moush opens"}
        >
          {toggle("autoStart")}
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Network">
        <SettingsCard
          icon="plug"
          title="Port"
          description={isHost ? "The port this computer listens on (default 24800)" : "Used when a host address has no port (default 24800)"}
          hint={restartHint}
          footer={port.error && <p className="field-error" id="port-error">{port.error}</p>}
        >
          {(ids) => (
            <TextBox
              ref={port.ref}
              className="textbox-small"
              inputMode="numeric"
              value={port.text}
              invalid={port.error !== null}
              aria-labelledby={ids.label}
              aria-describedby={port.error ? "port-error" : ids.desc}
              onChange={(e) => port.onChange(e.target.value)}
              onBlur={port.onBlur}
            />
          )}
        </SettingsCard>
        <SettingsCard
          icon="wifi"
          title={isHost ? "Let other computers find this one" : "Find hosts automatically"}
          description={isHost ? "Answer searches from Moush clients on this network" : "Search this network for Moush hosts"}
          hint={restartHint}
        >
          {toggle("discoveryEnabled")}
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Performance">
        <SettingsCard
          icon="speed"
          title="File transfer speed"
          description={
            settings.transfer.speed === "fast"
              ? "Fast: big files finish sooner; the mouse may feel slightly less smooth meanwhile"
              : "Balanced: the mouse and keyboard stay smoothest during transfers"
          }
        >
          {(ids) => (
            <Select
              value={settings.transfer.speed}
              options={SPEEDS}
              onChange={(speed) => update((s) => ({ ...s, transfer: { ...s.transfer, speed } }))}
              labelledBy={ids.label}
              describedBy={ids.desc}
            />
          )}
        </SettingsCard>
        {isHost && (
          <LinkCard icon="cursor" title="Pointer and switching" description="Pointer speed, switch delay and edge behaviour" onClick={() => onNavigate("features", "keyboard")} />
        )}
      </CardGroup>

      <CardGroup title="Troubleshooting">
        <LinkCard icon="pulse" title="Diagnostics" description="Latency, firewall, input statistics and the log" onClick={() => onNavigate("diagnostics")} />
        <SettingsCard icon="reset" title="Reset settings" description="Restore every option to its default. Your device name, paired devices and role are kept.">
          <Button onClick={() => setConfirmReset(true)}>Reset…</Button>
        </SettingsCard>
      </CardGroup>

      <CardGroup title="About">
        <div className="settings-card about-card">
          <Logo size={40} />
          <div className="about-text">
            <span className="about-name">Moush {snapshot.version}</span>
            <span className="settings-card-desc">
              Windows build {snapshot.platform.osBuild}
              {snapshot.platform.elevated ? " · running as administrator" : ""} · Device ID <span className="mono">{snapshot.deviceId}</span>
            </span>
            <span className="settings-card-desc">
              Fingerprint <span className="mono">{snapshot.fingerprint}</span>
            </span>
          </div>
        </div>
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
      {confirmReset && (
        <ConfirmDialog
          title="Reset all settings?"
          confirmLabel="Reset"
          destructive
          onConfirm={async () => {
            await reset();
            toast.notify("success", "Settings restored to their defaults.");
          }}
          onClose={() => setConfirmReset(false)}
        >
          <p>Every option in Settings and Features goes back to its default, including clipboard, keyboard, file transfer and notification options.</p>
          <p className="text-secondary reset-keep">
            <Icon name="shield" /> Kept: device name, role, paired devices and their permissions.
          </p>
        </ConfirmDialog>
      )}
    </Page>
  );
}
