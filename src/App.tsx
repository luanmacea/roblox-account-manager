import { useEffect, useRef } from "react";
import { StoreProvider, useStore } from "./store";
import { PromptProvider } from "./hooks/usePrompt";
import { PasswordScreen } from "./components/layout/PasswordScreen";
import { EncryptionSetupScreen } from "./components/layout/EncryptionSetupScreen";
import { FirstRunWalkthrough } from "./components/layout/FirstRunWalkthrough";
import { ScreenTourHost } from "./components/tour/ScreenTour";
import { AppErrorBoundary } from "./components/layout/AppErrorBoundary";
import { TitleBar } from "./components/layout/TitleBar";
import { ModalWindowControls } from "./components/layout/ModalWindowControls";
import { UpdateBanner } from "./components/layout/UpdateBanner";
import { SafeModeBanner } from "./components/layout/SafeModeBanner";
import { VaultKeyBanner } from "./components/layout/VaultKeyBanner";
import { LockOverlay } from "./components/layout/LockOverlay";
import { useInactivityLock } from "./hooks/useInactivityLock";
import { inactivityLockActive, normalizeLockMinutes } from "./utils/inactivityLock";
import { Toolbar } from "./components/layout/Toolbar";
import { AccountList } from "./components/accounts/AccountList";
import { ContextMenu } from "./components/menus/ContextMenu";
import { DetailSidebar } from "./components/accounts/DetailSidebar";
import { BottomActionBar } from "./components/layout/BottomActionBar";
import { ChooseGameScreen } from "./components/ChooseGameScreen";
import { StatusBar } from "./components/layout/StatusBar";
import { NavSidebar } from "./components/layout/NavSidebar";
import { ServerListDialog } from "./components/server-list/ServerListDialog";
import { ImportDialog } from "./components/dialogs/ImportDialog";
import { AccountFieldsDialog } from "./components/dialogs/AccountFieldsDialog";
import { AccountUtilsDialog } from "./components/dialogs/AccountUtilsDialog";
import { MissingAssetsDialog } from "./components/dialogs/MissingAssetsDialog";
import { UpdateDialog } from "./components/dialogs/UpdateDialog";
import { AfkModeDialog } from "./components/afk-mode/AfkModeDialog";
import { GeneratorDialog } from "./components/dialogs/GeneratorDialog";
import { VersionsDialog } from "./components/dialogs/VersionsDialog";
import { DiagnosticsDialog } from "./components/dialogs/DiagnosticsDialog";
import { PresetsDialog } from "./components/presets/PresetsDialog";
import { IsolationProgressOverlay } from "./components/IsolationProgressOverlay";
import { SessionPage } from "./components/pages/SessionPage";
import { AfkPage } from "./components/pages/AfkPage";
import { AvatarsPage } from "./components/pages/AvatarsPage";
import { GroupsPage } from "./components/pages/GroupsPage";
import { ScriptsPage } from "./components/pages/ScriptsPage";
import { ThemePage } from "./components/pages/ThemePage";
import { NexusPage } from "./components/pages/NexusPage";
import { SettingsPage } from "./components/pages/SettingsPage";
import { ChangelogPage } from "./components/pages/ChangelogPage";
import { useTr } from "./i18n/text";
import { useUpdateHandoffToast } from "./hooks/useUpdateHandoffToast";
import { useBackdropClose } from "./hooks/useBackdropClose";
import { useUiScale } from "./hooks/useUiScale";
import { TONE_STYLES } from "./utils/toastTone";
import { isMultiRobloxCloseProcessError } from "./utils/robloxErrors";
import { ENABLE_NEXUS } from "./featureFlags";
import { startGameListsSync } from "./components/server-list/gameListsSync";

