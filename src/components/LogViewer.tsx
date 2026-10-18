import { useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { backend, type LogLevel } from "../api";
import { useAction } from "../hooks/useAction";
import { useLogs } from "../hooks/useLogs";
import { useToast } from "../hooks/useToast";
import { formatClock } from "../lib/format";
import { Button } from "./Button";
import { Select, type SelectOption } from "./Select";
import { ToggleSwitch } from "./ToggleSwitch";

const RANK: Record<LogLevel, number> = { error: 0, warn: 1, info: 2, debug: 3 };

const LEVELS: readonly SelectOption<LogLevel>[] = [
  { value: "debug", label: "All messages" },
  { value: "info", label: "Info and above" },
  { value: "warn", label: "Warnings and errors" },
  { value: "error", label: "Errors only" },
];

const LEVEL_TEXT: Record<LogLevel, string> = { error: "ERROR", warn: "WARN", info: "INFO", debug: "DEBUG" };

export function LogViewer({ logPath }: { logPath: string | null }) {
  const entries = useLogs();
  const toast = useToast();
  const { busy, run } = useAction();
  const [minLevel, setMinLevel] = useState<LogLevel>("debug");
  const [autoScroll, setAutoScroll] = useState(true);
  const listRef = useRef<HTMLDivElement>(null);
  const autoScrollId = useId();

  const visible = useMemo(() => entries.filter((e) => RANK[e.level] <= RANK[minLevel]), [entries, minLevel]);

  useLayoutEffect(() => {
    const el = listRef.current;
    if (el && autoScroll) el.scrollTop = el.scrollHeight;
  }, [visible, autoScroll]);

  const copy = async () => {
    const text = visible.map((e) => `${formatClock(e.ts)} ${LEVEL_TEXT[e.level]} ${e.target}: ${e.message}`).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      toast.notify("success", `Copied ${visible.length} log lines to the clipboard.`);
    } catch (e) {
      toast.error(e, "Couldn't copy the log");
    }
  };

  return (
    <div className="log">
      <div className="log-toolbar">
        <Select value={minLevel} options={LEVELS} onChange={setMinLevel} label="Show" />
        <span className="log-autoscroll">
          <span id={autoScrollId}>Auto-scroll</span>
          <ToggleSwitch checked={autoScroll} onChange={setAutoScroll} labelledBy={autoScrollId} showState={false} />
        </span>
        <span className="log-toolbar-spacer" />
        <Button icon="copy" onClick={() => void copy()} disabled={visible.length === 0}>
          Copy
        </Button>
        <Button icon="folder" busy={busy === "folder"} onClick={() => void run("folder", () => backend.api.openLogFolder())}>
          Open log folder
        </Button>
      </div>
      <div className="log-view" ref={listRef} role="log" aria-label="Log messages" aria-live="off" tabIndex={0}>
        {visible.length === 0 ? (
          <div className="log-empty">No log messages to show.</div>
        ) : (
          visible.map((e, i) => (
            <div key={`${e.ts}-${i}`} className={`log-line log-${e.level}`}>
              <span className="log-time">{formatClock(e.ts)}</span>
              <span className="log-level">{LEVEL_TEXT[e.level]}</span>
              <span className="log-target">{e.target}</span>
              <span className="log-message">{e.message}</span>
            </div>
          ))
        )}
      </div>
      {logPath && <p className="caption text-secondary">Log files are saved in {logPath}</p>}
    </div>
  );
}
