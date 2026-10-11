import {
  createContext,
  useContext,
  useState,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Account,
  ThemeData,
  ThumbnailData,
  ParsedGroup,
  PlatformCapabilities,
  FriendLinkState,
  LaunchQueuePayload,
  ModerationStatus,
  ServerPreference,
  UnidentifiedClient,
  ClientDrop,
  ClientHealth,
  ClientMemory,
  AutoReconnectEntry,
  AutoReconnectPayload,
  VaultKeyWarning,
  LaunchPreset,
  LaunchPresetEvent,
  LaunchPresetView,
} from "./types";
import { playAfkBeep } from "./utils/afkBeep";
import type { AfkMode } from "./afkClickPoint";
import {
  orderGroupKeys,
  parseGroupName,
  parseGroupOrder,
  serializeGroupOrder,
  VAULT_KEY_WARNING_EVENT,
} from "./types";

/**
 * Valor guardado em `General.ServerPreference` → preferência válida.
 * Qualquer coisa desconhecida (ou um INI antigo) volta para `default`, que é o
 * comportamento de sempre: Job ID vazio e o Roblox escolhe o servidor.
 */
/** Páginas varridas por padrão na aba Servers (100 servidores cada). */
export const DEFAULT_SERVER_SCAN_PAGES = 30;
/** Teto: o backend aplica o mesmo, uma varredura sem fim martelaria a API. */
export const MAX_SERVER_SCAN_PAGES = 500;

/**
 * Valor guardado em `General.ServerScanPages` → número de páginas válido.
 * Vazio, zero ou lixo voltam para o padrão.
 */
export function normalizeServerScanPages(value: number | undefined): number {
  if (!Number.isFinite(value) || !value || value < 1) return DEFAULT_SERVER_SCAN_PAGES;
  return Math.min(Math.floor(value as number), MAX_SERVER_SCAN_PAGES);
}

/** Abas do diálogo do gerador de contas. */
export type GeneratorDialogTab = "provider" | "signup";

/** Abas do Modo AFK: cliques AFK (tecla/clique), Gravações e Auto Rejoin (ciclo de rejoin). */
export type AfkModeTab = "rejoin" | "clicks" | "recordings";

/**
 * O que está aberto no Modo AFK — `null` com a janela fechada.
 *
 * `targetUserIds` + `adoptRunning` vêm do "Em jogo" do Painel de Sessão: as
 * contas que já estão jogando, que o Auto Rejoin **adota** sem fechar nem
 * relançar. Sem `targetUserIds` o Auto Rejoin trabalha com as contas
 * selecionadas na lista, como sempre.
 */
export interface AfkModeDialogState {
  tab: AfkModeTab;
  targetUserIds?: number[];
  adoptRunning?: boolean;
  /** Jogo escolhido na abertura (clique direito num jogo). Vence o rascunho. */
  placeId?: string | null;
}

/** O que a tela do Modo AFK escolheu antes de adotar as contas em jogo. */
export interface AdoptBottingOptions {
  /** Place mostrado na tela (detectado ou digitado); sem ele, pergunta a presença. */
  placeId?: number;
  intervalMinutes?: number;
  launchDelaySeconds?: number;
  playerGraceMinutes?: number;
  playerUserIds?: number[];
}
/**
 * Páginas da área principal, escolhidas pela barra lateral (NavSidebar). Cada
 * uma ocupa a janela inteira à direita da barra; `accounts` é a lista de contas
 * (com a Choose Game por cima quando aberta). Ver docs/features/ui-layout.md.
 */
export type AppPage =
  | "accounts"
  | "session"
  | "afk"
  | "avatars"
  | "groups"
  | "scripts"
  | "theme"
  | "nexus"
  | "settings"
  | "changelog";

export function normalizeServerPreference(value: string | undefined): ServerPreference {
  switch ((value || "").trim().toLowerCase()) {
    case "none":
    case "off":
      return "none";
    case "random":
      return "random";
    case "emptiest":
      return "emptiest";
    case "fullest":
      return "fullest";
    // Desconhecido e o nome antigo ("default") caem no padrão novo.
    default:
      return "bestfit";
  }
}
import { applyThemeCssVariables, normalizeTheme, DEFAULT_THEME } from "./theme";
import i18n, { normalizeLanguage } from "./i18n";
import { REPO_URL } from "./repo";
import { isLaunchAlreadyActiveError } from "./utils/robloxErrors";
import { toneFromMessage, type ToastTone } from "./utils/toastTone";
import { tr } from "./i18n/text";
import { accountLabel, maskAccountName } from "./utils/accountName";
import { clientHealthLabel } from "./utils/clientHealth";
import { autoReconnectLabel } from "./utils/autoReconnect";
import { accountCheckSummaryText, type AccountCheckSummary } from "./utils/accountCheck";
import {
  type UpdaterReleaseChannel,
  type UpdaterFeatureChannel,
  normalizeUpdaterReleaseChannel,
  normalizeUpdaterFeatureChannel,
  getUpdaterSkipVersionKey,
} from "./updaterChannels";
import { addRecentJob, recordRecentGame } from "./components/server-list/types";

interface PresenceEntry {
  userId?: number;
  userPresenceType?: number;
  user_id?: number;
  user_presence_type?: number;
}

interface RunningInstanceEntry {
  userId?: number;
  user_id?: number;
  pid?: number;
  /** Aberto fora do app (pelo site) e reconhecido depois. */
  adopted?: boolean;
  /** Queda lida do log (`commands/client_health.rs`). */
  health?: ClientHealth | null;
  /** Memória e limite (`commands/memory_ceiling.rs`); só cliente do app. */
  memory?: ClientMemory | null;
}

interface OptimizationWarningPayload {
  pid?: number;
  message?: string;
}

interface LaunchProgressState {
  mode: "single" | "multi";
  current: number;
  total: number;
  userId: number | null;
}

export type LaunchLogLevel = "info" | "success" | "warn" | "error";

export interface LaunchLogEntry {
  id: number;
  userId: number | null;
  level: LaunchLogLevel;
  step: string;
  message: string;
  ts: number;
}

type ActionStatusTone = ToastTone;

interface ActionStatusState {
  message: string;
  tone: ActionStatusTone;
  at: number;
}

/** Estado do botão "Download"/"Reinstall" de Settings > General. */
export interface BrowserDownloadState {
  active: boolean;
  stage: "resolving" | "downloading" | "extracting" | "ready" | "error";
  percent: number | null;
  error: string | null;
}

/**
 * Um toast na fila. O tom é calculado uma vez, em `addToast`, e viaja junto —
 * quem desenha (`App`) não precisa reinspecionar o texto. O `id` é a chave
 * estável da lista: com `key={i}` a saída do primeiro toast renumerava os que
 * sobravam e reiniciava a animação de entrada deles.
 */
export interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
}

export interface BottingAccountStatus {
  userId: number;
  isPlayer: boolean;
  disconnected: boolean;
  phase: string;
  retryCount: number;
  nextRestartAtMs: number | null;
  playerGraceUntilMs: number | null;
  lastError: string | null;
}

export interface BottingStatus {
  active: boolean;
  startedAtMs: number | null;
  placeId: number;
  jobId: string;
  launchData: string;
  intervalMinutes: number;
  launchDelaySeconds: number;
  playerGraceMinutes: number;
  playerUserIds: number[];
  userIds: number[];
  accounts: BottingAccountStatus[];
}

export interface BottingStartConfig {
  userIds: number[];
  placeId: number;
  jobId: string;
  launchData: string;
  playerUserIds: number[];
  intervalMinutes: number;
  launchDelaySeconds: number;
  playerGraceMinutes: number;
  /**
   * A sessão nasce sobre contas que **já estão em jogo**: elas não são
   * fechadas nem relançadas na primeira passagem, só entram no ciclo.
   */
  adoptRunning?: boolean;
}

/** Uma conta que está no AFK mode, com o relógio dela. */
export interface AfkAccountStatus {
  userId: number;
  /** Último envio, ou a entrada no modo enquanto não houve envio. */
  lastSendAtMs: number;
  nextSendAtMs: number;
  sends: number;
  lastError: string | null;
  /**
   * `noWindow`, `focusDenied`, `keyRefused`, `clickRefused` ou `internal`. A
   * tela escolhe a frase traduzida por aqui, em vez de casar o texto em inglês
   * do backend.
   */
  lastErrorCode: string | null;
}

export interface AfkStatus {
  active: boolean;
  startedAtMs: number | null;
  /** Segundos entre dois envios da mesma conta, contados do fim do ciclo. */
  intervalSeconds: number;
  key: string;
  mode: AfkMode;
  /** Ponto padrão do modo clique, em % da janela. */
  clickX: number;
  clickY: number;
  accounts: AfkAccountStatus[];
  /**
   * O ciclo está na hora mas espera: há uma janela em tela cheia de outro
   * programa na frente (`Afk.WaitForFullscreen`). O backend sempre manda;
   * opcional só para os retratos antigos dos testes.
   */
  waitingFullscreen?: boolean;
}

export interface AfkStartConfig {
  userIds: number[];
  /** Minutos e segundos da tela somados; o backend trava em 5..=7200. */
  intervalSeconds: number;
  /** Uma das teclas de `afkKeys`; o backend recusa qualquer outra. No modo clique, ignorada. */
  key: string;
  mode: AfkMode;
  /** Ponto de quem não tem ponto próprio, em % da janela. */
  clickX: number;
  clickY: number;
}

/** O Marcar: de que conta é a janela sob o cursor e onde, em % dela. */
export interface AfkCapturedPoint {
  userId: number;
  xPct: number;
  yPct: number;
}

/** Onde uma conta está jogando agora, segundo a presença do Roblox. */
export interface AccountGameLocation {
  userId: number;
  inGame: boolean;
  placeId: number | null;
  /** Só vem com o cookie da própria conta. */
  jobId: string | null;
}

export interface GeneratorStatus {
  active: boolean;
  startedAtMs: number | null;
  provider: string;
  endpoint: string;
  accountType: string;
  extraDelaySeconds: number;
  targetGroup: string;
  maxAccounts: number;
  phase: string;
  nextAttemptAtMs: number | null;
  totalGenerated: number;
  lastUsername: string | null;
  lastUserId: number | null;
  lastError: string | null;
  lastGeneratedAtMs: number | null;
}

export interface GeneratorStartConfig {
  provider: string;
  endpoint: string;
  apiKey: string;
  accountType: string;
  extraDelaySeconds: number;
  targetGroup: string;
  maxAccounts: number;
}

// Mora em utils/ para os módulos puros (presets) usarem sem carregar o store.
export { parsePrivateServerCode } from "./utils/privateServerCode";
import { parsePrivateServerCode } from "./utils/privateServerCode";

/**
 * Explicit place/job for a launch. When provided, these take precedence over
 * the placeId/jobId held in store state — avoiding a stale-state race where a
 * caller sets place/job via setState and immediately triggers a launch (the
 * launch closure would otherwise still read the PREVIOUS place/job).
 */
/**
 * O que aconteceu com um launch de uma conta: `started` = o backend aceitou e o
 * cliente está subindo; `refused` = já havia uma sequência de launch em
 * andamento; `failed` = erro de launch (já reportado na tela). Quem chama usa
 * isto para não anunciar sucesso quando nada começou.
 */
export type LaunchAttempt = "started" | "refused" | "failed";

export interface LaunchTarget {
  placeId?: string;
  jobId?: string;
  /**
   * Overrides the store's `launchData` for this launch only (used by targets
   * resolved from a join link, which can carry their own launch data).
   */
  launchData?: string;
  /**
   * Forces a VIP/private join instead of parsing it out of `jobId`.
   * `joinServer` forwards it to `launch_roblox`; `launchMultiple` encodes it as
   * a `vip:<code>` job because `launch_multiple` has no such parameter.
   */
  joinVip?: boolean;
  /** VIP/private server code (link code or access code) used with `joinVip`. */
  linkCode?: string;
}

export interface StoreValue {
  accounts: Account[];
  groups: ParsedGroup[];
  loadAccounts: () => Promise<void>;
  saveAccounts: () => Promise<void>;
  /**
   * `password` só quando a linha colada trazia `usuario:senha` antes do cookie
   * (o formato do import): vai separado para o `add_account`, nunca no cookie.
   */
  addAccountByCookie: (cookie: string, password?: string) => Promise<void>;
  removeAccounts: (userIds: number[]) => Promise<void>;
  updateAccount: (account: Account) => Promise<void>;

  selectedIds: Set<number>;
  selectedAccount: Account | null;
  selectedAccounts: Account[];
  handleSelect: (userId: number, e: React.MouseEvent) => void;
  selectSingle: (userId: number) => void;
  selectAll: () => void;
  deselectAll: () => void;
  toggleSelectAll: () => void;
  setSelectedIds: (ids: Set<number>) => void;
  navigateSelection: (direction: "up" | "down", shift: boolean) => void;
  orderedUserIds: number[];

  searchQuery: string;
  setSearchQuery: (q: string) => void;
  showGroups: boolean;
  setShowGroups: (show: boolean) => void;
  collapsedGroups: Set<string>;
  toggleGroup: (group: string) => void;
  sidebarOpen: boolean;
  setSidebarOpen: (open: boolean) => void;
  chooseGameOpen: boolean;
  setChooseGameOpen: (open: boolean) => void;
  hideUsernames: boolean;
  setHideUsernames: (hide: boolean) => void;
  hiddenNameLetters: number;
  showAvatarsWhenHidden: boolean;
  hideRobuxWhenHidden: boolean;

  placeId: string;
  setPlaceId: (id: string) => void;
  jobId: string;
  setJobId: (id: string) => void;
  launchData: string;
  setLaunchData: (data: string) => void;
  shuffleJobId: boolean;
  setShuffleJobId: (shuffle: boolean) => void;

  /** Preferência de servidor do lote (item 4). Persistida em `General.ServerPreference`. */
  serverPreference: ServerPreference;
  setServerPreference: (preference: ServerPreference) => void;
  /**
   * Quantas páginas de 100 servidores a aba Servers varre antes de parar.
   * Persistido em `General.ServerScanPages`.
   */
  serverScanPages: number;
  setServerScanPages: (pages: number) => void;
  /** Código do país exigido ao escolher servidor (`BR`); vazio = sem filtro. */
  serverRegionFilter: string;
  setServerRegionFilter: (countryCode: string) => void;

  contextMenu: { x: number; y: number } | null;
  openContextMenu: (x: number, y: number) => void;
  closeContextMenu: () => void;

  settings: Record<string, Record<string, string>> | null;
  platformCapabilities: PlatformCapabilities | null;
  theme: ThemeData | null;
  applyThemePreview: (theme: ThemeData) => void;
  saveTheme: (theme: ThemeData) => Promise<void>;
  devMode: boolean;

  avatarUrls: Map<number, string>;
  presenceByUserId: Map<number, number>;
  /**
   * Moderação lida nesta sessão (banida/advertida/encerrada), por conta. Vem do
   * evento `account-moderation`: consulta no painel, "conferir contas" e a
   * checagem antes do launch. Só em memória.
   */
  moderationByUserId: Map<number, ModerationStatus>;
  /** Consulta a moderação de uma conta agora (sem refresh de sessão). */
  checkModeration: (userId: number) => Promise<ModerationStatus | null>;
  /**
   * "Check accounts": sessão e moderação de cada conta, só leitura e com ritmo
   * limitado (`check_accounts`). Termina com o toast de resumo.
   */
  checkAccounts: (userIds: number[]) => Promise<AccountCheckSummary | null>;
  /** Progresso do "Check accounts" em andamento; `null` quando parado. */
  accountCheckProgress: { done: number; total: number } | null;
  launchedByProgram: Set<number>;
  /** Contas de `launchedByProgram` cujo cliente foi aberto fora do app (pelo site). */
  adoptedClients: Set<number>;
  /** Clientes abertos fora do app que o backend não reconheceu sozinho. */
  unidentifiedClients: UnidentifiedClient[];
  /** Queda (com motivo) de cada conta em jogo, lida do log do Roblox. */
  clientHealth: Map<number, ClientHealth>;
  /**
   * Memória e limite de cada cliente que o app abriu (teto de memória, só com
   * a feature `memory-trim`). Atualizado no mesmo polling de 2,5 s.
   */
  clientMemory: Map<number, ClientMemory>;
  /** Diz ao app de quem é um cliente não identificado (não fecha nada). */
  identifyExternalClient: (pid: number, userId: number) => Promise<boolean>;
  /** Traz para a frente a janela de um cliente pelo PID. */
  focusClientWindow: (pid: number) => Promise<boolean>;

