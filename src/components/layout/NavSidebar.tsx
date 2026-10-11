import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  CircleHelp,
  Gamepad2,
  Keyboard,
  Layers,
  MessageSquareText,
  Palette,
  PanelLeftClose,
  PanelLeftOpen,
  Settings,
  Shirt,
  Sparkles,
  TerminalSquare,
  Users,
  UsersRound,
  type LucideIcon,
} from "lucide-react";
import { useStore, type AppPage } from "../../store";
import { useTr } from "../../i18n/text";
import { ENABLE_GROUPS, ENABLE_HELP_BUTTON, ENABLE_NEXUS } from "../../featureFlags";
import { Tooltip } from "../ui/Tooltip";
import { FeedbackDialog } from "../dialogs/FeedbackDialog";

/**
 * Barra lateral de navegação. Substitui os botões de ícone da Toolbar, que só
 * diziam o que faziam com o mouse parado em cima: aqui o nome fica sempre
 * visível, e cada item abre uma **página** na área principal (`activePage`).
 *
 * Recolhida, mostra só os ícones — e aí, só aí, o nome vem por tooltip. A
 * escolha fica no `localStorage` (preferência de quem está na máquina, como os
 * favoritos da Server List; ver docs/architecture.md, "Exceções à regra").
 * Janela estreita (menos de `NARROW_WIDTH`) recolhe sozinha: a lista de contas
 * precisa da largura mais do que os rótulos.
 */

