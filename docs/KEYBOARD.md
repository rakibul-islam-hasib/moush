# Keyboard, shortcuts and focus

## How typing works

Keyboard focus follows the shared cursor. Whatever computer shows the cursor gets your
keys; the others get nothing. Moush sends physical key positions (scan codes), so each
computer applies its own keyboard layout, as if the keyboard were plugged into it.

Each key goes where the cursor was when you **pressed** it, and its release always goes to
the same place. So a key can never get stuck down on one computer because you moved the
cursor while holding it. When the cursor leaves a client, everything still held there is
released.

- **Shift or Ctrl held while you move across an edge** is pressed on the new computer too,
  so Shift+click and Ctrl+click work across the edge. Coming back to the host, Moush presses
  the held Shift/Ctrl on the host as well.
- **Alt or Win held while crossing** is not carried over. If you then press a key on the
  new computer (Win+E, Alt+F4), the held modifier joins that shortcut there.
- **The cursor stays put while Alt+Tab or Win+Tab is open.** Once you press a key while
  holding Alt or Win, the cursor does not cross to another computer until you let go of
  Alt/Win. Those switchers are used with the mouse on the screen that shows them.

## Two kinds of switching

- **App switching** (Alt+Tab, Win+Tab, taskbar) happens on whichever computer has the
  cursor and only ever lists that computer's windows. Windows can't list windows of
  another PC, and Moush doesn't pretend it can.
- **Computer switching** moves the cursor (and keyboard focus) between computers. You can
  do it in four ways:
  - Move the mouse over a shared screen edge. With **Edge switching modifier** set to
    Ctrl, Shift or Alt, edges only switch while you hold that key.
  - **Ctrl+Alt+Shift+Arrow** moves to the computer next to the current one in that
    direction, following your layout. It also goes from one client to another and from a
    client back to the host. **Ctrl+Alt+Shift+Home** returns to the host. These keys
    never reach any computer. You can turn them off with **Switching hotkeys**.
  - **Ctrl+Alt+Esc** always brings the cursor home.
  - **Scroll Lock** locks the cursor to the current computer (edges stop switching).

## Settings

| Setting | Effect |
|---|---|
| Forward keyboard | Off = mouse-only mode. The cursor still moves to clients, but typing always goes to the host. Moush's hotkeys keep working. |
| Forward system shortcuts | On (default): Alt+Tab, Alt+Shift+Tab, Alt+Esc, Alt+F4, Ctrl+Esc, Ctrl+Shift+Esc, the Win key and Win+anything act on the computer with the cursor. Off: they act on the host even while you work on a client. Other shortcuts (Ctrl+C, Alt+F, ...) still go to the client. |
| Local shortcuts | Shortcuts that always act on the host, written like `Ctrl+Shift+Esc` or `Win+E`. They need at least one of Ctrl, Alt, Shift, Win. Keys you can use: letters, digits, F1-F24, arrows, Tab, Esc, Space, Enter, Home, End, PgUp, PgDn, Insert, Delete. Up to 16. Moush drops invalid entries and saves the rest in a standard form. |
| Switching hotkeys | Turns Ctrl+Alt+Shift+Arrow/Home on or off. |
| Edge switching modifier | None, Ctrl, Shift or Alt. |
| Focus on enter (client) | When the cursor arrives on this client, the window under it comes to the front, so you can type right away without clicking first. Moush won't take focus from a full-screen app or from the lock screen, and won't activate the desktop or the taskbar. Changes apply immediately. |

All of these apply right away, even with keys held down.

## What Moush cannot do

- **Ctrl+Alt+Del** is handled by Windows on the computer whose keyboard you press it on.
  No program can intercept or forward it. To reach a client's security screen, use that
  client's own keyboard or its remote-access tools.
- **Win+L** always locks the computer the keyboard is attached to (the host). Moush can't
  stop that. If the cursor is on a client, the client may get Win+L as well and lock too,
  depending on whether Windows lets Moush see the L first. To lock only the host, add
  `Win+L` to local shortcuts. Whenever the host locks, the cursor
  comes back to it first.
- Shortcuts that run on the host because of the settings above are typed into the host
  by Moush. Windows blocks that if the window in front on the host runs as administrator
  and Moush does not. In that case run Moush as administrator, or keep such shortcuts
  forwarded.
- Elevated windows on a client (Task Manager, installers) don't accept input from a
  non-elevated Moush there. This is the same Windows limitation as for any remote input.

## Technical notes

- The router (`moush-core/src/router.rs`) keeps a fixed 256-entry table indexed by
  virtual-key code. Each entry records where the key went: `Local`, `Remote` (the active
  client) or `Dropped` (a hotkey, or already released by a client that was left). The hook
  callback does a few array lookups per key and never allocates for bookkeeping.
- A shortcut is a non-modifier key pressed while modifiers are held. The first such key
  decides where the whole shortcut goes, until the last modifier is released. If it must
  run on the host while the cursor is on a client, Moush releases the modifiers on the
  client and presses them on the host (`SendInput`, tagged so its own hooks ignore it).
  Then it injects the key itself. Injected events queue behind the event being
  processed, so passing the original key through would put it ahead of its modifiers.
- A lone Win press and release opens the Start menu, and a lone Alt opens the menu bar.
  Wherever such a press saw its other keys swallowed, Moush taps virtual key 0xE8 before
  the release. 0xE8 is unassigned and is also AutoHotkey's default "menu mask key". A
  Ctrl tap, the classic trick, has side effects in some programs. This applies:
  - on the host, when input went to a client while Win/Alt was held on the host;
  - on the client, when it is told to release everything (`Leave`);
  - on the client, when a shortcut moves to the host.
- Missed key releases are handled in three ways:
  - **Session lock/unlock** (the secure desktop eats the releases): held keys are
    forgotten.
  - **Hook re-install after a detected bypass**: keys forwarded to the client are released
    there (`ReleaseAll`), and host keys are kept only if `GetAsyncKeyState` still reports
    them down.
  - **A "repeat" more than 5 s after the last press or repeat of that key**: treated as a
    new press. That is longer than any repeat delay Windows can be set to.
- Focus on enter (`moush-engine/src/win/focus.rs`) runs on its own thread. It finds the
  window with `WindowFromPoint` and `GetAncestor(GA_ROOT)`, then activates it with
  `SetForegroundWindow` while attached to the foreground thread's input state
  (`AttachThreadInput`). The documented foreground-lock rules don't guarantee that a
  background process may take the foreground just because it injected input, so Moush
  doesn't rely on that.
- Tests: `cargo test -p moush-core router` covers every scenario above. It includes a
  randomized check that a key not physically held is never down on any computer, through
  switches, hotkeys, settings changes, disconnects and session locks. The real hooks are
  exercised by hand with
  `cargo test -p moush-engine real_hooks -- --ignored --test-threads=1`. Focus activation
  is checked with `cargo test -p moush-engine focus -- --ignored`.
