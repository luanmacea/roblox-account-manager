/**
 * Ponto do clique do AFK mode: porcentagem da área interna da janela (0–100 nos
 * dois eixos), para cair no mesmo lugar em janela de qualquer tamanho.
 *
 * O ponto padrão fica no INI (`Afk.ClickX`/`Afk.ClickY`); o ponto próprio de uma
 * conta, em `Account.Fields` (`AfkClickX`/`AfkClickY`) — o backend lê os mesmos
 * campos a cada ciclo (`afk_point_from_fields`), então mudar vale no ciclo
 * seguinte sem religar o modo.
 */
/** `recording` toca a gravação de cada conta (docs/features/recordings.md). */
export type AfkMode = "key" | "click" | "recording";

export interface AfkPoint {
  x: number;
  y: number;
}

export const AFK_DEFAULT_POINT: AfkPoint = { x: 50, y: 50 };

const FIELD_X = "AfkClickX";
const FIELD_Y = "AfkClickY";

/** Travada em 0–100; o que não é número vira o meio (`clamp_afk_percent`). */
export function clampAfkPercent(value: number): number {
  return Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 50;
}

/** Ponto próprio da conta. Só vale com os dois números: um sem o outro cai no padrão. */
export function readAfkPoint(fields: Record<string, string> | undefined): AfkPoint | null {
  const x = Number.parseFloat(fields?.[FIELD_X] ?? "");
  const y = Number.parseFloat(fields?.[FIELD_Y] ?? "");
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x: clampAfkPercent(x), y: clampAfkPercent(y) };
}

/** `Fields` novo com o ponto gravado — ou apagado, com `null`. Os outros campos ficam. */
export function writeAfkPoint(
  fields: Record<string, string> | undefined,
  point: AfkPoint | null
): Record<string, string> {
  const next = { ...(fields ?? {}) };
  delete next[FIELD_X];
  delete next[FIELD_Y];
  if (point) {
    next[FIELD_X] = String(clampAfkPercent(point.x));
    next[FIELD_Y] = String(clampAfkPercent(point.y));
  }
  return next;
}

/** O ponto padrão do INI (`Afk.ClickX`/`Afk.ClickY`); cada eixo ilegível vira o meio. */
export function readAfkSettingsPoint(afk: Record<string, string> | undefined): AfkPoint {
  return {
    x: clampAfkPercent(Number.parseFloat(afk?.ClickX ?? "50")),
    y: clampAfkPercent(Number.parseFloat(afk?.ClickY ?? "50")),
  };
}

export function formatAfkPoint(point: AfkPoint): string {
  return `${clampAfkPercent(point.x)}% × ${clampAfkPercent(point.y)}%`;
}
