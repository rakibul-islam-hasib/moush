// Small inline icon set in the spirit of Fluent System Icons (regular weight).
// Drawn on a 20x20 grid with strokes so they scale cleanly to 16 and 20 px.

type Part = string | { d: string; fill: true };

const circle = (cx: number, cy: number, r: number) =>
  `M${cx - r} ${cy}a${r} ${r} 0 1 0 ${2 * r} 0a${r} ${r} 0 1 0 ${-2 * r} 0`;

function gearPath(): string {
  const teeth = 8;
  const ro = 7.6;
  const ri = 5.9;
  const step = (Math.PI * 2) / teeth;
  const halfTop = step * 0.2;
  const halfBase = step * 0.3;
  const pt = (r: number, a: number) => `${(10 + r * Math.cos(a)).toFixed(2)} ${(10 + r * Math.sin(a)).toFixed(2)}`;
  let d = "";
  for (let i = 0; i < teeth; i++) {
    const a = i * step;
    d += `${i === 0 ? "M" : "L"}${pt(ri, a - halfBase)}L${pt(ro, a - halfTop)}L${pt(ro, a + halfTop)}L${pt(ri, a + halfBase)}`;
    d += `A${ri} ${ri} 0 0 1 ${pt(ri, a + step - halfBase)}`;
  }
  return `${d}Z`;
}

