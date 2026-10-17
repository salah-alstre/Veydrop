import { useRef, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft, CheckCircle2, File as FileIcon, FolderOpen, Pause, Play, Plus, RotateCw, TriangleAlert, X, XCircle,
} from "lucide-react";
import { MAX_OFFER_FILES, type SendItem, type SendQueue } from "../lib/sendQueue";
import { Button, IconButton } from "../ui/Button";
import { LinkLoader } from "../ui/Loader";
import { ProgressBar, Badge, Screen, type Tone } from "../ui/Primitives";
import { useFmt } from "../ui/hooks";

type Fmt = ReturnType<typeof useFmt>;

/** `webkitdirectory` is non-standard, so it cannot go through typed JSX props. */
const DIR_ATTRS = { webkitdirectory: "", directory: "" } as Record<string, string>;

function isTerminal(i: SendItem): boolean {
  const p = i.progress?.phase;
  return p === "done" || p === "failed" || p === "cancelled";
}

function ItemRow({ item, phase, queue }: { item: SendItem; phase: string; queue: SendQueue }) {
  const { t } = useTranslation();
  const f = useFmt();
  const p = item.progress;
  const selecting = phase === "select" || phase === "rejected" || phase === "failed";
  const pPhase = p?.phase;

  let label = t("send.state.queued");
  let tone: Tone = "neutral";
  if (p && item.started) {
    switch (p.phase) {
      case "uploading": label = t("send.state.uploading"); tone = "accent"; break;
      case "paused": label = p.pausedBy === "pc" ? t("send.state.pcPaused") : t("send.state.paused"); tone = "warn"; break;
      case "interrupted": label = t("send.state.interrupted"); tone = "warn"; break;
      case "finalizing": label = t("send.state.finalizing"); tone = "accent"; break;
      case "done": label = t("send.state.done"); tone = "success"; break;
      case "failed": label = t("send.state.failed"); tone = "danger"; break;
      case "cancelled": label = t("send.state.cancelled"); tone = "neutral"; break;
    }
  }

  const fraction = pPhase === "done" ? 1 : item.size > 0 ? (p?.sent ?? 0) / item.size : 0;
  const showBar = !selecting && item.started && pPhase !== "cancelled";
  const errCode = p?.error ? (p.error as string) : null;
  const sub = item.relPath && item.relPath !== item.name ? item.relPath : null;

  return (
    <li className="vd-card p-3.5">
      <div className="flex items-start gap-3">
        <span className="mt-0.5 shrink-0 text-faint">
          {pPhase === "done" ? <CheckCircle2 size={22} className="text-success" aria-hidden />
            : pPhase === "failed" ? <XCircle size={22} className="text-danger" aria-hidden />
            : <FileIcon size={22} aria-hidden />}
        </span>
        <div className="min-w-0 flex-1">
          <div className="font-medium truncate ltr text-start" title={item.name}>{item.name}</div>
          {sub && <div className="text-xs text-faint truncate ltr text-start">{sub}</div>}
          <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted">
            <span className="num">{f.bytes(item.size)}</span>
            {!selecting && <Badge tone={tone}>{label}</Badge>}
          </div>
        </div>
        <div className="flex shrink-0 items-center">
          {selecting && (
            <IconButton label={t("common.remove")} icon={<X size={20} />} onClick={() => queue.remove(item.key)} />
          )}
          {!selecting && item.started && pPhase === "uploading" && (
            <IconButton label={t("common.pause")} icon={<Pause size={19} />} onClick={() => queue.pause(item.key)} />
          )}
          {!selecting && pPhase === "paused" && p?.pausedBy === "user" && (
            <IconButton label={t("common.resume")} icon={<Play size={19} />} onClick={() => queue.resume(item.key)} />
          )}
          {!selecting && (pPhase === "interrupted" || pPhase === "failed") && (
            <IconButton label={t("common.retry")} icon={<RotateCw size={19} />} onClick={() => queue.retry(item.key)} />
          )}
          {!selecting && !isTerminal(item) && (
            <IconButton label={t("common.cancel")} icon={<X size={20} />} onClick={() => queue.cancel(item.key)} />
          )}
        </div>
      </div>

      {showBar && (
        <div className="mt-3 space-y-1.5">
          <ProgressBar
            value={fraction}
            tone={pPhase === "done" ? "success" : pPhase === "failed" ? "danger" : pPhase === "paused" || pPhase === "interrupted" ? "warn" : "accent"}
            label={item.name}
          />
          {pPhase === "uploading" && p && (
            <div className="flex justify-between text-xs text-muted">
              <span className="num">{f.percent(fraction)}</span>
              <span className="num">
                {f.speed(p.speedBps)}
                {p.etaSecs != null && ` · ${t("send.eta", { eta: f.clock(p.etaSecs) })}`}
              </span>
            </div>
          )}
        </div>
      )}

      {errCode && (pPhase === "failed" || pPhase === "interrupted") && (
        <p className="mt-2 flex gap-2 text-sm text-danger leading-snug" role="alert">
          <TriangleAlert size={16} className="mt-0.5 shrink-0" aria-hidden />
          <span>{t(`errors.${errCode}`, { defaultValue: t("errors.other") })}</span>
        </p>
      )}
    </li>
  );
}

function errorText(t: (k: string, o?: Record<string, unknown>) => string, f: Fmt, code: string): string {
  if (code === "too_many") return t("send.tooMany", { n: f.num(MAX_OFFER_FILES) });
  return t(`errors.${code}`, { defaultValue: t("errors.other") });
}

