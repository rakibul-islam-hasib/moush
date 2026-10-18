# Troubleshooting

Logs: `%LOCALAPPDATA%\Moush\logs\moush.log` (Diagnostics → *Open log folder*).
The Diagnostics page also shows the firewall state, latency, and the most recent log lines.

## The client cannot find the host

- Both computers must be on the same network (same router / subnet). Guest Wi-Fi networks
  usually isolate devices from each other — use the main network.
- Discovery uses UDP port 24801 broadcasts, which some routers and VPN clients block.
  Connect by address instead: the host's IP addresses are listed on its Diagnostics page.
- On the host, check Settings → *Let other computers find this one*.

## "refused the connection" / "no answer"

- Is the other computer set to **Host** and sharing (Home shows *Sharing is on*)?
- **Windows Firewall** on the host must allow Moush. If you dismissed the prompt or
  your network is set to *Public*, open Diagnostics on the host and click
  *Add firewall rule* (asks for administrator approval; allows private and domain
  networks). Alternatively set the network to *Private* in Windows Settings → Network.
- Third-party firewalls/antivirus: allow `Moush.exe`, TCP 24800 and UDP 24801.
- Changed the port? Use `address:port` on the client.

## The cursor does not cross to the laptop

- Host → Layout: the laptop rectangle must **touch** a monitor edge (blue highlight). A
  warning appears when it does not.
- Scroll Lock may have locked the cursor (Home shows *Cursor locked to …*); press Scroll Lock again.
- Corner dead zone or switch delay (Settings) may be set high.
- No switching happens while a mouse button is held.

## Keys or clicks do not work on the laptop

- The window in front on the laptop is running as administrator → run Moush as
  administrator on the laptop (Windows blocks injected input into elevated windows).
- Lock screen, UAC and Ctrl+Alt+Del screens cannot be controlled remotely (Windows
  security). Use the laptop's own keyboard for those.
- If an elevated window has focus *on the host*, the host's hooks do not see input
  either; switch focus or run Moush elevated on the host.

## Stuck key or button

Should not happen: everything pressed is released when the cursor leaves a computer or a
connection drops. If it does (e.g. the client process was killed by Task Manager while a
key was down), press and release that key on the affected computer, or move the cursor
there and back. Please send the log.

## The cursor is "lost" on the client

Press **Ctrl + Alt + Esc** on the host keyboard, or use the tray menu *Bring cursor back
to this computer*. If the client vanished from the network, the cursor returns to the host
automatically within 3.5 s.

## "a different computer answered at …"

The client reached a device with another identity at the remembered address (the IP was
reassigned). Select the host again from Devices; Moush re-discovers paired hosts by
identity automatically.

## "cannot decrypt identity.key"

The identity file belongs to another Windows user or machine (e.g. copied settings).
Delete `%APPDATA%\Moush\identity.key`; a new identity is created and devices need to
be paired again.

## Reset everything

Quit Moush (tray → Quit) and delete `%APPDATA%\Moush`.
