import type { AppLang } from "@shared/format";
import i18n, { applyLanguage, browserLang } from "../i18n";

/**
 * The PC is the single authority for the language. The phone never chooses one and never
 * reports one back, so there is nothing to loop. The last language the PC told us is cached
 * only so the very first paint after a reload is already right.
 */
const KEY = "vd.desktopLang";

export function parseLang(v: unknown): AppLang | null {
  return v === "en" || v === "ar" ? v : null;
}

export function cachedDesktopLang(): AppLang | null {
  try {
    return parseLang(localStorage.getItem(KEY));
  } catch {
    return null;
  }
}

/** The language to paint before any contact with the PC: its last choice, else the browser's. */
export function initialLang(): AppLang {
  return cachedDesktopLang() ?? browserLang();
}

/**
 * Adopts the language the PC announced. Unknown values are ignored. Only the i18n language and
 * `<html lang/dir>` change: the React tree is not remounted, so in-flight transfers are untouched.
 */
export async function followDesktopLanguage(value: unknown): Promise<void> {
  const lang = parseLang(value);
  if (!lang) return;
  try {
    localStorage.setItem(KEY, lang);
  } catch {
    /* storage unavailable: the language still applies for this page */
  }
  if (i18n.language !== lang || document.documentElement.lang !== lang) await applyLanguage(lang);
}
