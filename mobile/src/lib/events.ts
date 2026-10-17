import type { HelloEvent, PhoneOffer, TransferState } from "./types";

export type LinkState = "connecting" | "online" | "reconnecting";

export interface EventHandlers {
  onLink(state: LinkState): void;
  onHello(e: HelloEvent): void;
  onLanguage(language: string): void;
  onShare(): void;
  onOffer(o: PhoneOffer): void;
  onTransfer(e: { transferId: string; state: TransferState; error?: string | null }): void;
  onCancelled(transferId: string): void;
  onRevoked(): void;
  onResync(): void;
}

type ESLike = {
  addEventListener(type: string, cb: (e: MessageEvent) => void): void;
  close(): void;
  onopen: ((e: Event) => void) | null;
  onerror: ((e: Event) => void) | null;
  readyState: number;
};

/**
 * Subscribes to the PC's server-sent events. EventSource reconnects by itself; every
 * reconnect delivers a fresh `hello`, which consumers treat as a full resync.
 */
export function connectEvents(h: EventHandlers, factory: (url: string) => ESLike = (u) => new EventSource(u)): () => void {
  let closed = false;
  let es: ESLike | null = null;
  let hadOpen = false;

  const parse = (e: MessageEvent): unknown => {
    try { return JSON.parse(e.data); } catch { return null; }
  };

  const open = () => {
    h.onLink(hadOpen ? "reconnecting" : "connecting");
    const src = factory("/api/events");
    es = src;
    src.onopen = () => {
      hadOpen = true;
      h.onLink("online");
    };
    src.onerror = () => {
      if (closed) return;
      h.onLink("reconnecting");
      // CLOSED means the browser gave up (for example a 401 after revocation): verify and resync manually.
      if (src.readyState === 2) {
        src.close();
        setTimeout(() => { if (!closed) { h.onResync(); open(); } }, 3000);
      }
    };
    src.addEventListener("hello", (e) => { const d = parse(e) as HelloEvent | null; if (d) h.onHello(d); });
    src.addEventListener("language", (e) => {
      const d = parse(e) as { language?: string } | null;
      if (d?.language) h.onLanguage(d.language);
    });
    src.addEventListener("share", () => h.onShare());
    src.addEventListener("offer", (e) => { const d = parse(e) as PhoneOffer | null; if (d) h.onOffer(d); });
    src.addEventListener("transfer", (e) => {
      const d = parse(e) as { transferId: string; state: TransferState; error?: string | null } | null;
      if (d) h.onTransfer(d);
    });
    src.addEventListener("cancelled", (e) => {
      const d = parse(e) as { transferId: string } | null;
      if (d) h.onCancelled(d.transferId);
    });
    src.addEventListener("revoked", () => { closed = true; src.close(); h.onRevoked(); });
    src.addEventListener("resync", () => h.onResync());
  };

  open();
  return () => {
    closed = true;
    es?.close();
  };
}
