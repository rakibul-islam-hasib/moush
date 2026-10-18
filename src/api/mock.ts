// In-memory stand-in for the Rust backend, used when the UI runs in a plain
// browser. It implements the same surface as backend.ts with realistic data.
//
// URL parameters (browser preview only):
//   ?mockRole=host|client|none   starting role (default host)
//   ?connected=0                 client role: start disconnected (default: connected)
//   ?theme=light|dark|system     starting theme setting
//   ?motion=system|reduce|full   starting motion setting
//   ?mica=1                      pretend the window has the Mica backdrop

import type { api as realApi } from "./backend";
import type { Backend, MockControls } from "./index";
import type {
  AppSnapshot,
  ClientLayoutView,
  ClientSnapshot,
  ClipboardEntry,
  ClipboardStatus,
  ClipKind,
  ConnectedClient,
  DevicePolicy,
  Diagnostics,
  DiscoveredHost,
  FeaturePeer,
  FirewallState,
  LatencyStats,
  LogEntry,
  LogLevel,
  MonitorInfo,
  PairingRequest,
  Role,
  Settings,
  SharedTextView,
  Theme,
  TransferFile,
  TransferView,
  TrustedDevice,
} from "./types";
import { boundsOf, touchesAny } from "../lib/layoutGeometry";
import { parseShortcut } from "../lib/shortcuts";

type Api = typeof realApi;

interface MockPeer {
  id: string;
  name: string;
  address: string;
  monitors: MonitorInfo[];
  x: number;
  y: number;
  connected: boolean;
  rttMs: number;
  connectedAt: number;
}

interface HostInfo {
  id: string;
  name: string;
  address: string;
}

