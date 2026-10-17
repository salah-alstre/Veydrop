import { FolderOpen, Inbox } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { toast, useApp } from "../lib/store";
import { useErrorText } from "../lib/hooks";
import { Button } from "../ui/Button";
import { OfferCard } from "../ui/OfferCard";
import { Badge, Card, EmptyState, PageHeader, SectionTitle } from "../ui/Primitives";
import { TransferRow } from "../ui/TransferRow";
import type { PageProps } from "./types";

export function Receive({ go }: PageProps) {
  const { t } = useTranslation();
  const errText = useErrorText();
  const { snap } = useApp();
  if (!snap) return null;
  const { offers, transfers, settings, server } = snap;
  const pending = offers.filter((o) => o.state === "pending");
  const incoming = transfers.filter((x) => x.direction === "incoming");

  const openDir = () => ipc.openDir(settings.downloadDir).catch((e) => toast("error", errText(e)));

  return (
    <div>
      <PageHeader
        title={t("receive.title")}
        subtitle={t("receive.subtitle")}
        actions={<Button icon={<FolderOpen size={16} />} onClick={openDir}>{t("receive.openFolder")}</Button>}
      />

      <Card className="mb-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="text-xs text-faint">{t("receive.destination")}</div>
            <div className="text-sm mt-0.5 truncate ltr num selectable">{settings.downloadDir}</div>
          </div>
          <Button size="sm" variant="ghost" onClick={() => go("settings")}>{t("receive.changeInSettings")}</Button>
        </div>
        {!settings.askBeforeReceive && (
          <div className="mt-3 text-xs text-muted">{t("receive.trustNote")}</div>
        )}
      </Card>

      {pending.length > 0 && (
        <div className="mb-6">
          <SectionTitle aside={<Badge tone="warn" dot>{t("receive.waiting")}</Badge>}>{t("receive.requests")}</SectionTitle>
          <div className="flex flex-col gap-3">
            {pending.map((o) => <Card key={o.id}><OfferCard offer={o} /></Card>)}
          </div>
        </div>
      )}

      <SectionTitle>{t("receive.incoming")}</SectionTitle>
      <Card pad={false}>
        {incoming.length === 0 ? (
          <EmptyState
            icon={<Inbox size={22} />}
            title={server.running ? t("receive.emptyRunning") : t("receive.emptyOff")}
            desc={server.running ? t("receive.emptyRunningDesc") : t("receive.emptyOffDesc")}
            action={!server.running ? <Button variant="primary" onClick={() => go("connect")}>{t("dashboard.connect")}</Button> : undefined}
          />
        ) : (
          <div className="px-5">{incoming.slice(0, 40).map((x) => <TransferRow key={x.id} tr={x} />)}</div>
        )}
      </Card>
    </div>
  );
}
