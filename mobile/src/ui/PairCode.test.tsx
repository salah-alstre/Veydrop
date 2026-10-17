import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PairCode } from "./PairCode";
import i18n, { applyLanguage } from "../i18n";

describe("PairCode", () => {
  for (const lang of ["en", "ar"] as const) {
    it(`shows ASCII Western digits unchanged in ${lang}, including leading zeros`, async () => {
      await applyLanguage(lang);
      for (const code of ["0042", "0000", "9017", "1234"]) {
        const { unmount } = render(<PairCode code={code} label="code" />);
        const el = screen.getByTestId("pair-code");
        expect(el.textContent).toBe(code);
        expect(el.textContent).toMatch(/^[0-9]{4}$/);
        expect(el.getAttribute("dir")).toBe("ltr");
        expect(el.getAttribute("lang")).toBe("en");
        unmount();
      }
      expect(i18n.language).toBe(lang);
    });
  }

  it("never contains Arabic-Indic digits", async () => {
    await applyLanguage("ar");
    render(<PairCode code="0123" />);
    expect(screen.getByTestId("pair-code").textContent).not.toMatch(/[\u0660-\u0669\u06F0-\u06F9]/);
  });
});
