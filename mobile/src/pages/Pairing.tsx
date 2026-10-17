import { useTranslation } from "react-i18next";
import { QrCode, ShieldAlert } from "lucide-react";
import type { PairState } from "../lib/pairing";
import { Button } from "../ui/Button";
import { LinkLoader } from "../ui/Loader";
import { Wordmark } from "../ui/Logo";
import { PairCode } from "../ui/PairCode";

/** Everything before the phone is connected: token missing, waiting for approval, or failed. */
export function Pairing({ state, onRetry }: { state: PairState; onRetry: () => void }) {
  const { t } = useTranslation();

  return (
    <div className="min-h-dvh flex flex-col safe-x safe-top safe-bottom vd-grid-bg">
      <div className="pt-8 pb-4 flex justify-center">
        <Wordmark size={26} />
      </div>
      <div className="flex-1 flex flex-col items-center justify-center text-center gap-6 pb-10">
        {state.kind === "checking" && (
          <>
            <LinkLoader state="connecting" />
            <p className="text-muted">{t("pair.checking")}</p>
          </>
        )}

        {state.kind === "needToken" && (
          <>
            <div className="size-16 rounded-2xl bg-accent-soft border border-accent/25 grid place-items-center text-accent">
              <QrCode size={30} aria-hidden />
            </div>
            <div className="max-w-sm space-y-2">
              <h1 className="text-2xl font-semibold tracking-tight">{t("pair.needToken.title")}</h1>
              <p className="text-muted leading-relaxed">{t("pair.needToken.body")}</p>
              <p className="text-sm text-faint">{t("pair.needToken.hint")}</p>
            </div>
          </>
        )}

        {state.kind === "waiting" && (
          <>
            <LinkLoader state="waiting" />
            <div className="max-w-sm space-y-2">
              <h1 className="text-2xl font-semibold tracking-tight">{t("pair.waiting.title")}</h1>
              <p className="text-muted leading-relaxed">{t("pair.waiting.body", { pc: state.pcName })}</p>
            </div>
            <div className="vd-card px-8 py-4">
              <div className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint mb-1">{t("pair.waiting.code")}</div>
              <PairCode code={state.code} label={t("pair.waiting.code")} className="text-4xl font-semibold" />
            </div>
          </>
        )}

        {state.kind === "failed" && (
          <>
            <div className="size-16 rounded-2xl bg-danger-soft border border-danger/25 grid place-items-center text-danger">
              <ShieldAlert size={30} aria-hidden />
            </div>
            <div className="max-w-sm space-y-2" role="alert">
              <h1 className="text-2xl font-semibold tracking-tight">{t(`pair.failed.${state.reason}.title`)}</h1>
              <p className="text-muted leading-relaxed">{t(`pair.failed.${state.reason}.body`)}</p>
            </div>
            {(state.reason === "network" || state.reason === "other") && (
              <Button variant="primary" onClick={onRetry}>
                {t("pair.tryAgain")}
              </Button>
            )}
          </>
        )}
      </div>
      <p className="pb-5 text-center text-xs text-faint">{t("app.tagline")}</p>
    </div>
  );
}
