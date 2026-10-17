import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import {
  Activity as ActivityIcon, Download, EyeOff, LayoutDashboard, Link2, Power, Send as SendIcon,
  Settings as SettingsIcon, ShieldCheck, Smartphone, UploadCloud,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { applyLanguage, resolveLang } from "./i18n";
import { PairCode } from "./ui/PairCode";
import { ipc } from "./lib/ipc";
import { clearCloseInfo, onEngineEvent, toast, useApp } from "./lib/store";
import { useErrorText, useFmt, useThemeSync } from "./lib/hooks";
import type { PairRequestInfo } from "./lib/types";
import { Button } from "./ui/Button";
import { Logo, Wordmark } from "./ui/Logo";
import { Modal } from "./ui/Modal";
import { OfferCard } from "./ui/OfferCard";
import { Badge } from "./ui/Primitives";
import { Splash } from "./ui/Splash";
import { Toaster } from "./ui/Toaster";
import { Activity } from "./pages/Activity";
import { Connect } from "./pages/Connect";
import { Dashboard } from "./pages/Dashboard";
import { Receive } from "./pages/Receive";
import { Send } from "./pages/Send";
import { Settings } from "./pages/Settings";
import type { PageId } from "./pages/types";

const NAV: { id: PageId; icon: typeof LayoutDashboard; key: string }[] = [
  { id: "dashboard", icon: LayoutDashboard, key: "nav.dashboard" },
  { id: "connect", icon: Smartphone, key: "nav.connect" },
  { id: "send", icon: SendIcon, key: "nav.send" },
  { id: "receive", icon: Download, key: "nav.receive" },
  { id: "activity", icon: ActivityIcon, key: "nav.activity" },
  { id: "settings", icon: SettingsIcon, key: "nav.settings" },
];

function PairRequestModal({ req, queued }: { req: PairRequestInfo | undefined; queued: number }) {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const [busy, setBusy] = useState<"approve" | "reject" | null>(null);
  useEffect(() => setBusy(null), [req?.id]);

  const decide = async (approve: boolean) => {
    if (!req) return;
    setBusy(approve ? "approve" : "reject");
    try { await ipc.decidePair(req.id, approve); }
    catch (e) { toast("error", errText(e)); setBusy(null); }
  };

  return (
    <Modal
      open={!!req}
      title={t("pair.title")}
      icon={<Link2 size={18} />}
      footer={
        <>
          <Button variant="danger" busy={busy === "reject"} disabled={busy === "approve"} onClick={() => decide(false)}>{t("pair.reject")}</Button>
          <Button variant="primary" busy={busy === "approve"} disabled={busy === "reject"} onClick={() => decide(true)}>{t("pair.approve")}</Button>
        </>
      }
    >
      {req && (
        <div className="flex flex-col gap-4">
          <p className="text-sm text-muted">{t("pair.desc")}</p>
          <div className="rounded-xl border border-line bg-surface-2/60 p-4">
            <div className="flex items-center gap-3">
              <div className="size-10 rounded-xl bg-surface-3 border border-line grid place-items-center text-accent"><Smartphone size={18} /></div>
              <div className="min-w-0">
                <div className="text-sm font-semibold truncate" dir="auto">{req.deviceLabel}</div>
                <div className="text-xs text-muted truncate">{req.platform} · <span className="num ltr inline-block">{req.remoteIp}</span></div>
              </div>
              {req.trusted && <span className="ms-auto"><Badge tone="success">{t("connect.trusted")}</Badge></span>}
            </div>
          </div>
          <div className="text-center">
            <div className="text-xs text-muted mb-2">{t("pair.codeHint")}</div>
            <PairCode code={req.code} label={t("pair.code")} className="text-4xl font-semibold text-accent" />
          </div>
          <p className="text-xs text-faint">{t("pair.warn")}</p>
          {queued > 0 && <p className="text-xs text-muted">{t("pair.queued", { n: f.num(queued) })}</p>}
        </div>
      )}
    </Modal>
  );
}

function CloseModal() {
  const { t } = useTranslation();
  const { closeInfo } = useApp();
  const f = useFmt();
  const [busy, setBusy] = useState(false);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try { await fn(); } finally { setBusy(false); clearCloseInfo(); }
  };
  const body = closeInfo
    ? closeInfo.activeTransfers > 0
      ? t("close.activeBody", { n: f.num(closeInfo.activeTransfers) })
      : closeInfo.serverRunning
        ? t("close.serverBody")
        : t("close.trayBody")
    : "";

  return (
    <Modal
      open={!!closeInfo}
      title={t("close.title")}
      icon={<Power size={18} />}
      onClose={clearCloseInfo}
      footer={
        <>
          <Button onClick={clearCloseInfo} disabled={busy}>{t("common.cancel")}</Button>
          <Button variant="danger" busy={busy} onClick={() => run(ipc.quitApp)}>{t("close.quit")}</Button>
          <Button variant="primary" icon={<EyeOff size={16} />} disabled={busy} onClick={() => run(ipc.hideToTray)}>{t("close.hide")}</Button>
        </>
      }
    >
      <p className="text-sm text-muted">{body}</p>
      {closeInfo?.serverRunning && (
        <p className="mt-3 text-xs text-faint flex items-start gap-2">
          <ShieldCheck size={14} className="mt-0.5 shrink-0" />
          <span>{t("close.quitNote")}</span>
        </p>
      )}
    </Modal>
  );
}

