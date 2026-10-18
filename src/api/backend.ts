// Thin typed wrappers around the Tauri commands exposed by src-tauri/src/commands.rs.
// All commands reject with a human-readable string on failure.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSnapshot,
  Diagnostics,
  LatencyTestResult,
  LayoutPlacement,
  LogEntry,
  Role,
  Settings,
} from "./types";

export const api = {
  getSnapshot: () => invoke<AppSnapshot>("get_snapshot"),
  /** Switch role (null returns to onboarding). Stops the current role's service. */
  setRole: (role: Role | null) => invoke<AppSnapshot>("set_role", { role }),

  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<Settings>("update_settings", { settings }),

  // Host
  hostStart: () => invoke<void>("host_start"),
  hostStop: () => invoke<void>("host_stop"),
  hostSetLayout: (placements: LayoutPlacement[]) => invoke<void>("host_set_layout", { placements }),
  /** Moves the shared cursor to the centre of the given client to test the layout. */
  hostTestSwitch: (clientId: string) => invoke<void>("host_test_switch", { clientId }),
  hostDisconnectClient: (clientId: string) => invoke<void>("host_disconnect_client", { clientId }),
  refreshMonitors: () => invoke<void>("refresh_monitors"),

  // Client
  /** address: "ip", "ip:port" or a host name. hostId: connect to a discovered host. */
  clientConnect: (address: string | null, hostId: string | null) =>
    invoke<void>("client_connect", { address, hostId }),
  clientDisconnect: () => invoke<void>("client_disconnect"),
  /** Sends a LAN discovery probe; results arrive through the snapshot event. */
  discoverHosts: () => invoke<void>("discover_hosts"),

  // Pairing & trust
  respondPairing: (requestId: string, accept: boolean) =>
    invoke<void>("respond_pairing", { requestId, accept }),
  forgetDevice: (deviceId: string) => invoke<void>("forget_device", { deviceId }),

  // Diagnostics
  getDiagnostics: () => invoke<Diagnostics>("get_diagnostics"),
  getLogs: (limit: number) => invoke<LogEntry[]>("get_logs", { limit }),
  runLatencyTest: () => invoke<LatencyTestResult>("run_latency_test"),
  openFirewallSetup: () => invoke<void>("open_firewall_setup"),
  openLogFolder: () => invoke<void>("open_log_folder"),
  quitApp: () => invoke<void>("quit_app"),
};

/** Fired whenever any part of AppSnapshot changes. */
export function onSnapshot(cb: (s: AppSnapshot) => void): Promise<UnlistenFn> {
  return listen<AppSnapshot>("snapshot", (e) => cb(e.payload));
}

/** Fired for every new log line (info and above). */
export function onLog(cb: (e: LogEntry) => void): Promise<UnlistenFn> {
  return listen<LogEntry>("log", (e) => cb(e.payload));
}

/** True when running inside the Tauri webview (false in a plain browser during UI work). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
