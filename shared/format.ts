// Locale-aware formatting shared by the desktop UI and the phone web app.
// Pure TypeScript: no package imports, so both apps can include it.

export type AppLang = "en" | "ar";

/** BCP-47 tag used for Intl. Arabic uses Arabic-Indic digits for localized numbers. */
export function intlLocale(lang: AppLang): string {
  return lang === "ar" ? "ar-u-nu-arab" : "en-US";
}

const UNITS = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte"] as const;

function nf(lang: AppLang, opts: Intl.NumberFormatOptions): Intl.NumberFormat {
  return new Intl.NumberFormat(intlLocale(lang), opts);
}

export function formatNumber(n: number, lang: AppLang): string {
  return nf(lang, {}).format(n);
}

/** Binary scaling (1024) like Windows Explorer, localized unit names via Intl. */
export function formatBytes(bytes: number, lang: AppLang): string {
  const b = Math.max(0, Number.isFinite(bytes) ? bytes : 0);
  let i = 0;
  let v = b;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = i === 0 || v >= 100 ? 0 : v >= 10 ? 1 : 2;
  // Intl's short English form for bytes is the word "byte"/"bytes", which reads oddly next to "kB"/"MB".
  if (i === 0 && lang === "en") return `${formatNumber(v, lang)} B`;
  return nf(lang, {
    style: "unit",
    unit: UNITS[i],
    unitDisplay: "short",
    maximumFractionDigits: digits,
    minimumFractionDigits: 0,
  }).format(v);
}

export function formatSpeed(bps: number, lang: AppLang): string {
  let i = 0;
  let v = Math.max(0, bps || 0);
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = i === 0 || v >= 100 ? 0 : 1;
  try {
    return nf(lang, {
      style: "unit",
      unit: `${UNITS[i]}-per-second`,
      unitDisplay: "short",
      maximumFractionDigits: digits,
    }).format(v);
  } catch {
    return `${formatBytes(bps, lang)}/s`;
  }
}

/** Clock style (m:ss or h:mm:ss) with localized digits. */
export function formatClock(totalSecs: number, lang: AppLang): string {
  const s = Math.max(0, Math.round(totalSecs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const two = nf(lang, { minimumIntegerDigits: 2, useGrouping: false });
  const one = nf(lang, { useGrouping: false });
  return h > 0
    ? `${one.format(h)}:${two.format(m)}:${two.format(sec)}`
    : `${one.format(m)}:${two.format(sec)}`;
}

export function formatPercent(fraction: number, lang: AppLang): string {
  const f = Math.min(1, Math.max(0, Number.isFinite(fraction) ? fraction : 0));
  return nf(lang, { style: "percent", maximumFractionDigits: 0 }).format(f);
}

export function formatDateTime(ms: number, lang: AppLang): string {
  return new Intl.DateTimeFormat(intlLocale(lang), {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(ms));
}

export function formatTime(ms: number, lang: AppLang): string {
  return new Intl.DateTimeFormat(intlLocale(lang), { timeStyle: "short" }).format(new Date(ms));
}

export function detectLang(candidates: readonly string[]): AppLang {
  for (const c of candidates) {
    const l = c.toLowerCase();
    if (l.startsWith("ar")) return "ar";
    if (l.startsWith("en")) return "en";
  }
  return "en";
}
