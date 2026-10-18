// Parsing and validation of shortcut text such as "Ctrl+Shift+Esc" for the
// "keep on this computer" list (settings.keyboard.localShortcuts).
//
// Grammar: zero or more modifiers (Ctrl, Alt, Shift, Win) followed by exactly one key,
// joined with "+". Names are case-insensitive and normalised to the canonical form
// below (modifiers in the order Ctrl, Alt, Shift, Win).

export const MAX_LOCAL_SHORTCUTS = 16;
const MAX_LEN = 64;

const MODIFIERS: Record<string, string> = {
  ctrl: "Ctrl",
  control: "Ctrl",
  alt: "Alt",
  shift: "Shift",
  win: "Win",
  windows: "Win",
  meta: "Win",
};
const MOD_ORDER = ["Ctrl", "Alt", "Shift", "Win"];

const NAMED_KEYS: Record<string, string> = {
  esc: "Esc",
  escape: "Esc",
  tab: "Tab",
  space: "Space",
  enter: "Enter",
  return: "Enter",
  backspace: "Backspace",
  delete: "Delete",
  del: "Delete",
  insert: "Insert",
  ins: "Insert",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pgup: "PageUp",
  pagedown: "PageDown",
  pgdn: "PageDown",
  up: "Up",
  down: "Down",
  left: "Left",
  right: "Right",
  printscreen: "PrintScreen",
  prtsc: "PrintScreen",
  print: "PrintScreen",
  pause: "Pause",
  break: "Pause",
  capslock: "CapsLock",
  numlock: "NumLock",
  apps: "Apps",
  menu: "Apps",
};

/** Keys that are reasonable on their own (without a modifier). */
const STANDALONE = new Set(["PrintScreen", "Pause"]);

function normaliseKey(raw: string): string | null {
  const t = raw.trim();
  const lower = t.toLowerCase();
  if (/^[a-z0-9]$/i.test(t)) return t.toUpperCase();
  const f = /^f([1-9]|1\d|2[0-4])$/i.exec(t);
  if (f) return `F${f[1]}`;
  return NAMED_KEYS[lower] ?? null;
}

export type ParseResult = { ok: true; value: string } | { ok: false; error: string };

/** Validates and normalises a shortcut. */
export function parseShortcut(text: string): ParseResult {
  const value = text.trim();
  if (!value) return { ok: false, error: "Type a shortcut, for example Ctrl+Shift+Esc." };
  if (value.length > MAX_LEN) return { ok: false, error: `Use ${MAX_LEN} characters or fewer.` };
  const parts = value.split("+").map((p) => p.trim());
  if (parts.some((p) => p === "")) return { ok: false, error: "Join keys with a single + and nothing in between, for example Ctrl+Alt+K." };

  const mods = new Set<string>();
  for (const p of parts.slice(0, -1)) {
    const m = MODIFIERS[p.toLowerCase()];
    if (!m) {
      return normaliseKey(p)
        ? { ok: false, error: `Only one key can follow the modifiers; "${p}" isn't Ctrl, Alt, Shift or Win.` }
        : { ok: false, error: `"${p}" isn't a modifier. Use Ctrl, Alt, Shift or Win.` };
    }
    if (mods.has(m)) return { ok: false, error: `${m} appears twice.` };
    mods.add(m);
  }
  const last = parts[parts.length - 1];
  if (MODIFIERS[last.toLowerCase()]) return { ok: false, error: "End the shortcut with a key, for example Ctrl+Shift+Esc." };
  const key = normaliseKey(last);
  if (!key) {
    return { ok: false, error: `"${last}" isn't a key Moush knows. Use a letter, a digit, F1–F24 or a name such as Esc, Tab, Delete or PrintScreen.` };
  }
  const isFunction = /^F\d+$/.test(key);
  if (mods.size === 0 && !isFunction && !STANDALONE.has(key)) {
    return { ok: false, error: "Add at least one modifier (Ctrl, Alt, Shift or Win), or use a function key." };
  }
  const ordered = MOD_ORDER.filter((m) => mods.has(m));
  return { ok: true, value: [...ordered, key].join("+") };
}

/** The keys of a shortcut, for rendering <kbd> chips. */
export function shortcutKeys(shortcut: string): string[] {
  return shortcut.split("+").map((p) => p.trim()).filter(Boolean);
}

/** Validation for adding to an existing list (duplicates, size limit). */
export function validateNewShortcut(text: string, existing: readonly string[]): ParseResult {
  if (existing.length >= MAX_LOCAL_SHORTCUTS) return { ok: false, error: `You can keep at most ${MAX_LOCAL_SHORTCUTS} shortcuts on this computer.` };
  const r = parseShortcut(text);
  if (!r.ok) return r;
  const lower = r.value.toLowerCase();
  if (existing.some((s) => (parseShortcut(s).ok ? (parseShortcut(s) as { value: string }).value.toLowerCase() : s.toLowerCase()) === lower)) {
    return { ok: false, error: `${r.value} is already in the list.` };
  }
  return r;
}

export const SHORTCUT_SUGGESTIONS = ["Ctrl+Shift+Esc", "Win+D", "Win+E", "Alt+PrintScreen", "Ctrl+Alt+T"];
