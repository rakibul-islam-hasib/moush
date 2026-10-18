import type { AppSnapshot, ClientSnapshot, HostSnapshot } from "../api";
import { plural } from "./format";

export type Tone = "neutral" | "accent" | "success" | "caution" | "critical";

export interface StatusText {
  title: string;
  detail: string;
  tone: Tone;
}

/** Name of the computer that currently has the shared cursor, or null for this computer. */
export function cursorOwnerName(host: HostSnapshot): string | null {
  if (!host.activeTarget) return null;
  const id = host.activeTarget;
  return (
    host.clients.find((c) => c.id === id)?.name ??
    host.layout.find((c) => c.clientId === id)?.name ??
    "another computer"
  );
}

export function hostStatus(host: HostSnapshot): StatusText {
  if (!host.running) {
    return {
      title: "Sharing is off",
      detail: "Start sharing so other computers can connect and use this mouse and keyboard.",
      tone: "neutral",
    };
  }
  const n = host.clients.length;
  return {
    title: "Sharing is on",
    detail: `Sharing on port ${host.listenPort} — ${
      n === 0 ? "no devices connected yet" : `${plural(n, "device")} connected`
    }`,
    tone: "success",
  };
}

/** Plain-language description of the client connection state. */
export function clientStatus(c: ClientSnapshot, retryInMs: number | null): StatusText {
  const host = c.hostName ?? c.target ?? "the host";
  switch (c.state) {
    case "idle":
      return {
        title: "Not connected",
        detail: "Choose a host computer to use its mouse and keyboard here.",
        tone: "neutral",
      };
    case "connecting":
      return {
        title: `Connecting to ${host}…`,
        detail: "Looking for the host on your network.",
        tone: "accent",
      };
    case "pairing":
      return {
        title: "Waiting for the host to approve this computer",
        detail: `Check that both computers show the same code, then select Allow on ${host}.`,
        tone: "accent",
      };
    case "connected":
      return {
        title: `Connected to ${host}`,
        detail: c.active
          ? "The shared mouse and keyboard are on this computer right now."
          : `Move the mouse past the edge of ${host}'s screen to use this computer.`,
        tone: "success",
      };
    case "reconnecting":
      return {
        title:
          retryInMs !== null && retryInMs > 0
            ? `Lost connection — retrying in ${Math.ceil(retryInMs / 1000)} s`
            : "Lost connection — reconnecting…",
        detail:
          c.lastError ??
          `Trying to reach ${host} again. Check that it's turned on and on the same network.`,
        tone: "caution",
      };
    case "error":
      return {
        title: "Couldn't connect",
        detail: c.lastError ?? "Something went wrong. Try connecting again.",
        tone: "critical",
      };
  }
}

/** Short status for screen-reader announcements (no ticking countdowns). */
export function liveSummary(s: AppSnapshot): string {
  if (s.role === "host" && s.host) {
    const h = s.host;
    if (!h.running) return "Sharing is off.";
    const owner = cursorOwnerName(h);
    return [
      `Sharing on. ${plural(h.clients.length, "device")} connected.`,
      owner ? `Cursor is on ${owner}.` : "Cursor is on this computer.",
      h.locked ? "Cursor is locked." : "",
    ]
      .filter(Boolean)
      .join(" ");
  }
  if (s.role === "client" && s.client) {
    const c = s.client;
    const text = clientStatus(c, null);
    return c.state === "connected" && c.active ? `${text.title}. Cursor is on this computer.` : `${text.title}.`;
  }
  return "";
}

/** Compact label for the navigation footer. */
export function shortStatus(s: AppSnapshot): { label: string; tone: Tone } {
  if (s.role === "host" && s.host) {
    return s.host.running ? { label: "Host · Sharing", tone: "success" } : { label: "Host · Not sharing", tone: "neutral" };
  }
  if (s.role === "client" && s.client) {
    switch (s.client.state) {
      case "connected":
        return { label: "Client · Connected", tone: "success" };
      case "connecting":
      case "pairing":
        return { label: "Client · Connecting", tone: "accent" };
      case "reconnecting":
        return { label: "Client · Reconnecting", tone: "caution" };
      case "error":
        return { label: "Client · Error", tone: "critical" };
      case "idle":
        return { label: "Client · Not connected", tone: "neutral" };
    }
  }
  return { label: "Not set up", tone: "neutral" };
}
