import { join } from "node:path";
import { mkdirSync, writeFileSync } from "node:fs";
import { test, expect, pair } from "./fixtures";
import { listFiles, openSend, sha256File, startSending, writeRandomFile } from "./helpers";

test("sending a folder keeps its structure on the PC", async ({ page, pc }) => {
  const root = join(pc.fixtures.upload, "Album");
  mkdirSync(join(root, "2024", "july"), { recursive: true });
  writeFileSync(join(root, "readme.txt"), "album");
  writeFileSync(join(root, "2024", "a.txt"), "aaa");
  await writeRandomFile(join(root, "2024", "july", "b.bin"), 1_500_000);
  await pair(page, pc);
  await openSend(page);
  await page.locator("input[webkitdirectory]").first().setInputFiles(root);
  await expect(page.getByText("Selected: 3")).toBeVisible();
  await startSending(page);
  await expect(page.getByText("All files sent and verified")).toBeVisible({ timeout: 40_000 });
  expect(listFiles(pc.downloads).sort()).toEqual(["Album/2024/a.txt", "Album/2024/july/b.bin", "Album/readme.txt"]);
  expect(await sha256File(join(pc.downloads, "Album", "2024", "july", "b.bin"))).toBe(
    await sha256File(join(root, "2024", "july", "b.bin")),
  );
});

test("the server refuses path traversal and absolute names in an offer", async ({ page, pc }) => {
  await pair(page, pc);
  for (const name of ["../escape.txt", "..\\escape.txt", "a/../../escape.txt", "C:\\Windows\\x.txt", "/etc/passwd"]) {
    const res = await page.evaluate(async (n) => {
      const r = await fetch("/api/offers", {
        method: "POST",
        headers: { "content-type": "application/json", "x-veydrop": "1" },
        body: JSON.stringify({ files: [{ clientId: "c1", name: n, relPath: n, size: 3 }] }),
      });
      return { status: r.status, body: await r.text() };
    }, name);
    // either rejected outright, or accepted with a sanitized name that stays inside the destination
    if (res.status >= 400) continue;
    await pc.wait((e) => e.event === "offer-decided");
  }
  await expect.poll(() => listFiles(pc.downloads).filter((f) => f.includes("escape"))).toEqual([]);
  const outside = join(pc.work, "escape.txt");
  await expect(sha256File(outside)).rejects.toThrow();
});
