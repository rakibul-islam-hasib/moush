# Security policy

Moush moves keyboard and mouse input, clipboard contents and files between computers. A
vulnerability can therefore mean control of another machine or exposure of what is typed,
copied or transferred. Please report security issues privately.

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |
| Older or unreleased builds | No |

Only the latest release on the `main` branch receives security fixes.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting:

<https://github.com/rakibul-islam-hasib/moush/security/advisories/new>

Please do **not** open a public issue for a security problem. If you cannot use the form,
contact the maintainer privately through GitHub
([@rakibul-islam-hasib](https://github.com/rakibul-islam-hasib)).

Include as much of this as you can:

- Moush version and Windows version/build.
- Which computer has which role (host or client).
- Steps to reproduce and the impact you believe it has.
- Whether the attacker needs a paired device, LAN access, a malicious file or clipboard
  payload, or code already running as the same Windows user.
- Relevant logs from `%LOCALAPPDATA%\Moush\logs`, with device names and IP addresses
  redacted.

## Scope

In scope:

- The Noise transport, pairing and trust decisions.
- Input capture, injection and the input router.
- Clipboard, text sharing and file transfer.
- The Tauri command surface and the UI/backend boundary.
- The installer, autostart, firewall rule and DPAPI-protected identity.

Out of scope (also explained in the threat model):

- Malware already running as the same Windows user.
- The secure desktop (Ctrl+Alt+Del, the lock screen, UAC prompts).
- Physical access to an unlocked computer.
- A paired host deliberately injecting input into a client; that is the product's purpose.

The full threat model, assets and design rules are in
[docs/SECURITY.md](docs/SECURITY.md). Read it before changing pairing, trust or the
transport.

## Disclosure

We will acknowledge the report, investigate it and coordinate a fix and disclosure date
with you. Please give us a chance to ship a fix before publishing details. Security
advisories are published in this repository.
