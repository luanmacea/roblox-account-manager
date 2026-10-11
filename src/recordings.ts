/**
 * Gravações: sequências de passos (tecla, clique num ponto relativo da janela,
 * espera) que o app toca na janela do Roblox de uma conta, uma janela por vez.
 * Espelho do `data/recordings.rs` — os limites aqui são os mesmos de lá, e o
 * backend normaliza de novo ao gravar. Ver docs/features/recordings.md.
 */

export type RecordingStep =
  | { type: "key"; key: string; holdMs: number }
  | { type: "keyDown"; key: string }
  | { type: "keyUp"; key: string }
  | { type: "click"; xPct: number; yPct: number }
  | { type: "wait"; ms: number };

export type RecordingStepType = RecordingStep["type"];

export interface Recording {
  id: string;
  name: string;
  steps: RecordingStep[];
  createdAt: number;
  updatedAt: number;
}

/** O que `get_recordings` devolve. */
export interface RecordingsPayload {
  recordings: Recording[];
  defaultId: string | null;
  /** user id (texto) → gravação própria da conta. */
  accountIds: Record<string, string>;
  /** A lista fechada de teclas, na ordem do backend. */
  keys: string[];
}

/** Resultado do "Tocar agora", por conta. */
export interface RecordingPlayResult {
  userId: number;
  errorCode: string | null;
  error: string | null;
}

export const MAX_RECORDING_STEPS = 500;
export const MAX_RECORDING_NAME_CHARS = 60;
export const MAX_WAIT_MS = 600_000;
export const MIN_HOLD_MS = 10;
export const MAX_HOLD_MS = 10_000;
export const DEFAULT_HOLD_MS = 40;
export const MAX_RECORDING_TOTAL_MS = 600_000;
/** O mesmo `CLICK_ESTIMATE_MS` do backend. */
export const CLICK_ESTIMATE_MS = 800;

/** Um passo novo, com valores que já podem ser tocados. */
export function newStep(type: RecordingStepType, firstKey = "Space"): RecordingStep {
  switch (type) {
    case "key":
      return { type, key: firstKey, holdMs: DEFAULT_HOLD_MS };
    case "keyDown":
    case "keyUp":
      return { type, key: firstKey };
    case "click":
      return { type, xPct: 50, yPct: 50 };
    case "wait":
      return { type, ms: 500 };
  }
}

/** Troca o tipo do passo, mantendo a tecla quando o novo tipo também tem. */
export function changeStepType(step: RecordingStep, type: RecordingStepType, firstKey = "Space"): RecordingStep {
  const key = "key" in step ? step.key : firstKey;
  return newStep(type, key);
}

/** Duração estimada (esperas + teclas seguradas + cliques), como o backend conta. */
export function recordingDurationMs(steps: RecordingStep[]): number {
  return steps.reduce((total, step) => {
    switch (step.type) {
      case "key":
        return total + step.holdMs;
      case "wait":
        return total + step.ms;
      case "click":
        return total + CLICK_ESTIMATE_MS;
      default:
        return total;
    }
  }, 0);
}

/** `1.2 s`, `45 s`, `3 min 5 s`. */
export function formatDurationMs(ms: number): string {
  if (ms < 10_000) return `${(Math.round(ms / 100) / 10).toFixed(1)} s`;
  const total = Math.round(ms / 1000);
  if (total < 60) return `${total} s`;
  const m = Math.floor(total / 60);
  const s = total % 60;
  return s ? `${m} min ${s} s` : `${m} min`;
}

/** O primeiro problema da gravação, como código; `null` quando pode ser salva. */
export type RecordingProblem = "noName" | "nameTooLong" | "tooManySteps" | "tooLong" | "badKey";

export function recordingProblem(name: string, steps: RecordingStep[], keys: string[]): RecordingProblem | null {
  const trimmed = name.trim();
  if (!trimmed) return "noName";
  if ([...trimmed].length > MAX_RECORDING_NAME_CHARS) return "nameTooLong";
  if (steps.length > MAX_RECORDING_STEPS) return "tooManySteps";
  if (steps.some((s) => "key" in s && !keys.includes(s.key))) return "badKey";
  if (recordingDurationMs(steps) > MAX_RECORDING_TOTAL_MS) return "tooLong";
  return null;
}

/** Inteiro dentro dos limites; o que não é número vira `fallback`. */
export function clampInt(value: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

/** A gravação que vale para a conta: a própria, senão a de todas. */
export function recordingForAccount(payload: RecordingsPayload | null, userId: number): Recording | null {
  if (!payload) return null;
  const find = (id: string | null | undefined) => (id ? payload.recordings.find((r) => r.id === id) ?? null : null);
  return find(payload.accountIds[String(userId)]) ?? find(payload.defaultId);
}

/** Move o passo `index` uma posição para cima (`-1`) ou para baixo (`+1`). */
export function moveStep(steps: RecordingStep[], index: number, delta: -1 | 1): RecordingStep[] {
  const target = index + delta;
  if (index < 0 || index >= steps.length || target < 0 || target >= steps.length) return steps;
  const next = [...steps];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}
