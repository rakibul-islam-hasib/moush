// Contract between the React UI and the Rust backend.
// Every type here mirrors a serde struct in src-tauri/src/dto.rs (camelCase on the wire).
// Keep the two files in sync.

export type Role = "host" | "client";

/** A physical monitor. Coordinates are physical pixels in that computer's virtual desktop. */
export interface MonitorInfo {
  id: string; // Windows device name, e.g. "\\\\.\\DISPLAY1"
  name: string; // Friendly name, e.g. "Display 1"
  x: number;
  y: number;
  width: number;
  height: number;
  dpi: number; // 96 = 100 %
  scale: number; // dpi / 96, e.g. 1.25
  primary: boolean;
}

export interface LatencyStats {
  count: number;
  mean: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
}

// ---------------------------------------------------------------- host

export interface ConnectedClient {
  id: string; // device id (16 hex chars, derived from the device's public key)
  name: string;
  address: string; // "ip:port"
  active: boolean; // currently receiving the shared mouse/keyboard
  rttMs: number | null; // smoothed round-trip time
  monitors: MonitorInfo[]; // in the client's own coordinates
  connectedAt: number; // unix ms
}

/** A client rectangle in the host layout. Layout units = host physical pixels. */
export interface ClientLayoutView {
  clientId: string;
  name: string;
  online: boolean;
  x: number; // top-left, layout units
  y: number;
  width: number; // layout units (client desktop normalised to the host's DPI)
  height: number;
  resolutionWidth: number; // client desktop size in its own physical pixels
  resolutionHeight: number;
  scale: number; // client primary monitor scale
  monitors: MonitorInfo[]; // client coordinates
  adjacent: boolean; // true when at least one edge touches a host monitor (cursor can reach it)
}

export interface HostSnapshot {
  running: boolean; // listening + capturing
  listenPort: number;
  captureActive: boolean; // low-level hooks installed
  activeTarget: string | null; // null = cursor on this computer, else client id
  locked: boolean; // Scroll Lock: cursor locked to the current computer
  clients: ConnectedClient[];
  monitors: MonitorInfo[]; // host monitors (layout coordinates)
  layout: ClientLayoutView[]; // connected + remembered clients
  error: string | null;
}

// ---------------------------------------------------------------- client

export type ClientState =
  | "idle" // not connecting
  | "connecting"
  | "pairing" // waiting for pairing approval on one or both sides
  | "connected"
  | "reconnecting" // connection lost, retrying automatically
  | "error"; // stopped with an error (e.g. pairing rejected)

export interface DiscoveredHost {
  id: string;
  name: string;
  address: string; // "ip:port"
  lastSeen: number; // unix ms
  trusted: boolean;
}

export interface ClientSnapshot {
  state: ClientState;
  target: string | null; // address or host name being connected to
  hostId: string | null;
  hostName: string | null;
  hostAddress: string | null;
  active: boolean; // shared cursor is currently on this computer
  rttMs: number | null;
  lastError: string | null;
  retryInMs: number | null; // when reconnecting
  monitors: MonitorInfo[];
}

// ---------------------------------------------------------------- shared

/** A pending pairing between this device and a peer. Shown until resolved or expired. */
export interface PairingRequest {
  requestId: string;
  peerId: string;
  peerName: string;
  peerAddress: string;
  peerRole: Role;
  code: string; // 6-digit verification code; identical on both computers
  needsMyApproval: boolean; // false = we already trust the peer, waiting on the other side
  expiresAt: number; // unix ms
}

export interface TrustedDevice {
  id: string;
  name: string;
  role: Role; // the role the peer had when paired
  fingerprint: string; // "a1b2-c3d4-e5f6-a7b8"
  lastAddress: string | null;
  pairedAt: number;
  lastSeen: number | null;
  policy: DevicePolicy; // what this device may do with us
  online: boolean; // currently connected
}

/** Per-device permissions, stored with the paired device. */
export interface DevicePolicy {
  clipboard: boolean;
  files: boolean;
  textShare: boolean;
}

export interface PlatformInfo {
  osBuild: number;
  windows11: boolean;
  mica: boolean; // window has the native Mica backdrop; render surfaces semi-transparent
  elevated: boolean; // running as administrator
}

export interface AppSnapshot {
  role: Role | null; // null => show onboarding
  deviceName: string;
  deviceId: string;
  fingerprint: string;
  version: string;
  platform: PlatformInfo;
  host: HostSnapshot | null; // present when role === "host"
  client: ClientSnapshot | null; // present when role === "client"
  pairing: PairingRequest[];
  trusted: TrustedDevice[];
  discovered: DiscoveredHost[]; // hosts found on the LAN (client role)
  features: FeaturesSnapshot;
}

// ---------------------------------------------------------------- features

/** A connected peer and the features BOTH sides currently allow with it. */
export interface FeaturePeer {
  id: string;
  name: string;
  clipboard: boolean;
  files: boolean;
  textShare: boolean;
}

export type ClipKind = "text" | "rich" | "image";

export interface ClipboardStatus {
  enabled: boolean; // synchronization enabled in settings
  lastSyncAt: number | null; // unix ms of the last update sent or received
  lastDirection: "sent" | "received" | null;
  lastPeer: string | null; // device name
  lastKind: ClipKind | null;
  historyCount: number;
  historyRevision: number; // changes whenever the history changes: refetch then
  error: string | null;
}

export interface TransferFile {
  name: string;
  size: number;
}

export type TransferState = "pending" | "active" | "done" | "failed" | "cancelled" | "rejected";

