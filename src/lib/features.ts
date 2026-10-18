// UI helpers for feature data (peers, transfers, clipboard): availability reasons,
// labels and formatting. Pure functions, no React.

import type { AppSnapshot, ClipKind, FeaturePeer, Settings, TransferState, TransferView, TrustedDevice } from "../api";
import { formatBytes, plural } from "./format";
import type { Tone } from "./status";

export type FeatureCap = "clipboard" | "files" | "textShare";

const FEATURE_NAME: Record<FeatureCap, string> = { clipboard: "Clipboard sync", files: "File transfer", textShare: "Text sharing" };
const SETTING_OFF: Partial<Record<FeatureCap, (s: Settings) => boolean>> = {
  clipboard: (s) => !s.clipboard.enabled,
  files: (s) => !s.transfer.enabled,
};

/**
 * Why a connected peer can't use a feature, or null when it can. The order mirrors how
 * capabilities are combined: this computer's setting, this computer's per-device policy,
 * then the other computer's own settings (which we only see as the effective result).
 */
export function unavailableReason(peer: FeaturePeer, cap: FeatureCap, settings: Settings, trusted: readonly TrustedDevice[]): string | null {
  if (peer[cap]) return null;
  if (SETTING_OFF[cap]?.(settings)) return `${FEATURE_NAME[cap]} is turned off on this computer.`;
  const policy = trusted.find((t) => t.id === peer.id)?.policy;
  const key = cap === "textShare" ? "textShare" : cap;
  if (policy && !policy[key]) return `You turned off ${FEATURE_NAME[cap].toLowerCase()} for ${peer.name} in Devices.`;
  return `${peer.name} doesn't accept it — it's turned off there, or that computer runs an older Moush.`;
}

/** Connected peers split by availability of a feature. */
export function peersFor(snapshot: AppSnapshot, settings: Settings, cap: FeatureCap) {
  const available: FeaturePeer[] = [];
  const unavailable: { peer: FeaturePeer; reason: string }[] = [];
  for (const p of snapshot.features.peers) {
    const reason = unavailableReason(p, cap, settings, snapshot.trusted);
    if (reason) unavailable.push({ peer: p, reason });
    else available.push(p);
  }
  return { available, unavailable };
}

export const isFinished = (s: TransferState) => s === "done" || s === "failed" || s === "cancelled" || s === "rejected";
export const isLive = (s: TransferState) => s === "active" || s === "pending";

export function transferTitle(t: TransferView): string {
  const first = t.files[0]?.name ?? "Files";
  return t.files.length > 1 ? `${first} and ${plural(t.files.length - 1, "more file")}` : first;
}

export function formatSpeed(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

export function formatEta(t: TransferView): string | null {
  if (t.state !== "active" || t.bytesPerSec <= 0) return null;
  const secs = Math.max(0, (t.totalBytes - t.doneBytes) / t.bytesPerSec);
  if (secs < 1) return "almost done";
  if (secs < 60) return `${Math.ceil(secs)} s left`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m} min ${Math.round(secs % 60)} s left`;
  return `${Math.floor(m / 60)} h ${m % 60} min left`;
}

export function transferPercent(t: TransferView): number {
  if (t.totalBytes <= 0) return t.state === "done" ? 100 : 0;
  return Math.min(100, Math.max(0, (t.doneBytes / t.totalBytes) * 100));
}

export interface StateLabel {
  label: string;
  tone: Tone;
}

export function transferState(t: TransferView): StateLabel {
  switch (t.state) {
    case "pending":
      return t.direction === "receive" ? { label: "Waiting for you", tone: "caution" } : { label: `Waiting for ${t.peerName}`, tone: "accent" };
    case "active":
      return { label: t.direction === "send" ? "Sending" : "Receiving", tone: "accent" };
    case "done":
      return { label: t.direction === "send" ? "Sent" : "Received", tone: "success" };
    case "failed":
      return { label: "Failed", tone: "critical" };
    case "cancelled":
      return { label: "Cancelled", tone: "neutral" };
    case "rejected":
      return { label: "Declined", tone: "neutral" };
  }
}

export const CLIP_KIND_LABEL: Record<ClipKind, string> = { text: "Text", rich: "Formatted text", image: "Image" };
