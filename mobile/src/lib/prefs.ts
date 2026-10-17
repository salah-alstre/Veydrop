export type ThemeSetting = "system" | "light" | "dark";
/** Language is deliberately absent: the PC decides it (see lib/language.ts). */
export interface Prefs { theme: ThemeSetting }

const KEY = "vd.prefs";
const DEFAULTS: Prefs = { theme: "system" };

export function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS;
    const p = JSON.parse(raw) as Partial<Prefs>;
    return {
      theme: p.theme === "light" || p.theme === "dark" || p.theme === "system" ? p.theme : DEFAULTS.theme,
    };
  } catch {
    return DEFAULTS;
  }
}

export function savePrefs(p: Prefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    /* storage unavailable (private mode): the choice lasts for this page only */
  }
}

export function systemPrefersLight(): boolean {
  try {
    return typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: light)").matches;
  } catch {
    return false;
  }
}

/** Sets `data-theme` on <html> and keeps the browser chrome colour in step. */
export function applyTheme(theme: ThemeSetting): "light" | "dark" {
  const resolved = theme === "system" ? (systemPrefersLight() ? "light" : "dark") : theme;
  const root = document.documentElement;
  root.dataset.theme = resolved;
  const meta = document.querySelector('meta[name="theme-color"]');
  meta?.setAttribute("content", resolved === "light" ? "#f5f6f8" : "#050505");
  return resolved;
}
