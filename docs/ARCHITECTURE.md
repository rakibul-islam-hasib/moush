# Architecture

## Components

```
                 host                                             client
┌───────────────────────────────────────────┐      ┌───────────────────────────────────┐
│ input-capture thread (HIGHEST priority)   │      │ client thread                      │
│  WH_MOUSE_LL / WH_KEYBOARD_LL ─► Router   │      │  socket read ─► decrypt ─► decode  │
│  (pure state machine) ─► per-client queue │      │  ─► InjectState ─► SendInput       │
│  hidden-cursor window, watchdog timer     │      │                                    │
├───────────────────────────────────────────┤      ├───────────────────────────────────┤
│ per-client writer: coalesce moves,        │ TCP  │ writer: pongs, screen updates,     │
│ encrypt, write; keepalive + dead-peer     │◄────►│ keepalive + dead-peer detection    │
│ per-client reader: pongs, screen info     │Noise │ display-change watcher             │
├───────────────────────────────────────────┤      ├───────────────────────────────────┤
│ control thread: clients, layout, displays │      │ reconnect loop with backoff,       │
│ accept thread, discovery responder (UDP)  │      │ re-discovery by device id          │
└───────────────────────────────────────────┘      └───────────────────────────────────┘
           ▲ snapshots / commands ▼                         ▲ snapshots / commands ▼
     Tauri app (src-tauri) + React UI (src)            Tauri app + React UI
```

The input path never touches the UI: the hook callback, the router and the queue push run
in microseconds on a dedicated thread; encryption and the socket write run on the
connection's writer thread. The UI receives coalesced state snapshots (at most one per
25 ms) through a Tauri event and calls typed commands; every command runs on a worker
thread so the window never blocks.

`moush-core` contains everything that does not need Windows (protocol, crypto, geometry,
router, networking, discovery, trust) and is tested in isolation, including real TCP
sessions on loopback. `moush-engine` adds the Windows pieces.

## Input capture (host)

Low-level hooks (`WH_MOUSE_LL`, `WH_KEYBOARD_LL`) see physical input before any window
and can drop it. They need no special privileges.

- **While the cursor is on the host** every event passes through untouched (the router
  only watches for edge crossings and hotkeys).
- **While a client is active** the host cursor is parked at the centre of the primary
  monitor and every event is swallowed. The movement delta is the difference between the
  position Windows *would* move to and the parked position, so the host's pointer
  acceleration still applies and the cursor feels the same on both machines. A tiny
  invisible topmost window under the parked cursor hides it.
- Events that arrive after the park warp but were generated before it would look like a
  huge jump; deltas larger than a third of the primary monitor are discarded.
- No switching while a mouse button is held (drag stays on its computer).
- Keys held while crossing: their release still goes to the computer that saw the press;
  held modifiers (Shift/Ctrl/Alt/Win) are mirrored to the client so Shift+click across
  the edge works.
- The display is kept awake (`SetThreadExecutionState`) while a client is active, because
  swallowed input does not reset the host's idle timer.
- When the host session locks (`WTS_SESSION_LOCK`) the cursor comes home first.

**Hook robustness.** Windows silently removes a low-level hook whose callback is ever too
slow, and a hook installed later by another program can hide events from ours. A
once-per-second watchdog compares `GetLastInputInfo` with the time of the last event our
hooks saw and re-installs the hooks (new pair first, so there is no gap) when Windows
registered input we never saw. Hooks are also re-installed whenever a client becomes
active, so they are first in the chain exactly when input must be swallowed.

While a WebView2 window of the *same process* has keyboard focus, Windows does not call
that process's keyboard hook (found during testing; a plain Win32 window does not cause
it). When a client becomes active and Moush's own window has focus, focus is moved
to the invisible helper window and restored when the cursor returns.

## Injection (client)

`SendInput` with absolute virtual-desktop coordinates for moves (bypassing the client's
own acceleration — the host already applied it), scan codes for keys (the client's
keyboard layout interprets them, as with a locally attached keyboard; Pause, Print Screen
and media keys use virtual-key codes). Coordinates map exactly to pixels: Windows converts
back with `n × size >> 16`, so the encoder rounds up (`normalize()`, tested for every pixel
of common widths and verified on hardware by `moush selftest`).

`InjectState` remembers every key and button it pressed and releases them on `Leave`,
`ReleaseAll`, disconnect and drop, releasing non-modifiers before modifiers.

## Layout geometry

Two coordinate systems: each computer's own virtual desktop in physical pixels, and a
shared *layout plane* whose units are host physical pixels. Host monitors sit at their real
Windows positions; each client is a rectangle the user places, sized
`client pixels × host DPI / client DPI`.

