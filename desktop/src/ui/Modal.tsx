import { useEffect, useRef, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";

interface Props {
  open: boolean;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  icon?: ReactNode;
  /** Escape / backdrop dismiss. Omit for decisions that must be answered. */
  onClose?: () => void;
  width?: number;
}

export function Modal({ open, title, children, footer, icon, onClose, width = 440 }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      prev?.focus?.();
    };
  }, [open, onClose]);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 grid place-items-center bg-black/60 p-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.18 }}
          onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}
        >
          <motion.div
            ref={ref}
            tabIndex={-1}
            role="dialog"
            aria-modal="true"
            aria-label={title}
            className="vd-card w-full outline-none shadow-2xl max-h-full overflow-auto"
            style={{ maxWidth: width }}
            initial={{ opacity: 0, y: 10, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.99 }}
            transition={{ duration: 0.2, ease: [0.2, 0.7, 0.2, 1] }}
          >
            <div className="p-6">
              <div className="flex items-center gap-3 mb-3">
                {icon && <div className="size-9 rounded-xl bg-accent-soft text-accent grid place-items-center shrink-0">{icon}</div>}
                <h2 className="text-lg font-semibold tracking-tight">{title}</h2>
              </div>
              <div className="text-sm text-muted">{children}</div>
            </div>
            {footer && <div className="flex flex-wrap justify-end gap-2 px-6 py-4 border-t border-line">{footer}</div>}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
