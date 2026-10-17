import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => {
  const actual = await vi.importActual<typeof import("./api")>("./api");
  return {
    ...actual,
    api: {
      postOffer: vi.fn(),
      getOffer: vi.fn(),
      getUploadStatus: vi.fn(),
      putChunk: vi.fn(),
      completeUpload: vi.fn(),
      cancelUpload: vi.fn(),
      setUploadPaused: vi.fn(),
    },
  };
});

import { api, ApiError } from "./api";
import { SendQueue } from "./sendQueue";
import type { PhoneOffer } from "./types";

const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const file = (name: string, text: string) => new File([text], name, { lastModified: 1 });
const offer = (over: Partial<PhoneOffer> = {}): PhoneOffer => ({ id: "o1", state: "pending", error: null, uploads: [], ...over });

beforeEach(() => {
  Object.values(mocked).forEach((m) => m.mockReset());
  mocked.cancelUpload.mockResolvedValue({ ok: true });
  mocked.setUploadPaused.mockResolvedValue({});
});

describe("SendQueue selection", () => {
  it("ignores an identical file added twice and supports removal", () => {
    const q = new SendQueue();
    const f = file("a.txt", "aaa");
    q.add([f]);
    q.add([f]);
    expect(q.getState().items).toHaveLength(1);
    q.remove(q.getState().items[0].key);
    expect(q.getState().items).toHaveLength(0);
  });

  it("refuses more than 10,000 files with the too_many error", () => {
    const q = new SendQueue();
    const many = Array.from({ length: 10_001 }, (_, i) => file(`f${i}.txt`, String(i)));
    expect(q.add(many)).toBe(false);
    expect(q.getState().error).toBe("too_many");
    expect(q.getState().items).toHaveLength(10_000);
  });
});

describe("SendQueue offer flow", () => {
  it("applies an accepted offer that arrived over SSE before the POST response returned", async () => {
    const q = new SendQueue();
    q.add([file("a.txt", "hello")]);
    const key = q.getState().items[0].key;

    let resolvePost!: (o: PhoneOffer) => void;
    mocked.postOffer.mockReturnValue(new Promise<PhoneOffer>((r) => (resolvePost = r)));
    mocked.getUploadStatus.mockResolvedValue({ transferId: "t1", received: 0, size: 5, state: "active", error: null });
    mocked.putChunk.mockResolvedValue({ received: 5 });
    mocked.completeUpload.mockResolvedValue({ state: "completed", error: null });

    const started = q.start();
    expect(q.getState().phase).toBe("offering");
    // SSE says "accepted" while the POST is still in flight.
    q.handleOffer(offer({ state: "accepted", uploads: [{ clientId: key, transferId: "t1" }] }));
    expect(q.getState().phase).toBe("offering");
    // Then the POST resolves with the stale "pending" snapshot.
    resolvePost(offer());
    await started;

    expect(q.getState().phase).toBe("sending");
    expect(q.getState().items[0].transferId).toBe("t1");
    await vi.waitFor(() => expect(q.finished).toBe(true));
    expect(q.getState().items[0].progress?.phase).toBe("done");
  });

  it("goes to waiting and then rejected when the PC declines", async () => {
    const q = new SendQueue();
    q.add([file("a.txt", "x")]);
    mocked.postOffer.mockResolvedValue(offer());
    await q.start();
    expect(q.getState().phase).toBe("waiting");
    q.handleOffer(offer({ state: "rejected" }));
    expect(q.getState().phase).toBe("rejected");
    expect(mocked.putChunk).not.toHaveBeenCalled();
  });

  it("surfaces the error code when posting the offer fails", async () => {
    const q = new SendQueue();
    q.add([file("a.txt", "x")]);
    mocked.postOffer.mockRejectedValue(new ApiError("forbidden", "no", 403));
    await q.start();
    expect(q.getState().phase).toBe("failed");
    expect(q.getState().error).toBe("forbidden");
  });

  it("ignores updates for a different offer", async () => {
    const q = new SendQueue();
    q.add([file("a.txt", "x")]);
    mocked.postOffer.mockResolvedValue(offer());
    await q.start();
    q.handleOffer(offer({ id: "other", state: "rejected" }));
    expect(q.getState().phase).toBe("waiting");
  });

  it("cancelling while waiting returns to selection without contacting uploads", async () => {
    const q = new SendQueue();
    q.add([file("a.txt", "x")]);
    mocked.postOffer.mockResolvedValue(offer());
    await q.start();
    q.cancelAll();
    expect(q.getState().phase).toBe("select");
    expect(mocked.cancelUpload).not.toHaveBeenCalled();
  });

  it("recovers a missed offer decision from a snapshot", async () => {
    const q = new SendQueue();
    q.add([file("a.txt", "x")]);
    mocked.postOffer.mockResolvedValue(offer());
    await q.start();
    q.applySnapshot({ offers: [offer({ state: "rejected" })], uploads: [] });
    expect(q.getState().phase).toBe("rejected");
  });
});

describe("SendQueue parallelism", () => {
  it("runs at most two uploads at a time", async () => {
    const q = new SendQueue();
    q.add([file("a", "1"), file("b", "2"), file("c", "3")]);
    const keys = q.getState().items.map((i) => i.key);
    mocked.postOffer.mockResolvedValue(
      offer({ state: "accepted", uploads: keys.map((k, i) => ({ clientId: k, transferId: `t${i}` })) }),
    );
    mocked.getUploadStatus.mockImplementation(async (tid: string) => ({ transferId: tid, received: 0, size: 1, state: "active", error: null }));
    const release: Array<() => void> = [];
    mocked.putChunk.mockImplementation(() => new Promise((r) => release.push(() => r({ received: 1 }))));
    mocked.completeUpload.mockResolvedValue({ state: "completed", error: null });

    await q.start();
    await vi.waitFor(() => expect(release.length).toBe(2));
    expect(q.getState().items.filter((i) => i.started)).toHaveLength(2);
    release[0]();
    await vi.waitFor(() => expect(release.length).toBe(3));
    release[1]();
    release[2]();
    await vi.waitFor(() => expect(q.finished).toBe(true));
  });
});
