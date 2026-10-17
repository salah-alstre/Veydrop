import { readFileSync } from "node:fs";
import { test, expect, pair } from "./fixtures";
import { sha256File } from "./helpers";

async function openReceive(page: import("@playwright/test").Page) {
  await page.getByRole("button", { name: /Receive from PC/ }).first().click();
  await page.getByRole("heading", { name: "Receive from PC" }).waitFor();
}

test("shows an empty state when the PC shares nothing", async ({ page, pc }) => {
  await pair(page, pc);
  await openReceive(page);
  await expect(page.getByText("Nothing shared yet")).toBeVisible();
});

test("lists only what the PC shares and downloads a file with identical bytes", async ({ page, pcSharing }) => {
  const pc = pcSharing;
  await pair(page, pc);
  await openReceive(page);
  await expect(page.getByText("hello.txt", { exact: true })).toBeVisible();
  await expect(page.getByText("big.bin", { exact: true })).toBeVisible();
  await expect(page.getByText("notes.txt", { exact: true })).toBeVisible();
  // nothing else on the PC's disk is offered
  await expect(page.getByText("Nothing shared yet")).toHaveCount(0);

  const [dl] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("link", { name: "Download: hello.txt" }).click(),
  ]);
  expect(dl.suggestedFilename()).toBe("hello.txt");
  const path = await dl.path();
  expect(await sha256File(path)).toBe(pc.fixtures.shared.smallSha);
});

test("a multi-chunk file downloads byte-identical to the PC original", async ({ page, pcSharing }) => {
  const pc = pcSharing;
  await pair(page, pc);
  await openReceive(page);
  const [dl] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("link", { name: "Download: big.bin" }).click(),
  ]);
  expect(await sha256File(await dl.path())).toBe(await sha256File(pc.fixtures.shared.big));
});

test("downloads everything as a ZIP that keeps the folder structure", async ({ page, pcSharing }) => {
  const pc = pcSharing;
  await pair(page, pc);
  await openReceive(page);
  const [dl] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("link", { name: "Download all as ZIP" }).click(),
  ]);
  const buf = readFileSync(await dl.path());
  expect(buf.subarray(0, 2).toString("latin1")).toBe("PK");
  const text = buf.toString("latin1");
  for (const name of ["hello.txt", "big.bin", "Trip/notes.txt", "Trip/day1/photo.bin"]) {
    expect(text).toContain(name);
  }
});

test("selected files can be downloaded as a ZIP containing only those files", async ({ page, pcSharing }) => {
  await pair(page, pcSharing);
  await openReceive(page);
  await page.getByRole("checkbox", { name: "hello.txt" }).check();
  await page.getByRole("checkbox", { name: "notes.txt" }).check();
  await expect(page.getByText("Selected: 2").last()).toBeVisible();
  const [dl] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("link", { name: "Download selected as ZIP" }).click(),
  ]);
  const text = readFileSync(await dl.path()).toString("latin1");
  expect(text).toContain("hello.txt");
  expect(text).toContain("notes.txt");
  expect(text).not.toContain("big.bin");
});

test("when the PC stops sharing, the list disappears and downloads are refused", async ({ page, pcSharing }) => {
  const pc = pcSharing;
  await pair(page, pc);
  await openReceive(page);
  await expect(page.getByText("hello.txt", { exact: true })).toBeVisible();
  const id = await page.evaluate(async () => {
    const r = await fetch("/api/share");
    return ((await r.json()) as { share: { items: { id: string; name: string }[] } }).share.items.find((i) => i.name === "hello.txt")!.id;
  });
  pc.cmd("share-off");
  await expect(page.getByText("The PC stopped sharing.")).toBeVisible();
  await expect(page.getByText("hello.txt", { exact: true })).toHaveCount(0);
  const status = await page.evaluate(async (i) => (await fetch(`/api/share/items/${i}/download`)).status, id);
  expect([403, 404, 409]).toContain(status);
});

test("a download without the session cookie is rejected", async ({ page, playwright, pcSharing }) => {
  const pc = pcSharing;
  await pair(page, pc);
  const id = await page.evaluate(async () => {
    const r = await fetch("/api/share");
    return ((await r.json()) as { share: { items: { id: string }[] } }).share.items[0].id;
  });
  const anon = await playwright.request.newContext({ baseURL: pc.origin });
  for (const p of [`/api/share/items/${id}/download`, "/api/share/zip", "/api/share"]) {
    expect((await anon.get(p)).status()).toBe(401);
  }
});
