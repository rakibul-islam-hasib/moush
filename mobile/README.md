# Moush for Android

A phone remote for a computer running Moush as **host**: touchpad, keyboard, media and
shortcut deck, remote screen viewer and file sharing. Kotlin and Jetpack Compose; no native
code. It speaks the same protocol as the desktop app (Noise XX over TCP, same pairing code).

## What it does

| Tab | |
|---|---|
| **Pad** | Touchpad with acceleration, tap / two-finger tap / three-finger tap, two-finger scroll, pinch zoom, tap-and-hold drag, three-finger desktop switching, and an air mouse that uses the gyroscope. |
| **Keys** | Live typing straight to the computer, sticky Ctrl/Alt/Shift/Win, arrows with auto-repeat, F1–F12, "type my clipboard". |
| **Screen** | Watch any display. A bottom sheet shows the monitors laid out like on your desk. Pinch to zoom: the computer then sends only the visible part at full detail. Tap to click, press and hold to aim with a magnifier, drag to pan, two fingers to scroll. |
| **Deck** | Media transport and volume, common shortcuts (copy, paste, snip, lock…), presenter controls. |
| **Files** | Send files and whole folders to the computer; progress, speed and cancel. Also appears in Android's **Share** menu. |
| **Link** | Find computers on the Wi-Fi, pair, pointer / scroll speed, haptics, theme. |

## Pairing

Open **Link**, tap a computer (or *Enter address*). Both screens show the same six digits;
confirm on the phone, then approve on the computer. Computers must be running Moush as
**host**. Per-device permissions (files, screen viewing) are in Moush › Devices on the PC.

## Build

Needs Android Studio's JDK (17+) and the Android SDK (platform 37).

```
cd mobile
./gradlew :app:assembleDebug        # app/build/outputs/apk/debug/app-debug.apk
```

Tests against a real host (builds nothing on Android, needs `cargo build -p moush-cli` first):

```
set MOUSH_CLI=..\target\debug\moush-cli.exe
gradlew :app:testDebugUnitTest
```

They run the Noise handshake, pairing, input, a folder transfer with hash verification and a
JPEG screen frame against `moush-cli host`.

## Limits

- Typing sends US-layout keys; the computer maps them through its own layout. Non-ASCII text
  is skipped.
- Receiving files on the phone is not implemented (the phone declines offers politely).
- Transfers and the screen stream pause when Android suspends the app in the background.
- A computer that blocks inbound connections (Windows Firewall) cannot be reached; allow
  Moush on private networks.
