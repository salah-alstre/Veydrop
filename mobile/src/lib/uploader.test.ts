import { createHash } from "node:crypto";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => {
  const actual = await vi.importActual<typeof import("./api")>("./api");
  return {
    ...actual,
    api: {
      getUploadStatus: vi.fn(),
      putChunk: vi.fn(),
      completeUpload: vi.fn(),
      cancelUpload: vi.fn(),
      setUploadPaused: vi.fn(),
    },
  };
});

import { api, ApiError } from "./api";
import { FileUploader, SpeedMeter, type UploadProgress } from "./uploader";

const m = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

function bytes(n: number): Uint8Array {
  const b = new Uint8Array(n);
  for (let i = 0; i < n; i++) b[i] = (i * 31 + 7) & 0xff;
  return b;
}
const sha = (b: Uint8Array) => createHash("sha256").update(b).digest("hex");
const mkFile = (b: Uint8Array) => new File([b as BlobPart], "f.bin", { lastModified: 1 });
const status = (received: number, size: number, state = "active") => ({ transferId: "t", received, size, state, error: null });

/** A fake PC that stores what it is sent, so tests can compare bytes and hash. */
function fakePc(size: number) {
  const store = { received: 0, chunks: [] as Uint8Array[] };
  m.getUploadStatus.mockImplementation(async () => status(store.received, size));
  m.putChunk.mockImplementation(async (_t: string, offset: number, body: ArrayBuffer) => {
    if (offset !== store.received) throw new ApiError("offset_mismatch", "bad offset", 409, { offset: store.received });
    store.chunks.push(new Uint8Array(body));
    store.received += body.byteLength;
    return { received: store.received };
  });
  return store;
}

const fast = { chunkSize: 1000, backoffMs: () => 1, maxAttempts: 3 };

beforeEach(() => {
  Object.values(m).forEach((f) => f.mockReset());
  m.cancelUpload.mockResolvedValue({ ok: true });
  m.setUploadPaused.mockResolvedValue({});
});