export const NAV_COLLAPSED_KEY = "ram_nav_collapsed";
const NARROW_WIDTH = 900;

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(NAV_COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

function writeCollapsed(collapsed: boolean) {
  try {
    localStorage.setItem(NAV_COLLAPSED_KEY, collapsed ? "1" : "0");
  } catch {
    // Armazenamento bloqueado: a escolha vale só até fechar o app.
  }
}

function useNarrowWindow(): boolean {
  const [narrow, setNarrow] = useState(() => typeof window !== "undefined" && window.innerWidth < NARROW_WIDTH);
  useEffect(() => {
    const onResize = () => setNarrow(window.innerWidth < NARROW_WIDTH);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return narrow;
}

interface NavItemDef {
  page: AppPage;
  label: string;
  icon: LucideIcon;
  tour?: string;
}

/**
 * Ordem combinada com o dono. Os grupos só separam visualmente: a lista, o que
 * roda agora, e o que se monta/ajusta. Função (e não constante) para as flags
 * serem lidas a cada render — os testes as trocam.
 */
const navGroups = (): NavItemDef[][] => [
  [{ page: "accounts", label: "Accounts", icon: Users, tour: "nav-accounts" }],
  [
    { page: "session", label: "Session", icon: Gamepad2, tour: "nav-session" },
    { page: "afk", label: "AFK Mode", icon: Keyboard, tour: "nav-afk" },
  ],
  [
    { page: "avatars", label: "Avatars", icon: Shirt },
    ...(ENABLE_GROUPS ? [{ page: "groups" as const, label: "Groups", icon: UsersRound }] : []),
    { page: "scripts", label: "Scripts", icon: TerminalSquare },
    { page: "theme", label: "Theme", icon: Palette },
    ...(ENABLE_NEXUS ? [{ page: "nexus" as const, label: "Nexus", icon: Layers }] : []),
    { page: "settings", label: "Settings", icon: Settings, tour: "nav-settings" },
  ],
];

/**
 * "What's new" (Novidades) fica no rodapé, ao lado do recolher: fala do
 * próprio app, não do trabalho com as contas, e quem procura um deles olha ali.
 * É página como as de cima (marca `aria-current`), não ação como o Help. (O
 * Help está escondido por `ENABLE_HELP_BUTTON` desde 08/10/2026.)
 */
const FOOTER_ITEMS: NavItemDef[] = [{ page: "changelog", label: "What's new", icon: Sparkles }];

export function NavSidebar() {
  const t = useTr();
  const store = useStore();
  const narrow = useNarrowWindow();
  const [userCollapsed, setUserCollapsed] = useState(readCollapsed);
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  const collapsed = userCollapsed || narrow;

  const toggleCollapsed = useCallback(() => {
    setUserCollapsed((prev) => {
      const next = !prev;
      writeCollapsed(next);
      return next;
    });
  }, []);

  const running = store.launchedByProgram.size;
  const afkOn = store.afkStatus?.active === true || store.bottingStatus?.active === true;

  function trailing(page: AppPage): { visual: ReactNode; spoken: string | null } {
    if (page === "accounts" && store.accounts.length > 0) {
      return {
        visual: collapsed ? null : (
          <span className="ml-auto text-[11px] tabular-nums text-[var(--panel-muted)] opacity-70">
            {store.accounts.length}
          </span>
        ),
        spoken: null,
      };
    }
    if (page === "session" && running > 0) {
      return {
        visual: (
          <span
            data-testid="nav-session-count"
            aria-hidden="true"
            className={
              collapsed
                ? "absolute top-1 right-1 min-w-[15px] h-[15px] px-1 rounded-full bg-[var(--accent-color)] text-[11px] font-semibold leading-[15px] text-center text-black"
                : "ml-auto min-w-[18px] h-[18px] px-1.5 rounded-full bg-[var(--accent-soft)] text-[var(--accent-color)] text-[11px] font-semibold leading-[18px] text-center tabular-nums"
            }
          >
            {running}
          </span>
        ),
        spoken: t("{{count}} running", { count: running }),
      };
    }
    if (page === "afk" && afkOn) {
      return {
        visual: (
          <span
            data-testid="nav-afk-active"
            aria-hidden="true"
            className={collapsed ? "absolute top-1.5 right-1.5 flex" : "ml-auto flex items-center pr-1"}
          >
            <span className="relative flex w-1.5 h-1.5">
              <span className="absolute inset-0 rounded-full bg-emerald-400 opacity-60 animate-ping motion-reduce:animate-none" />
              <span className="relative w-1.5 h-1.5 rounded-full bg-emerald-400" />
            </span>
          </span>
        ),
        spoken: t("On"),
      };
    }
    return { visual: null, spoken: null };
  }

  function renderItem(item: NavItemDef) {
    const active = store.activePage === item.page;
    const Icon = item.icon;
    const label = t(item.label);
    const extra = trailing(item.page);
    const button = (
      <button
        type="button"
        data-nav={item.page}
        data-tour={item.tour}
        aria-current={active ? "page" : undefined}
        onClick={() => store.setActivePage(item.page)}
        className={`nav-item group relative flex items-center rounded-lg text-[13px] transition-colors outline-none focus-visible:shadow-[0_0_0_2px_var(--input-focus)] ${
          collapsed ? "w-10 h-10 justify-center" : "w-full h-9 gap-3 px-2.5"
        } ${
          active
            ? "nav-item-active bg-[var(--panel-soft)] text-[var(--panel-fg)] font-medium"
            : "text-[var(--panel-muted)] hover:text-[var(--panel-fg)] hover:bg-[var(--row-hover)]"
        }`}
      >
        <Icon
          size={17}
          strokeWidth={active ? 2 : 1.6}
          aria-hidden="true"
          className={`shrink-0 ${active ? "text-[var(--accent-color)]" : ""}`}
        />
        <span className={collapsed ? "sr-only" : "truncate"}>{label}</span>
        {extra.spoken ? <span className="sr-only">{`, ${extra.spoken}`}</span> : null}
        {extra.visual}
      </button>
    );
    return (
      <li key={item.page} className={collapsed ? "flex justify-center" : undefined}>
        {collapsed ? (
          <Tooltip content={label} side="right" delayMs={200}>
            {button}
          </Tooltip>
        ) : (
          button
        )}
      </li>
    );
  }

  const helpLabel = t("Help");
  const helpButton = (
    <button
      type="button"
      onClick={store.openFirstRunWalkthroughFromSettings}
      aria-label={t("Help — replay the walkthrough")}
      className={`flex items-center rounded-lg text-[13px] text-[var(--panel-muted)] hover:text-[var(--panel-fg)] hover:bg-[var(--row-hover)] transition-colors outline-none focus-visible:shadow-[0_0_0_2px_var(--input-focus)] ${
        collapsed ? "w-10 h-10 justify-center" : "w-full h-9 gap-3 px-2.5"
      }`}
    >
      <CircleHelp size={17} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
      <span className={collapsed ? "sr-only" : "truncate"}>{helpLabel}</span>
    </button>
  );

  // Reportar problema / sugerir ideia: abre o formulário do GitHub no navegador
  // (FeedbackDialog). Fica no rodapé, junto do Help, porque é sobre o app.
  const feedbackLabel = t("Send feedback");
  const feedbackButton = (
    <button
      type="button"
      onClick={() => setFeedbackOpen(true)}
      aria-label={feedbackLabel}
      className={`flex items-center rounded-lg text-[13px] text-[var(--panel-muted)] hover:text-[var(--panel-fg)] hover:bg-[var(--row-hover)] transition-colors outline-none focus-visible:shadow-[0_0_0_2px_var(--input-focus)] ${
        collapsed ? "w-10 h-10 justify-center" : "w-full h-9 gap-3 px-2.5"
      }`}
    >
      <MessageSquareText size={17} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
      <span className={collapsed ? "sr-only" : "truncate"}>{feedbackLabel}</span>
    </button>
  );

  const collapseLabel = collapsed ? t("Expand sidebar") : t("Collapse sidebar");
  const CollapseIcon = collapsed ? PanelLeftOpen : PanelLeftClose;
  const collapseButton = (
    <button
      type="button"
      onClick={toggleCollapsed}
      aria-label={collapseLabel}
      aria-expanded={!collapsed}
      className={`flex items-center justify-center rounded-lg text-[var(--panel-muted)] hover:text-[var(--panel-fg)] hover:bg-[var(--row-hover)] transition-colors outline-none focus-visible:shadow-[0_0_0_2px_var(--input-focus)] ${
        collapsed ? "w-10 h-9" : "w-8 h-8"
      }`}
    >
      <CollapseIcon size={16} strokeWidth={1.6} aria-hidden="true" />
    </button>
  );

  return (
    <nav
      aria-label={t("Main navigation")}
      data-collapsed={collapsed ? "true" : "false"}
      className={`theme-panel theme-border border-r shrink-0 flex flex-col min-h-0 transition-[width] duration-200 ease-out motion-reduce:transition-none ${
        collapsed ? "w-[60px]" : "w-[188px]"
      }`}
    >
      <div className={`flex-1 min-h-0 overflow-y-auto overflow-x-hidden py-3 ${collapsed ? "px-2.5" : "px-2.5"}`}>
        {navGroups().map((group, index) => (
          <ul key={index} className={`space-y-0.5 ${index > 0 ? "mt-2 pt-2 border-t theme-border" : ""}`}>
            {group.map(renderItem)}
          </ul>
        ))}
      </div>

      <div className="shrink-0 border-t theme-border py-2 px-2.5">
        <ul className="mb-0.5 space-y-0.5">
          <li className={collapsed ? "flex justify-center" : undefined}>
            {collapsed ? (
              <Tooltip content={feedbackLabel} side="right" delayMs={200}>
                {feedbackButton}
              </Tooltip>
            ) : (
              feedbackButton
            )}
          </li>
          {ENABLE_HELP_BUTTON ? (
            <li className={collapsed ? "flex justify-center" : undefined}>
              {collapsed ? (
                <Tooltip content={helpLabel} side="right" delayMs={200}>
                  {helpButton}
                </Tooltip>
              ) : (
                helpButton
              )}
            </li>
          ) : null}
        </ul>
        {/* Última linha: "What's new" ao lado do recolher (pedido do dono,
            08/10/2026). Recolhida, os dois empilham no centro; janela estreita
            (que já recolhe sozinha) não tem o botão de recolher. */}
        <div
          data-testid="nav-footer-actions"
          className={collapsed ? "flex flex-col items-center gap-1" : "flex items-center gap-1"}
        >
          <ul className={collapsed ? undefined : "flex-1 min-w-0"}>{FOOTER_ITEMS.map(renderItem)}</ul>
          {narrow ? null : collapsed ? (
            <Tooltip content={collapseLabel} side="right" delayMs={200}>
              {collapseButton}
            </Tooltip>
          ) : (
            collapseButton
          )}
        </div>
      </div>
      <FeedbackDialog
        open={feedbackOpen}
        onClose={() => setFeedbackOpen(false)}
        reportSource={() => ({ logs: store.launchLogs, accounts: store.accounts })}
      />
    </nav>
  );
}
