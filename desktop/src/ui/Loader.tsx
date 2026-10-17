import type { CSSProperties, ReactNode } from "react";
import { Monitor, Smartphone } from "lucide-react";

type LinkState = "connecting" | "connected" | "failed" | "waiting";

/** Two device nodes with packets flowing between them. `state` drives colour and motion. */
export function LinkLoader({ state = "connecting", width = 220 }: { state?: LinkState; width?: number }) {
  const color = state === "failed" ? "var(--vd-danger)" : state === "connected" ? "var(--vd-success)" : "var(--vd-accent)";
  const dist = width - 88;
  const flowing = state === "connecting" || state === "waiting";
  const style = { width, height: 56, "--vd-flow-dist": `${dist}px` } as CSSProperties;
  return (
    <div className="relative flex items-center justify-between" style={style} aria-hidden="true">
      <Node><Monitor size={22} /></Node>
      <div
        className="absolute top-1/2 h-px -translate-y-1/2"
        style={{ insetInlineStart: 44, insetInlineEnd: 44, background: `linear-gradient(90deg, transparent, ${color}, transparent)`, opacity: state === "failed" ? 0.35 : 0.6 }}
      />
      {flowing && (
        <div className="absolute top-1/2 -translate-y-1/2" style={{ insetInlineStart: 44 }}>
          {[0, 0.5, 1].map((d) => (
            <span key={d} className="vd-packet absolute block size-1.5 rounded-full" style={{ background: color, animationDelay: `${d}s`, top: -3 }} />
          ))}
        </div>
      )}
      {state === "connected" && (
        <span className="absolute top-1/2 -translate-y-1/2 size-2 rounded-full start-1/2" style={{ background: color, boxShadow: `0 0 12px ${color}` }} />
      )}
      <Node pulse={state === "waiting"}><Smartphone size={22} /></Node>
    </div>
  );
}

function Node({ children, pulse }: { children: ReactNode; pulse?: boolean }) {
  return (
    <div className="relative size-11 rounded-xl border border-line-strong bg-surface-2 grid place-items-center text-fg">
      {pulse && <span className="vd-ring absolute inset-0 rounded-xl border border-accent" />}
      {children}
    </div>
  );
}

export function Spinner({ size = 16 }: { size?: number }) {
  return (
    <svg className="vd-spin" width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity=".2" strokeWidth="3" />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
    </svg>
  );
}

export function Skeleton({ className = "" }: { className?: string }) {
  return <div className={`vd-skeleton ${className}`} aria-hidden="true" />;
}
