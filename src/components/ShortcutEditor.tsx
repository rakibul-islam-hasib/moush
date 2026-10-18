import { useId, useState, type FormEvent } from "react";
import { cx } from "../lib/cx";
import { MAX_LOCAL_SHORTCUTS, parseShortcut, SHORTCUT_SUGGESTIONS, shortcutKeys, validateNewShortcut } from "../lib/shortcuts";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { TextBox } from "./TextBox";

export function KeyCombo({ shortcut }: { shortcut: string }) {
  const keys = shortcutKeys(shortcut);
  return (
    <span className="kbd-group" aria-label={keys.join(" plus ")}>
      {keys.map((k, i) => (
        <span key={i} aria-hidden="true">
          {i > 0 && <span className="kbd-plus">+</span>}
          <kbd>{k}</kbd>
        </span>
      ))}
    </span>
  );
}

interface ShortcutEditorProps {
  value: string[];
  onChange: (next: string[]) => void;
  disabled?: boolean;
  labelledBy?: string;
}

/** List editor for shortcuts that always stay on this computer, with inline validation. */
export function ShortcutEditor({ value, onChange, disabled = false, labelledBy }: ShortcutEditorProps) {
  const [text, setText] = useState("");
  const [attempted, setAttempted] = useState(false);
  const inputId = useId();
  const messageId = useId();
  const full = value.length >= MAX_LOCAL_SHORTCUTS;
  const check = text.trim() ? validateNewShortcut(text, value) : null;
  const normalisedOthers = value.map((s) => {
    const r = parseShortcut(s);
    return r.ok ? r.value : s;
  });
  const suggestions = SHORTCUT_SUGGESTIONS.filter((s) => !normalisedOthers.includes(s));

  const add = (e: FormEvent) => {
    e.preventDefault();
    setAttempted(true);
    const r = validateNewShortcut(text, value);
    if (!r.ok) return;
    onChange([...value, r.value]);
    setText("");
    setAttempted(false);
  };

  return (
    <div className={cx("shortcut-editor", disabled && "is-disabled")}>
      {value.length === 0 ? (
        <p className="shortcut-empty">No shortcuts yet. Every shortcut goes to the computer that has the cursor.</p>
      ) : (
        <ul className="shortcut-list" aria-labelledby={labelledBy}>
          {value.map((s, i) => (
            <li key={`${s}-${i}`} className="shortcut-chip">
              <KeyCombo shortcut={s} />
              <button
                type="button"
                className="btn btn-subtle btn-icon btn-small"
                aria-label={`Remove ${s}`}
                title={`Remove ${s}`}
                disabled={disabled}
                onClick={() => onChange(value.filter((_, j) => j !== i))}
              >
                <Icon name="dismiss" />
              </button>
            </li>
          ))}
        </ul>
      )}

      <form className="shortcut-add" onSubmit={add} noValidate>
        <label htmlFor={inputId} className="sr-only">
          Add a shortcut
        </label>
        <TextBox
          id={inputId}
          value={text}
          placeholder={full ? "The list is full" : "Type a shortcut, e.g. Ctrl+Shift+Esc"}
          disabled={disabled || full}
          invalid={attempted && check !== null && !check.ok}
          aria-describedby={messageId}
          onChange={(e) => {
            setText(e.target.value);
            setAttempted(false);
          }}
        />
        <Button type="submit" icon="add" disabled={disabled || full || !check?.ok}>
          Add
        </Button>
      </form>
      <p id={messageId} className={cx("shortcut-message", check && (check.ok ? "is-ok" : attempted ? "is-error" : "is-hint"))} aria-live="polite">
        {check === null ? (
          `${value.length} of ${MAX_LOCAL_SHORTCUTS} · Modifiers: Ctrl, Alt, Shift, Win. Keys: A–Z, 0–9, F1–F24, Esc, Tab, Delete, PrintScreen…`
        ) : check.ok ? (
          <>
            <Icon name="checkmark" /> Will be added as {check.value}
          </>
        ) : (
          <>
            <Icon name={attempted ? "errorCircle" : "info"} /> {check.error}
          </>
        )}
      </p>
      {suggestions.length > 0 && !full && (
        <div className="shortcut-suggestions">
          <span className="caption text-secondary">Suggestions</span>
          {suggestions.slice(0, 4).map((s) => (
            <button key={s} type="button" className="suggestion-chip" disabled={disabled} onClick={() => onChange([...value, s])} aria-label={`Add ${s}`}>
              <Icon name="add" />
              {s}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
