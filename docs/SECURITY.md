# Security

Moush lets one computer type and click on others, and (with the optional features) moves
clipboard contents, text and files between them. This document describes who we defend
against, what protects each asset, what is deliberately out of scope, and the rules every
feature module must follow. It complements the *Security* section of
[ARCHITECTURE.md](ARCHITECTURE.md).

To report a vulnerability, follow the repository's [security policy](../SECURITY.md) and use
GitHub's private reporting form. Do not open a public issue for a security problem.

## Assets

| Asset | Why it matters |
| --- | --- |
| Input injection on a client | Whoever controls it controls that computer as the logged-in user. |
| Keystrokes captured on the host | Passwords are typed through Moush while a client is active. |
| Clipboard contents, shared text | Often secrets (passwords, tokens, personal data). |
| Received files | Can be malicious; their *names* are attacker-chosen too. |
| The device identity key | Proves "I am this paired computer". DPAPI-encrypted at rest. |
| Availability of the host's input | A stuck hook or a blocked input path takes the user's own keyboard away. |

## Attackers

1. **Unpaired LAN attacker.** Can send anything to the TCP port (24800) and the discovery
   port (24801/UDP), spoof UDP source addresses, and watch or tamper with LAN traffic. Has
   no paired key.
2. **Malicious paired peer.** A device the user approved (or one that was later
   compromised). It holds a valid key and passes authorization, then sends hostile but
   well-encrypted traffic: malformed or huge feature messages, floods, out-of-order
   lifecycle messages, hostile file names and clipboard data.
3. **Malicious content.** Benign peers relaying hostile data: a file named
   `invoice‮fdp.exe`, a clipboard HTML fragment with scripts, a `desktop.ini`, a link to
   `search-ms:`.