function AppContent() {
  const t = useTr();
  const store = useStore();
  const hasCheckedForUpdatesRef = useRef(false);
  const showCloseRobloxAction = isMultiRobloxCloseProcessError(store.error);
  // O log de lançamento é a única explicação passo a passo do que falhou, e ele
  // mora na aba Console da Choose Game. Só vale apontar para lá quando existe
  // log: fora do launch, a faixa mandaria o usuário para uma tela vazia.
  const hasLaunchLog = store.launchLogs.length > 0;
  // Páginas (Settings, Theme, Scripts...) não entram aqui: não cobrem a janela,
  // então os botões da barra de título continuam onde estão.
  const anyModalOpen =
    store.serverListOpen ||
    store.importDialogOpen ||
    store.accountFieldsOpen ||
    store.accountUtilsOpen ||
    !!store.missingAssets ||
    !!store.afkModeDialog ||
    store.generatorDialogOpen ||
    store.updateDialogOpen ||
    store.diagnosticsOpen ||
    store.presetsDialog !== null ||
    store.firstRunWalkthroughOpen ||
    !!store.modal;
  const page = store.activePage;
  const onAccounts = page === "accounts";
  const leavePage = () => store.setActivePage("accounts");
  const modalBackdropClose = useBackdropClose(store.closeModal);

  // Volta de uma atualização silenciosa: "Atualizado para vX" (ou o aviso de
  // que a instalação não terminou). Ver updateHandoff.ts.
  useUpdateHandoffToast(store.initialized && !store.needsPassword, store.addToast, t);

  // Tamanho da interface (Settings › General): zoom nativo do WebView, que no
  // automático encolhe a interface em janela pequena. Antes dos `return`
  // antecipados para valer também na tela de senha. Ver uiScale.ts.
  useUiScale(store.settings?.General?.InterfaceScale, store.settings !== null);

  // Trancar por inatividade (ideia 27): só com a opção ligada e senha do app.
  // Antes dos `return` antecipados, como todo hook.
  useInactivityLock(
    store.initialized &&
      !store.needsPassword &&
      inactivityLockActive(store.settings?.General?.LockOnInactivity, store.accountsEncrypted),
    normalizeLockMinutes(store.settings?.General?.LockAfterMinutes),
    store.appLocked,
    store.lockApp
  );

  useEffect(() => {
    if (!store.initialized || store.needsPassword || store.firstRunWalkthroughOpen) return;
    if (hasCheckedForUpdatesRef.current) return;
    hasCheckedForUpdatesRef.current = true;
    store.checkForUpdates();
  }, [store.checkForUpdates, store.firstRunWalkthroughOpen, store.initialized, store.needsPassword]);

  if (!store.initialized) {
    return (
      <div className="theme-app flex h-screen items-center justify-center">
        <div className="text-sm theme-muted">{t("Loading...")}</div>
      </div>
    );
  }

  // A faixa da chave do vault acompanha estas duas telas, e não é detalhe: elas
  // são exatamente as telas do momento de pânico. A tela de senha é onde cai quem
  // não conseguiu abrir o vault, e a de criptografia é onde o backend manda o
  // usuário ("See the warning on screen") quando a chave não pôde ser criada. Sem
  // isto, o aviso existia mas ficava atrás de um `return` antecipado — ponteiro
  // quebrado no instante em que ele mais importa.
  // `flex h-screen flex-col` + `min-h-0 flex-1 overflow-auto` porque `body` tem
  // `overflow: hidden`: como irmãs soltas num fragmento, a faixa somava altura em
  // cima de uma tela de 100vh e o rodapé saía da janela sem rolagem — e na tela de
  // criptografia o rodapé são os botões Continue/Cancel. As duas telas passaram a
  // `h-full`/`min-h-full` para o container ser o dono do viewport.
  if (store.needsPassword) {
    return (
      <div className="theme-app flex h-screen flex-col">
        <VaultKeyBanner />
        <div className="min-h-0 flex-1 overflow-auto">
          <PasswordScreen />
        </div>
      </div>
    );
  }

  if (store.encryptionSetupOpen) {
    return (
      <div className="theme-app flex h-screen flex-col">
        <VaultKeyBanner />
        <div className="min-h-0 flex-1 overflow-auto">
          <EncryptionSetupScreen />
        </div>
      </div>
    );
  }

  return (
    <>
    {/* Trancado: a janela inteira fica `inert` (nem foco nem clique) e a tela de
        senha vem por cima — nada é desmontado, então nada que roda para. */}
    {store.appLocked && <LockOverlay />}
    <div className="theme-app flex h-screen flex-col" inert={store.appLocked || undefined}>
      <ModalWindowControls visible={anyModalOpen} />
      <TitleBar controlsHidden={anyModalOpen} />
      <UpdateBanner />
      <SafeModeBanner />
      <VaultKeyBanner />

      {/* Barra lateral + área principal. A barra lateral escolhe a página
          (`activePage`); a página de contas é a de sempre — Toolbar, lista,
          painel da conta, barra de ações e a Choose Game por cima. As outras
          ficam montadas o tempo todo (como os modais ficavam) e só aparecem
          quando ativas: Scripts mantém os workers em execução, Avatars o
          ouvinte do lote. Ver docs/features/ui-layout.md. */}
      <div className="flex flex-1 min-h-0">
        <NavSidebar />
        <main className="flex flex-1 min-w-0 min-h-0 flex-col">
          {onAccounts && <Toolbar />}

          {store.error && (
            <div className="mx-4 mt-2 rounded-lg bg-red-500/10 border border-red-500/20 px-4 py-2 text-sm text-red-400 flex flex-wrap items-start justify-between gap-y-1 animate-fade-in">
              {/* O `truncate` cortava justamente o fim da mensagem, que é onde o
                  backend explica o erro. Agora ela quebra linha; o teto de altura
                  com rolagem impede que um erro enorme vire painel. */}
              <span className="min-w-0 flex-1 whitespace-pre-wrap break-words max-h-24 overflow-y-auto">
                {store.error}
              </span>
              <div className="ml-2 flex items-center gap-2 shrink-0">
                {showCloseRobloxAction && (
                  <button
                    onClick={() => store.killAllRobloxProcesses()}
                    className="px-2 py-1 rounded-md bg-red-500/20 border border-red-500/30 text-red-300 hover:bg-red-500/30 transition-colors animate-pulse"
                  >
                    {t("Close Roblox")}
                  </button>
                )}
                {/* O detalhe do que falhou no launch só existe no log, em outra
                    tela. Enquanto houver log, a faixa diz onde ele está e abre a
                    Choose Game — a aba Console é escolhida lá dentro. */}
                {hasLaunchLog && !(onAccounts && store.chooseGameOpen) && (
                  <button
                    onClick={() => {
                      store.setActivePage("accounts");
                      store.setChooseGameOpen(true);
                    }}
                    className="px-2 py-1 rounded-md bg-red-500/10 border border-red-500/30 text-red-300 hover:bg-red-500/20 transition-colors"
                  >
                    {t("Open launch log")}
                  </button>
                )}
                {/* "O launch não faz nada" (ideia 16): a checagem só lê, então
                    pode ficar a um clique de qualquer erro de launch. */}
                {hasLaunchLog && (
                  <button
                    onClick={() => store.setDiagnosticsOpen(true)}
                    className="px-2 py-1 rounded-md bg-red-500/10 border border-red-500/30 text-red-300 hover:bg-red-500/20 transition-colors"
                  >
                    {t("Check what's wrong")}
                  </button>
                )}
                <button
                  onClick={() => store.setError(null)}
                  className="text-red-500/60 hover:text-red-400 transition-colors"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <path d="M18 6 6 18M6 6l12 12" />
                  </svg>
                </button>
              </div>
              {hasLaunchLog && (
                <p className="basis-full text-xs text-red-400/70">
                  {t("Step-by-step details of the last launch are in Choose Game › Console.")}
                </p>
              )}
            </div>
          )}

          {onAccounts && (
            <div className="flex flex-col flex-1 min-h-0">
              {store.chooseGameOpen ? (
                <ChooseGameScreen />
              ) : (
                <div className="flex flex-1 min-h-0">
                  <AccountList />
                  {/* O painel é de uma conta só. Quem garante que o botão da Toolbar
                      não promete um painel que não vem é o `disabled` de lá, que usa
                      exatamente esta condição — mudou aqui, muda lá. */}
                  {store.sidebarOpen && store.selectedAccounts.length === 1 && <DetailSidebar />}
                </div>
              )}
              {!store.chooseGameOpen && store.selectedIds.size > 0 && <BottomActionBar />}
            </div>
          )}

          <SessionPage active={page === "session"} onLeave={leavePage} />
          <AfkPage active={page === "afk"} onLeave={leavePage} />
          <AvatarsPage active={page === "avatars"} onLeave={leavePage} />
          <GroupsPage active={page === "groups"} onLeave={leavePage} />
          <ScriptsPage active={page === "scripts"} onLeave={leavePage} />
          <ThemePage active={page === "theme"} onLeave={leavePage} />
          {ENABLE_NEXUS && <NexusPage active={page === "nexus"} onLeave={leavePage} />}
          <SettingsPage
            active={page === "settings"}
            onLeave={leavePage}
            onSettingsChanged={store.reloadSettings}
            onRequestEncryptionSetup={store.openEncryptionSetupFromSettings}
          />
          {/* Só pede as versões ao GitHub quando aberta (ver ChangelogPage). */}
          <ChangelogPage active={page === "changelog"} onLeave={leavePage} />
        </main>
      </div>

      <StatusBar />

      <ContextMenu />

      {/* O tom vem pronto da store (`addToast` calcula uma vez) e a cor sai do
          mesmo mapa do Console de launch — antes a fila era toda cinza e um
          erro tinha exatamente a cara de um sucesso. A chave é o `id`: com
          `key={i}` a saída do primeiro toast remontava os que sobravam. */}
      {store.toasts.length > 0 && (
        <div className="fixed bottom-10 right-4 z-[60] flex flex-col gap-1.5">
          {store.toasts.map((toast) => (
            <div
              key={toast.id}
              className={`theme-panel theme-border backdrop-blur-lg px-4 py-2 rounded-lg text-xs shadow-xl animate-toast flex items-center gap-2 ${TONE_STYLES[toast.tone].text}`}
            >
              <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${TONE_STYLES[toast.tone].dot}`} />
              <span>{toast.message}</span>
            </div>
          ))}
        </div>
      )}

      <ServerListDialog
        open={store.serverListOpen}
        onClose={() => store.setServerListOpen(false)}
      />

      <ImportDialog
        open={store.importDialogOpen}
        onClose={() => store.setImportDialogOpen(false)}
        defaultTab={store.importDialogTab}
      />

      <AccountFieldsDialog
        open={store.accountFieldsOpen}
        onClose={() => store.setAccountFieldsOpen(false)}
      />

      <AccountUtilsDialog
        open={store.accountUtilsOpen}
        onClose={() => store.setAccountUtilsOpen(false)}
      />

      <MissingAssetsDialog />

      <AfkModeDialog />

      <GeneratorDialog
        open={store.generatorDialogOpen}
        initialTab={store.generatorDialogTab}
        onClose={() => store.setGeneratorDialogOpen(false)}
      />

      <VersionsDialog
        open={store.versionsDialogOpen}
        onClose={() => store.setVersionsDialogOpen(false)}
      />

      <DiagnosticsDialog open={store.diagnosticsOpen} onClose={() => store.setDiagnosticsOpen(false)} />

      <PresetsDialog />

      <IsolationProgressOverlay />

      <UpdateDialog />

      {store.firstRunWalkthroughOpen && <FirstRunWalkthrough />}

      {/* Tutorial de uma tela, aberto pelo botão Tutorial dela (nunca sozinho). */}
      <ScreenTourHost />

      {store.modal && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in"
          {...modalBackdropClose}
        >
          <div
            className="theme-panel theme-border rounded-xl p-5 max-w-2xl w-full mx-4 max-h-[80vh] flex flex-col shadow-2xl animate-scale-in"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-sm font-semibold text-[var(--panel-fg)]">{store.modal.title}</h3>
              <button
                onClick={store.closeModal}
                className="theme-muted hover:opacity-100 transition-opacity"
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </div>
            <pre className="theme-input text-xs font-mono rounded-lg p-4 overflow-auto flex-1">
              {store.modal.content}
            </pre>
          </div>
        </div>
      )}
    </div>
    </>
  );
}

function App() {
  // Favoritos e recentes: junta o RAMGameLists.json do backend com o cache do
  // localStorage, e de novo a cada restauração de backup. Não depende da senha
  // (as listas não ficam no vault). Ver server-list/gameListsSync.ts.
  useEffect(() => startGameListsSync(), []);

  // O boundary fica **fora** da store: um throw dentro do provider também tem
  // que cair nele, senão a tela fica branca do mesmo jeito.
  return (
    <AppErrorBoundary>
      <StoreProvider>
        <PromptProvider>
          <AppContent />
        </PromptProvider>
      </StoreProvider>
    </AppErrorBoundary>
  );
}

export default App;
