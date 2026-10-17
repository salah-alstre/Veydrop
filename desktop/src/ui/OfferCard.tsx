import { useState } from "react";
import { Check, FolderOpen, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { toast, useApp } from "../lib/store";
import { useErrorText, useFmt } from "../lib/hooks";
import type { OfferInfo } from "../lib/types";
import { Button } from "./Button";

const MAX_LISTED = 6;

/** An incoming-file request. Nothing is written until the user presses Accept. */
export function OfferCard({ offer, compact }: { offer: OfferInfo; compact?: boolean }) {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const { snap } = useApp();
  const [dest, setDest] = useState<string | null>(null);
  const [busy, setBusy] = useState<"accept" | "reject" | null>(null);
  const target = dest ?? snap?.settings.downloadDir ?? "";

  const decide = async (accept: boolean) => {
    setBusy(accept ? "accept" : "reject");
    try {
      await ipc.decideOffer(offer.id, accept, accept ? dest : null);
    } catch (e) {
      toast("error", errText(e));
      setBusy(null);
    }
  };

  const choose = async () => {
    try {
      const p = await ipc.pickFolder();
      if (p) setDest(p);
    } catch (e) { toast("error", errText(e)); }
  };

  const shown = offer.files.slice(0, MAX_LISTED);
  const more = offer.files.length - shown.length;

  return (
    <div className="flex flex-col gap-4">
      <div>
        <div className="text-sm text-muted">{t("offer.from")}</div>
        <div className="text-base font-semibold" dir="auto">{offer.deviceName}</div>
        <div className="text-sm text-muted mt-0.5">
          {t("offer.summary", { n: f.num(offer.files.length), size: f.bytes(offer.totalSize) })}
        </div>
      </div>

      <ul className={`rounded-xl border border-line bg-surface-2/60 divide-y divide-line ${compact ? "max-h-40" : "max-h-64"} overflow-y-auto`}>
        {shown.map((x) => (
          <li key={x.clientId} className="flex items-center justify-between gap-3 px-3 py-2 text-sm">
            <span className="truncate selectable" dir="auto">{x.relPath || x.name}</span>
            <span className="num text-xs text-muted shrink-0">{f.bytes(x.size)}</span>
          </li>
        ))}
        {more > 0 && <li className="px-3 py-2 text-xs text-muted">{t("offer.more", { n: f.num(more) })}</li>}
      </ul>

      <div>
        <div className="text-xs font-medium text-muted mb-1.5">{t("offer.saveTo")}</div>
        <div className="flex items-center gap-2">
          <div className="flex-1 min-w-0 h-9 px-3 rounded-[10px] bg-surface-2 border border-line flex items-center text-sm">
            <span className="truncate ltr num selectable">{target}</span>
          </div>
          <Button icon={<FolderOpen size={15} />} onClick={choose}>{t("offer.change")}</Button>
        </div>
      </div>

      <div className="flex items-center justify-end gap-2">
        <Button variant="danger" icon={<X size={16} />} busy={busy === "reject"} disabled={busy === "accept"} onClick={() => decide(false)}>
          {t("offer.reject")}
        </Button>
        <Button variant="primary" icon={<Check size={16} />} busy={busy === "accept"} disabled={busy === "reject"} onClick={() => decide(true)}>
          {t("offer.accept")}
        </Button>
      </div>
    </div>
  );
}