export function App() {
  const { t, i18n } = useTranslation();
  const reduce = useReducedMotion();
  const { loaded, loadError, snap, closeInfo } = useApp();
  const [page, setPage] = useState<PageId>("dashboard");
  const [dragging, setDragging] = useState(false);
  const readyRef = useRef(false);
  const pageRef = useRef(page);
  pageRef.current = page;
  const go = useCallback((p: PageId) => setPage(p), []);

  const settings = snap?.settings;
  useThemeSync(settings?.theme ?? "system");

  // Language follows the setting ("system" → OS language) and flips direction instantly.
  const langSetting = settings?.language;
  useEffect(() => {
    if (!langSetting) return;
    void applyLanguage(resolveLang(langSetting));
    // Phones mirror what this PC shows, including when the setting is "system".
    ipc.setSystemLanguage(resolveLang("system")).catch(() => {});
  }, [langSetting]);

  // Tray menu strings are native, so they are pushed to Rust whenever the language changes.
  useEffect(() => {
    if (!loaded) return;
    ipc.setTrayLabels(t("tray.open"), t("tray.stop"), t("tray.quit"), t("tray.tooltip")).catch(() => {});
  }, [loaded, i18n.language, t]);

  // Show the native window only after the first real paint (no white flash).
  useEffect(() => {
    if (!loaded || readyRef.current) return;
    readyRef.current = true;
    requestAnimationFrame(() => requestAnimationFrame(() => { ipc.uiReady().catch(() => {}); }));
  }, [loaded]);

  // Native notifications for things that need attention while the window is hidden.
  useEffect(() => onEngineEvent((ev) => {
    if (ev.type === "pairRequest") {
      ipc.notify(t("notify.pairTitle"), t("notify.pairBody", { name: ev.data.deviceLabel })).catch(() => {});
    } else if (ev.type === "offer" && ev.data.state === "pending") {
      ipc.notify(t("notify.offerTitle"), t("notify.offerBody", { name: ev.data.deviceName })).catch(() => {});
    } else if (ev.type === "transferDone") {
      const x = ev.data;
      if (x.state === "completed" && x.direction === "incoming") {
        ipc.notify(t("notify.receivedTitle"), x.name).catch(() => {});
      } else if (x.state === "failed") {
        ipc.notify(t("notify.failedTitle"), x.name).catch(() => {});
      }
    }
  }), [t]);

  // Drag & drop of files/folders onto the window adds them to the share.
  useEffect(() => {
    let off: (() => void) | undefined;
    let dead = false;
    getCurrentWebview().onDragDropEvent(async (e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setDragging(true);
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        if (!p.paths.length) return;
        try {
          const r = await ipc.addSharePaths(p.paths);
          setPage("send");
          if (r.skipped > 0) toast("info", t("send.skipped", { n: r.skipped }));
        } catch (err) {
          toast("error", t(`errors.${(err as { code?: string })?.code ?? "other"}`, { defaultValue: t("errors.other") }));
        }
      }
    }).then((u) => { if (dead) u(); else off = u; }).catch(() => {});
    return () => { dead = true; off?.(); };
  }, [t]);

  const pairQueue = snap?.pairRequests ?? [];
  const pendingOffers = useMemo(() => (snap?.offers ?? []).filter((o) => o.state === "pending"), [snap?.offers]);
  const offerModal = page !== "receive" ? pendingOffers[0] : undefined;
  const badge = (id: PageId) => (id === "receive" ? pendingOffers.length : 0);

  if (!loaded || !snap) return <Splash error={loadError} />;
  if (loadError) return <Splash error={loadError} />;

  const { server, sessions } = snap;
  const online = sessions.filter((s) => s.online).length;
  const f = { num: (n: number) => new Intl.NumberFormat(i18n.language === "ar" ? "ar-EG" : "en-US").format(n) };

  const Page = { dashboard: Dashboard, connect: Connect, send: Send, receive: Receive, activity: Activity, settings: Settings }[page];

  return (
    <div className="h-full flex bg-bg text-fg">
      <aside className="w-60 shrink-0 border-e border-line bg-surface/60 flex flex-col">
        <div className="px-5 pt-6 pb-5 flex items-center gap-3">
          <Logo size={34} animated={server.running} />
          <div>
            <Wordmark size={18} />
            <div className="text-[11px] text-faint leading-tight">{t("splash.tagline")}</div>
          </div>
        </div>

        <nav className="px-3 flex-1 flex flex-col gap-1" aria-label={t("nav.label")}>
          {NAV.map(({ id, icon: Icon, key }) => {
            const active = page === id;
            const n = badge(id);
            return (
              <button
                key={id}
                type="button"
                onClick={() => setPage(id)}
                aria-current={active ? "page" : undefined}
                className={`relative flex items-center gap-3 h-10 px-3 rounded-[10px] text-sm text-start transition-colors duration-200 outline-none focus-visible:ring-2 focus-visible:ring-accent/60 ${active ? "bg-accent-soft text-fg" : "text-muted hover:text-fg hover:bg-surface-2"}`}
              >
                {active && <motion.span layoutId="nav-active" transition={{ duration: reduce ? 0 : 0.2 }} className="absolute start-0 top-2 bottom-2 w-[3px] rounded-full bg-accent" />}
                <Icon size={17} className={active ? "text-accent" : ""} />
                <span className="flex-1">{t(key)}</span>
                {n > 0 && <span className="min-w-5 h-5 px-1.5 rounded-full bg-accent text-accent-ink text-[11px] font-semibold grid place-items-center num">{f.num(n)}</span>}
              </button>
            );
          })}
        </nav>

        <div className="m-3 rounded-xl border border-line bg-surface-2/60 p-3">
          <div className="flex items-center gap-2 text-xs">
            <span className={`size-2 rounded-full ${!server.running ? "bg-faint" : server.addressLost ? "bg-warn" : online > 0 ? "bg-success" : "bg-accent vd-dot"}`} />
            <span className="text-muted">
              {!server.running ? t("dashboard.status.off") : server.addressLost ? t("dashboard.status.addressLost") : online > 0 ? t("dashboard.status.connected", { n: f.num(online) }) : t("dashboard.status.waiting")}
            </span>
          </div>
          {server.running && server.ip && <div className="mt-1.5 text-[11px] text-faint num ltr truncate">{server.ip}:{server.port}</div>}
        </div>
      </aside>

      <main className="flex-1 min-w-0 overflow-y-auto">
        <div className="max-w-4xl mx-auto px-8 py-8">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={page}
              initial={reduce ? false : { opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={reduce ? undefined : { opacity: 0 }}
              transition={{ duration: reduce ? 0 : 0.18 }}
            >
              <Page go={go} />
            </motion.div>
          </AnimatePresence>
        </div>
      </main>

      <AnimatePresence>
        {dragging && (
          <motion.div
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.15 }}
            className="fixed inset-0 z-50 bg-bg/80 backdrop-blur-sm grid place-items-center pointer-events-none"
          >
            <div className="border-2 border-dashed border-accent/60 rounded-3xl px-16 py-14 text-center bg-surface/70">
              <UploadCloud size={40} className="mx-auto text-accent" />
              <div className="mt-4 text-lg font-semibold">{t("drop.title")}</div>
              <div className="text-sm text-muted mt-1">{t("drop.desc")}</div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <PairRequestModal req={pairQueue[0]} queued={Math.max(0, pairQueue.length - 1)} />

      <Modal open={!!offerModal && !closeInfo} title={t("offer.title")} icon={<Download size={18} />} width={500}>
        {offerModal && <OfferCard key={offerModal.id} offer={offerModal} compact />}
      </Modal>

      <CloseModal />
      <Toaster />
    </div>
  );
}
