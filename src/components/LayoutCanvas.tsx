import { memo, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import type { MonitorInfo } from "../api";
import { useElementSize } from "../hooks/useElementSize";
import { cx } from "../lib/cx";
import { formatResolution, formatScale } from "../lib/format";
import { boundsOf, innerMonitorRects, sharedEdge, type Point, type Rect, type Segment } from "../lib/layoutGeometry";

export interface CanvasClient {
  id: string;
  name: string;
  online: boolean;
  /** Currently has the shared cursor. */
  active: boolean;
  rect: Rect;
  monitors: MonitorInfo[];
  resolutionWidth: number;
  resolutionHeight: number;
  scale: number;
  adjacent: boolean;
}

export interface MoveRequest {
  id: string;
  /** Requested top-left in layout units, before snapping / collision handling. */
  position: Point;
  /** Snap distance in layout units, or null for keyboard nudges (no snapping). */
  snapThreshold: number | null;
}

interface LayoutCanvasProps {
  monitors: MonitorInfo[];
  clients?: CanvasClient[];
  selectedId?: string | null;
  onSelect?: (id: string | null) => void;
  onMove?: (req: MoveRequest) => void;
  label: string;
  /** id of an element describing how to use the canvas. */
  describedBy?: string;
}

interface View {
  scale: number;
  minX: number;
  minY: number;
  ox: number;
  oy: number;
  width: number;
  height: number;
}

interface DragState {
  id: string;
  pointerId: number;
  startX: number;
  startY: number;
  origin: Rect;
  view: View;
  moved: boolean;
}

const NO_CLIENTS: CanvasClient[] = [];
const PADDING_PX = 20;
const LINE = 16;
const SNAP_UNITS = 40;
const SNAP_MIN_PX = 8;

function computeView(rects: Rect[], width: number, height: number): View {
  const b = boundsOf(rects) ?? { x: 0, y: 0, width: 1920, height: 1080 };
  // Leave room around the scene so devices can be dragged outward.
  const margin = Math.max(b.width, b.height) * 0.06;
  const bw = b.width + margin * 2;
  const bh = b.height + margin * 2;
  const scale = Math.max(0.0005, Math.min((width - PADDING_PX * 2) / bw, (height - PADDING_PX * 2) / bh));
  return {
    scale,
    minX: b.x - margin,
    minY: b.y - margin,
    ox: (width - bw * scale) / 2,
    oy: (height - bh * scale) / 2,
    width,
    height,
  };
}

function truncate(text: string, maxChars: number): string {
  if (maxChars <= 1) return "";
  return text.length <= maxChars ? text : `${text.slice(0, maxChars - 1)}…`;
}

/** Centered text lines that are dropped / truncated to fit the box. */
function BoxLabel({ x, y, w, h, lines }: { x: number; y: number; w: number; h: number; lines: { text: string; secondary?: boolean; strong?: boolean }[] }) {
  const fit = Math.min(lines.length, Math.floor((h - 8) / LINE));
  const maxChars = Math.floor((w - 12) / 6.8);
  if (fit <= 0 || maxChars < 3) return null;
  const shown = lines.slice(0, fit);
  const top = y + h / 2 - (shown.length * LINE) / 2 + 12;
  return (
    <text className="lc-label" x={x + w / 2} y={top} textAnchor="middle">
      {shown.map((l, i) => (
        <tspan
          key={i}
          x={x + w / 2}
          dy={i === 0 ? 0 : LINE}
          className={cx(l.secondary && "lc-label-secondary", l.strong && "lc-label-strong")}
        >
          {truncate(l.text, maxChars)}
        </tspan>
      ))}
    </text>
  );
}

/**
 * Draws the host monitors (fixed) and client rectangles (draggable) of the
 * display layout, auto-fitted to the available space.
 */
export const LayoutCanvas = memo(function LayoutCanvas({ monitors, clients = NO_CLIENTS, selectedId = null, onSelect, onMove, label, describedBy }: LayoutCanvasProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const size = useElementSize(containerRef);
  const dragRef = useRef<DragState | null>(null);
  const [frozenView, setFrozenView] = useState<View | null>(null);
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [focusVisibleId, setFocusVisibleId] = useState<string | null>(null);

  const width = size.width || 640;
  const height = size.height || 360;
  // Freeze the zoom while dragging so the scene doesn't jump under the pointer.
  const view = frozenView ?? computeView([...monitors, ...clients.map((c) => c.rect)], width, height);
  const sx = (x: number) => view.ox + (x - view.minX) * view.scale;
  const sy = (y: number) => view.oy + (y - view.minY) * view.scale;
  const toScreen = (r: Rect) => ({ x: sx(r.x), y: sy(r.y), w: r.width * view.scale, h: r.height * view.scale });

  const edges: Segment[] = [];
  for (const c of clients) {
    for (const m of monitors) {
      const e = sharedEdge(c.rect, m);
      if (e) edges.push(e);
    }
  }

  const endDrag = () => {
    dragRef.current = null;
    setFrozenView(null);
    setDraggingId(null);
  };

  const onPointerDown = (e: PointerEvent<SVGGElement>, c: CanvasClient) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    onSelect?.(c.id);
    if (!onMove) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    dragRef.current = { id: c.id, pointerId: e.pointerId, startX: e.clientX, startY: e.clientY, origin: c.rect, view, moved: false };
    setFrozenView(view);
  };

  const onPointerMove = (e: PointerEvent<SVGGElement>) => {
    const d = dragRef.current;
    if (!d || d.pointerId !== e.pointerId || !onMove) return;
    const dx = e.clientX - d.startX;
    const dy = e.clientY - d.startY;
    if (!d.moved) {
      if (Math.hypot(dx, dy) < 3) return;
      d.moved = true;
      setDraggingId(d.id);
    }
    const v = d.view;
    // Keep the rectangle inside the visible canvas.
    const visMinX = v.minX - v.ox / v.scale;
    const visMinY = v.minY - v.oy / v.scale;
    const visMaxX = visMinX + v.width / v.scale - d.origin.width;
    const visMaxY = visMinY + v.height / v.scale - d.origin.height;
    const x = Math.min(Math.max(d.origin.x + dx / v.scale, visMinX), Math.max(visMinX, visMaxX));
    const y = Math.min(Math.max(d.origin.y + dy / v.scale, visMinY), Math.max(visMinY, visMaxY));
    onMove({ id: d.id, position: { x, y }, snapThreshold: Math.max(SNAP_UNITS, SNAP_MIN_PX / v.scale) });
  };

  const onPointerUp = (e: PointerEvent<SVGGElement>) => {
    const d = dragRef.current;
    if (!d || d.pointerId !== e.pointerId) return;
    endDrag();
  };

  const onKeyDown = (e: KeyboardEvent<SVGGElement>, c: CanvasClient) => {
    if (!onMove) return;
    const step = e.shiftKey ? 100 : 10;
    let dx = 0;
    let dy = 0;
    if (e.key === "ArrowLeft") dx = -step;
    else if (e.key === "ArrowRight") dx = step;
    else if (e.key === "ArrowUp") dy = -step;
    else if (e.key === "ArrowDown") dy = step;
    else return;
    e.preventDefault();
    onMove({ id: c.id, position: { x: c.rect.x + dx, y: c.rect.y + dy }, snapThreshold: null });
  };

  return (
    <div className="lc" ref={containerRef}>
      <svg
        width={width}
        height={height}
        role="group"
        aria-label={label}
        aria-describedby={describedBy}
        onPointerDown={() => onSelect?.(null)}
      >
        {monitors.map((m) => {
          const r = toScreen(m);
          return (
            <g key={m.id} className="lc-host">
              <title>{`${m.name}${m.primary ? " (primary)" : ""}, ${formatResolution(m.width, m.height)}, ${formatScale(m.scale)}`}</title>
              <rect className="lc-mon" x={r.x} y={r.y} width={r.w} height={r.h} rx={3} />
              {m.primary && <rect className="lc-primary-bar" x={r.x + 6} y={r.y + 6} width={Math.min(28, r.w / 4)} height={3} rx={1.5} />}
              <BoxLabel
                x={r.x}
                y={r.y}
                w={r.w}
                h={r.h}
                lines={[
                  { text: m.name, strong: true },
                  { text: formatResolution(m.width, m.height), secondary: true },
                  { text: formatScale(m.scale) + (m.primary ? " · Primary" : ""), secondary: true },
                ]}
              />
            </g>
          );
        })}

        {clients.map((c) => {
          const r = toScreen(c.rect);
          const inner = innerMonitorRects(c.rect, c.monitors).map(toScreen);
          const selected = c.id === selectedId;
          const status = c.active ? "Has the cursor" : c.online ? "Online" : "Offline";
          return (
            <g
              key={c.id}
              className={cx(
                "lc-client",
                !c.online && "is-offline",
                selected && "is-selected",
                c.active && "is-active",
                draggingId === c.id && "is-dragging",
                onMove && "is-movable",
              )}
              tabIndex={0}
              role="button"
              aria-pressed={selected}
              aria-label={`${c.name}, ${formatResolution(c.resolutionWidth, c.resolutionHeight)} at ${formatScale(c.scale)}, ${status}${
                c.adjacent ? "" : ", not touching any screen of this computer"
              }`}
              onPointerDown={(e) => onPointerDown(e, c)}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={endDrag}
              onLostPointerCapture={endDrag}
              onKeyDown={(e) => onKeyDown(e, c)}
              onFocus={(e) => {
                onSelect?.(c.id);
                if (e.currentTarget.matches(":focus-visible")) setFocusVisibleId(c.id);
              }}
              onBlur={() => setFocusVisibleId(null)}
            >
              <rect className="lc-client-frame" x={r.x} y={r.y} width={r.w} height={r.h} rx={3} />
              {inner.map((m, i) => (
                <rect
                  key={i}
                  className="lc-client-monitor"
                  x={m.x + 3}
                  y={m.y + 3}
                  width={Math.max(0, m.w - 6)}
                  height={Math.max(0, m.h - 6)}
                  rx={2}
                />
              ))}
              <BoxLabel
                x={r.x}
                y={r.y}
                w={r.w}
                h={r.h}
                lines={[
                  { text: c.name, strong: true },
                  { text: `${formatResolution(c.resolutionWidth, c.resolutionHeight)} · ${formatScale(c.scale)}`, secondary: true },
                  { text: status, secondary: true },
                ]}
              />
              {!c.adjacent && r.w > 28 && r.h > 28 && (
                <g className="lc-warning" transform={`translate(${r.x + r.w - 22} ${r.y + 6})`}>
                  <path d="M8 1.5 15 14.5H1z" />
                  <path className="lc-warning-mark" d="M8 6v4M8 12v.01" />
                </g>
              )}
              {focusVisibleId === c.id && (
                <rect className="lc-focus" x={r.x - 4} y={r.y - 4} width={r.w + 8} height={r.h + 8} rx={6} />
              )}
            </g>
          );
        })}

        {edges.map((e, i) => (
          <line key={i} className="lc-edge" x1={sx(e.x1)} y1={sy(e.y1)} x2={sx(e.x2)} y2={sy(e.y2)} />
        ))}
      </svg>
    </div>
  );
});
