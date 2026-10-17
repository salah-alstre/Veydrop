import { ArrowDownToLine, ArrowUpFromLine, Send, Smartphone, Wifi } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useApp } from "../lib/store";
import { useFmt } from "../lib/hooks";
import { Button } from "../ui/Button";
import { LinkLoader } from "../ui/Loader";
import { Badge, Card, EmptyState, PageHeader, SectionTitle } from "../ui/Primitives";
import { TransferRow, isLive } from "../ui/TransferRow";
import type { PageProps } from "./types";

export function Dashboard({ go }: PageProps) {
  const { t } = useTranslation();
  const f = useFmt();
  const { snap } = useApp();
  if (!snap) return null;
  const { server, sessions, stats, network, settings, transfers } = snap;
  const online = sessions.filter((s) => s.online);
  const iface = network.find((n) => n.ip === server.ip) ?? network.find((n) => n.recommended) ?? network[0];
  const live = transfers.filter((x) => isLive(x.state));
  const recent = transfers.filter((x) => !isLive(x.state)).slice(0, 4);

  const linkState = !server.running ? "failed" : online.length > 0 ? "connected" : "waiting";
  const status = !server.running
    ? { tone: "neutral" as const, text: t("dashboard.status.off") }
    : server.addressLost
      ? { tone: "warn" as const, text: t("dashboard.status.addressLost") }
      : online.length > 0
        ? { tone: "success" as const, text: t("dashboard.status.connected", { n: f.num(online.length) }) }
        : { tone: "accent" as const, text: t("dashboard.status.waiting") };

  return (
    <div>
      <PageHeader
        title={t("dashboard.title")}
        subtitle={t("dashboard.subtitle")}
        actions={
          <>
            <Button icon={<Send size={16} />} onClick={() => go("send")}>{t("dashboard.quickSend")}</Button>
            <Button variant="primary" icon={<Smartphone size={16} />} onClick={() => go("connect")}>
              {server.running ? t("dashboard.showQr") : t("dashboard.connect")}
            </Button>
          </>
        }
      />

      <Card className="vd-grid-bg overflow-hidden" pad={false}>
        <div className="p-6 flex flex-wrap items-center justify-between gap-6">
          <div className="min-w-0">
            <div className="flex items-center gap-2 mb-2">
              <Badge tone={status.tone} dot>{status.text}</Badge>
            </div>
            <div className="text-xl font-semibold tracking-tight truncate selectable" dir="auto">{settings.deviceName}</div>
            <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-faint">{t("dashboard.network")}</dt>
              <dd className="flex items-center gap-1.5">
                <Wifi size={14} className="text-muted" />
                <span dir="auto">{iface ? `${iface.name}` : t("dashboard.noNetwork")}</span>
              </dd>
              <dt className="text-faint">{t("dashboard.localIp")}</dt>
              <dd className="num selectable ltr">{server.running && server.ip ? `${server.ip}:${server.port}` : iface?.ip ?? "—"}</dd>
              <dt className="text-faint">{t("dashboard.phones")}</dt>
              <dd className="num">{f.num(online.length)}</dd>
            </dl>
          </div>
          <LinkLoader state={linkState} width={260} />
        </div>
      </Card>

      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3 mt-4">
        <Stat label={t("dashboard.stats.sent")} value={f.bytes(stats.sentBytes)} sub={t("dashboard.stats.files", { n: f.num(stats.sentFiles) })} icon={<ArrowUpFromLine size={16} />} />
        <Stat label={t("dashboard.stats.received")} value={f.bytes(stats.receivedBytes)} sub={t("dashboard.stats.files", { n: f.num(stats.receivedFiles) })} icon={<ArrowDownToLine size={16} />} />
        <Stat label={t("dashboard.stats.failed")} value={f.num(stats.failed)} sub={t("dashboard.stats.thisSession")} />
        <Stat label={t("dashboard.stats.active")} value={f.num(live.length)} sub={t("dashboard.stats.transfers")} />
      </div>

      <div className="mt-6">
        <SectionTitle>{t("dashboard.activity")}</SectionTitle>
        <Card pad={false}>
          {live.length === 0 && recent.length === 0 ? (
            <EmptyState
              icon={<ArrowDownToLine size={20} />}
              title={t("dashboard.empty.title")}
              desc={t("dashboard.empty.desc")}
            />
          ) : (
            <div className="px-5">
              {[...live, ...recent].map((x) => <TransferRow key={x.id} tr={x} />)}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}

function Stat({ label, value, sub, icon }: { label: string; value: string; sub: string; icon?: React.ReactNode }) {
  return (
    <Card>
      <div className="flex items-center justify-between text-faint text-xs">
        <span>{label}</span>
        {icon}
      </div>
      <div className="mt-2 text-xl font-semibold tracking-tight num">{value}</div>
      <div className="text-xs text-muted mt-0.5">{sub}</div>
    </Card>
  );
}
