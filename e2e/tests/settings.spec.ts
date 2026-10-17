import { test, expect, pair } from "./fixtures";

test("the phone has no language picker of its own: it shows the language chosen on the PC", async ({ page, pc }) => {
  await pair(page, pc);
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
  await expect(page.getByTestId("language-current")).toHaveText("English");
  await expect(page.getByText("Follows the language chosen in Veydrop on your PC.")).toBeVisible();
  await expect(page.getByRole("radio", { name: "العربية" })).toHaveCount(0);
  await expect(page.getByRole("radio", { name: "English" })).toHaveCount(0);
});

test("the theme choice is applied and remembered", async ({ page, pc }) => {
  await pair(page, pc);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("radio", { name: "Light" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("radio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("the settings sheet states that the connection is not encrypted", async ({ page, pc }) => {
  await pair(page, pc);
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByText(/plain HTTP on your local network/)).toBeVisible();
});

test("no horizontal overflow on the phone viewport in either direction", async ({ page, pc }) => {
  await pair(page, pc);
  const over = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(await over()).toBeLessThanOrEqual(0);
  pc.cmd("lang ar");
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  expect(await over()).toBeLessThanOrEqual(0);
});
