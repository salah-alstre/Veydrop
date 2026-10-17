// Mobile README screenshots. These are rendered by a desktop Chromium in Pixel 7 style
// emulation against a local test engine: browser emulation, NOT a physical phone.
import { join, resolve } from "node:path";
import { writeFileSync } from "node:fs";
import { test, expect, pair } from "../tests/fixtures";
import { openSend, pickFiles } from "../tests/helpers";

const OUT = resolve(process.cwd(), "..", "docs", "screenshots");
const shot = (page: import("@playwright/test").Page, name: string) => page.screenshot({ path: join(OUT, name) });
const phone = (browser: import("@playwright/test").Browser, colorScheme: "dark" | "light" = "dark") =>
  browser.newContext({ colorScheme, viewport: { width: 412, height: 800 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });

test("mobile screenshots (dark)", async ({ browser, pcSharing: pc }) => {
  // Connection screen: the phone is waiting for the PC user to approve it (shows the code).
  const ctx = await phone(browser);
  const page = await ctx.newPage();
  pc.cmd("approve never");
  await page.goto(pc.pairUrl);
  const req = await pc.wait((e) => e.event === "pair-request");
  await expect(page.getByTestId("pair-code")).toHaveText(String(req.code));
  await page.waitForTimeout(600);
  await shot(page, "mobile-connect-dark.png");
  pc.cmd(`decide ${String(req.id)} approve`);
  await expect(page.getByText(/^Connected to /).first()).toBeVisible();
  await page.waitForTimeout(500);
  await shot(page, "mobile-home-dark.png");

  await page.getByRole("button", { name: /Receive from PC/ }).first().click();
  await expect(page.getByText("hello.txt", { exact: true })).toBeVisible();
  await page.waitForTimeout(900);
  await shot(page, "mobile-receive-dark.png");
  await page.getByRole("button", { name: "Back" }).click();

  const a = join(pc.fixtures.upload, "holiday-photo.jpg");
  const b = join(pc.fixtures.upload, "notes.txt");
  writeFileSync(a, Buffer.alloc(2_400_000, 7));
  writeFileSync(b, "meeting notes");
  await openSend(page);
  await pickFiles(page, [a, b]);
  await expect(page.getByText("Selected: 2")).toBeVisible();
  await page.waitForTimeout(900);
  await shot(page, "mobile-send-dark.png");

  // The PC switches to Arabic; the connected phone follows live, no reload, no re-pairing.
  await page.getByRole("button", { name: "Back" }).click();
  pc.cmd("lang ar");
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await page.waitForTimeout(700);
  await shot(page, "mobile-home-arabic-dark.png");
  await ctx.close();
});

test("mobile screenshots (light)", async ({ browser, pcSharing: pc }) => {
  const ctx = await phone(browser, "light");
  const page = await ctx.newPage();
  await pair(page, pc);
  await page.waitForTimeout(500);
  await shot(page, "mobile-home-light.png");
  await ctx.close();
});
