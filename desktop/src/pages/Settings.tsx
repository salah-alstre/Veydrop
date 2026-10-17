import { useEffect, useState, type ReactNode } from "react";
import { FolderOpen, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { toast, useApp } from "../lib/store";
import { useErrorText, useFmt } from "../lib/hooks";
import type { AppInfo, Settings as S } from "../lib/types";
import { Button } from "../ui/Button";
import { Logo } from "../ui/Logo";
import { Card, PageHeader, Row, SectionTitle, Segmented, Switch, inputCls } from "../ui/Primitives";

function NumberField({ value, min, max, step = 1, unit, label, onCommit }: {
  value: number; min: number; max: number; step?: number; unit?: string; label: string; onCommit: (n: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const n = Math.round(Number(draft));
    if (!Number.isFinite(n)) { setDraft(String(value)); return; }
    const c = Math.min(max, Math.max(min, n));
    setDraft(String(c));
    if (c !== value) onCommit(c);
  };
  return (
    <div className="flex items-center gap-2">
      <input
        type="number" inputMode="numeric" min={min} max={max} step={step}
        aria-label={label}
        className={`${inputCls} w-28 num ltr text-end`}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }}
      />
      {unit && <span className="text-xs text-muted">{unit}</span>}
    </div>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mb-6">
      <SectionTitle>{title}</SectionTitle>
      <Card pad={false}><div className="px-5 divide-y divide-line">{children}</div></Card>
    </section>
  );
}

