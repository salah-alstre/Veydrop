import { AnimatePresence, motion } from "motion/react";
import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { dismissToast, useApp } from "../lib/store";

const icons = {
  info: <Info size={16} className="text-accent-2" />,
  success: <CheckCircle2 size={16} className="text-success" />,
  error: <AlertCircle size={16} className="text-danger" />,
};

export function Toaster() {
  const { toasts } = useApp();
  const { t } = useTranslation();
  return (
    <div className="fixed bottom-4 end-4 z-[60] flex flex-col gap-2 w-80 pointer-events-none" aria-live="polite">
      <AnimatePresence initial={false}>
        {toasts.map((x) => (
          <motion.div
            key={x.id}
            layout
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97 }}
            transition={{ duration: 0.2 }}
            className="vd-card pointer-events-auto flex items-start gap-3 p-3 pe-2 shadow-xl"
            role={x.kind === "error" ? "alert" : "status"}
          >
            <span className="mt-0.5 shrink-0">{icons[x.kind]}</span>
            <span className="text-sm flex-1 selectable">{x.text}</span>
            <button
              type="button"
              aria-label={t("common.dismiss")}
              onClick={() => dismissToast(x.id)}
              className="size-6 grid place-items-center rounded-md text-faint hover:text-fg hover:bg-surface-2"
            >
              <X size={14} />
            </button>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
