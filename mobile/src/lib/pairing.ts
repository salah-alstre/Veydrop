import { api, ApiError, isAbort } from "./api";
import { describeDevice, getDeviceId } from "./device";
import { followDesktopLanguage } from "./language";
import type { SessionInfo } from "./types";

/** Reads `#t=<token>` from the URL and removes it so it is not kept in history or shared. */
export function takePairingToken(): string | null {
  const hash = location.hash;
  if (!hash || hash.length < 2) return null;
  const params = new URLSearchParams(hash.slice(1));
  const token = params.get("t");
  if (token !== null) {
    try {
      history.replaceState(null, "", location.pathname + location.search);
    } catch {
      /* ignore */
    }
  }
  return token && token.length <= 128 ? token : null;
}

export type PairState =
  | { kind: "checking" }
  | { kind: "needToken" }
  | { kind: "waiting"; code: string; expiresAt: number; pcName: string }
  | { kind: "paired"; session: SessionInfo; pcName: string }
  | { kind: "failed"; reason: "rejected" | "expired" | "pairing_expired" | "pairing_used" | "network" | "other"; message?: string };

export interface PairHandle {
  cancel(): void;
}

/**
 * Establishes a session: reuses an existing cookie session, otherwise redeems the QR token
 * and long-polls until the PC user approves or rejects the request.
 */
export function startPairing(token: string | null, onState: (s: PairState) => void): PairHandle {
  const ctrl = new AbortController();
  let dead = false;
  const emit = (s: PairState) => { if (!dead) onState(s); };

  void (async () => {
    try {
      emit({ kind: "checking" });
      try {
        const s = await api.getSession();
        if (s.paired) {
          await followDesktopLanguage(s.language);
          return emit({ kind: "paired", session: s.session, pcName: s.pcName });
        }
      } catch (e) {
        if (e instanceof ApiError && e.code === "network") return emit({ kind: "failed", reason: "network" });
        throw e;
      }
      if (!token) return emit({ kind: "needToken" });

      const dev = describeDevice();
      const req = await api.pairRequest({ token, deviceId: getDeviceId(), label: dev.label, platform: dev.platform });
      await followDesktopLanguage(req.language);
      emit({ kind: "waiting", code: req.code, expiresAt: req.expiresAt, pcName: req.pcName });

      let failures = 0;
      while (!dead) {
        try {
          const r = await api.pairClaim(req.id, req.claim, ctrl.signal);
          failures = 0;
          if (r.status === "approved") {
            await followDesktopLanguage(r.language);
            return emit({ kind: "paired", session: r.session, pcName: r.pcName });
          }
          if (r.status === "rejected") return emit({ kind: "failed", reason: "rejected" });
          if (r.status === "expired") return emit({ kind: "failed", reason: "expired" });
        } catch (e) {
          if (isAbort(e)) return;
          if (e instanceof ApiError && e.transient && ++failures < 4) {
            await new Promise((res) => setTimeout(res, 1000 * failures));
            continue;
          }
          throw e;
        }
      }
    } catch (e) {
      if (isAbort(e)) return;
      if (e instanceof ApiError) {
        const reason =
          e.code === "pairing_expired" || e.code === "pairing_used" ? e.code : e.code === "network" ? "network" : "other";
        return emit({ kind: "failed", reason, message: e.code });
      }
      emit({ kind: "failed", reason: "other" });
    }
  })();

  return {
    cancel() {
      dead = true;
      ctrl.abort();
    },
  };
}
