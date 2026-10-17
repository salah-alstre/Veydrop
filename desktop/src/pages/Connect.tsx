import { useEffect, useState } from "react";
import { AlertTriangle, Check, Copy, Lock, Power, RefreshCw, Smartphone, Unplug } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { toast, useApp } from "../lib/store";
import { useErrorText, useFmt, useTick } from "../lib/hooks";
import type { Diagnostics } from "../lib/types";
import { Button, IconButton } from "../ui/Button";
import { LinkLoader, Skeleton } from "../ui/Loader";
import { Badge, Card, EmptyState, PageHeader, SectionTitle, inputCls } from "../ui/Primitives";
import { DiagnosticsPanel, IfaceSelect } from "./DiagnosticsPanel";
import { QrCode } from "../ui/QrCode";

export function Connect() {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const { snap } = useApp();
  const [busy, setBusy] = useState<"start" | "stop" | "regen" | null>(null);
  const [copied, setCopied] = useState(false);
  const [diag, setDiag] = useState<Diagnostics | null>(null);

  const running = !!snap?.server.running;
  const pairing = snap?.pairing ?? null;
  useTick(1000, running && !!pairing);

  const sessionsKey = snap?.sessions.length ?? 0;
  const addrLost = snap?.server.addressLost;
  useEffect(() => {
    if (!running) { setDiag(null); return; }
    ipc.diagnostics().then(setDiag).catch(() => {});
  }, [running, sessionsKey, addrLost, snap?.network, snap?.server.phoneSeen, snap?.server.ip, snap?.server.port]);

  if (!snap) return null;
  const { sessions, network, settings, pairRequests } = snap;

  const run = async (kind: "start" | "stop" | "regen", fn: () => Promise<unknown>) => {
    setBusy(kind);
    try { await fn(); } catch (e) { toast("error", errText(e)); } finally { setBusy(null); }
  };

  const copy = async () => {
    if (!pairing) return;
    try {
      await navigator.clipboard.writeText(pairing.url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch { toast("error", t("connect.copyFailed")); }
  };

  const chooseIface = (ip: string) =>
    ipc.updateSettings({ preferredIp: ip }).catch((e) => toast("error", errText(e)));

  const remaining = pairing ? Math.max(0, Math.ceil((pairing.expiresAt - Date.now()) / 1000)) : 0;
  const expired = !!pairing && remaining === 0 && !pairing.used;

  return (
    <div>
      <PageHeader
        title={t("connect.title")}
        subtitle={t("connect.subtitle")}
        actions={
          running ? (
            <Button variant="danger" icon={<Power size={16} />} busy={busy === "stop"} onClick={() => run("stop", ipc.stopServer)}>
              {t("connect.stop")}
            </Button>
          ) : undefined
        }
      />

      {!running ? (
        <Card pad={false}>
          <div className="p-8 flex flex-col items-center text-center gap-5">
            <LinkLoader state="failed" width={240} />
            <div>
              <div className="text-lg font-semibold">{t("connect.offTitle")}</div>
              <p className="text-sm text-muted mt-1 max-w-md">{t("connect.offDesc")}</p>
            </div>
            {network.length > 1 && (
              <label className="flex flex-col gap-1.5 text-start w-full max-w-sm">
                <span className="text-xs font-medium text-muted">{t("connect.interface")}</span>
                <IfaceSelect network={network} value={settings.preferredIp} onChange={chooseIface} />
              </label>
            )}
            {network.length === 0 && (
              <div className="flex items-center gap-2 text-warn text-sm"><AlertTriangle size={16} />{t("connect.noNetwork")}</div>
            )}
            <Button variant="primary" size="lg" icon={<Smartphone size={18} />} busy={busy === "start"} disabled={network.length === 0}
              onClick={() => run("start", ipc.startServer)}>
              {t("connect.start")}
            </Button>
          </div>
        </Card>
      ) : (
        <div className="grid gap-4 lg:grid-cols-[auto_1fr]">
          <Card className="flex flex-col items-center gap-4 lg:w-[320px]">
            {pairing ? (
              <div className={`relative transition-opacity duration-200 ${expired || pairing.used ? "opacity-25" : ""}`}>
                <QrCode matrix={pairing.qr} size={248} label={t("connect.qrLabel")} />
              </div>
            ) : (
              <Skeleton className="size-[248px] rounded-2xl" />
            )}

            {pairing && (
              <>
                <div className="text-xs text-muted h-5">
                  {pairing.used ? (
                    <span className="text-success">{t("connect.used")}</span>
                  ) : expired ? (
                    <span className="text-warn">{t("connect.expired")}</span>
                  ) : (
                    <span>{t("connect.expiresIn", { time: f.clock(remaining) })}</span>
                  )}
                </div>
                <div className="w-full flex items-center gap-1">
                  <input readOnly value={pairing.url} aria-label={t("connect.url")} className={`${inputCls} ltr num selectable text-xs`} onFocus={(e) => e.currentTarget.select()} />
                  <IconButton label={copied ? t("common.copied") : t("connect.copy")} onClick={copy}>
                    {copied ? <Check size={16} className="text-success" /> : <Copy size={16} />}
                  </IconButton>
                </div>
                <Button className="w-full" icon={<RefreshCw size={15} />} busy={busy === "regen"} onClick={() => run("regen", ipc.regeneratePairing)}>
                  {t("connect.regenerate")}
                </Button>
              </>
            )}
          </Card>

          <div className="flex flex-col gap-4 min-w-0">
            <Card>
              <SectionTitle aside={<Badge tone={sessions.some((s) => s.online) ? "success" : "accent"} dot>
                {sessions.some((s) => s.online) ? t("connect.state.connected") : pairRequests.length ? t("connect.state.approval") : t("connect.state.waiting")}
              </Badge>}>{t("connect.steps.title")}</SectionTitle>
              <ol className="text-sm text-muted space-y-1.5 list-decimal ps-5 marker:text-faint">
                <li>{t("connect.steps.one")}</li>
                <li>{t("connect.steps.two")}</li>
                <li>{t("connect.steps.three")}</li>
              </ol>
              <div className="mt-4 flex items-start gap-2 text-xs text-muted border-t border-line pt-3">
                <Lock size={14} className="mt-0.5 shrink-0 text-faint" />
                <span>{t("connect.plainHttp")}</span>
              </div>
            </Card>

            <Card pad={false}>
              <div className="px-5 pt-5"><SectionTitle>{t("connect.devices")}</SectionTitle></div>
              {sessions.length === 0 ? (
                <EmptyState icon={<Smartphone size={20} />} title={t("connect.noDevices")} desc={t("connect.noDevicesDesc")} />
              ) : (
                <ul className="px-5 pb-3">
                  {sessions.map((s) => (
                    <li key={s.id} className="flex items-center gap-3 py-3 border-b border-line last:border-b-0">
                      <div className="size-9 rounded-xl bg-surface-2 border border-line grid place-items-center"><Smartphone size={16} className={s.online ? "text-success" : "text-faint"} /></div>
                      <div className="min-w-0 flex-1">
                        <div className="text-sm font-medium truncate" dir="auto">{s.deviceLabel}</div>
                        <div className="text-xs text-muted flex flex-wrap gap-x-3">
                          <span>{s.platform}</span>
                          <span className="num ltr">{s.remoteIp}</span>
                          <span>{s.online ? t("connect.online") : t("connect.offline")}</span>
                          <span>{t("connect.since", { time: f.time(s.connectedAt) })}</span>
                        </div>
                      </div>
                      {s.trusted && <Badge tone="accent">{t("connect.trusted")}</Badge>}
                      <Button size="sm" variant="danger" icon={<Unplug size={14} />} onClick={() => ipc.revokeSession(s.id).catch((e) => toast("error", errText(e)))}>
                        {t("connect.disconnect")}
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </Card>

            <DiagnosticsPanel diag={diag} network={network} preferredIp={settings.preferredIp} pairedPhones={sessions.length} onDiag={setDiag} />
          </div>
        </div>
      )}
    </div>
  );
}