  joinServer: (userId: number, target?: LaunchTarget) => Promise<LaunchAttempt>;
  launchMultiple: (userIds: number[], target?: LaunchTarget) => Promise<void>;
  /**
   * Presets de launch (ideia 13). `presetsDialog` nulo = fechado; com `draft`
   * nulo abre na lista, com rascunho abre no editor. `presetsRevision` sobe a
   * cada resultado de preset (inclusive pelo horário) para a tela reler.
   */
  presetsDialog: { draft: LaunchPreset | null } | null;
  openPresetsDialog: (draft?: LaunchPreset | null) => void;
  closePresetsDialog: () => void;
  presetsRevision: number;
  /** Abre um preset pela fila normal. Devolve se começou. */
  launchPreset: (preset: LaunchPresetView) => Promise<LaunchAttempt>;
  restartRobloxClients: (userIds: number[]) => Promise<void>;
  focusRobloxClient: (userId: number) => Promise<boolean>;
  /** Fecha os clientes das contas informadas; devolve quantos foram fechados. */
  closeRobloxClients: (userIds: number[]) => Promise<number>;
  killAllRobloxProcesses: () => Promise<void>;

  /** Fila de launch do lote atual (Painel de Sessão). */
  launchQueue: LaunchQueuePayload | null;
  /**
   * Make Friends em andamento: uma entrada por conta, com o estado de cada uma.
   * Vem inteiro do backend (`friend-link-state`) — a tela que remonta no meio
   * não perde o progresso, e os dois lugares que disparam a operação mostram a
   * mesma coisa em vez de cada um manter o seu `useState`.
   */
  friendLinkState: FriendLinkState | null;
  refreshLaunchQueue: () => Promise<void>;
  /** Tira UMA conta da fila. Nunca fecha um cliente já aberto. */
  cancelAccountLaunch: (userId: number) => Promise<boolean>;
  /** Esvazia a fila; devolve quantas contas saíram. Não fecha clientes. */
  stopLaunchQueue: () => Promise<number>;
  /**
   * Reconexão automática das contas que caíram (`commands/reconnect.rs`):
   * vem inteira do backend (evento `auto-reconnect`).
   */
  autoReconnect: AutoReconnectEntry[];
  /** "Parar" / "Dispensar": a conta sai da reconexão. Não fecha cliente. */
  stopAutoReconnect: (userId: number) => Promise<boolean>;
  /** "Tentar agora" / "Tentar de novo". */
  retryAutoReconnect: (userId: number) => Promise<boolean>;
  startBottingMode: (config: BottingStartConfig) => Promise<void>;
  /**
   * Liga o Auto Rejoin nas contas que **já estão em jogo**, sem fechar nem
   * relançar o cliente delas.
   *
   * Com sessão ativa é só entrar nela; sem sessão, o place vem da **presença**
   * da conta — usar o place da tela mandaria a conta para outro jogo no
   * primeiro reinício do ciclo.
   */
  adoptRunningIntoBotting: (userIds: number[], options?: AdoptBottingOptions) => Promise<void>;
  /**
   * O place em que as contas estão jogando agora, pela presença de cada uma (a
   * primeira que responder em jogo). `null` quando nenhuma diz.
   */
  detectRunningGamePlace: (userIds: number[]) => Promise<number | null>;
  stopBottingMode: (closeBotAccounts: boolean) => Promise<void>;
  addBottingAccounts: (userIds: number[]) => Promise<void>;
  setBottingPlayerAccounts: (userIds: number[]) => Promise<void>;
  bottingAccountAction: (
    userId: number,
    action: "disconnect" | "close" | "closeDisconnect" | "restartClient" | "restartLoop"
  ) => Promise<void>;
  refreshBottingStatus: () => Promise<void>;
  /**
   * Liga o AFK mode nas contas escolhidas. A cada intervalo o app traz a janela
   * de cada uma para frente, uma depois da outra, manda a tecla e só devolve o
   * foco depois da última — é a única forma de o cliente do Roblox receber a
   * tecla. Sem tecla escolhida o backend recusa ligar.
   */
  startAfkMode: (config: AfkStartConfig) => Promise<void>;
  /** Para na hora, inclusive um ciclo em andamento. Não fecha cliente nenhum. */
  stopAfkMode: () => Promise<void>;
  /** Troca quem está no modo numa sessão em andamento. Lista vazia desliga. */
  setAfkAccounts: (userIds: number[]) => Promise<void>;
  refreshAfkStatus: () => Promise<void>;
  /**
   * Um ciclo agora, nas contas passadas: é assim que o usuário confere que o
   * envio funciona sem esperar o intervalo. Tecla ou clique é o da sessão
   * ligada. Devolve quantas contas receberam.
   */
  afkTriggerNow: (userIds: number[]) => Promise<number>;
  /**
   * Lê a posição do cursor uma vez (a contagem é da tela) e devolve de que conta
   * é a janela embaixo e onde. Rejeita com um **código** (`noCursor`,
   * `noWindow`, `notAnAccountWindow`, `outsideGameArea`) que a tela traduz.
   */
  captureAfkPoint: () => Promise<AfkCapturedPoint>;
  afkStatus: AfkStatus | null;
  /** A lista fechada de teclas que o backend aceita. */
  afkKeys: string[];
  startGenerator: (config: GeneratorStartConfig) => Promise<GeneratorStatus>;
  stopGenerator: () => Promise<void>;
  refreshGeneratorStatus: () => Promise<void>;
  refreshCookie: (userId: number) => Promise<boolean>;
  moveToGroup: (userIds: number[], group: string) => Promise<void>;
  sortGroupAlphabetically: (groupKey: string) => void;
  reorderAccounts: (draggedUserId: number, targetUserId: number) => Promise<void>;
  joiningAccounts: Set<number>;
  launchProgress: LaunchProgressState | null;
  launchLogs: LaunchLogEntry[];
  clearLaunchLogs: () => void;

  dragState: { userId: number; sourceGroup: string } | null;
  setDragState: (s: { userId: number; sourceGroup: string } | null) => void;

  /**
   * Grupo sendo arrastado pelo cabeçalho. Separado do `dragState` (que é de
   * conta) de propósito: misturar os dois faria o drop de conta agir sobre um
   * arrasto de grupo.
   */
  groupDragState: { groupKey: string } | null;
  setGroupDragState: (s: { groupKey: string } | null) => void;
  /** Move um grupo para a posição de outro e guarda a ordem em `General.GroupOrder`. */
  reorderGroups: (draggedKey: string, targetKey: string) => Promise<void>;

  toasts: Toast[];
  addToast: (msg: string, tone?: ToastTone) => void;
  actionStatus: ActionStatusState | null;
  modal: { title: string; content: string } | null;
  showModal: (title: string, content: string) => void;
  closeModal: () => void;

  error: string | null;
  setError: (e: string | null) => void;
  needsPassword: boolean;
  /**
   * Destranca as contas. Com `rememberHours`, guarda a senha protegida pelo SO
   * por esse tempo; sem ele, apaga qualquer lembrete anterior.
   */
  unlocking: boolean;
  unlock: (password: string, rememberHours?: number) => Promise<void>;
  /**
   * Tela trancada por inatividade (ideia 27). Só a tela: o app segue montado
   * por baixo e nada que esteja rodando para.
   */
  appLocked: boolean;
  lockApp: () => void;
  /** Confere a senha (`verify_app_password`) e destranca; devolve o erro, se houver. */
  unlockApp: (password: string) => Promise<string | null>;
  encryptionSetupOpen: boolean;
  encryptionSetupMode: "firstRun" | "settings";
  accountsEncrypted: boolean | null;
  /** Problema com o AccountData.key; a faixa fixa desenha isto. */
  vaultKeyWarning: VaultKeyWarning | null;
  applyingEncryption: boolean;
  encryptionSetupError: string | null;
  openEncryptionSetupFromSettings: () => void;
  closeEncryptionSetup: () => void;
  applyEncryptionMethod: (method: "default" | "password", password?: string) => Promise<void>;
  firstRunWalkthroughOpen: boolean;
  firstRunWalkthroughMode: "firstRun" | "manual";
  openFirstRunWalkthroughFromSettings: () => void;
  closeFirstRunWalkthrough: () => void;
  completeFirstRunWalkthrough: () => Promise<void>;
  skipFirstRunWalkthrough: () => Promise<void>;
  initialized: boolean;

  /** Página aberta na área principal. Ver `AppPage`. */
  activePage: AppPage;
  setActivePage: (page: AppPage) => void;

  /**
   * Os `set<Algo>Open` das telas que viraram página são adaptadores: `true`
   * navega para a página, `false` volta para a lista de contas **só se** aquela
   * página for a aberta (fechar o que não está aberto não tira ninguém do lugar).
   */
  setSettingsOpen: (open: boolean) => void;
  reloadSettings: () => Promise<void>;
  /**
   * Grava uma setting fora da página Settings (ex.: o padrão de reconexão na
   * página Session). A tela muda na hora; se o INI recusar, volta o valor
   * antigo e avisa. A página Settings relê tudo ao abrir, então as duas telas
   * mostram o mesmo valor.
   */
  updateSetting: (section: string, key: string, value: string) => Promise<void>;

  serverListOpen: boolean;
  setServerListOpen: (open: boolean) => void;

  accountUtilsOpen: boolean;
  setAccountUtilsOpen: (open: boolean) => void;
  accountFieldsOpen: boolean;
  setAccountFieldsOpen: (open: boolean) => void;
  importDialogOpen: boolean;
  setImportDialogOpen: (open: boolean) => void;
  importDialogTab: "cookie" | "userpass" | "legacy";
  setImportDialogTab: (tab: "cookie" | "userpass" | "legacy") => void;
  setThemeEditorOpen: (open: boolean) => void;
  /**
   * Janela do Modo AFK (Auto Rejoin + cliques AFK). Uma só: antes eram dois
   * diálogos, e quem ligava o Auto Rejoin pelo "Em jogo" não achava onde
   * configurá-lo nem pará-lo.
   */
  afkModeDialog: AfkModeDialogState | null;
  openAfkMode: (opts?: Partial<AfkModeDialogState>) => void;
  closeAfkMode: () => void;
  /**
   * Abre o Modo AFK na aba Auto Rejoin, opcionalmente já com um jogo escolhido
   * (clique direito numa lista de jogos). O jogo **vence o rascunho salvo**.
   */
  openBottingDialog: (placeId?: string) => void;
  bottingStatus: BottingStatus | null;
  generatorDialogOpen: boolean;
  /**
   * Aba em que o diálogo do gerador abre. O menu "Add" tem uma entrada para
   * cada forma de conseguir conta, e cada uma abre o diálogo já na sua aba.
   */
  generatorDialogTab: GeneratorDialogTab;
  openGeneratorDialog: (tab: GeneratorDialogTab) => void;
  setGeneratorDialogOpen: (open: boolean) => void;
  generatorStatus: GeneratorStatus | null;
  versionsDialogOpen: boolean;
  setVersionsDialogOpen: (open: boolean) => void;
  /** Checagem "o launch não faz nada" (ideia 16) — Settings e a faixa de erro. */
  diagnosticsOpen: boolean;
  setDiagnosticsOpen: (open: boolean) => void;
  /** Adicionar conta por Quick Login (ideia 12) — menu Add e AddAccountDialog. */
  quickLoginOpen: boolean;
  setQuickLoginOpen: (open: boolean) => void;
  /** Abre (ou fecha) o Modo AFK na aba de cliques AFK — o atalho da barra. */
  setAfkDialogOpen: (open: boolean) => void;
  setAvatarsDialogOpen: (open: boolean) => void;
  /** Invalida e busca de novo o headshot das contas cujo avatar mudou. */
  refreshAvatarHeadshots: (userIds: number[]) => Promise<void>;
  setSessionDialogOpen: (open: boolean) => void;
  setDefaultVersion: (versionId: string | null) => void;
  missingAssets: { userId: number; username: string; assetIds: number[] } | null;
  setMissingAssets: (v: { userId: number; username: string; assetIds: number[] } | null) => void;

  setNexusOpen: (open: boolean) => void;
  setScriptsOpen: (open: boolean) => void;

  updateInfo: {
    version: string;
    currentVersion: string;
    date: string;
    body: string;
    releaseChannel: UpdaterReleaseChannel;
    featureChannel: UpdaterFeatureChannel;
    /** O diálogo baixa e instala sozinho (troca de edição pedida pela pessoa). */
    autoInstall?: boolean;
  } | null;
  updateDialogOpen: boolean;
  setUpdateDialogOpen: (open: boolean) => void;
  /** `true` quando achou uma atualização e abriu o diálogo. */
  checkForUpdates: (
    manual?: boolean,
    channels?: { releaseChannel?: string; featureChannel?: string },
    options?: { autoInstall?: boolean; noUpdateMessage?: string }
  ) => Promise<boolean>;
  /**
   * Passa o updater para a edição completa (`General.UpdaterFeatureChannel =
   * nexus-ws`) e baixa e instala o instalador dela pelo próprio updater do app —
   * mesmo na mesma versão. Ver docs/features/avatars.md.
   */
  switchToCompleteEdition: () => Promise<boolean>;
  openUpdatePreviewDialog: () => void;

  openLoginBrowser: () => Promise<void>;
  openAccountBrowser: (userId: number) => Promise<void>;
  browserDownload: BrowserDownloadState | null;
  /** `force=true` apaga a instalação existente e baixa de novo (botão Reinstall). */
  ensureBrowserDownload: (force?: boolean) => Promise<boolean>;
}

const StoreContext = createContext<StoreValue | null>(null);

export function useStore() {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error("useStore requires StoreProvider");
  return ctx;
}

/** Tentativas de buscar o headshot novo depois de mudar o avatar, e a pausa entre elas. */
const HEADSHOT_REFRESH_ATTEMPTS = 3;
const HEADSHOT_REFRESH_RETRY_MS = 3000;

