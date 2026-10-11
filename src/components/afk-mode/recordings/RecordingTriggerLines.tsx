import { useTr } from "../../../i18n/text";
import { formatAfkIntervalSeconds, type RecordingTriggers } from "./triggers";

/**
 * As duas linhas "quando a gravação toca": no Modo AFK (de quanto em quanto
 * tempo) e depois da reconexão automática (quanto tempo depois). A mesma frase
 * na aba Recordings e no resumo da página Session.
 */
export function RecordingTriggerLines({ triggers, className = "" }: { triggers: RecordingTriggers; className?: string }) {
  const t = useTr();
  return (
    <ul className={`space-y-0.5 text-[11.5px] leading-snug text-[var(--panel-muted)] ${className}`} data-testid="recording-triggers">
      <li>
        {triggers.afkRepeats
          ? t("AFK mode: plays it every {{interval}}.", { interval: formatAfkIntervalSeconds(triggers.intervalSeconds) })
          : t("AFK mode: off (it sends a key or a click instead).")}
      </li>
      <li>
        {triggers.afterReconnect
          ? t("After an automatic reconnect: plays once, {{seconds}} s after the account is back in the game.", {
              seconds: triggers.delaySeconds,
            })
          : t("After an automatic reconnect: off.")}
      </li>
    </ul>
  );
}