export function Settings() {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const { snap } = useApp();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [name, setName] = useState("");

  useEffect(() => { ipc.appInfo().then(setInfo).catch(() => {}); }, []);
  const deviceName = snap?.settings.deviceName;
  useEffect(() => { if (deviceName != null) setName(deviceName); }, [deviceName]);

  if (!snap) return null;
  const s = snap.settings;
  const set = (patch: Partial<S>) => ipc.updateSettings(patch).catch((e) => toast("error", errText(e)));

  const pickDir = async () => {
    try { const p = await ipc.pickFolder(); if (p) await set({ downloadDir: p }); }
    catch (e) { toast("error", errText(e)); }
  };
  const commitName = () => {
    const v = name.trim();
    if (!v) { setName(s.deviceName); return; }
    if (v !== s.deviceName) set({ deviceName: v });
  };

  return (
    <div>
      <PageHeader title={t("settings.title")} subtitle={t("settings.subtitle")} />

      <Group title={t("settings.general")}>
        <Row title={t("settings.language")} desc={t("settings.languageDesc")}>
          <Segmented label={t("settings.language")} value={s.language} onChange={(v) => set({ language: v })} options={[
            { value: "system", label: t("settings.system") },
            { value: "en", label: "English" },
            { value: "ar", label: "العربية" },
          ]} />
        </Row>
        <Row title={t("settings.theme")} desc={t("settings.themeDesc")}>
          <Segmented label={t("settings.theme")} value={s.theme} onChange={(v) => set({ theme: v })} options={[
            { value: "system", label: t("settings.system") },
            { value: "dark", label: t("settings.dark") },
            { value: "light", label: t("settings.light") },
          ]} />
        </Row>
        <Row title={t("settings.deviceName")} desc={t("settings.deviceNameDesc")}>
          <input className={`${inputCls} w-56`} dir="auto" maxLength={48} aria-label={t("settings.deviceName")}
            value={name} onChange={(e) => setName(e.target.value)} onBlur={commitName}
            onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }} />
        </Row>
        <Row title={t("settings.downloadDir")} desc={t("settings.downloadDirDesc")}>
          <div className="flex items-center gap-2 max-w-md">
            <span className="text-sm truncate ltr num selectable max-w-64" title={s.downloadDir}>{s.downloadDir}</span>
            <Button icon={<FolderOpen size={15} />} onClick={pickDir}>{t("settings.change")}</Button>
          </div>
        </Row>
      </Group>

      <Group title={t("settings.system")}>
        <Row title={t("settings.notifications")} desc={t("settings.notificationsDesc")}>
          <Switch checked={s.notifications} onChange={(v) => set({ notifications: v })} label={t("settings.notifications")} />
        </Row>
        <Row title={t("settings.startup")} desc={t("settings.startupDesc")}>
          <Switch checked={s.launchAtStartup} onChange={(v) => set({ launchAtStartup: v })} label={t("settings.startup")} />
        </Row>
        <Row title={t("settings.tray")} desc={t("settings.trayDesc")}>
          <Switch checked={s.minimizeToTray} onChange={(v) => set({ minimizeToTray: v })} label={t("settings.tray")} />
        </Row>
      </Group>

      <Group title={t("settings.pairing")}>
        <Row title={t("settings.askReceive")} desc={t("settings.askReceiveDesc")}>
          <Switch checked={s.askBeforeReceive} onChange={(v) => set({ askBeforeReceive: v })} label={t("settings.askReceive")} />
        </Row>
        <Row title={t("settings.trustKnown")} desc={t("settings.trustKnownDesc")}>
          <Switch checked={s.trustKnownDevices} onChange={(v) => set({ trustKnownDevices: v })} label={t("settings.trustKnown")} />
        </Row>
        <Row title={t("settings.pairingSeconds")} desc={t("settings.pairingSecondsDesc")}>
          <NumberField label={t("settings.pairingSeconds")} value={s.pairingSeconds} min={30} max={900} unit={t("settings.unitSeconds")} onCommit={(n) => set({ pairingSeconds: n })} />
        </Row>
        <Row title={t("settings.sessionMinutes")} desc={t("settings.sessionMinutesDesc")}>
          <NumberField label={t("settings.sessionMinutes")} value={s.sessionMinutes} min={5} max={1440} unit={t("settings.unitMinutes")} onCommit={(n) => set({ sessionMinutes: n })} />
        </Row>
        <div className="py-4">
          <div className="text-sm font-medium">{t("settings.trusted")}</div>
          <div className="text-xs text-muted mt-0.5">{t("settings.trustedDesc")}</div>
          {s.trustedDevices.length === 0 ? (
            <div className="text-sm text-faint mt-3">{t("settings.noTrusted")}</div>
          ) : (
            <ul className="mt-3 space-y-2">
              {s.trustedDevices.map((d) => (
                <li key={d.id} className="flex items-center justify-between gap-3 rounded-xl border border-line bg-surface-2/60 px-3 py-2">
                  <div className="min-w-0">
                    <div className="text-sm truncate" dir="auto">{d.name}</div>
                    <div className="text-xs text-muted num">{t("settings.trustedSince", { date: f.dateTime(d.addedAt) })}</div>
                  </div>
                  <Button size="sm" variant="danger" onClick={() => ipc.forgetTrusted(d.id).catch((e) => toast("error", errText(e)))}>{t("settings.forget")}</Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Group>

      <Group title={t("settings.transfers")}>
        <Row title={t("settings.chunkSize")} desc={t("settings.chunkSizeDesc")}>
          <NumberField label={t("settings.chunkSize")} value={s.chunkSizeMib} min={1} max={16} unit="MiB" onCommit={(n) => set({ chunkSizeMib: n })} />
        </Row>
        <Row title={t("settings.parallel")} desc={t("settings.parallelDesc")}>
          <NumberField label={t("settings.parallel")} value={s.maxParallelUploads} min={1} max={4} onCommit={(n) => set({ maxParallelUploads: n })} />
        </Row>
        <Row title={t("settings.maxFile")} desc={t("settings.maxFileDesc")}>
          <NumberField label={t("settings.maxFile")} value={s.maxFileSizeGib} min={0} max={1024} unit="GiB" onCommit={(n) => set({ maxFileSizeGib: n })} />
        </Row>
        <Row title={t("settings.partialRetention")} desc={t("settings.partialRetentionDesc")}>
          <NumberField label={t("settings.partialRetention")} value={s.partialRetentionHours} min={1} max={336} unit={t("settings.unitHours")} onCommit={(n) => set({ partialRetentionHours: n })} />
        </Row>
        <Row title={t("settings.port")} desc={t("settings.portDesc")}>
          <NumberField label={t("settings.port")} value={s.preferredPort} min={0} max={65535} onCommit={(n) => set({ preferredPort: n })} />
        </Row>
      </Group>

      <Group title={t("settings.history")}>
        <Row title={t("settings.retention")} desc={t("settings.retentionDesc")}>
          <NumberField label={t("settings.retention")} value={s.historyRetentionDays} min={0} max={3650} unit={t("settings.unitDays")} onCommit={(n) => set({ historyRetentionDays: n })} />
        </Row>
        <Row title={t("settings.clearHistory")} desc={t("settings.clearHistoryDesc")}>
          <Button variant="danger" onClick={() => ipc.clearHistory().then(() => toast("success", t("settings.historyCleared"))).catch((e) => toast("error", errText(e)))}>
            {t("activity.clear")}
          </Button>
        </Row>
      </Group>

      <section className="mb-6">
        <SectionTitle>{t("settings.about")}</SectionTitle>
        <Card>
          <div className="flex items-start gap-4">
            <Logo size={44} />
            <div className="min-w-0 flex-1">
              <div className="text-base font-semibold">Veydrop</div>
              <div className="text-sm text-muted">{t("splash.tagline")}</div>
              <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                <dt className="text-faint">{t("settings.version")}</dt>
                <dd className="num ltr selectable">{info?.version ?? "—"}</dd>
                <dt className="text-faint">{t("settings.dataDir")}</dt>
                <dd className="ltr num selectable truncate">{info?.dataDir ?? "—"}</dd>
              </dl>
              <div className="mt-4 flex items-start gap-2 text-xs text-muted border-t border-line pt-3">
                <ShieldCheck size={14} className="mt-0.5 shrink-0 text-faint" />
                <span>{t("settings.aboutNote")}</span>
              </div>
            </div>
          </div>
        </Card>
      </section>
    </div>
  );
}
