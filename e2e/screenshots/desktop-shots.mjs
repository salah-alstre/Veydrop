// Captures the README's desktop screenshots from the REAL release build of Veydrop.
//
//  - The app runs from a scratch copy with VEYDROP_DATA_DIR pointing at a scratch folder, so the
//    developer's real settings, history and downloads are never read or written.
//  - The LAN address and the one-time pairing token are masked in the page before each capture.
//  - It is driven over the WebView2 DevTools port; only the web contents of the app window
//    are captured (no window chrome, terminal, devtools or other applications).
//  - The phone is an emulated Pixel 7 (browser emulation, NOT a physical device).
//  - All files are generated sample files with made-up names.
//
// Usage (from e2e/):  node screenshots/desktop-shots.mjs
import { spawn, execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { randomBytes } from "node:crypto";
import { chromium, devices } from "@playwright/test";

const ROOT = resolve(process.cwd(), "..");
const OUT = join(ROOT, "docs", "screenshots");
const SCRATCH = "C:\\VeydropShots";
const APP = join(SCRATCH, "app");
const DATA = join(SCRATCH, "data");
const WEBVIEW = join(SCRATCH, "webview");
const INBOX = join(SCRATCH, "Received");
const SAMPLES = join(SCRATCH, "samples");
const PORT = 9222;
const W = 1280, H = 1000;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log("[shots]", ...a);

rmSync(SCRATCH, { recursive: true, force: true });
for (const d of [APP, DATA + "\\Roaming", DATA + "\\Local", INBOX, SAMPLES]) mkdirSync(d, { recursive: true });
cpSync(join(ROOT, "target", "release", "veydrop.exe"), join(APP, "veydrop.exe"));
cpSync(join(ROOT, "desktop", "src-tauri", "resources", "mobile"), join(APP, "mobile"), { recursive: true });

const sample = (name, bytes) => writeFileSync(join(SAMPLES, name), randomBytes(bytes));
sample("Summer-trip.jpg", 3_400_000);
sample("Quarterly-report.pdf", 1_250_000);
sample("Demo-video.mp4", 18_000_000);
sample("Design-notes.txt", 4_200);
sample("Album-cover.png", 860_000);
mkdirSync(join(SAMPLES, "Project-files"));
writeFileSync(join(SAMPLES, "Project-files", "readme.txt"), "sample");
writeFileSync(join(SAMPLES, "Project-files", "data.csv"), "a,b\n1,2\n");

writeFileSync(
  join(DATA, "settings.json"),
  JSON.stringify({ language: "en", theme: "dark", downloadDir: INBOX, deviceName: "Studio PC", askBeforeReceive: true, notifications: false }),
);

const child = spawn(join(APP, "veydrop.exe"), [], {
  env: {
    ...process.env,
    VEYDROP_DATA_DIR: DATA,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
    WEBVIEW2_USER_DATA_FOLDER: WEBVIEW,
  },
  stdio: "ignore",
});
let exited = false;
child.on("exit", () => (exited = true));

async function connect() {
  for (let i = 0; i < 60; i++) {
    try {
      const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const p = b.contexts()[0]?.pages()[0];
      if (p) return { b, p };
    } catch {}
    await sleep(500);
  }
  throw new Error("could not attach to the Veydrop window");
}

const { b: browser, p: page } = await connect();
log("attached", page.url());
let phoneBrowser;
const cdp = await browser.newBrowserCDPSession();
try {
  const { windowId } = await cdp.send("Browser.getWindowForTarget", {});
  await cdp.send("Browser.setWindowBounds", { windowId, bounds: { windowState: "normal", width: W + 16, height: H + 39 } });
} catch (e) { log("window resize skipped:", e.message); }
await page.setViewportSize({ width: W, height: H });
await page.waitForSelector("nav");
const invoke = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
// Hides the LAN address and the pairing token (the token also dies with the process).
// React re-renders controlled inputs (the QR countdown ticks every second), so the mask is
// re-applied on a fast timer for as long as the capture takes.
const mask = () => page.evaluate(() => {
  const hide = (t) => t.replace(/[0-9]{1,3}(?:[.][0-9]{1,3}){3}/g, "192.168.1.20").replace(/#t=[A-Za-z0-9_-]+/g, "#t=••••••••••••");
  const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = w.nextNode(); n; n = w.nextNode()) {
    const h = hide(n.nodeValue);
    if (h !== n.nodeValue) n.nodeValue = h;
  }
  // Wrap each field's own `value` accessor so every write (including React's) is masked;
  // timers alone lose the race when the WebView throttles them.
  const run = () => document.querySelectorAll("input, textarea").forEach((el) => {
    if (!el.__masked) {
      el.__masked = true;
      const proto = Object.getPrototypeOf(el);
      const d = Object.getOwnPropertyDescriptor(el, "value") || Object.getOwnPropertyDescriptor(proto, "value");
      if (d && d.set && d.get) {
        Object.defineProperty(el, "value", { configurable: true, get() { return d.get.call(this); }, set(v) { d.set.call(this, hide(String(v))); } });
      }
    }
    const h = hide(el.value);
    if (h !== el.value) el.value = h;
  });
  run();
  window.__maskTimer && clearInterval(window.__maskTimer);
  window.__maskTimer = setInterval(run, 15);
});
const unmask = () => page.evaluate(() => window.__maskTimer && clearInterval(window.__maskTimer));
const shot = async (name) => {
  await sleep(900);
  await mask();
  await sleep(120);
  if (process.env.SHOTS_DEBUG) log("inputs:", JSON.stringify(await page.evaluate(() => [...document.querySelectorAll("input")].map((e) => e.value.slice(0, 40)))));
  await page.screenshot({ path: join(OUT, name) });
  await unmask();
  log("saved", name);
};
const nav = (label) => page.locator("nav").getByText(label, { exact: true }).click();

try {
  // 1. Start the server and pair an emulated phone
  await nav("Connect Device");
  await page.getByRole("button", { name: "Start connection" }).first().click().catch(() => {});
  await page.waitForSelector("svg[aria-label], [role=img]", { timeout: 20000 });
  await sleep(1200);
  const snap = await invoke("get_snapshot");
  const pairUrl = snap.pairing.url;
  log("server up, port", snap.server.port);

  // Connect Device (QR) while waiting for a phone
  await shot("desktop-connect.png");

  phoneBrowser = await chromium.launch();
  const phoneCtx = await phoneBrowser.newContext({ ...devices["Pixel 7"], colorScheme: "dark" });
  const phone = await phoneCtx.newPage();
  await phone.goto(pairUrl);
  await page.getByRole("button", { name: "Approve", exact: true }).waitFor({ timeout: 20000 });
  await shot("desktop-approval.png");
  await page.getByRole("button", { name: "Approve", exact: true }).click();
  await phone.getByText(/^Connected to /).first().waitFor({ timeout: 20000 });

  // 2. Share things so the Send page and the phone's "Receive from PC" have content
  await invoke("add_share_paths", { paths: [join(SAMPLES, "Summer-trip.jpg"), join(SAMPLES, "Quarterly-report.pdf"), join(SAMPLES, "Project-files"), join(SAMPLES, "Demo-video.mp4")] });

  await invoke("set_share_active", { active: true });

  // 3. Phone -> PC upload; accept the incoming offer on the desktop
  await phone.getByRole("button", { name: /Send to PC/ }).first().click();
  await phone.getByRole("heading", { name: "Send to PC" }).waitFor();
  await phone.locator('input[type="file"]:not([webkitdirectory])').first().setInputFiles([join(SAMPLES, "Album-cover.png"), join(SAMPLES, "Design-notes.txt")]);
  await phone.getByRole("button", { name: "Send to PC", exact: true }).last().click();
  await page.getByRole("button", { name: "Accept" }).first().waitFor({ timeout: 20000 });
  await shot("desktop-incoming.png");
  await page.getByRole("button", { name: "Accept" }).first().click();
  await sleep(2500);

  await nav("Dashboard");
  await shot("desktop-dashboard.png");
  await nav("Send Files");
  await shot("desktop-send.png");
  await nav("Receive Files");
  await shot("desktop-receive.png");
  await nav("Activity");
  await shot("desktop-activity.png");
  await nav("Settings");
  await shot("desktop-settings.png");
  await nav("Connect Device");
  await shot("desktop-connect-paired.png");
  await phoneCtx.close();
} finally {
  // Kill the token: regenerate, stop the server, quit.
  try { await invoke("regenerate_pairing"); } catch {}
  try { await invoke("stop_server"); } catch {}
  await phoneBrowser?.close().catch(() => {});
  await browser.close().catch(() => {});
  try { execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" }); } catch {}
  await sleep(800);
  log("app closed:", exited || true);
}
process.exit(0);
