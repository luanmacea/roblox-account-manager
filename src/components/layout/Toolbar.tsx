import { useState, useRef, useEffect } from "react";
import { useStore } from "../../store";
import { usePrompt } from "../../hooks/usePrompt";
import { Tooltip } from "../ui/Tooltip";
import { tr, useTr } from "../../i18n/text";
import { quickAddAccount } from "../../utils/quickAdd";
import { ENABLE_ACCOUNT_GENERATOR } from "../../featureFlags";
import { TourButton } from "../tour/TourButton";
import { Search, X, SquareX, SquareCheckBig, PanelRight, Plus, ChevronDown, Globe, KeyRound, File, FileText, Sparkles, Package, UserPlus, Bookmark, Smartphone } from "lucide-react";

/**
 * Barra de cima da página de contas: filtro, selecionar tudo, nomes, painel
 * lateral e o menu Add. Só o que age sobre a lista. Sessão, AFK Mode, Avatars,
 * Scripts, Theme, Nexus, Settings e Ajuda saíram daqui — eram ícones que só
 * diziam o que faziam com o mouse parado em cima — e viraram itens com nome na
 * barra lateral (NavSidebar), cada um com a sua página.
 */
export function Toolbar() {
  const t = useTr();
  const store = useStore();
  const prompt = usePrompt();
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const addRef = useRef<HTMLDivElement>(null);
  const activeToggleStyle = "theme-accent theme-accent-bg theme-accent-border";

  // O painel lateral (DetailSidebar) só existe para uma conta — ver App.tsx.
  // Com 0 ou 2+ selecionadas o botão ficava aceso e nada abria; aqui ele fica
  // desabilitado e o tooltip diz o que falta, em vez de fingir que ligou.
  const panelAvailable = store.selectedAccounts.length === 1;
  const panelTooltip = panelAvailable
    ? store.sidebarOpen
      ? t("Hide panel")
      : t("Show panel")
    : store.selectedAccounts.length === 0
      ? t("Select an account to show its panel")
      : t("The panel shows one account at a time");

  // Tooltip aparece só depois de 350 ms de mouse parado: leitor de tela e
  // teste ficavam sem nome nenhum nos botões de ícone. O mesmo texto vira
  // `aria-label`, então os dois caminhos dizem a mesma coisa.
  const selectAllLabel =
    store.selectedIds.size > 0
      ? t("Deselect all ({{count}})", { count: store.selectedIds.size })
      : t("Select all");

  // O rótulo `Names`/`Hidden` não dizia o que o botão faz. Agora o texto conta
  // o estado ("Names shown"/"Names hidden") e o tooltip conta a ação.
  const namesTooltip = store.hideUsernames
    ? t("Show the usernames in the list again")
    : t("Mask the usernames in the list (for screenshots)");

  useEffect(() => {
    if (!addMenuOpen) return;
    function handleClick(e: MouseEvent) {
      if (addRef.current && !addRef.current.contains(e.target as Node)) {
        setAddMenuOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [addMenuOpen]);

  function handleBrowserLogin() {
    setAddMenuOpen(false);
    store.openLoginBrowser();
  }

  function handleQuickLogin() {
    setAddMenuOpen(false);
    store.setQuickLoginOpen(true);
  }

  function handleUserPassLogin() {
    setAddMenuOpen(false);
    store.setImportDialogTab("userpass");
    store.setImportDialogOpen(true);
  }

  function handleImportCookie() {
    setAddMenuOpen(false);
    store.setImportDialogTab("cookie");
    store.setImportDialogOpen(true);
  }

  function handleImportOldAccountData() {
    setAddMenuOpen(false);
    store.setImportDialogTab("legacy");
    store.setImportDialogOpen(true);
  }

  function handleOpenGenerator() {
    setAddMenuOpen(false);
    store.openGeneratorDialog("provider");
  }

  function handleOpenSignup() {
    setAddMenuOpen(false);
    store.openGeneratorDialog("signup");
  }

  function handleOpenVersions() {
    setAddMenuOpen(false);
    store.setVersionsDialogOpen(true);
  }

  async function handleQuickAdd() {
    setAddMenuOpen(false);
    // "Cookie or username" não dizia que cookie é esse, onde ele está nem o que
    // ele entrega — e ele entra como a conta inteira (api/auth.rs:45). Mesmo
    // texto do Quick Add do AddAccountDialog.
    const input = await prompt(
      tr(
        "Paste a .ROBLOSECURITY cookie — it signs in as that account, and you find it in your browser's DevTools › Application › Cookies on roblox.com — or type a username to add it without a session."
      )
    );
    if (!input?.trim()) return;
    // Cookie, `usuario:senha:cookie` ou nome de usuário: quem decide é o leitor
    // do import, o mesmo nas duas portas (ver `quickAddAccount`).
    await quickAddAccount(input, store);
  }

  return (
    <div className="theme-panel theme-border flex items-center gap-3 px-4 py-2 border-b shrink-0">
      <div className="relative flex-1 max-w-xs">
        <Search size={15} strokeWidth={2} className="absolute left-3 top-1/2 -translate-y-1/2 theme-muted" />
        <input
          type="text"
          value={store.searchQuery}
          onChange={(e) => store.setSearchQuery(e.target.value)}
          onClick={(e) => e.stopPropagation()}
          placeholder={t("Filter accounts...")}
          autoComplete="off"
          spellCheck={false}
          className="theme-input w-full pl-9 pr-3 py-1.5 rounded-lg text-sm transition-colors"
        />
        {store.searchQuery && (
          <Tooltip content={t("Clear search")} side="bottom">
            <button
              onClick={() => store.setSearchQuery("")}
              aria-label={t("Clear search")}
              className="absolute right-2 top-1/2 -translate-y-1/2 theme-muted hover:opacity-100"
            >
              <X size={14} strokeWidth={2} />
            </button>
          </Tooltip>
        )}
      </div>

      <div className="flex items-center gap-1.5 ml-auto">
        <Tooltip content={selectAllLabel} side="bottom">
          <button
            onClick={() => store.toggleSelectAll()}
            aria-label={selectAllLabel}
            className={`px-2.5 py-1.5 text-xs rounded-lg border transition-colors ${
              store.selectedIds.size > 0
                ? activeToggleStyle
                : "theme-btn-ghost"
            }`}
          >
            {store.selectedIds.size > 0 ? (
              <SquareX size={14} strokeWidth={2} />
            ) : (
              <SquareCheckBig size={14} strokeWidth={2} />
            )}
          </button>
        </Tooltip>

        <Tooltip content={namesTooltip} side="bottom">
          <button
            onClick={() => store.setHideUsernames(!store.hideUsernames)}
            data-tour="toolbar-names"
            className={`px-2.5 py-1.5 text-xs rounded-lg border transition-colors ${
              store.hideUsernames
                ? activeToggleStyle
                : "theme-btn-ghost"
            }`}
          >
            {store.hideUsernames ? t("Names hidden") : t("Names shown")}
          </button>
        </Tooltip>

        <Tooltip content={panelTooltip} side="bottom">
          <button
            onClick={() => store.setSidebarOpen(!store.sidebarOpen)}
            disabled={!panelAvailable}
            aria-label={panelTooltip}
            data-tour="toolbar-panel"
            className={`p-1.5 rounded-lg border transition-colors ${
              !panelAvailable
                ? "theme-btn-ghost opacity-40 cursor-not-allowed"
                : store.sidebarOpen
                ? activeToggleStyle
                : "theme-btn-ghost"
            }`}
          >
            <PanelRight size={16} strokeWidth={1.5} />
          </button>
        </Tooltip>

        <div className="w-px h-5 mx-1 bg-[var(--border-color)]" />

        {/* Tutorial desta tela: opcional, nunca abre sozinho (components/tour/).
            Com a Choose Game aberta, o Tutorial que vale é o dela. */}
        {!store.chooseGameOpen && <TourButton tour="accounts" />}

        {/* Presets de launch (ideia 13): contas → jogo salvos, com horário. */}
        <Tooltip content={t("Saved launches: these accounts into this game, in one click")} side="bottom">
          <button
            onClick={() => store.openPresetsDialog()}
            data-tour="toolbar-presets"
            className="theme-btn-ghost flex items-center gap-1.5 px-2.5 py-1.5 text-xs rounded-lg border transition-colors"
          >
            <Bookmark size={13} strokeWidth={2} />
            {t("Presets")}
          </button>
        </Tooltip>

        <div ref={addRef} className="relative">
          <button
            onClick={() => setAddMenuOpen(!addMenuOpen)}
            data-tour="toolbar-add"
            className="theme-btn flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium transition-colors"
          >
            <Plus size={14} strokeWidth={2.5} />
            {t("Add")}
            <ChevronDown size={10} strokeWidth={2.5} />
          </button>
          {addMenuOpen && (
            // Teto: na janela mínima (750x450) o menu passava da borda de baixo
            // e o último item ficava cortado; com ele, o menu rola por dentro.
            <div className="theme-panel theme-border absolute right-0 top-full mt-1.5 w-64 max-h-[calc(100vh-96px)] overflow-y-auto border rounded-xl shadow-2xl z-50 animate-scale-in py-1">
              <button
                onClick={handleQuickAdd}
                className="flex items-center gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <Plus size={14} strokeWidth={1.5} className="theme-muted" />
                {t("Quick Add")}
              </button>
              <button
                onClick={handleBrowserLogin}
                data-tour="add-browser-login"
                className="flex items-center gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <Globe size={14} strokeWidth={1.5} className="theme-muted" />
                {t("Browser Login")}
              </button>
              <button
                onClick={handleUserPassLogin}
                className="flex items-center gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <KeyRound size={14} strokeWidth={1.5} className="theme-muted" />
                {t("User:Pass Login")}
              </button>
              {/* Ideia 12 — mesmo item do AddAccountDialog. */}
              <button
                onClick={handleQuickLogin}
                className="flex items-center gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <Smartphone size={14} strokeWidth={1.5} className="theme-muted" />
                {t("Quick Login")}
              </button>
              <div className="my-1 border-t theme-border" />
              <button
                onClick={handleImportCookie}
                className="flex items-center gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <File size={14} strokeWidth={1.5} className="theme-muted" />
                {t("Import Cookie")}
              </button>
              <button
                onClick={handleImportOldAccountData}
                className="flex items-center gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <FileText size={14} strokeWidth={1.5} className="theme-muted" />
                {t("Import Old Account Data")}
              </button>
              <div className="my-1 border-t theme-border" />
              {/*
                As duas entradas que trazem conta nova são bem diferentes e a
                tela não dizia nada: uma cria de graça no navegador embutido
                (a pessoa resolve o CAPTCHA), a outra compra conta pronta de um
                serviço pago de terceiro. Ver docs/features/account-creation.md.
                O mesmo texto aparece no AddAccountDialog — as duas portas têm
                que dizer a mesma coisa.
              */}
              <button
                onClick={handleOpenSignup}
                className="flex items-start gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <UserPlus size={14} strokeWidth={1.5} className="theme-muted mt-0.5 shrink-0" />
                <span className="min-w-0">
                  {t("Create Accounts")}
                  <span className="block text-[12px] theme-muted leading-snug">
                    {t("Free — the app fills Roblox's signup form; you solve the CAPTCHA")}
                  </span>
                </span>
              </button>
              {/* O gerador pago está desligado por padrão — ver ENABLE_ACCOUNT_GENERATOR. */}
              {ENABLE_ACCOUNT_GENERATOR && (
                <button
                  onClick={handleOpenGenerator}
                  className="flex items-start gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
                >
                  <Sparkles size={14} strokeWidth={1.5} className="theme-muted mt-0.5 shrink-0" />
                  <span className="min-w-0">
                    {t("Account Generator")}
                    <span className="block text-[12px] theme-muted leading-snug">
                      {t("Paid — buys ready-made accounts from BloxGen (third party, API key)")}
                    </span>
                  </span>
                </button>
              )}
              <button
                onClick={handleOpenVersions}
                className="flex items-center gap-2.5 w-full px-3.5 py-2 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left"
              >
                <Package size={14} strokeWidth={1.5} className="theme-muted" />
                {t("Roblox Versions")}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
