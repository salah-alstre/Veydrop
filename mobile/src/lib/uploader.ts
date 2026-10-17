import { api, ApiError, isAbort, MAX_CHUNK } from "./api";
import { Sha256 } from "./sha256";
import type { TransferState } from "./types";

export type UploadPhase =
  | "uploading" | "paused" | "interrupted" | "finalizing" | "done" | "failed" | "cancelled";

export interface UploadProgress {
  phase: UploadPhase;
  /** Bytes the PC has confirmed plus in-flight bytes of the current chunk. */
  sent: number;
  speedBps: number;
  etaSecs: number | null;
  /** Error code (`errors.<code>`) when phase is failed or interrupted. */
  error: string | null;
  pausedBy: "user" | "pc" | null;
}

export interface UploaderOptions {
  chunkSize?: number;
  maxAttempts?: number;
  backoffMs?: (attempt: number) => number;
  now?: () => number;
}

/** Smoothed throughput from cumulative byte samples. */
export class SpeedMeter {
  private last = 0;
  private lastAt = 0;
  private ema = 0;
  constructor(private now: () => number = () => performance.now()) {}
  reset(bytes: number): void {
    this.last = bytes;
    this.lastAt = this.now();
    this.ema = 0;
  }
  sample(bytes: number): number {
    const t = this.now();
    const dt = t - this.lastAt;
    if (dt >= 400) {
      const inst = (Math.max(0, bytes - this.last) * 1000) / dt;
      this.ema = this.ema === 0 ? inst : this.ema * 0.7 + inst * 0.3;
      this.last = bytes;
      this.lastAt = t;
    }
    return this.ema;
  }
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const defaultBackoff = (attempt: number) => Math.min(15000, 1000 * 2 ** (attempt - 1));

/**
 * Uploads one accepted file in sequential chunks, resuming from the PC's committed offset after
 * any interruption, and verifies the whole file with SHA-256 at the end.
 */
export class FileUploader {
  private chunkSize: number;
  private maxAttempts: number;
  private backoff: (attempt: number) => number;
  private meter: SpeedMeter;

  private pos = 0;
  private hasher = new Sha256();
  private hasherPos = 0;
  private digest: string | null = null;

  private ctrl: AbortController | null = null;
  private running = false;
  private stopped = false;
  private userPaused = false;
  private pcPaused = false;
  private gate: (() => void) | null = null;
  private size: number;

  private p: UploadProgress = { phase: "uploading", sent: 0, speedBps: 0, etaSecs: null, error: null, pausedBy: null };

  constructor(
    readonly file: File,
    readonly transferId: string,
    private onChange: (p: UploadProgress) => void,
    opts: UploaderOptions = {},
  ) {
    this.size = file.size;
    this.chunkSize = Math.max(1, Math.min(opts.chunkSize ?? MAX_CHUNK, MAX_CHUNK));
    this.maxAttempts = opts.maxAttempts ?? 6;
    this.backoff = opts.backoffMs ?? defaultBackoff;
    this.meter = new SpeedMeter(opts.now);
  }

  get progress(): UploadProgress {
    return this.p;
  }

  private set(patch: Partial<UploadProgress>): void {
    this.p = { ...this.p, ...patch };
    this.onChange(this.p);
  }

  private report(sent: number): void {
    const bounded = Math.min(sent, this.size);
    const speed = this.meter.sample(bounded);
    this.set({
      sent: bounded,
      speedBps: speed,
      etaSecs: speed > 0 ? (this.size - bounded) / speed : null,
    });
  }

  /** Runs until the file is done, failed, cancelled, or the connection gave up (interrupted). */
  async run(): Promise<void> {
    if (this.running || this.stopped) return;
    this.running = true;
    try {
      await this.loop();
    } finally {
      this.running = false;
    }
  }

  private openGate(): void {
    const g = this.gate;
    this.gate = null;
    g?.();
  }

  private waitGate(): Promise<void> {
    return new Promise((r) => { this.gate = r; });
  }

  pause(): void {
    if (this.stopped || this.userPaused || this.isFinal()) return;
    this.userPaused = true;
    this.set({ phase: "paused", pausedBy: "user" });
    this.ctrl?.abort();
    api.setUploadPaused(this.transferId, true).catch(() => {});
  }

  async resume(): Promise<void> {
    if (this.stopped || !this.userPaused) return;
    this.userPaused = false;
    try {
      await api.setUploadPaused(this.transferId, false);
    } catch (e) {
      if (!(e instanceof ApiError) || e.code !== "conflict") {
        this.userPaused = true;
        throw e;
      }
    }
    this.set({ phase: "uploading", pausedBy: null });
    this.openGate();
    void this.run();
  }

  /** Restart after the connection gave up. */
  retry(): void {
    if (this.stopped || this.running || this.p.phase !== "interrupted") return;
    this.set({ phase: "uploading", error: null });
    void this.run();
  }

  cancel(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.set({ phase: "cancelled" });
    this.ctrl?.abort();
    this.openGate();
    api.cancelUpload(this.transferId).catch(() => {});
  }

