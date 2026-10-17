import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../i18n";
import type { Diagnostics } from "../lib/types";

const diagnostics = vi.fn();
vi.mock("../lib/ipc", () => ({
  ipc: {
    diagnostics: (...a: unknown[]) => diagnostics(...a),
    updateSettings: vi.fn(async () => ({})),
    restartServer: vi.fn(async () => ({})),
  },
}));

import { DiagnosticsPanel } from "../pages/DiagnosticsPanel";

const iface = { name: "Ethernet 2", ip: "10.0.0.25", kind: "ethernet" as const, isPrivate: true, isVirtual: false, score: 90, recommended: true };

function mk(over: Partial<Diagnostics> = {}): Diagnostics {
  return {
    checkedAt: Date.now(), deep: true, running: true, bind: "10.0.0.25:47821",
    interface: iface, recommended: iface, interfaceCount: 1, addressLost: false,
    local: { ok: true, url: "http://10.0.0.25:47821/", status: 200, millis: 3, pageOk: true },
    external: { reached: false, requests: 0 },
    windows: { checked: true, alias: "Network", category: "Public", firewallEnabled: true, defaultInbound: "Block" },
    firewall: { state: "no_rule", profile: "Public", matchingAllow: [], matchingBlock: [], exe: "C:\\app\\veydrop.exe", suggestedCommand: "New-NetFirewallRule -DisplayName \"Veydrop\"" },
    pairing: { active: true, used: false, pendingRequests: 0, sessions: 0 },
    phoneSeen: false, pairedPhones: 0, warnings: ["public_profile", "firewall_no_rule", "not_reached"],
    ...over,
  };
}

beforeEach(async () => { await applyLanguage("en"); diagnostics.mockReset(); });
afterEach(cleanup);

const renderPanel = (d: Diagnostics | null, onDiag = vi.fn()) =>
  render(<DiagnosticsPanel diag={d} network={[iface]} preferredIp="" pairedPhones={0} onDiag={onDiag} />);

describe("DiagnosticsPanel", () => {
  it("does not treat a passing self-check as proof of phone reachability", () => {
    renderPanel(mk());
    expect(screen.getByText(/never passes the firewall/i)).toBeTruthy();
    expect(screen.getByText("No other device has reached the server yet")).toBeTruthy();
    expect(screen.queryByText(/Reached by/)).toBeNull();
  });

  it("shows external reach only when another device actually connected", () => {
    renderPanel(mk({ external: { reached: true, requests: 4, lastIp: "10.0.0.4", lastAt: Date.now() }, warnings: [] }));
    expect(screen.getByText(/Reached by 10\.0\.0\.4 \(4 requests\)/)).toBeTruthy();
  });

  it("offers a copy-only firewall command and never runs anything itself", () => {
    renderPanel(mk());
    expect(screen.getByText(/never changes firewall or network settings/i)).toBeTruthy();
    expect(screen.getByText(/New-NetFirewallRule/)).toBeTruthy();
    expect(diagnostics).not.toHaveBeenCalled();
  });

  it("hides the firewall command when a rule already allows traffic", () => {
    renderPanel(mk({ firewall: { state: "allowed", matchingAllow: ["Veydrop"], matchingBlock: [], suggestedCommand: null }, warnings: [] }));
    expect(screen.queryByText(/New-NetFirewallRule/)).toBeNull();
  });

  it("Run diagnostics performs the deep check and reports the result", async () => {
    const fresh = mk({ windows: { checked: true, category: "Private", alias: "Home" }, warnings: [] });
    diagnostics.mockResolvedValue(fresh);
    const onDiag = vi.fn();
    renderPanel(mk({ deep: false }), onDiag);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /Run diagnostics/ })); });
    await waitFor(() => expect(onDiag).toHaveBeenCalledWith(fresh));
    expect(diagnostics).toHaveBeenCalledWith(true);
  });
});
