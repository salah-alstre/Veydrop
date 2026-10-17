import { useTranslation } from "react-i18next";
import { Unplug } from "lucide-react";
import { Wordmark } from "../ui/Logo";

/** The session is gone (revoked on the PC, server stopped, or the phone disconnected). */
export function Ended() {
  const { t } = useTranslation();
  return (
    <div className="min-h-dvh flex flex-col safe-x safe-top safe-bottom vd-grid-bg">
      <div className="pt-8 pb-4 flex justify-center">
        <Wordmark size={26} />
      </div>
      <div className="flex-1 flex flex-col items-center justify-center text-center gap-5 pb-10">
        <div className="size-16 rounded-2xl bg-surface-2 border border-line-strong grid place-items-center text-muted">
          <Unplug size={30} aria-hidden />
        </div>
        <div className="max-w-sm space-y-2" role="status">
          <h1 className="text-2xl font-semibold tracking-tight">{t("ended.title")}</h1>
          <p className="text-muted leading-relaxed">{t("ended.body")}</p>
        </div>
      </div>
    </div>
  );
}
