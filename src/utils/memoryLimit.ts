/**
 * Teto de memória dos clientes que o app abriu (commands/memory_ceiling.rs).
 * O padrão de todas as contas é `Optimization.MemoryLimit` (MB, 0 = sem
 * limite); a conta pode ter o seu no campo `MemoryLimit` ("0" = sem limite),
 * que vence o padrão e vale também no próximo launch. Nada aqui decide quando
 * liberar ou fechar: é só o espelho do que o backend lê.
 */

export const MEMORY_LIMIT_FIELD = "MemoryLimit";
/** `Optimization.MemoryLimit`: o padrão de todas as contas. */
export const MEMORY_LIMIT_SETTING = { section: "Optimization", key: "MemoryLimit" } as const;

const MIN_MB = 256;
const MAX_MB = 65536;

/** Os tamanhos da lista, em MB. */
export const MEMORY_LIMIT_PRESETS_MB = [1024, 1536, 2048, 3072, 4096];

/** `parse_memory_limit`: número em MB (0 = sem limite) ou `null` (vazio/ilegível). */
export function parseMemoryLimit(raw: string | undefined | null): number | null {
  const value = (raw ?? "").trim();
  if (!value) return null;
  if (value.toLowerCase() === "off") return 0;
  if (!/^\d+$/.test(value)) return null;
  const mb = Number(value);
  if (mb === 0) return 0;
  return Math.min(MAX_MB, Math.max(MIN_MB, mb));
}

export interface MemoryLimitChoice {
  /** O da conta: `null` segue o padrão; `0` sem limite. */
  own: number | null;
  /** O padrão de todas as contas (0 = sem limite). */
  defaultMb: number;
  /** O que vale (0 = sem limite). */
  effectiveMb: number;
}

export function memoryLimitChoice(
  fields: Record<string, string> | undefined,
  defaultRaw: string | undefined
): MemoryLimitChoice {
  const own = parseMemoryLimit(fields?.[MEMORY_LIMIT_FIELD]);
  const defaultMb = parseMemoryLimit(defaultRaw) ?? 0;
  return { own, defaultMb, effectiveMb: own ?? defaultMb };
}

/** Os campos com o limite da conta (`null` tira o campo: volta ao padrão). */
export function fieldsWithMemoryLimit(
  fields: Record<string, string> | undefined,
  value: number | null
): Record<string, string> {
  const next = { ...(fields ?? {}) };
  if (value === null) delete next[MEMORY_LIMIT_FIELD];
  else next[MEMORY_LIMIT_FIELD] = String(value);
  return next;
}

/** "900 MB", "1 GB", "1.5 GB". */
export function formatMemoryMb(mb: number): string {
  if (mb < 1024) return `${Math.round(mb)} MB`;
  const gb = Math.round((mb / 1024) * 10) / 10;
  return `${Number.isInteger(gb) ? gb.toFixed(0) : gb.toFixed(1)} GB`;
}

/** O que a pessoa digitou no "Custom…": MB, ou GB com a unidade. */
export function parseCustomMemoryLimit(raw: string): number | null {
  const match = raw.trim().toLowerCase().match(/^(\d+(?:[.,]\d+)?)\s*(gb|g|mb|m)?$/);
  if (!match) return null;
  const amount = Number(match[1].replace(",", "."));
  if (!Number.isFinite(amount) || amount <= 0) return null;
  const mb = match[2]?.startsWith("g") ? Math.round(amount * 1024) : Math.round(amount);
  return mb > 0 ? mb : null;
}
