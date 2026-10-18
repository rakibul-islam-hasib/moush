/**
 * Structural sharing for snapshots: returns `next`, but reuses every part of `prev`
 * that is deeply equal. Snapshots arrive several times per second during transfers;
 * keeping unchanged sub-objects identical lets React.memo / useMemo skip work for
 * everything that did not change. Arrays of objects with a string `id` are matched by
 * id, so inserting a new transfer at the top keeps the identity of the others.
 */
export function share<T>(prev: unknown, next: T): T {
  if (Object.is(prev, next)) return next;
  if (typeof prev !== "object" || typeof next !== "object" || prev === null || next === null) return next;

  if (Array.isArray(next)) {
    if (!Array.isArray(prev)) return next;
    const byId = idIndex(prev);
    let same = prev.length === next.length;
    const out = next.map((item: unknown, i) => {
      const id = idOf(item);
      const old = id !== null && byId ? byId.get(id) : prev[i];
      const shared = share(old, item);
      if (shared !== prev[i]) same = false;
      return shared;
    });
    return (same ? prev : out) as T;
  }
  if (Array.isArray(prev)) return next;

  const p = prev as Record<string, unknown>;
  const n = next as Record<string, unknown>;
  const keys = Object.keys(n);
  let same = keys.length === Object.keys(p).length;
  const out: Record<string, unknown> = {};
  for (const k of keys) {
    const shared = share(p[k], n[k]);
    out[k] = shared;
    if (shared !== p[k]) same = false;
  }
  return (same ? p : out) as T;
}

function idOf(item: unknown): string | null {
  if (item && typeof item === "object" && "id" in item) {
    const id = (item as { id: unknown }).id;
    return typeof id === "string" ? id : null;
  }
  return null;
}

function idIndex(items: unknown[]): Map<string, unknown> | null {
  if (items.length === 0 || idOf(items[0]) === null) return null;
  const map = new Map<string, unknown>();
  for (const item of items) {
    const id = idOf(item);
    if (id !== null) map.set(id, item);
  }
  return map;
}
