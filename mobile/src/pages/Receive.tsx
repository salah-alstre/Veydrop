import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Archive, Download, File as FileIcon, FolderDown, Info, TriangleAlert } from "lucide-react";
import type { ShareInfo, ShareItemInfo } from "../lib/types";
import { downloadUrl, zipUrl } from "../lib/api";
import { Button, IconButton } from "../ui/Button";
import { Badge, Screen } from "../ui/Primitives";
import { useFmt } from "../ui/hooks";

function platformNote(): "noteIos" | "noteAndroid" | "noteGeneric" {
  const ua = navigator.userAgent;
  const iPadOS = navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1;
  if (/iPhone|iPad|iPod/i.test(ua) || iPadOS) return "noteIos";
  if (/Android/i.test(ua)) return "noteAndroid";
  return "noteGeneric";
}

const linkBtn =
  "inline-flex items-center justify-center gap-2 rounded-[12px] font-medium select-none min-h-11 px-4 text-[15px] " +
  "transition-[transform,background-color] duration-200 active:scale-[0.98] ";

export function Receive({ share, onBack }: { share: ShareInfo | null; onBack: () => void }) {
  const { t } = useTranslation();
  const f = useFmt();
  const [picked, setPicked] = useState<Set<string>>(new Set());

  const active = !!share?.active;
  // The PC reports "no share" as null once it stops sharing; remember that something was shared so
  // the phone can say it stopped instead of showing the first-visit empty state.
  const [wasActive, setWasActive] = useState(active);
  if (active && !wasActive) setWasActive(true);
  const items = useMemo(() => (active && share ? share.items : []), [active, share]);
  const ready = useMemo(() => items.filter((i) => i.ready && !i.missing), [items]);
  // drop selections that are no longer available after the PC changed the share
  const valid = useMemo(() => new Set(ready.map((i) => i.id)), [ready]);
  const selectedIds = [...picked].filter((id) => valid.has(id));
  const selectedSize = ready.filter((i) => picked.has(i.id)).reduce((s, i) => s + i.size, 0);

  const toggle = (id: string) =>
    setPicked((prev) => {
      const n = new Set(prev);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const header = (
    <div className="flex items-center gap-1 min-h-14">
      <IconButton label={t("common.back")} icon={<ArrowLeft size={22} className="flip-rtl" />} onClick={onBack} />
      <h1 className="text-lg font-semibold">{t("receive.title")}</h1>
    </div>
  );

  let footer = null;
  if (ready.length > 0) {
    footer = (
      <div className="max-w-md mx-auto space-y-2">
        <div className="flex justify-between text-sm text-muted">
          <span>{selectedIds.length ? t("receive.selected", { n: f.num(selectedIds.length) }) : t("receive.files", { n: f.num(ready.length) })}</span>
          <span className="num">{t("receive.total", { size: f.bytes(selectedIds.length ? selectedSize : ready.reduce((s, i) => s + i.size, 0)) })}</span>
        </div>
        {ready.length > 1 || selectedIds.length > 1 ? (
          <a
            className={`${linkBtn} w-full min-h-[52px] bg-accent text-accent-ink`}
            href={zipUrl(selectedIds.length > 1 ? { ids: selectedIds } : {})}
            download
          >
            <Archive size={18} aria-hidden />
            {selectedIds.length > 1 ? t("receive.zipSelected") : t("receive.zipAll")}
          </a>
        ) : null}
      </div>
    );
  }

  return (
    <Screen header={header} footer={footer}>
      <div className="max-w-md mx-auto space-y-4">
        {!active && (
          <div className="flex flex-col items-center text-center gap-4 py-12">
            <div className="size-16 rounded-2xl bg-surface-2 border border-line-strong grid place-items-center text-muted">
              <FolderDown size={30} aria-hidden />
            </div>
            <div className="space-y-1.5 max-w-xs" role="status">
              <h2 className="text-xl font-semibold">{wasActive || share !== null ? t("receive.sharingStopped") : t("receive.empty.title")}</h2>
              <p className="text-muted leading-relaxed">{t("receive.empty.body")}</p>
            </div>
          </div>
        )}

        {active && (
          <>
            <p className="flex gap-2.5 text-sm text-muted leading-relaxed">
              <Info size={18} className="shrink-0 mt-0.5 text-faint" aria-hidden />
              <span>
                <span className="font-medium text-fg">{t("receive.noteTitle")}. </span>
                {t(`receive.${platformNote()}`)}
              </span>
            </p>

            {ready.length > 1 && (
              <div className="flex items-center justify-between">
                <h2 className="text-sm font-semibold text-muted">{t("receive.files", { n: f.num(ready.length) })}</h2>
                <div className="flex gap-1">
                  <Button variant="ghost" onClick={() => setPicked(new Set(ready.map((i) => i.id)))}>{t("receive.selectAll")}</Button>
                  <Button variant="ghost" onClick={() => setPicked(new Set())} disabled={picked.size === 0}>{t("receive.selectNone")}</Button>
                </div>
              </div>
            )}

            <ul className="space-y-2.5">
              {items.map((i) => (
                <Row key={i.id} item={i} checked={picked.has(i.id)} selectable={ready.length > 1} onToggle={() => toggle(i.id)} />
              ))}
            </ul>
          </>
        )}
      </div>
    </Screen>
  );
}

function Row({ item, checked, selectable, onToggle }: { item: ShareItemInfo; checked: boolean; selectable: boolean; onToggle: () => void }) {
  const { t } = useTranslation();
  const f = useFmt();
  const usable = item.ready && !item.missing;
  const sub = item.relPath && item.relPath !== item.name ? item.relPath : null;

  return (
    <li className="vd-card p-3.5 flex items-center gap-3">
      {selectable && usable && (
        <input
          type="checkbox"
          checked={checked}
          onChange={onToggle}
          aria-label={item.name}
          className="size-5 shrink-0 accent-[var(--vd-accent)]"
        />
      )}
      <span className="shrink-0 text-faint"><FileIcon size={22} aria-hidden /></span>
      <div className="min-w-0 flex-1">
        <div className="font-medium truncate ltr text-start" title={item.name}>{item.name}</div>
        {sub && <div className="text-xs text-faint truncate ltr text-start">{sub}</div>}
        <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-muted">
          <span className="num">{f.bytes(item.size)}</span>
          {item.missing && (
            <Badge tone="danger"><TriangleAlert size={12} aria-hidden />{t("receive.missing")}</Badge>
          )}
          {!item.missing && !item.ready && <Badge tone="warn">{t("receive.preparing")}</Badge>}
        </div>
      </div>
      {usable && (
        <a
          className={`${linkBtn} shrink-0 !px-0 w-11 bg-surface-3 border border-line-strong text-fg`}
          href={downloadUrl(item.id)}
          download={item.name}
          aria-label={`${t("receive.download")}: ${item.name}`}
          title={t("receive.download")}
        >
          <Download size={20} aria-hidden />
        </a>
      )}
    </li>
  );
}
