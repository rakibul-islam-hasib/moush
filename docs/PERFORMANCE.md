# Performance and validation

All numbers below were measured on the development machine (16 logical cores,
Windows 11 build 26200, one 1920×1080 monitor at 125 %), release build. **Two-computer
tests over real Ethernet/Wi-Fi have not been run yet** — see the end of this document for
how to measure them; the in-app Diagnostics page reports the same statistics live.

## Targets

| Metric | Target | Measured |
| --- | --- | --- |
| Hook callback time (host) | < 50 µs typical, < 1 ms max | 4–21 µs for forwarded events; ~4 ms once per crossing (window z-order + cursor warp) |
| Host processing: hook → encrypted socket write | < 0.5 ms p99 | 49 µs p50, 136 µs p99 (loopback bench) |
| End-to-end processing overhead (enqueue → injected on client, excluding network) | < 1 ms p99 | 53 µs p50, 139 µs p99, 261 µs max (3000 events @ 1 kHz, loopback) |
| Network RTT on LAN | ≤ 2 ms wired, ≤ 10 ms Wi-Fi | 0.18 ms p50 loopback; LAN not yet measured |
| Reconnect to a paired host after a drop | < 1 s after the network is back | 28 ms handshake + authorization (loopback test); detection of a dead peer: ≤ 3.5 s |
| Handshake + authorization (trusted devices) | < 100 ms | 3.1 ms (loopback) |
| Idle CPU (host or client) | < 0.1 % | 0.01 % host with client connected; 0.00 % headless client |
| Idle memory, window closed to tray | < 50 MB | 16 MB working set, 3.4 MB private (host with a connected client) |
| Memory with window open | — | Moush.exe 32 MB; the WebView2 processes add ~216 MB private while the window is open |

Expected end-to-end latency on a real LAN = network one-way time (≈ RTT/2) + ~0.1 ms
processing, i.e. well under 1 ms wired and a few ms on typical Wi-Fi.

## How the numbers were obtained

- `moush bench --events 3000`: real host server and client session over loopback TCP,
  with encryption, framing and decoding; a mock injector timestamps every event. Measures
  everything except the OS injection call and the physical network.
- `moush selftest`: verifies on the actual hardware that `SendInput` lands on the exact
  target pixel (6/6 positions including both corners) and that the hooks install
  (≈ 6 ms).
