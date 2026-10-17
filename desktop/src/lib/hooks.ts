import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  formatBytes, formatClock, formatDateTime, formatNumber, formatPercent, formatSpeed, formatTime,
  type AppLang,
} from "@shared/format";
import { ipc } from "./ipc";
import type { CmdError, ThemeSetting } from "./types";

export function useLang(): AppLang {
  const { i18n } = useTranslation();
  return i18n.language === "ar" ? "ar" : "en";
}

/** Formatters bound to the active language (localized digits, units, dates). */
export function useFmt() {
  const lang = useLang();
  return useMemo(() => ({
    lang,
    num: (n: number) => formatNumber(n, lang),
    bytes: (n: number) => formatBytes(n, lang),
    speed: (n: number) => formatSpeed(n, lang),
    clock: (s: number) => formatClock(s, lang),
    percent: (f: number) => formatPercent(f, lang),
    dateTime: (ms: number) => formatDateTime(ms, lang),
    time: (ms: number) => formatTime(ms, lang),
  }), [lang]);
}

/** Applies data-theme and keeps the native title bar in sync; follows the OS for "system". */
export function useThemeSync(setting: ThemeSetting) {
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      const eff = setting === "system" ? (mq.matches ? "light" : "dark") : setting;
      document.documentElement.dataset.theme = eff;
    };
    apply();
    ipc.setWindowTheme(setting === "system" ? null : setting).catch(() => {});
    if (setting !== "system") return;
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [setting]);
}

/** Translates a backend {code} error into the current language. */
export function useErrorText() {
  const { t, i18n } = useTranslation();
  return (e: unknown): string => {
    const err = e as Partial<CmdError>;
    const code = err?.code ?? "other";
    const key = `errors.${code}`;
    return i18n.exists(key) ? t(key) : t("errors.other");
  };
}

/** Re-renders on an interval (countdowns); no timer while inactive. */
export function useTick(intervalMs: number, active = true): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    if (!active) return;
    const id = setInterval(() => setN((x) => x + 1), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs, active]);
  return n;
}
