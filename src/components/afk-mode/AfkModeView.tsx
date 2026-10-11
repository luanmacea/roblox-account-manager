import { useRef, useState, type KeyboardEvent } from "react";
import { Clapperboard, MousePointerClick, Repeat, X, type LucideIcon } from "lucide-react";
import { useStore, type AfkModeTab } from "../../store";
import { useTr } from "../../i18n/text";
import { RejoinTab } from "./RejoinTab";
import { ClicksTab } from "./ClicksTab";
import { RecordingsTab } from "./RecordingsTab";
import { formatAfkIntervalSeconds, recordingTriggers } from "./recordings/triggers";

export interface AfkModeViewProps {
  /** `modal`: dentro do `AfkModeDialog`. `page`: página inteira da navegação. */
  variant: "modal" | "page";
  initialTab?: AfkModeTab;
  /** Contas de quem abriu (o "Em jogo"); sem isto, a seleção da lista. */
  targetUserIds?: number[];
  /** As contas já estão em jogo: o Auto Rejoin adota, sem relançar. */
  adoptRunning?: boolean;
  /** Jogo escolhido na abertura (clique direito num jogo). */
  initialPlaceId?: string | null;
  /** Só no modal: o X do cabeçalho. */
  onClose?: () => void;
}

/**
 * Modo AFK: as duas formas de deixar as contas no jogo sem você — o **Auto
 * Rejoin** (fecha e reabre o cliente das alts de tempo em tempo) e os **cliques
 * AFK** (uma tecla ou clique de tempo em tempo, sem fechar nada). Antes eram
 * dois diálogos separados.
 *
 * Cada aba mostra se o modo dela está ligado, na própria aba e na barra de
 * estado do topo, com ligar/parar sempre à vista.
 *
 * Precisa de um pai com altura definida (o modal tem; a página deve dar
 * `h-full`): as abas rolam por dentro, e a barra de estado fica parada.
 */
