import { useCallback, useEffect, useRef, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { api } from "./lib/api";
import { connectEvents, type LinkState } from "./lib/events";
import { applyTheme, loadPrefs, savePrefs, type Prefs } from "./lib/prefs";
import { startPairing, takePairingToken, type PairState } from "./lib/pairing";
import { SendQueue } from "./lib/sendQueue";
import type { ShareInfo } from "./lib/types";
import { followDesktopLanguage } from "./lib/language";
import { Ended } from "./pages/Ended";
import { Home } from "./pages/Home";
import { Pairing } from "./pages/Pairing";
import { Receive } from "./pages/Receive";
import { Send } from "./pages/Send";
import { LinkBanner } from "./ui/LinkBanner";
import { SettingsSheet } from "./ui/SettingsSheet";

type Route = "home" | "send" | "receive";

/**
 * The pairing token is read exactly once, at load: reading it removes it from the address bar,
 * and it is single-use on the PC, so a second read (or a dev-mode double mount) must not happen.
 */
const INITIAL_TOKEN = takePairingToken();

export default function App() {
  const reduce = useReducedMotion();
  const token = useRef<string | null>(INITIAL_TOKEN);
  const [pair, setPair] = useState<PairState>({ kind: "checking" });
  const [attempt, setAttempt] = useState(0);
  const [ended, setEnded] = useState(false);
  const [route, setRoute] = useState<Route>("home");
  const [link, setLink] = useState<LinkState>("connecting");
  const [share, setShare] = useState<ShareInfo | null>(null);
  const [prefs, setPrefs] = useState<Prefs>(loadPrefs);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [queue] = useState(() => new SendQueue());

  const paired = pair.kind === "paired";
  const pcName = pair.kind === "paired" ? pair.pcName : "";

  // ---- pairing ----------------------------------------------------------------------------
  useEffect(() => {
    const h = startPairing(token.current, setPair);
    return () => h.cancel();
  }, [attempt]);

  // ---- preferences ------------------------------------------------------------------------
  const changePrefs = useCallback((p: Prefs) => {
    setPrefs(p);
    savePrefs(p);
    applyTheme(p.theme);
  }, []);

  useEffect(() => {
    if (prefs.theme !== "system" || typeof matchMedia !== "function") return;
    const mq = matchMedia("(prefers-color-scheme: light)");
    const on = () => applyTheme("system");
    mq.addEventListener?.("change", on);
    return () => mq.removeEventListener?.("change", on);
  }, [prefs.theme]);

  // ---- live connection --------------------------------------------------------------------
  const refreshShare = useCallback(async () => {
    try {
      setShare(await api.getShare());
    } catch (e) {
      if ((e as { code?: string }).code === "unauthorized") setEnded(true);
    }
  }, []);

  const resync = useCallback(async () => {
    try {
      const s = await api.getSession();
      if (!s.paired) return setEnded(true);
      await followDesktopLanguage(s.language);
      await refreshShare();
      queue.applySnapshot(await api.getTransfers());
      await queue.refreshOffer();
    } catch (e) {
      if ((e as { code?: string }).code === "unauthorized") setEnded(true);
      // network errors: the event stream is already retrying and will resync on reconnect
    }
  }, [queue, refreshShare]);

  useEffect(() => {
    if (!paired || ended) return;
    const close = connectEvents({
      onLink: setLink,
      onHello: (h) => {
        void followDesktopLanguage(h.language);
        queue.applySnapshot(h.transfers);
        void refreshShare();
        void queue.refreshOffer();
      },
      onLanguage: (l) => void followDesktopLanguage(l),
      onShare: () => void refreshShare(),
      onOffer: (o) => queue.handleOffer(o),
      onTransfer: (e) => queue.handleTransfer(e),
      onCancelled: (id) => queue.handleCancelled(id),
      onRevoked: () => {
        queue.cancelAll();
        setEnded(true);
      },
      onResync: () => void resync(),
    });
    return close;
  }, [paired, ended, queue, refreshShare, resync]);

  // Mobile browsers freeze background tabs: when the page returns, catch up instead of polling.
  useEffect(() => {
    if (!paired || ended) return;
    const onVis = () => {
      if (document.visibilityState === "visible") void resync();
    };
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  }, [paired, ended, resync]);

  const disconnect = useCallback(async () => {
    queue.cancelAll();
    try {
      await api.logout();
    } catch {
      /* already gone */
    }
    setEnded(true);
  }, [queue]);

  // ---- render -----------------------------------------------------------------------------
  let body;
  if (ended) {
    body = <Ended />;
  } else if (pair.kind !== "paired") {
    body = <Pairing state={pair} onRetry={() => setAttempt((n) => n + 1)} />;
  } else {
    body = (
      <>
        <LinkBanner link={link} />
        <motion.div
          key={route}
          initial={{ opacity: 0, y: reduce ? 0 : 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: reduce ? 0 : 0.2, ease: [0.2, 0.7, 0.2, 1] }}
        >
          {route === "home" && (
            <Home
              pcName={pcName}
              share={share}
              onSend={() => setRoute("send")}
              onReceive={() => {
                setRoute("receive");
                void refreshShare();
              }}
              onSettings={() => setSettingsOpen(true)}
              onDisconnect={disconnect}
            />
          )}
          {route === "send" && <Send queue={queue} onBack={() => setRoute("home")} />}
          {route === "receive" && <Receive share={share} onBack={() => setRoute("home")} />}
        </motion.div>
      </>
    );
  }

  return (
    <>
      {body}
      <SettingsSheet open={settingsOpen} prefs={prefs} onChange={changePrefs} onClose={() => setSettingsOpen(false)} />
    </>
  );
}
