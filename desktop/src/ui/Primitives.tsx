import { useId, type ReactNode } from "react";
import { motion } from "motion/react";

export function Card({ children, className = "", pad = true }: { children: ReactNode; className?: string; pad?: boolean }) {
  return <section className={`vd-card ${pad ? "p-5" : ""} ${className}`}>{children}</section>;
}

export function SectionTitle({ children, aside }: { children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="flex items-center justify-between mb-3">
      <h3 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint">{children}</h3>
      {aside}
    </div>
  );
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: string; actions?: ReactNode }) {
  return (
    <header className="flex items-start justify-between gap-4 mb-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        {subtitle && <p className="text-muted mt-1 text-sm max-w-xl">{subtitle}</p>}
      </div>
      {actions && <div className="flex items-center gap-2 shrink-0">{actions}</div>}
    </header>
  );
}

export type Tone = "neutral" | "accent" | "success" | "warn" | "danger";
const tones: Record<Tone, string> = {
  neutral: "bg-surface-2 text-muted border-line",
  accent: "bg-accent-soft text-accent border-accent/25",
  success: "bg-success/10 text-success border-success/25",
  warn: "bg-warn/10 text-warn border-warn/25",
  danger: "bg-danger-soft text-danger border-danger/25",
};
export function Badge({ tone = "neutral", children, dot }: { tone?: Tone; children: ReactNode; dot?: boolean }) {
  return (
    <span className={`inline-flex items-center gap-1.5 h-6 px-2.5 rounded-full border text-xs font-medium ${tones[tone]}`}>
      {dot && <span className="size-1.5 rounded-full bg-current" />}
      {children}
    </span>
  );
}

export function ProgressBar({ value, tone = "accent", indeterminate, label }: { value: number; tone?: "accent" | "success" | "danger" | "warn"; indeterminate?: boolean; label?: string }) {
  const pct = Math.min(1, Math.max(0, value));
  const bg =
    tone === "success" ? "var(--vd-success)"
    : tone === "danger" ? "var(--vd-danger)"
    : tone === "warn" ? "var(--vd-warn)"
    : "linear-gradient(90deg, var(--vd-accent), var(--vd-accent-2))";
  return (
    <div
      className="h-1.5 w-full rounded-full bg-surface-3 overflow-hidden"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(pct * 100)}
    >
      {indeterminate ? (
        <motion.div className="h-full w-1/3 rounded-full" style={{ background: bg }} animate={{ x: ["-100%", "300%"] }} transition={{ duration: 1.4, repeat: Infinity, ease: "linear" }} />
      ) : (
        <div className="h-full rounded-full transition-[width] duration-200" style={{ width: `${pct * 100}%`, background: bg }} />
      )}
    </div>
  );
}

export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 rounded-full border transition-colors duration-200 disabled:opacity-45 ${checked ? "bg-accent border-accent" : "bg-surface-3 border-line-strong"}`}
    >
      <span className={`absolute top-0.5 size-[18px] rounded-full transition-all duration-200 ${checked ? "bg-accent-ink start-[22px]" : "bg-fg/80 start-0.5"}`} />
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; label: string }) {
  return (
    <div role="radiogroup" aria-label={label} className="inline-flex p-0.5 rounded-[10px] bg-surface-2 border border-line">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          onClick={() => onChange(o.value)}
          className={`h-8 px-3.5 text-xs font-medium rounded-lg transition-colors duration-200 ${o.value === value ? "bg-surface-3 text-fg shadow-sm" : "text-muted hover:text-fg"}`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  const id = useId();
  return (
    <div className="flex flex-col gap-1.5" role="group" aria-labelledby={id}>
      <span id={id} className="text-xs font-medium text-muted">{label}</span>
      {children}
      {hint && <span className="text-xs text-faint">{hint}</span>}
    </div>
  );
}

export const inputCls =
  "h-9 w-full rounded-[10px] bg-surface-2 border border-line-strong px-3 text-sm placeholder:text-faint focus:border-accent outline-none transition-colors duration-200 disabled:opacity-50";

export function Row({ title, desc, children }: { title: string; desc?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3.5 border-b border-line last:border-b-0">
      <div className="min-w-0">
        <div className="text-sm font-medium">{title}</div>
        {desc && <div className="text-xs text-muted mt-0.5 max-w-md">{desc}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function EmptyState({ icon, title, desc, action }: { icon: ReactNode; title: string; desc?: string; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center text-center py-14 px-6 gap-3">
      <div className="size-12 rounded-2xl bg-surface-2 border border-line grid place-items-center text-muted">{icon}</div>
      <div className="font-medium">{title}</div>
      {desc && <div className="text-sm text-muted max-w-sm">{desc}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