An edge transfers the cursor only if it is not shared with another monitor of the same
computer and it touches (within 2 units) another computer's rectangle. Entry points are
mapped proportionally along the shared edge; returning lands one pixel inside the host
monitor so the cursor does not bounce. Optional corner dead zones and a switch delay
guard against accidental crossings. Client-to-client crossings work the same way.

The host is authoritative for where the shared cursor is on the client and sends absolute
positions, so a client just injects; a burst of moves queued behind a slow socket can be
coalesced to the latest position.

## Protocol and transport

One TCP connection per client, `TCP_NODELAY`, carrying Noise transport frames
(`[u16 length][ciphertext]`). A frame's plaintext is a sequence of compact binary messages
(`[type u8][fixed fields]`, little-endian; a mouse move is 9 bytes). The writer drains its
queue, merges consecutive moves, and sends everything in one frame and one `write` call.

*Why TCP and not UDP:* key and button transitions must arrive exactly once and in order
relative to moves (a click must land where the cursor is). TCP provides that; on a LAN a
lost segment is repaired by fast retransmit within milliseconds while input flows, and
move coalescing means a stall never replays stale positions. UDP would require
re-implementing ordering and retransmission for exactly the events that matter, while
mouse moves alone gain little because they are already tiny and absolute.

Messages: `Hello`, `Authorize`, `Bye`, `Screens` (client → host, on connect and on
display change), `Enter`, `Leave`, `MouseMove`, `MouseButton`, `Wheel`, `Key`,
`ReleaseAll`, `Ping`/`Pong`. Every field is range-checked; a malformed message drops the
connection. A client never sends input; the host disconnects one that tries.

**Keepalive.** Each side pings once per second; a peer silent for 3.5 s is disconnected.
Write timeouts and bounded queues (a full queue disconnects the client) keep a stalled
peer from blocking the host.

**Reconnection.** The client retries with backoff (0.25 → 5 s). After a successful
connection it pins the host's device id: a different device at that address is refused,
and after repeated failures it re-discovers the host by id in case its IP changed.

## Optional features (clipboard, files, shared text)

Feature traffic shares the input connection but can never delay input:

- **Envelope.** Protocol version 2 adds `Capabilities { features }` and
  `Feature { channel, body }`. The core only validates the envelope (channel ≠ 0, body ≤
  32 KiB, so one message always fits one encrypted frame); each feature module encodes and
  validates its own body with the bounds-checked `protocol::Reader`. Frames are split at
  message boundaries.
- **Input first.** At most 16 bulk messages may wait in a connection's queue (`BulkGate`),
  so bulk traffic can never fill the queue input depends on. The writer sends all pending
  input and control messages first and interleaves bulk messages one at a time.
- **Flow control.** Senders keep at most a window of unacknowledged bytes in flight
  (`flow::Window`; 128 KiB "balanced", 1 MiB "fast"); receivers acknowledge after
  consuming data (e.g. after writing to disk), which gives end-to-end backpressure and
  bounds how much bulk data can sit in socket buffers ahead of a key press. Measured on
  loopback (release): input p50 50 µs while an 8 MB stream runs at 40 MB/s.
- **Feature hub.** `moush_engine::features::FeatureHub` lives for the whole app; services
  (`FeatureService`) register once and see peers come and go regardless of role (host:
  every connected client; client: the host). `on_message` runs on the connection's reader
  thread — on a client that thread also injects input — so services only decode, validate
  and hand work to their own threads through bounded queues.
- **Capabilities.** Each side announces which features it allows with a peer (global
  settings combined with the paired device's policy, stored in the trust store) and
  re-announces after every settings or policy change. A feature is used only when both
  sides allow it, and every receiver re-checks its own settings before acting.

## Security

- Each installation has a long-term X25519 key pair, stored encrypted with DPAPI (only the
  same Windows user on the same machine can use it).
- Connections run `Noise_XX_25519_ChaChaPoly_BLAKE2s`: encrypted, mutually authenticated,
  replay- and tamper-proof frames.
- A device becomes trusted only after its user approves it **on both computers**, after
  comparing a 6-digit code derived from the handshake hash. A man in the middle runs two
  handshakes and produces different codes on the two screens. (Limitation: a 6-digit
  comparison without a commitment step gives an attacker who can actively intercept LAN
  traffic a small chance per attempt of matching codes; every attempt is a visible prompt.)
- Until both sides authorize, no input is routed or injected. The client also authorizes
  the host — a rogue host cannot drive a client that never approved it.
- Discovery packets are unauthenticated hints only; identity is always proven by the
  handshake.

## Threads and resource use

Host: capture, control, accept, discovery, and a reader/writer pair per client. Client:
session thread, writer, display watcher. All block on events (sockets, channels,
`GetMessage`); the only timers are the 1 s keepalive and the 1 s hook watchdog. The window
and its WebView2 processes exist only while the window is open; closing to the tray
destroys them.
