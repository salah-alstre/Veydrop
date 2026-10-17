import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError, downloadUrl, inlineUrl, isAbort, zipUrl } from "./api";

function jsonResponse(body: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(body), { headers: { "content-type": "application/json" }, ...init });
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("ApiError.transient", () => {
  it("is true for network errors, 5xx and rate limiting", () => {
    expect(new ApiError("network", "x").transient).toBe(true);
    expect(new ApiError("io", "x", 500).transient).toBe(true);
    expect(new ApiError("rate_limited", "x", 429).transient).toBe(true);
  });
  it("is false for client errors and for conditions retrying cannot fix", () => {
    expect(new ApiError("forbidden", "x", 403).transient).toBe(false);
    expect(new ApiError("hash_mismatch", "x", 422).transient).toBe(false);
    expect(new ApiError("insufficient_space", "x", 507).transient).toBe(false);
    expect(new ApiError("too_large", "x", 413).transient).toBe(false);
  });
});

describe("request error mapping", () => {
  it("turns a JSON error body into an ApiError with code, status, offset and retry-after", async () => {
    vi.stubGlobal("fetch", vi.fn(async () =>
      jsonResponse({ code: "offset_mismatch", message: "bad", offset: 42 }, { status: 409, headers: { "retry-after": "3" } })));
    const err = await api.getInfo().then(() => { throw new Error("expected rejection"); }, (e: ApiError) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.code).toBe("offset_mismatch");
    expect(err.status).toBe(409);
    expect(err.extra).toEqual({ offset: 42, retryAfter: 3 });
  });

  it("falls back to code 'other' for a non-JSON error body", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("boom", { status: 502, statusText: "Bad Gateway" })));
    const err = await api.getSession().then(() => { throw new Error("expected rejection"); }, (e: ApiError) => e);
    expect(err.code).toBe("other");
    expect(err.status).toBe(502);
    expect(err.transient).toBe(true);
  });

  it("maps a failed fetch to the 'network' code", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new TypeError("Failed to fetch"); }));
    const err = await api.getInfo().then(() => { throw new Error("expected rejection"); }, (e: ApiError) => e);
    expect(err.code).toBe("network");
    expect(err.transient).toBe(true);
  });

  it("lets an abort through unchanged", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new DOMException("aborted", "AbortError"); }));
    const err = await api.pairClaim("i", "c", new AbortController().signal).catch((e) => e);
    expect(isAbort(err)).toBe(true);
    expect(err).not.toBeInstanceOf(ApiError);
  });
});

describe("request shape", () => {
  it("sends mutating requests with the x-veydrop header, JSON body and same-origin credentials", async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ ok: true }));
    vi.stubGlobal("fetch", fetchMock);
    await api.logout();
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("/api/logout");
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>)["x-veydrop"]).toBe("1");
    expect(init.credentials).toBe("same-origin");
    expect(init.cache).toBe("no-store");
  });

  it("cancelUpload uses DELETE with the x-veydrop header and an encoded id", async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ ok: true }));
    vi.stubGlobal("fetch", fetchMock);
    await api.cancelUpload("a/b c");
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("/api/uploads/a%2Fb%20c");
    expect(init.method).toBe("DELETE");
    expect((init.headers as Record<string, string>)["x-veydrop"]).toBe("1");
  });

  it("getShare unwraps the share object and passes null through", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse({ share: null })));
    expect(await api.getShare()).toBeNull();
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse({ share: { id: "s", items: [] } })));
    expect(await api.getShare()).toEqual({ id: "s", items: [] });
  });
});

describe("URL helpers", () => {
  it("builds encoded download and inline URLs", () => {
    expect(downloadUrl("a b")).toBe("/api/share/items/a%20b/download");
    expect(inlineUrl("x")).toBe("/api/share/items/x/download?inline=1");
  });
  it("builds ZIP URLs for ids, a folder prefix, or everything", () => {
    expect(zipUrl({ ids: ["a", "b/c"] })).toBe("/api/share/zip?ids=a,b%2Fc");
    expect(zipUrl({ prefix: "Album/2024" })).toBe("/api/share/zip?prefix=Album%2F2024");
    expect(zipUrl()).toBe("/api/share/zip");
    expect(zipUrl({ ids: [] })).toBe("/api/share/zip");
  });
});

