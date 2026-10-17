import { useState, type ReactNode } from "react";
import { AlertTriangle, Check, CircleHelp, Copy, RefreshCw, ShieldAlert, Stethoscope, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { toast } from "../lib/store";
import { useErrorText, useFmt } from "../lib/hooks";
import type { Diagnostics, FirewallState, NetIface } from "../lib/types";
import { Button } from "../ui/Button";
import { Badge, Card, SectionTitle, inputCls, type Tone } from "../ui/Primitives";

type Mark = "ok" | "bad" | "warn" | "unknown";

function MarkIcon({ mark }: { mark: Mark }) {
  if (mark === "ok") return <Check size={15} className="text-success" />;
  if (mark === "bad") return <X size={15} className="text-danger" />;
  if (mark === "warn") return <AlertTriangle size={15} className="text-warn" />;
  return <CircleHelp size={15} className="text-faint" />;
}

function Row({ label, mark, children, note }: { label: string; mark?: Mark; children: ReactNode; note?: string }) {
  return (
    <div className="py-2.5 border-b border-line last:border-b-0">
      <div className="flex items-start gap-3">
        <div className="w-40 shrink-0 text-xs font-medium text-muted pt-0.5">{label}</div>
        <div className="min-w-0 flex-1 text-sm flex items-start gap-2">
          {mark && <span className="mt-0.5 shrink-0"><MarkIcon mark={mark} /></span>}
          <span className="min-w-0 break-words">{children}</span>
        </div>
      </div>
      {note && <p className="text-xs text-faint mt-1 ps-[10.75rem]">{note}</p>}
    </div>
  );
}

const fwMark: Record<FirewallState, Mark> = { allowed: "ok", blocked: "bad", no_rule: "bad", off: "warn", unknown: "unknown" };
const fwTone: Record<FirewallState, Tone> = { allowed: "success", blocked: "danger", no_rule: "warn", off: "neutral", unknown: "neutral" };

export function IfaceSelect({ network, value, onChange }: { network: NetIface[]; value: string; onChange: (ip: string) => void }) {
  const { t } = useTranslation();
  return (
    <select className={inputCls} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="">{t("connect.autoInterface")}</option>
      {network.map((n) => (
        <option key={n.ip} value={n.ip}>
          {n.name} · {n.ip} · {t(`iface.${n.kind}`)}{n.recommended ? ` · ${t("connect.recommended")}` : ""}
        </option>
      ))}
    </select>
  );
}

interface Props {
  diag: Diagnostics | null;
  network: NetIface[];
  preferredIp: string;
  pairedPhones: number;
  onDiag: (d: Diagnostics) => void;
}

/** Honest connectivity diagnostics: local self-checks are kept visibly apart
 *  from proof that another device actually reached this PC. */
export function DiagnosticsPanel({ diag, network, preferredIp, pairedPhones, onDiag }: Props) {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const [busy, setBusy] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [copied, setCopied] = useState(false);

  const runDiag = async () => {
    setBusy(true);
    try { onDiag(await ipc.diagnostics(true)); }
    catch (e) { toast("error", `${t("diag.failed")} ${errText(e)}`); }
    finally { setBusy(false); }
  };

  const switchTo = async (ip: string) => {
    if (pairedPhones > 0 && !window.confirm(t("diag.switchConfirm", { n: pairedPhones }))) return;
    setSwitching(true);
    try {
      await ipc.updateSettings({ preferredIp: ip });
      await ipc.restartServer();
      onDiag(await ipc.diagnostics(diag?.deep ?? false));
    } catch (e) { toast("error", `${t("diag.switchFailed")} ${errText(e)}`); }
    finally { setSwitching(false); }
  };

  const copyCmd = async () => {
    if (!diag?.firewall?.suggestedCommand) return;
    try {
      await navigator.clipboard.writeText(diag.firewall.suggestedCommand);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch { toast("error", t("connect.copyFailed")); }
  };

  const warnings = diag?.warnings ?? [];
  const fw = diag?.firewall ?? null;
  const showFix = !!fw?.suggestedCommand && (fw.state === "no_rule" || fw.state === "unknown");
  const rec = diag?.recommended ?? null;
  const showSwitch = !!diag && !!rec && !!diag.interface && rec.ip !== diag.interface.ip
    && (warnings.includes("better_interface") || warnings.includes("address_lost"));
  const alwaysTips = ["firewall", "sameWifi", "guest", "vpn"];

  const local = diag?.local ?? null;
  const ext = diag?.external ?? null;
  const pr = diag?.pairing ?? null;

  return (
    <Card>
      <SectionTitle
        aside={
          <Button size="sm" icon={<Stethoscope size={14} />} busy={busy} onClick={runDiag}>
            {busy ? t("diag.running") : t("diag.run")}
          </Button>
        }
      >
        {t("diag.title")}
      </SectionTitle>
      <p className="text-xs text-muted mb-3">{t("diag.intro")}</p>

      {warnings.length > 0 && (
        <ul className="space-y-2 mb-3">
          {warnings.map((w) => (
            <li key={w} className="flex items-start gap-2 text-sm">
              <AlertTriangle size={15} className="mt-0.5 shrink-0 text-warn" />
              <span>{t(`trouble.warn.${w}`, { defaultValue: w })}</span>
            </li>
          ))}
        </ul>
      )}

      {diag && (
        <div className="rounded-xl border border-line bg-surface-2/40 px-4">
          <Row label={t("diag.server")} mark={diag.running ? "ok" : "bad"}>
            {diag.running ? t("diag.serverUp") : t("diag.serverDown")}
          </Row>
          <Row label={t("diag.listening")} mark={diag.bind ? "ok" : "unknown"}>
            <span className="num ltr">{diag.bind ?? "—"}</span>
          </Row>
          <Row label={t("diag.network")}>
            {diag.interface ? (
              <span><span dir="auto">{diag.interface.name}</span> · <span className="num ltr">{diag.interface.ip}</span> · {t(`iface.${diag.interface.kind}`)}</span>
            ) : "—"}
          </Row>
          {rec && diag.interface && rec.ip !== diag.interface.ip && (
            <Row label={t("diag.recommendedAdapter")}>
              <span><span dir="auto">{rec.name}</span> · <span className="num ltr">{rec.ip}</span></span>
            </Row>
          )}
          <Row label={t("diag.localHeading")} mark={local ? (local.ok && local.pageOk ? "ok" : "bad") : "unknown"} note={t("diag.localNote")}>
            {local
              ? local.ok && local.pageOk
                ? t("diag.localOk", { ms: local.millis ?? 0 })
                : t("diag.localBad", { error: local.error ?? `HTTP ${local.status ?? "?"}` })
              : "—"}
          </Row>
          <Row label={t("diag.externalHeading")} mark={ext?.reached ? "ok" : "warn"} note={t("diag.externalNote")}>
            {ext?.reached
              ? <>
                  {t("diag.externalYes", { ip: ext.lastIp ?? "?", n: ext.requests })}
                  {ext.lastAt ? <span className="text-faint"> · {f.time(ext.lastAt)}</span> : null}
                </>
              : t("diag.externalNo")}
          </Row>
          <Row label={t("diag.profile")} mark={!diag.windows.checked ? "unknown" : diag.windows.category === "Private" || diag.windows.category === "DomainAuthenticated" ? "ok" : "warn"}>
            {diag.windows.checked
              ? <span>{diag.windows.category ?? "?"}{diag.windows.alias ? <span className="text-faint"> · <span dir="auto">{diag.windows.alias}</span></span> : null}</span>
              : <span className="text-faint">{t("diag.profileUnknown")}</span>}
          </Row>
          <Row label={t("diag.firewall")} mark={fw ? fwMark[fw.state] : "unknown"}>
            {fw
              ? <span className="inline-flex flex-wrap items-center gap-2"><Badge tone={fwTone[fw.state]}>{t(`diag.fw.${fw.state}`)}</Badge></span>
              : <span className="text-faint">{t("diag.profileUnknown")}</span>}
          </Row>
          <Row label={t("diag.pairing")}>
            {pr?.used ? t("diag.pairingUsed") : pr?.active ? t("diag.pairingActive") : t("diag.pairingNone")}
            {pr && (pr.pendingRequests > 0 || pr.sessions > 0) && (
              <span className="text-faint"> · {t("diag.pairingMore", { pending: pr.pendingRequests, sessions: pr.sessions })}</span>
            )}
          </Row>
        </div>
      )}
      {diag && !diag.deep && <p className="text-xs text-faint mt-2">{t("diag.notRun")}</p>}
      {diag && <p className="text-xs text-faint mt-1">{t("diag.checkedAt", { time: f.time(diag.checkedAt) })}</p>}

      {showSwitch && rec && (
        <div className="mt-4 rounded-xl border border-accent/25 bg-accent-soft p-4">
          <div className="text-sm font-medium">{t("diag.switchTitle")}</div>
          <p className="text-xs text-muted mt-1">{t("diag.switchDesc", { name: rec.name, ip: rec.ip })}</p>
          <Button className="mt-3" size="sm" icon={<RefreshCw size={14} />} busy={switching} onClick={() => switchTo(rec.ip)}>
            {t("diag.switch")}
          </Button>
        </div>
      )}

      {network.length > 1 && (
        <label className="flex flex-col gap-1.5 mt-4">
          <span className="text-xs font-medium text-muted">{t("connect.interface")}</span>
          <IfaceSelect network={network} value={preferredIp} onChange={(ip) => { void switchTo(ip); }} />
        </label>
      )}

      {showFix && fw?.suggestedCommand && (
        <div className="mt-4 rounded-xl border border-warn/25 bg-warn/10 p-4">
          <div className="flex items-center gap-2 text-sm font-medium"><ShieldAlert size={15} className="text-warn" />{t("diag.fixTitle")}</div>
          <p className="text-xs text-muted mt-1">{t("diag.fixDesc")}</p>
          <pre className="mt-3 text-xs ltr num selectable whitespace-pre-wrap break-all rounded-lg border border-line bg-surface-2 p-3">{fw.suggestedCommand}</pre>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <Button size="sm" icon={copied ? <Check size={14} /> : <Copy size={14} />} onClick={copyCmd}>
              {copied ? t("diag.fixCopied") : t("diag.fixCopy")}
            </Button>
            <span className="text-xs text-faint flex-1 min-w-48">{t("diag.fixAlt")}</span>
          </div>
        </div>
      )}

      <details className="group mt-4">
        <summary className="cursor-pointer text-sm text-accent select-none">{t("trouble.more")}</summary>
        <ul className="mt-3 space-y-2 text-sm text-muted list-disc ps-5 marker:text-faint">
          {alwaysTips.map((k) => <li key={k}>{t(`trouble.tip.${k}`)}</li>)}
        </ul>
      </details>
    </Card>
  );
}
