import type { KeyboardEvent } from "react";
import { backend } from "../api";
import { useApp } from "../hooks/useApp";
import { cx } from "../lib/cx";
import type { Navigate, PageId } from "../lib/navigation";
import { shortStatus } from "../lib/status";
import { StatusDot } from "./Badge";
import { Icon, type IconName } from "./Icon";

const ITEMS: { id: PageId; label: string; icon: IconName }[] = [
  { id: "home", label: "Home", icon: "home" },
  { id: "devices", label: "Devices", icon: "devices" },
  { id: "layout", label: "Layout", icon: "layout" },
  { id: "settings", label: "Settings", icon: "settings" },
  { id: "diagnostics", label: "Diagnostics", icon: "pulse" },
];

/** Moves focus between nav items with the arrow keys, Home and End. */
function onListKeyDown(e: KeyboardEvent<HTMLUListElement>) {
  const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>("button"));
  const index = buttons.findIndex((b) => b === document.activeElement);
  if (index < 0) return;
  const target =
    e.key === "ArrowDown"
      ? buttons[(index + 1) % buttons.length]
      : e.key === "ArrowUp"
        ? buttons[(index - 1 + buttons.length) % buttons.length]
        : e.key === "Home"
          ? buttons[0]
          : e.key === "End"
            ? buttons[buttons.length - 1]
            : null;
  if (target) {
    e.preventDefault();
    target.focus();
  }
}

export function NavPane({ page, onNavigate }: { page: PageId; onNavigate: Navigate }) {
  const { snapshot } = useApp();
  const status = shortStatus(snapshot);
  const mock = backend.mock;

  return (
    <nav className="nav" aria-label="Main">
      <div className="nav-header">
        <Icon name="mouse" size={20} className="nav-logo" />
        <span className="nav-title">Moush</span>
      </div>
      <ul className="nav-list" onKeyDown={onListKeyDown}>
        {ITEMS.map((item) => {
          const selected = item.id === page;
          return (
            <li key={item.id}>
              <button
                type="button"
                className={cx("nav-item", selected && "is-selected")}
                aria-current={selected ? "page" : undefined}
                title={item.label}
                onClick={() => onNavigate(item.id)}
              >
                <Icon name={item.icon} size={16} />
                <span className="nav-label">{item.label}</span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className="nav-footer">
        {mock && (
          <div className="nav-mock" aria-label="Preview controls" role="group">
            <span className="nav-mock-title">Preview data</span>
            <button type="button" className="nav-mock-btn" onClick={mock.togglePairing}>
              Toggle pairing request
            </button>
            <button type="button" className="nav-mock-btn" onClick={mock.simulateConnectionDrop}>
              Simulate connection drop
            </button>
          </div>
        )}
        <div className="nav-status" title={`${snapshot.deviceName} — ${status.label}`}>
          <StatusDot tone={status.tone} />
          <span className="nav-status-text">
            <span className="nav-status-name">{snapshot.deviceName}</span>
            <span className="nav-status-label">{status.label}</span>
          </span>
        </div>
      </div>
    </nav>
  );
}
