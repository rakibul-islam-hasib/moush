//! Serialisable types sent to the UI. Mirrors `src/api/types.ts`; keep them in sync.

use serde::Serialize;

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
