import { test as base, expect, type Page } from "@playwright/test";
import { Harness } from "../harness";

type Fx = {
  /** Engine + server started for this test, with nothing shared. */
  pc: Harness;
  /** Engine + server started for this test, with a file, a big file and a folder shared. */
  pcSharing: Harness;
};

export const test = base.extend<Fx>({
  pc: async ({}, use) => {
    const h = await Harness.start();
    await use(h);
    await h.stop();
  },
  pcSharing: async ({}, use) => {
    const h = await Harness.start({ share: true });
    await use(h);
    await h.stop();
  },
});

export { expect };

/** Opens the pairing link on the phone and waits until the landing screen shows. */
export async function pair(page: Page, pc: Harness) {
  await page.goto(pc.pairUrl);
  await expect(page.getByText(/^Connected to /).first()).toBeVisible();
}
