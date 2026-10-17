import { test, expect, pair } from "./fixtures";

test("revoking the phone on the PC ends the session on the phone and locks the API", async ({ page, pc }) => {
  await pair(page, pc);
  pc.cmd("revoke");
  await expect(page.getByText("Connection ended")).toBeVisible();
  const status = await page.evaluate(async () => (await fetch("/api/share")).status);
  expect(status).toBe(401);
});

test("stopping the server on the PC ends the session on the phone", async ({ page, pc }) => {
  await pair(page, pc);
  pc.cmd("stop");
  await expect(page.getByText(/Connection ended|Connection to the PC was lost/)).toBeVisible({ timeout: 20_000 });
});

test("disconnecting from the phone asks first, then ends the session", async ({ page, pc }) => {
  await pair(page, pc);
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible();
  // backing out keeps the session
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByText(/^Connected to /).first()).toBeVisible();
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Disconnect" }).click();
  await expect(page.getByText("Connection ended")).toBeVisible();
  const status = await page.evaluate(async () => (await fetch("/api/share")).status);
  expect(status).toBe(401);
});

test("a reload keeps the session through the cookie, without the pairing link", async ({ page, pc }) => {
  await pair(page, pc);
  await page.goto(pc.origin + "/");
  await expect(page.getByText(/^Connected to /).first()).toBeVisible();
});

test("the session cookie is HttpOnly and unreadable from page scripts", async ({ page, pc, browserName }) => {
  await pair(page, pc);
  expect(await page.evaluate(() => document.cookie)).not.toContain("vd_s");
  const cookies = await page.context().cookies();
  const c = cookies.find((x) => x.name === "vd_s");
  expect(c?.httpOnly).toBe(true);
  // Playwright's WebKit build reports every cookie's sameSite as "None" through this API; the
  // SameSite=Strict attribute itself is asserted on the raw Set-Cookie header in the Rust integration tests.
  if (browserName !== "webkit") expect(c?.sameSite).toBe("Strict");
});