export function StoreProvider({ children }: { children: ReactNode }) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [lastClickedId, setLastClickedId] = useState<number | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [showGroups, setShowGroups] = useState(true);
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const [placeId, _setPlaceId] = useState("");
  const [jobId, _setJobId] = useState("");
  const [launchData, _setLaunchData] = useState("");

  const setPlaceId = useCallback((v: string) => {
    _setPlaceId(v);
    invoke("update_setting", { section: "General", key: "SavedPlaceId", value: v }).catch(() => {});
  }, []);
  const setJobId = useCallback((v: string) => {
    _setJobId(v);
    invoke("update_setting", { section: "General", key: "SavedJobId", value: v }).catch(() => {});
  }, []);
  const setLaunchData = useCallback((v: string) => {
    _setLaunchData(v);
    invoke("update_setting", { section: "General", key: "SavedLaunchData", value: v }).catch(() => {});
  }, []);
  const [hideUsernamesState, setHideUsernamesState] = useState(false);
  const hideUsernames = hideUsernamesState;
  const setHideUsernames = useCallback((hide: boolean) => {
    setHideUsernamesState(hide);
    invoke("update_setting", {
      section: "General",
      key: "HideUsernames",
      value: hide ? "true" : "false",
    }).catch(() => {});
  }, []);
  const [shuffleJobId, setShuffleJobId] = useState(false);
  const [serverPreference, _setServerPreference] = useState<ServerPreference>("bestfit");
  const [serverRegionFilter, _setServerRegionFilter] = useState("");

  const setServerPreference = useCallback((preference: ServerPreference) => {
    _setServerPreference(preference);
    invoke("update_setting", {
      section: "General",
      key: "ServerPreference",
      value: preference,
    }).catch(() => {});
  }, []);

  const [serverScanPages, _setServerScanPages] = useState(DEFAULT_SERVER_SCAN_PAGES);

  const setServerScanPages = useCallback((pages: number) => {
    const normalized = normalizeServerScanPages(pages);
    _setServerScanPages(normalized);
    invoke("update_setting", {
      section: "General",
      key: "ServerScanPages",
      value: String(normalized),
    }).catch(() => {});
  }, []);

  const setServerRegionFilter = useCallback((countryCode: string) => {
    const normalized = countryCode.trim().toUpperCase();
    _setServerRegionFilter(normalized);
    invoke("update_setting", {
      section: "General",
      key: "ServerRegionFilter",
      value: normalized,
    }).catch(() => {});
  }, []);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [chooseGameOpen, setChooseGameOpen] = useState(false);
  const [settings, setSettings] = useState<Record<string, Record<string, string>> | null>(null);
  const [platformCapabilities, setPlatformCapabilities] = useState<PlatformCapabilities | null>(null);
  const [theme, setThemeState] = useState<ThemeData | null>(null);
  const [avatarUrls, setAvatarUrls] = useState<Map<number, string>>(new Map());
  const [presenceByUserId, setPresenceByUserId] = useState<Map<number, number>>(new Map());
  const [moderationByUserId, setModerationByUserId] = useState<Map<number, ModerationStatus>>(new Map());
  const [accountCheckProgress, setAccountCheckProgress] = useState<{ done: number; total: number } | null>(null);
  const rememberModeration = useCallback((userId: number, status: ModerationStatus) => {
    setModerationByUserId((prev) => {
      const next = new Map(prev);
      next.set(userId, status);
      return next;
    });
  }, []);
  const [launchedByProgram, setLaunchedByProgram] = useState<Set<number>>(new Set());
  const [adoptedClients, setAdoptedClients] = useState<Set<number>>(new Set());
  const [unidentifiedClients, setUnidentifiedClients] = useState<UnidentifiedClient[]>([]);
  const [clientHealth, setClientHealth] = useState<Map<number, ClientHealth>>(new Map());
  const [clientMemory, setClientMemory] = useState<Map<number, ClientMemory>>(new Map());
  // O efeito do polling registra aqui o seu refresh, para identificar um
  // cliente refletir na hora em vez de esperar o próximo tique.
  const refreshRunningRef = useRef<() => Promise<void>>(async () => {});
  const [error, setError] = useState<string | null>(null);
  const [needsPassword, setNeedsPassword] = useState(false);
  const [unlocking, setUnlocking] = useState(false);
  const [appLocked, setAppLocked] = useState(false);
  const lockApp = useCallback(() => setAppLocked(true), []);
  const unlockApp = useCallback(async (password: string): Promise<string | null> => {
    try {
      await invoke("verify_app_password", { password });
      setAppLocked(false);
      return null;
    } catch (e) {
      // Sem senha não há o que conferir (não deveria acontecer: a opção só
      // vale com senha). Destrancar é melhor que prender a pessoa para sempre.
      if (String(e).includes("No app password is set")) {
        setAppLocked(false);
        return null;
      }
      return String(e);
    }
  }, []);
  const [encryptionSetupOpen, setEncryptionSetupOpen] = useState(false);
  const [encryptionSetupMode, setEncryptionSetupMode] = useState<"firstRun" | "settings">("firstRun");
  const [accountsEncrypted, setAccountsEncrypted] = useState<boolean | null>(null);
  const [vaultKeyWarning, setVaultKeyWarning] = useState<VaultKeyWarning | null>(null);
  const [applyingEncryption, setApplyingEncryption] = useState(false);
  const [encryptionSetupError, setEncryptionSetupError] = useState<string | null>(null);
  const [firstRunWalkthroughOpen, setFirstRunWalkthroughOpen] = useState(false);
  const [firstRunWalkthroughMode, setFirstRunWalkthroughMode] = useState<"firstRun" | "manual">("firstRun");
  const [initialized, setInitialized] = useState(false);
  const [dragState, setDragState] = useState<{ userId: number; sourceGroup: string } | null>(null);
  const [groupDragState, setGroupDragState] = useState<{ groupKey: string } | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastIdRef = useRef(0);
  const [modal, setModal] = useState<{ title: string; content: string } | null>(null);
  const [activePage, setActivePageState] = useState<AppPage>("accounts");
  const setActivePage = useCallback((page: AppPage) => setActivePageState(page), []);
  /** Adaptador de `set<Algo>Open` para página — ver o comentário em `StoreValue`. */
  const togglePage = useCallback((page: AppPage, open: boolean) => {
    setActivePageState((current) => (open ? page : current === page ? "accounts" : current));
  }, []);
  const setSettingsOpen = useCallback((open: boolean) => togglePage("settings", open), [togglePage]);
  const setThemeEditorOpen = useCallback((open: boolean) => togglePage("theme", open), [togglePage]);
  const setAvatarsDialogOpen = useCallback((open: boolean) => togglePage("avatars", open), [togglePage]);
  const setSessionDialogOpen = useCallback((open: boolean) => togglePage("session", open), [togglePage]);
  const setNexusOpen = useCallback((open: boolean) => togglePage("nexus", open), [togglePage]);
  const setScriptsOpen = useCallback((open: boolean) => togglePage("scripts", open), [togglePage]);
  const [serverListOpen, setServerListOpen] = useState(false);
  const [accountUtilsOpen, setAccountUtilsOpen] = useState(false);
  const [accountFieldsOpen, setAccountFieldsOpen] = useState(false);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [importDialogTab, setImportDialogTab] = useState<"cookie" | "userpass" | "legacy">("cookie");
  const [afkModeDialog, setAfkModeDialog] = useState<AfkModeDialogState | null>(null);
  const openAfkMode = useCallback((opts?: Partial<AfkModeDialogState>) => {
    // Sem aba pedida, os cliques AFK (a aba padrão do Modo AFK).
    setAfkModeDialog({ ...opts, tab: opts?.tab ?? "clicks" });
  }, []);
  const closeAfkMode = useCallback(() => setAfkModeDialog(null), []);
  /**
   * Abrir sem jogo **limpa** o jogo da abertura anterior: senão o place escolhido
   * num clique direito continuaria carimbando a tela aberta pela barra.
   */
  const openBottingDialog = useCallback((placeId?: string) => {
    setAfkModeDialog({ tab: "rejoin", placeId: placeId?.trim() ? placeId.trim() : null });
  }, []);
  const setAfkDialogOpen = useCallback((open: boolean) => {
    setAfkModeDialog(open ? { tab: "clicks" } : null);
  }, []);
  const [bottingStatus, setBottingStatus] = useState<BottingStatus | null>(null);
  const [afkStatus, setAfkStatus] = useState<AfkStatus | null>(null);
  const [afkKeys, setAfkKeys] = useState<string[]>([]);
  const [generatorDialogOpen, setGeneratorDialogOpen] = useState(false);
  const [generatorDialogTab, setGeneratorDialogTab] = useState<GeneratorDialogTab>("provider");

  const openGeneratorDialog = useCallback((tab: GeneratorDialogTab) => {
    setGeneratorDialogTab(tab);
    setGeneratorDialogOpen(true);
  }, []);
  const [generatorStatus, setGeneratorStatus] = useState<GeneratorStatus | null>(null);
  const [versionsDialogOpen, setVersionsDialogOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [quickLoginOpen, setQuickLoginOpen] = useState(false);
  const [presetsDialog, setPresetsDialog] = useState<{ draft: LaunchPreset | null } | null>(null);
  const [presetsRevision, setPresetsRevision] = useState(0);
  const openPresetsDialog = useCallback((draft?: LaunchPreset | null) => {
    setPresetsDialog({ draft: draft ?? null });
  }, []);
  const closePresetsDialog = useCallback(() => setPresetsDialog(null), []);
  const [launchQueue, setLaunchQueue] = useState<LaunchQueuePayload | null>(null);
  const [friendLinkState, setFriendLinkState] = useState<FriendLinkState | null>(null);
  const [autoReconnect, setAutoReconnect] = useState<AutoReconnectEntry[]>([]);
  // Retrato anterior, para avisar só na mudança (desistiu, parou, reconectou).
  const autoReconnectRef = useRef<AutoReconnectEntry[]>([]);
  const [missingAssets, setMissingAssets] = useState<{ userId: number; username: string; assetIds: number[] } | null>(null);
  const [updateInfo, setUpdateInfo] = useState<{
    version: string;
    currentVersion: string;
    date: string;
    body: string;
    releaseChannel: UpdaterReleaseChannel;
    featureChannel: UpdaterFeatureChannel;
    autoInstall?: boolean;
  } | null>(null);
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const [joiningAccounts, setJoiningAccounts] = useState<Set<number>>(new Set());
  const [launchProgress, setLaunchProgress] = useState<LaunchProgressState | null>(null);
  const [launchLogs, setLaunchLogs] = useState<LaunchLogEntry[]>([]);
  const launchLogIdRef = useRef(0);
  // Latest accounts for long-lived event listeners (their effects don't
  // re-subscribe on every accounts change, so a captured value goes stale).
  const accountsRef = useRef<Account[]>([]);
  accountsRef.current = accounts;
  const clearLaunchLogs = useCallback(() => setLaunchLogs([]), []);
  const [actionStatus, setActionStatus] = useState<ActionStatusState | null>(null);
  const [browserDownload, setBrowserDownload] = useState<BrowserDownloadState | null>(null);

  const avatarLoadingRef = useRef<Set<number>>(new Set());
  const launchClearTimeoutRef = useRef<number | null>(null);
  const actionStatusTimeoutRef = useRef<number | null>(null);
  const walkthroughOpenTimeoutRef = useRef<number | null>(null);

  const devMode = settings?.Developer?.DevMode === "true";
  const hiddenNameLetters = parseInt(settings?.General?.HiddenNameLetters || "0") || 0;
  const showAvatarsWhenHidden = settings?.General?.ShowAvatarsWhenHidden === "true";
  const hideRobuxWhenHidden = settings?.General?.HideRobuxWhenHidden === "true";
  /**
   * O "Names hidden" de agora, para os toasts que nomeiam uma conta — inclusive
   * os dos listeners montados uma vez só, que veriam o valor do primeiro render.
   */
  const nameMaskingRef = useRef({ hideUsernames, hiddenNameLetters });
  nameMaskingRef.current = { hideUsernames, hiddenNameLetters };

  const filteredAccounts = useMemo(() => {
    if (!searchQuery) return accounts;
    const q = searchQuery.toLowerCase();
    return accounts.filter(
      (a) =>
        (a.Username || "").toLowerCase().includes(q) ||
        (a.Alias || "").toLowerCase().includes(q) ||
        (a.Description || "").toLowerCase().includes(q) ||
        (a.Group || "").toLowerCase().includes(q)
    );
  }, [accounts, searchQuery]);

  const groups = useMemo(() => {
    if (!showGroups) {
      return [
        {
          key: "__all__",
          displayName: tr("Accounts"),
          sortKey: 0,
          accounts: filteredAccounts,
        },
      ];
    }

    const groupMap = new Map<string, Account[]>();
    for (const account of filteredAccounts) {
      const group = account.Group || "Default";
      if (!groupMap.has(group)) groupMap.set(group, []);
      groupMap.get(group)!.push(account);
    }
    const parsed: ParsedGroup[] = [];
    for (const [key, accts] of groupMap) {
      const { sortKey, displayName } = parseGroupName(key);
      parsed.push({ key, displayName, sortKey, accounts: accts });
    }
    // Ordem manual (arrastada pelo usuário) primeiro; o resto na ordem
    // automática de sempre. Sem `GroupOrder`, nada muda.
    const manual = orderGroupKeys(
      parsed.map((g) => g.key),
      parseGroupOrder(settings?.General?.GroupOrder)
    );
    const posicao = new Map(manual.map((key, index) => [key, index]));
    parsed.sort((a, b) => (posicao.get(a.key) ?? 0) - (posicao.get(b.key) ?? 0));

    // If only one group and it's "Default", treat as flat list (no header).
    // Headers only appear when the user has created named groups.
    if (parsed.length === 1 && parsed[0].key === "Default") {
      return [{ key: "__all__", displayName: tr("Accounts"), sortKey: 0, accounts: parsed[0].accounts }];
    }

    return parsed;
  }, [filteredAccounts, settings?.General?.GroupOrder, showGroups]);

  const orderedUserIds = useMemo(() => {
    const ids: number[] = [];
    for (const group of groups) {
      if (!showGroups || !collapsedGroups.has(group.key)) {
        for (const account of group.accounts) ids.push(account.UserID);
      }
    }
    return ids;
  }, [groups, collapsedGroups, showGroups]);

  const selectedAccount = useMemo(() => {
    if (selectedIds.size !== 1) return null;
    const id = [...selectedIds][0];
    return accounts.find((a) => a.UserID === id) || null;
  }, [accounts, selectedIds]);

  const selectedAccounts = useMemo(
    () => accounts.filter((a) => selectedIds.has(a.UserID)),
    [accounts, selectedIds]
  );

  // Seleção e filtro andavam separados: com "No matches" na tela, a barra
  // inferior ainda dizia "2 accounts selected" e Remove/Choose Game agiam sobre
  // contas que o usuário não estava vendo.
  //
  // Decisão: enquanto houver filtro, a seleção é podada para o que está
  // visível. Das três saídas possíveis (podar, restringir só as ações, ou
  // avisar), esta é a única que conserta *todos* os consumidores de uma vez —
  // barra inferior, StatusBar, menu de contexto e diálogos leem `selectedIds` /
  // `selectedAccounts` direto, então restringir ação por ação deixaria contagem
  // e efeito divergindo de novo. O usuário não perde nada silenciosamente: a
  // contagem cai junto com as linhas que somem, na mesma tela.
  //
  // Sem filtro nada é podado — limpar a busca mantém o que sobreviveu, em vez
  // de ressuscitar uma seleção que o usuário já não vê há vários caracteres.
  useEffect(() => {
    if (!searchQuery) return;
    setSelectedIds((prev) => {
      if (prev.size === 0) return prev;
      const visible = new Set(filteredAccounts.map((a) => a.UserID));
      const next = new Set([...prev].filter((id) => visible.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [filteredAccounts, searchQuery]);

  const handleSelect = useCallback(
    (userId: number, e: React.MouseEvent) => {
      const toggle = e.altKey || e.ctrlKey || e.metaKey;
      if (e.shiftKey && lastClickedId !== null) {
        const startIdx = orderedUserIds.indexOf(lastClickedId);
        const endIdx = orderedUserIds.indexOf(userId);
        if (startIdx >= 0 && endIdx >= 0) {
          const lo = Math.min(startIdx, endIdx);
          const hi = Math.max(startIdx, endIdx);
          const range = new Set(orderedUserIds.slice(lo, hi + 1));
          if (toggle) {
            setSelectedIds((prev) => new Set([...prev, ...range]));
          } else {
            setSelectedIds(range);
          }
        }
      } else if (toggle) {
        setSelectedIds((prev) => {
          const next = new Set(prev);
          if (next.has(userId)) next.delete(userId);
          else next.add(userId);
          return next;
        });
      } else {
        setSelectedIds(new Set([userId]));
      }
      setLastClickedId(userId);
    },
    [lastClickedId, orderedUserIds]
  );

  const selectSingle = useCallback((userId: number) => {
    setSelectedIds(new Set([userId]));
    setLastClickedId(userId);
  }, []);

  const selectAll = useCallback(() => {
    setSelectedIds(new Set(filteredAccounts.map((a) => a.UserID)));
  }, [filteredAccounts]);

  const deselectAll = useCallback(() => {
    setSelectedIds(new Set());
  }, []);

  const toggleSelectAll = useCallback(() => {
    setSelectedIds((prev) => {
      if (prev.size > 0) return new Set();
      return new Set(filteredAccounts.map((a) => a.UserID));
    });
  }, [filteredAccounts]);

  const navigateSelection = useCallback(
    (direction: "up" | "down", shift: boolean) => {
      if (orderedUserIds.length === 0) return;

      const anchorId = lastClickedId ?? orderedUserIds[0];
      const currentIdx = orderedUserIds.indexOf(anchorId);
      if (currentIdx < 0) return;

      const nextIdx = direction === "up"
        ? Math.max(0, currentIdx - 1)
        : Math.min(orderedUserIds.length - 1, currentIdx + 1);
      const nextId = orderedUserIds[nextIdx];

      if (shift) {
        setSelectedIds((prev) => {
          const next = new Set(prev);
          next.add(nextId);
          return next;
        });
      } else {
        setSelectedIds(new Set([nextId]));
      }
      setLastClickedId(nextId);
    },
    [lastClickedId, orderedUserIds]
  );

  const toggleGroup = useCallback((group: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(group)) next.delete(group);
      else next.add(group);
      return next;
    });
  }, []);

  /**
   * A linha de estado do rodapé (`StatusBar`): "isto **está acontecendo
   * agora**" — progresso de download, conta N de M, o que o `useSettings`
   * anuncia por `ram-action-status`. É substituível: a mensagem seguinte troca
   * a anterior, e o timeout apaga.
   *
   * Não confundir com `addToast`, que é "isto **acabou de acontecer**". Os dois
   * escreviam a mesma frase (com durações diferentes), então depois que o
   * rodapé passou a desenhar `actionStatus` cada toast apareceria duas vezes na
   * tela — por isso `addToast` não escreve mais aqui.
   */
  const setActionStatusMessage = useCallback(
    (message: string, tone: ActionStatusTone = "info", timeoutMs = 3500) => {
      // `message` is usually an i18n key, but some call sites pass an already-localized string.
      const localized = i18n.exists(message) ? tr(message) : message;
      if (actionStatusTimeoutRef.current !== null) {
        window.clearTimeout(actionStatusTimeoutRef.current);
        actionStatusTimeoutRef.current = null;
      }
      setActionStatus({
        message: localized,
        tone,
        at: Date.now(),
      });
      if (timeoutMs > 0) {
        actionStatusTimeoutRef.current = window.setTimeout(() => {
          setActionStatus((prev) => (prev?.message === localized ? null : prev));
          actionStatusTimeoutRef.current = null;
        }, timeoutMs);
      }
    },
    []
  );

  /**
   * Tira do rodapé a linha `message` — **só se ela ainda for a que está lá**.
   * Quem anunciou "Launching X…" e foi recusado não pode deixar a frase no ar
   * até o timeout dela (5 s dizendo que lança o que acabou de ser recusado);
   * mas se outra ação já escreveu por cima nesse meio-tempo, a linha dela fica.
   */
  const withdrawActionStatus = useCallback((message: string) => {
    const localized = i18n.exists(message) ? tr(message) : message;
    setActionStatus((prev) => (prev?.message === localized ? null : prev));
  }, []);

  /**
   * A fila de toasts: "isto **acabou de acontecer**". O tom sai do texto uma
   * única vez, aqui, e vai junto no item — os ~200 call sites continuam
   * chamando `addToast(frase)` e nada mais.
   *
   * O tom é deduzido de `msg` (não do texto localizado) porque o catálogo
   * garante que a tradução preserva o marcador — ver `src/i18n/locales.test.ts`.
   */
  const addToast = useCallback((msg: string, tone?: ToastTone) => {
    // `msg` is usually an i18n key, but some call sites pass an already-localized string (interpolated).
    const localized = i18n.exists(msg) ? tr(msg) : msg;
    const id = ++toastIdRef.current;
    // O heuristico serve aos ~200 call sites que nao se importam com o tom; quem
    // sabe o tom da sua mensagem passa explicito e vence o texto. Sem isso,
    // "Aviso de copia de credencial desligado" — que e confirmacao de ajuste —
    // saia pintado de ambar so por conter a palavra "aviso".
    setToasts((prev) => [...prev, { id, message: localized, tone: tone ?? toneFromMessage(msg) }]);
    // Remover por `id`, não por posição: dois toasts com vidas sobrepostas
    // fariam o `slice(1)` derrubar o vizinho errado.
    setTimeout(() => setToasts((prev) => prev.filter((toast) => toast.id !== id)), 2500);
  }, []);

  function clearLaunchTimeout() {
    if (launchClearTimeoutRef.current !== null) {
      window.clearTimeout(launchClearTimeoutRef.current);
      launchClearTimeoutRef.current = null;
    }
  }

  const showModal = useCallback((title: string, content: string) => {
    setModal({ title, content });
  }, []);

  const closeModal = useCallback(() => setModal(null), []);

  const openContextMenu = useCallback((x: number, y: number) => {
    setContextMenu({ x, y });
  }, []);

  const closeContextMenu = useCallback(() => setContextMenu(null), []);

  async function loadAvatars(accts: Account[]) {
    await loadAvatarIds(accts.map((a) => a.UserID));
  }

  /**
   * Busca o headshot das contas que ainda não têm foto. Com `force`, busca de
   * novo mesmo quem já tem — é o caso do avatar que acabou de mudar.
   *
   * Devolve quem ficou **sem** foto nova: o Roblox respondeu sem `imageUrl`
   * ("Pending"), a chamada falhou ou (com `force`) a conta já estava sendo
   * buscada por outra chamada, que pode ter saído antes da mudança.
   */
  async function loadAvatarIds(userIds: number[], force = false): Promise<number[]> {
    const busy = userIds.filter((id) => avatarLoadingRef.current.has(id));
    const ids = userIds.filter(
      (id) => (force || !avatarUrls.has(id)) && !avatarLoadingRef.current.has(id)
    );
    if (ids.length === 0) return force ? busy : [];
    ids.forEach((id) => avatarLoadingRef.current.add(id));
    try {
      const results = await invoke<ThumbnailData[]>("batched_get_avatar_headshots", {
        userIds: ids,
        size: "48x48",
      });
      const fresh = (Array.isArray(results) ? results : []).filter((r) => r.imageUrl);
      setAvatarUrls((prev) => {
        const next = new Map(prev);
        for (const r of fresh) next.set(r.targetId, r.imageUrl as string);
        return next;
      });
      const got = new Set(fresh.map((r) => r.targetId));
      return [...ids.filter((id) => !got.has(id)), ...(force ? busy : [])];
    } catch {
      return [...ids, ...(force ? busy : [])];
    } finally {
      ids.forEach((id) => avatarLoadingRef.current.delete(id));
    }
  }

  /**
   * O avatar dessas contas mudou (lote de avatares): o headshot em cache no
   * backend está velho. Invalida o cache e busca de novo.
   *
   * A foto antiga **nunca** é apagada antes: logo depois de vestir, o Roblox
   * costuma responder "Pending" (sem `imageUrl`), e apagar deixava a conta sem
   * foto até recarregar o app. A busca forçada só sobrescreve quando chega a
   * imagem nova; quem volta sem ela é tentado de novo algumas vezes.
   */
  async function refreshAvatarHeadshots(userIds: number[]) {
    let pending = [...new Set(userIds)].filter((id) => id > 0);
    for (let attempt = 0; attempt < HEADSHOT_REFRESH_ATTEMPTS && pending.length > 0; attempt++) {
      if (attempt > 0) await new Promise((resolve) => setTimeout(resolve, HEADSHOT_REFRESH_RETRY_MS));
      // A cada tentativa: um "Pending" que tenha ido para o cache não pode responder a próxima.
      try {
        await invoke("invalidate_avatar_headshots", { userIds: pending });
      } catch {}
      pending = await loadAvatarIds(pending, true);
    }
  }

  /**
   * Um problema com o `AccountData.key` não pode morrer num `eprintln!` do
   * backend: numa build GUI aquilo não vai a lugar nenhum, e é justamente o
   * defeito que passa o dia inteiro invisível (a chave está em memória, tudo
   * funciona) para virar "não abre mais" no boot seguinte.
   *
   * Duas correções em relação à primeira tentativa, que usava a linha de status:
   * ela é **substituível** (qualquer "Launching…" apagava o aviso) e nunca era
   * limpa quando o problema sumia. Agora é estado, desenhado por `VaultKeyBanner`,
   * e **`null` limpa**.
   *
   * Chamado no boot (o efeito de inicialização não passa por `loadAccounts`, e o
   * boot é justamente quando o backend descobre o problema) e depois de cada
   * recarga de contas.
   *
   * Uma resposta que chega depois de um evento do aviso (ver abaixo) é **mais
   * velha** que ele — a pergunta saiu antes — e é descartada. Sem isso, a
   * leitura de um `loadAccounts` que cruzasse com a gravação de fundo apagava a
   * faixa que o evento acabara de pôr, e ela só voltaria na mudança seguinte.
   */
  const vaultKeyWarningEvents = useRef(0);
  const refreshVaultKeyWarning = useCallback(async () => {
    const eventsBefore = vaultKeyWarningEvents.current;
    try {
      const warning = await invoke<VaultKeyWarning | null>("vault_key_warning");
      if (vaultKeyWarningEvents.current !== eventsBefore) return;
      setVaultKeyWarning(warning ?? null);
    } catch {}
  }, []);

  /**
   * O aviso também nasce em gravação **de fundo** — Auto Rejoin a cada ciclo,
   * Watcher, servidor HTTP —, e nenhuma delas passa pelas leituras acima. Com o
   * Auto Rejoin a noite inteira, o `.key` que ficava ruim de madrugada virava
   * aviso só no backend, o dono fechava o app sem ver faixa nenhuma e o boot
   * seguinte caía em lockout. O backend publica cada mudança (inclusive a que
   * resolve) neste evento; `null` limpa.
   *
   * Fica ligado sempre, inclusive nas telas de senha e de criptografia: são as
   * telas do momento de pânico, e a faixa é desenhada nelas também.
   */
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    listen<VaultKeyWarning | null>(VAULT_KEY_WARNING_EVENT, (e) => {
      vaultKeyWarningEvents.current += 1;
      setVaultKeyWarning(e.payload ?? null);
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  async function loadAccounts() {
    try {
      const result = await invoke<Account[]>("get_accounts");
      setAccounts(result);
      setError(null);
      loadAvatars(result);
      void refreshVaultKeyWarning();
    } catch (e) {
      setError(String(e));
    }
  }

  async function saveAccounts() {
    try {
      await invoke("save_accounts");
      addToast(tr("Accounts saved"));
    } catch (e) {
      setError(String(e));
    }
  }

  async function addAccountByCookie(cookie: string, password?: string) {
    try {
      const info = await invoke<{ user_id: number; name: string }>("validate_cookie", {
        cookie,
      });
      const alreadyExists = accounts.some((a) => a.UserID === info.user_id);
      // Sem senha a chamada fica exatamente como sempre foi (o `add_account`
      // recebe `Option<String>` e só troca a senha guardada quando vem uma).
      await invoke("add_account", {
        securityToken: cookie,
        username: info.name,
        userId: info.user_id,
        ...(password ? { password } : {}),
      });
      await loadAccounts();
      const { hideUsernames: hidden, hiddenNameLetters: letters } = nameMaskingRef.current;
      addToast(
        tr(alreadyExists ? "Updated {{name}}" : "Added {{name}}", {
          name: maskAccountName(info.name, hidden, letters),
        })
      );
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeAccounts(userIds: number[]) {
    try {
      for (const id of userIds) {
        await invoke("remove_account", { userId: id });
      }
      setSelectedIds((prev) => {
        const next = new Set(prev);
        userIds.forEach((id) => next.delete(id));
        return next;
      });
      await loadAccounts();
      addToast(tr("Removed {{count}} account(s)", { count: userIds.length }));
    } catch (e) {
      setError(String(e));
    }
  }

  async function updateAccount(account: Account) {
    try {
      await invoke("update_account", { account });
      setAccounts((prev) => prev.map((a) => (a.UserID === account.UserID ? account : a)));
    } catch (e) {
      setError(String(e));
    }
  }

  /**
   * Moderação de uma conta, agora (ignora o cache do backend). Leitura: nunca
   * renova a sessão. Erro (limite do Roblox, rede, cookie vencido) vira toast
   * e **não** muda o que a tela mostra — 429 nunca vira "banida".
   */
  async function checkModeration(userId: number): Promise<ModerationStatus | null> {
    try {
      const status = await invoke<ModerationStatus>("check_account_moderation", {
        userId,
        force: true,
      });
      rememberModeration(userId, status);
      return status;
    } catch (e) {
      addToast(String(e), "warn");
      return null;
    }
  }

  async function checkAccounts(userIds: number[]): Promise<AccountCheckSummary | null> {
    if (userIds.length === 0) return null;
    setAccountCheckProgress({ done: 0, total: userIds.length });
    try {
      const summary = await invoke<AccountCheckSummary>("check_accounts", { userIds });
      // `Valid` pode ter mudado no backend (401 = inválida, 200 = válida de novo).
      await loadAccounts().catch(() => {});
      const tone: ToastTone = summary.invalid > 0 || summary.banned > 0 ? "warn" : "success";
      addToast(accountCheckSummaryText(summary, tr), tone);
      return summary;
    } catch (e) {
      addToast(tr("Check failed: {{error}}", { error: String(e) }), "error");
      return null;
    } finally {
      setAccountCheckProgress(null);
    }
  }

  async function refreshCookie(userId: number): Promise<boolean> {
    const ok = await invoke<boolean>("refresh_cookie", { userId });
    await loadAccounts();
    return ok;
  }

  async function moveToGroup(userIds: number[], group: string) {
    const updated = accounts.map((a) =>
      userIds.includes(a.UserID) ? { ...a, Group: group } : a
    );
    setAccounts(updated);
    for (const a of updated) {
      if (userIds.includes(a.UserID)) {
        await invoke("update_account", { account: a }).catch(() => {});
      }
    }
    addToast(tr("Moved to {{group}}", { group: parseGroupName(group).displayName }));
  }

  function sortGroupAlphabetically(groupKey: string) {
    setAccounts((prev) => {
      const targetIndices: number[] = [];
      const targetAccounts: Account[] = [];

      prev.forEach((acc, idx) => {
        if ((acc.Group || "Default") === groupKey) {
          targetIndices.push(idx);
          targetAccounts.push(acc);
        }
      });

      if (targetAccounts.length <= 1) {
        return prev;
      }

      targetAccounts.sort((a, b) => {
        const na = a.Alias || a.Username;
        const nb = b.Alias || b.Username;
        return na.localeCompare(nb);
      });

      const next = [...prev];
      targetIndices.forEach((idx, i) => {
        next[idx] = targetAccounts[i];
      });

      invoke("reorder_accounts", {
        userIds: next.map((a) => a.UserID),
      }).catch(() => {});

      return next;
    });
    addToast(tr("Sorted {{group}}", { group: parseGroupName(groupKey).displayName }));
  }

  /**
   * Reordena grupos. Grava a lista **inteira** de grupos existentes, não só a
   * visível: com busca ativa a tela mostra um subconjunto, e gravar "o que está
   * na tela" apagaria da ordem os grupos escondidos pelo filtro.
   */
  async function reorderGroups(draggedKey: string, targetKey: string) {
    if (!draggedKey || !targetKey || draggedKey === targetKey) return;
    // `__all__` é o grupo sintético da lista sem cabeçalho: não tem o que ordenar.
    if (draggedKey === "__all__" || targetKey === "__all__") return;

    const existentes = accounts.map((a) => a.Group || "Default");
    const atual = orderGroupKeys(existentes, parseGroupOrder(settings?.General?.GroupOrder));
    const from = atual.indexOf(draggedKey);
    const to = atual.indexOf(targetKey);
    if (from < 0 || to < 0 || from === to) return;

    const proxima = [...atual];
    const [movido] = proxima.splice(from, 1);
    proxima.splice(to, 0, movido);

    // Otimista: a lista reordena na hora, sem esperar o disco.
    setSettings((prev) => ({
      ...(prev || {}),
      General: { ...(prev?.General || {}), GroupOrder: serializeGroupOrder(proxima) },
    }));
    try {
      await invoke("update_setting", {
        section: "General",
        key: "GroupOrder",
        value: serializeGroupOrder(proxima),
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function reorderAccounts(draggedUserId: number, targetUserId: number) {
    if (draggedUserId === targetUserId) return;

    const next = [...accounts];
    const from = next.findIndex((a) => a.UserID === draggedUserId);
    const to = next.findIndex((a) => a.UserID === targetUserId);
    if (from < 0 || to < 0) return;

    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);

    setAccounts(next);

    try {
      await invoke("reorder_accounts", {
        userIds: next.map((a) => a.UserID),
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function openLoginBrowser() {
    try {
      await invoke("open_login_browser");
    } catch (e) {
      setError(String(e));
    }
  }

  async function openAccountBrowser(userId: number) {
    try {
      await invoke("open_account_browser", { userId });
    } catch (e) {
      setError(String(e));
    }
  }

  async function ensureBrowserDownload(force?: boolean): Promise<boolean> {
    setBrowserDownload({ active: true, stage: "resolving", percent: null, error: null });
    try {
      await invoke("ensure_browser", { force: force === true });
      setBrowserDownload({ active: false, stage: "ready", percent: 100, error: null });
      return true;
    } catch (e) {
      setBrowserDownload({ active: false, stage: "error", percent: null, error: String(e) });
      return false;
    }
  }

  /**
   * O backend recusa um launch quando já existe uma sequência em andamento
   * (duas filas ao mesmo tempo brigariam pelo mutex do Multi Roblox, pelo
   * registro e pelo `ClientAppSettings.json`, que é global). A mensagem é a
   * mesma no launch de uma conta e no de várias — e é aqui que o código do
   * backend vira frase traduzida.
   *
   * **Só o toast.** A linha de status do rodapé mostrava a mesma frase ao mesmo
   * tempo, e a tela de Choose Game ainda devolve uma linha inline no lugar do
   * clique: eram três cópias simultâneas de uma frase de uma linha. Toast e
   * rodapé são os dois globais, então saiu o rodapé — ficam o toast (vale para
   * qualquer origem do launch, inclusive as que não têm linha inline) e a linha
   * inline (fica onde o usuário clicou).
   */
  function reportLaunchAlreadyActive() {
    addToast(tr("A launch is already in progress"), "warn");
  }

  /**
   * Launch de uma conta. Nenhum erro sobe daqui — quem chama não precisa de
   * `try/catch` —, mas o resultado **diz se começou**: a tela que anuncia
   * "seguindo com 1 conta..." em cima de um aviso de recusa ou de uma faixa
   * vermelha de erro está mentindo para o usuário.
   */
  async function joinServer(userId: number, target?: LaunchTarget): Promise<LaunchAttempt> {
    clearLaunchTimeout();
    setJoiningAccounts(new Set([userId]));
    setLaunchProgress({
      mode: "single",
      current: 1,
      total: 1,
      userId,
    });
    const launchAccount = accounts.find((a) => a.UserID === userId);
    const accountName = accountLabel(launchAccount, nameMaskingRef.current, userId);
    const launchingLine = tr("Launching {{name}}...", { name: accountName });
    setActionStatusMessage(launchingLine, "info", 5000);

    try {
      const pid = parseInt(target?.placeId ?? placeId) || 5315046213;
      const rawJobId = (target?.jobId ?? jobId).trim();
      let resolvedJobId = rawJobId;
      let joinVip = false;
      let linkCode = "";

      const parsedCode = parsePrivateServerCode(rawJobId);
      if (parsedCode) {
        // `vip:` states the intent; a pasted link only supplies the code (the
        // backend resolves the access code from it).
        joinVip = /^vip:/i.test(rawJobId);
        linkCode = parsedCode;
        resolvedJobId = "";
      }

      // An already-resolved target (e.g. a pasted join link) wins over the
      // string parsing above. Absent fields keep the parsed defaults, so
      // callers that only pass placeId/jobId behave exactly as before.
      if (target?.joinVip !== undefined) joinVip = target.joinVip;
      if (target?.linkCode !== undefined) linkCode = target.linkCode;
      if (joinVip) resolvedJobId = "";

      await invoke("launch_roblox", {
        userId,
        placeId: pid,
        jobId: resolvedJobId,
        launchData: target?.launchData ?? launchData,
        followUser: false,
        joinVip,
        linkCode,
        shuffleJob: shuffleJobId,
      });
      await loadAccounts();
      void recordRecentGame(pid, userId, parseInt(settings?.General?.MaxRecentGames || "8") || 8).catch(() => {});
      // O servidor também vira "recente": num alvo VIP o Job ID vai vazio e o
      // código viaja em `linkCode`, então guarda-se o `vip:<código>` — a forma
      // que o campo de Job ID e o `resolve_launch_job` sabem reabrir.
      const recentJob = linkCode ? `vip:${linkCode}` : resolvedJobId;
      if (recentJob) {
        // O cliente já subiu: uma escrita recusada pelo `localStorage` (cota,
        // perfil sem storage) cairia no `catch` abaixo e diria "Launch failed"
        // sobre um launch que deu certo. É síncrono, então `.catch` não serve.
        try {
          addRecentJob(recentJob, pid, parseInt(settings?.General?.MaxRecentJobs || "12") || 12, [userId]);
        } catch {
          // Guardar recentes é conveniência; nunca derruba o launch.
        }
      }
      addToast(tr("Launching game..."));
    } catch (e) {
      setJoiningAccounts((prev) => {
        const next = new Set(prev);
        next.delete(userId);
        return next;
      });
      setLaunchProgress((prev) => (prev?.mode === "single" && prev.userId === userId ? null : prev));
      if (isLaunchAlreadyActiveError(e)) {
        // Recusa, não falha: o backend não deixa duas sequências de launch
        // rodarem juntas. A faixa vermelha de erro (com "abrir o log") diria a
        // coisa errada, então isto sai como aviso — e o "Launching X…" que
        // este launch pôs no rodapé sai junto: nada está sendo lançado.
        withdrawActionStatus(launchingLine);
        reportLaunchAlreadyActive();
        return "refused";
      }
      setError(String(e));
      setActionStatusMessage(tr("Launch failed: {{error}}", { error: String(e) }), "error", 5000);
      return "failed";
    }

    launchClearTimeoutRef.current = window.setTimeout(() => {
      setJoiningAccounts((prev) => {
        const next = new Set(prev);
        next.delete(userId);
        return next;
      });
      setLaunchProgress((prev) => (prev?.mode === "single" && prev.userId === userId ? null : prev));
      launchClearTimeoutRef.current = null;
    }, 7000);
    return "started";
  }

  /**
   * Preset de launch: o backend abre pela mesma fila do `launch_multiple` e
   * anota quais clientes esta execução abriu (é o que "fechar" usa). Aqui só o
   * mesmo retorno visual do lote e os recentes.
   */
  async function launchPreset(preset: LaunchPresetView): Promise<LaunchAttempt> {
    const total = preset.userIds.length;
    clearLaunchTimeout();
    setLaunchProgress({ mode: "multi", current: 0, total, userId: preset.userIds[0] ?? null });
    const launchingLine = tr("Launching {{count}} accounts...", { count: total });
    setActionStatusMessage(launchingLine, "info", 5000);
    try {
      await invoke<number>("launch_preset", { id: preset.id });
      await loadAccounts();
      void recordRecentGame(preset.placeId, preset.userIds[0] ?? null, parseInt(settings?.General?.MaxRecentGames || "8") || 8).catch(() => {});
      setPresetsRevision((n) => n + 1);
      return "started";
    } catch (e) {
      setJoiningAccounts(new Set());
      setLaunchProgress(null);
      if (isLaunchAlreadyActiveError(e)) {
        withdrawActionStatus(launchingLine);
        reportLaunchAlreadyActive();
        return "refused";
      }
      setError(String(e));
      setActionStatusMessage(tr("Launch failed: {{error}}", { error: String(e) }), "error", 5000);
      return "failed";
    }
  }

  async function launchMultiple(userIds: number[], target?: LaunchTarget) {
    if (userIds.length === 0) return;
    if (userIds.length > 1 && platformCapabilities?.os === "linux" && !platformCapabilities.supportsMultiLaunch) {
      const message =
        platformCapabilities.reasons[0] ||
        platformCapabilities.warnings[0] ||
        "Linux multi-launch requires a compatible custom runner and experimental Multi Roblox";
      setError(message);
      setActionStatusMessage(message, "error", 5000);
      throw new Error(message);
    }

    clearLaunchTimeout();
    setJoiningAccounts(new Set([userIds[0]]));
    setLaunchProgress({
      mode: "multi",
      current: 0,
      total: userIds.length,
      userId: userIds[0],
    });
    const launchingLine = tr("Launching {{count}} accounts...", { count: userIds.length });
    setActionStatusMessage(launchingLine, "info", 5000);

    try {
      const pid = parseInt(target?.placeId ?? placeId) || 5315046213;
      const rawJobId = (target?.jobId ?? jobId).trim();
      // `launch_multiple` has no joinVip/linkCode parameters: the backend's
      // resolve_launch_job understands the `vip:<code>` job prefix instead.
      // A code can come from the resolved target OR from a link pasted into
      // the Job ID field — a single launch accepts both, so this must too.
      const explicitCode = target?.joinVip ? (target.linkCode || "").trim() : "";
      const vipCode = explicitCode || parsePrivateServerCode(rawJobId);
      await invoke("launch_multiple", {
        userIds,
        placeId: pid,
        jobId: vipCode ? `vip:${vipCode}` : rawJobId,
        launchData: target?.launchData ?? launchData,
        // Cada conta sorteia o próprio servidor público (o backend ignora o
        // shuffle quando há Job ID ou VIP).
        shuffleJob: shuffleJobId,
      });
      await loadAccounts();
      void recordRecentGame(pid, userIds[0], parseInt(settings?.General?.MaxRecentGames || "8") || 8).catch(() => {});
      // O alvo é das contas **todas** que entraram: é isso que decide para quem
      // um servidor privado volta a aparecer nos recentes.
      const recentJob = vipCode ? `vip:${vipCode}` : rawJobId;
      if (recentJob) {
        // Como no launch único, e aqui é pior: este `catch` **relança**, então
        // uma escrita recusada interromperia o que vem depois de um lote que já
        // subiu os clientes.
        try {
          addRecentJob(recentJob, pid, parseInt(settings?.General?.MaxRecentJobs || "12") || 12, userIds);
        } catch {
          // Guardar recentes é conveniência; nunca derruba o launch.
        }
      }
      addToast(tr("Launching {{count}} accounts...", { count: userIds.length }));
    } catch (e) {
      setJoiningAccounts(new Set());
      setLaunchProgress(null);
      if (isLaunchAlreadyActiveError(e)) {
        // Ver `joinServer`: recusa por sequência já em andamento é aviso, e o
        // "Launching N accounts…" deste lote sai do rodapé. O erro original é
        // relançado com o código intacto para quem chamou reconhecer (a tela de
        // Choose Game não repete o toast).
        withdrawActionStatus(launchingLine);
        reportLaunchAlreadyActive();
        throw e;
      }
      setError(String(e));
      setActionStatusMessage(tr("Launch failed: {{error}}", { error: String(e) }), "error", 5000);
      throw e;
    }
  }

  async function killAllRobloxProcesses() {
    try {
      const killed = await invoke<number>("cmd_kill_all_roblox");
      clearLaunchTimeout();
      setJoiningAccounts(new Set());
      setLaunchProgress(null);
      addToast(killed > 0
        ? tr(killed === 1 ? "Closed {{count}} Roblox process" : "Closed {{count}} Roblox processes", { count: killed })
        : tr("No open Roblox processes found"));
      setError(null);
    } catch (e) {
      setError(String(e));
      setActionStatusMessage(tr("Failed to close Roblox: {{error}}", { error: String(e) }), "error", 5000);
    }
  }

  async function identifyExternalClient(pid: number, userId: number): Promise<boolean> {
    const ok = await invoke<boolean>("identify_external_client", { pid, userId });
    await refreshRunningRef.current();
    return ok;
  }

  async function focusClientWindow(pid: number): Promise<boolean> {
    return await invoke<boolean>("focus_client_window", { pid });
  }

  async function focusRobloxClient(userId: number): Promise<boolean> {
    try {
      return await invoke<boolean>("focus_roblox_window", { userId });
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  /**
   * Fecha os clientes das contas informadas, uma a uma. Diferente de
   * `killAllRobloxProcesses`, não toca em clientes de contas fora da lista.
   */
  async function closeRobloxClients(userIds: number[]): Promise<number> {
    const uniqueIds = Array.from(new Set(userIds));
    let closed = 0;
    let failures = 0;
    for (const userId of uniqueIds) {
      try {
        if (await invoke<boolean>("cmd_kill_roblox", { userId })) closed += 1;
        else failures += 1;
      } catch {
        failures += 1;
      }
    }
    if (failures > 0) {
      addToast(
        tr("{{count}} client(s) could not be closed", { count: failures })
      );
    }
    return closed;
  }

  async function refreshLaunchQueue(): Promise<void> {
    try {
      const payload = await invoke<LaunchQueuePayload>("get_launch_queue");
      setLaunchQueue(payload ?? null);
    } catch {
      // Backend sem fila ativa (ou comando indisponível): não é erro de usuário.
    }
  }

  /**
   * Cancela a entrada de UMA conta. Regra do produto: cancelar nunca fecha um
   * cliente que já abriu — por isso aqui não há `cmd_kill_roblox`.
   */
  async function cancelAccountLaunch(userId: number): Promise<boolean> {
    const ok = await invoke<boolean>("cancel_account_launch", { userId });
    await refreshLaunchQueue();
    return ok;
  }

  /** Para a fila inteira. Também não fecha nenhum cliente já aberto. */
  async function stopLaunchQueue(): Promise<number> {
    const removed = await invoke<number>("stop_launch_queue");
    await refreshLaunchQueue();
    return removed;
  }

  async function stopAutoReconnect(userId: number): Promise<boolean> {
    return invoke<boolean>("stop_auto_reconnect", { userId });
  }

  async function retryAutoReconnect(userId: number): Promise<boolean> {
    return invoke<boolean>("retry_auto_reconnect", { userId });
  }

  async function restartRobloxClients(userIds: number[]) {
    const uniqueIds = Array.from(new Set(userIds));
    const launchedIds = uniqueIds.filter((userId) => launchedByProgram.has(userId));
    if (launchedIds.length === 0) {
      addToast(tr("No launched Roblox clients selected"));
      return;
    }

    let closeFailures = 0;
    for (const userId of launchedIds) {
      try {
        const closed = await invoke<boolean>("cmd_kill_roblox", { userId });
        if (!closed) closeFailures += 1;
      } catch {
        closeFailures += 1;
      }
    }

    await new Promise((resolve) => setTimeout(resolve, 250));
    if (closeFailures > 0) {
      addToast(tr("Some clients could not be closed before restart"));
    }

    if (launchedIds.length === 1) {
      // O que deu errado (recusa ou falha) já foi reportado pelo próprio
      // `joinServer`, e aqui não há nada a fazer com o resultado.
      await joinServer(launchedIds[0]);
      return;
    }

    try {
      await launchMultiple(launchedIds);
    } catch {
    }
  }

  async function refreshBottingStatus() {
    try {
      const status = await invoke<BottingStatus>("get_botting_mode_status");
      setBottingStatus(status);
    } catch (e) {
      setError(String(e));
    }
  }

  async function startBottingMode(config: BottingStartConfig) {
    if (platformCapabilities?.os === "linux" && !platformCapabilities.supportsBotting) {
      const message =
        platformCapabilities.reasons[0] ||
        platformCapabilities.warnings[0] ||
        "Auto Rejoin is unavailable for the active Linux runner";
      setError(message);
      throw new Error(message);
    }
    try {
      const status = await invoke<BottingStatus>("start_botting_mode", {
        userIds: config.userIds,
        placeId: config.placeId,
        jobId: config.jobId,
        launchData: config.launchData,
        playerUserIds: config.playerUserIds,
        intervalMinutes: config.intervalMinutes,
        launchDelaySeconds: config.launchDelaySeconds,
        playerGraceMinutes: config.playerGraceMinutes,
        adoptRunning: config.adoptRunning ?? false,
      });
      setBottingStatus(status);
      addToast(tr("Auto Rejoin started ({{count}} accounts)", { count: config.userIds.length }));
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  /**
   * Adota no Auto Rejoin contas que já estão jogando.
   *
   * O caminho antigo era abrir o diálogo, colar o Place ID e dar Start — e o
   * Start **fecha e relança** todo mundo, tirando as contas do servidor em que
   * já estavam. Aqui nada é fechado.
   */
  async function detectRunningGamePlace(userIds: number[]): Promise<number | null> {
    for (const id of [...new Set(userIds)].filter((it) => it > 0)) {
      try {
        const found = await invoke<AccountGameLocation>("get_account_game_location", {
          userId: id,
        });
        if (found?.inGame && found.placeId) return found.placeId;
      } catch {
        // Presença indisponível para esta conta; tenta a próxima.
      }
    }
    return null;
  }

  async function adoptRunningIntoBotting(userIds: number[], options: AdoptBottingOptions = {}) {
    const ids = [...new Set(userIds)].filter((id) => id > 0);
    if (ids.length === 0) return;

    // Já existe sessão: `add_botting_accounts` adota sem relançar.
    if (bottingStatus?.active) {
      await addBottingAccounts(ids);
      return;
    }

    if (ids.length < 2) {
      const message = tr(
        "Auto Rejoin needs at least two accounts. Select another one, or start the cycle from the Auto Rejoin dialog."
      );
      addToast(message);
      throw new Error(message);
    }

    // O place vem de onde a conta ESTÁ, não do campo da tela principal: com o
    // place errado, o primeiro reinício do ciclo a jogaria em outro jogo. Quem
    // passa `placeId` é a tela do Modo AFK, que mostrou esse place (detectado
    // pela mesma presença, ou digitado) antes do Start.
    const placeId =
      options.placeId && options.placeId > 0 ? options.placeId : await detectRunningGamePlace(ids);

    if (!placeId) {
      const message = tr(
        "Could not tell which game these accounts are in. Open the Auto Rejoin dialog and set the Place ID."
      );
      addToast(message);
      throw new Error(message);
    }

    const general = settings?.General || {};
    await startBottingMode({
      userIds: ids,
      placeId,
      // O job fica de fora de propósito: o ciclo relança no place, e fixar o
      // servidor atual mandaria todo reinício para um servidor que pode não
      // existir mais.
      jobId: "",
      launchData: "",
      playerUserIds: (options.playerUserIds ?? []).filter((id) => ids.includes(id)),
      intervalMinutes:
        options.intervalMinutes ??
        (parseInt(general.BottingDefaultIntervalMinutes || "19", 10) || 19),
      launchDelaySeconds:
        options.launchDelaySeconds ??
        (parseInt(general.BottingLaunchDelaySeconds || "20", 10) || 20),
      playerGraceMinutes:
        options.playerGraceMinutes ??
        (parseInt(general.BottingPlayerGraceMinutes || "15", 10) || 15),
      adoptRunning: true,
    });
  }

  async function stopBottingMode(closeBotAccounts: boolean) {
    try {
      await invoke("stop_botting_mode", { closeBotAccounts });
      await refreshBottingStatus();
      addToast(tr(closeBotAccounts
        ? "Auto Rejoin stopped and alt accounts closed"
        : "Auto Rejoin stopped"));
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function addBottingAccounts(userIds: number[]) {
    if (userIds.length === 0) return;
    try {
      const status = await invoke<BottingStatus>("add_botting_accounts", {
        userIds,
      });
      setBottingStatus(status);
      addToast(
        tr(
          userIds.length === 1
            ? "Added {{count}} account to Auto Rejoin"
            : "Added {{count}} accounts to Auto Rejoin",
          { count: userIds.length }
        )
      );
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function setBottingPlayerAccounts(userIds: number[]) {
    try {
      const status = await invoke<BottingStatus>("set_botting_player_accounts", {
        playerUserIds: userIds,
      });
      setBottingStatus(status);
      addToast(tr(userIds.length === 0 ? "Main accounts cleared" : "Main accounts updated"));
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function bottingAccountAction(
    userId: number,
    action: "disconnect" | "close" | "closeDisconnect" | "restartClient" | "restartLoop"
  ) {
    try {
      const status = await invoke<BottingStatus>("botting_account_action", {
        userId,
        action,
      });
      setBottingStatus(status);
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function refreshAfkStatus() {
    try {
      const status = await invoke<AfkStatus>("get_afk_mode_status");
      setAfkStatus(status);
    } catch (e) {
      setError(String(e));
    }
  }

  async function startAfkMode(config: AfkStartConfig) {
    try {
      const status = await invoke<AfkStatus>("start_afk_mode", {
        userIds: config.userIds,
        intervalSeconds: config.intervalSeconds,
        key: config.key,
        mode: config.mode,
        clickX: config.clickX,
        clickY: config.clickY,
      });
      setAfkStatus(status);
      addToast(
        config.userIds.length === 1
          ? tr("AFK mode started for 1 account")
          : tr("AFK mode started for {{count}} accounts", { count: config.userIds.length })
      );
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function stopAfkMode() {
    try {
      await invoke("stop_afk_mode");
      await refreshAfkStatus();
      addToast(tr("AFK mode off"));
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function afkTriggerNow(userIds: number[]): Promise<number> {
    try {
      return await invoke<number>("afk_trigger_now", { userIds });
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  // Sem `setError`: o erro é um código, e quem escreve a frase é a tela.
  function captureAfkPoint(): Promise<AfkCapturedPoint> {
    return invoke<AfkCapturedPoint>("afk_capture_point");
  }

  async function setAfkAccounts(userIds: number[]) {
    try {
      const status = await invoke<AfkStatus>("set_afk_accounts", { userIds });
      setAfkStatus(status);
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function refreshGeneratorStatus() {
    try {
      const status = await invoke<GeneratorStatus>("get_generator_status");
      setGeneratorStatus(status);
    } catch (e) {
      setError(String(e));
    }
  }

  async function startGenerator(config: GeneratorStartConfig) {
    try {
      const status = await invoke<GeneratorStatus>("start_generator", {
        provider: config.provider,
        endpoint: config.endpoint,
        apiKey: config.apiKey,
        accountType: config.accountType,
        extraDelaySeconds: config.extraDelaySeconds,
        targetGroup: config.targetGroup,
        maxAccounts: config.maxAccounts,
      });
      setGeneratorStatus(status);
      addToast(tr("Account generator started"));
      return status;
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function stopGenerator() {
    try {
      await invoke("stop_generator");
      await refreshGeneratorStatus();
      addToast(tr("Account generator stopped"));
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  const applyThemePreview = useCallback((nextTheme: ThemeData) => {
    const normalized = normalizeTheme(nextTheme);
    setThemeState(normalized);
    applyThemeCssVariables(normalized);
  }, []);

  const saveTheme = useCallback(async (nextTheme: ThemeData) => {
    const normalized = normalizeTheme(nextTheme);
    await invoke("update_theme", { theme: normalized });
    setThemeState(normalized);
    applyThemeCssVariables(normalized);
  }, []);

  async function updateSetting(section: string, key: string, value: string) {
    let previous: string | undefined;
    setSettings((prev) => {
      previous = prev?.[section]?.[key];
      return { ...(prev || {}), [section]: { ...(prev?.[section] || {}), [key]: value } };
    });
    try {
      await invoke("update_setting", { section, key, value });
    } catch (e) {
      setSettings((prev) => {
        const rest = { ...(prev?.[section] || {}) };
        if (previous === undefined) delete rest[key];
        else rest[key] = previous;
        return { ...(prev || {}), [section]: rest };
      });
      addToast(tr("Could not save the setting: {{error}}", { error: String(e) }), "error");
    }
  }

  async function reloadSettings() {
    try {
      const s = await invoke<Record<string, Record<string, string>>>("get_all_settings");
      setSettings(s);
      void i18n.changeLanguage(normalizeLanguage(s?.General?.Language));
      try {
        const capabilities = await invoke<PlatformCapabilities>("get_platform_capabilities");
        setPlatformCapabilities(capabilities);
      } catch {
      }
    } catch {}
  }

  const refreshPlatformCapabilities = useCallback(async () => {
    try {
      const capabilities = await invoke<PlatformCapabilities>("get_platform_capabilities");
      setPlatformCapabilities(capabilities);
    } catch {
    }
  }, []);

  const refreshEncryptionState = useCallback(async () => {
    try {
      const encrypted = await invoke<boolean>("is_accounts_encrypted");
      setAccountsEncrypted(encrypted);
    } catch {}
  }, []);

  const openEncryptionSetupFromSettings = useCallback(() => {
    setEncryptionSetupError(null);
    setEncryptionSetupMode("settings");
    setEncryptionSetupOpen(true);
    void refreshEncryptionState();
  }, [refreshEncryptionState]);

  const closeEncryptionSetup = useCallback(() => {
    if (encryptionSetupMode === "firstRun") return;
    setEncryptionSetupOpen(false);
    setEncryptionSetupError(null);
  }, [encryptionSetupMode]);

  const openFirstRunWalkthroughFromSettings = useCallback(() => {
    if (walkthroughOpenTimeoutRef.current !== null) {
      window.clearTimeout(walkthroughOpenTimeoutRef.current);
      walkthroughOpenTimeoutRef.current = null;
    }
    // O tour começa na lista de contas (é lá que ficam o Add e a lista), de
    // qualquer página que a Ajuda tenha sido clicada.
    setActivePageState("accounts");
    setFirstRunWalkthroughMode("manual");
    setFirstRunWalkthroughOpen(false);
    walkthroughOpenTimeoutRef.current = window.setTimeout(() => {
      setFirstRunWalkthroughOpen(true);
      walkthroughOpenTimeoutRef.current = null;
    }, 120);
  }, []);

  const closeFirstRunWalkthrough = useCallback(() => {
    if (walkthroughOpenTimeoutRef.current !== null) {
      window.clearTimeout(walkthroughOpenTimeoutRef.current);
      walkthroughOpenTimeoutRef.current = null;
    }
    setFirstRunWalkthroughOpen(false);
    setFirstRunWalkthroughMode("manual");
  }, []);

  const setDefaultVersion = useCallback(
    async (versionId: string | null) => {
      const value = versionId ?? "";
      let previous = "";
      setSettings((prev) => {
        if (!prev) return prev;
        previous = prev.Versions?.DefaultVersion ?? "";
        return {
          ...prev,
          Versions: {
            ...(prev.Versions || {}),
            DefaultVersion: value,
          },
        };
      });
      try {
        await invoke("versions_set_default", { versionId });
      } catch (e) {
        setSettings((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            Versions: {
              ...(prev.Versions || {}),
              DefaultVersion: previous,
            },
          };
        });
        addToast(tr("Failed to set version: {{error}}", { error: String(e) }));
      }
    },
    [addToast]
  );

  const setFirstRunWalkthroughStateLocal = useCallback((state: "completed" | "skipped") => {
    setSettings((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        General: {
          ...(prev.General || {}),
          FirstRunWalkthroughState: state,
        },
      };
    });
  }, []);

  const persistFirstRunWalkthroughState = useCallback(async (state: "completed" | "skipped") => {
    await invoke("update_setting", {
      section: "General",
      key: "FirstRunWalkthroughState",
      value: state,
    }).catch(() => {});
  }, []);

  const completeFirstRunWalkthrough = useCallback(async () => {
    const shouldPersist =
      firstRunWalkthroughMode === "firstRun" ||
      settings?.General?.FirstRunWalkthroughState === "pending";

    if (shouldPersist) {
      setFirstRunWalkthroughStateLocal("completed");
    }
    setFirstRunWalkthroughOpen(false);

    if (shouldPersist) {
      await persistFirstRunWalkthroughState("completed");
      addToast(tr("First-Time Walkthrough complete"));
    }
    setFirstRunWalkthroughMode("manual");
  }, [
    addToast,
    firstRunWalkthroughMode,
    persistFirstRunWalkthroughState,
    setFirstRunWalkthroughStateLocal,
    settings?.General?.FirstRunWalkthroughState,
  ]);

  const skipFirstRunWalkthrough = useCallback(async () => {
    const shouldPersist =
      firstRunWalkthroughMode === "firstRun" ||
      settings?.General?.FirstRunWalkthroughState === "pending";

    if (shouldPersist) {
      setFirstRunWalkthroughStateLocal("skipped");
    }
    setFirstRunWalkthroughOpen(false);

    if (shouldPersist) {
      await persistFirstRunWalkthroughState("skipped");
      addToast(tr("First-Time Walkthrough skipped"));
    }
    setFirstRunWalkthroughMode("manual");
  }, [
    addToast,
    firstRunWalkthroughMode,
    persistFirstRunWalkthroughState,
    setFirstRunWalkthroughStateLocal,
    settings?.General?.FirstRunWalkthroughState,
  ]);

  const applyEncryptionMethod = useCallback(async (method: "default" | "password", password?: string) => {
    setApplyingEncryption(true);
    setEncryptionSetupError(null);
    setError(null);
    try {
      if (method === "password") {
        await invoke("set_encryption_password", {
          password: password ?? "",
        });
      } else {
        await invoke("set_encryption_password", { password: null });
      }

      await Promise.all([
        invoke("update_setting", {
          section: "General",
          key: "EncryptionOnboardingState",
          value: "completed",
        }),
        invoke("update_setting", {
          section: "General",
          key: "EncryptionMethod",
          value: method,
        }),
      ]);

      setSettings((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          General: {
            ...(prev.General || {}),
            EncryptionOnboardingState: "completed",
            EncryptionMethod: method,
          },
        };
      });

      await refreshEncryptionState();
      await loadAccounts();
      setEncryptionSetupOpen(false);
      setEncryptionSetupMode("settings");
      if (
        encryptionSetupMode === "firstRun" &&
        (settings?.General?.FirstRunWalkthroughState ?? "pending") === "pending"
      ) {
        setFirstRunWalkthroughMode("firstRun");
        setFirstRunWalkthroughOpen(true);
      }
      addToast(method === "password" ? tr("Password lock enabled") : tr("Default encryption enabled"));
    } catch (e) {
      setEncryptionSetupError(String(e));
      // **Também no erro.** O caminho que falha é justamente o que pode ter
      // deixado um aviso novo (chave que não pôde ser criada), e antes o aviso só
      // era lido no sucesso — então ele só apareceria no próximo boot, depois de o
      // usuário já ter fechado o app achando que era só "deu erro, tento outra
      // vez".
      await refreshVaultKeyWarning();
      throw e;
    } finally {
      setApplyingEncryption(false);
    }
  }, [
    addToast,
    encryptionSetupMode,
    loadAccounts,
    refreshEncryptionState,
    refreshVaultKeyWarning,
    settings?.General?.FirstRunWalkthroughState,
  ]);

  async function unlock(password: string, rememberHours?: number) {
    setUnlocking(true);
    setError(null);
    try {
      await invoke("unlock_accounts", { password, rememberHours: rememberHours ?? null });
      setNeedsPassword(false);
      await loadAccounts();
      await refreshEncryptionState();
    } catch (e) {
      setError(String(e));
    } finally {
      setUnlocking(false);
    }
  }

  useEffect(() => {
    (async () => {
      // Apply defaults immediately so the UI has a consistent baseline while we load persisted theme.
      applyThemePreview(DEFAULT_THEME);
      let needs = false;
      let loadedAccounts: Account[] = [];
      let accountsLoaded = false;
      try {
        needs = await invoke<boolean>("needs_password");
        // Senha lembrada (e ainda no prazo) destranca sem mostrar a tela.
        if (needs) {
          try {
            if (await invoke<boolean>("try_remembered_unlock")) needs = false;
          } catch {}
        }
        setNeedsPassword(needs);
        if (!needs) {
          loadedAccounts = await invoke<Account[]>("get_accounts");
          setAccounts(loadedAccounts);
          setError(null);
          loadAvatars(loadedAccounts);
          accountsLoaded = true;
        }
      } catch (e) {
        setError(String(e));
      }

      // **No boot, e fora do try acima.** O backend descobre o problema com o
      // `AccountData.key` durante o `load()` do startup, e este efeito não passa
      // por `loadAccounts` — chama `get_accounts` direto. Sem esta linha o aviso
      // só apareceria depois de uma mutação, e quem usa a chave do aparelho (o
      // único afetado) pode passar a sessão inteira sem fazer nenhuma.
      await refreshVaultKeyWarning();

      let loadedSettings: Record<string, Record<string, string>> | null = null;
      try {
        const s = await invoke<Record<string, Record<string, string>>>("get_all_settings");
        loadedSettings = s;
        setSettings(s);
        try {
          const capabilities = await invoke<PlatformCapabilities>("get_platform_capabilities");
          setPlatformCapabilities(capabilities);
        } catch {
        }
        void i18n.changeLanguage(normalizeLanguage(s?.General?.Language));
        if (s?.General?.HideUsernames === "true") setHideUsernamesState(true);
        if (s?.General?.ShuffleJobId === "true") setShuffleJobId(true);
        _setServerPreference(normalizeServerPreference(s?.General?.ServerPreference));
        _setServerRegionFilter((s?.General?.ServerRegionFilter || "").trim().toUpperCase());
        _setServerScanPages(normalizeServerScanPages(Number(s?.General?.ServerScanPages)));
        if (s?.General?.SavedPlaceId) _setPlaceId(s.General.SavedPlaceId);
        if (s?.General?.SavedJobId) _setJobId(s.General.SavedJobId);
        if (s?.General?.SavedLaunchData) _setLaunchData(s.General.SavedLaunchData);
      } catch {}

      await refreshEncryptionState();

      if (
        !needs &&
        accountsLoaded &&
        loadedAccounts.length === 0 &&
        loadedSettings?.General?.EncryptionOnboardingState === "pending"
      ) {
        setEncryptionSetupError(null);
        setEncryptionSetupMode("firstRun");
        setEncryptionSetupOpen(true);
      }

      if (
        !needs &&
        loadedSettings?.General?.FirstRunWalkthroughState === "pending" &&
        loadedSettings?.General?.EncryptionOnboardingState !== "pending"
      ) {
        setFirstRunWalkthroughMode("firstRun");
        setFirstRunWalkthroughOpen(true);
      }

      try {
        const t = await invoke<ThemeData>("get_theme");
        applyThemePreview(t);
      } catch {}
      setInitialized(true);
    })();
  }, [applyThemePreview, refreshEncryptionState]);

  useEffect(() => {
    if (!initialized || needsPassword) return;
    if (encryptionSetupOpen || firstRunWalkthroughOpen) return;
    if (settings?.General?.FirstRunWalkthroughState !== "pending") return;
    if (settings?.General?.EncryptionOnboardingState === "pending") return;
    setFirstRunWalkthroughMode("firstRun");
    setFirstRunWalkthroughOpen(true);
  }, [
    encryptionSetupOpen,
    firstRunWalkthroughOpen,
    initialized,
    needsPassword,
    settings?.General?.EncryptionOnboardingState,
    settings?.General?.FirstRunWalkthroughState,
  ]);

  useEffect(() => {
    void i18n.changeLanguage(normalizeLanguage(settings?.General?.Language));
  }, [settings?.General?.Language]);

  useEffect(() => {
    if (!initialized) return;
    void refreshPlatformCapabilities();
  }, [
    initialized,
    refreshPlatformCapabilities,
    settings?.Linux?.PreferredRunner,
    settings?.Linux?.CustomLaunchCommand,
    settings?.Linux?.CustomProcessMatch,
    settings?.Linux?.CustomLogDir,
    settings?.Linux?.EnableExperimentalMultiRbx,
    settings?.Linux?.WindowControlBackend,
  ]);

  useEffect(() => {
    if (!theme) return;
    const normalized = normalizeTheme(theme);
    const themedNavbar = settings?.General?.ThemeWindowsNavbar === "true";
    const style = document.documentElement.style;

    if (themedNavbar) {
      style.setProperty("--titlebar-bg", normalized.forms_background);
      style.setProperty("--titlebar-fg", normalized.forms_foreground);
    } else {
      style.setProperty("--titlebar-bg", normalized.dark_top_bar ? "#09090b" : normalized.forms_background);
      style.setProperty("--titlebar-fg", normalized.dark_top_bar ? "#a1a1aa" : normalized.forms_foreground);
    }
  }, [theme, settings?.General?.ThemeWindowsNavbar]);

  useEffect(() => {
    if (!initialized) return;
    void invoke("sync_windows_navbar_theme").catch(() => {});
  }, [initialized, settings?.General?.ThemeWindowsNavbar, theme?.dark_top_bar]);

  useEffect(() => {
    const unlisten = listen("browser-login-detected", async () => {
      let cookie = "";
      for (let i = 0; i < 8; i++) {
        try {
          cookie = await invoke<string>("extract_browser_cookie");
          if (cookie.trim().length > 0) break;
        } catch {}
        await new Promise((r) => setTimeout(r, 350));
      }

      if (cookie.trim().length === 0) {
        // A frase antiga ("No .ROBLOSECURITY cookie found after login...") não
        // casava com nenhum marcador de `toneFromMessage`, então uma falha de
        // login saía cinza de `info`. O "Login failed" à frente é o que dá o
        // tom — em inglês, em português ("Falha") e em alemão ("Fehler").
        addToast("Login failed: no .ROBLOSECURITY cookie found. Please try again.");
        await invoke("close_login_browser").catch(() => {});
        return;
      }

      await addAccountByCookie(cookie);
      await invoke("close_login_browser").catch(() => {});
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    // O backend também emite este evento durante um login/browser normal
    // (download automático na primeira vez), não só pelo botão de Settings —
    // por isso `browserDownload` fica de fora da dependência: ele só reflete o
    // que chegou pelo evento, nunca reinicia o listener.
    let lastPercent = -1;
    const unlisten = listen<{ stage: string; downloaded: number; total: number }>(
      "chromium-download-progress",
      (e) => {
        const { stage, downloaded, total } = e.payload;
        if (stage === "resolving") {
          lastPercent = -1;
          setBrowserDownload({ active: true, stage: "resolving", percent: null, error: null });
        } else if (stage === "downloading") {
          const pct = total > 0 ? Math.round((downloaded / total) * 100) : 0;
          setActionStatusMessage(tr("Downloading browser ({{percent}}%)", { percent: pct }), "info", 4000);
          // Um `chromium-download-progress` por bloco de ~2MB baixado geraria
          // uma re-render por bloco; sem o dedupe, uma conexão rápida virava
          // uma barra de progresso "tremendo" em vez de andar suave.
          if (pct === lastPercent) return;
          lastPercent = pct;
          setBrowserDownload({ active: true, stage: "downloading", percent: pct, error: null });
        } else if (stage === "extracting") {
          setActionStatusMessage(tr("Preparing browser..."), "info", 4000);
          setBrowserDownload({ active: true, stage: "extracting", percent: null, error: null });
        } else if (stage === "ready") {
          setActionStatusMessage(tr("Browser ready"), "success", 3000);
          setBrowserDownload({ active: false, stage: "ready", percent: 100, error: null });
        } else if (stage === "error") {
          // O texto do erro vem só do `catch` de `ensureBrowserDownload`
          // (`invoke` rejeita com a mensagem do backend); este evento é
          // disparado antes disso e não carrega o texto, então preserva o que
          // já estava guardado em vez de apagar.
          setBrowserDownload((prev) => ({
            active: false,
            stage: "error",
            percent: null,
            error: prev?.error ?? null,
          }));
        }
      }
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setActionStatusMessage]);

  useEffect(() => {
    // Disparado por `resolve_browser_binary` quando o download falhou (ou
    // ninguém baixou nada ainda) e um Chrome/Edge/Chromium/Brave instalado foi
    // usado no lugar. O usuário continua conseguindo logar; só o navegador por
    // trás é outro, com flags e versão fora do nosso controle.
    const unlisten = listen<{ browser: string; error: string }>("chromium-fallback", (e) => {
      addToast(
        tr(
          "Browser download failed, using {{browser}} instead. You can point Settings > General > Login Browser at your own copy, or retry the download.",
          { browser: e.payload.browser }
        ),
        "warn"
      );
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [addToast]);

  useEffect(() => {
    if (needsPassword || !initialized) return;

    const unsubs: Array<() => void> = [];

    const listeners = [
      listen<{ userId: number | null; level: LaunchLogLevel; step: string; message: string }>(
        "launch-log",
        (e) => {
          const p = e.payload;
          setLaunchLogs((prev) => {
            const entry: LaunchLogEntry = {
              id: launchLogIdRef.current++,
              userId: p.userId ?? null,
              level: p.level ?? "info",
              step: p.step ?? "",
              message: p.message ?? "",
              ts: Date.now(),
            };
            // Cap the buffer so a long botting session can't grow unbounded.
            const next = prev.length >= 500 ? prev.slice(prev.length - 499) : prev;
            return [...next, entry];
          });
        }
      ),
      // Queda lida do log do Roblox (commands/client_health.rs): vale com o
      // Watcher desligado também — só avisa, não fecha nada.
      listen<{ userId: number; drop?: ClientDrop | null; notResponding?: boolean }>("roblox-client-health", (e) => {
        const { userId, drop, notResponding } = e.payload;
        void refreshRunningRef.current();
        if (!drop && !notResponding) return;
        const status = clientHealthLabel({ pid: 0, logFound: true, drop: drop ?? null, notResponding }, tr);
        if (!status) return;
        const acct = accountsRef.current.find((a) => a.UserID === userId);
        const name = accountLabel(acct, nameMaskingRef.current, userId);
        addToast(tr("{{name}} in Roblox — {{status}}", { name, status: status.label }), "warn");
      }),
      // Presets (ideia 13). O resultado de quem clicou já aparece onde o clique
      // foi; o que vem do horário precisa de um toast, porque ninguém clicou.
      listen<LaunchPresetEvent>("launch-preset", (e) => {
        const p = e.payload;
        setPresetsRevision((n) => n + 1);
        if (!p.scheduled) return;
        if (!p.ok) {
          addToast(tr("Preset {{name}} failed: {{error}}", { name: p.name, error: p.error ?? "" }), "error");
        } else if (p.action === "open") {
          addToast(tr("Preset {{name}} opened {{count}} account(s) on schedule", { name: p.name, count: p.count }), "success");
        } else {
          addToast(tr("Preset {{name}} closed {{count}} window(s) on schedule", { name: p.name, count: p.count }), "info");
        }
      }),
      // Moderação lida pelo backend (painel, "conferir contas", antes do launch).
      listen<{ userId: number; status: ModerationStatus }>("account-moderation", (e) => {
        if (e.payload?.status) rememberModeration(e.payload.userId, e.payload.status);
      }),
      listen<{ done: number; total: number }>("account-check-progress", (e) => {
        const { done, total } = e.payload ?? { done: 0, total: 0 };
        // Só atualiza um check em andamento: um evento atrasado depois do fim
        // não pode reacender o "Checking…".
        setAccountCheckProgress((prev) => (prev ? { done, total } : prev));
      }),
      listen<{ userId: number; group?: string }>("account-moderated", (e) => {
        const userId = e.payload.userId;
        // The backend already moved the account into the "moderadas" group and
        // persisted it — reload so the list reflects the new grouping right away.
        void loadAccounts().catch(() => {});
        const acct = accountsRef.current.find((a) => a.UserID === userId);
        const name = accountLabel(acct, nameMaskingRef.current, userId);
        addToast(tr("{{name}} is moderated — moved to 'moderadas'", { name }));
      }),
      listen<{ userId: number; index: number; total: number }>("launch-progress", (e) => {
        const current = (e.payload.index ?? 0) + 1;
        const total = Math.max(1, e.payload.total ?? 1);
        const userId = e.payload.userId ?? null;
        setLaunchProgress({
          mode: "multi",
          current: Math.min(current, total),
          total,
          userId,
        });
        setJoiningAccounts(userId !== null ? new Set([userId]) : new Set());
        setActionStatusMessage(tr("Launching account {{current}}/{{total}}...", { current, total }), "info", 2000);
      }),
      listen("launch-complete", () => {
        setJoiningAccounts(new Set());
        setLaunchProgress((prev) => {
          if (!prev) return null;
          return {
            ...prev,
            current: prev.total,
          };
        });
        setActionStatusMessage(tr("Launch sequence complete"), "success", 3000);
        clearLaunchTimeout();
        launchClearTimeoutRef.current = window.setTimeout(() => {
          setLaunchProgress((prev) => (prev?.mode === "multi" ? null : prev));
          launchClearTimeoutRef.current = null;
        }, 1500);
      }),
      // The launcher installs a new Roblox production build by itself (so the
      // Roblox installer never runs and closes the other clients); a big
      // download would otherwise look like a frozen launch.
      listen<{ version: string; stage: string; current: number; total: number; message?: string | null }>(
        "roblox-build-install",
        (e) => {
          const { stage, current, total, message } = e.payload || ({} as never);
          if (stage === "starting" || stage === "resolving") {
            setActionStatusMessage(tr("Downloading the new Roblox version..."), "info", 60000);
          } else if (stage === "installing" && total > 0) {
            setActionStatusMessage(
              tr("Downloading the new Roblox version... {{current}}/{{total}}", { current, total }),
              "info",
              60000
            );
          } else if (stage === "ready") {
            setActionStatusMessage(tr("New Roblox version installed"), "success", 4000);
          } else if (stage === "error") {
            addToast(tr("Could not install the Roblox version: {{error}}", { error: message || "" }));
          }
        }
      ),
      listen<OptimizationWarningPayload>("roblox-optimization-warning", (e) => {
        const pid = typeof e.payload?.pid === "number" ? e.payload.pid : null;
        const message =
          typeof e.payload?.message === "string" && e.payload.message.trim().length > 0
            ? e.payload.message.trim()
            : tr("Unknown");
        addToast(
          pid !== null
            ? tr("Optimization warning for PID {{pid}}: {{message}}", { pid, message })
            : tr("Optimization warning: {{message}}", { message })
        );
      }),
    ];

    // listen() resolves asynchronously: if cleanup already ran, unsubscribe
    // immediately instead of leaking a duplicate handler.
    let disposed = false;
    Promise.all(listeners)
      .then((fns) => (disposed ? fns.forEach((fn) => fn()) : fns.forEach((fn) => unsubs.push(fn))))
      .catch(() => {});

    return () => {
      disposed = true;
      unsubs.forEach((fn) => fn());
    };
  }, [needsPassword, initialized, setActionStatusMessage, addToast]);

  useEffect(() => {
    if (needsPassword || !initialized) return;

    refreshBottingStatus();
    refreshGeneratorStatus();
    refreshAfkStatus();
    // A lista de teclas do AFK mode é do backend: a tela oferece exatamente o
    // que ele aceita, em vez de manter uma segunda lista que sai do lugar.
    invoke<string[]>("get_afk_keys")
      .then((keys) => setAfkKeys(keys))
      .catch(() => {});
    const unsubs: Array<() => void> = [];
    const listeners = [
      listen<BottingStatus>("botting-status", (e) => {
        setBottingStatus(e.payload);
      }),
      listen<AfkStatus>("afk-status", (e) => {
        setAfkStatus(e.payload);
      }),
      // Ciclo concluído: o bipe é opcional e explica o piscar de foco que o
      // usuário acabou de ver. A chave é lida na hora porque este ouvinte é
      // montado uma vez e leria um `settings` velho do closure.
      listen<{ sent?: number }>("afk-cycle", async (e) => {
        if ((e.payload?.sent ?? 0) <= 0) return;
        try {
          const enabled = await invoke<string | null>("get_setting", {
            section: "Afk",
            key: "BeepOnCycle",
          });
          if (enabled === "true") playAfkBeep();
        } catch {
          // Sem som é perda aceitável; nada a mostrar na tela.
        }
      }),
      listen("afk-stopped", () => {
        setAfkStatus((prev) => (prev ? { ...prev, active: false, accounts: [] } : prev));
      }),
      listen<GeneratorStatus>("generator-status", (e) => {
        setGeneratorStatus(e.payload);
      }),
      listen("generator-account-added", () => {
        loadAccounts();
      }),
      listen("generator-stopped", () => {
        setGeneratorStatus((prev) => (prev ? { ...prev, active: false } : prev));
      }),
      listen("botting-stopped", () => {
        setBottingStatus((prev) =>
          prev
            ? { ...prev, active: false }
            : {
                active: false,
                startedAtMs: null,
                placeId: 0,
                jobId: "",
                launchData: "",
                intervalMinutes: 19,
                launchDelaySeconds: 20,
                playerGraceMinutes: 15,
                playerUserIds: [],
                userIds: [],
                accounts: [],
              }
        );
      }),
      listen<{ userId?: number; ok?: boolean; error?: string | null }>("botting-account-cycle", (e) => {
        const uid = e.payload?.userId;
        const ok = e.payload?.ok;
        if (typeof uid === "number" && ok === false) {
          const errorText =
            typeof e.payload?.error === "string" && e.payload.error.trim().length > 0
              ? `: ${e.payload.error}`
              : "";
          setActionStatusMessage(
            `${tr("Auto Rejoin failed for {{userId}}", { userId: uid })}${errorText}`,
            "warn",
            3500
          );
        }
      }),
    ];
    // listen() resolves asynchronously: if cleanup already ran, unsubscribe
    // immediately instead of leaking a duplicate handler.
    let disposed = false;
    Promise.all(listeners)
      .then((fns) => (disposed ? fns.forEach((fn) => fn()) : fns.forEach((fn) => unsubs.push(fn))))
      .catch(() => {});

    return () => {
      disposed = true;
      unsubs.forEach((fn) => fn());
    };
  }, [needsPassword, initialized, setActionStatusMessage]);

  useEffect(() => {
    if (needsPassword || !initialized) return;

    // Fila de launch: estado inicial + evento. O Painel de Sessão (Console e
    // diálogo da barra) lê daqui, então os dois mostram sempre a mesma coisa.
    let disposed = false;
    const unsubs: Array<() => void> = [];
    invoke<LaunchQueuePayload>("get_launch_queue")
      .then((payload) => {
        if (!disposed) setLaunchQueue(payload ?? null);
      })
      .catch(() => {});
    // listen() resolve de forma assíncrona: se o cleanup já rodou, desinscreve
    // na hora em vez de deixar um handler duplicado vivo.
    listen<LaunchQueuePayload>("launch-queue", (e) => {
      setLaunchQueue(e.payload ?? null);
    })
      .then((fn) => (disposed ? fn() : unsubs.push(fn)))
      .catch(() => {});

    // Make Friends: mesmo par (retrato inicial + evento), pelo mesmo motivo.
    invoke<FriendLinkState>("get_friend_link_state")
      .then((payload) => {
        if (!disposed) setFriendLinkState(payload ?? null);
      })
      .catch(() => {});
    listen<FriendLinkState>("friend-link-state", (e) => {
      setFriendLinkState(e.payload ?? null);
    })
      .then((fn) => (disposed ? fn() : unsubs.push(fn)))
      .catch(() => {});

    // Reconexão automática (commands/reconnect.rs): mesmo par. Avisa quando
    // uma conta desiste, para de vez ou volta ao jogo depois de relançada.
    // Só aceita a lista de verdade: um `[]` no lugar do objeto tem `.entries`
    // (o método do Array), e uma função no setState vira updater do React.
    const entriesOf = (payload: AutoReconnectPayload | null | undefined): AutoReconnectEntry[] =>
      Array.isArray(payload?.entries) ? payload.entries : [];
    invoke<AutoReconnectPayload>("get_auto_reconnect_status")
      .then((payload) => {
        if (disposed) return;
        autoReconnectRef.current = entriesOf(payload);
        setAutoReconnect(autoReconnectRef.current);
      })
      .catch(() => {});
    listen<AutoReconnectPayload>("auto-reconnect", (e) => {
      const next = entriesOf(e.payload);
      const before = new Map(autoReconnectRef.current.map((entry) => [entry.userId, entry]));
      autoReconnectRef.current = next;
      setAutoReconnect(next);
      const nameOf = (userId: number) =>
        accountLabel(accountsRef.current.find((a) => a.UserID === userId), nameMaskingRef.current, userId);
      for (const entry of next) {
        const previous = before.get(entry.userId);
        const final = entry.phase === "gaveUp" || entry.phase === "stopped";
        if (!final || previous?.phase === entry.phase) continue;
        const status = autoReconnectLabel(entry, Date.now(), tr).label;
        addToast(tr("Auto-reconnect ({{name}}) — {{status}}", { name: nameOf(entry.userId), status }), "warn");
      }
      // Relançada e conferida: ficou no jogo.
      for (const userId of Array.isArray(e.payload?.reconnected) ? e.payload.reconnected : []) {
        addToast(tr("{{name}} is back in the game", { name: nameOf(userId) }), "success");
      }
    })
      .then((fn) => (disposed ? fn() : unsubs.push(fn)))
      .catch(() => {});

    return () => {
      disposed = true;
      unsubs.forEach((fn) => fn());
    };
  }, [needsPassword, initialized]);

  useEffect(() => {
    if (needsPassword || !initialized) return;

    let cancelled = false;
    const refreshRunningInstances = async () => {
      try {
        const rows = await invoke<RunningInstanceEntry[]>("get_running_instances");
        const next = new Set<number>();
        const adopted = new Set<number>();
        const health = new Map<number, ClientHealth>();
        const memory = new Map<number, ClientMemory>();
        for (const row of rows) {
          const userId = row.userId ?? row.user_id;
          if (typeof userId === "number") {
            next.add(userId);
            if (row.adopted) adopted.add(userId);
            if (row.health) health.set(userId, row.health);
            if (row.memory) memory.set(userId, row.memory);
          }
        }
        if (!cancelled) {
          setLaunchedByProgram(next);
          setAdoptedClients(adopted);
          setClientHealth(health);
          setClientMemory(memory);
        }
      } catch {
      }
      // Separado: um backend sem o comando não pode apagar a lista acima.
      try {
        const unidentified = await invoke<UnidentifiedClient[]>("get_unidentified_clients");
        if (!cancelled) setUnidentifiedClients(Array.isArray(unidentified) ? unidentified : []);
      } catch {
        if (!cancelled) setUnidentifiedClients([]);
      }
    };

    refreshRunningRef.current = refreshRunningInstances;
    refreshRunningInstances();
    const timer = window.setInterval(refreshRunningInstances, 2500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [needsPassword, initialized]);

  useEffect(() => {
    function onActionStatus(event: Event) {
      const custom = event as CustomEvent<{ message?: string; tone?: ActionStatusTone; timeoutMs?: number }>;
      if (!custom.detail?.message) return;
      setActionStatusMessage(
        custom.detail.message,
        custom.detail.tone || "success",
        custom.detail.timeoutMs ?? 2800
      );
    }
    window.addEventListener("ram-action-status", onActionStatus as EventListener);
    return () => {
      window.removeEventListener("ram-action-status", onActionStatus as EventListener);
    };
  }, [setActionStatusMessage]);

  useEffect(() => {
    if (needsPassword || !initialized) return;

    if (settings?.General?.ShowPresence !== "true") {
      setPresenceByUserId(new Map());
      return;
    }

    let cancelled = false;
    const userIds = accounts.map((a) => a.UserID);
    const intervalMinutes = Math.max(
      1,
      parseInt(settings?.General?.PresenceUpdateRate || "5", 10) || 5
    );
    const intervalMs = Math.max(30_000, intervalMinutes * 60 * 1000);

    const refreshPresence = async () => {
      if (userIds.length === 0) {
        if (!cancelled) setPresenceByUserId(new Map());
        return;
      }

      const next = new Map<number, number>();
      try {
        for (let i = 0; i < userIds.length; i += 100) {
          const chunk = userIds.slice(i, i + 100);
          const result = await invoke<PresenceEntry[]>("get_presence", { userIds: chunk });
          for (const presence of result) {
            const userId = presence.userId ?? presence.user_id;
            const presenceType = presence.userPresenceType ?? presence.user_presence_type ?? 0;
            if (typeof userId === "number") {
              next.set(userId, presenceType);
            }
          }
        }
        if (!cancelled) {
          setPresenceByUserId(next);
        }
      } catch {
      }
    };

    refreshPresence();
    const timer = window.setInterval(refreshPresence, intervalMs);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [
    accounts,
    settings?.General?.ShowPresence,
    settings?.General?.PresenceUpdateRate,
    needsPassword,
    initialized,
  ]);

  useEffect(() => {
    if (needsPassword || !initialized) return;
    const autoRefresh = settings?.General?.AutoCookieRefresh;
    if (autoRefresh === "false") return;

    const interval = window.setInterval(async () => {
      const now = Date.now();
      for (const account of accounts) {
        if (account.Fields?.NoCookieRefresh === "true") continue;
        const daysSinceUse = (now - new Date(account.LastUse).getTime()) / 86400000;
        if (daysSinceUse < 20) continue;
        const daysSinceRefresh =
          (now - new Date(account.LastAttemptedRefresh).getTime()) / 86400000;
        if (daysSinceRefresh < 7) continue;
        await refreshCookie(account.UserID);
        await new Promise((r) => setTimeout(r, 5000));
      }
    }, 5 * 60 * 1000);

    return () => clearInterval(interval);
  }, [accounts, needsPassword, initialized, settings]);

  useEffect(() => {
    if (needsPassword || !initialized) return;
    if (settings?.Watcher?.Enabled !== "true") {
      invoke("stop_watcher").catch(() => {});
      return;
    }

    let disposed = false;
    const unsubs: Array<() => void> = [];
    invoke("start_watcher").catch(() => {});

    const listeners = [
      listen<{ userId: number }>("roblox-process-died", (e) => {
        addToast(tr("Watcher: process closed for {{userId}}", { userId: e.payload.userId }));
      }),
      listen<{ userId: number; memoryMb: number }>("roblox-low-memory", (e) => {
        addToast(tr("Watcher: low memory {{memoryMb}}MB ({{userId}})", { memoryMb: e.payload.memoryMb, userId: e.payload.userId }));
      }),
      // Teto de memória (memory_ceiling.rs): continuou acima depois de liberar.
      listen<{ userId: number; memoryMb: number; limitMb: number }>("roblox-memory-limit", (e) => {
        addToast(
          tr("Closed {{name}}: memory stayed over its limit after it was freed", {
            name: accountLabel(accountsRef.current.find((a) => a.UserID === e.payload.userId), nameMaskingRef.current, e.payload.userId),
          }),
          "warn"
        );
      }),
      listen<{ userId: number; expected: string }>("roblox-title-mismatch", (e) => {
        addToast(tr("Watcher: title mismatch for {{userId}} ({{expected}})", { userId: e.payload.userId, expected: e.payload.expected }));
      }),
      listen<{ userId: number; title: string }>("roblox-beta-detected", (e) => {
        addToast(tr("Watcher: beta build detected for {{userId}}", { userId: e.payload.userId }));
      }),
      listen<{ userId: number; timeout: number }>("roblox-no-connection", (e) => {
        addToast(tr("Watcher: no connection timeout ({{timeout}}s) for {{userId}}", { timeout: e.payload.timeout, userId: e.payload.userId }));
      }),
      listen<{ userId: number; seconds: number }>("roblox-not-responding", (e) => {
        addToast(tr("Watcher: closed a client that stopped responding ({{userId}})", { userId: e.payload.userId }));
      }),
    ];

    Promise.all(listeners)
      .then((fns) => {
        if (disposed) {
          fns.forEach((fn) => fn());
          return;
        }
        fns.forEach((fn) => unsubs.push(fn));
      })
      .catch(() => {});

    return () => {
      disposed = true;
      invoke("stop_watcher").catch(() => {});
      unsubs.forEach((fn) => fn());
    };
  }, [needsPassword, initialized, settings?.Watcher?.Enabled, addToast]);

  useEffect(() => {
    return () => {
      clearLaunchTimeout();
      if (actionStatusTimeoutRef.current !== null) {
        window.clearTimeout(actionStatusTimeoutRef.current);
        actionStatusTimeoutRef.current = null;
      }
      if (walkthroughOpenTimeoutRef.current !== null) {
        window.clearTimeout(walkthroughOpenTimeoutRef.current);
        walkthroughOpenTimeoutRef.current = null;
      }
    };
  }, []);

  const checkForUpdates = useCallback(async (
    manual?: boolean,
    channels?: { releaseChannel?: string; featureChannel?: string },
    options?: { autoInstall?: boolean; noUpdateMessage?: string }
  ): Promise<boolean> => {
    if (!manual && settings?.General?.CheckForUpdates === "false") return false;

    const releaseChannel = normalizeUpdaterReleaseChannel(
      channels?.releaseChannel ?? settings?.General?.UpdaterReleaseChannel ?? "beta"
    );
    const featureChannel = normalizeUpdaterFeatureChannel(
      channels?.featureChannel ?? settings?.General?.UpdaterFeatureChannel ?? "standard"
    );

    try {
      const update = await invoke<{
        version: string;
        currentVersion: string;
        date: string;
        body: string;
        releaseChannel: UpdaterReleaseChannel;
        featureChannel: UpdaterFeatureChannel;
      } | null>("check_for_updates_with_channels", {
        releaseChannel,
        featureChannel,
        // Só a checagem pedida pela pessoa troca de edição na mesma versão; a
        // automática do boot nunca (ver `update_is_offered` no updater.rs).
        allowEditionSwitch: manual === true,
      });

      if (!update) {
        if (manual) addToast(options?.noUpdateMessage ?? tr("No updates available"));
        return false;
      }

      const resolvedReleaseChannel = normalizeUpdaterReleaseChannel(update.releaseChannel);
      const resolvedFeatureChannel = normalizeUpdaterFeatureChannel(update.featureChannel);
      const skipped = localStorage.getItem(
        getUpdaterSkipVersionKey(resolvedReleaseChannel, resolvedFeatureChannel)
      );
      if (!manual && skipped === update.version) return false;

      setUpdateInfo({
        version: update.version,
        currentVersion: update.currentVersion,
        date: update.date ?? "",
        body: update.body ?? "",
        releaseChannel: resolvedReleaseChannel,
        featureChannel: resolvedFeatureChannel,
        autoInstall: options?.autoInstall === true,
      });
      setUpdateDialogOpen(true);
      return true;
    } catch (e) {
      if (manual) addToast(tr("Update check failed"));
      return false;
    }
  }, [
    settings?.General?.CheckForUpdates,
    settings?.General?.UpdaterFeatureChannel,
    settings?.General?.UpdaterReleaseChannel,
    addToast,
  ]);

  const switchToCompleteEdition = useCallback(async (): Promise<boolean> => {
    // Gravada antes da checagem: a setting mora no INI da pasta de dados e
    // sobrevive à atualização, então toda checagem depois desta (inclusive a
    // automática do boot, na edição completa) continua no canal completo.
    try {
      await invoke("update_setting", { section: "General", key: "UpdaterFeatureChannel", value: "nexus-ws" });
    } catch (e) {
      addToast(tr("Could not switch to the complete edition: {{error}}", { error: String(e) }), "error");
      return false;
    }
    setSettings((prev) => ({
      ...(prev || {}),
      General: { ...(prev?.General || {}), UpdaterFeatureChannel: "nexus-ws" },
    }));
    return checkForUpdates(
      true,
      { featureChannel: "nexus-ws" },
      {
        autoInstall: true,
        noUpdateMessage: tr("The complete edition is not available right now. Try again later."),
      }
    );
  }, [checkForUpdates, addToast]);

  const openUpdatePreviewDialog = useCallback(() => {
    const previewBody = [
      "> [!WARNING]",
      "> This is a beta release. Missing features, bugs and crashes are possible. Run at your own risk.",
      "",
      "Channel: Beta",
      "Release commit: d9530e6",
      "Release commit message: Merge pull request #21 from luanmacea/fix/windows-client-settings-runtime-overrides",
      "App version: 4.2.6",
      "",
      "## What's Changed",
      "",
      "\\* fix(client-settings): add Windows runtime overrides via GlobalBasicSettings_13.xml by @niccdevs in #21",
      "* fix(update-dialog): render release notes with GitHub-style bullets, callouts, and links",
      "* chore(ui): improve update modal note spacing for long changelogs",
      "",
      `Full Changelog: ${REPO_URL}/compare/v4.2.5-beta...v4.2.6-beta`,
      "",
      "## Contributors",
      "",
      "<a href=\"https://github.com/niccdevs\"><img src=\"https://github.com/niccdevs.png?size=64\" width=\"32\" height=\"32\" alt=\"@niccdevs\" /></a>",
      "",
      "[@niccdevs](https://github.com/niccdevs)",
    ].join("\n");

    setUpdateInfo({
      version: "4.2.6-beta",
      currentVersion: "4.2.5",
      date: new Date().toISOString(),
      body: previewBody,
      releaseChannel: "beta",
      featureChannel: "standard",
    });
    setUpdateDialogOpen(true);
  }, []);

  const value: StoreValue = {
    accounts,
    groups,
    loadAccounts,
    saveAccounts,
    addAccountByCookie,
    removeAccounts,
    updateAccount,
    selectedIds,
    selectedAccount,
    selectedAccounts,
    handleSelect,
    selectSingle,
    selectAll,
    deselectAll,
    toggleSelectAll,
    setSelectedIds,
    navigateSelection,
    orderedUserIds,
    searchQuery,
    setSearchQuery,
    showGroups,
    setShowGroups,
    collapsedGroups,
    toggleGroup,
    sidebarOpen,
    setSidebarOpen,
    chooseGameOpen,
    setChooseGameOpen,
    hideUsernames,
    setHideUsernames,
    hiddenNameLetters,
    showAvatarsWhenHidden,
    hideRobuxWhenHidden,
    placeId,
    setPlaceId,
    jobId,
    setJobId,
    launchData,
    setLaunchData,
    shuffleJobId,
    setShuffleJobId,
    serverPreference,
    setServerPreference,
    serverRegionFilter,
    setServerRegionFilter,
    serverScanPages,
    setServerScanPages,
    contextMenu,
    openContextMenu,
    closeContextMenu,
    settings,
    platformCapabilities,
    theme,
    applyThemePreview,
    saveTheme,
    devMode,
    avatarUrls,
    presenceByUserId,
    moderationByUserId,
    checkModeration,
    checkAccounts,
    accountCheckProgress,
    launchedByProgram,
    adoptedClients,
    unidentifiedClients,
    clientHealth,
    clientMemory,
    identifyExternalClient,
    focusClientWindow,
    joinServer,
    launchMultiple,
    presetsDialog,
    openPresetsDialog,
    closePresetsDialog,
    presetsRevision,
    launchPreset,
    restartRobloxClients,
    focusRobloxClient,
    closeRobloxClients,
    killAllRobloxProcesses,
    launchQueue,
    friendLinkState,
    refreshLaunchQueue,
    cancelAccountLaunch,
    stopLaunchQueue,
    autoReconnect,
    stopAutoReconnect,
    retryAutoReconnect,
    startBottingMode,
    adoptRunningIntoBotting,
    detectRunningGamePlace,
    stopBottingMode,
    addBottingAccounts,
    setBottingPlayerAccounts,
    bottingAccountAction,
    refreshBottingStatus,
    startAfkMode,
    stopAfkMode,
    setAfkAccounts,
    refreshAfkStatus,
    afkTriggerNow,
    captureAfkPoint,
    afkStatus,
    afkKeys,
    setAfkDialogOpen,
    setAvatarsDialogOpen,
    refreshAvatarHeadshots,
    startGenerator,
    stopGenerator,
    refreshGeneratorStatus,
    refreshCookie,
    moveToGroup,
    sortGroupAlphabetically,
    reorderAccounts,
    reorderGroups,
    joiningAccounts,
    launchProgress,
    launchLogs,
    clearLaunchLogs,
    dragState,
    setDragState,
    groupDragState,
    setGroupDragState,
    toasts,
    addToast,
    actionStatus,
    modal,
    showModal,
    closeModal,
    error,
    setError,
    needsPassword,
    unlocking,
    unlock,
    appLocked,
    lockApp,
    unlockApp,
    encryptionSetupOpen,
    encryptionSetupMode,
    accountsEncrypted,
    vaultKeyWarning,
    applyingEncryption,
    encryptionSetupError,
    openEncryptionSetupFromSettings,
    closeEncryptionSetup,
    applyEncryptionMethod,
    firstRunWalkthroughOpen,
    firstRunWalkthroughMode,
    openFirstRunWalkthroughFromSettings,
    closeFirstRunWalkthrough,
    completeFirstRunWalkthrough,
    skipFirstRunWalkthrough,
    initialized,
    activePage,
    setActivePage,
    setSettingsOpen,
    reloadSettings,
    updateSetting,
    serverListOpen,
    setServerListOpen,
    accountUtilsOpen,
    setAccountUtilsOpen,
    accountFieldsOpen,
    setAccountFieldsOpen,
    importDialogOpen,
    setImportDialogOpen,
    importDialogTab,
    setImportDialogTab,
    setThemeEditorOpen,
    afkModeDialog,
    openAfkMode,
    closeAfkMode,
    openBottingDialog,
    bottingStatus,
    generatorDialogOpen,
    generatorDialogTab,
    openGeneratorDialog,
    setGeneratorDialogOpen,
    generatorStatus,
    versionsDialogOpen,
    setVersionsDialogOpen,
    diagnosticsOpen,
    setDiagnosticsOpen,
    quickLoginOpen,
    setQuickLoginOpen,
    setSessionDialogOpen,
    setDefaultVersion,
    missingAssets,
    setMissingAssets,
    setNexusOpen,
    setScriptsOpen,
    updateInfo,
    updateDialogOpen,
    setUpdateDialogOpen,
    checkForUpdates,
    switchToCompleteEdition,
    openUpdatePreviewDialog,
    openLoginBrowser,
    openAccountBrowser,
    browserDownload,
    ensureBrowserDownload,
  };

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}
