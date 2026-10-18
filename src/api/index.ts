// Single entry point for talking to the backend. Inside Tauri this is the real
// command layer; in a plain browser (`npm run dev`) an in-memory mock is loaded
// lazily so the UI can be previewed without the Rust side.

import { api, inTauri, onLog, onSnapshot } from "./backend";

/** Extra knobs offered only by the browser preview mock. */
export interface MockControls {
  /** Adds a pending pairing request, or clears the current one. */
  togglePairing: () => void;
  /** Host: a client drops off / comes back. Client: the connection is lost and retried. */
  simulateConnectionDrop: () => void;
}

export interface Backend {
  api: typeof api;
  onSnapshot: typeof onSnapshot;
  onLog: typeof onLog;
  /** Non-null only when running against the browser preview mock. */
  mock: MockControls | null;
}

export const backend: Backend = inTauri
  ? { api, onSnapshot, onLog, mock: null }
  : (await import("./mock")).createMockBackend();

export type * from "./types";
