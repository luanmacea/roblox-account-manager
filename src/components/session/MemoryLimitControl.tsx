import { usePrompt } from "../../hooks/usePrompt";
import { useTr } from "../../i18n/text";
import type { ClientMemory } from "../../types";
import {
  MEMORY_LIMIT_PRESETS_MB,
  formatMemoryMb,
  parseCustomMemoryLimit,
  parseMemoryLimit,
  type MemoryLimitChoice,
} from "../../utils/memoryLimit";

/**
 * Teto de memória de um cliente, compacto, na linha da lista "Em jogo"
 * (Painel de Sessão): um seletor nativo — padrão, sem limite, os tamanhos
 * comuns, o próprio da conta e "Custom…". Grava no campo `MemoryLimit` da
 * conta (vale na hora, sem relançar, e nos próximos launches). Passou do
 * limite, o app libera a memória primeiro; fecha só com a opção do Watcher.
 */

/** `null` = volta ao padrão; `0` = sem limite; MB. */
export type MemoryLimitValue = number | null;

/** As opções do seletor (a da linha e a do lote). */
function LimitOptions({ defaultMb, own }: { defaultMb: number; own: number | null }) {
  const t = useTr();
  const custom = own !== null && own > 0 && !MEMORY_LIMIT_PRESETS_MB.includes(own) ? own : null;
  return (
    <>
      <option value="default">
        {defaultMb > 0
          ? t("Default ({{limit}})", { limit: formatMemoryMb(defaultMb) })
          : t("Default (no limit)")}
      </option>
      <option value="0">{t("No limit")}</option>
      {MEMORY_LIMIT_PRESETS_MB.map((mb) => (
        <option key={mb} value={String(mb)}>
          {formatMemoryMb(mb)}
        </option>
      ))}
      {custom !== null && <option value={String(custom)}>{formatMemoryMb(custom)}</option>}
      <option value="custom">{t("Custom…")}</option>
    </>
  );
}

/**
 * O seletor. `ariaLabel` diz de quem é (a conta, ou "N selecionadas" no
 * lote); `choice` `null` no lote (não há um valor só).
 */
export function MemoryLimitSelect({
  ariaLabel,
  choice,
  defaultMb,
  disabled = false,
  onChange,
}: {
  ariaLabel: string;
  choice: MemoryLimitChoice | null;
  defaultMb: number;
  disabled?: boolean;
  onChange: (value: MemoryLimitValue) => void;
}) {
  const t = useTr();
  const prompt = usePrompt();
  const own = choice?.own ?? null;
  const value = choice === null ? "" : own === null ? "default" : String(own);
  const defaultText = defaultMb > 0 ? formatMemoryMb(defaultMb) : t("no limit");
  const title =
    choice === null
      ? t("Above the limit, MultiAlt first asks Windows to free the client's memory.")
      : own === null
        ? t("Following the default ({{limit}}). Above the limit, MultiAlt first asks Windows to free the client's memory.", {
            limit: defaultText,
          })
        : t("Set for this account. Above the limit, MultiAlt first asks Windows to free the client's memory.");

  async function pick(raw: string) {
    if (raw === "default") return onChange(null);
    if (raw === "custom") {
      const typed = await prompt(t("Memory limit for this client, in MB or GB (for example 1800 or 2.5 GB)"), "");
      if (typed === null) return;
      const mb = parseCustomMemoryLimit(typed);
      if (mb === null) return;
      return onChange(parseMemoryLimit(String(mb)));
    }
    const mb = parseMemoryLimit(raw);
    if (mb !== null) onChange(mb);
  }

  return (
    <select
      aria-label={ariaLabel}
      title={title}
      value={value}
      disabled={disabled}
      onChange={(e) => void pick(e.target.value)}
      className="w-[104px] shrink-0 rounded-md border theme-border bg-[var(--panel-bg)] px-1.5 py-0.5 text-[11px] text-[var(--panel-fg)] disabled:opacity-50"
    >
      {choice === null && (
        <option value="" disabled>
          {t("Choose a limit")}
        </option>
      )}
      <LimitOptions defaultMb={defaultMb} own={own} />
    </select>
  );
}

/** A memória do cliente agora ("1.2 GB"); âmbar acima do limite. */
export function MemoryReading({ memory }: { memory: ClientMemory | undefined }) {
  const t = useTr();
  if (!memory || memory.memoryMb === null) return null;
  const limit = memory.limitMb !== null ? formatMemoryMb(memory.limitMb) : null;
  const title = memory.over && limit
    ? memory.trimmedAtMs !== null
      ? t("Over its limit of {{limit}}: MultiAlt asked Windows to free this client's memory.", { limit })
      : t("Over its limit of {{limit}}.", { limit })
    : limit
      ? t("Memory in use now. Limit: {{limit}}.", { limit })
      : t("Memory in use now.");
  return (
    <span
      title={title}
      className={`shrink-0 tabular-nums text-[11px] ${memory.over ? "text-amber-400" : "theme-muted"}`}
    >
      {formatMemoryMb(memory.memoryMb)}
    </span>
  );
}

/**
 * O padrão de todos os clientes (`Optimization.MemoryLimit`), no resumo da
 * página Session: sem limite, os tamanhos comuns, o valor atual se for outro e
 * "Custom…". `onChange` recebe MB (0 = sem limite).
 */
export function MemoryDefaultSelect({
  valueMb,
  onChange,
}: {
  valueMb: number;
  onChange: (mb: number) => void;
}) {
  const t = useTr();
  const prompt = usePrompt();
  const custom = valueMb > 0 && !MEMORY_LIMIT_PRESETS_MB.includes(valueMb) ? valueMb : null;

  async function pick(raw: string) {
    if (raw === "custom") {
      const typed = await prompt(t("Memory limit for each client, in MB or GB (for example 1800 or 2.5 GB)"), "");
      if (typed === null) return;
      const mb = parseCustomMemoryLimit(typed);
      const limit = mb === null ? null : parseMemoryLimit(String(mb));
      if (limit !== null) onChange(limit);
      return;
    }
    const mb = parseMemoryLimit(raw);
    if (mb !== null) onChange(mb);
  }

  return (
    <select
      aria-label={t("Memory limit for every client")}
      value={String(valueMb)}
      onChange={(e) => void pick(e.target.value)}
      className="rounded-md border theme-border bg-[var(--panel-bg)] px-2 py-1 text-[12px] text-[var(--panel-fg)]"
    >
      <option value="0">{t("No limit")}</option>
      {MEMORY_LIMIT_PRESETS_MB.map((mb) => (
        <option key={mb} value={String(mb)}>
          {formatMemoryMb(mb)}
        </option>
      ))}
      {custom !== null && <option value={String(custom)}>{formatMemoryMb(custom)}</option>}
      <option value="custom">{t("Custom…")}</option>
    </select>
  );
}
