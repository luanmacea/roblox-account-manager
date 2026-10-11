/**
 * Shared test harness for component tests.
 *
 * `store.tsx` keeps its React context private (only `useStore` / `StoreProvider`
 * are exported), so tests replace the whole module with `vi.mock` and read the
 * value from `storeRef`:
 *
 * ```ts
 * vi.mock("../../store", async () =>
 *   (await import("../../test-utils/renderWithStore")).storeModuleMock()
 * );
 * ```
 *
 * `renderWithStore(<Component />, { ...overrides })` then installs a realistic
 * default `StoreValue` with `vi.fn()` actions and renders the tree.
 */
import type { ReactElement, ReactNode } from "react";
import { render, type RenderOptions, type RenderResult } from "@testing-library/react";
import { vi } from "vitest";
import type { BottingStatus, GeneratorStatus, StoreValue } from "../store";
import type { Account, ModerationStatus, ParsedGroup, PlatformCapabilities } from "../types";
import { parseGroupName } from "../types";

/** A realistic account row; override any field per test. */
export function makeAccount(overrides: Partial<Account> = {}): Account {
  const userId = overrides.UserID ?? 1001;
  return {
    Valid: true,
    SecurityToken: `cookie-${userId}`,
    Username: `user${userId}`,
    LastUse: new Date().toISOString(),
    Alias: "",
    Description: "",
    Password: "",
    Group: "Default",
    UserID: userId,
    Fields: {},
    LastAttemptedRefresh: "",
    BrowserTrackerID: "",
    ...overrides,
  };
}

/** Groups accounts the way the real store does (by `Group`, `Default` fallback). */
export function groupAccounts(accounts: Account[]): ParsedGroup[] {
  const byKey = new Map<string, Account[]>();
  for (const account of accounts) {
    const key = account.Group || "Default";
    const list = byKey.get(key);
    if (list) list.push(account);
    else byKey.set(key, [account]);
  }
  return [...byKey.entries()].map(([key, list]) => ({
    key,
    displayName: parseGroupName(key).displayName,
    sortKey: parseGroupName(key).sortKey,
    accounts: list,
  }));
}

export function makeBottingStatus(overrides: Partial<BottingStatus> = {}): BottingStatus {
  return {
    active: false,
    startedAtMs: null,
    placeId: 0,
    jobId: "",
    launchData: "",
    intervalMinutes: 30,
    launchDelaySeconds: 5,
    playerGraceMinutes: 5,
    playerUserIds: [],
    userIds: [],
    accounts: [],
    ...overrides,
  };
}

export function makeGeneratorStatus(overrides: Partial<GeneratorStatus> = {}): GeneratorStatus {
  return {
    active: false,
    startedAtMs: null,
    provider: "",
    endpoint: "",
    accountType: "",
    extraDelaySeconds: 0,
    targetGroup: "Default",
    maxAccounts: 0,
    phase: "idle",
    nextAttemptAtMs: null,
    totalGenerated: 0,
    lastUsername: null,
    lastUserId: null,
    lastError: null,
    lastGeneratedAtMs: null,
    ...overrides,
  };
}

export function makePlatformCapabilities(
  overrides: Partial<PlatformCapabilities> = {}
): PlatformCapabilities {
  return {
    os: "windows",
    sessionType: "desktop",
    preferredRunner: "native",
    detectedRunner: "native",
    runnerPath: null,
    supportsSingleLaunch: true,
    supportsMultiLaunch: true,
    supportsWatcher: true,
    supportsWatcherMemory: true,
    supportsWindowControls: true,
    supportsBotting: true,
    supportsUpdater: true,
    supportsClientSettings: true,
    supportsLiveAudio: false,
    reasons: [],
    warnings: [],
    ...overrides,
  };
}

/** Default settings map — mirrors the shape the INI parser produces. */
export function defaultSettings(): Record<string, Record<string, string>> {
  return {
    General: {
      ShowPresence: "false",
      HideUsernames: "false",
      HiddenNameLetters: "0",
      DisableAgingAlert: "true",
      WarnOnOnlineJoin: "false",
      BottingEnabled: "false",
      RestrictedBackgroundStyle: "waves",
      GridGap: "0",
    },
    WebServer: {},
    Developer: {},
    Isolation: {},
  };
}

/**
 * Builds a complete, realistic `StoreValue`. Every action is a `vi.fn()` so
 * tests can assert on calls without stubbing the whole surface each time.
 */
