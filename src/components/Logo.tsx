import { useId } from "react";

/**
 * The Moush mark: a mouse whose scroll wheel becomes a link to a second screen,
 * on the brand gradient tile.
 */
export function Logo({ size = 24, className }: { size?: number; className?: string }) {
  const id = useId();
  return (
    <svg className={className ? `logo ${className}` : "logo"} width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id={`${id}-g`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="var(--brand-1)" />
          <stop offset="1" stopColor="var(--brand-2)" />
        </linearGradient>
      </defs>
      <rect x="1" y="1" width="30" height="30" rx="9" fill={`url(#${id}-g)`} />
      <path d="M11.5 9.5a4.5 4.5 0 0 1 9 0v7a4.5 4.5 0 0 1-9 0z" fill="none" stroke="#fff" strokeWidth="2" />
      <path d="M16 9.2v3" stroke="#fff" strokeWidth="2" strokeLinecap="round" />
      <path d="M20.5 22.5h3.2a2.3 2.3 0 0 0 2.3-2.3v-1.4" fill="none" stroke="#fff" strokeOpacity="0.75" strokeWidth="1.8" strokeLinecap="round" />
      <circle cx="26" cy="15.6" r="1.6" fill="#fff" fillOpacity="0.75" />
    </svg>
  );
}
