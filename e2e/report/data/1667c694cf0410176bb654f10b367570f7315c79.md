# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: receive.spec.ts >> selected files can be downloaded as a ZIP containing only those files
- Location: tests\receive.spec.ts:62:1

# Error details

```
Test timeout of 60000ms exceeded.
```

```
Error: page.waitForEvent: Test timeout of 60000ms exceeded.
=========================== logs ===========================
waiting for event "download"
============================================================
```

# Page snapshot

```yaml
- generic [ref=e2]:
  - status [ref=e3]:
    - generic [ref=e11]: Connection to the PC was lost. Retrying. Transfers continue where they stopped.
  - generic [ref=e13]:
    - generic [ref=e15]:
      - button "Back" [ref=e16]
      - heading "Receive from PC" [level=1] [ref=e19]
    - main [ref=e20]:
      - generic [ref=e21]:
        - paragraph [ref=e22]:
          - generic [ref=e25]:
            - generic [ref=e26]: About downloads.
            - text: On iPhone, Safari saves downloads to the Files app. Tap the download icon in the address bar to open them.
        - generic [ref=e27]:
          - heading "Files · 4" [level=2] [ref=e28]
          - generic [ref=e29]:
            - button "Select all" [ref=e30]
            - button "Select none" [ref=e31]
        - list [ref=e32]:
          - listitem [ref=e33]:
            - checkbox "hello.txt" [checked] [ref=e34]
            - generic [ref=e39]:
              - generic "hello.txt" [ref=e40]
              - generic [ref=e41]: 18 B
            - 'link "Download: hello.txt" [ref=e43]':
              - /url: /api/share/items/zyEtPPrqKJkR/download
          - listitem [ref=e47]:
            - checkbox "big.bin" [ref=e48]
            - generic [ref=e53]:
              - generic "big.bin" [ref=e54]
              - generic [ref=e55]: 6 MB
            - 'link "Download: big.bin" [ref=e57]':
              - /url: /api/share/items/y8cz8roJBbHY/download
          - listitem [ref=e61]:
            - checkbox "photo.bin" [ref=e62]
            - generic [ref=e67]:
              - generic "photo.bin" [ref=e68]
              - generic [ref=e69]: Trip/day1/photo.bin
              - generic [ref=e70]: 2 kB
            - 'link "Download: photo.bin" [ref=e72]':
              - /url: /api/share/items/zUTf1267-QJo/download
          - listitem [ref=e76]:
            - checkbox "notes.txt" [checked] [ref=e77]
            - generic [ref=e82]:
              - generic "notes.txt" [ref=e83]
              - generic [ref=e84]: Trip/notes.txt
              - generic [ref=e85]: 10 B
            - 'link "Download: notes.txt" [ref=e87]':
              - /url: /api/share/items/zcMeagixVlbm/download
    - generic [ref=e92]:
      - generic [ref=e93]:
        - generic [ref=e94]: "Selected: 2"
        - generic [ref=e95]: "Total size: 28 B"
      - link "Download selected as ZIP" [ref=e96]:
        - /url: /api/share/zip?ids=zyEtPPrqKJkR,zcMeagixVlbm
```

# Test source

