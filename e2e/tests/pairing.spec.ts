import { test, expect, pair } from "./fixtures";

test("pairs after the PC approves, showing the 4-digit code on the phone", async ({ page, pc }) => {
  pc.cmd("approve never"); // the PC user has not decided yet
  await page.goto(pc.pairUrl);
  const reqEvt = await pc.wait((e) => e.event === "pair-request");
  await expect(page.getByText("Approve this phone on your PC")).toBeVisible();
  // the code on the phone is the same code the PC shows for approval
  await expect(page.getByTestId("pair-code")).toHaveText(String(reqEvt.code));
  expect(String(reqEvt.code)).toMatch(/^\d{4}$/);
  // the single-use token must not stay in the address bar
  expect(page.url()).not.toContain("t=");
  // the desktop user approves (the harness stands in for the click on "Approve")
  pc.cmd(`decide ${String(reqEvt.id)} approve`);
  await expect(page.getByText(/^Connected to /).first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Send to PC" }).or(page.getByText("Send to PC")).first()).toBeVisible();
  await expect(page.getByText("Receive from PC").first()).toBeVisible();
});

test("auto-approved pairing lands on the home screen", async ({ page, pc }) => {
  await pair(page, pc);
  await expect(page.getByText("Send to PC").first()).toBeVisible();
  await expect(page.getByText("Receive from PC").first()).toBeVisible();
});

test("a declined request is reported and does not grant access", async ({ page, pc }) => {
  pc.cmd("approve reject");
  await page.goto(pc.pairUrl);
  await expect(page.getByText("Request declined")).toBeVisible();
  // no session was created
  const r = await page.request.get(`${pc.origin}/api/share`);
  expect(r.status()).toBe(401);
});

test("a pairing link works only once", async ({ browser, pc }) => {
  const a = await (await browser.newContext()).newPage();
  await pair(a, pc);
  const b = await (await browser.newContext()).newPage();
  await b.goto(pc.pairUrl);
  await expect(b.getByText("QR code already used")).toBeVisible();
});

test("opening the site without a QR code asks for a scan and exposes nothing", async ({ page, pc }) => {
  await page.goto(pc.origin + "/");
  await expect(page.getByText("Scan the QR code on your PC")).toBeVisible();
  for (const path of ["/api/share", "/api/transfers", "/api/session/../share"]) {
    const r = await page.request.get(pc.origin + path);
    expect([401, 404]).toContain(r.status());
  }
});

test("a regenerated QR code invalidates the previous link", async ({ browser, pc }) => {
  const oldUrl = pc.pairUrl;
  await pc.regenerate();
  expect(pc.pairUrl).not.toBe(oldUrl);
  const page = await (await browser.newContext()).newPage();
  await page.goto(oldUrl);
  await expect(page.getByText(/QR code expired|QR code already used|Couldn't connect/)).toBeVisible();
  const page2 = await (await browser.newContext()).newPage();
  await page2.goto(pc.pairUrl);
  await expect(page2.getByText(/^Connected to /).first()).toBeVisible();
});

test("API rejects a forged Host header and cross-origin writes", async ({ playwright, pc }) => {
  const ctx = await playwright.request.newContext({ baseURL: pc.origin });
  const bad = await ctx.get("/api/info", { headers: { Host: "evil.example" } });
  expect(bad.status()).toBe(421);
  const cross = await ctx.post("/api/logout", { headers: { Origin: "http://evil.example", "x-veydrop": "1" } });
  expect([403, 421]).toContain(cross.status());
  const noHeader = await ctx.post("/api/logout");
  expect([400, 403]).toContain(noHeader.status());
});
