import { motion } from "motion/react";
import { useTranslation } from "react-i18next";
import { Logo } from "./Logo";

/** Shown while the first snapshot loads. */
export function Splash({ error }: { error?: string | null }) {
  const { t } = useTranslation();
  return (
    <div className="h-full w-full flex flex-col items-center justify-center gap-6 bg-bg">
      <motion.div initial={{ opacity: 0, scale: 0.92 }} animate={{ opacity: 1, scale: 1 }} transition={{ duration: 0.3 }}>
        <Logo size={96} animated />
      </motion.div>
      <div className="text-center">
        <div className="text-lg font-semibold tracking-tight">Veydrop</div>
        <div className="text-muted text-xs mt-1">{error ? t("splash.failed") : t("splash.tagline")}</div>
        {error && <div className="text-danger text-xs mt-3 max-w-xs selectable">{error}</div>}
      </div>
    </div>
  );
}