/** What the remote computer allows with us (its own settings), for the effective capabilities. */
interface RemoteCaps {
  clipboard: boolean;
  files: boolean;
  textShare: boolean;
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
/** Mirrors the real backend, which rejects with a plain string. */
function fail(message: string): never {
  throw message;
}
const rand = (min: number, max: number) => min + Math.random() * (max - min);
const randomCode = () => String(Math.floor(rand(0, 1_000_000))).padStart(6, "0");
const randomId = () => Array.from({ length: 16 }, () => Math.floor(Math.random() * 16).toString(16)).join("");
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const MB = 1024 * 1024;

function monitor(index: number, name: string, x: number, y: number, width: number, height: number, scale: number, primary: boolean): MonitorInfo {
  return { id: `\\\\.\\DISPLAY${index}`, name, x, y, width, height, dpi: Math.round(96 * scale), scale, primary };
}

const HOST_MONITORS: MonitorInfo[] = [
  monitor(1, "Display 1", 0, 0, 2560, 1440, 1, true),
  monitor(2, "Display 2", 2560, 0, 2560, 1440, 1, false),
];
const LAPTOP_MONITORS: MonitorInfo[] = [monitor(1, "Built-in display", 0, 0, 1920, 1200, 1.5, true)];
const MEDIA_MONITORS: MonitorInfo[] = [monitor(1, "LG TV", 0, 0, 3840, 2160, 2, true)];
const LIVINGROOM_MONITORS: MonitorInfo[] = [monitor(1, "Dell U2723", 0, 0, 1920, 1080, 1, true)];

const LAPTOP_ID = "9f3c2a71b04e8d15";
const MEDIA_ID = "7a2e5c90d13f4b66";
const LIVINGROOM_ID = "c81f06d2e4a97b33";
const STUDIO_ID = "41d7e9a0c3b25f68";
const OFFICE_ID = "e2b5c7194f0a6d81";

const fingerprintOf = (id: string) => id.match(/.{4}/g)?.join("-") ?? id;

function latency(base: number, count: number): LatencyStats {
  const p50 = base * rand(0.9, 1.1);
  return { count, mean: p50 * 1.15, p50, p95: p50 * rand(2, 2.6), p99: p50 * rand(3.2, 4.2), max: p50 * rand(7, 12) };
}

const params = () => new URLSearchParams(window.location.search);

function startingRole(): Role | null {
  const param = params().get("mockRole");
  if (param === "client") return "client";
  if (param === "none") return null;
  return "host";
}

function defaultSettings(deviceName: string, lastHostAddress: string | null): Settings {
  return {
    deviceName,
    theme: "system",
    launchAtStartup: false,
    startMinimized: false,
    closeToTray: true,
    autoStart: true,
    port: 24800,
    discoveryEnabled: true,
    switchDelayMs: 0,
    cornerDeadZonePx: 0,
    clientPointerSpeed: 1,
    scrollLockToggle: true,
    lastHostAddress,
    clipboard: { enabled: true, text: true, richText: true, images: true, maxSizeKb: 10 * 1024, skipSensitive: true, history: true, historyLimit: 25 },
    keyboard: {
      forwardKeyboard: true,
      forwardSystemShortcuts: true,
      localShortcuts: [],
      switchHotkeys: true,
      focusOnEnter: false,
      edgeSwitchModifier: "none",
    },
    transfer: { enabled: true, downloadDir: "", askBeforeReceiving: true, overwrite: "rename", speed: "balanced", openFolderWhenDone: false },
    notifications: { enabled: true, connections: true, transfers: true, sharedText: true, clipboard: false },
    appearance: { transparency: true, motion: "system" },
  };
}

/** Validation mirroring settings.rs / core::features (clamps ranges, normalises enums). */
function validateSettings(next: Settings): Settings {
  if (!Number.isInteger(next.port) || next.port < 1024 || next.port > 65535) fail("Port must be between 1024 and 65535");
  const deviceName = next.deviceName.trim().slice(0, 64);
  if (!deviceName) fail("Device name can't be empty");
  const k = next.keyboard;
  // Like keys.rs: normalise, drop entries that don't parse, remove duplicates.
  const shortcuts = [...new Set(k.localShortcuts.map((s) => parseShortcut(s)).flatMap((r) => (r.ok ? [r.value] : [])))];
  return {
    ...next,
    deviceName,
    theme: (["system", "light", "dark"] as Theme[]).includes(next.theme) ? next.theme : "system",
    switchDelayMs: clamp(Math.round(next.switchDelayMs), 0, 1000),
    cornerDeadZonePx: clamp(Math.round(next.cornerDeadZonePx), 0, 200),
    clientPointerSpeed: Number.isFinite(next.clientPointerSpeed) ? clamp(next.clientPointerSpeed, 0.25, 4) : 1,
    clipboard: {
      ...next.clipboard,
      maxSizeKb: clamp(Math.round(next.clipboard.maxSizeKb), 16, 64 * 1024),
      historyLimit: clamp(Math.round(next.clipboard.historyLimit), 1, 200),
    },
    keyboard: {
      ...k,
      localShortcuts: shortcuts.slice(0, 16),
      edgeSwitchModifier: ["none", "ctrl", "shift", "alt"].includes(k.edgeSwitchModifier) ? k.edgeSwitchModifier : "none",
    },
    transfer: {
      ...next.transfer,
      downloadDir: next.transfer.downloadDir.trim(),
      overwrite: ["rename", "overwrite", "skip"].includes(next.transfer.overwrite) ? next.transfer.overwrite : "rename",
      speed: next.transfer.speed === "fast" ? "fast" : "balanced",
    },
    appearance: {
      ...next.appearance,
      motion: ["system", "reduce", "full"].includes(next.appearance.motion) ? next.appearance.motion : "system",
    },
  };
}

/** Layout rectangle for a peer: its desktop normalised to the host DPI (100 %). */
function peerSize(p: MockPeer) {
  const b = boundsOf(p.monitors) ?? { x: 0, y: 0, width: 1920, height: 1080 };
  const scale = p.monitors.find((m) => m.primary)?.scale ?? 1;
  return { resW: b.width, resH: b.height, scale, width: b.width / scale, height: b.height / scale };
}

const CLIP_SAMPLES: { kind: ClipKind; preview: string; bytes: number }[] = [
  { kind: "text", preview: "git switch -c feature/clipboard-history && npm run dev", bytes: 54 },
  { kind: "rich", preview: "Quarterly roadmap — Q4 priorities: input latency, clipboard images, file transfer resume", bytes: 4_812 },
  { kind: "image", preview: "Image 1920 × 1080", bytes: 2_412_544 },
  { kind: "text", preview: "https://learn.microsoft.com/windows/win32/dataxchg/clipboard", bytes: 61 },
  { kind: "text", preview: "Meeting moved to 15:30, room 4.12 (bring the HDMI adapter)", bytes: 58 },
  { kind: "image", preview: "Image 812 × 460", bytes: 381_220 },
];

export function createMockBackend(): Backend {
  const startedAt = Date.now();
  const query = params();
  let role = startingRole();
  const thisIsLaptop = role === "client";
  const myName = thisIsLaptop ? "SURFACE-LAPTOP" : "STUDIO-PC";

  let settings: Settings = defaultSettings(myName, thisIsLaptop ? "192.168.1.20:24800" : null);
  settings = {
    ...settings,
    launchAtStartup: true,
    switchDelayMs: 150,
    cornerDeadZonePx: 40,
    keyboard: { ...settings.keyboard, localShortcuts: ["Ctrl+Shift+Esc", "Win+D"] },
  };
  const themeParam = query.get("theme");
  if (themeParam === "light" || themeParam === "dark" || themeParam === "system") settings.theme = themeParam;
  const motionParam = query.get("motion");
  if (motionParam === "reduce" || motionParam === "full" || motionParam === "system") {
    settings.appearance = { ...settings.appearance, motion: motionParam };
  }
  const micaCapable = query.get("mica") === "1";

  // ---- host side
  let hostRunning = true;
  let activeTarget: string | null = null;
  let locked = false;
  const peers: MockPeer[] = [
    {
      id: LAPTOP_ID,
      name: "SURFACE-LAPTOP",
      address: "192.168.1.34:52110",
      monitors: LAPTOP_MONITORS,
      x: -1280,
      y: 320,
      connected: true,
      rttMs: 1.8,
      connectedAt: Date.now() - 42 * 60_000,
    },
    {
      id: MEDIA_ID,
      name: "MEDIA-PC",
      address: "192.168.1.61:50982",
      monitors: MEDIA_MONITORS,
      x: 5120,
      y: 360,
      connected: true,
      rttMs: 3.4,
      connectedAt: Date.now() - 7 * 60_000,
    },
  ];
  /** What each remote computer allows (its own settings). MEDIA-PC has file transfer turned off. */
  const remote: Record<string, RemoteCaps> = {
    [LAPTOP_ID]: { clipboard: true, files: true, textShare: true },
    [MEDIA_ID]: { clipboard: true, files: false, textShare: true },
    [LIVINGROOM_ID]: { clipboard: true, files: true, textShare: true },
    [STUDIO_ID]: { clipboard: true, files: true, textShare: true },
    [OFFICE_ID]: { clipboard: true, files: true, textShare: true },
  };

  // ---- client side
  const idleClient = (): ClientSnapshot => ({
    state: "idle",
    target: null,
    hostId: null,
    hostName: null,
    hostAddress: null,
    active: false,
    rttMs: null,
    lastError: null,
    retryInMs: null,
    monitors: LAPTOP_MONITORS,
  });
  let client = idleClient();
  if (thisIsLaptop && query.get("connected") !== "0") {
    client = {
      ...idleClient(),
      state: "connected",
      target: "STUDIO-PC",
      hostId: STUDIO_ID,
      hostName: "STUDIO-PC",
      hostAddress: "192.168.1.20:24800",
      rttMs: 1.6,
    };
  }
  let discovered: DiscoveredHost[] = [
    { id: STUDIO_ID, name: "STUDIO-PC", address: "192.168.1.20:24800", lastSeen: Date.now(), trusted: true },
    { id: OFFICE_ID, name: "OFFICE-DESKTOP", address: "192.168.1.41:24800", lastSeen: Date.now(), trusted: false },
  ];

  // ---- shared
  const policy = (p?: Partial<DevicePolicy>): DevicePolicy => ({ clipboard: true, files: true, textShare: true, ...p });
  let pairing: PairingRequest[] = [];
  let trusted: Omit<TrustedDevice, "online">[] = thisIsLaptop
    ? [
        {
          id: STUDIO_ID,
          name: "STUDIO-PC",
          role: "host",
          fingerprint: fingerprintOf(STUDIO_ID),
          lastAddress: "192.168.1.20:24800",
          pairedAt: Date.now() - 20 * 86_400_000,
          lastSeen: Date.now() - 3 * 3_600_000,
          policy: policy(),
        },
      ]
    : [
        {
          id: LAPTOP_ID,
          name: "SURFACE-LAPTOP",
          role: "client",
          fingerprint: fingerprintOf(LAPTOP_ID),
          lastAddress: "192.168.1.34:52110",
          pairedAt: Date.now() - 20 * 86_400_000,
          lastSeen: Date.now(),
          policy: policy(),
        },
        {
          id: MEDIA_ID,
          name: "MEDIA-PC",
          role: "client",
          fingerprint: fingerprintOf(MEDIA_ID),
          lastAddress: "192.168.1.61:50982",
          pairedAt: Date.now() - 6 * 86_400_000,
          lastSeen: Date.now(),
          policy: policy({ clipboard: false }),
        },
        {
          id: OFFICE_ID,
          name: "OFFICE-LAPTOP",
          role: "client",
          fingerprint: fingerprintOf(OFFICE_ID),
          lastAddress: "192.168.1.77:51022",
          pairedAt: Date.now() - 45 * 86_400_000,
          lastSeen: Date.now() - 4 * 86_400_000,
          policy: policy(),
        },
      ];
  let firewall: FirewallState = "unknown";
  let latencyTestRunning = false;
  const counters = { captured: 182_340, forwarded: 96_118, injected: 0, bytesSent: 14_820_331, bytesReceived: 2_310_442 };

  // ---- features
  const otherName = thisIsLaptop ? "STUDIO-PC" : "SURFACE-LAPTOP";
  const otherId = thisIsLaptop ? STUDIO_ID : LAPTOP_ID;
  let clipHistory: ClipboardEntry[] = CLIP_SAMPLES.slice(0, 5).map((c, i) => ({
    id: randomId(),
    kind: c.kind,
    preview: c.preview,
    bytes: c.bytes,
    origin: i % 3 === 0 ? "received" : i % 3 === 1 ? "sent" : "local",
    source: i % 3 === 0 ? otherName : "This PC",
    at: Date.now() - (i * 7 + 2) * 60_000,
  }));
  let clip: ClipboardStatus = {
    enabled: true,
    lastSyncAt: Date.now() - 2 * 60_000,
    lastDirection: "received",
    lastPeer: otherName,
    lastKind: "text",
    historyCount: clipHistory.length,
    historyRevision: 1,
    error: null,
  };
  const DOWNLOADS = "C:\\Users\\you\\Downloads\\Moush";

  const transfer = (t: Partial<TransferView> & Pick<TransferView, "direction" | "files" | "state">): TransferView => {
    const total = t.files.reduce((n, f) => n + f.size, 0);
    return {
      id: randomId(),
      peerId: otherId,
      peerName: otherName,
      totalBytes: total,
      doneBytes: 0,
      bytesPerSec: 0,
      error: null,
      startedAt: Date.now(),
      finishedAt: null,
      destination: null,
      canRetry: false,
      ...t,
    };
  };
  let transfers: TransferView[] = [
    transfer({
      direction: "receive",
      state: "pending",
      files: [
        { name: "Invoice-2026-10.pdf", size: 412_331 },
        { name: "Screenshot 2026-10-19 093112.png", size: 1_842_112 },
        { name: "notes.md", size: 6_120 },
      ],
      startedAt: Date.now() - 20_000,
    }),
    transfer({
      direction: "send",
      state: "active",
      files: [{ name: "Project-Q3-recording.mp4", size: 1_480 * MB }],
      doneBytes: 560 * MB,
      bytesPerSec: 46 * MB,
      startedAt: Date.now() - 14_000,
    }),
    transfer({
      direction: "receive",
      state: "done",
      files: [{ name: "holiday-photos.zip", size: 228 * MB }],
      doneBytes: 228 * MB,
      startedAt: Date.now() - 50 * 60_000,
      finishedAt: Date.now() - 49 * 60_000,
      destination: DOWNLOADS,
    }),
    transfer({
      direction: "send",
      state: "failed",
      files: [
        { name: "design-assets.fig", size: 84 * MB },
        { name: "brand-guide.pdf", size: 12 * MB },
      ],
      doneBytes: 31 * MB,
      error: "The connection was lost.",
      canRetry: true,
      startedAt: Date.now() - 3 * 3_600_000,
      finishedAt: Date.now() - 3 * 3_600_000 + 40_000,
    }),
    transfer({
      direction: "receive",
      state: "rejected",
      files: [{ name: "setup.exe", size: 88 * MB }],
      startedAt: Date.now() - 26 * 3_600_000,
      finishedAt: Date.now() - 26 * 3_600_000,
    }),
  ];
  let sharedTexts: SharedTextView[] = [
    {
      id: randomId(),
      direction: "received",
      peerName: otherName,
      preview: "https://github.com/moush-app/moush/pull/482",
      chars: 43,
      isLink: true,
      at: Date.now() - 4 * 60_000,
    },
    {
      id: randomId(),
      direction: "received",
      peerName: otherName,
      preview: "Wi-Fi for the guests: Moush-Guest / password lighthouse-47-orbit",
      chars: 64,
      isLink: false,
      at: Date.now() - 38 * 60_000,
    },
    {
      id: randomId(),
      direction: "sent",
      peerName: otherName,
      preview: "SELECT id, name FROM devices WHERE last_seen > now() - interval '1 day';",
      chars: 72,
      isLink: false,
      at: Date.now() - 2 * 3_600_000,
    },
  ];

  const snapshotListeners = new Set<(s: AppSnapshot) => void>();
  const logListeners = new Set<(e: LogEntry) => void>();
  const logs: LogEntry[] = [];

  function log(level: LogLevel, target: string, message: string) {
    const entry: LogEntry = { ts: Date.now(), level, target: `moush::${target}`, message };
    logs.push(entry);
    if (logs.length > 2000) logs.splice(0, logs.length - 2000);
    if (level !== "debug") logListeners.forEach((cb) => cb(structuredClone(entry)));
  }

  // Seed some history.
  {
    const seed: [LogLevel, string, string][] = [
      ["info", "app", "Moush 0.2.0 starting (Windows 11 build 26200)"],
      ["info", "config", "Loaded settings (schema 2)"],
      ["debug", "input", "Monitor layout: 2 displays, virtual desktop 5120x1440"],
      ["info", "net", "Listening on 0.0.0.0:24800"],
      ["info", "discovery", "Answering LAN discovery on UDP 24801"],
      ["warn", "firewall", "Could not verify Windows Firewall rule (access denied)"],
      ["info", "input", "Low-level keyboard and mouse hooks installed"],
      ["info", "net", "Connection from 192.168.1.34:52110"],
      ["info", "pairing", "SURFACE-LAPTOP is trusted (fingerprint 9f3c-2a71-b04e-8d15)"],
      ["info", "net", "SURFACE-LAPTOP connected, rtt 1.9 ms"],
      ["info", "features", "SURFACE-LAPTOP: clipboard, files, text sharing"],
      ["debug", "input", "Edge crossing: Display 1 left -> SURFACE-LAPTOP"],
      ["error", "net", "Write to 192.168.1.34:52110 failed: connection reset; reconnecting"],
      ["info", "net", "SURFACE-LAPTOP reconnected in 412 ms"],
    ];
    const base = Date.now() - seed.length * 37_000;
    seed.forEach(([level, target, message], i) => logs.push({ ts: base + i * 37_000, level, target: `moush::${target}`, message }));
  }

  function layoutView(p: MockPeer): ClientLayoutView {
    const s = peerSize(p);
    const rect = { x: p.x, y: p.y, width: s.width, height: s.height };
    return {
      clientId: p.id,
      name: p.name,
      online: hostRunning && p.connected,
      x: p.x,
      y: p.y,
      width: s.width,
      height: s.height,
      resolutionWidth: s.resW,
      resolutionHeight: s.resH,
      scale: s.scale,
      monitors: p.monitors,
      adjacent: touchesAny(rect, HOST_MONITORS),
    };
  }

  /** Connected peers as {id, name}. Host: connected clients. Client: the host. */
  function connectedPeers(): { id: string; name: string }[] {
    if (role === "host") return hostRunning ? peers.filter((p) => p.connected).map((p) => ({ id: p.id, name: p.name })) : [];
    if (role === "client" && client.state === "connected" && client.hostId) return [{ id: client.hostId, name: client.hostName ?? "Host" }];
    return [];
  }

  function policyOf(id: string): DevicePolicy {
    return trusted.find((t) => t.id === id)?.policy ?? policy();
  }

  function featurePeers(): FeaturePeer[] {
    return connectedPeers().map(({ id, name }) => {
      const pol = policyOf(id);
      const r = remote[id] ?? { clipboard: true, files: true, textShare: true };
      return {
        id,
        name,
        clipboard: settings.clipboard.enabled && pol.clipboard && r.clipboard,
        files: settings.transfer.enabled && pol.files && r.files,
        textShare: pol.textShare && r.textShare,
      };
    });
  }

  function build(): AppSnapshot {
    const clients: ConnectedClient[] = hostRunning
      ? peers
          .filter((p) => p.connected)
          .map((p) => ({
            id: p.id,
            name: p.name,
            address: p.address,
            active: activeTarget === p.id,
            rttMs: p.rttMs,
            monitors: p.monitors,
            connectedAt: p.connectedAt,
          }))
      : [];
    const online = new Set(connectedPeers().map((p) => p.id));
    return {
      role,
      deviceName: settings.deviceName,
      deviceId: thisIsLaptop ? LAPTOP_ID : STUDIO_ID,
      fingerprint: fingerprintOf(thisIsLaptop ? LAPTOP_ID : STUDIO_ID),
      version: "0.2.0",
      platform: { osBuild: 26200, windows11: true, mica: micaCapable && settings.appearance.transparency, elevated: false },
      host:
        role === "host"
          ? {
              running: hostRunning,
              listenPort: settings.port,
              captureActive: hostRunning,
              activeTarget,
              locked,
              clients,
              monitors: HOST_MONITORS,
              layout: peers.map(layoutView),
              error: null,
            }
          : null,
      client: role === "client" ? client : null,
      pairing,
      trusted: trusted.map((t) => ({ ...t, online: online.has(t.id) })),
      discovered: role === "client" ? discovered : [],
      features: {
        peers: featurePeers(),
        clipboard: { ...clip, enabled: settings.clipboard.enabled, historyCount: clipHistory.length },
        transfers,
        sharedTexts,
      },
    };
  }

  // Coalesce bursts of changes into one snapshot, like the real backend (25 ms).
  let emitTimer: number | undefined;
  function emit() {
    if (emitTimer !== undefined) return;
    emitTimer = window.setTimeout(() => {
      emitTimer = undefined;
      const snap = build();
      snapshotListeners.forEach((cb) => cb(structuredClone(snap)));
    }, 25);
  }

  function connectClient(host: HostInfo) {
    client = { ...idleClient(), state: "connected", target: host.name, hostId: host.id, hostName: host.name, hostAddress: host.address, rttMs: 1.6 };
    settings = { ...settings, lastHostAddress: host.address };
    discovered = discovered.map((d) => (d.id === host.id ? { ...d, trusted: true } : d));
    trusted = trusted.map((t) => (t.id === host.id ? { ...t, lastSeen: Date.now(), lastAddress: host.address } : t));
    log("info", "net", `Connected to ${host.name} (${host.address})`);
    emit();
  }

  function requestPairingAsClient(host: HostInfo) {
    client = { ...client, state: "pairing", target: host.name, hostId: host.id, hostName: host.name, hostAddress: host.address };
    pairing = [
      {
        requestId: randomId(),
        peerId: host.id,
        peerName: host.name,
        peerAddress: host.address,
        peerRole: "host",
        code: randomCode(),
        needsMyApproval: true,
        expiresAt: Date.now() + 120_000,
      },
    ];
    log("info", "pairing", `Pairing with ${host.name}; waiting for approval`);
    emit();
  }

  function resolveHost(address: string | null, hostId: string | null): HostInfo {
    if (hostId) {
      const found = discovered.find((d) => d.id === hostId);
      return found ? { id: found.id, name: found.name, address: found.address } : fail("That host is no longer available. Try scanning again.");
    }
    const text = (address ?? "").trim();
    if (!text) fail("Enter the host's address or computer name.");
    const known = discovered.find(
      (d) => d.address === text || d.name.toLowerCase() === text.split(":")[0].toLowerCase() || d.address.split(":")[0] === text,
    );
    if (known) return { id: known.id, name: known.name, address: known.address };
    const withPort = text.includes(":") ? text : `${text}:${settings.port}`;
    return { id: randomId(), name: text.split(":")[0].toUpperCase(), address: withPort };
  }

  function addClip(entry: Omit<ClipboardEntry, "id" | "at">) {
    if (settings.clipboard.history) {
      clipHistory = [{ ...entry, id: randomId(), at: Date.now() }, ...clipHistory].slice(0, settings.clipboard.historyLimit);
    }
    clip = {
      ...clip,
      lastSyncAt: Date.now(),
      lastDirection: entry.origin === "received" ? "received" : "sent",
      lastPeer: entry.origin === "received" ? entry.source : otherName,
      lastKind: entry.kind,
      historyRevision: clip.historyRevision + 1,
    };
  }

  function findPeerForFeature(peerId: string | null, cap: "files" | "textShare", what: string): FeaturePeer[] {
    const list = featurePeers();
    if (list.length === 0) fail(role === "client" ? "Connect to a host first." : "No computers are connected.");
    if (peerId === null) {
      if (role === "host" && cap === "files" && list.length > 1) fail("Choose which computer to send the files to.");
      const ok = list.filter((p) => p[cap]);
      if (ok.length === 0) fail(`${what} is turned off for the connected computers.`);
      return ok;
    }
    const p = list.find((x) => x.id === peerId);
    if (!p) fail("That computer is no longer connected.");
    if (!p[cap]) fail(`${what} is turned off for ${p.name}.`);
    return [p];
  }

  function sizeForName(name: string): number {
    const ext = name.split(".").pop()?.toLowerCase() ?? "";
    if (["mp4", "mov", "mkv", "iso"].includes(ext)) return Math.round(rand(300, 2400) * MB);
    if (["zip", "7z", "fig", "psd"].includes(ext)) return Math.round(rand(20, 400) * MB);
    if (["png", "jpg", "jpeg", "pdf"].includes(ext)) return Math.round(rand(0.2, 8) * MB);
    return Math.round(rand(2, 900) * 1024);
  }

  function startTransfer(t: TransferView) {
    transfers = transfers.map((x) => (x.id === t.id ? { ...x, state: "active", bytesPerSec: 0 } : x));
    emit();
  }

  // Transfer progress: ~4 updates per second, like the real backend's throttle.
  setInterval(() => {
    let changed = false;
    transfers = transfers.map((t) => {
      if (t.state !== "active") return t;
      changed = true;
      const target = settings.transfer.speed === "fast" ? 92 * MB : 46 * MB;
      const speed = Math.max(1 * MB, (t.bytesPerSec || target * 0.6) * 0.7 + target * rand(0.8, 1.15) * 0.3);
      const done = Math.min(t.totalBytes, t.doneBytes + speed / 4);
      if (done >= t.totalBytes) {
        log("info", "transfer", `${t.direction === "send" ? "Sent" : "Received"} ${t.files.length} file(s) ${t.direction === "send" ? "to" : "from"} ${t.peerName}`);
        return {
          ...t,
          doneBytes: t.totalBytes,
          bytesPerSec: 0,
          state: "done",
          finishedAt: Date.now(),
          destination: t.direction === "receive" ? settings.transfer.downloadDir || DOWNLOADS : null,
        };
      }
      return { ...t, doneBytes: done, bytesPerSec: speed };
    });
    if (changed) emit();
  }, 250);

  // Background activity: RTT jitter, pairing expiry, discovery freshness, clipboard syncs.
  let tick = 0;
  setInterval(() => {
    tick++;
    let changed = false;
    if (role === "host" && hostRunning) {
      for (const p of peers) if (p.connected) p.rttMs = Math.max(0.4, p.rttMs + rand(-0.4, 0.4));
      counters.captured += Math.floor(rand(50, 900));
      counters.forwarded += Math.floor(rand(10, 300));
      changed = peers.some((p) => p.connected);
    }
    if (role === "client" && client.state === "connected") {
      client = { ...client, rttMs: Math.max(0.4, (client.rttMs ?? 1.5) + rand(-0.4, 0.4)) };
      counters.injected += Math.floor(rand(5, 200));
      changed = true;
    }
    if (tick % 9 === 0 && settings.clipboard.enabled && featurePeers().some((p) => p.clipboard)) {
      const sample = CLIP_SAMPLES[Math.floor(rand(0, CLIP_SAMPLES.length))];
      const allowed = (sample.kind === "text" && settings.clipboard.text) || (sample.kind === "rich" && settings.clipboard.richText) || (sample.kind === "image" && settings.clipboard.images);
      if (allowed && sample.bytes <= settings.clipboard.maxSizeKb * 1024) {
        const received = Math.random() < 0.5;
        addClip({ kind: sample.kind, preview: sample.preview, bytes: sample.bytes, origin: received ? "received" : "sent", source: received ? otherName : "This PC" });
        changed = true;
      }
    }
    const now = Date.now();
    const expired = pairing.filter((p) => p.expiresAt <= now);
    if (expired.length > 0) {
      pairing = pairing.filter((p) => p.expiresAt > now);
      if (role === "client" && client.state === "pairing") {
        client = { ...client, state: "error", lastError: "The pairing request expired before it was approved." };
      }
      log("warn", "pairing", "Pairing request expired");
      changed = true;
    }
    counters.bytesSent += Math.floor(rand(200, 9000));
    counters.bytesReceived += Math.floor(rand(100, 2000));
    if (changed) emit();
  }, 2000);

  setInterval(() => {
    if (logListeners.size === 0) return;
    const samples: [LogLevel, string, string][] = [
      ["info", "net", "Heartbeat ok"],
      ["debug", "input", "Edge crossing: Display 1 left -> SURFACE-LAPTOP"],
      ["info", "discovery", "Discovery probe answered (192.168.1.34)"],
      ["warn", "net", "RTT spike: 18.4 ms"],
    ];
    const [level, target, message] = samples[Math.floor(rand(0, samples.length))];
    log(level, target, message);
  }, 3000);

  const api: Api = {
    async getSnapshot() {
      await sleep(120);
      return structuredClone(build());
    },
    async setRole(next) {
      await sleep(250);
      role = next;
      activeTarget = null;
      locked = false;
      client = idleClient();
      pairing = [];
      log("info", "app", next ? `Role set to ${next}` : "Role cleared");
      emit();
      return structuredClone(build());
    },
    async getSettings() {
      await sleep(60);
      return structuredClone(settings);
    },
    async updateSettings(next) {
      await sleep(120);
      const prev = settings;
      settings = { ...validateSettings(next), lastHostAddress: prev.lastHostAddress };
      if (role === "host" && hostRunning && (prev.port !== settings.port || prev.discoveryEnabled !== settings.discoveryEnabled || prev.deviceName !== settings.deviceName)) {
        log("info", "app", "restarting sharing to apply new network settings");
      }
      if (!settings.transfer.enabled) {
        transfers = transfers.map((t) =>
          t.state === "active" || t.state === "pending" ? { ...t, state: "cancelled", bytesPerSec: 0, finishedAt: Date.now(), canRetry: t.direction === "send" } : t,
        );
      }
      if (!settings.clipboard.history && clipHistory.length > 0) {
        clipHistory = [];
        clip = { ...clip, historyRevision: clip.historyRevision + 1 };
      }
      emit();
      return structuredClone(settings);
    },

    async hostStart() {
      await sleep(300);
      if (role !== "host") fail("This computer isn't set up as the host.");
      hostRunning = true;
      log("info", "net", `Listening on 0.0.0.0:${settings.port}`);
      emit();
    },
    async hostStop() {
      await sleep(200);
      hostRunning = false;
      activeTarget = null;
      log("info", "net", "Sharing stopped");
      emit();
    },
    async hostSetLayout(placements) {
      await sleep(200);
      for (const pl of placements) {
        const p = peers.find((x) => x.id === pl.clientId);
        if (!p) fail(`Unknown device ${pl.clientId}.`);
        else {
          p.x = pl.x;
          p.y = pl.y;
        }
      }
      log("info", "layout", `Saved layout for ${placements.length} device(s)`);
      emit();
    },
    async hostTestSwitch(clientId) {
      await sleep(100);
      const p = peers.find((x) => x.id === clientId);
      if (!p || !p.connected || !hostRunning) fail(`${p?.name ?? "That device"} isn't connected.`);
      activeTarget = clientId;
      emit();
      setTimeout(() => {
        if (activeTarget === clientId) {
          activeTarget = null;
          emit();
        }
      }, 2000);
    },
    async hostDisconnectClient(clientId) {
      await sleep(200);
      const p = peers.find((x) => x.id === clientId);
      if (!p) fail("That device isn't connected.");
      else {
        p.connected = false;
        if (activeTarget === clientId) activeTarget = null;
        log("info", "net", `Disconnected ${p.name}`);
      }
      emit();
    },
    async refreshMonitors() {
      await sleep(200);
      log("info", "input", "Display layout refreshed: 2 displays");
      emit();
    },

    async clientConnect(address, hostId) {
      await sleep(150);
      if (role !== "client") fail("This computer isn't set up as a client.");
      const host = resolveHost(address, hostId);
      client = { ...idleClient(), state: "connecting", target: host.name };
      log("info", "net", `Connecting to ${host.address}`);
      emit();
      setTimeout(() => {
        if (client.state !== "connecting") return;
        if (/^fail/i.test(host.name)) {
          client = {
            ...client,
            state: "error",
            lastError: `Couldn't reach ${host.name}. Make sure Moush is running there and both computers are on the same network.`,
          };
          log("error", "net", `Connect to ${host.address} failed: timed out`);
          emit();
          return;
        }
        if (trusted.some((t) => t.id === host.id)) connectClient(host);
        else requestPairingAsClient(host);
      }, 900);
    },
    async clientDisconnect() {
      await sleep(150);
      client = idleClient();
      pairing = [];
      log("info", "net", "Disconnected from host");
      emit();
    },
    async discoverHosts() {
      await sleep(80);
      setTimeout(() => {
        discovered = discovered.map((d) => ({ ...d, lastSeen: Date.now() }));
        emit();
      }, 600);
    },

    async respondPairing(requestId, accept) {
      await sleep(200);
      const req = pairing.find((p) => p.requestId === requestId);
      if (!req) fail("This pairing request is no longer active.");
      else if (!accept) {
        pairing = pairing.filter((p) => p.requestId !== requestId);
        if (role === "client" && client.state === "pairing") {
          client = { ...client, state: "error", lastError: "Pairing was cancelled on this computer." };
        }
        log("info", "pairing", `Pairing with ${req.peerName} rejected`);
        emit();
      } else if (role === "host") {
        pairing = pairing.filter((p) => p.requestId !== requestId);
        trusted = [
          ...trusted.filter((t) => t.id !== req.peerId),
          {
            id: req.peerId,
            name: req.peerName,
            role: req.peerRole,
            fingerprint: fingerprintOf(req.peerId),
            lastAddress: req.peerAddress,
            pairedAt: Date.now(),
            lastSeen: Date.now(),
            policy: policy(),
          },
        ];
        if (!peers.some((p) => p.id === req.peerId)) {
          peers.push({
            id: req.peerId,
            name: req.peerName,
            address: req.peerAddress,
            monitors: LIVINGROOM_MONITORS,
            x: 1000,
            y: 1440,
            connected: true,
            rttMs: 3.2,
            connectedAt: Date.now(),
          });
        }
        log("info", "pairing", `Paired with ${req.peerName}`);
        emit();
      } else {
        pairing = pairing.map((p) => (p.requestId === requestId ? { ...p, needsMyApproval: false } : p));
        emit();
        setTimeout(() => {
          if (!pairing.some((p) => p.requestId === requestId)) return;
          pairing = pairing.filter((p) => p.requestId !== requestId);
          trusted = [
            ...trusted.filter((t) => t.id !== req.peerId),
            {
              id: req.peerId,
              name: req.peerName,
              role: "host",
              fingerprint: fingerprintOf(req.peerId),
              lastAddress: req.peerAddress,
              pairedAt: Date.now(),
              lastSeen: Date.now(),
              policy: policy(),
            },
          ];
          connectClient({ id: req.peerId, name: req.peerName, address: req.peerAddress });
        }, 2000);
      }
    },
    async forgetDevice(deviceId) {
      await sleep(150);
      const t = trusted.find((d) => d.id === deviceId);
      if (!t) fail("That device isn't in the trusted list.");
      trusted = trusted.filter((d) => d.id !== deviceId);
      discovered = discovered.map((d) => (d.id === deviceId ? { ...d, trusted: false } : d));
      log("info", "pairing", `Forgot ${t?.name ?? deviceId}`);
      emit();
    },
    async setDevicePolicy(deviceId, next) {
      await sleep(150);
      if (!trusted.some((t) => t.id === deviceId)) fail("That device isn't paired any more.");
      trusted = trusted.map((t) => (t.id === deviceId ? { ...t, policy: { ...next } } : t));
      log("info", "features", `Permissions changed for ${deviceId}`);
      emit();
    },
    async resetSettings() {
      await sleep(200);
      settings = { ...defaultSettings(settings.deviceName, settings.lastHostAddress) };
      log("info", "config", "Settings restored to defaults");
      emit();
      return structuredClone(settings);
    },

    // ---- clipboard
    async getClipboardHistory() {
      await sleep(80);
      return structuredClone(clipHistory);
    },
    async copyClipboardEntry(id) {
      await sleep(80);
      const e = clipHistory.find((c) => c.id === id);
      if (!e) fail("That clipboard item is no longer in the history.");
      clipHistory = [{ ...e, at: Date.now(), origin: "local", source: "This PC" }, ...clipHistory.filter((c) => c.id !== id)];
      clip = { ...clip, historyRevision: clip.historyRevision + 1 };
      emit();
    },
    async deleteClipboardEntry(id) {
      await sleep(80);
      clipHistory = clipHistory.filter((c) => c.id !== id);
      clip = { ...clip, historyRevision: clip.historyRevision + 1 };
      emit();
    },
    async clearClipboardHistory() {
      await sleep(120);
      clipHistory = [];
      clip = { ...clip, historyRevision: clip.historyRevision + 1 };
      emit();
    },

    // ---- shared text
    async shareText(peerId, text) {
      await sleep(150);
      const value = text.trim();
      if (!value) fail("Type or paste something to share.");
      if (value.length > 65_536) fail("The text is too long to share (64 KB at most).");
      const targets = findPeerForFeature(peerId, "textShare", "Text sharing");
      const isLink = /^https?:\/\/\S+$/i.test(value);
      sharedTexts = [
        ...targets.map(
          (t): SharedTextView => ({ id: randomId(), direction: "sent", peerName: t.name, preview: value.slice(0, 4096), chars: value.length, isLink, at: Date.now() }),
        ),
        ...sharedTexts,
      ].slice(0, 20);
      log("info", "share", `Shared ${value.length} characters with ${targets.map((t) => t.name).join(", ")}`);
      emit();
    },
    async copySharedText(id) {
      await sleep(60);
      if (!sharedTexts.some((t) => t.id === id)) fail("That item is no longer available.");
    },
    async openSharedLink(id) {
      await sleep(60);
      const t = sharedTexts.find((x) => x.id === id);
      if (!t) fail("That item is no longer available.");
      else if (!t.isLink) fail("Only web links can be opened.");
      else log("info", "share", "Opened link in the default browser (preview: nothing opens)");
    },
    async dismissSharedText(id) {
      await sleep(60);
      sharedTexts = sharedTexts.filter((t) => t.id !== id);
      emit();
    },

    // ---- file transfer
    async sendFiles(peerId, paths) {
      await sleep(150);
      if (!settings.transfer.enabled) fail("File transfer is turned off in Features.");
      if (paths.length === 0) fail("Choose at least one file.");
      const [target] = findPeerForFeature(peerId, "files", "File transfer");
      const files: TransferFile[] = paths.map((p) => {
        const name = p.split(/[\\/]/).pop() || p;
        return { name, size: sizeForName(name) };
      });
      const t = transfer({ direction: "send", state: "pending", files, peerId: target.id, peerName: target.name });
      transfers = [t, ...transfers].slice(0, 50);
      log("info", "transfer", `Offering ${files.length} file(s) to ${target.name}`);
      emit();
      setTimeout(() => {
        if (transfers.find((x) => x.id === t.id)?.state === "pending") startTransfer(t);
      }, 1200);
      return t.id;
    },
    async pickFiles() {
      await sleep(400);
      return ["C:\\Users\\you\\Documents\\Budget 2027.xlsx", "C:\\Users\\you\\Pictures\\whiteboard.jpg"];
    },
    async pickFolder() {
      await sleep(400);
      return "D:\\Inbox";
    },
    async respondTransfer(id, accept) {
      await sleep(120);
      const t = transfers.find((x) => x.id === id);
      if (!t || t.state !== "pending" || t.direction !== "receive") fail("This transfer is no longer waiting for an answer.");
      if (accept) startTransfer(t);
      else {
        transfers = transfers.map((x) => (x.id === id ? { ...x, state: "rejected", finishedAt: Date.now() } : x));
        emit();
      }
    },
    async cancelTransfer(id) {
      await sleep(120);
      const t = transfers.find((x) => x.id === id);
      if (!t || (t.state !== "active" && t.state !== "pending")) fail("This transfer has already finished.");
      transfers = transfers.map((x) =>
        x.id === id ? { ...x, state: "cancelled", bytesPerSec: 0, finishedAt: Date.now(), canRetry: x.direction === "send" } : x,
      );
      emit();
    },
    async retryTransfer(id) {
      await sleep(150);
      const t = transfers.find((x) => x.id === id);
      if (!t || !t.canRetry) fail("This transfer can't be retried.");
      if (!featurePeers().some((p) => p.id === t.peerId && p.files)) fail(`${t.peerName} isn't connected or doesn't accept files right now.`);
      const again: TransferView = { ...t, state: "pending", doneBytes: 0, bytesPerSec: 0, error: null, canRetry: false, startedAt: Date.now(), finishedAt: null };
      transfers = [again, ...transfers.filter((x) => x.id !== id)];
      emit();
      setTimeout(() => startTransfer(again), 1000);
    },
    async clearFinishedTransfers() {
      await sleep(80);
      transfers = transfers.filter((t) => t.state === "active" || t.state === "pending");
      emit();
    },
    async openTransferLocation(id) {
      await sleep(60);
      const t = transfers.find((x) => x.id === id);
      if (!t?.destination) fail("The files aren't on this computer.");
      log("info", "transfer", `Showing ${t?.destination} in Explorer (preview: nothing opens)`);
    },
    async defaultDownloadDir() {
      await sleep(40);
      return DOWNLOADS;
    },

    async getDiagnostics(): Promise<Diagnostics> {
      await sleep(40);
      const isHost = role === "host";
      return {
        uptimeSecs: (Date.now() - startedAt) / 1000 + 3600 * 2.4,
        process: { cpuPercent: rand(0.2, 2.4), memoryMb: rand(38, 42), threads: 23 },
        input: {
          hookInstalled: isHost && hostRunning,
          eventsCaptured: isHost ? counters.captured : 0,
          eventsForwarded: isHost ? counters.forwarded : 0,
          eventsInjected: isHost ? 0 : counters.injected,
          injectFailures: isHost ? 0 : 2,
          hookLatencyUs: isHost ? latency(6, counters.captured) : latency(0, 0),
          sendLatencyUs: isHost ? latency(180, counters.forwarded) : latency(0, 0),
          injectLatencyUs: isHost ? latency(0, 0) : latency(240, counters.injected),
        },
        network: {
          rttMs: latency(1.6, 1240),
          bytesSent: counters.bytesSent,
          bytesReceived: counters.bytesReceived,
          framesSent: Math.floor(counters.bytesSent / 38),
          framesReceived: Math.floor(counters.bytesReceived / 30),
          reconnects: 1,
          lastReconnectMs: 412,
          interfaces: [
            { name: "Ethernet", address: "192.168.1.20" },
            { name: "vEthernet (WSL)", address: "172.29.144.1" },
          ],
        },
        firewall,
        elevated: false,
        logPath: "C:\\Users\\you\\AppData\\Local\\Moush\\logs",
      };
    },
    async getLogs(limit) {
      await sleep(80);
      return structuredClone(logs.slice(-limit));
    },
    async runLatencyTest() {
      if (latencyTestRunning) fail("A latency test is already running.");
      const peerName = connectedPeers()[0]?.name;
      if (!peerName) fail("Connect a device first, then run the latency test.");
      latencyTestRunning = true;
      await sleep(1500);
      latencyTestRunning = false;
      return { peerName: peerName ?? "", sent: 200, received: 199, rttMs: latency(1.4, 199) };
    },
    async openFirewallSetup() {
      await sleep(600);
      firewall = "allowed";
      log("info", "firewall", "Firewall rule added for Moush (Private networks)");
    },
    async openLogFolder() {
      await sleep(50);
      log("info", "app", "Opened log folder (preview: nothing to open in the browser)");
    },
    async quitApp() {
      await sleep(50);
      log("info", "app", "Quit requested (preview: the browser tab stays open)");
    },
  };

  const mock: MockControls = {
    togglePairing() {
      if (pairing.length > 0) {
        pairing = [];
        if (role === "client" && client.state === "pairing") client = idleClient();
        emit();
        return;
      }
      if (role === "host") {
        pairing = [
          {
            requestId: randomId(),
            peerId: LIVINGROOM_ID,
            peerName: "LIVINGROOM-PC",
            peerAddress: "192.168.1.57:51234",
            peerRole: "client",
            code: randomCode(),
            needsMyApproval: true,
            expiresAt: Date.now() + 120_000,
          },
        ];
        log("info", "pairing", "LIVINGROOM-PC wants to connect");
        emit();
      } else if (role === "client") {
        requestPairingAsClient({ id: OFFICE_ID, name: "OFFICE-DESKTOP", address: "192.168.1.41:24800" });
      }
    },
    simulateConnectionDrop() {
      if (role === "host") {
        const laptop = peers.find((p) => p.id === LAPTOP_ID);
        if (!laptop) return;
        laptop.connected = !laptop.connected;
        if (laptop.connected) laptop.connectedAt = Date.now();
        if (!laptop.connected && activeTarget === LAPTOP_ID) activeTarget = null;
        log(laptop.connected ? "info" : "warn", "net", `${laptop.name} ${laptop.connected ? "connected" : "disconnected"}`);
        emit();
        return;
      }
      if (role === "client" && client.state === "connected") {
        const host: HostInfo = { id: client.hostId ?? STUDIO_ID, name: client.hostName ?? "STUDIO-PC", address: client.hostAddress ?? "192.168.1.20:24800" };
        client = { ...client, state: "reconnecting", active: false, rttMs: null, retryInMs: 5000, lastError: "The host stopped responding." };
        log("error", "net", `Lost connection to ${host.name}; retrying in 5 s`);
        emit();
        setTimeout(() => {
          if (client.state !== "reconnecting") return;
          client = { ...client, state: "connecting", retryInMs: null };
          emit();
          setTimeout(() => {
            if (client.state === "connecting") connectClient(host);
          }, 900);
        }, 5000);
      }
    },
    simulateIncomingTransfer() {
      const from = featurePeers().find((p) => p.files);
      if (!from) return;
      const t = transfer({
        direction: "receive",
        state: settings.transfer.askBeforeReceiving ? "pending" : "active",
        files: [{ name: "Site-photos-batch-3.zip", size: 640 * MB }],
        peerId: from.id,
        peerName: from.name,
      });
      transfers = [t, ...transfers].slice(0, 50);
      log("info", "transfer", `${from.name} wants to send 1 file`);
      emit();
    },
    simulateSharedText() {
      const from = featurePeers().find((p) => p.textShare);
      if (!from) return;
      const link = Math.random() < 0.5;
      const text = link ? "https://www.figma.com/design/k2Lq/Moush-onboarding?node-id=12-4" : "Remember: rotate the API key before Friday's release.";
      sharedTexts = [{ id: randomId(), direction: "received" as const, peerName: from.name, preview: text, chars: text.length, isLink: link, at: Date.now() }, ...sharedTexts].slice(0, 20);
      emit();
    },
    simulateClipboardSync() {
      const sample = CLIP_SAMPLES[Math.floor(rand(0, CLIP_SAMPLES.length))];
      addClip({ kind: sample.kind, preview: sample.preview, bytes: sample.bytes, origin: "received", source: otherName });
      emit();
    },
  };

  return {
    api,
    onSnapshot: (cb) => {
      snapshotListeners.add(cb);
      return Promise.resolve(() => {
        snapshotListeners.delete(cb);
      });
    },
    onLog: (cb) => {
      logListeners.add(cb);
      return Promise.resolve(() => {
        logListeners.delete(cb);
      });
    },
    mock,
  };
}
