import { useState } from "react";
import { File as FileIcon, FilePlus2, FolderPlus, Image as ImageIcon, Film, Music, Archive, Share2, StopCircle, Trash2, UploadCloud, X, AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { toast, useApp } from "../lib/store";
import { useErrorText, useFmt } from "../lib/hooks";
import { Button, IconButton } from "../ui/Button";
import { Badge, Card, EmptyState, PageHeader, SectionTitle } from "../ui/Primitives";
import { TransferRow } from "../ui/TransferRow";
import type { PageProps } from "./types";

const ext = (n: string) => n.slice(n.lastIndexOf(".") + 1).toLowerCase();
function kindIcon(name: string) {
  const e = ext(name);
  if (["jpg", "jpeg", "png", "gif", "webp", "heic", "avif", "bmp", "svg"].includes(e)) return <ImageIcon size={16} />;
  if (["mp4", "mov", "mkv", "avi", "webm", "m4v"].includes(e)) return <Film size={16} />;
  if (["mp3", "wav", "flac", "m4a", "aac", "ogg"].includes(e)) return <Music size={16} />;
  if (["zip", "rar", "7z", "tar", "gz"].includes(e)) return <Archive size={16} />;
  return <FileIcon size={16} />;
}

export function Send({ go }: PageProps) {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const { snap } = useApp();
  const [busy, setBusy] = useState<string | null>(null);
  if (!snap) return null;
  const { share, server, sessions, transfers } = snap;
  const items = share?.items ?? [];
  const active = !!share?.active;
  const outgoing = transfers.filter((x) => x.direction === "outgoing");
  const online = sessions.some((s) => s.online);

  const guard = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    try { await fn(); } catch (e) { toast("error", errText(e)); } finally { setBusy(null); }
  };

  const report = (r: { skipped: number }) => {
    if (r.skipped > 0) toast("info", t("send.skipped", { n: f.num(r.skipped) }));
  };

  const addFiles = () => guard("files", async () => {
    const paths = await ipc.pickFiles();
    if (paths.length) report(await ipc.addSharePaths(paths));
  });
  const addFolder = () => guard("folder", async () => {
    const p = await ipc.pickFolder();
    if (p) report(await ipc.addSharePaths([p]));
  });

  const toggle = () => guard("toggle", async () => {
    if (!active && !server.running) await ipc.startServer();
    await ipc.setShareActive(!active);
  });

  return (
    <div>
      <PageHeader
        title={t("send.title")}
        subtitle={t("send.subtitle")}
        actions={
          <>
            <Button icon={<FilePlus2 size={16} />} busy={busy === "files"} onClick={addFiles}>{t("send.addFiles")}</Button>
            <Button icon={<FolderPlus size={16} />} busy={busy === "folder"} onClick={addFolder}>{t("send.addFolder")}</Button>
          </>
        }
      />

      <Card pad={false}>
        {items.length === 0 ? (
          <EmptyState
            icon={<UploadCloud size={22} />}
            title={t("send.emptyTitle")}
            desc={t("send.emptyDesc")}
          />
        ) : (
          <>
            <div className="flex flex-wrap items-center justify-between gap-3 px-5 pt-5 pb-2">
              <div className="flex items-center gap-2 text-sm">
                <span className="font-medium">{t("send.items", { n: f.num(items.length) })}</span>
                <span className="text-muted num">{f.bytes(share?.totalSize ?? 0)}</span>
                {share && !share.indexed && <Badge tone="warn" dot>{t("send.indexing")}</Badge>}
              </div>
              <Button size="sm" variant="ghost" icon={<Trash2 size={14} />} disabled={active} onClick={() => guard("clear", ipc.clearShare)}>
                {t("send.clear")}
              </Button>
            </div>
            <ul className="px-5 pb-3 max-h-[46vh] overflow-y-auto">
              {items.map((it) => (
                <li key={it.id} className="flex items-center gap-3 py-2.5 border-b border-line last:border-b-0">
                  <div className="size-9 shrink-0 rounded-xl bg-surface-2 border border-line grid place-items-center text-muted">{kindIcon(it.name)}</div>
                  <div className="min-w-0 flex-1">
                    <div className="text-sm truncate selectable" dir="auto">{it.relPath || it.name}</div>
                    <div className="text-xs text-muted flex gap-3">
                      <span className="num">{f.bytes(it.size)}</span>
                      {it.missing ? (
                        <span className="text-danger flex items-center gap-1"><AlertTriangle size={12} />{t("send.missing")}</span>
                      ) : !it.ready ? (
                        <span>{t("send.preparing")}</span>
                      ) : null}
                    </div>
                  </div>
                  <IconButton label={t("send.remove")} disabled={active} onClick={() => guard("rm", () => ipc.removeShareItem(it.id))}>
                    <X size={16} />
                  </IconButton>
                </li>
              ))}
            </ul>
          </>
        )}

        {items.length > 0 && (
          <div className="border-t border-line px-5 py-4 flex flex-wrap items-center justify-between gap-3">
            <div className="text-sm text-muted max-w-xl">
              {active
                ? online ? t("send.liveHint") : t("send.liveNoPhone")
                : t("send.idleHint")}
              {active && !online && (
                <button type="button" className="ms-2 text-accent hover:underline" onClick={() => go("connect")}>{t("send.openConnect")}</button>
              )}
            </div>
            <Button variant={active ? "danger" : "primary"} size="lg" icon={active ? <StopCircle size={18} /> : <Share2 size={18} />} busy={busy === "toggle"} onClick={toggle}>
              {active ? t("send.stop") : t("send.start")}
            </Button>
          </div>
        )}
      </Card>

      {outgoing.length > 0 && (
        <div className="mt-6">
          <SectionTitle>{t("send.progress")}</SectionTitle>
          <Card pad={false}>
            <div className="px-5">{outgoing.slice(0, 30).map((x) => <TransferRow key={x.id} tr={x} />)}</div>
          </Card>
        </div>
      )}

      <p className="mt-4 text-xs text-faint">{t("send.dropHint")}</p>
    </div>
  );
}
