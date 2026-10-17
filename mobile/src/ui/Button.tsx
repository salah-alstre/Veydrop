import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Spinner } from "./Loader";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const base =
  "inline-flex items-center justify-center gap-2 font-medium rounded-[12px] transition-[background,border-color,opacity,transform] duration-200 disabled:opacity-45 disabled:pointer-events-none active:scale-[0.98] select-none";

const variants: Record<Variant, string> = {
  primary: "bg-accent text-accent-ink active:brightness-110",
  secondary: "bg-surface-2 border border-line-strong active:bg-surface-3",
  ghost: "text-muted active:text-fg active:bg-surface-2",
  danger: "bg-danger-soft text-danger border border-danger/30 active:bg-danger/20",
};

const sizes = {
  md: "min-h-11 px-4 text-[15px]",
  lg: "min-h-[52px] px-5 text-base w-full",
};

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: keyof typeof sizes;
  busy?: boolean;
  icon?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, Props>(function Button(
  { variant = "secondary", size = "md", busy, icon, className = "", children, disabled, type = "button", ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      disabled={disabled || busy}
      className={`${base} ${variants[variant]} ${sizes[size]} ${className}`}
      {...rest}
    >
      {busy ? <Spinner size={16} /> : icon}
      {children}
    </button>
  );
});

/** Icon-only button with a 44px touch target. `label` is the accessible name. */
export const IconButton = forwardRef<HTMLButtonElement, Omit<Props, "children" | "size"> & { label: string }>(
  function IconButton({ label, icon, variant = "ghost", className = "", ...rest }, ref) {
    return (
      <Button
        ref={ref}
        variant={variant}
        aria-label={label}
        title={label}
        className={`!min-h-11 !min-w-11 !px-0 ${className}`}
        icon={icon}
        {...rest}
      />
    );
  },
);
