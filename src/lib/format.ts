const DASH = "—";

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** "123456" -> "123 456" */
export function formatCode(code: string): string {
  const digits = code.replace(/\D/g, "");
  return digits.length === 6 ? `${digits.slice(0, 3)} ${digits.slice(3)}` : code;
}

export function formatMs(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return DASH;
  if (ms < 1) return `${ms.toFixed(2)} ms`;
  if (ms < 10) return `${ms.toFixed(1)} ms`;
  return `${Math.round(ms)} ms`;
}

/** Microseconds, switching to milliseconds above 1000 µs. */
export function formatUs(us: number): string {
  if (!Number.isFinite(us)) return DASH;
  if (us < 1000) return `${Math.round(us)} µs`;
  const ms = us / 1000;
  if (ms < 10) return `${ms.toFixed(2)} ms`;
  if (ms < 100) return `${ms.toFixed(1)} ms`;
  return `${Math.round(ms)} ms`;
}

export function formatBytes(n: number): string {
  if (!Number.isFinite(n)) return DASH;
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return unit === 0 ? `${value} B` : `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

export function formatCount(n: number): string {
  return Number.isFinite(n) ? n.toLocaleString() : DASH;
}

export function formatDuration(totalSecs: number): string {
  const s = Math.max(0, Math.floor(totalSecs));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ${s % 60} s`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} h ${m % 60} min`;
  return `${Math.floor(h / 24)} d ${h % 24} h`;
}

export function formatRelative(ts: number, now: number): string {
  const diff = Math.max(0, now - ts);
  const min = Math.round(diff / 60_000);
  if (diff < 45_000) return "just now";
  if (min < 90) return `${Math.max(1, min)} min ago`;
  const h = Math.round(diff / 3_600_000);
  if (h < 36) return `${h} h ago`;
  return new Date(ts).toLocaleDateString();
}

/** Remaining time for countdowns: "45 s" or "1:05". */
export function formatCountdown(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  if (s < 60) return `${s} s`;
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function formatScale(scale: number): string {
  return `${Math.round(scale * 100)}%`;
}

export function formatResolution(width: number, height: number): string {
  return `${width} × ${height}`;
}

/** HH:MM:SS.mmm in local time. */
export function formatClock(ts: number): string {
  const d = new Date(ts);
  const p = (n: number, len = 2) => String(n).padStart(len, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}