export function createStoreValue(overrides: Partial<StoreValue> = {}): StoreValue {
  const accounts = overrides.accounts ?? [];
  const selectedIds = overrides.selectedIds ?? new Set<number>();
  const selectedAccounts =
    overrides.selectedAccounts ?? accounts.filter((a) => selectedIds.has(a.UserID));

  const base: StoreValue = {
    accounts,
    groups: groupAccounts(accounts),
    loadAccounts: vi.fn(async () => {}),
    saveAccounts: vi.fn(async () => {}),
    addAccountByCookie: vi.fn(async () => {}),
    removeAccounts: vi.fn(async () => {}),
    updateAccount: vi.fn(async () => {}),

    selectedIds,
    selectedAccount: selectedAccounts.length === 1 ? selectedAccounts[0] : null,
    selectedAccounts,
    handleSelect: vi.fn(),
    selectSingle: vi.fn(),
    selectAll: vi.fn(),
    deselectAll: vi.fn(),
    toggleSelectAll: vi.fn(),
    setSelectedIds: vi.fn(),
    navigateSelection: vi.fn(),
    orderedUserIds: accounts.map((a) => a.UserID),

    searchQuery: "",
    setSearchQuery: vi.fn(),
    showGroups: true,
    setShowGroups: vi.fn(),
    collapsedGroups: new Set<string>(),
    toggleGroup: vi.fn(),
    sidebarOpen: false,
    setSidebarOpen: vi.fn(),
    chooseGameOpen: false,
    setChooseGameOpen: vi.fn(),
    hideUsernames: false,
    setHideUsernames: vi.fn(),
    hiddenNameLetters: 0,
    showAvatarsWhenHidden: true,
    hideRobuxWhenHidden: false,

    placeId: "",
    setPlaceId: vi.fn(),
    jobId: "",
    setJobId: vi.fn(),
    launchData: "",
    setLaunchData: vi.fn(),
    shuffleJobId: false,
    // O dublê começa em "none" (o app começa em "bestfit"): assim um teste que
    // não é sobre escolha de servidor não dispara `pick_server` sem querer.
    serverPreference: "none" as const,
    setServerPreference: vi.fn(),
    serverRegionFilter: "",
    setServerRegionFilter: vi.fn(),
    serverScanPages: 30,
    setServerScanPages: vi.fn(),
    setShuffleJobId: vi.fn(),

    contextMenu: null,
    openContextMenu: vi.fn(),
    closeContextMenu: vi.fn(),

    settings: defaultSettings(),
    platformCapabilities: makePlatformCapabilities(),
    theme: null,
    applyThemePreview: vi.fn(),
    saveTheme: vi.fn(async () => {}),
    devMode: false,

    avatarUrls: new Map<number, string>(),
    presenceByUserId: new Map<number, number>(),
    moderationByUserId: new Map<number, ModerationStatus>(),
    checkModeration: vi.fn(async () => null),
    checkAccounts: vi.fn(async () => null),
    accountCheckProgress: null,
    launchedByProgram: new Set<number>(),
    adoptedClients: new Set<number>(),
    unidentifiedClients: [],
    clientHealth: new Map(),
    identifyExternalClient: vi.fn(async () => true),
    focusClientWindow: vi.fn(async () => true),

    joinServer: vi.fn(async () => "started" as const),
    launchMultiple: vi.fn(async () => {}),
    presetsDialog: null,
    openPresetsDialog: vi.fn(),
    closePresetsDialog: vi.fn(),
    presetsRevision: 0,
    launchPreset: vi.fn(async () => "started" as const),
    restartRobloxClients: vi.fn(async () => {}),
    focusRobloxClient: vi.fn(async () => true),
    closeRobloxClients: vi.fn(async (userIds: number[]) => userIds.length),
    killAllRobloxProcesses: vi.fn(async () => {}),
    launchQueue: null,
    friendLinkState: null,
    refreshLaunchQueue: vi.fn(async () => {}),
    cancelAccountLaunch: vi.fn(async () => true),
    stopLaunchQueue: vi.fn(async () => 0),
    autoReconnect: [],
    stopAutoReconnect: vi.fn(async () => true),
    retryAutoReconnect: vi.fn(async () => true),
    startBottingMode: vi.fn(async () => {}),
    adoptRunningIntoBotting: vi.fn(async () => {}),
    detectRunningGamePlace: vi.fn(async () => null),
    stopBottingMode: vi.fn(async () => {}),
    addBottingAccounts: vi.fn(async () => {}),
    setBottingPlayerAccounts: vi.fn(async () => {}),
    bottingAccountAction: vi.fn(async () => {}),
    refreshBottingStatus: vi.fn(async () => {}),
    startAfkMode: vi.fn(async () => {}),
    stopAfkMode: vi.fn(async () => {}),
    setAfkAccounts: vi.fn(async () => {}),
    refreshAfkStatus: vi.fn(async () => {}),
    afkTriggerNow: vi.fn(async () => 1),
    captureAfkPoint: vi.fn(async () => ({ userId: 0, xPct: 50, yPct: 50 })),
    afkStatus: null,
    afkKeys: [],
    startGenerator: vi.fn(async () => makeGeneratorStatus()),
    stopGenerator: vi.fn(async () => {}),
    refreshGeneratorStatus: vi.fn(async () => {}),
    refreshCookie: vi.fn(async () => true),
    moveToGroup: vi.fn(async () => {}),
    sortGroupAlphabetically: vi.fn(),
    reorderAccounts: vi.fn(async () => {}),
    joiningAccounts: new Set<number>(),
    launchProgress: null,
    launchLogs: [],
    clearLaunchLogs: vi.fn(),

    dragState: null,
    groupDragState: null,
    setGroupDragState: vi.fn(),
    reorderGroups: vi.fn(),
    setDragState: vi.fn(),

    toasts: [],
    addToast: vi.fn(),
    actionStatus: null,
    modal: null,
    showModal: vi.fn(),
    closeModal: vi.fn(),

    error: null,
    setError: vi.fn(),
    needsPassword: false,
    unlocking: false,
    unlock: vi.fn(async () => {}),
    encryptionSetupOpen: false,
    encryptionSetupMode: "firstRun",
    accountsEncrypted: false,
    vaultKeyWarning: null,
    applyingEncryption: false,
    encryptionSetupError: null,
    openEncryptionSetupFromSettings: vi.fn(),
    closeEncryptionSetup: vi.fn(),
    applyEncryptionMethod: vi.fn(async () => {}),
    firstRunWalkthroughOpen: false,
    firstRunWalkthroughMode: "firstRun",
    openFirstRunWalkthroughFromSettings: vi.fn(),
    closeFirstRunWalkthrough: vi.fn(),
    completeFirstRunWalkthrough: vi.fn(async () => {}),
    skipFirstRunWalkthrough: vi.fn(async () => {}),
    initialized: true,

    activePage: "accounts" as const,
    setActivePage: vi.fn(),
    setSettingsOpen: vi.fn(),
    reloadSettings: vi.fn(async () => {}),
    updateSetting: vi.fn(async () => {}),

    serverListOpen: false,
    setServerListOpen: vi.fn(),

    accountUtilsOpen: false,
    setAccountUtilsOpen: vi.fn(),
    accountFieldsOpen: false,
    setAccountFieldsOpen: vi.fn(),
    importDialogOpen: false,
    setImportDialogOpen: vi.fn(),
    importDialogTab: "cookie",
    setImportDialogTab: vi.fn(),
    setThemeEditorOpen: vi.fn(),
    afkModeDialog: null,
    openAfkMode: vi.fn(),
    closeAfkMode: vi.fn(),
    openBottingDialog: vi.fn(),
    bottingStatus: null,
    generatorDialogOpen: false,
    generatorDialogTab: "provider" as const,
    openGeneratorDialog: vi.fn(),
    setGeneratorDialogOpen: vi.fn(),
    generatorStatus: null,
    versionsDialogOpen: false,
    setVersionsDialogOpen: vi.fn(),
    diagnosticsOpen: false,
    appLocked: false,
    lockApp: vi.fn(),
    unlockApp: vi.fn(async () => null),
    setDiagnosticsOpen: vi.fn(),
    setAfkDialogOpen: vi.fn(),
    setAvatarsDialogOpen: vi.fn(),
    refreshAvatarHeadshots: vi.fn(async () => {}),
    setSessionDialogOpen: vi.fn(),
    setDefaultVersion: vi.fn(),
    missingAssets: null,
    setMissingAssets: vi.fn(),

    setNexusOpen: vi.fn(),
    setScriptsOpen: vi.fn(),

    updateInfo: null,
    updateDialogOpen: false,
    setUpdateDialogOpen: vi.fn(),
    checkForUpdates: vi.fn(async () => false),
    switchToCompleteEdition: vi.fn(async () => true),
    openUpdatePreviewDialog: vi.fn(),

    openLoginBrowser: vi.fn(async () => {}),
    openAccountBrowser: vi.fn(async () => {}),
    browserDownload: null,
    ensureBrowserDownload: vi.fn(async () => true),
  };

  return { ...base, ...overrides };
}

/** The store instance the mocked `useStore()` hands to components. */
export const storeRef: { current: StoreValue } = { current: createStoreValue() };

/** Replaces the active store value and returns it for assertions. */
export function setStore(overrides: Partial<StoreValue> = {}): StoreValue {
  storeRef.current = createStoreValue(overrides);
  return storeRef.current;
}

/**
 * Module replacement for `../store`. Use as:
 * `vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock())`
 */
export function storeModuleMock() {
  return {
    useStore: () => storeRef.current,
    StoreProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
    // Constantes puras do módulo continuam valendo no dublê: componentes as
    // importam junto com o hook, e omiti-las quebra o render com um erro de
    // mock em vez de um erro de teste.
    DEFAULT_SERVER_SCAN_PAGES: 30,
    MAX_SERVER_SCAN_PAGES: 500,
  };
}

export interface RenderWithStoreResult extends RenderResult {
  store: StoreValue;
}

/** Installs a default store (plus `overrides`) and renders `ui`. */
export function renderWithStore(
  ui: ReactElement,
  overrides: Partial<StoreValue> = {},
  options?: RenderOptions
): RenderWithStoreResult {
  const store = setStore(overrides);
  return { store, ...render(ui, options) };
}
