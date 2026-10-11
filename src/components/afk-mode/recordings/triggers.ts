/**
 * Os dois gatilhos das gravações, lidos do INI e da biblioteca: o **Modo AFK**
 * no modo gravação (`Afk.Mode = recording`, a cada `Afk.IntervalMinutes` +
 * `Afk.IntervalSeconds`) e **depois da reconexão** (`Recordings.AfterReconnect`,
 * `Recordings.AfterReconnectDelaySeconds`). Um resumo só, mostrado na aba
 * Recordings, no cartão da aba no Modo AFK e no resumo da página Session — para
 * a pessoa não ter de caçar onde cada coisa liga.
 */
import { parseAfkMode, readAfkInterval } from "../clicks/useClicksController";
import type { RecordingsPayload } from "../../../recordings";

export interface RecordingTriggers {
  /** O Modo AFK, quando ligado, toca a gravação de cada conta. */
  afkRepeats: boolean;
  /** Intervalo do Modo AFK, em segundos. */
  intervalSeconds: number;
  /** A conta que a reconexão devolveu toca a gravação dela. */
  afterReconnect: boolean;
  /** Tempo no jogo antes de tocar depois da reconexão. */
  delaySeconds: number;
  /** A gravação de todas as contas (`null` = nenhuma). */
  allAccountsName: string | null;
  /** Contas com gravação própria (que existe). */
  ownCount: number;
}

/** O clamp do backend (`Recordings.AfterReconnectDelaySeconds`). */
const MIN_DELAY_S = 5;
const MAX_DELAY_S = 3_600;

export function recordingTriggers(
  payload: RecordingsPayload | null,
  settings: Record<string, Record<string, string>> | null | undefined
): RecordingTriggers {
  const afk = settings?.Afk;
  const rec = settings?.Recordings ?? {};
  const interval = readAfkInterval(afk);
  const delay = Number.parseInt(rec.AfterReconnectDelaySeconds ?? "", 10);
  const exists = (id: string | null | undefined) => !!id && !!payload?.recordings.some((r) => r.id === id);
  return {
    afkRepeats: parseAfkMode(afk?.Mode) === "recording",
    intervalSeconds: interval.minutes * 60 + interval.seconds,
    afterReconnect: rec.AfterReconnect === "true",
    delaySeconds: Number.isFinite(delay) ? Math.min(MAX_DELAY_S, Math.max(MIN_DELAY_S, delay)) : 30,
    allAccountsName: exists(payload?.defaultId)
      ? payload!.recordings.find((r) => r.id === payload!.defaultId)!.name
      : null,
    ownCount: Object.values(payload?.accountIds ?? {}).filter((id) => exists(id)).length,
  };
}

/** "10 min", "2 min 30 s", "45 s". */
export function formatAfkIntervalSeconds(total: number): string {
  const m = Math.floor(total / 60);
  const s = total % 60;
  if (m === 0) return `${s} s`;
  return s ? `${m} min ${s} s` : `${m} min`;
}
