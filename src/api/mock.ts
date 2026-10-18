// In-memory stand-in for the Rust backend, used when the UI runs in a plain
// browser. It implements the same surface as backend.ts with realistic data.
// Append ?mockRole=host|client|none to the URL to choose the starting role.

import type { api as realApi } from "./backend";
import type { Backend, MockControls } from "./index";
import type {
  AppSnapshot,
  ClientLayoutView,
  ClientSnapshot,
  ConnectedClient,
  Diagnostics,
  DiscoveredHost,
  FirewallState,
  LatencyStats,
  LogEntry,
  LogLevel,
  MonitorInfo,
  PairingRequest,
  Role,
  Settings,
  TrustedDevice,
} from "./types";
import { boundsOf, touchesAny } from "../lib/layoutGeometry";

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

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
/** Mirrors the real backend, which rejects with a plain string. */
function fail(message: string): never {
  throw message;
}
const rand = (min: number, max: number) => min + Math.random() * (max - min);
const randomCode = () => String(Math.floor(rand(0, 1_000_000))).padStart(6, "0");
const randomId = () =>
  Array.from({ length: 16 }, () => Math.floor(Math.random() * 16).toString(16)).join("");

function monitor(
  index: number,
  name: string,
  x: number,
  y: number,
  width: number,
  height: number,
  scale: number,
  primary: boolean,
): MonitorInfo {
  return { id: `\\\\.\\DISPLAY${index}`, name, x, y, width, height, dpi: Math.round(96 * scale), scale, primary };
}

const HOST_MONITORS: MonitorInfo[] = [
  monitor(1, "Display 1", 0, 0, 2560, 1440, 1, true),
  monitor(2, "Display 2", 2560, 0, 2560, 1440, 1, false),
];
const LAPTOP_MONITORS: MonitorInfo[] = [monitor(1, "Built-in display", 0, 0, 1920, 1200, 1.5, true)];
const LIVINGROOM_MONITORS: MonitorInfo[] = [monitor(1, "LG TV", 0, 0, 1920, 1080, 1, true)];

const LAPTOP_ID = "9f3c2a71b04e8d15";
const LIVINGROOM_ID = "c81f06d2e4a97b33";
const STUDIO_ID = "41d7e9a0c3b25f68";
const OFFICE_ID = "e2b5c7194f0a6d81";

function latency(base: number, count: number): LatencyStats {
  const p50 = base * rand(0.9, 1.1);
  return {
    count,
    mean: p50 * 1.15,
    p50,
    p95: p50 * rand(2, 2.6),
    p99: p50 * rand(3.2, 4.2),
    max: p50 * rand(7, 12),
  };
}

function startingRole(): Role | null {
  const param = new URLSearchParams(window.location.search).get("mockRole");
  if (param === "client") return "client";
  if (param === "none") return null;
  return "host";
}

/** Layout rectangle for a peer: its desktop normalised to the host DPI (100 %). */
function peerSize(p: MockPeer) {
  const b = boundsOf(p.monitors) ?? { x: 0, y: 0, width: 1920, height: 1080 };
  const scale = p.monitors.find((m) => m.primary)?.scale ?? 1;
  return { resW: b.width, resH: b.height, scale, width: b.width / scale, height: b.height / scale };
}

