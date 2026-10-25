import { useId } from "react";

/**
 * The Moush mark (Concept B — The Infinity Glide):
 * Continuous low-latency input ribbon looping across dual computing domains,
 * resolving into a forward-pointing cursor arrow on a Fluent acrylic squircle tile.
 * Zero blur filters, zero shadows, pure crisp vector geometry.
 */
export function Logo({ size = 24, className }: { size?: number; className?: string }) {
  const id = useId();
  return (
    <svg className={className ? `logo ${className}` : "logo"} width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id={`${id}-bg`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#0F172A" />
          <stop offset="100%" stopColor="#020408" />
        </linearGradient>
        <linearGradient id={`${id}-flow`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#00F0FF" />
          <stop offset="35%" stopColor="#38BDF8" />
          <stop offset="70%" stopColor="#2563EB" />
          <stop offset="100%" stopColor="#6366F1" />
        </linearGradient>
      </defs>
      {/* Acrylic Squircle Base (Zero shadow) */}
      <rect x="1" y="1" width="30" height="30" rx="7.5" fill={`url(#${id}-bg)`} stroke="#38BDF8" strokeOpacity={0.3} strokeWidth={0.8} />

      {/* Infinity Continuum Ribbon */}
      <g transform="translate(3.5, 3.2) scale(0.098)">
        <path fill={`url(#${id}-flow)`} fillRule="evenodd" d="
          M 80 64
          C 44.7 64 16 92.7 16 128
          C 16 163.3 44.7 192 80 192
          C 106 192 128 176 142 152
          L 170 186
          C 192 186 214 174 226 156
          L 240 170
          V 124
          H 194
          L 208 138
          C 198 152 184 160 170 160
          C 156 160 144 150 134 138
          L 122 124
          C 134 108 152 96 170 96
          C 186 96 200 104 210 118
          L 230 98
          C 216 78 194 64 170 64
          C 144 64 122 78 108 102
          C 96 79 89 64 80 64
          Z
          M 80 96
          C 97.7 96 112 110.3 112 128
          C 112 145.7 97.7 160 80 160
          C 62.3 160 48 145.7 48 128
          C 48 110.3 62.3 96 80 96
          Z
        " />
        <path fill="#FFFFFF" fillOpacity={0.35} d="
          M 194 124
          L 240 124
          L 240 170
          Z
        " />
      </g>
    </svg>
  );
}
