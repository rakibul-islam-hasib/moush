# Changelog

All notable changes to Moush are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-26

First public release. Moush is Windows-only and still 0.x software; the wire protocol,
settings format and UI may change before 1.0.

### Added

- One mouse and keyboard shared between Windows computers on the same LAN, with the cursor
  moving across screen edges and the keyboard following it.
- Multi-monitor and mixed-DPI support with a drag-and-drop screen layout editor.
- Event-driven input capture and injection over a single TCP connection with `TCP_NODELAY`.
- Noise `XX_25519_ChaChaPoly_BLAKE2s` transport and two-sided 6-digit pairing approval.
- Guaranteed key release when the cursor moves, the connection drops, the session locks or
  the app exits.
- Keyboard shortcuts for jumping between computers and bringing the cursor home.
- Shared clipboard for text, rich text and images, with sensitive-clip filtering and an
  optional in-memory history.
- Encrypted, integrity-checked, resumable file transfer with receiver approval.
- Per-device feature switches, tray menu, autostart, and light, dark, high-contrast and
  reduced-motion themes.
- `moush-cli` headless host/client, benchmark, discovery and self-test tool.
- Install, keyboard, architecture, security, performance and troubleshooting documentation.
- Static website and launch film.
- GitHub Actions CI, issue and pull request templates, Dependabot, security policy, code of
  conduct and contributor guide.

### Security

- Identity keys are protected at rest with DPAPI.
- New devices require approval on both computers; pairing attempts are isolated from trusted
  devices and rate-limited.
- Firewall rule helper and documented threat model.

### Known limitations

- Windows 10 and 11 only. macOS and Linux are not supported.
- Ctrl+Alt+Del, the lock screen and UAC prompts run on the secure desktop and cannot be
  captured or injected into.
- Elevated windows on a client reject input from a non-elevated Moush process (UIPI).
- Release installers are currently unsigned, so Windows SmartScreen may warn on first run.

[Unreleased]: https://github.com/rakibul-islam-hasib/moush/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rakibul-islam-hasib/moush/releases/tag/v0.1.0
