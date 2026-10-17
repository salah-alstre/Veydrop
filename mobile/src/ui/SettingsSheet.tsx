import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Info, X } from "lucide-react";
import type { Prefs, ThemeSetting } from "../lib/prefs";
import { IconButton } from "./Button";

function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div>
      <div className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint mb-2">{label}</div>
      <div role="radiogroup" aria-label={label} className="grid grid-flow-col auto-cols-fr gap-1 p-1 rounded-[12px] bg-surface-2 border border-line">
        {options.map((o) => {
          const on = o.value === value;
          return (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => onChange(o.value)}
              className={`min-h-11 rounded-[9px] text-[15px] font-medium transition-colors duration-200 ${
                on ? "bg-surface-3 text-fg shadow-[0_0_0_1px_var(--vd-border-strong)]" : "text-muted"
              }`}
            >
              {o.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function SettingsSheet({
  open,
  prefs,
  onChange,
  onClose,
}: {
  open: boolean;
  prefs: Prefs;
  onChange: (p: Prefs) => void;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation();
  const reduce = useReducedMotion();
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const themes: { value: ThemeSetting; label: string }[] = [
    { value: "system", label: t("settings.themeSystem") },
    { value: "light", label: t("settings.themeLight") },
    { value: "dark", label: t("settings.themeDark") },
  ];

  return (
    <AnimatePresence>
      {open && (
        <div className="fixed inset-0 z-50" role="dialog" aria-modal="true" aria-label={t("settings.title")}>
          <motion.div
            className="absolute inset-0 bg-black/60"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduce ? 0 : 0.2 }}
            onClick={onClose}
          />
          <motion.div
            className="absolute inset-x-0 bottom-0 bg-surface border-t border-line-strong rounded-t-[20px] safe-x safe-bottom pt-4 pb-5"
            initial={{ y: reduce ? 0 : 40, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: reduce ? 0 : 40, opacity: 0 }}
            transition={{ duration: reduce ? 0 : 0.24, ease: [0.2, 0.7, 0.2, 1] }}
          >
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-lg font-semibold">{t("settings.title")}</h2>
              <IconButton ref={closeRef} label={t("common.close")} icon={<X size={20} />} onClick={onClose} />
            </div>
            <div className="space-y-5">
              <div>
                <div className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint mb-2">{t("settings.language")}</div>
                <div className="rounded-[12px] bg-surface-2 border border-line px-4 py-3">
                  <div className="font-medium" data-testid="language-current">{i18n.language === "ar" ? "العربية" : "English"}</div>
                  <p className="text-sm text-muted mt-1">{t("settings.languageFollows")}</p>
                </div>
              </div>
              <Segmented
                label={t("settings.theme")}
                value={prefs.theme}
                options={themes}
                onChange={(theme) => onChange({ ...prefs, theme })}
              />
              <p className="flex gap-2.5 text-sm text-muted leading-relaxed">
                <Info size={18} className="shrink-0 mt-0.5 text-warn" aria-hidden />
                <span>{t("settings.notice")}</span>
              </p>
            </div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}