const ICONS = {
  home: ["M3.5 8.6 10 3.2l6.5 5.4v7.4a1 1 0 0 1-1 1H12.5v-4.5h-5V17H4.5a1 1 0 0 1-1-1z"],
  devices: [
    "M14.5 6V4.5a1 1 0 0 0-1-1h-10a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1H9",
    "M5 16h4M7 12.5V16",
    "M12.5 8h4a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z",
  ],
  laptop: ["M4.5 4h11a1 1 0 0 1 1 1v8.5h-13V5a1 1 0 0 1 1-1z", "M2 16h16"],
  desktop: ["M3.5 3.5h13a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-13a1 1 0 0 1-1-1v-8a1 1 0 0 1 1-1z", "M7 17h6M10 13.5V17"],
  layout: [
    "M4 3.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5H4a.5.5 0 0 1-.5-.5V4a.5.5 0 0 1 .5-.5z",
    "M12 3.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5h-4a.5.5 0 0 1-.5-.5V4a.5.5 0 0 1 .5-.5z",
    "M4 11.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5H4a.5.5 0 0 1-.5-.5v-4a.5.5 0 0 1 .5-.5z",
    "M12 11.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5h-4a.5.5 0 0 1-.5-.5v-4a.5.5 0 0 1 .5-.5z",
  ],
  settings: [gearPath(), circle(10, 10, 2.4)],
  pulse: ["M2 10.5h3.5l2-5.5 3 10 2.2-6 1.3 1.5H18"],
  link: [
    "M8.6 11.4a3 3 0 0 0 4.24 0l2.6-2.6a3 3 0 0 0-4.24-4.24l-.9.9",
    "M11.4 8.6a3 3 0 0 0-4.24 0l-2.6 2.6a3 3 0 0 0 4.24 4.24l.9-.9",
  ],
  unlink: [
    "M12.2 5.4l.4-.4a3 3 0 0 1 4.24 4.24l-1.6 1.6",
    "M7.8 14.6l-.4.4a3 3 0 0 1-4.24-4.24l1.6-1.6",
    "M3 3l14 14",
  ],
  shield: ["M10 2.5 4 4.8v4.6c0 3.6 2.5 6.3 6 8.1 3.5-1.8 6-4.5 6-8.1V4.8z"],
  lock: ["M5.5 9h9a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1z", "M7 9V6.5a3 3 0 0 1 6 0V9"],
  warning: [
    "M8.7 3.8a1.5 1.5 0 0 1 2.6 0l6 10.7a1.5 1.5 0 0 1-1.3 2.3H4a1.5 1.5 0 0 1-1.3-2.3z",
    "M10 7.8v3.7M10 14v.01",
  ],
  checkmark: ["M4 10.5l4 4 8-9"],
  dismiss: ["M5 5l10 10M15 5 5 15"],
  refresh: ["M15.8 10a5.8 5.8 0 1 1-1.7-4.1", "M14.6 2.6v3.6H11"],
  play: ["M6.5 4.3v11.4a.5.5 0 0 0 .76.43l9-5.7a.5.5 0 0 0 0-.86l-9-5.7a.5.5 0 0 0-.76.43z"],
  stop: ["M6 5h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"],
  mouse: ["M10 2.5a4.5 4.5 0 0 1 4.5 4.5v6a4.5 4.5 0 0 1-9 0V7A4.5 4.5 0 0 1 10 2.5z", "M10 5.5v2.5"],
  keyboard: [
    "M3 5.5h14a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1z",
    "M5 8.5h.01M8 8.5h.01M11 8.5h.01M14 8.5h.01M6.5 11.5h7",
  ],
  wifi: ["M2.5 7.7a10.5 10.5 0 0 1 15 0", "M5 10.5a7 7 0 0 1 10 0", "M7.5 13.3a3.5 3.5 0 0 1 5 0", "M10 16.2v.01"],
  copy: [
    "M7.5 6.5h7a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-8a1 1 0 0 1 1-1z",
    "M4.5 13.5v-9a1 1 0 0 1 1-1h7",
  ],
  folder: ["M2.5 5.5a1 1 0 0 1 1-1h4l2 2h7a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-13a1 1 0 0 1-1-1z"],
  info: [circle(10, 10, 7.5), "M10 9v5M10 6.5v.01"],
  successCircle: [circle(10, 10, 7.5), "M6.8 10.2l2.1 2.1 4.3-4.6"],
  errorCircle: [circle(10, 10, 7.5), "M7.6 7.6l4.8 4.8M12.4 7.6l-4.8 4.8"],
  chevronRight: ["M8 5l5 5-5 5"],
  chevronDown: ["M5 8l5 5 5-5"],
  timer: [circle(10, 11, 6.2), "M10 8v3l2 1.5M8 2.5h4"],
  theme: [circle(10, 10, 7), { d: "M10 3a7 7 0 0 1 0 14z", fill: true }],
  power: ["M10 2.5V9", "M6.2 5.3a6 6 0 1 0 7.6 0"],
  edit: ["M12.9 3.9a1.6 1.6 0 0 1 2.3 0l.9.9a1.6 1.6 0 0 1 0 2.3L7.5 15.7 4 16.5l.8-3.5z", "M11.5 5.3l3.2 3.2"],
  window: ["M3.5 3.5h13a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1h-13a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1z", "M2.5 7h15"],
  speed: ["M3.5 14.5a6.5 6.5 0 1 1 13 0", "M10 14.5l3.2-4"],
  corner: ["M3.5 9V4.5a1 1 0 0 1 1-1H9", "M16.5 11v4.5a1 1 0 0 1-1 1H11", "M7 7l6 6"],
  plug: ["M7.5 2.5v4M12.5 2.5v4", "M5 6.5h10v3a5 5 0 0 1-10 0z", "M10 14.5v3"],
  cpu: [
    "M6.5 5.5h7a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1z",
    "M8 2.5v3M12 2.5v3M8 14.5v3M12 14.5v3M2.5 8h3M2.5 12h3M14.5 8h3M14.5 12h3",
  ],
  document: [
    "M5.5 2.5h6l3.5 3.5v10.5a1 1 0 0 1-1 1h-8.5a1 1 0 0 1-1-1v-13a1 1 0 0 1 1-1z",
    "M11.5 2.5V6H15M7.5 10h5M7.5 13h5",
  ],
  cursor: ["M5.5 3.2v12.6l3.2-3.1 2.4 4.8 2-1-2.4-4.7h4.6z"],
  save: [
    "M4.5 3.5h9l3 3v9a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1z",
    "M6.5 3.5v3.5h6V3.5M6.5 16.5v-5h7v5",
  ],
  undo: ["M5 7.5h7.2a4 4 0 0 1 0 8H8.5", "M7.8 4.5l-3 3 3 3"],
  target: [circle(10, 10, 7), circle(10, 10, 3.5), "M10 10h.01"],
} satisfies Record<string, Part[]>;

export type IconName = keyof typeof ICONS;

interface IconProps {
  name: IconName;
  size?: 16 | 20 | 24 | 32;
  className?: string;
  /** Accessible name; omit for decorative icons. */
  label?: string;
}

export function Icon({ name, size = 16, className, label }: IconProps) {
  const parts: Part[] = ICONS[name];
  return (
    <svg
      className={className ? `icon ${className}` : "icon"}
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={size <= 16 ? 1.5 : 1.35}
      strokeLinecap="round"
      strokeLinejoin="round"
      focusable="false"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {parts.map((p, i) =>
        typeof p === "string" ? (
          <path key={i} d={p} />
        ) : (
          <path key={i} d={p.d} fill="currentColor" stroke="none" />
        ),
      )}
    </svg>
  );
}
