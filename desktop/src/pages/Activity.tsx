import { useEffect, useState } from "react";
import { ArrowDownToLine, ArrowUpFromLine, FolderOpen, History, Search, Trash2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { toast, useApp } from "../lib/store";
import { useErrorText, useFmt } from "../lib/hooks";
import type { HistoryEntry, HistoryQuery } from "../lib/types";
import { Button, IconButton } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Badge, Card, EmptyState, PageHeader, Segmented, inputCls } from "../ui/Primitives";
import { stateTone } from "../ui/TransferRow";

type Dir = "all" | "incoming" | "outgoing";
type Status = "all" | "completed" | "failed" | "cancelled";

export function Activity() {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const { historyRev } = useApp();
  const [search, setSearch] = useState("");
  const [dir, setDir] = useState<Dir>("all");
  const [status, setStatus] = useState<Status>("all");
  const [rows, setRows] = useState<HistoryEntry[] | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    const q: HistoryQuery = { limit: 500 };
    if (search.trim()) q.search = search.trim();
    if (dir !== "all") q.direction = dir;
    if (status !== "all") q.status = status;
    let stale = false;
    const h = setTimeout(() => {
      ipc.history(q).then((r) => { if (!stale) setRows(r); }).catch((e) => { if (!stale) { setRows([]); toast("error", errText(e)); } });
    }, search ? 200 : 0);
    return () => { stale = true; clearTimeout(h); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search, dir, status, historyRev]);

  const clear = async () => {
    try { await ipc.clearHistory(); } catch (e) { toast("error", errText(e)); }
    setConfirmClear(false);
  };

  return (
    <div>
      <PageHeader
        title={t("activity.title")}
        subtitle={t("activity.subtitle")}
        actions={
          <Button variant="danger" icon={<Trash2 size={15} />} disabled={!rows?.length} onClick={() => setConfirmClear(true)}>
            {t("activity.clear")}
          </Button>
        }
      />

      <div className="flex flex-wrap items-center gap-3 mb-4">
        <div className="relative flex-1 min-w-52">
          <Search size={15} className="absolute start-3 top-1/2 -translate-y-1/2 text-faint pointer-events-none" />
          <input
            className={`${inputCls} ps-9`}
            placeholder={t("activity.search")}
            aria-label={t("activity.search")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <Segmented<Dir> label={t("activity.direction")} value={dir} onChange={setDir} options={[
          { value: "all", label: t("activity.all") },
          { value: "incoming", label: t("activity.received") },
          { value: "outgoing", label: t("activity.sent") },
        ]} />
        <Segmented<Status> label={t("activity.status")} value={status} onChange={setStatus} options={[
          { value: "all", label: t("activity.all") },
          { value: "completed", label: t("state.completed") },
          { value: "failed", label: t("state.failed") },
          { value: "cancelled", label: t("state.cancelled") },
        ]} />
      </div>

      <Card pad={false}>
        {rows === null ? (
          <div className="p-5 space-y-3">{[0, 1, 2, 3].map((i) => <div key={i} className="vd-skeleton h-10 rounded-lg" />)}</div>
        ) : rows.length === 0 ? (
          <EmptyState icon={<History size={22} />} title={t("activity.emptyTitle")} desc={search || dir !== "all" || status !== "all" ? t("activity.emptyFiltered") : t("activity.emptyDesc")} />
        ) : (
          <ul className="px-5">
            {rows.map((h) => (
              <li key={h.id} className="flex items-center gap-4 py-3 border-b border-line last:border-b-0">
                <div className="size-9 shrink-0 rounded-xl bg-surface-2 border border-line grid place-items-center text-accent">
                  {h.direction === "incoming" ? <ArrowDownToLine size={16} /> : <ArrowUpFromLine size={16} />}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium selectable" dir="auto">{h.relPath || h.name}</span>
                    <Badge tone={stateTone[h.state]}>{t(`state.${h.state}`)}</Badge>
                  </div>
                  <div className="mt-1 flex flex-wrap gap-x-3 text-xs text-muted">
                    <span className="num">{f.bytes(h.size)}</span>
                    <span>{h.direction === "incoming" ? t("activity.from", { name: h.deviceName }) : t("activity.to", { name: h.deviceName })}</span>
                    <span className="num">{f.dateTime(h.finishedAt)}</span>
                    {h.durationMs > 0 && <span className="num">{f.clock(Math.round(h.durationMs / 1000))}</span>}
                    {h.error && <span className="text-danger">{errText({ code: h.error })}</span>}
                  </div>
                  {h.sha256 && h.state === "completed" && (
                    <div className="mt-1 text-[11px] text-faint ltr num truncate selectable" title={h.sha256}>SHA-256 {h.sha256}</div>
                  )}
                </div>
                <div className="flex items-center shrink-0">
                  {h.savedPath && h.state === "completed" && h.direction === "incoming" && (
                    <IconButton label={t("transfer.reveal")} onClick={() => ipc.revealFile(h.savedPath!).catch((e) => toast("error", errText(e)))}>
                      <FolderOpen size={16} />
                    </IconButton>
                  )}
                  <IconButton label={t("activity.remove")} onClick={() => ipc.removeHistory(h.id).catch((e) => toast("error", errText(e)))}>
                    <X size={16} />
                  </IconButton>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Modal
        open={confirmClear}
        title={t("activity.clearTitle")}
        icon={<Trash2 size={18} />}
        onClose={() => setConfirmClear(false)}
        footer={
          <>
            <Button onClick={() => setConfirmClear(false)}>{t("common.cancel")}</Button>
            <Button variant="danger" onClick={clear}>{t("activity.clearConfirm")}</Button>
          </>
        }
      >
        <p className="text-sm text-muted">{t("activity.clearDesc")}</p>
      </Modal>
    </div>
  );
}
