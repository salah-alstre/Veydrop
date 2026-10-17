import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import en from "../i18n/en.json";
import ar from "../i18n/ar.json";

type Tree = { [k: string]: string | Tree };

function flatten(tree: Tree, prefix = ""): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(tree)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "string") out[key] = v;
    else Object.assign(out, flatten(v, key));
  }
  return out;
}

const flatEn = flatten(en as Tree);
const flatAr = flatten(ar as Tree);

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return n === "__tests__" ? [] : walk(p);
    return /\.(ts|tsx)$/.test(n) ? [p] : [];
  });
}

const SRC = resolve(__dirname, "..");
const used = new Set<string>();
const dynamicPrefixes = new Set<string>();
for (const file of walk(SRC)) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/\bt\(\s*["']([\w.]+)["']/g)) used.add(m[1]);
  for (const m of text.matchAll(/\bt\(\s*`([\w.]+\.)\$\{/g)) dynamicPrefixes.add(m[1]);
}

const placeholders = (s: string) => [...s.matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map((m) => m[1]).sort();

describe("translations", () => {
  it("English and Arabic have identical key sets", () => {
    expect(Object.keys(flatAr).sort()).toEqual(Object.keys(flatEn).sort());
  });

  it("has no empty strings", () => {
    for (const [k, v] of [...Object.entries(flatEn), ...Object.entries(flatAr)]) {
      expect(v.trim().length, k).toBeGreaterThan(0);
    }
  });

  it("uses the same interpolation variables in both languages", () => {
    for (const k of Object.keys(flatEn)) {
      expect(placeholders(flatAr[k]), k).toEqual(placeholders(flatEn[k]));
    }
  });

  it("every literal t() key used in the source exists in both languages", () => {
    expect(used.size).toBeGreaterThan(50);
    const missing = [...used].filter((k) => !(k in flatEn) || !(k in flatAr));
    expect(missing).toEqual([]);
  });

  it("every dynamic key family has entries in both languages", () => {
    for (const p of dynamicPrefixes) {
      expect(Object.keys(flatEn).some((k) => k.startsWith(p)), p).toBe(true);
      expect(Object.keys(flatAr).some((k) => k.startsWith(p)), p).toBe(true);
    }
  });

  it("covers every backend error code and transfer state", () => {
    const codes = ["io", "permission_denied", "path_not_found", "port_in_use", "invalid", "not_found", "conflict",
      "unauthorized", "forbidden", "rate_limited", "pairing_expired", "pairing_used", "server_not_running",
      "no_network", "insufficient_space", "too_large", "hash_mismatch", "cancelled", "offset_mismatch",
      "zip_too_large", "other"];
    for (const c of codes) {
      expect(flatEn[`errors.${c}`], c).toBeTruthy();
      expect(flatAr[`errors.${c}`], c).toBeTruthy();
    }
    const states = ["pending", "queued", "active", "paused", "interrupted", "finalizing", "completed", "failed", "cancelled", "rejected"];
    for (const s of states) {
      expect(flatEn[`state.${s}`], s).toBeTruthy();
      expect(flatAr[`state.${s}`], s).toBeTruthy();
    }
  });

  it("Arabic strings contain Arabic script", () => {
    const untranslated = Object.keys(flatAr).filter((k) => !/[؀-ۿ]/.test(flatAr[k]));
    // Allow strings that are purely brand/technical (e.g. a bare product name or URL).
    expect(untranslated.filter((k) => flatAr[k] !== flatEn[k])).toEqual([]);
    expect(untranslated.length).toBeLessThan(5);
  });
});
