import { api, ApiError } from "./api";
import { randomId } from "./device";
import type { OfferFile, PhoneOffer, PhoneSnapshot, TransferState } from "./types";
import { FileUploader, type UploadProgress, type UploaderOptions } from "./uploader";

export const MAX_OFFER_FILES = 10_000;
const MAX_PARALLEL = 2;

export type QueuePhase = "select" | "offering" | "waiting" | "sending" | "rejected" | "failed";

export interface SendItem {
  key: string;
  name: string;
  relPath: string | null;
  size: number;
  transferId: string | null;
  /** False until the item is handed a transfer slot. */
  started: boolean;
  progress: UploadProgress | null;
}

export interface QueueState {
  phase: QueuePhase;
  items: SendItem[];
  /** Error code (`errors.<code>`) for the `failed` phase, or a rejected selection. */
  error: string | null;
}

const fileSig = (f: File, rel: string | null) => `${rel ?? ""}|${f.name}|${f.size}|${f.lastModified}`;

/**
 * One batch of files going phone → PC: select, ask the PC to accept, then upload every accepted
 * file with at most MAX_PARALLEL running at once. Framework-agnostic; the UI subscribes.
 */
export class SendQueue {
  private state: QueueState = { phase: "select", items: [], error: null };
  private listeners = new Set<() => void>();
  private files = new Map<string, File>();
  private uploaders = new Map<string, FileUploader>();
  private offerId: string | null = null;
  /** Offer updates that arrived over SSE while the POST /offers response was still in flight. */
  private early = new Map<string, PhoneOffer>();
  private uploaderOpts: UploaderOptions;

  constructor(opts: UploaderOptions = {}) {
    this.uploaderOpts = opts;
  }