- `scripts/e2e-local.ps1`: the real GUI app as host (real hooks) and a dry-run client on
  the same machine, driven by synthetic input. Checks crossing, cursor parking, relative
  motion, key down/up, buttons, wheel and return. Passed 8/8 and 3/3 consecutive runs after
  the hook-focus fix described in ARCHITECTURE.md (before the fix, keys were lost in about a
  third of runs while Moush's own window had focus).
- Idle resources: process CPU time and memory sampled over 20 s with Windows performance
  counters, host GUI + connected client.
- Unit and integration tests: `cargo test --workspace` (58 tests: protocol fuzz-style
  rejection, geometry for the reference desk layout, router state machine incl. stuck-key
  scenarios, Noise tampering, pairing, loopback sessions incl. rejection and reconnect).

## Reliability behaviours verified

| Scenario | Result |
| --- | --- |
| Client killed while connected | Host notices immediately (connection reset), cursor returns home |
| Client disconnected while a key is held | Client releases the key (integration test) |
| Host disconnects ("kick") a client | Client stops without auto-reconnecting |
| Pairing rejected on either side | Both stop, nothing is stored as trusted |
| Paired client reconnects | No prompt, 28 ms |
| GUI killed while hooks are installed | Windows removes the hooks with the process; no input left captured |
| Our keyboard hook bypassed (own WebView2 window focused) | Focus moved to the helper window while remote; watchdog re-installs hooks when bypassed by others |

## Measuring on two computers

1. Run Moush on both machines and pair them.
2. On either machine open **Diagnostics → Run latency test** (100 pings 10 ms apart) and
   read the RTT percentiles; move the mouse on the laptop for a while and read
   *hook → socket* (host) and *frame → SendInput* (client).
3. For the Wi-Fi interruption test: disable Wi-Fi on the laptop for 2 s and 10 s while the
   cursor is on it. Expected: cursor returns to the host within 3.5 s of the drop; the
   laptop releases all keys; it reconnects automatically (Diagnostics shows the reconnect
   duration).
4. Load test: start a large download on the laptop and repeat the latency test.

The headless tool gives the same data without the UI: `moush host` on the desktop,
`moush client <desktop-ip>` on the laptop; both print latency statistics when stopped
with Enter.

## Release size, idle cost and latency after the feature foundation (October 2026)

Measured on the same machine while other builds were running (latency percentiles are
therefore noisy; the medians are stable). Reproduce with `scripts\measure-size.ps1`
(one build variant: sizes, `moush bench`, loopback latency with and without bulk traffic,
transport throughput) and `scripts\measure-idle.ps1` (idle CPU, wake-ups, memory, start-up).

### Binary size

`Moush.exe` is the desktop app, `moush.exe` the CLI. "Crypto" is the CPU-bound transport
throughput (encode + encrypt / decrypt + decode of 32 KiB chunks, `tests/throughput.rs`),
"bench" is `moush bench --events 3000` (enqueue on the host to injected on the client,
loopback). Each row changes one thing relative to the row above it unless it says otherwise.

| Variant | Moush.exe | moush.exe | Crypto MB/s | bench p50 / p99 |
| --- | ---: | ---: | ---: | ---: |
| Previous release (0.1.0, before the foundation) | 10,681,344 | — | — | 53 / 139 µs |
| Baseline: foundation, `opt-level=3`, thin LTO, `strip="debuginfo"` | 10,917,888 | 1,669,120 | 590–744 / 634–706 | 52 / 108 µs |
| baseline + `strip=true` | 10,915,840 | 1,669,120 | — | — |
| baseline + fat LTO | 10,384,384 | 1,602,048 | 532–609 / 653–816 | 50 / 120 µs |
| baseline + `opt-level="s"` | 8,034,816 | 1,333,248 | 252–277 / 241–280 | 45 / 86 µs |
| baseline + `opt-level="z"` | 6,838,272 | 1,250,816 | 165–179 / 174–217 | 56 / 137 µs |
| baseline + `panic="abort"` (not adopted) | 5,914,624 | 1,226,752 | 657 / 869 | 44 / 105 µs |
| `"s"`, hot crates at 3 | 8,290,304 | 1,558,528 | 787 / 694 | 42 / 81 µs |
| `"s"`, hot crates at 3, fat LTO, `strip=true` | 8,012,288 | 1,522,176 | 937 / 897 | 54 / 138 µs |
| previous + snow without `ring`/AES/SHA-2 | 7,959,040 | 1,468,928 | 680 / 808 | 49 / 121 µs |
| same with `"z"` | 6,790,144 | 1,440,256 | 685 / 805 | 55 / 133 µs |
| **same + Tauri without `dynamic-acl` (chosen)** | **6,489,600** | **1,440,256** | **933 / 865** | (CLI unchanged) |
| chosen, final tree (all fixes in this review) | 6,492,672 | 1,442,304 | 663 / 699 | 50 / 135 µs |
| chosen, Tauri also without `compression` | 6,541,824 | — | — | — |
| chosen + `panic="abort"` (not adopted) | 3,662,848 | 1,005,568 | — | — |

Final build with `npx tauri build` (chosen config): `Moush.exe` 6,492,672 bytes, NSIS
installer `Moush_0.1.0_x64-setup.exe` **1,869,590 bytes (1.78 MiB)**, down from 2.48 MB for
0.1.0. The app starts, shows its first-run page through IPC, and with the window open uses
29.9 MB working set / 9.8 MB private (Moush.exe alone; WebView2 processes extra), compared
with 32 MB before.

"Hot crates" = `moush-core`, `moush-engine`, `snow`, `chacha20poly1305`, `chacha20`,
`poly1305`, `blake2`, `curve25519-dalek`: the hook/router, the transport and the crypto.

Findings:
- Input latency does not depend on the optimisation level (p50 42–56 µs in every
  variant): it is dominated by thread wake-ups, not computation.
- Crypto does: with a global `"z"` it runs ~4x slower (~180 µs instead of ~50 µs to decrypt
  a 32 KiB chunk), and a client decrypts bulk chunks on the thread that injects input. The
  chosen config keeps the hot crates at `opt-level=3`, so transport speed is unchanged
  while everything else (Tauri, WebView2 glue, serde, UI commands) is size-optimized.
- `strip=true` saves only 2 KB (PE files keep no debug info; symbols are in the PDB).
- `snow`'s default `std` feature enables its optional `ring` dependency (`ring/std`), and its
  default resolver adds AES-GCM and SHA-2; Moush uses only ChaCha20-Poly1305, BLAKE2s and
  X25519. Without them: −53 KB per binary, and `ring` (C/assembly) is no longer compiled.
- Tauri's `dynamic-acl` only keeps the raw ACL for `add_capability` at run time (unused);
  the resolved ACL is still enforced. `x11`/`dbus` do nothing on Windows. `compression`
  (brotli-compressed frontend assets) is worth keeping (−52 KB).
- `panic="abort"` would save a further 2.8 MB (unwind tables and landing pads in all of
  Tauri/wry) but is not used: the hook callback relies on `catch_unwind` (an internal error
  lets the event through instead of killing the process), and a panic triggered by a peer's
  data must end one connection, not the app. Revisit once feature parsers are fuzzed and a
  panic hook that restores input is in place.

Debug builds: the crypto crates (`snow`, `chacha20`, `poly1305`, `chacha20poly1305`,
`blake2`, `curve25519-dalek`, `universal-hash`, `cipher`) are built with `opt-level=3` in the
dev profile too. Unoptimized, the transport ran at 3–4 MB/s and input latency during the
bulk loopback test was 16 ms p50 (each 32 KiB chunk took ~8 ms to decrypt on the
injection thread); with the override: 81 / 313 MB/s and 0.15 ms p50, for ~15 s of extra
first-build time.

### Idle cost (60 s, release CLI, host with real hooks + dry-run client on loopback)

| | CPU | Context switches | Working set | Private | Threads |
| --- | --- | --- | --- | --- | --- |
| Host (`moush host`) | 31 ms in 65 s = 0.05 % of one core (0.003 % of the machine) | ~9.4/s | 10.6 MB | 2.6 MB | 11 |
| Client (`moush client --dry-run`) | 62 ms in 65 s = 0.10 % of one core (0.006 % of the machine) | ~8.8/s | 10.3 MB | 2.4 MB | 9 |

CPU time on Windows advances in 15.6 ms ticks, so these are 2–4 ticks: at the resolution
limit, consistent with the earlier 0.01 % figure. The wake-ups are the 1 s keepalive (writer
and reader on each side), the 1 s hook watchdog on the host, and the CLI's own status loop
(2/s on the host, 3.3/s on the client; the GUI has no such loop). No regression from the
feature foundation: the hub adds no thread and no timer.

Start-up: the host engine is ready (identity, hooks, listener, discovery) 38–137 ms after
process launch; a client is connected 330–390 ms after launch (the CLI polls its state
every 300 ms; the handshake plus authorization itself takes 23 ms in `moush bench`,
reconnect with stored trust 28–43 ms).

### Input latency with and without bulk traffic (release, loopback)

| Scenario | p50 | p95 / p99 | max |
| --- | --- | --- | --- |
| 1 kHz key events, no other traffic (3 runs) | 41–63 µs | p99 73–113 µs | 92–302 µs |
| Same during an 8 MB flow-controlled stream (128 KiB window) | 43–68 µs | p95 86–154 µs | 223–346 µs |

The flow-controlled stream runs at ~40 MB/s on loopback (limited by the 128 KiB window and
the acknowledgement round trip, not by CPU). Unthrottled, eight threads pushed 636 MB in 2 s
(`bulk_contention_never_refuses_input`) without a single input event being refused; the
host's `try_send` never took more than 0.75 ms (scheduler noise on a loaded machine; typical
20–40 µs). A client whose reader stalls for 1.2 s loses nothing; one that stops reading is
disconnected after ~4 s (dead-peer check), and the host's input path never waits.

Compared with the numbers at the top of this document: latency unchanged, idle cost
unchanged, release app 39 % smaller than 0.1.0 (6.49 MB vs 10.68 MB) and installer 25 %
smaller (1.87 MB vs 2.48 MB), even though the foundation added ~0.24 MB.
