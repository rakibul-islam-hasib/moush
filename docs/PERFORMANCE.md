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
