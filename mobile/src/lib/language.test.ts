import { afterEach, beforeEach, describe, expect, it } from "vitest";
import i18n, { applyLanguage } from "../i18n";
import { cachedDesktopLang, followDesktopLanguage, initialLang, parseLang } from "./language";

beforeEach(async () => {
  localStorage.clear();
  await applyLanguage("en");
});
afterEach(async () => {
  await applyLanguage("en");
});

describe("desktop language sync (phone side)", () => {
  it("accepts only en and ar", () => {
    expect(parseLang("en")).toBe("en");
    expect(parseLang("ar")).toBe("ar");
    for (const bad of ["fr", "EN", "", null, undefined, 1, {}]) expect(parseLang(bad)).toBeNull();
  });

  it("switches to Arabic with RTL, then back to English with LTR, without a reload", async () => {
    await followDesktopLanguage("ar");
    expect(i18n.language).toBe("ar");
    expect(document.documentElement.lang).toBe("ar");
    expect(document.documentElement.dir).toBe("rtl");
    expect(i18n.t("common.close")).toBe(i18n.getFixedT("ar")("common.close"));

    await followDesktopLanguage("en");
    expect(i18n.language).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    expect(document.documentElement.dir).toBe("ltr");
  });

  it("ignores unknown values and keeps the current language", async () => {
    await followDesktopLanguage("ar");
    await followDesktopLanguage("klingon");
    await followDesktopLanguage(undefined);
    expect(i18n.language).toBe("ar");
    expect(document.documentElement.dir).toBe("rtl");
  });

  it("remembers the desktop language so a reload paints correctly before contacting the PC", async () => {
    expect(cachedDesktopLang()).toBeNull();
    await followDesktopLanguage("ar");
    expect(cachedDesktopLang()).toBe("ar");
    expect(initialLang()).toBe("ar");
    await followDesktopLanguage("en");
    expect(initialLang()).toBe("en");
  });

  it("the cached desktop language wins over the browser language", async () => {
    localStorage.setItem("vd.desktopLang", "en");
    const desc = Object.getOwnPropertyDescriptor(Navigator.prototype, "languages");
    Object.defineProperty(navigator, "languages", { value: ["ar-SA"], configurable: true });
    try {
      expect(initialLang()).toBe("en");
    } finally {
      delete (navigator as unknown as Record<string, unknown>).languages;
      if (desc) Object.defineProperty(Navigator.prototype, "languages", desc);
    }
  });

  it("ignores a corrupt cached value", () => {
    localStorage.setItem("vd.desktopLang", "zz");
    expect(cachedDesktopLang()).toBeNull();
  });
});
