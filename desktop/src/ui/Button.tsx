import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Spinner } from "./Loader";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md" | "lg";

const base =
  "inline-flex items-center justify-center gap-2 font-medium rounded-[10px] transition-[background,border-color,opacity,transform] duration-200 disabled:opacity-45 disabled:pointer-events-none active:scale-[0.98] select-none whitespace-nowrap";
const variants: Record<Variant, string> = {
  primary: "bg-accent text-accent-ink hover:brightness-110",
  secondary: "bg-surface-2 border border-line-strong hover:bg-surface-3",
  ghost: "text-muted hover:text-fg hover:bg-surface-2",
  danger: "bg-danger-soft text-danger border border-danger/30 hover:bg-danger/20",
};
const sizes: Record<Size, string> = { sm: "h-8 px-3 text-xs", md: "h-9 px-4 text-sm", lg: "h-11 px-6 text-sm" };

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  busy?: boolean;
  icon?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, Props>(function Button(
  { variant = "secondary", size = "md", busy, icon, children, className = "", disabled, ...rest },
  ref,
) {
  return (
    <button ref={ref} type="button" disabled={disabled || busy} className={`${base} ${variants[variant]} ${sizes[size]} ${className}`} {...rest}>
      {busy ? <Spinner size={14} /> : icon}
      {children}
    </button>
  );
});

export function IconButton({ label, children, className = "", ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`size-8 grid place-items-center rounded-lg text-muted hover:text-fg hover:bg-surface-2 transition-colors duration-200 disabled:opacity-40 ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}
