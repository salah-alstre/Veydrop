import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { detectLang, type AppLang } from "@shared/format";
import en from "./en.json";
import ar from "./ar.json";
import type { LangSetting } from "../lib/types";

export const resources = { en: { translation: en }, ar: { translation: ar } } as const;

export function resolveLang(setting: LangSetting): AppLang {
  if (setting === "en" || setting === "ar") return setting;
  const nav = typeof navigator !== "undefined"
    ? [...(navigator.languages ?? []), navigator.language].filter(Boolean)
    : [];
  return detectLang(nav);
}

/** Switches language instantly and flips document direction (RTL for Arabic). */
export async function applyLanguage(lang: AppLang) {
  await i18n.changeLanguage(lang);
  const root = document.documentElement;
  root.lang = lang;
  root.dir = lang === "ar" ? "rtl" : "ltr";
}

void i18n.use(initReactI18next).init({
  resources,
  lng: resolveLang("system"),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
