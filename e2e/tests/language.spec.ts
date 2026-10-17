import { join } from "node:path";
import { test, expect, pair } from "./fixtures";
import { openSend, pickFiles, sha256File, startSending, writeRandomFile } from "./helpers";

const html = (page: import("@playwright/test").Page) => page.locator("html");

test("Arabic on the PC: the whole pairing flow on the phone is Arabic, RTL, with Western-digit code", async ({ page, pc }) => {
  pc.cmd("lang ar");
  pc.cmd("approve never");
  await page.goto(pc.pairUrl);
  const req = await pc.wait((e) => e.event === "pair-request");
  await expect(page.getByText("وافق على هذا الهاتف من الكمبيوتر")).toBeVisible();
  await expect(html(page)).toHaveAttribute("dir", "rtl");
  await expect(html(page)).toHaveAttribute("lang", "ar");
  const code = page.getByTestId("pair-code");
  await expect(code).toHaveText(String(req.code));
  expect(String(req.code)).toMatch(/^[0-9]{4}$/);
  expect(await code.evaluate((el) => getComputedStyle(el).direction)).toBe("ltr");
  // still English-free: no latin sentence on the waiting screen
  pc.cmd(`decide ${String(req.id)} approve`);
  await expect(page.getByText(/^متصل بـ /).first()).toBeVisible();
  await expect(page.getByText("إرسال إلى الكمبيوتر").first()).toBeVisible();
  await expect(page.getByText("Send to PC")).toHaveCount(0);
});

test("English on the PC wins over an Arabic phone browser", async ({ browser, pc }) => {
  const ctx = await browser.newContext({ locale: "ar-SA" });
  const page = await ctx.newPage();
  await pair(page, pc);
  await expect(html(page)).toHaveAttribute("dir", "ltr");
  await expect(html(page)).toHaveAttribute("lang", "en");
  await ctx.close();
});

test("Arabic on the PC wins over an English phone browser", async ({ browser, pc }) => {
  pc.cmd("lang ar");
  const ctx = await browser.newContext({ locale: "en-US" });
  const page = await ctx.newPage();
  await page.goto(pc.pairUrl);
  await expect(page.getByText(/^متصل بـ /).first()).toBeVisible();
  await expect(html(page)).toHaveAttribute("dir", "rtl");
  await ctx.close();
});

test("changing the PC language updates a connected phone live, both ways, without any reload", async ({ page, pc }) => {
  await pair(page, pc);
  await page.evaluate(() => ((window as unknown as { __alive: number }).__alive = 1));
  const alive = () => page.evaluate(() => (window as unknown as { __alive?: number }).__alive);
  const navs = () => page.evaluate(() => performance.getEntriesByType("navigation").length);
  const navsBefore = await navs();

  pc.cmd("lang ar");
  await expect(html(page)).toHaveAttribute("dir", "rtl");
  await expect(html(page)).toHaveAttribute("lang", "ar");
  await expect(page.getByText("إرسال إلى الكمبيوتر").first()).toBeVisible();
  await expect(page.getByText("استلام من الكمبيوتر").first()).toBeVisible();

  pc.cmd("lang en");
  await expect(html(page)).toHaveAttribute("dir", "ltr");
  await expect(page.getByText("Send to PC").first()).toBeVisible();

  pc.cmd("lang ar");
  await expect(html(page)).toHaveAttribute("dir", "rtl");

  // same document the whole time: no refresh, no new page load, still paired
  expect(await alive()).toBe(1);
  expect(await navs()).toBe(navsBefore);
  const r = await page.request.get(`${pc.origin}/api/session`);
  expect(r.status()).toBe(200);
});

test("the open settings sheet follows the PC language and names it", async ({ page, pc }) => {
  await pair(page, pc);
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByTestId("language-current")).toHaveText("English");
  pc.cmd("lang ar");
  await expect(page.getByRole("dialog", { name: "الإعدادات" })).toBeVisible();
  await expect(page.getByTestId("language-current")).toHaveText("العربية");
});

test("a reloaded or reconnecting phone restores the language chosen on the PC", async ({ page, pc }) => {
  await pair(page, pc);
  pc.cmd("lang ar");
  await expect(html(page)).toHaveAttribute("dir", "rtl");
  // forget everything the phone cached: the language must come back from the PC alone
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await expect(html(page)).toHaveAttribute("dir", "rtl");
  await expect(page.getByText(/^متصل بـ /).first()).toBeVisible();
  // and with the cache intact the very next load is already right
  await page.reload();
  await expect(html(page)).toHaveAttribute("dir", "rtl");
});

test("switching the PC language mid-upload neither interrupts nor corrupts the transfer", async ({ page, pc }) => {
  const src = join(pc.fixtures.upload, "mid-switch.bin");
  await writeRandomFile(src, 40 * 1024 * 1024);
  await pair(page, pc);
  await page.evaluate(() => ((window as unknown as { __alive: number }).__alive = 1));
  await openSend(page);
  await pickFiles(page, [src]);
  await startSending(page);
  pc.cmd("lang ar");
  await expect(html(page)).toHaveAttribute("dir", "rtl");
  pc.cmd("lang en");
  await expect(html(page)).toHaveAttribute("dir", "ltr");
  pc.cmd("lang ar");
  const done = await pc.wait((e) => e.event === "transfer-done" && e.name === "mid-switch.bin", 60_000);
  expect(done.state).toBe("completed");
  expect(await sha256File(join(pc.downloads, "mid-switch.bin"))).toBe(await sha256File(src));
  expect(await page.evaluate(() => (window as unknown as { __alive?: number }).__alive)).toBe(1);
  await expect(html(page)).toHaveAttribute("dir", "rtl");
});
