import type {
  ClaimResponse, CompleteResponse, OfferFile, PairRequestResponse, PhoneOffer,
  PhoneSnapshot, SessionResponse, ShareInfo, UploadStatus,
} from "./types";

/** Error from the desktop server (`code` matches `errors.<code>` in the locale files) or the network. */
export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
    public status = 0,
    public extra: { offset?: number; retryAfter?: number } = {},
  ) {
    super(message);
    this.name = "ApiError";
  }
  /** True when retrying the same request later may succeed. */
  get transient(): boolean {
    if (this.code === "insufficient_space" || this.code === "too_large") return false; // 507/413: retrying cannot help
    return this.code === "network" || this.status >= 500 || this.code === "rate_limited";
  }
}

export const MAX_CHUNK = 8 * 1024 * 1024;

const JSON_HEADERS = { "content-type": "application/json", "x-veydrop": "1" };

async function toError(res: Response): Promise<ApiError> {
  let code = "other";
  let message = res.statusText;
  let offset: number | undefined;
  try {
    const body = await res.json();
    if (body && typeof body.code === "string") code = body.code;
    if (body && typeof body.message === "string") message = body.message;
    if (body && typeof body.offset === "number") offset = body.offset;
  } catch {
    /* not JSON */
  }
  const ra = Number(res.headers.get("retry-after"));
  return new ApiError(code, message, res.status, { offset, retryAfter: Number.isFinite(ra) && ra > 0 ? ra : undefined });
}

async function request<T>(path: string, init: RequestInit = {}, signal?: AbortSignal): Promise<T> {
  let res: Response;
  try {
    res = await fetch(path, { credentials: "same-origin", cache: "no-store", ...init, signal });
  } catch (e) {
    if ((e as Error)?.name === "AbortError") throw e;
    throw new ApiError("network", "network error");
  }
  if (!res.ok) throw await toError(res);
  return (await res.json()) as T;
}

const post = <T>(path: string, body: unknown, signal?: AbortSignal) =>
  request<T>(path, { method: "POST", headers: JSON_HEADERS, body: JSON.stringify(body) }, signal);

export const api = {
  getInfo: () => request<{ app: string; version: string; apiVersion: number }>("/api/info"),
  getSession: () => request<SessionResponse>("/api/session"),
  pairRequest: (body: { token: string; deviceId: string; label: string; platform: string }) =>
    post<PairRequestResponse>("/api/pair/request", body),
  pairClaim: (id: string, claim: string, signal?: AbortSignal) =>
    post<ClaimResponse>("/api/pair/claim", { id, claim }, signal),
  logout: () => post<{ ok: boolean }>("/api/logout", {}),
  postOffer: (files: OfferFile[]) => post<PhoneOffer>("/api/offers", { files }),
  getOffer: (id: string) => request<PhoneOffer>(`/api/offers/${encodeURIComponent(id)}`),
  getTransfers: () => request<PhoneSnapshot>("/api/transfers"),
  getShare: () => request<{ share: ShareInfo | null }>("/api/share").then((r) => r.share),

  getUploadStatus: (tid: string, signal?: AbortSignal) =>
    request<UploadStatus>(`/api/uploads/${encodeURIComponent(tid)}`, {}, signal),
  setUploadPaused: (tid: string, paused: boolean) =>
    post<unknown>(`/api/uploads/${encodeURIComponent(tid)}/state`, { paused }),
  completeUpload: (tid: string, sha256: string) =>
    post<CompleteResponse>(`/api/uploads/${encodeURIComponent(tid)}/complete`, { sha256 }),
  cancelUpload: (tid: string) =>
    request<{ ok: boolean }>(`/api/uploads/${encodeURIComponent(tid)}`, { method: "DELETE", headers: { "x-veydrop": "1" } }),

  /** Sends one chunk. XHR (not fetch) for upload progress events and a reliable abort. */
  putChunk(
    tid: string,
    offset: number,
    body: ArrayBuffer,
    opts: { signal?: AbortSignal; onProgress?: (sent: number) => void } = {},
  ): Promise<{ received: number }> {
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open("PUT", `/api/uploads/${encodeURIComponent(tid)}`);
      xhr.withCredentials = true;
      xhr.responseType = "text";
      xhr.setRequestHeader("x-veydrop", "1");
      xhr.setRequestHeader("Upload-Offset", String(offset));
      xhr.setRequestHeader("content-type", "application/octet-stream");
      const abort = () => xhr.abort();
      opts.signal?.addEventListener("abort", abort, { once: true });
      const done = () => opts.signal?.removeEventListener("abort", abort);
      if (opts.onProgress) xhr.upload.onprogress = (e) => opts.onProgress!(e.loaded);
      xhr.onload = () => {
        done();
        let parsed: Record<string, unknown> = {};
        try { parsed = JSON.parse(xhr.responseText); } catch { /* empty */ }
        if (xhr.status >= 200 && xhr.status < 300) {
          resolve({ received: typeof parsed.received === "number" ? parsed.received : offset + body.byteLength });
        } else {
          const ra = Number(xhr.getResponseHeader("retry-after"));
          reject(new ApiError(
            typeof parsed.code === "string" ? parsed.code : "other",
            typeof parsed.message === "string" ? parsed.message : xhr.statusText,
            xhr.status,
            { offset: typeof parsed.offset === "number" ? parsed.offset : undefined, retryAfter: ra > 0 ? ra : undefined },
          ));
        }
      };
      xhr.onerror = () => { done(); reject(new ApiError("network", "network error")); };
      xhr.ontimeout = xhr.onerror;
      xhr.onabort = () => { done(); reject(new DOMException("aborted", "AbortError")); };
      if (opts.signal?.aborted) { xhr.abort(); return; }
      xhr.send(body);
    });
  },
};

export const isAbort = (e: unknown): boolean => (e as { name?: string })?.name === "AbortError";

export function downloadUrl(itemId: string): string {
  return `/api/share/items/${encodeURIComponent(itemId)}/download`;
}
export function inlineUrl(itemId: string): string {
  return `${downloadUrl(itemId)}?inline=1`;
}
export function zipUrl(opts: { ids?: string[]; prefix?: string } = {}): string {
  if (opts.ids?.length) return `/api/share/zip?ids=${opts.ids.map(encodeURIComponent).join(",")}`;
  if (opts.prefix) return `/api/share/zip?prefix=${encodeURIComponent(opts.prefix)}`;
  return "/api/share/zip";
}
