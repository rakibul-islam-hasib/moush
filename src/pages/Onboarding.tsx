import { useState } from "react";
import { backend, type Role } from "../api";
import { Button } from "../components/Button";
import { Icon, type IconName } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { useApp } from "../hooks/useApp";
import { errorMessage } from "../lib/errors";

const ROLES: { role: Role; icon: IconName; title: string; summary: string; detail: string }[] = [
  {
    role: "host",
    icon: "desktop",
    title: "Host",
    summary: "This computer has the mouse and keyboard.",
    detail: "Other computers are controlled from here. Move the cursor off the edge of a screen to reach them.",
  },
  {
    role: "client",
    icon: "laptop",
    title: "Client",
    summary: "This computer will be controlled.",
    detail: "It uses the mouse and keyboard of the host computer. You'll connect to the host in the next step.",
  },
];

const NOTES: { icon: IconName; title: string; text: string }[] = [
  {
    icon: "shield",
    title: "Windows Firewall",
    text: "Windows may ask whether Moush can use the network. Select Allow on Private networks so your computers can find each other.",
  },
  {
    icon: "lock",
    title: "Apps running as administrator",
    text: "To control windows that run as administrator on a client, run Moush as administrator on that client too.",
  },
  {
    icon: "info",
    title: "Lock screen and Ctrl+Alt+Del",
    text: "Windows keeps the lock screen and the Ctrl+Alt+Del screen private for security, so they can't be controlled from another computer.",
  },
];

export function Onboarding({ onComplete }: { onComplete: (role: Role) => void }) {
  const { setSnapshot } = useApp();
  const [choice, setChoice] = useState<Role | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!choice) return;
    setBusy(true);
    setError(null);
    try {
      const snap = await backend.api.setRole(choice);
      onComplete(choice);
      setSnapshot(snap);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <main className="onboarding">
      <div className="onboarding-inner">
        <header className="onboarding-header">
          <span className="onboarding-logo">
            <Icon name="mouse" size={32} />
          </span>
          <h1 className="page-title">Welcome to Moush</h1>
          <p className="text-secondary">Use one mouse and keyboard with all the computers on your desk.</p>
        </header>

        <fieldset className="role-picker">
          <legend className="card-group-title">How will this computer be used?</legend>
          <div className="role-options">
            {ROLES.map((r) => (
              <label key={r.role} className={choice === r.role ? "role-card is-selected" : "role-card"}>
                <input
                  type="radio"
                  name="role"
                  value={r.role}
                  checked={choice === r.role}
                  onChange={() => setChoice(r.role)}
                  className="role-radio"
                />
                <span className="role-card-icon">
                  <Icon name={r.icon} size={32} />
                </span>
                <span className="role-card-title">{r.title}</span>
                <span className="role-card-summary">{r.summary}</span>
                <span className="role-card-detail">{r.detail}</span>
              </label>
            ))}
          </div>
        </fieldset>

        <section className="card-group" aria-labelledby="onboarding-notes">
          <h2 className="card-group-title" id="onboarding-notes">
            Good to know
          </h2>
          <ul className="list-card">
            {NOTES.map((n) => (
              <li key={n.title} className="list-row">
                <Icon name={n.icon} size={20} className="settings-card-icon" />
                <div className="settings-card-text">
                  <span className="settings-card-title">{n.title}</span>
                  <span className="settings-card-desc">{n.text}</span>
                </div>
              </li>
            ))}
          </ul>
        </section>

        {error && (
          <InfoBar severity="error" title="Couldn't set up this computer" live>
            {error}
          </InfoBar>
        )}

        <div className="onboarding-actions">
          <Button variant="accent" busy={busy} disabled={!choice} onClick={() => void submit()}>
            {choice === "client" ? "Continue to connect" : "Continue"}
          </Button>
        </div>
      </div>
    </main>
  );
}
