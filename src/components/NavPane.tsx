import { memo, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { backend, type AppSnapshot } from "../api";
import { readFlag, useMediaQuery, writeFlag } from "../hooks/useMediaQuery";
import { cx } from "../lib/cx";
import type { NavSection, PageId } from "../lib/navigation";
import { shortStatus } from "../lib/status";
import { StatusDot } from "./Badge";
import { Icon, type IconName } from "./Icon";
import { Logo } from "./Logo";

const ITEMS: { id: NavSection; label: string; icon: IconName }[] = [
  { id: "home", label: "Home", icon: "home" },
  { id: "devices", label: "Devices", icon: "devices" },
  { id: "features", label: "Features", icon: "sparkle" },
  { id: "transfers", label: "Transfers", icon: "transfer" },
  { id: "settings", label: "Settings", icon: "settings" },
];

const WIDE_QUERY = "(min-width: 1008px)";
const COLLAPSED_KEY = "moush.nav.collapsed";

/** Moves focus between nav items with the arrow keys, Home and End. */
function onListKeyDown(e: KeyboardEvent<HTMLUListElement>) {
  const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>("button.nav-item"));
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

interface NavPaneProps {
  section: NavSection;
  snapshot: AppSnapshot;
  onNavigate: (page: PageId) => void;
  /** Dev preview: start collapsed. */
  forceCollapsed?: boolean;
}

/**
 * Collapsible navigation pane. Wide windows show labels (collapsible to an icon rail);
 * narrow windows show the rail and open the full pane as an overlay. A single selection
 * indicator slides between items.
 */
export const NavPane = memo(function NavPane({ section, snapshot, onNavigate, forceCollapsed = false }: NavPaneProps) {
  const wide = useMediaQuery(WIDE_QUERY);
  const [collapsed, setCollapsed] = useState(() => forceCollapsed || readFlag(COLLAPSED_KEY, false));
  const [overlay, setOverlay] = useState(false);
  const listRef = useRef<HTMLUListElement>(null);
  const navRef = useRef<HTMLElement>(null);
  const [indicatorTop, setIndicatorTop] = useState<number | null>(null);
  const [animate, setAnimate] = useState(false);
  const paneId = useId();

  const expanded = wide ? !collapsed : overlay;
  const status = shortStatus(snapshot);
  const mock = backend.mock;
  const live = snapshot.features.transfers.filter((t) => t.state === "active" || t.state === "pending");
  const waiting = live.filter((t) => t.state === "pending" && t.direction === "receive").length;
  const badges: Partial<Record<NavSection, { count: number; label: string; urgent: boolean }>> = {
    transfers:
      live.length > 0
        ? {
            count: live.length,
            urgent: waiting > 0,
            label: waiting > 0 ? `${waiting} waiting for your answer` : `${live.length} in progress`,
          }
        : undefined,
  };

  useEffect(() => {
    if (wide) setOverlay(false);
  }, [wide]);

  // Close the overlay on Escape or a click outside the pane.
  useEffect(() => {
    if (!overlay) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") setOverlay(false);
    };
    const onPointer = (e: PointerEvent) => {
      if (navRef.current && !navRef.current.contains(e.target as Node)) setOverlay(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  }, [overlay]);

  useLayoutEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-nav="${section}"]`);
    if (!el) return;
    setIndicatorTop(el.offsetTop + el.offsetHeight / 2 - 8);
  }, [section]);

  // Slide only after the first placement (no animation from the top on load).
  useEffect(() => {
    if (indicatorTop === null || animate) return;
    const id = requestAnimationFrame(() => setAnimate(true));
    return () => cancelAnimationFrame(id);
  }, [indicatorTop, animate]);

  const toggle = () => {
    if (wide) {
      setCollapsed((c) => {
        writeFlag(COLLAPSED_KEY, !c);
        return !c;
      });
    } else {
      setOverlay((o) => !o);
    }
  };

  return (
    <nav ref={navRef} className={cx("nav", expanded ? "is-expanded" : "is-compact", overlay && "is-overlay")} aria-label="Main">
      <div className="nav-panel" id={paneId}>
        <div className="nav-header">
          <button
            type="button"
            className="btn btn-subtle btn-icon nav-toggle"
            aria-label={expanded ? "Collapse navigation" : "Expand navigation"}
            aria-expanded={expanded}
            aria-controls={paneId}
            title={expanded ? "Collapse navigation" : "Expand navigation"}
            onClick={toggle}
          >
            <Icon name="menu" />
          </button>
          <span className="nav-brand">
            <Logo size={20} />
            <span className="nav-title">Moush</span>
          </span>
        </div>

        <div className="nav-list-wrap">
        {indicatorTop !== null && (
          <span className={cx("nav-indicator", animate && "is-animated")} style={{ transform: `translateY(${indicatorTop}px)` }} aria-hidden="true" />
        )}
        <ul className="nav-list" ref={listRef} onKeyDown={onListKeyDown}>
          {ITEMS.map((item) => {
            const selected = item.id === section;
            const badge = badges[item.id];
            return (
              <li key={item.id}>
                <button
                  type="button"
                  data-nav={item.id}
                  className={cx("nav-item", selected && "is-selected")}
                  aria-current={selected ? "page" : undefined}
                  aria-label={badge ? `${item.label}, ${badge.label}` : undefined}
                  title={expanded ? undefined : badge ? `${item.label} — ${badge.label}` : item.label}
                  onClick={() => {
                    onNavigate(item.id);
                    setOverlay(false);
                  }}
                >
                  <span className="nav-item-icon">
                    <Icon name={item.icon} size={16} />
                    {badge && <span className={cx("nav-dot", badge.urgent && "is-urgent")} aria-hidden="true" />}
                  </span>
                  <span className="nav-label">{item.label}</span>
                  {badge && (
                    <span className={cx("nav-count", badge.urgent && "is-urgent")} aria-hidden="true">
                      {badge.count}
                    </span>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
        </div>

        <div className="nav-footer">
          {mock && expanded && (
            <details className="nav-mock">
              <summary>Preview data</summary>
              <div className="nav-mock-actions" role="group" aria-label="Preview controls">
                <button type="button" className="nav-mock-btn" onClick={mock.togglePairing}>
                  Toggle pairing request
                </button>
                <button type="button" className="nav-mock-btn" onClick={mock.simulateConnectionDrop}>
                  Simulate connection drop
                </button>
                <button type="button" className="nav-mock-btn" onClick={mock.simulateIncomingTransfer}>
                  Incoming file
                </button>
                <button type="button" className="nav-mock-btn" onClick={mock.simulateSharedText}>
                  Incoming shared text
                </button>
                <button type="button" className="nav-mock-btn" onClick={mock.simulateClipboardSync}>
                  Clipboard sync
                </button>
              </div>
            </details>
          )}
          <div className="nav-status" title={`${snapshot.deviceName} — ${status.label}`}>
            <StatusDot tone={status.tone} />
            <span className="nav-status-text">
              <span className="nav-status-name">{snapshot.deviceName}</span>
              <span className="nav-status-label">{status.label}</span>
            </span>
          </div>
        </div>
      </div>
    </nav>
  );
});
