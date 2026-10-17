import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createHash, randomBytes } from "node:crypto";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const exe = join(root, "target", "debug", "examples", process.platform === "win32" ? "e2e_harness.exe" : "e2e_harness");
const mobileDir = join(root, "desktop", "src-tauri", "resources", "mobile");

export type HarnessEvent = { event: string; [k: string]: unknown };

export interface Fixtures {
  /** Files the PC shares for the Receive tests. */
  shared: { small: string; big: string; folder: string; smallSha: string };
  /** A directory for files the phone will send. */
  upload: string;
}

/**
 * One real engine + server per test. The process plays "the person at the PC" (approve pairing,
 * accept offers) through stdin; everything the phone talks to is the real server.
 */
export class Harness {
  readonly events: HarnessEvent[] = [];
  readonly work: string;
  readonly fixtures: Fixtures;
  pairUrl = "";
  origin = "";
  downloads = "";
  private proc!: ChildProcessWithoutNullStreams;
  private waiters: { pred: (e: HarnessEvent) => boolean; resolve: (e: HarnessEvent) => void }[] = [];
  private buf = "";
  stderr = "";

  private constructor(work: string, fixtures: Fixtures) {
    this.work = work;
    this.fixtures = fixtures;
  }

  static async start(opts: { share?: boolean } = {}): Promise<Harness> {
    const work = mkdtempSync(join(tmpdir(), "veydrop-e2e-"));
    const sharedDir = join(work, "shared");
    mkdirSync(join(sharedDir, "Trip", "day1"), { recursive: true });
    const small = join(sharedDir, "hello.txt");
    writeFileSync(small, "hello from the pc\n");
    const big = join(sharedDir, "big.bin");
    const bigBytes = randomBytes(6 * 1024 * 1024);
    writeFileSync(big, bigBytes);
    writeFileSync(join(sharedDir, "Trip", "notes.txt"), "trip notes");
    writeFileSync(join(sharedDir, "Trip", "day1", "photo.bin"), randomBytes(2048));
    const upload = join(work, "upload");
    mkdirSync(upload);
    const fixtures: Fixtures = {
      shared: {
        small,
        big,
        folder: join(sharedDir, "Trip"),
        smallSha: createHash("sha256").update("hello from the pc\n").digest("hex"),
      },
      upload,
    };
    const h = new Harness(work, fixtures);
    const args = [mobileDir, work];
    if (opts.share) args.push(small, big, fixtures.shared.folder);
    h.proc = spawn(exe, args, { stdio: ["pipe", "pipe", "pipe"] });
    h.proc.stdout.setEncoding("utf8");
    h.proc.stderr.setEncoding("utf8");
    h.proc.stdout.on("data", (d: string) => h.onData(d));
    h.proc.stderr.on("data", (d: string) => (h.stderr += d));
    const ready = await h.wait((e) => e.event === "ready" || e.event === "error", 20_000);
    if (ready.event === "error") throw new Error(`harness failed: ${String(ready.message)}`);
    h.pairUrl = String(ready.pairUrl);
    h.origin = new URL(h.pairUrl).origin;
    h.downloads = String(ready.downloads);
    return h;
  }

  private onData(d: string) {
    this.buf += d;
    let i: number;
    while ((i = this.buf.indexOf("\n")) >= 0) {
      const line = this.buf.slice(0, i).trim();
      this.buf = this.buf.slice(i + 1);
      if (!line) continue;
      try {
        const e = JSON.parse(line) as HarnessEvent;
        this.events.push(e);
        this.waiters = this.waiters.filter((w) => {
          if (!w.pred(e)) return true;
          w.resolve(e);
          return false;
        });
      } catch {
        /* not ours */
      }
    }
  }

  /** Resolves with the first matching event, including ones that already happened. */
  wait(pred: (e: HarnessEvent) => boolean, ms = 15_000): Promise<HarnessEvent> {
    const past = this.events.find(pred);
    if (past) return Promise.resolve(past);
    return new Promise((res, rej) => {
      const t = setTimeout(() => rej(new Error("timed out waiting for harness event")), ms);
      this.waiters.push({
        pred,
        resolve: (e) => {
          clearTimeout(t);
          res(e);
        },
      });
    });
  }

  count(name: string): number {
    return this.events.filter((e) => e.event === name).length;
  }

  cmd(line: string) {
    this.proc.stdin.write(line + "\n");
  }

  /** A fresh single-use pairing link (the previous one stays valid until used or expired). */
  async regenerate(): Promise<string> {
    const before = this.count("pairing");
    this.cmd("regen");
    const e = await this.wait((x) => x.event === "pairing" && this.count("pairing") > before);
    const latest = [...this.events].reverse().find((x) => x.event === "pairing")!;
    this.pairUrl = String(latest.pairUrl ?? e.pairUrl);
    return this.pairUrl;
  }

  async stop() {
    try {
      this.cmd("quit");
    } catch {
      /* already gone */
    }
    await new Promise<void>((res) => {
      const t = setTimeout(() => {
        this.proc.kill();
        res();
      }, 5000);
      this.proc.once("exit", () => {
        clearTimeout(t);
        res();
      });
    });
    try {
      rmSync(this.work, { recursive: true, force: true });
    } catch {
      /* temp cleanup is best effort */
    }
  }
}
