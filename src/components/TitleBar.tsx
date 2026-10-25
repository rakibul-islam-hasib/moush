import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { inTauri } from "../api/backend";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { Logo } from "./Logo";

/** Navigation toggle shown at the left of the title bar (absent during onboarding). */
export interface TitleBarNav {
  expanded: boolean;
  controls: string;
  onToggle: () => void;
}

// Caption glyphs drawn on a 10x10 grid with hairline strokes, like the Windows 11 caption buttons.
const GLYPHS = {
  minimize: "M0 5.5h10",
  maximize: "M1.5.5h7a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1z",
  restore: "M2.5 2.5h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1zM3.5.5h4.5a2 2 0 0 1 2 2V7",
  close: "M.5.5l9 9M9.5.5l-9 9",
} as const;

function Glyph({ name }: { name: keyof typeof GLYPHS }) {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true" focusable="false">
      <path d={GLYPHS[name]} fill="none" stroke="currentColor" strokeWidth="1" />
    </svg>
  );
}

/** Tracks the window's maximized and focused state (static outside Tauri). */
function useWindowState() {
  const [maximized, setMaximized] = useState(false);
  const [focused, setFocused] = useState(true);

  useEffect(() => {
    if (!inTauri) return;
    const win = getCurrentWindow();
    let alive = true;
    const unlisten: (() => void)[] = [];
    const sync = () => {
      win.isMaximized().then((m) => alive && setMaximized(m)).catch(() => undefined);
    };
    sync();
    win.isFocused().then((f) => alive && setFocused(f)).catch(() => undefined);
    win.onResized(sync).then((u) => (alive ? unlisten.push(u) : u()));
    win.onFocusChanged(({ payload }) => setFocused(payload)).then((u) => (alive ? unlisten.push(u) : u()));
    return () => {
      alive = false;
      unlisten.forEach((u) => u());
    };
  }, []);

  return { maximized, focused };
}

const run = (action: (w: ReturnType<typeof getCurrentWindow>) => Promise<void>) => () => {
  if (inTauri) action(getCurrentWindow()).catch(() => undefined);
};

/**
 * Custom window title bar (the native frame is turned off in tauri.conf.json).
 * The empty area drags the window and double-click maximizes; caption buttons follow
 * Windows 11 sizing and colours. Closing goes through CloseRequested, so close-to-tray
 * still applies.
 */
export function TitleBar({ nav }: { nav?: TitleBarNav }) {
  const { maximized, focused } = useWindowState();

  return (
    <header className={cx("titlebar", !focused && "is-inactive")} data-tauri-drag-region>
      {nav && (
        <button
          type="button"
          className="btn btn-subtle btn-icon titlebar-nav"
          aria-label={nav.expanded ? "Collapse navigation" : "Expand navigation"}
          aria-expanded={nav.expanded}
          aria-controls={nav.controls}
          title={nav.expanded ? "Collapse navigation" : "Expand navigation"}
          onClick={nav.onToggle}
        >
          <Icon name="menu" size={16} />
        </button>
      )}
      <span className={cx("titlebar-brand", !nav && "is-solo")} data-tauri-drag-region>
        <Logo size={16} />
        <span className="titlebar-title" data-tauri-drag-region>
          Moush
        </span>
      </span>

      <div className="titlebar-caption" role="group" aria-label="Window">
        <button type="button" className="caption-btn" tabIndex={-1} aria-label="Minimize" title="Minimize" onClick={run((w) => w.minimize())}>
          <Glyph name="minimize" />
        </button>
        <button
          type="button"
          className="caption-btn"
          tabIndex={-1}
          aria-label={maximized ? "Restore" : "Maximize"}
          title={maximized ? "Restore Down" : "Maximize"}
          onClick={run((w) => w.toggleMaximize())}
        >
          <Glyph name={maximized ? "restore" : "maximize"} />
        </button>
        <button type="button" className="caption-btn caption-close" tabIndex={-1} aria-label="Close" title="Close" onClick={run((w) => w.close())}>
          <Glyph name="close" />
        </button>
      </div>
    </header>
  );
}
