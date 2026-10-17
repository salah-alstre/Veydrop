import { join } from "node:path";
import { writeFileSync } from "node:fs";
import { test, expect, pair } from "./fixtures";
import { listFiles, openSend, pickFiles, sha256File, startSending, writeRandomFile } from "./helpers";

test("sends one small file to the PC and the PC copy is byte-identical", async ({ page, pc }) => {
  const src = join(pc.fixtures.upload, "note.txt");
  writeFileSync(src, "phone says hi\n");
  await pair(page, pc);
  await openSend(page);
  await pickFiles(page, [src]);
  await expect(page.getByText("note.txt")).toBeVisible();
  await startSending(page);
  await expect(page.getByText("Sent and verified", { exact: true })).toBeVisible();
  await expect(page.getByText("All files sent and verified")).toBeVisible();
  const done = await pc.wait((e) => e.event === "transfer-done" && e.name === "note.txt");
  expect(done.state).toBe("completed");
  expect(await sha256File(join(pc.downloads, "note.txt"))).toBe(await sha256File(src));
});

test("sends several files including a multi-chunk binary, all verified by SHA-256", async ({ page, pc }) => {
  const a = join(pc.fixtures.upload, "a.bin");
  const b = join(pc.fixtures.upload, "b.txt");
  const c = join(pc.fixtures.upload, "empty.dat");
  await writeRandomFile(a, 9 * 1024 * 1024 + 123);
  writeFileSync(b, "bbb");
  writeFileSync(c, "");
  await pair(page, pc);
  await openSend(page);
  await pickFiles(page, [a, b, c]);
  await expect(page.getByText("Selected: 3")).toBeVisible();
  await startSending(page);
  await expect(page.getByText("All files sent and verified")).toBeVisible({ timeout: 40_000 });
  for (const p of [a, b, c]) {
    const name = p.split(/[\\/]/).pop()!;
    expect(await sha256File(join(pc.downloads, name))).toBe(await sha256File(p));
  }
  expect(listFiles(pc.downloads).sort()).toEqual(["a.bin", "b.txt", "empty.dat"]);
});

test("a file name that already exists on the PC gets a safe unique name, never overwriting", async ({ page, pc }) => {
  writeFileSync(join(pc.downloads, "dup.txt"), "original on the PC");
  const src = join(pc.fixtures.upload, "dup.txt");
  writeFileSync(src, "new from phone");
  await pair(page, pc);
  await openSend(page);
  await pickFiles(page, [src]);
  await startSending(page);
  await expect(page.getByText("All files sent and verified")).toBeVisible();
  const files = listFiles(pc.downloads);
  expect(files).toHaveLength(2);
  expect(await sha256File(join(pc.downloads, "dup.txt"))).not.toBe(await sha256File(src));
  const other = files.find((f) => f !== "dup.txt")!;
  expect(other).toMatch(/^dup.+\.txt$/);
  expect(await sha256File(join(pc.downloads, other))).toBe(await sha256File(src));
});

test("when the PC declines, nothing is written and the phone says so", async ({ page, pc }) => {
  pc.cmd("offers reject");
  const src = join(pc.fixtures.upload, "no.txt");
  writeFileSync(src, "nope");
  await pair(page, pc);
  await openSend(page);
  await pickFiles(page, [src]);
  await startSending(page);
  await expect(page.getByText("Your PC declined these files.")).toBeVisible();
  expect(listFiles(pc.downloads)).toEqual([]);
});

test("a file can be removed from the selection before sending", async ({ page, pc }) => {
  const a = join(pc.fixtures.upload, "keep.txt");
  const b = join(pc.fixtures.upload, "drop.txt");
  writeFileSync(a, "keep");
  writeFileSync(b, "drop");
  await pair(page, pc);
  await openSend(page);
  await pickFiles(page, [a, b]);
  await expect(page.getByText("Selected: 2")).toBeVisible();
  await page.getByRole("listitem").filter({ hasText: "drop.txt" }).getByRole("button", { name: "Remove" }).click();
  await expect(page.getByText("Selected: 1")).toBeVisible();
  await startSending(page);
  await expect(page.getByText("All files sent and verified")).toBeVisible();
  expect(listFiles(pc.downloads)).toEqual(["keep.txt"]);
});

test("cancelling an upload in progress leaves no finished file on the PC", async ({ page, pc }) => {
  test.setTimeout(180_000);
  const big = join(pc.fixtures.upload, "huge.bin");
  await writeRandomFile(big, 160 * 1024 * 1024);
  // Loopback is so fast that 160 MB finishes in about two seconds; hold every chunk back in the
  // browser so the upload is reliably still running when Cancel is clicked.
  await page.route("**/api/uploads/*", async (route) => {
    if (route.request().method() === "PUT") await new Promise((r) => setTimeout(r, 1500));
    await route.continue();
  });
  await pair(page, pc);
  await openSend(page);
  await pickFiles(page, [big]);
  await startSending(page);
  const row = page.getByRole("listitem").filter({ hasText: "huge.bin" });
  await expect(row.getByText("Sending")).toBeVisible({ timeout: 20_000 });
  await row.getByRole("button", { name: "Cancel" }).click();
  await expect(row.getByText("Cancelled")).toBeVisible();
  await expect.poll(() => listFiles(pc.downloads).filter((f) => f === "huge.bin")).toEqual([]);
  const done = pc.events.find((e) => e.event === "transfer-done" && e.name === "huge.bin");
  if (done) expect(done.state).not.toBe("completed");
});

test("pausing and resuming an upload still ends with a verified identical file", async ({ page, pc }) => {
  test.setTimeout(180_000);
  const big = join(pc.fixtures.upload, "resume.bin");
  await writeRandomFile(big, 48 * 1024 * 1024);
  // Hold each chunk back so the upload is reliably still running when Pause is clicked.
  await page.route("**/api/uploads/*", async (route) => {
    if (route.request().method() === "PUT") await new Promise((r) => setTimeout(r, 700));
    await route.continue();
  });
  await pair(page, pc);
  await openSend(page);
  await pickFiles(page, [big]);
  await startSending(page);
  const row = page.getByRole("listitem").filter({ hasText: "resume.bin" });
  await expect(row.getByText("Sending")).toBeVisible({ timeout: 20_000 });
  await row.getByRole("button", { name: "Pause" }).click();
  await expect(row.getByText("Paused")).toBeVisible();
  await row.getByRole("button", { name: "Resume" }).click();
  await expect(page.getByText("All files sent and verified")).toBeVisible({ timeout: 120_000 });
  expect(await sha256File(join(pc.downloads, "resume.bin"))).toBe(await sha256File(big));
});
