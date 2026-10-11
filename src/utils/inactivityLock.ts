/**
 * Trancar o app por inatividade (ideia 27). Desligado por padrão e só com
 * senha do app: sem senha não há o que digitar para destrancar.
 *
 * Trancar é **só a tela**: o app continua montado por baixo (Scripts, AFK,
 * reconexão, Auto Rejoin seguem rodando) e destrancar só confere a senha
 * (`verify_app_password`), sem reler as contas.
 */

export const LOCK_MINUTES_DEFAULT = 10;
export const LOCK_MINUTES_MIN = 1;
export const LOCK_MINUTES_MAX = 240;

/** O valor do INI vira um número de minutos dentro dos limites. */
export function normalizeLockMinutes(raw: string | number | undefined | null): number {
  const n = typeof raw === "number" ? raw : Number.parseInt(String(raw ?? ""), 10);
  if (!Number.isFinite(n)) return LOCK_MINUTES_DEFAULT;
  return Math.min(LOCK_MINUTES_MAX, Math.max(LOCK_MINUTES_MIN, Math.round(n)));
}

/** A opção vale agora? Ligada no INI **e** com senha do app. */
export function inactivityLockActive(settingValue: string | undefined, hasAppPassword: boolean | null): boolean {
  return settingValue === "true" && hasAppPassword === true;
}

/** Já passou tempo demais desde a última interação na janela? */
export function idleLongEnough(lastActivityMs: number, nowMs: number, minutes: number): boolean {
  return nowMs - lastActivityMs >= normalizeLockMinutes(minutes) * 60_000;
}

/** O erro do `verify_app_password` (em inglês, do backend) na língua da tela. */
export function lockErrorText(error: string, t: (text: string, options?: Record<string, unknown>) => string): string {
  if (error.includes("Wrong password")) return t("Wrong password.");
  const wait = /Wait (\d+) seconds/.exec(error);
  if (wait) return t("Too many wrong passwords. Wait {{seconds}} seconds and try again.", { seconds: Number(wait[1]) });
  return error;
}
