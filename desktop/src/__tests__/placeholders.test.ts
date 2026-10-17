import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";

const SRC = join(__dirname, "..");

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return n === "__tests__" ? [] : files(p);
    return /\.tsx?$/.test(n) ? [p] : [];
  });
}

const lookup = (tree: unknown, key: string): unknown =>
  key.split(".").reduce<unknown>((o, k) => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined), tree);

describe("translation placeholders", () => {
  it("a t(\"key\") call without values never resolves to a string that has {{placeholders}}", () => {
    const bad: string[] = [];
    for (const f of files(SRC)) {
      const text = readFileSync(f, "utf8");
      for (const m of text.matchAll(/\bt\(\s*"([A-Za-z0-9_.]+)"\s*\)/g)) {
        for (const [lang, tree] of [["en", en], ["ar", ar]] as const) {
          const v = lookup(tree, m[1]);
          if (typeof v === "string" && /\{\{\s*\w+\s*\}\}/.test(v)) bad.push(`${lang}:${m[1]} in ${f}`);
        }
      }
    }
    expect(bad).toEqual([]);
  });
});