export function Send({ queue, onBack }: { queue: SendQueue; onBack: () => void }) {
  const { t } = useTranslation();
  const f = useFmt();
  const state = useSyncExternalStore(queue.subscribe, queue.getState);
  const fileInput = useRef<HTMLInputElement>(null);
  const dirInput = useRef<HTMLInputElement>(null);

  const { phase, items, error } = state;
  const selecting = phase === "select" || phase === "rejected" || phase === "failed";
  const sending = phase === "sending";
  const finished = sending && items.length > 0 && items.every(isTerminal);
  const total = items.reduce((s, i) => s + i.size, 0);
  const done = items.filter((i) => i.progress?.phase === "done").length;
  const failed = items.filter((i) => i.progress?.phase === "failed").length;
  const sentBytes = items.reduce((s, i) => s + (i.progress?.phase === "done" ? i.size : Math.min(i.size, i.progress?.sent ?? 0)), 0);

  const onPick = (e: React.ChangeEvent<HTMLInputElement>) => {
    const list = e.target.files ? Array.from(e.target.files) : [];
    // iOS/Safari keeps the previous selection otherwise and would not fire change for the same files
    e.target.value = "";
    if (list.length) queue.add(list);
  };

  const header = (
    <div className="flex items-center gap-1 min-h-14">
      <IconButton label={t("common.back")} icon={<ArrowLeft size={22} className="flip-rtl" />} onClick={onBack} />
      <h1 className="text-lg font-semibold">{t("send.title")}</h1>
    </div>
  );

  let footer = null;
  if (selecting) {
    footer = (
      <div className="max-w-md mx-auto space-y-2">
        {items.length > 0 && (
          <div className="flex justify-between text-sm text-muted">
            <span>{t("send.selected", { n: f.num(items.length) })}</span>
            <span>{t("send.total", { size: f.bytes(total) })}</span>
          </div>
        )}
        <Button variant="primary" size="lg" disabled={items.length === 0} onClick={() => void queue.start()}>
          {t("send.start")}
        </Button>
      </div>
    );
  } else if (phase === "offering" || phase === "waiting") {
    footer = (
      <div className="max-w-md mx-auto">
        <Button size="lg" onClick={() => queue.cancelAll()} disabled={phase === "offering"}>
          {t("common.cancel")}
        </Button>
      </div>
    );
  } else if (sending) {
    footer = (
      <div className="max-w-md mx-auto space-y-2">
        <div className="flex justify-between text-sm text-muted">
          <span>{finished ? (failed ? t("send.someFailed") : t("send.allDone")) : t("send.progress", { done: f.num(done), total: f.num(items.length) })}</span>
          <span className="num">{f.bytes(sentBytes)} / {f.bytes(total)}</span>
        </div>
        {finished ? (
          <Button variant="primary" size="lg" onClick={() => queue.reset()}>{t("send.sendMore")}</Button>
        ) : (
          <Button size="lg" onClick={() => queue.cancelAll()}>{t("send.cancelAll")}</Button>
        )}
      </div>
    );
  }

  return (
    <Screen header={header} footer={footer}>
      <input ref={fileInput} type="file" multiple hidden onChange={onPick} />
      <input ref={dirInput} type="file" multiple hidden onChange={onPick} {...DIR_ATTRS} />

      <div className="max-w-md mx-auto space-y-4">
        {selecting && (
          <>
            {items.length === 0 && <p className="text-muted leading-relaxed">{t("send.intro")}</p>}

            {phase === "rejected" && (
              <p className="vd-card p-3.5 flex gap-2.5 text-sm text-warn" role="alert">
                <TriangleAlert size={18} className="shrink-0 mt-0.5" aria-hidden />
                <span>{t("send.rejected")}</span>
              </p>
            )}
            {phase === "failed" && (
              <p className="vd-card p-3.5 flex gap-2.5 text-sm text-danger" role="alert">
                <TriangleAlert size={18} className="shrink-0 mt-0.5" aria-hidden />
                <span>{t("send.offerFailed")} {error && errorText(t, f, error)}</span>
              </p>
            )}
            {phase === "select" && error && (
              <p className="vd-card p-3.5 flex gap-2.5 text-sm text-warn" role="alert">
                <TriangleAlert size={18} className="shrink-0 mt-0.5" aria-hidden />
                <span>{errorText(t, f, error)}</span>
              </p>
            )}

            <div className="grid grid-cols-2 gap-3">
              <Button icon={items.length ? <Plus size={18} aria-hidden /> : <FileIcon size={18} aria-hidden />} onClick={() => fileInput.current?.click()}>
                {items.length ? t("send.addMore") : t("send.pickFiles")}
              </Button>
              <Button icon={<FolderOpen size={18} aria-hidden />} onClick={() => dirInput.current?.click()}>
                {t("send.pickFolder")}
              </Button>
            </div>

            {items.length === 0 && <p className="text-center text-sm text-faint pt-6">{t("send.empty")}</p>}
          </>
        )}

        {(phase === "offering" || phase === "waiting") && (
          <div className="flex flex-col items-center text-center gap-5 py-10">
            <LinkLoader state="waiting" />
            <p className="text-muted leading-relaxed max-w-xs" role="status">{t("send.waiting")}</p>
            <p className="text-sm text-faint max-w-xs">{t("send.keepOpen")}</p>
          </div>
        )}

        {sending && !finished && <p className="text-sm text-faint">{t("send.keepOpen")}</p>}

        {items.length > 0 && (
          <ul className="space-y-2.5">
            {items.map((i) => (
              <ItemRow key={i.key} item={i} phase={phase} queue={queue} />
            ))}
          </ul>
        )}
      </div>
    </Screen>
  );
}
