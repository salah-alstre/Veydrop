import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownToLine, ArrowUpFromLine, ChevronRight, Settings, Unplug } from "lucide-react";
import type { ShareInfo } from "../lib/types";
import { Button, IconButton } from "../ui/Button";
import { Wordmark } from "../ui/Logo";
import { Badge, Screen } from "../ui/Primitives";
import { useFmt } from "../ui/hooks";

function Tile({ icon, title, desc, extra, onClick }: { icon: ReactNode; title: string; desc: string; extra?: ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="vd-card w-full p-5 flex items-center gap-4 text-start transition-[transform,border-color] duration-200 active:scale-[0.99] active:border-line-strong"
    >
      <span className="size-12 shrink-0 rounded-[14px] bg-accent-soft border border-accent/25 text-accent grid place-items-center">{icon}</span>
      <span className="flex-1 min-w-0">
        <span className="block text-lg font-semibold leading-snug">{title}</span>
        <span className="block text-sm text-muted mt-0.5 leading-snug">{desc}</span>
        {extra && <span className="block mt-2">{extra}</span>}
      </span>
      <ChevronRight size={20} className="shrink-0 text-faint flip-rtl" aria-hidden />
    </button>
  );
}

export function Home({
  pcName,
  share,
  onSend,
  onReceive,
  onSettings,
  onDisconnect,
}: {
  pcName: string;
  share: ShareInfo | null;
  onSend: () => void;
  onReceive: () => void;
  onSettings: () => void;
  onDisconnect: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const f = useFmt();
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const count = share?.active ? share.items.filter((i) => i.ready && !i.missing).length : 0;

  return (
    <Screen
      header={
        <div className="flex items-center justify-between min-h-14">
          <Wordmark size={22} />
          <IconButton label={t("settings.title")} icon={<Settings size={20} />} onClick={onSettings} />
        </div>
      }
    >
      <div className="max-w-md mx-auto space-y-5">
        <div className="vd-card px-4 py-3 flex items-center gap-3">
          <span className="size-2.5 rounded-full bg-success shrink-0 vd-dot" aria-hidden />
          <div className="min-w-0 flex-1">
            <div className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint">{t("link.online")}</div>
            <div className="font-medium truncate">{t("home.connectedTo", { pc: pcName })}</div>
          </div>
        </div>

        <div className="space-y-3">
          <Tile icon={<ArrowUpFromLine size={24} aria-hidden />} title={t("home.send.title")} desc={t("home.send.desc")} onClick={onSend} />
          <Tile
            icon={<ArrowDownToLine size={24} aria-hidden />}
            title={t("home.receive.title")}
            desc={t("home.receive.desc")}
            extra={count > 0 ? <Badge tone="accent">{t("home.sharedCount", { n: f.num(count) })}</Badge> : undefined}
            onClick={onReceive}
          />
        </div>

        <div className="pt-2">
          {asking ? (
            <div className="vd-card p-4 space-y-3" role="alertdialog" aria-label={t("home.disconnect")}>
              <p className="text-[15px]">{t("home.disconnectAsk", { pc: pcName })}</p>
              <div className="flex gap-2">
                <Button className="flex-1" onClick={() => setAsking(false)} disabled={busy}>
                  {t("common.cancel")}
                </Button>
                <Button
                  className="flex-1"
                  variant="danger"
                  busy={busy}
                  onClick={async () => {
                    setBusy(true);
                    try {
                      await onDisconnect();
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  {t("home.disconnectConfirm")}
                </Button>
              </div>
            </div>
          ) : (
            <Button variant="ghost" className="w-full" icon={<Unplug size={18} aria-hidden />} onClick={() => setAsking(true)}>
              {t("home.disconnect")}
            </Button>
          )}
        </div>
      </div>
    </Screen>
  );
}
