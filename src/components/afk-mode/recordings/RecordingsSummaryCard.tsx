import { Clapperboard } from "lucide-react";
import { useStore } from "../../../store";
import { useTr } from "../../../i18n/text";
import { useRecordings } from "./useRecordings";
import { recordingTriggers } from "./triggers";
import { RecordingTriggerLines } from "./RecordingTriggerLines";

/**
 * Cartão "Recordings" do resumo da página Session: qual gravação vale para
 * todas as contas, quando ela toca (Modo AFK de tanto em tanto tempo, depois
 * da reconexão) e o atalho para a aba Recordings do Modo AFK. Só leitura —
 * tudo se muda na aba.
 */
export function RecordingsSummaryCard() {
  const t = useTr();
  const store = useStore();
  const { payload } = useRecordings();
  const triggers = recordingTriggers(payload, store.settings);
  return (
    <section className="rounded-xl border theme-border px-4 py-3" aria-label={t("Recordings")} data-testid="session-recordings">
      <h2 className="text-[12px] font-semibold text-[var(--panel-fg)]">{t("Recordings")}</h2>
      <p className="mt-1.5 text-[12px] leading-snug text-[var(--panel-muted)]">
        {triggers.allAccountsName
          ? t("All accounts: {{name}}.", { name: triggers.allAccountsName })
          : t("No recording for all accounts.")}
        {triggers.ownCount > 0
          ? ` ${t("{{count}} account(s) with their own.", { count: triggers.ownCount })}`
          : ""}
      </p>
      <RecordingTriggerLines triggers={triggers} className="mt-1" />
      <button
        type="button"
        onClick={() => store.openAfkMode({ tab: "recordings" })}
        className="theme-btn mt-3 inline-flex items-center gap-1.5 px-3 py-1.5 text-[12px] font-medium"
      >
        <Clapperboard size={13} strokeWidth={1.8} aria-hidden="true" />
        {t("Open Recordings")}
      </button>
    </section>
  );
}
