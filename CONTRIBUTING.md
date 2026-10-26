# Contributing to Moush

Moush is MIT-licensed and open to changes of any size: bug reports, fixes, new features, docs,
translations of the UI copy, or a fork that takes it somewhere new. By participating you agree
to the [Code of Conduct](CODE_OF_CONDUCT.md) and to license your contributions under MIT.

## Getting set up

Requirements: Windows 10/11, Rust 1.85+ (MSVC toolchain), Node.js 22+ (Vite 8 also accepts
20.19+), WebView2 (preinstalled on Windows 11). The `rust-toolchain.toml` installs the `rustfmt`
and `clippy` components automatically.

```powershell
git clone https://github.com/rakibul-islam-hasib/moush
cd moush
npm install
npx tauri dev                       # app with hot reload
cargo test --workspace              # unit + loopback integration tests
cargo build --release -p moush-cli  # headless tool, handy for two-machine testing
```

You don't need two PCs to work on most of Moush. `moush-cli client <ip> --dry-run` logs input
instead of injecting it, and `scripts\e2e-local.ps1` runs a real GUI host against a dry-run client
on one machine.

## Where things live

| You want to change… | Look in |
| --- | --- |
| Wire protocol, Noise channel, layout geometry, input routing | `crates/moush-core` (OS-independent, fully unit-tested) |
| Hooks, `SendInput`, monitors, DPAPI, firewall | `crates/moush-engine` |
| Tauri commands, tray, window | `src-tauri` |
| The UI | `src` (React + TypeScript); `src/api/types.ts` is the UI ↔ backend contract |
| The website | `website` (static, deployed to GitHub Pages on push to `main`) |
| The launch film and screenshots | `website/assets` (media credits: `THIRD_PARTY_NOTICES.md`) |

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the pieces fit together, and
[docs/SECURITY.md](docs/SECURITY.md) the threat model. Read the second one before touching pairing
or the transport.

## Before you open a pull request

- `cargo fmt --all` (the repo uses 120 columns, see `rustfmt.toml`) and
  `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace` passes.
- `npm run build` passes (type check + Vite build) if you touched `src`.
- UI changes follow the existing design language: tonal depth instead of shadows, borders only
  where they mark state, Geist, motion at or under 200 ms (`src/styles/tokens.css`). Include a
  screenshot in light and dark.
- Anything on the input path: say how you measured latency (see
  [docs/PERFORMANCE.md](docs/PERFORMANCE.md)).

Small, focused PRs get reviewed fastest. For a bigger change, open an issue first so we can agree
on the approach.

## Reporting bugs

Open an issue with your Windows version, the role of each computer (host or client), what you
did and what happened. Logs are in `%LOCALAPPDATA%\Moush\logs`; check them for device names or IPs
you'd rather not share before attaching them.

Security issues: please don't open a public issue. Follow [SECURITY.md](SECURITY.md) and report
privately.
