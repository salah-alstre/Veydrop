import { useId } from "react";

/** The Veydrop mark: a screen and a phone with data moving between them. */
export function Logo({ size = 32, animated = false, plate = true }: { size?: number; animated?: boolean; plate?: boolean }) {
  const gid = useId().replace(/:/g, "");
  const stroke = `url(#${gid})`;
  return (
    <svg width={size} height={size} viewBox="0 0 512 512" fill="none" aria-hidden="true">
      <defs>
        <linearGradient id={gid} x1="80" y1="140" x2="436" y2="400" gradientUnits="userSpaceOnUse">
          <stop stopColor="#5EEAD4" />
          <stop offset="1" stopColor="#38BDF8" />
        </linearGradient>
      </defs>
      {plate && <rect width="512" height="512" rx="112" fill="#0A0B0D" />}
      <rect x="80" y="214" width="196" height="150" rx="30" stroke={stroke} strokeWidth="24" />
      <path d="M146 400h64" stroke={stroke} strokeWidth="24" strokeLinecap="round" />
      <rect x="352" y="140" width="84" height="170" rx="26" stroke={stroke} strokeWidth="24" />
      <path d="M380 282h28" stroke={stroke} strokeWidth="20" strokeLinecap="round" />
      <g fill={stroke}>
        <circle className={animated ? "vd-dot" : undefined} cx="298" cy="262" r="9" opacity={animated ? undefined : 0.45} />
        <circle className={animated ? "vd-dot" : undefined} cx="318" cy="252" r="12" opacity={animated ? undefined : 0.75} />
        <circle className={animated ? "vd-dot" : undefined} cx="340" cy="238" r="14" />
      </g>
    </svg>
  );
}

export function Wordmark({ size = 20 }: { size?: number }) {
  return (
    <span className="inline-flex items-center gap-2.5">
      <Logo size={size + 8} />
      <span className="font-semibold tracking-tight" style={{ fontSize: size }}>Veydrop</span>
    </span>
  );
}
