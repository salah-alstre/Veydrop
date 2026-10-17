import { createHash, randomBytes } from "node:crypto";
import { describe, expect, it } from "vitest";
import { Sha256, sha256Hex } from "./sha256";

const enc = (s: string) => new TextEncoder().encode(s);

describe("sha256", () => {
  it("matches the published vectors", () => {
    expect(sha256Hex(enc(""))).toBe("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    expect(sha256Hex(enc("abc"))).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    expect(sha256Hex(enc("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))).toBe(
      "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );
  });

  it("hashes one million 'a' characters", () => {
    expect(sha256Hex(new Uint8Array(1_000_000).fill(97))).toBe(
      "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
    );
  });

  it("is identical when fed in arbitrary chunk sizes", () => {
    const data = new Uint8Array(randomBytes(200_003));
    const want = createHash("sha256").update(data).digest("hex");
    for (const step of [1, 7, 63, 64, 65, 1000, 65536, 200_003]) {
      const h = new Sha256();
      for (let i = 0; i < data.length; i += step) h.update(data.subarray(i, i + step));
      expect(h.length).toBe(data.length);
      expect(h.digestHex()).toBe(want);
    }
  });

  it("handles every padding boundary length", () => {
    for (let n = 0; n <= 200; n++) {
      const data = new Uint8Array(randomBytes(n));
      expect(sha256Hex(data)).toBe(createHash("sha256").update(data).digest("hex"));
    }
  });
});
