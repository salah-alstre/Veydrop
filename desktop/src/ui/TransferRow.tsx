import { ArrowDownToLine, ArrowUpFromLine, FolderOpen, Pause, Play, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc } from "../lib/ipc";
import { useErrorText, useFmt } from "../lib/hooks";
import { toast } from "../lib/store";
import type { TransferInfo, TransferState } from "../lib/types";
import { IconButton } from "./Button";
import { Badge, ProgressBar, type Tone } from "./Primitives";

export const stateTone: Record<TransferState, Tone> = {
  pending: "neutral",
  queued: "neutral",
  active: "accent",
  paused: "warn",
  interrupted: "warn",
  finalizing: "accent",
  completed: "success",
  failed: "danger",
  cancelled: "neutral",
  rejected: "neutral",
};

export const isLive = (s: TransferState) => !["completed", "failed", "cancelled", "rejected"].includes(s);

/** One live transfer: real progress, speed and ETA straight from the engine. */
export function TransferRow({ tr }: { tr: TransferInfo }) {
  const { t } = useTranslation();
  const f = useFmt();
  const errText = useErrorText();
  const incoming = tr.direction === "incoming";
  const live = isLive(tr.state);
  const frac = tr.size > 0 ? tr.transferred / tr.size : tr.state === "completed" ? 1 : 0;
  const run = (p: Promise<unknown>) => p.catch((e) => toast("error", errText(e)));

  return (
    <div className="flex items-center gap-4 py-3 border-b border-line last:border-b-0">
      <div className="size-9 shrink-0 rounded-xl bg-surface-2 border border-line grid place-items-center text-accent">
        {incoming ? <ArrowDownToLine size={16} /> : <ArrowUpFromLine size={16} />}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium selectable" dir="auto">{tr.relPath || tr.name}</span>
          <Badge tone={stateTone[tr.state]}>{t(`state.${tr.state}`)}</Badge>
        </div>
        <div className="mt-2">
          <ProgressBar
            value={frac}
            tone={tr.state === "completed" ? "success" : tr.state === "failed" ? "danger" : tr.state === "paused" || tr.state === "interrupted" ? "warn" : "accent"}
            indeterminate={tr.state === "finalizing"}
            label={tr.name}
          />
        </div>
        <div className="mt-1.5 flex flex-wrap gap-x-3 text-xs text-muted">
          <span className="num">{f.bytes(tr.transferred)} / {f.bytes(tr.size)}</span>
          {tr.state === "active" && tr.speedBps > 0 && <span className="num">{f.speed(tr.speedBps)}</span>}
          {tr.state === "active" && tr.etaSecs != null && <span className="num">{t("transfer.eta", { time: f.clock(tr.etaSecs) })}</span>}
          <span dir="auto">{tr.deviceName}</span>
          {tr.error && <span className="text-danger">{errText({ code: tr.error })}</span>}
        </div>
      </div>
      <div className="flex items-center gap-0.5 shrink-0">
        {tr.state === "active" && (
          <IconButton label={t("transfer.pause")} onClick={() => run(ipc.pauseTransfer(tr.id))}><Pause size={16} /></IconButton>
        )}
        {(tr.state === "paused" || tr.state === "interrupted") && (
          <IconButton label={t("transfer.resume")} onClick={() => run(ipc.resumeTransfer(tr.id))}><Play size={16} /></IconButton>
        )}
        {live && (
          <IconButton label={t("transfer.cancel")} onClick={() => run(ipc.cancelTransfer(tr.id))}><X size={16} /></IconButton>
        )}
        {tr.state === "completed" && tr.savedPath && incoming && (
          <IconButton label={t("transfer.reveal")} onClick={() => run(ipc.revealFile(tr.savedPath!))}><FolderOpen size={16} /></IconButton>
        )}
      </div>
    </div>
  );
}
