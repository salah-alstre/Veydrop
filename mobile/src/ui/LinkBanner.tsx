import { useTranslation } from "react-i18next";
import { WifiOff } from "lucide-react";
import type { LinkState } from "../lib/events";

/** Shown only while the live connection to the PC is down. Never polls; the stream reconnects itself. */
export function LinkBanner({ link }: { link: LinkState }) {
  const { t } = useTranslation();
  if (link === "online") return null;
  return (
    <div role="status" className="safe-x flex items-start gap-2.5 py-2.5 bg-warn/10 border-b border-warn/25 text-warn text-sm leading-snug">
      <WifiOff size={18} className="shrink-0 mt-0.5" aria-hidden />
      <span>{link === "connecting" ? t("link.connecting") : t("link.lost")}</span>
    </div>
  );
}
