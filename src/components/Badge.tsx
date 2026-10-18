import type { ReactNode } from "react";
import type { Tone } from "../lib/status";

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}

/** Small colored dot; the meaning must also be conveyed by adjacent text. */
export function StatusDot({ tone }: { tone: Tone }) {
  return <span className={`status-dot status-dot-${tone}`} aria-hidden="true" />;
}
