import { useTranslation } from "react-i18next";
import { formatBytes, formatClock, formatNumber, formatPercent, formatSpeed, type AppLang } from "@shared/format";

/** Active UI language plus number/size formatters bound to it. */
export function useFmt() {
  const { i18n } = useTranslation();
  const lang: AppLang = i18n.language === "ar" ? "ar" : "en";
  return {
    lang,
    num: (n: number) => formatNumber(n, lang),
    bytes: (n: number) => formatBytes(n, lang),
    speed: (n: number) => formatSpeed(n, lang),
    clock: (s: number) => formatClock(s, lang),
    percent: (f: number) => formatPercent(f, lang),
  };
}