export interface TransferView {
  id: string;
  direction: "send" | "receive";
  peerId: string;
  peerName: string;
  files: TransferFile[];
  totalBytes: number;
  doneBytes: number;
  bytesPerSec: number;
  /** "pending": receive = waiting for this user's approval; send = waiting for the peer. */
  state: TransferState;
  error: string | null;
  startedAt: number;
  finishedAt: number | null;
  destination: string | null; // receive: folder the files were saved to
  canRetry: boolean;
}

/** Text, a link or a snippet shared manually between computers. */
export interface SharedTextView {
  id: string;
  direction: "sent" | "received";
  peerName: string;
  preview: string; // at most 4 KB
  chars: number;
  isLink: boolean; // the whole text is one http(s) URL
  at: number;
}

export interface FeaturesSnapshot {
  peers: FeaturePeer[];
  clipboard: ClipboardStatus;
  transfers: TransferView[]; // newest first, at most 50
  sharedTexts: SharedTextView[]; // newest first, at most 20, memory only
}

export interface ClipboardEntry {
  id: string;
  kind: ClipKind;
  preview: string; // first 300 chars, or e.g. "Image 1920 x 1080"
  bytes: number;
  origin: "local" | "sent" | "received";
  source: string; // device name, "This PC" for local copies
  at: number;
}

export type Theme = "system" | "light" | "dark";

export interface ClipboardSettings {
  enabled: boolean; // master switch for clipboard synchronization
  text: boolean; // plain Unicode text
  richText: boolean; // HTML + RTF alongside the text
  images: boolean;
  maxSizeKb: number; // 16..65536; larger contents are not synchronized
  skipSensitive: boolean; // skip clips marked sensitive by password managers
  history: boolean; // keep a history in memory (never on disk)
  historyLimit: number; // 1..200
}

export interface KeyboardSettings {
  forwardKeyboard: boolean; // off = only the mouse moves; typing stays on the host
  forwardSystemShortcuts: boolean; // Alt+Tab, Win, Alt+F4, Ctrl+Esc go to the active computer
  localShortcuts: string[]; // shortcuts that always stay on the host, e.g. "Ctrl+Shift+Esc" (max 16)
  switchHotkeys: boolean; // Ctrl+Alt+Shift+Arrow jumps to the neighbouring computer; +Home returns
  focusOnEnter: boolean; // client focuses the window under the cursor on entry
  edgeSwitchModifier: "none" | "ctrl" | "shift" | "alt"; // edge switching only while held
}

export interface TransferSettings {
  enabled: boolean;
  downloadDir: string; // "" = Downloads\Moush
  askBeforeReceiving: boolean;
  overwrite: "rename" | "overwrite" | "skip";
  speed: "balanced" | "fast"; // balanced keeps input latency lowest
  openFolderWhenDone: boolean; // never opens or runs the files themselves
}

export interface NotificationSettings {
  enabled: boolean;
  connections: boolean;
  transfers: boolean;
  sharedText: boolean;
  clipboard: boolean;
}

export interface AppearanceSettings {
  transparency: boolean; // Mica backdrop on Windows 11
  motion: "system" | "reduce" | "full";
}

export interface Settings {
  deviceName: string;
  theme: Theme;
  launchAtStartup: boolean;
  startMinimized: boolean;
  closeToTray: boolean;
  autoStart: boolean; // host: start sharing on launch; client: reconnect to last host on launch
  port: number; // TCP port the host listens on (default 24800)
  discoveryEnabled: boolean; // host: answer LAN discovery; client: scan the LAN
  switchDelayMs: number; // 0..1000, time the cursor must rest on an edge before switching
  cornerDeadZonePx: number; // 0..200, no switching this close to a monitor corner
  clientPointerSpeed: number; // 0.25..4.0, multiplier for pointer speed on clients
  scrollLockToggle: boolean; // Scroll Lock locks the cursor to the current computer
  lastHostAddress: string | null; // client: last successfully used host address
  clipboard: ClipboardSettings;
  keyboard: KeyboardSettings;
  transfer: TransferSettings;
  notifications: NotificationSettings;
  appearance: AppearanceSettings;
}

export interface ProcessStats {
  cpuPercent: number; // averaged since the previous diagnostics call
  memoryMb: number; // working set
  threads: number;
}

export interface InputStats {
  hookInstalled: boolean;
  eventsCaptured: number; // host: events seen by the hooks
  eventsForwarded: number; // host: events sent to clients
  eventsInjected: number; // client: events injected via SendInput
  injectFailures: number;
  hookLatencyUs: LatencyStats; // host: time spent inside the hook callback
  sendLatencyUs: LatencyStats; // host: hook -> encrypted write on the socket
  injectLatencyUs: LatencyStats; // client: frame received -> SendInput returned
}

export interface NetworkStats {
  rttMs: LatencyStats;
  bytesSent: number;
  bytesReceived: number;
  framesSent: number;
  framesReceived: number;
  reconnects: number;
  lastReconnectMs: number | null; // duration of the last successful reconnect
  interfaces: { name: string; address: string }[];
}

export type FirewallState = "allowed" | "missing" | "unknown";

export interface Diagnostics {
  uptimeSecs: number;
  process: ProcessStats;
  input: InputStats;
  network: NetworkStats;
  firewall: FirewallState;
  elevated: boolean;
  logPath: string;
}

export type LogLevel = "error" | "warn" | "info" | "debug";

export interface LogEntry {
  ts: number; // unix ms
  level: LogLevel;
  target: string;
  message: string;
}

export interface LatencyTestResult {
  peerName: string;
  sent: number;
  received: number;
  rttMs: LatencyStats;
}

export interface LayoutPlacement {
  clientId: string;
  x: number;
  y: number;
}