export function createMockBackend(): Backend {
  const startedAt = Date.now();
  let role = startingRole();
  const thisIsLaptop = role === "client";

  let settings: Settings = {
    deviceName: thisIsLaptop ? "SURFACE-LAPTOP" : "STUDIO-PC",
    theme: "system",
    launchAtStartup: true,
    startMinimized: false,
    closeToTray: true,
    autoStart: true,
    port: 24800,
    discoveryEnabled: true,
    switchDelayMs: 150,
    cornerDeadZonePx: 40,
    clientPointerSpeed: 1,
    scrollLockToggle: true,
    lastHostAddress: thisIsLaptop ? "192.168.1.20:24800" : null,
  };

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
  ];

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
  let discovered: DiscoveredHost[] = [
    { id: STUDIO_ID, name: "STUDIO-PC", address: "192.168.1.20:24800", lastSeen: Date.now(), trusted: true },
    { id: OFFICE_ID, name: "OFFICE-DESKTOP", address: "192.168.1.41:24800", lastSeen: Date.now(), trusted: false },
  ];

  // ---- shared
  let pairing: PairingRequest[] = [];
  let trusted: TrustedDevice[] = thisIsLaptop
    ? [
        {
          id: STUDIO_ID,
          name: "STUDIO-PC",
          role: "host",
          fingerprint: "41d7-e9a0-c3b2-5f68",
          lastAddress: "192.168.1.20:24800",
          pairedAt: Date.now() - 20 * 86_400_000,
          lastSeen: Date.now() - 3 * 3_600_000,
        },
      ]
    : [
        {
          id: LAPTOP_ID,
          name: "SURFACE-LAPTOP",
          role: "client",
          fingerprint: "9f3c-2a71-b04e-8d15",
          lastAddress: "192.168.1.34:52110",
          pairedAt: Date.now() - 20 * 86_400_000,
          lastSeen: Date.now(),
        },
      ];
  let firewall: FirewallState = "unknown";
  let latencyTestRunning = false;
  const counters = { captured: 182_340, forwarded: 96_118, injected: 0, bytesSent: 14_820_331, bytesReceived: 2_310_442 };

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
      ["info", "app", "Moush 0.1.0 starting (Windows 11 build 26200)"],
      ["info", "config", "Loaded settings"],
      ["debug", "input", "Monitor layout: 2 displays, virtual desktop 5120x1440"],
      ["info", "net", "Listening on 0.0.0.0:24800"],
      ["info", "discovery", "Answering LAN discovery on UDP 24801"],
      ["warn", "firewall", "Could not verify Windows Firewall rule (access denied)"],
      ["info", "input", "Low-level keyboard and mouse hooks installed"],
      ["info", "net", "Connection from 192.168.1.34:52110"],
      ["info", "pairing", "SURFACE-LAPTOP is trusted (fingerprint 9f3c-2a71-b04e-8d15)"],
      ["info", "net", "SURFACE-LAPTOP connected, rtt 1.9 ms"],
      ["debug", "input", "Edge crossing: Display 1 left -> SURFACE-LAPTOP"],
      ["debug", "input", "Edge crossing: SURFACE-LAPTOP right -> Display 1"],
      ["error", "net", "Write to 192.168.1.34:52110 failed: connection reset; reconnecting"],
      ["info", "net", "SURFACE-LAPTOP reconnected in 412 ms"],
    ];
    const base = Date.now() - seed.length * 37_000;
    seed.forEach(([level, target, message], i) =>
      logs.push({ ts: base + i * 37_000, level, target: `moush::${target}`, message }),
    );
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
    return {
      role,
      deviceName: settings.deviceName,
      deviceId: thisIsLaptop ? LAPTOP_ID : STUDIO_ID,
      fingerprint: thisIsLaptop ? "9f3c-2a71-b04e-8d15" : "41d7-e9a0-c3b2-5f68",
      version: "0.1.0",
      platform: { osBuild: 26200, windows11: true, mica: false, elevated: false },
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
      trusted,
      discovered: role === "client" ? discovered : [],
    };
  }

  function emit() {
    const snap = build();
    snapshotListeners.forEach((cb) => cb(structuredClone(snap)));
  }

  function connectClient(host: HostInfo) {
    client = {
      ...idleClient(),
      state: "connected",
      target: host.name,
      hostId: host.id,
      hostName: host.name,
      hostAddress: host.address,
      rttMs: 1.6,
    };
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

  // Background activity: RTT jitter, pairing expiry, discovery freshness.
  setInterval(() => {
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
      if (!Number.isInteger(next.port) || next.port < 1024 || next.port > 65535) {
        fail("The port must be a whole number between 1024 and 65535.");
      }
      if (!next.deviceName.trim()) fail("The device name can't be empty.");
      const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
      settings = {
        ...next,
        deviceName: next.deviceName.trim(),
        switchDelayMs: clamp(Math.round(next.switchDelayMs), 0, 1000),
        cornerDeadZonePx: clamp(Math.round(next.cornerDeadZonePx), 0, 200),
        clientPointerSpeed: clamp(next.clientPointerSpeed, 0.25, 4),
      };
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
            fingerprint: req.peerId.match(/.{4}/g)?.join("-") ?? req.peerId,
            lastAddress: req.peerAddress,
            pairedAt: Date.now(),
            lastSeen: Date.now(),
          },
        ];
        if (!peers.some((p) => p.id === req.peerId)) {
          peers.push({
            id: req.peerId,
            name: req.peerName,
            address: req.peerAddress,
            monitors: LIVINGROOM_MONITORS,
            x: 5700,
            y: 1750,
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
              fingerprint: req.peerId.match(/.{4}/g)?.join("-") ?? req.peerId,
              lastAddress: req.peerAddress,
              pairedAt: Date.now(),
              lastSeen: Date.now(),
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
      const peerName =
        role === "host"
          ? peers.find((p) => p.connected && hostRunning)?.name
          : client.state === "connected"
            ? client.hostName ?? undefined
            : undefined;
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
        const host: HostInfo = {
          id: client.hostId ?? STUDIO_ID,
          name: client.hostName ?? "STUDIO-PC",
          address: client.hostAddress ?? "192.168.1.20:24800",
        };
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