export function AfkModeView({
  variant,
  // Os cliques AFK são o padrão e a primeira aba (pedido do dono, 03/10/2026).
  initialTab = "clicks",
  targetUserIds,
  adoptRunning,
  initialPlaceId = null,
  onClose,
}: AfkModeViewProps) {
  const t = useTr();
  const store = useStore();
  const [tab, setTab] = useState<AfkModeTab>(initialTab);
  const tabRefs = useRef<Record<AfkModeTab, HTMLButtonElement | null>>({ rejoin: null, clicks: null, recordings: null });
  const page = variant === "page";
  // O cartão da aba Recordings diz quando a gravação toca, sem abrir a aba.
  const triggers = recordingTriggers(null, store.settings);
  const recordingsHint =
    triggers.afkRepeats && triggers.afterReconnect
      ? t("Every {{interval}} in AFK mode, and after a reconnect", {
          interval: formatAfkIntervalSeconds(triggers.intervalSeconds),
        })
      : triggers.afkRepeats
        ? t("Every {{interval}} in AFK mode", { interval: formatAfkIntervalSeconds(triggers.intervalSeconds) })
        : triggers.afterReconnect
          ? t("Plays after an automatic reconnect")
          : t("Sequences of keys, clicks and waits played on each window");

  const tabs: {
    id: AfkModeTab;
    label: string;
    hint: string;
    Icon: LucideIcon;
    running: boolean;
  }[] = [
    {
      id: "clicks",
      label: t("AFK clicks"),
      hint: t("Sends a key or a click so accounts do not go idle"),
      Icon: MousePointerClick,
      running: store.afkStatus?.active === true,
    },
    {
      // Gravações (docs/features/recordings.md): o que tocam fica nesta aba;
      // o "rodando" é o do Modo AFK no modo gravação.
      id: "recordings",
      label: t("Recordings"),
      hint: recordingsHint,
      Icon: Clapperboard,
      running: store.afkStatus?.active === true && store.afkStatus?.mode === "recording",
    },
    {
      id: "rejoin",
      label: t("Auto Rejoin"),
      hint: t("Closes and reopens alt clients on a timer"),
      Icon: Repeat,
      running: !!store.bottingStatus?.active,
    },
  ];

  function onTabKeyDown(e: KeyboardEvent<HTMLButtonElement>) {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const order = tabs.map((it) => it.id);
    const at = order.indexOf(tab);
    const next: AfkModeTab = order[(at + (e.key === "ArrowRight" ? 1 : order.length - 1)) % order.length];
    setTab(next);
    tabRefs.current[next]?.focus();
  }

  const tabList = (
    <div role="tablist" aria-label={t("AFK Mode")} data-tour="afk-tabs" className={`grid shrink-0 grid-cols-3 gap-2 ${page ? "" : "min-w-[min(100%,520px)] flex-1"}`}>
      {tabs.map(({ id, label, hint, Icon, running }) => {
        const selected = tab === id;
        return (
          <button
            key={id}
            ref={(el) => {
              tabRefs.current[id] = el;
            }}
            type="button"
            role="tab"
            id={`afk-mode-tab-${id}`}
            data-tour={`afk-tab-${id}`}
            aria-selected={selected}
            aria-controls={`afk-mode-panel-${id}`}
            tabIndex={selected ? 0 : -1}
            onClick={() => setTab(id)}
            onKeyDown={onTabKeyDown}
            className={`group relative flex min-w-0 items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--input-focus)] ${
              selected
                ? "theme-accent-border bg-[var(--accent-soft)]"
                : "theme-border bg-[var(--panel-soft)] hover:brightness-110"
            }`}
          >
            <span
              className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border ${
                selected ? "theme-accent-border theme-accent" : "theme-border theme-muted"
              }`}
              aria-hidden
            >
              <Icon size={16} strokeWidth={1.75} />
            </span>
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-2">
                <span className="truncate text-[13px] font-semibold text-[var(--panel-fg)]">{label}</span>
                <span
                  data-testid={`afk-mode-tab-state-${id}`}
                  className={`inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-px text-[11px] ${
                    running ? "bg-emerald-500/15 text-emerald-300" : "theme-muted"
                  }`}
                >
                  <span
                    className={`h-1.5 w-1.5 rounded-full ${
                      running ? "bg-emerald-400 motion-safe:animate-pulse" : "bg-[var(--panel-muted)] opacity-60"
                    }`}
                    aria-hidden
                  />
                  {running ? t("Running") : t("Stopped")}
                </span>
              </span>
              <span className="mt-0.5 block truncate text-[11px] theme-muted">{hint}</span>
            </span>
          </button>
        );
      })}
    </div>
  );

  const title = (
    <div className="min-w-0 shrink-0">
      <h2
        className={`font-semibold tracking-tight text-[var(--panel-fg)] ${
          page ? "text-[20px]" : "text-[16px]"
        }`}
      >
        {t("AFK Mode")}
      </h2>
      <p className="mt-0.5 text-[12px] theme-muted">
        {t("Keep your accounts in the game while you are away.")}
      </p>
    </div>
  );

  return (
    <div
      data-testid="afk-mode-view"
      data-variant={variant}
      className={`flex h-full min-h-0 flex-col ${page ? "gap-4 px-6 pb-5 pt-4" : "gap-3 px-5 pb-5 pt-4"}`}
    >
      {page ? (
        // Na página o título e a descrição são do PageShell (AfkPage): aqui só as abas.
        tabList
      ) : (
        // No modal, título, abas e o X dividem uma linha: a altura vai para a
        // lista ao vivo, não para cabeçalho.
        <header className="flex shrink-0 items-start gap-3">
          <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-5 gap-y-3">
            {title}
            {tabList}
          </div>
          {onClose ? (
            <button
              onClick={onClose}
              aria-label={t("Close")}
              title={t("Close")}
              className="p-1 rounded-md theme-muted hover:text-[var(--panel-fg)] transition-colors"
            >
              <X size={16} strokeWidth={2} />
            </button>
          ) : null}
        </header>
      )}

      {/* As duas abas ficam montadas: trocar de aba não perde o que foi
          marcado na outra (contas, tecla, rascunho do ciclo). */}
      <div
        role="tabpanel"
        id="afk-mode-panel-rejoin"
        aria-labelledby="afk-mode-tab-rejoin"
        hidden={tab !== "rejoin"}
        className="min-h-0 flex-1"
      >
        <RejoinTab
          targetUserIds={targetUserIds}
          adoptRunning={adoptRunning}
          initialPlaceId={initialPlaceId}
        />
      </div>
      <div
        role="tabpanel"
        id="afk-mode-panel-clicks"
        aria-labelledby="afk-mode-tab-clicks"
        hidden={tab !== "clicks"}
        className="min-h-0 flex-1"
      >
        <ClicksTab targetUserIds={targetUserIds} />
      </div>
      <div
        role="tabpanel"
        id="afk-mode-panel-recordings"
        aria-labelledby="afk-mode-tab-recordings"
        hidden={tab !== "recordings"}
        className="min-h-0 flex-1"
      >
        <RecordingsTab />
      </div>
    </div>
  );
}
