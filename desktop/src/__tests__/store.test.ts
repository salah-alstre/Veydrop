import { beforeAll, describe, expect, it, vi } from "vitest";
import type { Snapshot, TransferInfo } from "../lib/types";

type Handler = (e: { payload: unknown }) => void;

const { handlers, baseSnap } = vi.hoisted(() => ({
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
  baseSnap: {
    server: { running: false, phoneSeen: false, addressLost: false },
    pairing: null, pairRequests: [], sessions: [], share: null, offers: [], transfers: [], network: [],
    settings: { language: "en" },
    stats: { sentBytes: 0, receivedBytes: 0, sentFiles: 0, receivedFiles: 0, failed: 0 },
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, h: Handler) => {
    handlers.set(name, h);
    return () => handlers.delete(name);
  },
}));
vi.mock("../lib/ipc", () => ({ ipc: { snapshot: vi.fn(async () => structuredClone(baseSnap) as unknown as Snapshot) } }));

const emit = (type: string, data?: unknown) => handlers.get("engine")!({ payload: { type, data } });
const tr = (id: string, startedAt: number, transferred: number): TransferInfo => ({
  id, direction: "incoming", name: id, size: 100, transferred, state: "active", speedBps: 0,
  deviceName: "Phone", startedAt,
} as TransferInfo);

let store: typeof import("../lib/store");
beforeAll(async () => {
  store = await import("../lib/store");
  await store.startStore();
});

describe("engine event reducer", () => {
  it("loads the snapshot on start", () => {
    expect(store.getApp().loaded).toBe(true);
    expect(store.getApp().snap?.server.running).toBe(false);
  });

  it("applies server and pairing updates", () => {
    emit("server", { running: true, ip: "192.168.1.5", port: 47821, phoneSeen: false, addressLost: false });
    expect(store.getApp().snap?.server.port).toBe(47821);
    emit("pairing", { token: "x" });
    expect(store.getApp().snap?.pairing).toEqual({ token: "x" });
    emit("pairing", null);
    expect(store.getApp().snap?.pairing).toBeNull();
  });

  it("adds, de-duplicates and removes pair requests", () => {
    emit("pairRequest", { id: "p1", code: "1234" });
    emit("pairRequest", { id: "p1", code: "1234" });
    emit("pairRequest", { id: "p2", code: "9999" });
    expect(store.getApp().snap?.pairRequests.map((p) => p.id)).toEqual(["p1", "p2"]);
    emit("pairRequestResolved", { id: "p1" });
    expect(store.getApp().snap?.pairRequests.map((p) => p.id)).toEqual(["p2"]);
  });

  it("merges transfer deltas by id, newest first", () => {
    emit("transfers", [tr("a", 1, 10), tr("b", 2, 0)]);
    emit("transfers", [tr("a", 1, 60)]);
    const list = store.getApp().snap!.transfers;
    expect(list.map((t) => t.id)).toEqual(["b", "a"]);
    expect(list.find((t) => t.id === "a")!.transferred).toBe(60);
    emit("transferDone", { ...tr("a", 1, 100), state: "completed" });
    const a = store.getApp().snap!.transfers.find((t) => t.id === "a")!;
    expect(a.state).toBe("completed");
    expect(a.transferred).toBe(100);
  });

  it("tracks offers", () => {
    emit("offer", { id: "o1", state: "pending" });
    expect(store.getApp().snap!.offers).toHaveLength(1);
    emit("offerResolved", { id: "o1" });
    expect(store.getApp().snap!.offers).toHaveLength(0);
  });

  it("applies settings and stats events", () => {
    emit("settings", { language: "ar" });
    expect(store.getApp().snap!.settings.language).toBe("ar");
    emit("stats", { sentBytes: 5, receivedBytes: 6, sentFiles: 1, receivedFiles: 2, failed: 0 });
    expect(store.getApp().snap!.stats.receivedFiles).toBe(2);
  });

  it("bumps the history revision and notifies hooks", () => {
    const before = store.getApp().historyRev;
    const seen: string[] = [];
    const off = store.onEngineEvent((e) => seen.push(e.type));
    emit("historyChanged");
    off();
    emit("historyChanged");
    expect(store.getApp().historyRev).toBe(before + 2);
    expect(seen).toEqual(["historyChanged"]);
  });

  it("stores close requests and refetches on engine-resync", async () => {
    handlers.get("close-requested")!({ payload: { serverRunning: true, activeTransfers: 1, minimizeToTray: true } });
    expect(store.getApp().closeInfo?.activeTransfers).toBe(1);
    store.clearCloseInfo();
    expect(store.getApp().closeInfo).toBeNull();

    const before = store.getApp().historyRev;
    handlers.get("engine-resync")!({ payload: null });
    await vi.waitFor(() => expect(store.getApp().historyRev).toBeGreaterThan(before));
    expect(store.getApp().snap!.pairRequests).toEqual([]);
  });

  it("queues and dismisses toasts", () => {
    store.toast("error", "boom");
    const t = store.getApp().toasts.at(-1)!;
    expect(t.text).toBe("boom");
    store.dismissToast(t.id);
    expect(store.getApp().toasts.find((x) => x.id === t.id)).toBeUndefined();
  });
});
