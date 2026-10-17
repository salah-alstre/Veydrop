import { act, cleanup, render, screen } from "@testing-library/react";
import { useTranslation } from "react-i18next";
import { afterEach, describe, expect, it } from "vitest";
import i18n, { applyLanguage, resolveLang } from "../i18n";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";

function Probe() {
  const { t } = useTranslation();
  return <h1>{t("dashboard.status.connected", { n: "2" })}</h1>;
}

afterEach(async () => {
  cleanup();
  await applyLanguage("en");
});

describe("language switching", () => {
  it("switches text instantly and sets lang/dir on the document", async () => {
    await act(async () => { await applyLanguage("en"); });
    render(<Probe />);
    expect(screen.getByRole("heading").textContent).toBe(en.dashboard.status.connected.replace("{{n}}", "2"));
    expect(document.documentElement.dir).toBe("ltr");

    await act(async () => { await applyLanguage("ar"); });
    expect(screen.getByRole("heading").textContent).toBe(ar.dashboard.status.connected.replace("{{n}}", "2"));
    expect(document.documentElement.dir).toBe("rtl");
    expect(document.documentElement.lang).toBe("ar");

    await act(async () => { await applyLanguage("en"); });
    expect(screen.getByRole("heading").textContent).toBe(en.dashboard.status.connected.replace("{{n}}", "2"));
    expect(document.documentElement.dir).toBe("ltr");
    expect(document.documentElement.lang).toBe("en");
  });

  it("does not HTML-escape interpolated values", async () => {
    await applyLanguage("ar");
    expect(i18n.t("dashboard.status.connected", { n: "٢" })).toContain("٢");
  });

  it("resolves explicit settings and falls back for system", () => {
    expect(resolveLang("ar")).toBe("ar");
    expect(resolveLang("en")).toBe("en");
    expect(["en", "ar"]).toContain(resolveLang("system"));
  });
});