  // ---- store plumbing -------------------------------------------------------------------

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };

  getState = (): QueueState => this.state;

  private set(patch: Partial<QueueState>): void {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((l) => l());
  }

  private patchItem(key: string, patch: Partial<SendItem>): void {
    this.set({ items: this.state.items.map((i) => (i.key === key ? { ...i, ...patch } : i)) });
  }

  // ---- selection ------------------------------------------------------------------------

  /** Adds files to the selection. Returns false when the 10,000-file limit would be exceeded. */
  add(list: File[]): boolean {
    if (this.state.phase !== "select" && this.state.phase !== "rejected" && this.state.phase !== "failed") return false;
    const seen = new Set(this.state.items.map((i) => {
      const f = this.files.get(i.key);
      return f ? fileSig(f, i.relPath) : i.key;
    }));
    const next = [...this.state.items];
    for (const f of list) {
      const rel = f.webkitRelativePath ? f.webkitRelativePath : null;
      const sig = fileSig(f, rel);
      if (seen.has(sig)) continue;
      if (next.length >= MAX_OFFER_FILES) {
        this.set({ items: next, phase: "select", error: "too_many" });
        return false;
      }
      seen.add(sig);
      const key = randomId(16);
      this.files.set(key, f);
      next.push({ key, name: f.name, relPath: rel, size: f.size, transferId: null, started: false, progress: null });
    }
    this.set({ items: next, phase: "select", error: null });
    return true;
  }

  remove(key: string): void {
    if (this.state.phase !== "select" && this.state.phase !== "rejected" && this.state.phase !== "failed") return;
    this.files.delete(key);
    this.set({ items: this.state.items.filter((i) => i.key !== key) });
  }

  // ---- offer ----------------------------------------------------------------------------

  async start(): Promise<void> {
    const items = this.state.items;
    if (!items.length || (this.state.phase !== "select" && this.state.phase !== "rejected" && this.state.phase !== "failed")) return;
    this.early.clear();
    this.set({ phase: "offering", error: null });
    const files: OfferFile[] = items.map((i) => ({ clientId: i.key, name: i.name, relPath: i.relPath, size: i.size }));
    try {
      const offer = await api.postOffer(files);
      this.offerId = offer.id;
      this.set({ phase: "waiting" });
      this.handleOffer(this.early.get(offer.id) ?? offer);
      this.early.clear();
    } catch (e) {
      const code = e instanceof ApiError ? e.code : "other";
      this.set({ phase: "failed", error: code });
    }
  }

  /** An offer changed on the PC (SSE event, snapshot, or the POST response). */
  handleOffer(o: PhoneOffer): void {
    if (this.state.phase === "offering" && this.offerId === null) {
      this.early.set(o.id, o);
      return;
    }
    if (o.id !== this.offerId) return;
    if (this.state.phase !== "waiting" && this.state.phase !== "offering") return;
    if (o.state === "rejected") {
      this.offerId = null;
      this.set({ phase: "rejected", error: null });
    } else if (o.state === "failed") {
      this.offerId = null;
      this.set({ phase: "failed", error: o.error ?? "other" });
    } else if (o.state === "accepted") {
      this.accepted(o);
    }
  }

  private accepted(o: PhoneOffer): void {
    const byClient = new Map(o.uploads.map((u) => [u.clientId, u.transferId]));
    const items = this.state.items.map((i) => ({ ...i, transferId: byClient.get(i.key) ?? null }));
    for (const it of items) {
      const file = this.files.get(it.key);
      if (!file || !it.transferId) continue;
      const up = new FileUploader(file, it.transferId, (p) => this.onProgress(it.key, p), this.uploaderOpts);
      this.uploaders.set(it.key, up);
    }
    this.set({ phase: "sending", items });
    this.pump();
  }

  // ---- running --------------------------------------------------------------------------

  private onProgress(key: string, p: UploadProgress): void {
    this.patchItem(key, { progress: p });
    if (p.phase === "done" || p.phase === "failed" || p.phase === "cancelled" || p.phase === "interrupted" || p.phase === "paused") this.pump();
  }

  private pump(): void {
    if (this.state.phase !== "sending") return;
    const running = this.state.items.filter((i) => i.started && (i.progress?.phase === "uploading" || i.progress?.phase === "finalizing")).length;
    let free = MAX_PARALLEL - running;
    for (const it of this.state.items) {
      if (free <= 0) break;
      if (it.started) continue;
      const up = this.uploaders.get(it.key);
      if (!up) continue;
      free--;
      this.patchItem(it.key, { started: true, progress: up.progress });
      void up.run();
    }
  }

  pause(key: string): void {
    this.uploaders.get(key)?.pause();
  }

  resume(key: string): void {
    void this.uploaders.get(key)?.resume().catch(() => {});
  }

  retry(key: string): void {
    const up = this.uploaders.get(key);
    if (!up) return;
    this.patchItem(key, { started: true });
    up.retry();
  }

  cancel(key: string): void {
    this.uploaders.get(key)?.cancel();
  }

  cancelAll(): void {
    if (this.state.phase === "waiting" && this.offerId) {
      this.offerId = null;
      this.set({ phase: "select", error: null });
      return;
    }
    for (const up of this.uploaders.values()) up.cancel();
  }

  // ---- PC → phone state -----------------------------------------------------------------

  handleTransfer(e: { transferId: string; state: TransferState; error?: string | null }): void {
    const it = this.state.items.find((i) => i.transferId === e.transferId);
    if (it) this.uploaders.get(it.key)?.externalState(e.state, e.error);
  }

  handleCancelled(transferId: string): void {
    this.handleTransfer({ transferId, state: "cancelled" });
  }

  /** Full state from the PC after a reconnect: resolves a missed offer and re-syncs transfers. */
  applySnapshot(s: PhoneSnapshot): void {
    if (this.offerId) {
      const o = s.offers.find((x) => x.id === this.offerId);
      if (o) this.handleOffer(o);
    }
    for (const u of s.uploads) this.handleTransfer({ transferId: u.transferId, state: u.state, error: u.error });
  }

  /** Pull-based fallback when the offer event was missed (for example while the tab slept). */
  async refreshOffer(): Promise<void> {
    if (!this.offerId || this.state.phase !== "waiting") return;
    try {
      this.handleOffer(await api.getOffer(this.offerId));
    } catch {
      /* the next SSE event or snapshot will catch up */
    }
  }

  // ---- derived --------------------------------------------------------------------------

  /** True once every item reached a terminal phase. */
  get finished(): boolean {
    const { phase, items } = this.state;
    if (phase !== "sending" || !items.length) return false;
    return items.every((i) => {
      const p = i.progress?.phase;
      return p === "done" || p === "failed" || p === "cancelled";
    });
  }

  /** Back to an empty selection after a finished batch. */
  reset(): void {
    this.uploaders.clear();
    this.files.clear();
    this.offerId = null;
    this.set({ phase: "select", items: [], error: null });
  }

  /** Stops everything without telling the PC about each file (page teardown). */
  dispose(): void {
    this.listeners.clear();
  }
}