Out of scope: malware running as the same Windows user (it can already read the clipboard,
log keys and edit Moush's files), a malicious *host* injecting input into a client that
paired with it (that is the product's purpose; pairing is the trust decision), and physical
access.

## What protects what

### Transport and identity
- `Noise_XX_25519_ChaChaPoly_BLAKE2s` on every connection: confidentiality, integrity,
  replay protection and mutual authentication. Any tampered, replayed or reordered frame
  fails decryption and ends the connection.
- Pairing requires approval **on both computers** after comparing a 6-digit code derived
  from the handshake hash. Until both sides authorize, the connection carries only
  `Hello`/`Authorize`/`Bye`/`Ping`/`Pong`; anything else ends it.
- The identity key is DPAPI-protected (same user, same machine).

### Before authentication (attacker 1)
- At most 4 connections may be in the handshake at once, and each must finish the Noise
  handshake **and** send its `Hello` within 10 s in total (`net::SETUP_DEADLINE`), however
  slowly it drips bytes.
- After `Hello`, devices that need a pairing prompt move to a separate pool of 2 *pairing
  slots*; a third simultaneous attempt is refused ("busy"). Trusted devices never wait
  behind pairing attempts, so a pairing flood cannot lock out paired computers.
- A pairing prompt is withdrawn as soon as the other side disconnects (the connection is
  watched while the local user decides), instead of lingering for the 90 s timeout.
- Discovery replies are hints only (identity is always proven by the handshake) and are
  rate-limited (burst 32, 16/s), so spoofed probes cannot turn the host into a UDP
  reflector.

### Decoding (attackers 1 and 2)
- Every field is range-checked (`protocol.rs`); strings are length-limited, valid UTF-8
  and free of control characters; a feature body is at most 32 KiB and must fit its frame.
  A malformed message ends the connection.
- The encoding is canonical and the decoder never panics: `tests/protocol_fuzz.rs` runs
  200 000 random buffers, 20 000 mutated valid frames (bit flips, truncation, insertion,
  extreme length bytes) and random `Reader` operation sequences, and checks that whatever
  decodes re-encodes to identical bytes.
- `encoded_len` is an upper bound for every variant (tested, including over-long strings and
  too many monitors), so the writer never splits a message across frames.
- Memory per connection is bounded: frames are at most 64 KiB, the reader releases the
  buffer of a frame packed with tens of thousands of tiny messages, setup-phase queues hold
  at most one frame.

### The input path (attacker 2, slow or stuck peers)
- The hook thread only ever calls `try_send` on a bounded queue (4096); it never waits.
- Bulk feature messages are gated per connection: at most 16 wait in the queue, so they
  can never fill the queue input depends on. The writer always sends pending input first
  and writes bulk messages one at a time in between.
- A peer that stops reading is disconnected by the 3 s write timeout or the 3.5 s
  dead-peer check; tests show the host's `try_send` stays under 0.1 ms throughout and no
  input is refused during a 1.2 s stall (`tests/hardening.rs`).
- A paired peer's floods are absorbed cheaply: identical capability announcements cause no
  callbacks, screen reports are coalesced to at most 10 per second, and the "queue full"
  warning is logged at most once per second.
- `panic = "unwind"` is kept on purpose: the hook callback wraps the router in
  `catch_unwind` (an internal error lets the event through instead of killing the
  process), and a panic on one connection's thread ends only that connection.

### Feature gating (attacker 2)
- Each side announces the features it allows with that peer (`Capabilities`); a feature is
  used only if both allow it. An unpaired device gets no capabilities at all (the policy
  returns 0 for unknown ids).
- The feature hub drops traffic from a connection that is not the peer's current one
  (stale `conn_no`), traffic before the peer is registered, traffic on unknown channels,
  and traffic on a channel whose capability this side does not currently allow with that
  peer (clipboard channel: `CLIPBOARD` or `TEXT_SHARE`; files channel: `FILES`).
  Services must still re-check their own settings: the hub is a second line of defence.
- Connection numbers are unique for the whole process, so a late disconnect from a stopped
  host server can never remove a newer connection's registration.

### Logs
- `Message`'s `Debug` output never contains feature bodies, only channel and length, so
  diagnostics cannot leak clipboard or file contents.
- Nothing logs clipboard contents, file contents or typed keys in the app. (The CLI's
  `--dry-run` test mode logs injected keys at info level; never use it with real typing.)

### The desktop UI
- Content Security Policy: scripts only from the app itself (no inline scripts, no `eval`),
  no plugins, no frames, no `<base>` and no form posts; network access only to Tauri IPC.
- `withGlobalTauri` is off; the window has only the `core:default` permission set, no shell
  or file-system plugins; every privileged action is a typed Rust command.

## Findings of the October 2026 review

| # | Sev. | Attacker | Finding | Status |
| --- | --- | --- | --- | --- |
| 1 | Medium | 1 | Handshake reads had only a per-read timeout: dripping one byte every few seconds held one of the 4 pending slots for minutes; four such connections locked out every client, trusted reconnects included. | Fixed: 10 s overall setup deadline; test `dripping_peer_is_cut_off_before_authentication`. |
| 2 | Medium | 1 | Pairing prompts kept their pending slot for up to 90 s: four unanswered pairing attempts locked out trusted clients for 90 s, repeatably. | Fixed: separate pairing slots (2); test `pairing_flood_cannot_lock_out_trusted_clients`. |
| 3 | Low | 1 | A device that left during pairing kept its prompt (and thread) until the 90 s timeout, because the host stopped reading while waiting for its user. On Windows a local `shutdown` does not wake a blocked read, so the client's "cancel" also waited. | Fixed: the socket is watched while the user decides. |
| 4 | Medium | 2, 3 | Protocol-violation log lines printed messages with `{:?}`, which included feature bodies (clipboard text, file data). | Fixed: redacting `Debug`; test `debug_output_never_contains_feature_bodies`. |
| 5 | Medium | 2 | The hub delivered traffic on channels the user had disabled for that device; privacy depended on every service re-checking. | Fixed: hub-level channel gating (defence in depth). |
| 6 | Low | 2 | `refresh_caps` evaluated the settings policy while holding the lock that reader threads (the client's injection thread) take for every feature message; concurrent refreshes could apply an older evaluation last. | Fixed: policy evaluated outside the lock, refreshes serialized, re-check after `peer_up`. |
| 7 | Low | 2 | Every `Capabilities` message called every service, even when nothing changed. | Fixed: callbacks only on change. |
| 8 | Low | 2 | Ping flood while not reading wrote one "queue full" warning per ping (log growth of hundreds of MB possible within the write timeout). | Fixed: rate-limited. |
| 9 | Low | 2 | Screen-report flood: one layout rebuild and one log line per message, through an unbounded control queue. | Fixed: coalesced, at most 10/s. |
| 10 | Low | 1 | Discovery answered every probe with a ~28x larger reply to a spoofable source. | Fixed: rate limit. |
| 11 | Low | — | Connection numbers restarted with every host server: a late disconnect from the previous server could unregister the new connection's features. A stale connection could also overwrite the new one's screen info. | Fixed: process-wide counter; `conn_no` checked. |
| 12 | Low | 2 | A reader kept ~2.6 MB of decoded-message capacity after one frame packed with tiny messages. | Fixed. |
| 13 | Low | — | `send_control` accepted bulk messages in release builds (bypassing the bulk gate and size check); `Window::acquire`/`BulkGate` panicked on `Duration::MAX` and could overflow on huge sizes. | Fixed; tests. |
| 14 | Low | 3 | CSP lacked `object-src`, `base-uri`, `form-action`, `frame-src`. | Fixed in `tauri.conf.json`. |

Not fixed (owner or rationale):

- **Corrupt or invalid settings reset to defaults, which enable clipboard sync and file
  transfer** (`src-tauri/src/settings.rs`: one invalid field such as a port below 1024
  replaces the whole file with defaults). Fail-open for privacy settings; recommended fix:
  validate field by field, and after a corrupt file keep clipboard/transfer off until the
  user reviews settings.
- `trusted.json`, `settings.json` and `layout.json` are not integrity-protected; same-user
  malware could add a trusted device. Out of scope (that attacker already controls the
  session); the identity key itself is DPAPI-protected.
- 6-digit pairing code without a commitment step (see ARCHITECTURE.md): an active MITM
  has a ~1e-6 chance per visible prompt.
- Discovery reveals the device name and id to the LAN; users can turn discovery off.
- `ClientHandle::outbox` is public: code that pushes `Message::Feature` straight into it
  bypasses the bulk gate. Only use `PeerSender`.
- A peer that stops reading delays *its own* input by up to the 3 s write timeout before it
  is disconnected; other clients have their own writers and are unaffected.
- `core:default` permissions were not narrowed further (they are read-only window/app/event
  permissions; narrowing needs UI testing).

## Rules for feature authors

### Threading and flow
1. `FeatureService::on_message` runs on the connection's reader thread (on a client, the
   thread that injects input). Decode, validate, `try_send` to your own worker over a
   **bounded** channel, return. If the channel is full, drop and end that operation. Never
   touch disk, the clipboard, the UI, or a lock that slow code holds.
2. Reply from reader threads only with `PeerSender::try_send_feature` (acks). Use
   `send_feature(.., timeout)` from worker threads, and handle `false` (timeout or closed).
3. Never put `Message::Feature` into an `Outbox` directly; never send bulk data through
   `send_control` (it is refused).
4. Bound bytes in flight with `flow::Window`; release only what you actually sent
   (`min(ack, in_flight_for_this_transfer)`), and `close()` the window in `peer_down`.
5. Do not cache capabilities from `caps_changed` as the source of truth (callbacks from
   different threads can arrive out of order); call `hub.effective_caps(peer)` before
   sending and re-check `hub.local_caps(peer)` plus your current settings on receive.
6. Abort everything for a peer in `peer_down`, and expect `peer_down` + `peer_up` for the
   same id when it reconnects.

### Validating peer data
1. Decode with `protocol::Reader` and finish with `finish()`; the first byte is your message
   kind; unknown kinds are ignored (or end that operation), never a panic.
2. Every number from a peer is hostile: check sizes **before** allocating, use
   `checked_*`/`saturating_*` arithmetic, bound counts (files per transfer, chunks, history
   entries, concurrent operations per peer) and reject anything above your limit.
3. Ids from a peer are lookup keys only: look them up in a bounded map scoped to that peer
   (a peer must never address another peer's transfer).
4. The receiver enforces its own settings (`max_size_kb`, enabled formats, transfer
   enabled, ask-before-receiving) regardless of what the sender announced.

### Files (received)
1. **Never execute or open received files.** "Open folder" opens Explorer on the folder.
2. Treat every name as one path component chosen by an attacker. Replace or reject
   `\ / : * ? " < > |`, control characters, leading/trailing spaces and trailing dots,
   `.` and `..`, reserved device names with or without extension (`CON`, `PRN`, `AUX`,
   `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, including the superscript-digit forms), and
   Unicode bidirectional overrides (U+202A-U+202E, U+2066-U+2069) that disguise
   extensions. Limit a component to 255 UTF-16 units.
3. For folders, split relative paths on `/`, validate each component, reject absolute
   paths, drive letters and UNC prefixes, and verify after joining that the result is
   still inside the destination. Do not follow existing junctions or symbolic links inside
   the destination (check `FILE_ATTRIBUTE_REPARSE_POINT`).
4. Write to a temporary name in the destination, create it with `create_new` (no races
   with an existing file), rename when complete, delete it on cancel or error. Check free
   space against the announced size and stop when more data arrives than announced.
5. Add the **Mark of the Web** to every received file: an alternate data stream
   `Zone.Identifier` containing `[ZoneTransfer]\r\nZoneId=3\r\n`, so SmartScreen,
   Office Protected View and script hosts treat it as coming from another computer.
6. Refuse or neutralise shell-active names that act without being opened:
   `desktop.ini`, `autorun.inf`, and `.lnk`, `.url`, `.scf`, `.library-ms`,
   `.searchConnector-ms` (Explorer resolves their icons, which can leak the user's NTLM
   hash to a remote server). Never apply attributes (hidden, system) sent by the peer.

### Clipboard and shared text
1. Never log clipboard contents or shared text (lengths and format names are fine).
2. Skip data the source marks as private when `skip_sensitive` is on: the formats
   `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0 and
   `CanUploadToCloudClipboard` = 0.
3. Accept only the formats you implement (text, HTML, RTF, bitmap). Never accept file
   lists (`CF_HDROP`, `FileGroupDescriptor`) or arbitrary registered formats from a peer.
4. Check image dimensions and byte counts against limits before allocating or decoding.
5. History stays in memory, bounded by `history_limit`; clear it when the feature is turned
   off.
6. Do not echo back what you just received (loops, and sending a peer's data on to third
   peers it never chose).
7. `open_shared_link`: parse the URL and allow only `http` and `https`. Never pass
   `file:`, UNC paths, `javascript:` or protocol handlers (`ms-msdt:`, `search-ms:`,
   `ms-officecmd:` ...) to the shell.

### UI and notifications
1. Render peer-provided strings (device names, file names, shared text, clipboard previews)
   as React text only; never `dangerouslySetInnerHTML`, never as `href`/`src` without the
   URL check above. HTML clipboard content is shown as plain text.
2. Keep clipboard and shared-text bodies out of Windows notifications (they appear on the
   lock screen and in the notification history); show the device and a short, sanitized
   description instead. Strip bidi overrides from file names shown in notifications.
3. If images from the clipboard must be shown, `data:` URLs are allowed by the CSP; do not
   widen the CSP to remote origins.
