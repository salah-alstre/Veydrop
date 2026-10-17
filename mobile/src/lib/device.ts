const KEY = "vd.deviceId";
const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

export function randomId(len = 24): string {
  const bytes = new Uint8Array(len);
  crypto.getRandomValues(bytes);
  let out = "";
  for (const b of bytes) out += ALPHABET[b & 63];
  return out;
}

let memoryId: string | null = null;

/** A stable per-browser id so the PC can recognise a returning phone. Falls back to memory if storage is blocked. */
export function getDeviceId(): string {
  try {
    const stored = localStorage.getItem(KEY);
    if (stored && /^[A-Za-z0-9_-]{8,64}$/.test(stored)) return stored;
    const id = randomId();
    localStorage.setItem(KEY, id);
    return id;
  } catch {
    memoryId ??= randomId();
    return memoryId;
  }
}

export interface DeviceDescription { label: string; platform: string }

/** Human label and platform guess from the user agent. Only used for display on the PC. */
export function describeDevice(ua: string = navigator.userAgent, maxTouch: number = navigator.maxTouchPoints ?? 0): DeviceDescription {
  if (/iPhone/i.test(ua)) return { label: "iPhone", platform: "ios" };
  if (/iPad/i.test(ua) || (/Macintosh/i.test(ua) && maxTouch > 1)) return { label: "iPad", platform: "ios" };
  if (/Android/i.test(ua)) {
    const model = /Android[^;]*;\s*([^;)]+?)(?:\sBuild|\)|;)/i.exec(ua)?.[1]?.trim();
    const clean = model && model.length <= 32 && !/^(wv|mobile|k)$/i.test(model) ? model : "Android";
    return { label: clean, platform: "android" };
  }
  if (/Windows/i.test(ua)) return { label: "Windows PC", platform: "windows" };
  if (/Macintosh|Mac OS X/i.test(ua)) return { label: "Mac", platform: "macos" };
  if (/Linux/i.test(ua)) return { label: "Linux", platform: "linux" };
  return { label: "Phone", platform: "other" };
}
