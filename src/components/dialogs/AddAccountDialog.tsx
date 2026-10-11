import { File, FileText, Globe, KeyRound, Package, Plus, Smartphone, Sparkles, UserPlus, X } from "lucide-react";
import { useStore } from "../../store";
import { usePrompt } from "../../hooks/usePrompt";
import { useBackdropClose } from "../../hooks/useBackdropClose";
import { tr, useTr } from "../../i18n/text";
import { quickAddAccount } from "../../utils/quickAdd";
import { ENABLE_ACCOUNT_GENERATOR } from "../../featureFlags";

interface AddAccountDialogProps {
  open: boolean;
  onClose: () => void;
}

export function AddAccountDialog({ open, onClose }: AddAccountDialogProps) {
  const t = useTr();
  const store = useStore();
  const prompt = usePrompt();
  const backdropClose = useBackdropClose(onClose);

  if (!open) return null;

  async function handleQuickAdd() {
    onClose();
    // Mesmo pedido do Quick Add da toolbar: o cookie `.ROBLOSECURITY` entra
    // como a conta inteira (api/auth.rs:45), então a tela tem que dizer o que é
    // e onde ele fica.
    const input = await prompt(
      tr(
        "Paste a .ROBLOSECURITY cookie — it signs in as that account, and you find it in your browser's DevTools › Application › Cookies on roblox.com — or type a username to add it without a session."
      )
    );
    if (!input?.trim()) return;
    // O mesmo Quick Add da toolbar, numa função só (`quickAddAccount`): as duas
    // cópias já divergiram uma vez.
    await quickAddAccount(input, store);
  }

  async function handleBrowserLogin() {
    onClose();
    await store.openLoginBrowser();
  }

  function handleQuickLogin() {
    onClose();
    store.setQuickLoginOpen(true);
  }

  function handleUserPassLogin() {
    onClose();
    store.setImportDialogTab("userpass");
    store.setImportDialogOpen(true);
  }

  function handleImportCookie() {
    onClose();
    store.setImportDialogTab("cookie");
    store.setImportDialogOpen(true);
  }

  function handleImportOldAccountData() {
    onClose();
    store.setImportDialogTab("legacy");
    store.setImportDialogOpen(true);
  }

  function handleOpenSignup() {
    onClose();
    store.openGeneratorDialog("signup");
  }

  function handleOpenGenerator() {
    onClose();
    store.openGeneratorDialog("provider");
  }

  function handleOpenVersions() {
    onClose();
    store.setVersionsDialogOpen(true);
  }

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in"
      {...backdropClose}
    >
      <div
        className="theme-modal-scope theme-panel theme-border bg-zinc-900 border border-zinc-800/80 rounded-2xl shadow-2xl w-[420px] max-w-[calc(100vw-2rem)] max-h-[calc(100vh-24px)] overflow-y-auto animate-scale-in"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800/70">
          <h2 className="text-sm font-semibold text-zinc-100">{t("Add Account")}</h2>
          <button
            onClick={onClose}
            className="theme-muted hover:opacity-100 transition-opacity"
            aria-label={t("Close")}
          >
            <X size={16} strokeWidth={2} />
          </button>
        </div>

        {/* Com as 8 entradas a lista passa da tela em janela baixa: ela rola. */}
        <div className="px-4 py-3 max-h-[70vh] overflow-y-auto">
          <p className="text-xs text-zinc-400 mb-3">{t("Choose how to add an account")}</p>

          {/*
            Este diálogo é a única porta de quem tem zero conta (estado vazio da
            AccountList) e oferecia 4 das 8 entradas do menu `Add` da toolbar —
            faltavam justamente as duas que criam conta nova. Ao mexer aqui,
            mexa também no menu da toolbar: as duas portas oferecem o mesmo.
          */}
          <div className="space-y-1.5">
            <button
              onClick={handleQuickAdd}
              className="flex items-center gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <Plus size={15} strokeWidth={1.75} className="theme-muted" />
              {t("Quick Add")}
            </button>

            <button
              onClick={handleBrowserLogin}
              className="flex items-center gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <Globe size={15} strokeWidth={1.75} className="theme-muted" />
              {t("Browser Login")}
            </button>

            <button
              onClick={handleUserPassLogin}
              className="flex items-center gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <KeyRound size={15} strokeWidth={1.75} className="theme-muted" />
              {t("User:Pass Login")}
            </button>

            {/* Ideia 12: aprova um código num aparelho já logado — sem colar
                cookie nem digitar senha aqui. Mesmo item no menu Add da toolbar. */}
            <button
              onClick={handleQuickLogin}
              className="flex items-start gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <Smartphone size={15} strokeWidth={1.75} className="theme-muted mt-0.5 shrink-0" />
              <span className="min-w-0">
                {t("Quick Login")}
                <span className="block text-[12px] theme-muted leading-snug">
                  {t("Approve a code on a phone or PC already signed in")}
                </span>
              </span>
            </button>

            <div className="my-1 border-t border-zinc-800/70" />

            <button
              onClick={handleImportCookie}
              className="flex items-center gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <File size={15} strokeWidth={1.75} className="theme-muted" />
              {t("Import Cookie")}
            </button>

            <button
              onClick={handleImportOldAccountData}
              className="flex items-center gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <FileText size={15} strokeWidth={1.75} className="theme-muted" />
              {t("Import Old Account Data")}
            </button>

            <div className="my-1 border-t border-zinc-800/70" />

            <p className="px-3 pt-1 text-[12px] text-zinc-500">{t("No account yet? Get a new one")}</p>

            {/*
              Mesmo texto do menu da toolbar, de propósito: uma cria de graça no
              navegador embutido (a pessoa resolve o CAPTCHA), a outra compra
              conta pronta de um serviço pago de terceiro.
              Ver docs/features/account-creation.md.
            */}
            <button
              onClick={handleOpenSignup}
              className="flex items-start gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <UserPlus size={15} strokeWidth={1.75} className="theme-muted mt-0.5 shrink-0" />
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
                className="flex items-start gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
              >
                <Sparkles size={15} strokeWidth={1.75} className="theme-muted mt-0.5 shrink-0" />
                <span className="min-w-0">
                  {t("Account Generator")}
                  <span className="block text-[12px] theme-muted leading-snug">
                    {t("Paid — buys ready-made accounts from BloxGen (third party, API key)")}
                  </span>
                </span>
              </button>
            )}

            <div className="my-1 border-t border-zinc-800/70" />

            <button
              onClick={handleOpenVersions}
              className="flex items-center gap-2.5 w-full px-3 py-2.5 text-sm text-[var(--panel-fg)] hover:bg-[var(--panel-soft)] text-left rounded-lg transition-colors"
            >
              <Package size={15} strokeWidth={1.75} className="theme-muted" />
              {t("Roblox Versions")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
