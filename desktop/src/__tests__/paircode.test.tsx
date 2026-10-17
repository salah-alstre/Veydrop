import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { applyLanguage } from "../i18n";
import { PairCode } from "../ui/PairCode";

afterEach(async () => {
  cleanup();
  await applyLanguage("en");
});

describe("PairCode (desktop approval dialog)", () => {
  for (const lang of ["en", "ar"] as const) {
    it(`renders the code as ASCII digits, LTR, with leading zeros in ${lang}`, async () => {
      await applyLanguage(lang);
      for (const code of ["0042", "0000", "0907", "9999"]) {
        const { unmount } = render(<PairCode code={code} label="Verification code" />);
        const el = screen.getByTestId("pair-code");
        expect(el.textContent).toBe(code);
        expect(el.textContent).toMatch(/^[0-9]{4}$/);
        expect(el.textContent).not.toMatch(/[٠-٩۰-۹]/);
        expect(el.getAttribute("dir")).toBe("ltr");
        expect(el.className).toContain("pair-code");
        unmount();
      }
    });
  }
});