```ts
  1   | import { readFileSync } from "node:fs";
  2   | import { test, expect, pair } from "./fixtures";
  3   | import { sha256File } from "./helpers";
  4   | 
  5   | async function openReceive(page: import("@playwright/test").Page) {
  6   |   await page.getByRole("button", { name: /Receive from PC/ }).first().click();
  7   |   await page.getByRole("heading", { name: "Receive from PC" }).waitFor();
  8   | }
  9   | 
  10  | test("shows an empty state when the PC shares nothing", async ({ page, pc }) => {
  11  |   await pair(page, pc);
  12  |   await openReceive(page);
  13  |   await expect(page.getByText("Nothing shared yet")).toBeVisible();
  14  | });
  15  | 
  16  | test("lists only what the PC shares and downloads a file with identical bytes", async ({ page, pcSharing }) => {
  17  |   const pc = pcSharing;
  18  |   await pair(page, pc);
  19  |   await openReceive(page);
  20  |   await expect(page.getByText("hello.txt", { exact: true })).toBeVisible();
  21  |   await expect(page.getByText("big.bin", { exact: true })).toBeVisible();
  22  |   await expect(page.getByText("notes.txt", { exact: true })).toBeVisible();
  23  |   // nothing else on the PC's disk is offered
  24  |   await expect(page.getByText("Nothing shared yet")).toHaveCount(0);
  25  | 
  26  |   const [dl] = await Promise.all([
  27  |     page.waitForEvent("download"),
  28  |     page.getByRole("link", { name: "Download: hello.txt" }).click(),
  29  |   ]);
  30  |   expect(dl.suggestedFilename()).toBe("hello.txt");
  31  |   const path = await dl.path();
  32  |   expect(await sha256File(path)).toBe(pc.fixtures.shared.smallSha);
  33  | });
  34  | 
  35  | test("a multi-chunk file downloads byte-identical to the PC original", async ({ page, pcSharing }) => {
  36  |   const pc = pcSharing;
  37  |   await pair(page, pc);
  38  |   await openReceive(page);
  39  |   const [dl] = await Promise.all([
  40  |     page.waitForEvent("download"),
  41  |     page.getByRole("link", { name: "Download: big.bin" }).click(),
  42  |   ]);
  43  |   expect(await sha256File(await dl.path())).toBe(await sha256File(pc.fixtures.shared.big));
  44  | });
  45  | 
  46  | test("downloads everything as a ZIP that keeps the folder structure", async ({ page, pcSharing }) => {
  47  |   const pc = pcSharing;
  48  |   await pair(page, pc);
  49  |   await openReceive(page);
  50  |   const [dl] = await Promise.all([
  51  |     page.waitForEvent("download"),
  52  |     page.getByRole("link", { name: "Download all as ZIP" }).click(),
  53  |   ]);
  54  |   const buf = readFileSync(await dl.path());
  55  |   expect(buf.subarray(0, 2).toString("latin1")).toBe("PK");
  56  |   const text = buf.toString("latin1");
  57  |   for (const name of ["hello.txt", "big.bin", "Trip/notes.txt", "Trip/day1/photo.bin"]) {
  58  |     expect(text).toContain(name);
  59  |   }
  60  | });
  61  | 
  62  | test("selected files can be downloaded as a ZIP containing only those files", async ({ page, pcSharing }) => {
  63  |   await pair(page, pcSharing);
  64  |   await openReceive(page);
  65  |   await page.getByRole("checkbox", { name: "hello.txt" }).check();
  66  |   await page.getByRole("checkbox", { name: "notes.txt" }).check();
  67  |   await expect(page.getByText("Selected: 2").last()).toBeVisible();
  68  |   const [dl] = await Promise.all([
> 69  |     page.waitForEvent("download"),
      |          ^ Error: page.waitForEvent: Test timeout of 60000ms exceeded.
  70  |     page.getByRole("link", { name: "Download selected as ZIP" }).click(),
  71  |   ]);
  72  |   const text = readFileSync(await dl.path()).toString("latin1");
  73  |   expect(text).toContain("hello.txt");
  74  |   expect(text).toContain("notes.txt");
  75  |   expect(text).not.toContain("big.bin");
  76  | });
  77  | 
  78  | test("when the PC stops sharing, the list disappears and downloads are refused", async ({ page, pcSharing }) => {
  79  |   const pc = pcSharing;
  80  |   await pair(page, pc);
  81  |   await openReceive(page);
  82  |   await expect(page.getByText("hello.txt", { exact: true })).toBeVisible();
  83  |   const id = await page.evaluate(async () => {
  84  |     const r = await fetch("/api/share");
  85  |     return ((await r.json()) as { share: { items: { id: string; name: string }[] } }).share.items.find((i) => i.name === "hello.txt")!.id;
  86  |   });
  87  |   pc.cmd("share-off");
  88  |   await expect(page.getByText("The PC stopped sharing.")).toBeVisible();
  89  |   await expect(page.getByText("hello.txt", { exact: true })).toHaveCount(0);
  90  |   const status = await page.evaluate(async (i) => (await fetch(`/api/share/items/${i}/download`)).status, id);
  91  |   expect([403, 404, 409]).toContain(status);
  92  | });
  93  | 
  94  | test("a download without the session cookie is rejected", async ({ page, playwright, pcSharing }) => {
  95  |   const pc = pcSharing;
  96  |   await pair(page, pc);
  97  |   const id = await page.evaluate(async () => {
  98  |     const r = await fetch("/api/share");
  99  |     return ((await r.json()) as { share: { items: { id: string }[] } }).share.items[0].id;
  100 |   });
  101 |   const anon = await playwright.request.newContext({ baseURL: pc.origin });
  102 |   for (const p of [`/api/share/items/${id}/download`, "/api/share/zip", "/api/share"]) {
  103 |     expect((await anon.get(p)).status()).toBe(401);
  104 |   }
  105 | });
  106 | 
```