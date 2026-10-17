import { describe, expect, it } from "vitest";
import en from "./en.json";
import ar from "./ar.json";
import i18n, { applyLanguage, browserLang } from "./index";

function flatten(obj: Record<string, unknown>, prefix = ""): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object") Object.assign(out, flatten(v as Record<string, unknown>, key));
    else out[key] = String(v);
  }
  return out;
}

const placeholders = (s: string) => [...s.matchAll(/{{\s*(\w+)\s*}}/g)].map((m) => m[1]).sort();

describe("locale files", () => {
  const fe = flatten(en);
  const fa = flatten(ar);

  it("Arabic and English define exactly the same keys", () => {
    expect(Object.keys(fa).sort()).toEqual(Object.keys(fe).sort());
  });

  it("no string is empty", () => {
    for (const [k, v] of [...Object.entries(fe), ...Object.entries(fa)]) expect(v.trim(), k).not.toBe("");
  });

  it("interpolation placeholders match between languages", () => {
    for (const k of Object.keys(fe)) expect(placeholders(fa[k]), k).toEqual(placeholders(fe[k]));
  });

  it("Arabic strings are actually translated, not copies of English", () => {
    const same = Object.keys(fe).filter((k) => fe[k] === fa[k] && /[A-Za-z]{4,}/.test(fe[k]));
    // brand name / technical terms may legitimately match; everything else must differ
    const allowed = new Set(["app.name"]);
    expect(same.filter((k) => !allowed.has(k)), same.join(", ")).toEqual([]);
  });

  it("covers every error code the PC server and the phone client can produce", () => {
    const codes = [
      "io", "permission_denied", "path_not_found", "port_in_use", "invalid", "not_found", "conflict", "unauthorized",
      "forbidden", "rate_limited", "pairing_expired", "pairing_used", "server_not_running", "no_network",
      "insufficient_space", "too_large", "hash_mismatch", "cancelled", "offset_mismatch", "zip_too_large",
      "network", "file_changed", "other",
    ];
    // "too_many" is shown through send.tooMany (it carries the limit as a parameter)
    expect(fe["send.tooMany"]).toBeTruthy();
    expect(fa["send.tooMany"]).toBeTruthy();
    for (const c of codes) {
      expect(fe[`errors.${c}`], `en errors.${c}`).toBeTruthy();
      expect(fa[`errors.${c}`], `ar errors.${c}`).toBeTruthy();
    }
  });
});

describe("language switching", () => {
  it("switches text instantly and flips direction for Arabic", async () => {
    await applyLanguage("en");
    expect(document.documentElement.dir).toBe("ltr");
    expect(document.documentElement.lang).toBe("en");
    const enText = i18n.t("errors.network");
    await applyLanguage("ar");
    expect(document.documentElement.dir).toBe("rtl");
    expect(document.documentElement.lang).toBe("ar");
    expect(i18n.t("errors.network")).not.toBe(enText);
    await applyLanguage("en");
    expect(document.documentElement.dir).toBe("ltr");
  });

  it("browserLang picks Arabic for an Arabic browser and English otherwise (pre-pairing only)", () => {
    const ar = vi_languages(["ar-EG", "en"]);
    expect(browserLang()).toBe("ar");
    ar.restore();
    const fr = vi_languages(["fr-FR"]);
    expect(browserLang()).toBe("en");
    fr.restore();
  });
});

function vi_languages(list: string[]) {
  const langDesc = Object.getOwnPropertyDescriptor(Navigator.prototype, "languages");
  const oneDesc = Object.getOwnPropertyDescriptor(Navigator.prototype, "language");
  Object.defineProperty(navigator, "languages", { value: list, configurable: true });
  Object.defineProperty(navigator, "language", { value: list[0], configurable: true });
  return {
    restore() {
      delete (navigator as unknown as Record<string, unknown>).languages;
      delete (navigator as unknown as Record<string, unknown>).language;
      if (langDesc) Object.defineProperty(Navigator.prototype, "languages", langDesc);
      if (oneDesc) Object.defineProperty(Navigator.prototype, "language", oneDesc);
    },
  };
}
