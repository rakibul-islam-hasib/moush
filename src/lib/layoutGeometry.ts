// Pure geometry for the display layout editor. All values are layout units
// (host physical pixels).

export interface Point {
  x: number;
  y: number;
}

export interface Rect extends Point {
  width: number;
  height: number;
}

export interface Segment {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

/**
 * Edges closer than this count as touching. Positions are saved as integers
 * while client sizes may be fractional, so exact equality is not reachable.
 */
const TOUCH_TOLERANCE = 1;

/** True when the rectangles share interior area (touching edges is allowed). */
function overlapsInterior(a: Rect, b: Rect): boolean {
  const e = TOUCH_TOLERANCE;
  return (
    a.x < b.x + b.width - e &&
    b.x < a.x + a.width - e &&
    a.y < b.y + b.height - e &&
    b.y < a.y + a.height - e
  );
}

/** The segment where two rectangles touch, or null when they don't share an edge. */
export function sharedEdge(a: Rect, b: Rect): Segment | null {
  const e = TOUCH_TOLERANCE;
  const ar = a.x + a.width;
  const ab = a.y + a.height;
  const br = b.x + b.width;
  const bb = b.y + b.height;

  const vx = Math.abs(ar - b.x) <= e ? b.x : Math.abs(br - a.x) <= e ? a.x : null;
  if (vx !== null) {
    const y1 = Math.max(a.y, b.y);
    const y2 = Math.min(ab, bb);
    if (y2 - y1 > e) return { x1: vx, y1, x2: vx, y2 };
  }
  const hy = Math.abs(ab - b.y) <= e ? b.y : Math.abs(bb - a.y) <= e ? a.y : null;
  if (hy !== null) {
    const x1 = Math.max(a.x, b.x);
    const x2 = Math.min(ar, br);
    if (x2 - x1 > e) return { x1, y1: hy, x2, y2: hy };
  }
  return null;
}

export function boundsOf(rects: readonly Rect[]): Rect | null {
  if (rects.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const r of rects) {
    minX = Math.min(minX, r.x);
    minY = Math.min(minY, r.y);
    maxX = Math.max(maxX, r.x + r.width);
    maxY = Math.max(maxY, r.y + r.height);
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

/**
 * Snaps the rectangle's edges to nearby target edges: touching (left to right,
 * top to bottom) and alignment (left to left, top to top, ...). Each axis snaps
 * independently to the closest candidate within the threshold.
 */
export function snapPosition(rect: Rect, targets: readonly Rect[], threshold: number): Point {
  let bestDx: number | null = null;
  let bestDy: number | null = null;
  const consider = (current: number | null, delta: number) =>
    Math.abs(delta) <= threshold && (current === null || Math.abs(delta) < Math.abs(current)) ? delta : current;

  const r = rect.x + rect.width;
  const b = rect.y + rect.height;
  for (const t of targets) {
    const tr = t.x + t.width;
    const tb = t.y + t.height;
    const nearVertically = rect.y < tb + threshold && t.y < b + threshold;
    const nearHorizontally = rect.x < tr + threshold && t.x < r + threshold;
    if (nearVertically) {
      for (const d of [tr - rect.x, t.x - r, t.x - rect.x, tr - r]) bestDx = consider(bestDx, d);
    }
    if (nearHorizontally) {
      for (const d of [tb - rect.y, t.y - b, t.y - rect.y, tb - b]) bestDy = consider(bestDy, d);
    }
  }
  return { x: rect.x + (bestDx ?? 0), y: rect.y + (bestDy ?? 0) };
}

/**
 * Returns the free position closest to the rectangle's position, pushing it out
 * of any obstacle it overlaps (it ends up touching that obstacle). Falls back to
 * `fallback` when no free position is found nearby.
 */
export function resolveCollisions(rect: Rect, obstacles: readonly Rect[], fallback: Point): Point {
  const at = (p: Point): Rect => ({ x: p.x, y: p.y, width: rect.width, height: rect.height });
  const isFree = (p: Point) => obstacles.every((o) => !overlapsInterior(at(p), o));
  const origin: Point = { x: rect.x, y: rect.y };
  if (isFree(origin)) return origin;

  const pushOuts = (p: Point): Point[] =>
    obstacles
      .filter((o) => overlapsInterior(at(p), o))
      .flatMap((o) => [
        { x: o.x - rect.width, y: p.y },
        { x: o.x + o.width, y: p.y },
        { x: p.x, y: o.y - rect.height },
        { x: p.x, y: o.y + o.height },
      ]);

  const first = pushOuts(origin);
  const candidates = [...first, ...first.flatMap(pushOuts)];
  let best: Point | null = null;
  let bestDist = Infinity;
  for (const c of candidates) {
    if (!isFree(c)) continue;
    const d = Math.hypot(c.x - origin.x, c.y - origin.y);
    if (d < bestDist) {
      best = c;
      bestDist = d;
    }
  }
  return best ?? fallback;
}

/** Maps monitors (in their own computer's coordinates) proportionally into a frame. */
export function innerMonitorRects(frame: Rect, monitors: readonly Rect[]): Rect[] {
  const b = boundsOf(monitors);
  if (!b || b.width <= 0 || b.height <= 0) return [];
  const sx = frame.width / b.width;
  const sy = frame.height / b.height;
  return monitors.map((m) => ({
    x: frame.x + (m.x - b.x) * sx,
    y: frame.y + (m.y - b.y) * sy,
    width: m.width * sx,
    height: m.height * sy,
  }));
}

/** True when the rectangle touches at least one of the targets along an edge. */
export function touchesAny(rect: Rect, targets: readonly Rect[]): boolean {
  return targets.some((t) => sharedEdge(rect, t) !== null);
}