describe("putChunk (XHR)", () => {
  class FakeXhr {
    static last: FakeXhr;
    headers: Record<string, string> = {};
    method = ""; url = ""; status = 0; statusText = ""; responseText = ""; withCredentials = false;
    responseType = "";
    upload: { onprogress: ((e: { loaded: number }) => void) | null } = { onprogress: null };
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    ontimeout: (() => void) | null = null;
    onabort: (() => void) | null = null;
    sent: unknown = null;
    respHeaders: Record<string, string> = {};
    constructor() { FakeXhr.last = this; }
    open(m: string, u: string) { this.method = m; this.url = u; }
    setRequestHeader(k: string, v: string) { this.headers[k.toLowerCase()] = v; }
    getResponseHeader(k: string) { return this.respHeaders[k.toLowerCase()] ?? null; }
    send(b: unknown) { this.sent = b; }
    abort() { this.onabort?.(); }
  }

  it("sends the offset and security headers and resolves with the committed length", async () => {
    vi.stubGlobal("XMLHttpRequest", FakeXhr);
    const body = new ArrayBuffer(10);
    const p = api.putChunk("t1", 100, body);
    const x = FakeXhr.last;
    expect(x.method).toBe("PUT");
    expect(x.url).toBe("/api/uploads/t1");
    expect(x.headers["upload-offset"]).toBe("100");
    expect(x.headers["x-veydrop"]).toBe("1");
    expect(x.withCredentials).toBe(true);
    x.status = 200; x.responseText = JSON.stringify({ received: 110 });
    x.onload!();
    expect(await p).toEqual({ received: 110 });
  });

  it("falls back to offset+length when the response has no body", async () => {
    vi.stubGlobal("XMLHttpRequest", FakeXhr);
    const p = api.putChunk("t1", 5, new ArrayBuffer(7));
    FakeXhr.last.status = 204;
    FakeXhr.last.onload!();
    expect(await p).toEqual({ received: 12 });
  });

  it("rejects with the server's code, offset and retry-after", async () => {
    vi.stubGlobal("XMLHttpRequest", FakeXhr);
    const p = api.putChunk("t1", 0, new ArrayBuffer(1));
    const x = FakeXhr.last;
    x.status = 409; x.responseText = JSON.stringify({ code: "offset_mismatch", message: "m", offset: 8 });
    x.respHeaders["retry-after"] = "2";
    x.onload!();
    const err = await p.then(() => { throw new Error("expected rejection"); }, (e: ApiError) => e);
    expect(err.code).toBe("offset_mismatch");
    expect(err.extra).toEqual({ offset: 8, retryAfter: 2 });
  });

  it("maps connection errors to network and aborts to AbortError", async () => {
    vi.stubGlobal("XMLHttpRequest", FakeXhr);
    const p1 = api.putChunk("t", 0, new ArrayBuffer(1));
    FakeXhr.last.onerror!();
    expect(((await p1.catch((e) => e)) as ApiError).code).toBe("network");

    const ac = new AbortController();
    const p2 = api.putChunk("t", 0, new ArrayBuffer(1), { signal: ac.signal });
    ac.abort();
    expect(isAbort(await p2.catch((e) => e))).toBe(true);
  });

  it("reports upload progress", async () => {
    vi.stubGlobal("XMLHttpRequest", FakeXhr);
    const seen: number[] = [];
    const p = api.putChunk("t", 0, new ArrayBuffer(4), { onProgress: (n) => seen.push(n) });
    FakeXhr.last.upload.onprogress!({ loaded: 3 });
    FakeXhr.last.status = 200;
    FakeXhr.last.onload!();
    await p;
    expect(seen).toEqual([3]);
  });
});
