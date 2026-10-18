# Moush — Install & Usage Guide

Moush lets you use one mouse and keyboard across several Windows computers on the
same network. Move the cursor off the edge of your desktop and it continues on your
laptop; the keyboard follows the cursor.

```
[ Laptop ] [ Monitor 1 ] [ Monitor 2 ]      ← arranged by you in the layout editor
   client        host           host
```

- The **host** is the computer the mouse and keyboard are plugged into.
- A **client** is controlled by the host.
- One app does both; you pick the role on first launch and can change it later in Settings.

---

## 1. Requirements

| | |
| --- | --- |
| OS | Windows 10 or 11 (x64) |
| Network | All computers on the same LAN/subnet (same router) |
| Installer | No admin rights needed — installs per-user |
| WebView2 | Preinstalled on Windows 11; on Windows 10 install the Evergreen runtime |

---

## 2. Install from the installer (recommended)

1. Download `Moush_x.y.z_x64-setup.exe`.
2. Run it on **every** computer you want to include. It installs for the current user
   (no administrator prompt).
3. Launch Moush from the Start menu. It lives in the notification area (tray) and
   uses ~16 MB of RAM when its window is closed.

---

## 3. First-time setup (two computers)

1. **Host** — on the computer with the mouse and keyboard, choose **Host**. When Windows
   asks whether Moush may communicate on networks, allow it on **private** networks
   (or use *Diagnostics → Add firewall rule*).
2. **Client** — on the other computer, choose **Client**. The host appears under *Devices*;
   click it. If it does not appear, type the host's IP address shown on the host's
   *Diagnostics* page.
3. **Pair** — a dialog appears on **both** computers. Check that the 6-digit codes match
   and click **Allow** on both. Devices stay paired and reconnect automatically.
4. **Arrange monitors (host)** — open **Layout**. Your monitors show up as Windows
   arranges them. Drag the laptop so one of its edges touches the side of the monitor it
   sits next to on your desk (the touching segment turns blue), then **Save**. **Test**
   jumps the cursor to the laptop.

Now move the cursor through the highlighted edge — it appears on the laptop and the
keyboard types there. Move back to return.

---

## 4. Daily use

### Switching between computers

- Move the cursor across a shared edge to hand over input; move back to return.
- **Ctrl + Alt + Esc** (on the host keyboard) — bring the cursor back to the host
  immediately.
- **Scroll Lock** — lock the cursor to the computer it is on (toggle; can be disabled in
  Settings).
- Tray icon menu → *Bring cursor back to this computer*.

### Settings and pages

- **Home** — current role and sharing state (*Sharing is on*).
- **Devices** — discover and connect to hosts; manage paired devices.
- **Layout** — place each computer's monitors on the shared plane.
- **Settings** — role, device name, discovery toggle, port, autostart, cursor options.
- **Diagnostics** — firewall state, latency, device id / public key, recent log lines,
  *Add firewall rule*, *Open log folder*.

---

## 5. What Windows does not allow

- **Ctrl+Alt+Del, the lock screen and UAC prompts** run on the secure desktop; no app can
  capture or inject input there. When the host locks, the cursor returns home automatically.
- **Win+L** always locks the host.
- **Elevated (administrator) windows on the client** reject injected input from a
  non-elevated app. Run Moush as administrator on the client to control such windows.
  The same applies on the host: while an elevated window has focus, its hooks see no input.

---

## 6. Build and run from source (developers)

Requirements: Windows 10/11, Rust 1.85+ (MSVC toolchain), Node.js 20+, WebView2 runtime.

```powershell
npm install
npx tauri build                     # release app + NSIS installer in target\release\bundle\nsis
npx tauri dev                       # development build with hot reload
cargo test --workspace              # unit + loopback integration tests
cargo build --release -p moush-cli # headless tool (see below)
```

The built installer lands in `target\release\bundle\nsis`.

### Headless tool (`moush.exe`)

Runs the same engine without the UI — handy for testing on two machines, benchmarking and
troubleshooting:

```
moush monitors                        list monitors as Moush sees them
moush discover                        find hosts on the LAN
moush host [--port N]                 run as host (pairing prompts in the console)
moush client <ip|host-id> [--dry-run] run as client; --dry-run logs instead of injecting
moush bench [--events N]              full network-path latency benchmark on loopback
moush selftest                        verify input capture/injection APIs on this machine
moush identity                        print this device's id and public key
```

`scripts\e2e-local.ps1` runs a live end-to-end test on one machine (real GUI host with real
hooks, dry-run client, synthetic input; see the script header).

---

## 7. Files, ports and reset

| What | Where |
| --- | --- |
| Settings, identity, paired devices, layout | `%APPDATA%\Moush` |
| Logs | `%LOCALAPPDATA%\Moush\logs\moush.log` |
| TCP | 24800 (input connection) |
| UDP | 24801 (discovery broadcasts) |

**Reset everything:** quit Moush (tray → Quit) and delete `%APPDATA%\Moush`.

If the client cannot find the host, or the cursor does not cross, see
[TROUBLESHOOTING.md](TROUBLESHOOTING.md).