describe("FileUploader", () => {
  it("uploads in chunks and completes with the SHA-256 of the whole file", async () => {
    const data = bytes(2500);
    const pc = fakePc(data.length);
    m.completeUpload.mockResolvedValue({ state: "completed", error: null });
    const seen: UploadProgress[] = [];
    const up = new FileUploader(mkFile(data), "t", (p) => seen.push(p), fast);
    await up.run();
    expect(up.progress.phase).toBe("done");
    expect(pc.chunks.map((c) => c.length)).toEqual([1000, 1000, 500]);
    expect(m.completeUpload).toHaveBeenCalledWith("t", sha(data));
    expect(up.progress.sent).toBe(2500);
  });

  it("handles an empty file", async () => {
    fakePc(0);
    m.completeUpload.mockResolvedValue({ state: "completed", error: null });
    const up = new FileUploader(mkFile(new Uint8Array(0)), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("done");
    expect(m.completeUpload).toHaveBeenCalledWith("t", sha(new Uint8Array(0)));
    expect(m.putChunk).not.toHaveBeenCalled();
  });

  it("resumes from the PC's committed offset and still hashes the bytes it skipped", async () => {
    const data = bytes(3000);
    const pc = fakePc(data.length);
    pc.received = 1500; // a previous page load already delivered half
    m.completeUpload.mockResolvedValue({ state: "completed", error: null });
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("done");
    expect(m.putChunk.mock.calls[0][1]).toBe(1500);
    expect(m.completeUpload).toHaveBeenCalledWith("t", sha(data));
  });

  it("recovers from an offset_mismatch by adopting the PC's offset", async () => {
    const data = bytes(2000);
    const pc = fakePc(data.length);
    let first = true;
    const real = m.putChunk.getMockImplementation()!;
    m.putChunk.mockImplementation(async (t: string, o: number, b: ArrayBuffer) => {
      if (first) {
        first = false;
        pc.received = 1000; // the PC already had the first chunk (lost response)
        pc.chunks.push(data.slice(0, 1000));
        throw new ApiError("offset_mismatch", "x", 409, { offset: 1000 });
      }
      return real(t, o, b);
    });
    m.completeUpload.mockResolvedValue({ state: "completed", error: null });
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("done");
    expect(m.completeUpload).toHaveBeenCalledWith("t", sha(data));
  });

  it("retries transient network errors and then succeeds", async () => {
    const data = bytes(1500);
    fakePc(data.length);
    const real = m.putChunk.getMockImplementation()!;
    let fails = 1;
    m.putChunk.mockImplementation(async (t: string, o: number, b: ArrayBuffer) => {
      if (fails-- > 0) throw new ApiError("network", "net");
      return real(t, o, b);
    });
    m.completeUpload.mockResolvedValue({ state: "completed", error: null });
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("done");
  });

  it("becomes interrupted (not failed) after repeated network errors, and can retry", async () => {
    const data = bytes(1500);
    const pc = fakePc(data.length);
    const real = m.putChunk.getMockImplementation()!;
    let down = true;
    m.putChunk.mockImplementation(async (t: string, o: number, b: ArrayBuffer) => {
      if (down) throw new ApiError("network", "net");
      return real(t, o, b);
    });
    m.completeUpload.mockResolvedValue({ state: "completed", error: null });
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("interrupted");
    expect(up.progress.error).toBe("network");
    down = false;
    up.retry();
    await vi.waitFor(() => expect(up.progress.phase).toBe("done"));
    expect(pc.received).toBe(1500);
  });

  it("reports failed when the PC says the hash does not match, never done", async () => {
    const data = bytes(500);
    fakePc(data.length);
    m.completeUpload.mockResolvedValue({ state: "failed", error: "hash_mismatch" });
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("failed");
    expect(up.progress.error).toBe("hash_mismatch");
  });

  it("fails permanently on a non-transient error such as insufficient_space", async () => {
    const data = bytes(500);
    fakePc(data.length);
    m.putChunk.mockRejectedValue(new ApiError("insufficient_space", "full", 507));
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("failed");
    expect(up.progress.error).toBe("insufficient_space");
    expect(m.putChunk).toHaveBeenCalledTimes(1);
  });

  it("cancel stops the upload, tells the PC, and ends in cancelled", async () => {
    const data = bytes(3000);
    fakePc(data.length);
    let release!: () => void;
    const real = m.putChunk.getMockImplementation()!;
    m.putChunk.mockImplementationOnce(
      (t: string, o: number, b: ArrayBuffer, opts: { signal: AbortSignal }) =>
        new Promise((res, rej) => {
          release = () => res(real(t, o, b));
          opts.signal.addEventListener("abort", () => rej(new DOMException("aborted", "AbortError")));
        }),
    );
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    const run = up.run();
    await vi.waitFor(() => expect(m.putChunk).toHaveBeenCalled());
    up.cancel();
    await run;
    release?.();
    expect(up.progress.phase).toBe("cancelled");
    expect(m.cancelUpload).toHaveBeenCalledWith("t");
    expect(m.completeUpload).not.toHaveBeenCalled();
  });

  it("pause and resume continue from the committed offset to a verified finish", async () => {
    const data = bytes(3000);
    const pc = fakePc(data.length);
    m.completeUpload.mockResolvedValue({ state: "completed", error: null });
    const real = m.putChunk.getMockImplementation()!;
    let pauseNow: (() => void) | null = null;
    m.putChunk.mockImplementation(async (t: string, o: number, b: ArrayBuffer, opts: { signal: AbortSignal }) => {
      const r = await real(t, o, b, opts);
      if (o === 0) pauseNow?.();
      return r;
    });
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    pauseNow = () => up.pause();
    void up.run();
    await vi.waitFor(() => expect(up.progress.phase).toBe("paused"));
    expect(up.progress.pausedBy).toBe("user");
    expect(m.setUploadPaused).toHaveBeenCalledWith("t", true);
    pauseNow = null;
    await up.resume();
    await vi.waitFor(() => expect(up.progress.phase).toBe("done"));
    expect(pc.received).toBe(3000);
    expect(m.completeUpload).toHaveBeenCalledWith("t", sha(data));
  });

  it("an external cancelled event from the PC ends the upload as cancelled", async () => {
    const data = bytes(3000);
    fakePc(data.length);
    m.putChunk.mockImplementation((_t: string, _o: number, _b: ArrayBuffer, opts: { signal: AbortSignal }) =>
      new Promise((_res, rej) => opts.signal.addEventListener("abort", () => rej(new DOMException("a", "AbortError")))));
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    const run = up.run();
    await vi.waitFor(() => expect(m.putChunk).toHaveBeenCalled());
    up.externalState("cancelled");
    await run;
    expect(up.progress.phase).toBe("cancelled");
  });

  it("fails with file_changed if the file shrank after selection", async () => {
    const f = mkFile(bytes(1000));
    const up = new FileUploader(f, "t", () => {}, fast);
    Object.defineProperty(f, "size", { value: 900 });
    await up.run();
    expect(up.progress.phase).toBe("failed");
    expect(up.progress.error).toBe("file_changed");
  });

  it("adopts a PC-side 'completed' status without re-uploading", async () => {
    const data = bytes(800);
    m.getUploadStatus.mockResolvedValue(status(800, 800, "completed"));
    const up = new FileUploader(mkFile(data), "t", () => {}, fast);
    await up.run();
    expect(up.progress.phase).toBe("done");
    expect(m.putChunk).not.toHaveBeenCalled();
  });
});

describe("SpeedMeter", () => {
  it("computes bytes per second from samples at least 400 ms apart", () => {
    let t = 0;
    const meter = new SpeedMeter(() => t);
    meter.reset(0);
    t = 1000;
    expect(meter.sample(1000)).toBe(1000);
    t = 1100; // too soon: unchanged
    expect(meter.sample(5000)).toBe(1000);
  });
});
