import { createHash, randomBytes } from "node:crypto";
import { createReadStream, createWriteStream, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";

export function sha256File(path: string): Promise<string> {
  return new Promise((res, rej) => {
    const h = createHash("sha256");
    createReadStream(path)
      .on("data", (d) => h.update(d))
      .on("end", () => res(h.digest("hex")))
      .on("error", rej);
  });
}

/** All regular files under `dir`, as paths relative to it (forward slashes). */
export function listFiles(dir: string, rel = ""): string[] {
  const out: string[] = [];
  for (const name of readdirSync(join(dir, rel))) {
    const r = rel ? `${rel}/${name}` : name;
    if (statSync(join(dir, r)).isDirectory()) out.push(...listFiles(dir, r));
    else out.push(r);
  }
  return out;
}

export async function writeRandomFile(path: string, bytes: number): Promise<void> {
  const out = createWriteStream(path);
  let left = bytes;
  while (left > 0) {
    const n = Math.min(left, 4 * 1024 * 1024);
    if (!out.write(randomBytes(n))) await new Promise((r) => out.once("drain", r));
    left -= n;
  }
  await new Promise<void>((r) => out.end(r));
}

export async function openSend(page: Page) {
  await page.getByRole("button", { name: /Send to PC/ }).first().click();
  await page.getByRole("heading", { name: "Send to PC" }).waitFor();
}

export async function pickFiles(page: Page, paths: string[]) {
  await page.locator('input[type="file"]:not([webkitdirectory])').first().setInputFiles(paths);
}

export async function startSending(page: Page) {
  await page.getByRole("button", { name: "Send to PC", exact: true }).last().click();
}
