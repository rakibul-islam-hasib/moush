//! Serialisable types sent to the UI. Mirrors `src/api/types.ts`; keep them in sync.

use serde::Serialize;

use moush_engine::core::features::DevicePolicy;
use moush_engine::core::stats::LatencySummary;
use moush_engine::core::trust::TrustRole;

pub use moush_engine::core::pairing::PairingRequest;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub dpi: u32,
    pub scale: f64,
    pub primary: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedClient {
    pub id: String,
    pub name: String,
    pub address: String,
    pub active: bool,
    pub rtt_ms: Option<f64>,
    pub monitors: Vec<MonitorInfo>,
    pub connected_at: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientLayoutView {
    pub client_id: String,
    pub name: String,
    pub online: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub resolution_width: i32,
    pub resolution_height: i32,
    pub scale: f64,
    pub monitors: Vec<MonitorInfo>,
    pub adjacent: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    pub running: bool,
    pub listen_port: u16,
    pub capture_active: bool,
    pub active_target: Option<String>,
    pub locked: bool,
    pub clients: Vec<ConnectedClient>,
    pub monitors: Vec<MonitorInfo>,
    pub layout: Vec<ClientLayoutView>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub last_seen: u64,
    pub trusted: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSnapshot {
    pub state: &'static str,
    pub target: Option<String>,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
    pub host_address: Option<String>,
    pub active: bool,
    pub rtt_ms: Option<f64>,
    pub last_error: Option<String>,
    pub retry_in_ms: Option<u64>,
    pub monitors: Vec<MonitorInfo>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
    pub role: TrustRole,
    pub fingerprint: String,
    pub last_address: Option<String>,
    pub paired_at: u64,
    pub last_seen: Option<u64>,
    pub policy: DevicePolicy,
    pub online: bool,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os_build: u32,
    pub windows11: bool,
    pub mica: bool,
    pub elevated: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub role: Option<&'static str>,
    pub device_name: String,
    pub device_id: String,
    pub fingerprint: String,
    pub version: &'static str,
    pub platform: PlatformInfo,
    pub host: Option<HostSnapshot>,
    pub client: Option<ClientSnapshot>,
    pub pairing: Vec<PairingRequest>,
    pub trusted: Vec<TrustedDevice>,
    pub discovered: Vec<DiscoveredHost>,
    pub features: FeaturesSnapshot,
}

// ---------------------------------------------------------------- features

/// A connected peer and the features both sides currently allow with it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeaturePeer {
    pub id: String,
    pub name: String,
    pub clipboard: bool,
    pub files: bool,
    pub text_share: bool,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardStatus {
    /// Synchronization is enabled in settings.
    pub enabled: bool,
    /// Unix ms of the last clipboard update sent or received.
    pub last_sync_at: Option<u64>,
    /// "sent" or "received".
    pub last_direction: Option<&'static str>,
    pub last_peer: Option<String>,
    /// "text", "rich" or "image".
    pub last_kind: Option<&'static str>,
    pub history_count: u32,
    /// Increases whenever the history changes; the UI refetches it then.
    pub history_revision: u64,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferFile {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: String,
    /// "send" or "receive".
    pub direction: &'static str,
    pub peer_id: String,
    pub peer_name: String,
    pub files: Vec<TransferFile>,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub bytes_per_sec: f64,
    /// "pending" (waiting for approval), "active", "done", "failed", "cancelled", "rejected".
    pub state: &'static str,
    pub error: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    /// Receive: folder the files were saved to.
    pub destination: Option<String>,
    pub can_retry: bool,
}

/// Text, a link or a snippet shared manually between computers.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedTextView {
    pub id: String,
    /// "sent" or "received".
    pub direction: &'static str,
    pub peer_name: String,
    /// At most 4 KB; the full text is copied with `copy_shared_text`.
    pub preview: String,
    pub chars: u32,
    /// The whole text is a single http(s) URL.
    pub is_link: bool,
    pub at: u64,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesSnapshot {
    pub peers: Vec<FeaturePeer>,
    pub clipboard: ClipboardStatus,
    /// Active, pending and recent transfers, newest first (at most 50).
    pub transfers: Vec<TransferView>,
    /// Recent shared texts, newest first (at most 20, in memory only).
    pub shared_texts: Vec<SharedTextView>,
}

/// An entry of the clipboard history (`get_clipboard_history`).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEntry {
    pub id: String,
    /// "text", "rich" or "image".
    pub kind: &'static str,
    /// First 300 characters of the text, or e.g. "Image 1920 x 1080".
    pub preview: String,
    pub bytes: u64,
    /// "local", "sent" or "received".
    pub origin: &'static str,
    /// Device the clip came from ("This PC" for local copies).
    pub source: String,
    pub at: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStats {
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub threads: u32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputStats {
    pub hook_installed: bool,
    pub events_captured: u64,
    pub events_forwarded: u64,
    pub events_injected: u64,
    pub inject_failures: u64,
    pub hook_latency_us: LatencySummary,
    pub send_latency_us: LatencySummary,
    pub inject_latency_us: LatencySummary,
}

#[derive(Clone, Serialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub address: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStats {
    pub rtt_ms: LatencySummary,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub frames_sent: u64,
    pub frames_received: u64,
    pub reconnects: u64,
    pub last_reconnect_ms: Option<u64>,
    pub interfaces: Vec<InterfaceInfo>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub uptime_secs: u64,
    pub process: ProcessStats,
    pub input: InputStats,
    pub network: NetworkStats,
    pub firewall: &'static str,
    pub elevated: bool,
    pub log_path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyTestResult {
    pub peer_name: String,
    pub sent: u32,
    pub received: u32,
    pub rtt_ms: LatencySummary,
}
