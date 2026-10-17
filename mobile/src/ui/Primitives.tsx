import type { ReactNode } from "react";
import { motion, useReducedMotion } from "motion/react";

export function ProgressBar({
  value,
  tone = "accent",
  indeterminate,
  label,
}: {
  value: number;
  tone?: "accent" | "success" | "danger" | "warn";
  indeterminate?: boolean;
  label?: string;
}) {
  const reduce = useReducedMotion();
  const pct = Math.min(1, Math.max(0, value));
  const color =
    tone === "success" ? "var(--vd-success)"
    : tone === "danger" ? "var(--vd-danger)"
    : tone === "warn" ? "var(--vd-warn)"
    : "var(--vd-accent)";
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(pct * 100)}
      className="h-1.5 w-full rounded-full bg-surface-3 overflow-hidden"
    >
      {indeterminate ? (
        <div className="vd-skeleton h-full w-full" />
      ) : (
        <motion.div
          className="h-full rounded-full"
          style={{ background: color }}
          initial={false}
          animate={{ width: `${pct * 100}%` }}
          transition={{ duration: reduce ? 0 : 0.2, ease: [0.2, 0.7, 0.2, 1] }}
        />
      )}
    </div>
  );
}

export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <section className={`vd-card p-4 ${className}`}>{children}</section>;
}

export type Tone = "neutral" | "accent" | "success" | "warn" | "danger";
const tones: Record<Tone, string> = {
  neutral: "bg-surface-2 text-muted border-line",
  accent: "bg-accent-soft text-accent border-accent/25",
  success: "bg-success/10 text-success border-success/25",
  warn: "bg-warn/10 text-warn border-warn/25",
  danger: "bg-danger-soft text-danger border-danger/25",
};

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return (
    <span className={`inline-flex items-center gap-1.5 h-6 px-2.5 rounded-full border text-xs font-medium ${tones[tone]}`}>
      {children}
    </span>
  );
}

/** Full-screen scaffold: safe-area padding, a scrolling body and an optional pinned footer. */
export function Screen({
  header,
  children,
  footer,
}: {
  header?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div className="min-h-dvh flex flex-col">
      {header && <div className="safe-top safe-x sticky top-0 z-20 bg-bg/90 backdrop-blur-sm border-b border-line">{header}</div>}
      <main className="safe-x flex-1 py-5">{children}</main>
      {footer && (
        <div className="safe-x safe-bottom sticky bottom-0 z-20 bg-bg/95 border-t border-line pt-3 pb-3">{footer}</div>
      )}
    </div>
  );
}
