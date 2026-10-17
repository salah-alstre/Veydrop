import { describe, expect, it } from "vitest";
import {
  detectLang, formatBytes, formatClock, formatNumber, formatPercent, formatSpeed,
} from "@shared/format";

describe("formatters", () => {
  it("formats bytes with binary scaling", () => {
    expect(formatBytes(13, "en")).toBe("13 B");
    expect(formatBytes(1024, "en")).toMatch(/^1\s?kB$/);
    expect(formatBytes(5 * 1024 * 1024, "en")).toMatch(/^5\s?MB$/);
    expect(formatBytes(1.5 * 1024 ** 3, "en")).toMatch(/^1\.5\s?GB$/);
  });

  it("clamps invalid sizes to zero", () => {
    expect(formatBytes(-5, "en")).toBe(formatBytes(0, "en"));
    expect(formatBytes(Number.NaN, "en")).toBe(formatBytes(0, "en"));
  });

  it("uses Arabic-Indic digits for Arabic", () => {
    expect(formatNumber(1234, "ar")).toMatch(/[٠-٩]/);
    expect(formatNumber(1234, "ar")).not.toMatch(/[0-9]/);
    expect(formatBytes(2048, "ar")).not.toMatch(/[0-9]/);
    expect(formatPercent(0.5, "ar")).not.toMatch(/[0-9]/);
  });

  it("formats clocks", () => {
    expect(formatClock(65, "en")).toBe("1:05");
    expect(formatClock(3725, "en")).toBe("1:02:05");
    expect(formatClock(-3, "en")).toBe("0:00");
    expect(formatClock(65, "ar")).not.toMatch(/[0-9]/);
  });

  it("formats percent within bounds", () => {
    expect(formatPercent(0.256, "en")).toBe("26%");
    expect(formatPercent(7, "en")).toBe("100%");
    expect(formatPercent(-1, "en")).toBe("0%");
  });

  it("formats speed", () => {
    expect(formatSpeed(10 * 1024 * 1024, "en")).toMatch(/10\s?MB\/s/);
  });

  it("detects language", () => {
    expect(detectLang(["ar-SA", "en-US"])).toBe("ar");
    expect(detectLang(["fr-FR", "en-GB"])).toBe("en");
    expect(detectLang(["de"])).toBe("en");
    expect(detectLang([])).toBe("en");
  });
});
