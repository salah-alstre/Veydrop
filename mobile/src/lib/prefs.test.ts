import { beforeEach, describe, expect, it } from "vitest";
import { applyTheme, loadPrefs, savePrefs } from "./prefs";
import { describeDevice, getDeviceId, randomId } from "./device";

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("prefs", () => {
  it("defaults to the system theme and has no language preference", () => {
    expect(loadPrefs()).toEqual({ theme: "system" });
  });
  it("round-trips saved preferences", () => {
    savePrefs({ theme: "light" });
    expect(loadPrefs()).toEqual({ theme: "light" });
  });
  it("ignores corrupt or out-of-range stored values", () => {
    localStorage.setItem("vd.prefs", "{not json");
    expect(loadPrefs()).toEqual({ theme: "system" });
    localStorage.setItem("vd.prefs", JSON.stringify({ theme: "neon" }));
    expect(loadPrefs()).toEqual({ theme: "system" });
  });
  it("ignores a language stored by an older build: the PC decides the language", () => {
    localStorage.setItem("vd.prefs", JSON.stringify({ lang: "ar", theme: "dark" }));
    expect(loadPrefs()).toEqual({ theme: "dark" });
  });
  it("applyTheme sets data-theme and honours explicit choices", () => {
    expect(applyTheme("light")).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(applyTheme("dark")).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});

describe("device identity", () => {
  it("creates a stable id and reuses it", () => {
    const a = getDeviceId();
    expect(a).toMatch(/^[A-Za-z0-9_-]{8,64}$/);
    expect(getDeviceId()).toBe(a);
  });
  it("replaces a malformed stored id", () => {
    localStorage.setItem("vd.deviceId", "bad id!");
    expect(getDeviceId()).toMatch(/^[A-Za-z0-9_-]{8,64}$/);
  });
  it("randomId yields distinct ids of the requested length", () => {
    expect(randomId(16)).toHaveLength(16);
    expect(randomId()).not.toBe(randomId());
  });
  it("describes common phones from the user agent", () => {
    expect(describeDevice("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)", 5)).toEqual({ label: "iPhone", platform: "ios" });
    expect(describeDevice("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", 5)).toEqual({ label: "iPad", platform: "ios" });
    expect(describeDevice("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", 0)).toEqual({ label: "Mac", platform: "macos" });
    expect(describeDevice("Mozilla/5.0 (Linux; Android 14; Pixel 7 Build/UP1A) Chrome/120", 5)).toEqual({ label: "Pixel 7", platform: "android" });
    expect(describeDevice("Mozilla/5.0 (Linux; Android 14; K) Chrome/120", 5)).toEqual({ label: "Android", platform: "android" });
    expect(describeDevice("curl/8", 0)).toEqual({ label: "Phone", platform: "other" });
  });
});
