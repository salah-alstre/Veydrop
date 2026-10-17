import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "./ipc";
import type {
  CloseInfo, EngineEvent, OfferInfo, PairRequestInfo, Snapshot, TransferInfo,
} from "./types";

export interface Toast { id: number; kind: "info" | "success" | "error"; text: string }

export interface AppState {
  loaded: boolean;
  loadError: string | null;
  snap: Snapshot | null;
  /** Bumped whenever history changes so the Activity page refetches. */
  historyRev: number;
  toasts: Toast[];
  closeInfo: CloseInfo | null;
}

let state: AppState = {
  loaded: false, loadError: null, snap: null, historyRev: 0, toasts: [], closeInfo: null,
};
const subs = new Set<() => void>();
let toastSeq = 1;

function set(patch: Partial<AppState>) {
  state = { ...state, ...patch };
  subs.forEach((f) => f());
}
function setSnap(fn: (s: Snapshot) => Snapshot) {
  if (!state.snap) return;
  set({ snap: fn(state.snap) });
}

export function useApp(): AppState {
  return useSyncExternalStore((cb) => { subs.add(cb); return () => subs.delete(cb); }, () => state);
}
export const getApp = () => state;

export function toast(kind: Toast["kind"], text: string) {
  const id = toastSeq++;
  set({ toasts: [...state.toasts, { id, kind, text }].slice(-4) });
  setTimeout(() => dismissToast(id), kind === "error" ? 7000 : 3800);
}
export function dismissToast(id: number) {
  set({ toasts: state.toasts.filter((t) => t.id !== id) });
}
export function clearCloseInfo() { set({ closeInfo: null }); }

function mergeTransfers(list: TransferInfo[], delta: TransferInfo[]): TransferInfo[] {
  const byId = new Map(list.map((t) => [t.id, t]));
  for (const t of delta) byId.set(t.id, t);
  return [...byId.values()].sort((a, b) => b.startedAt - a.startedAt);
}
function upsert<T extends { id: string }>(list: T[], item: T): T[] {
  const i = list.findIndex((x) => x.id === item.id);
  if (i < 0) return [...list, item];
  const next = list.slice();
  next[i] = item;
  return next;
}

export type EventHook = (ev: EngineEvent) => void;
const hooks = new Set<EventHook>();
export function onEngineEvent(h: EventHook): () => void { hooks.add(h); return () => hooks.delete(h); }

function apply(ev: EngineEvent) {
  switch (ev.type) {
    case "server": return setSnap((s) => ({ ...s, server: ev.data }));
    case "pairing": return setSnap((s) => ({ ...s, pairing: ev.data }));
    case "pairRequest": return setSnap((s) => ({ ...s, pairRequests: upsert<PairRequestInfo>(s.pairRequests, ev.data) }));
    case "pairRequestResolved":
      return setSnap((s) => ({ ...s, pairRequests: s.pairRequests.filter((p) => p.id !== ev.data.id) }));
    case "sessions": return setSnap((s) => ({ ...s, sessions: ev.data }));
    case "share": return setSnap((s) => ({ ...s, share: ev.data }));
    case "offer": return setSnap((s) => ({ ...s, offers: upsert<OfferInfo>(s.offers, ev.data) }));
    case "offerResolved": return setSnap((s) => ({ ...s, offers: s.offers.filter((o) => o.id !== ev.data.id) }));
    case "transfers": return setSnap((s) => ({ ...s, transfers: mergeTransfers(s.transfers, ev.data) }));
    case "transferDone":
      setSnap((s) => ({ ...s, transfers: mergeTransfers(s.transfers, [ev.data]) }));
      return;
    case "historyChanged": return set({ historyRev: state.historyRev + 1 });
    case "network": return setSnap((s) => ({ ...s, network: ev.data }));
    case "settings": return setSnap((s) => ({ ...s, settings: ev.data }));
    case "stats": return setSnap((s) => ({ ...s, stats: ev.data }));
  }
}

async function resync() {
  try {
    const snap = await ipc.snapshot();
    set({ snap, loaded: true, loadError: null, historyRev: state.historyRev + 1 });
  } catch (e) {
    set({ loadError: (e as { message?: string }).message ?? "load failed", loaded: true });
  }
}

let started = false;
export async function startStore() {
  if (started) return;
  started = true;
  await listen<EngineEvent>("engine", (e) => {
    apply(e.payload);
    hooks.forEach((h) => h(e.payload));
  });
  await listen("engine-resync", () => void resync());
  await listen<CloseInfo>("close-requested", (e) => set({ closeInfo: e.payload }));
  await resync();
}