  /** The PC (or another tab) changed this transfer's state; delivered over SSE. */
  externalState(state: TransferState, error?: string | null): void {
    if (this.stopped || this.isFinal()) return;
    if (state === "cancelled") {
      this.stopped = true;
      this.set({ phase: "cancelled" });
      this.ctrl?.abort();
      this.openGate();
    } else if (state === "failed" || state === "rejected") {
      this.stopped = true;
      this.set({ phase: "failed", error: error ?? "other" });
      this.ctrl?.abort();
      this.openGate();
    } else if (state === "paused") {
      if (!this.userPaused) {
        this.pcPaused = true;
        this.set({ phase: "paused", pausedBy: "pc" });
        this.ctrl?.abort();
      }
    } else if (this.pcPaused && (state === "queued" || state === "active" || state === "interrupted")) {
      this.pcPaused = false;
      if (!this.userPaused) {
        this.set({ phase: "uploading", pausedBy: null });
        this.openGate();
        void this.run();
      }
    }
  }

  private isFinal(): boolean {
    return this.p.phase === "done" || this.p.phase === "failed" || this.p.phase === "cancelled";
  }

  /** Brings the incremental hash to exactly `pos`, hashing bytes the PC already has if needed. */
  private async catchUpHash(pos: number): Promise<void> {
    if (this.hasherPos > pos) {
      this.hasher = new Sha256();
      this.hasherPos = 0;
      this.digest = null;
    }
    while (this.hasherPos < pos) {
      const end = Math.min(pos, this.hasherPos + this.chunkSize);
      const buf = new Uint8Array(await this.file.slice(this.hasherPos, end).arrayBuffer());
      this.hasher.update(buf);
      this.hasherPos = end;
    }
  }

  /** Reads the PC's committed offset. Returns true when the transfer is already final. */
  private async refresh(signal: AbortSignal): Promise<boolean> {
    const st = await api.getUploadStatus(this.transferId, signal);
    switch (st.state) {
      case "completed":
        this.pos = this.size;
        this.report(this.size);
        this.set({ phase: "done", error: null });
        return true;
      case "failed":
      case "rejected":
        this.stopped = true;
        this.set({ phase: "failed", error: st.error ?? "other" });
        return true;
      case "cancelled":
        this.stopped = true;
        this.set({ phase: "cancelled" });
        return true;
      case "paused":
        if (!this.userPaused) this.pcPaused = true;
        break;
      default:
        break;
    }
    this.pos = Math.min(st.received, this.size);
    this.meter.reset(this.pos);
    this.report(this.pos);
    return false;
  }

  private async loop(): Promise<void> {
    let attempt = 0;
    let needStatus = true;
    let checked = false;
    this.set({ phase: "uploading", error: null, pausedBy: null });

    while (!this.stopped) {
      if (this.userPaused || this.pcPaused) {
        this.set({ phase: "paused", pausedBy: this.userPaused ? "user" : "pc" });
        await this.waitGate();
        needStatus = true;
        continue;
      }
      const ctrl = new AbortController();
      this.ctrl = ctrl;
      try {
        if (!checked) {
          if (this.file.size !== this.size) throw new ApiError("file_changed", "file changed");
          checked = true;
        }
        if (needStatus) {
          if (await this.refresh(ctrl.signal)) return;
          needStatus = false;
          if (this.pcPaused) continue;
        }
        await this.catchUpHash(this.pos);

        if (this.pos >= this.size) {
          this.set({ phase: "finalizing" });
          this.digest ??= this.hasher.digestHex();
          const r = await api.completeUpload(this.transferId, this.digest);
          if (r.state === "completed") {
            this.report(this.size);
            this.set({ phase: "done", error: null });
          } else if (r.state === "failed" || r.state === "cancelled") {
            this.stopped = true;
            this.set({ phase: r.state === "failed" ? "failed" : "cancelled", error: r.error ?? "other" });
          } else {
            // Not final yet (still finalizing on the PC): re-read its state shortly.
            needStatus = true;
            await sleep(400);
            if (++attempt >= this.maxAttempts) {
              this.set({ phase: "interrupted", error: "network" });
              return;
            }
            continue;
          }
          return;
        }

        const base = this.pos;
        const end = Math.min(base + this.chunkSize, this.size);
        const buf = await this.file.slice(base, end).arrayBuffer();
        if (buf.byteLength !== end - base) throw new ApiError("file_changed", "file changed");
        await api.putChunk(this.transferId, base, buf, { signal: ctrl.signal, onProgress: (l) => this.report(base + l) });
        this.hasher.update(new Uint8Array(buf));
        this.hasherPos = end;
        this.pos = end;
        attempt = 0;
        this.report(end);
      } catch (e) {
        if (isAbort(e)) {
          needStatus = true;
          continue;
        }
        const err = e instanceof ApiError ? e : new ApiError("other", String(e));
        needStatus = true;
        if (err.code === "offset_mismatch" && typeof err.extra.offset === "number") {
          this.pos = Math.min(err.extra.offset, this.size);
          needStatus = false;
          continue;
        }
        if (err.code === "cancelled") {
          this.stopped = true;
          this.set({ phase: "cancelled" });
          return;
        }
        if (err.code === "rate_limited") {
          if (++attempt >= this.maxAttempts * 2) return this.giveUp(err);
          await sleep((err.extra.retryAfter ?? 2) * 1000);
          continue;
        }
        if (err.code === "conflict" || err.transient) {
          if (++attempt >= this.maxAttempts) return this.giveUp(err);
          await sleep(this.backoff(attempt));
          continue;
        }
        this.stopped = true;
        this.set({ phase: "failed", error: err.code });
        return;
      }
    }
  }

  private giveUp(err: ApiError): void {
    this.set({ phase: "interrupted", error: err.code === "conflict" ? "network" : err.code });
  }
}
