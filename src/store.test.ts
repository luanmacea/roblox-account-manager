import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Account, PlatformCapabilities } from "./types";
import { VAULT_KEY_WARNING_EVENT } from "./types";

const invokeMock = vi.fn();
const recordRecentGameMock = vi.fn(async () => {});
const addRecentJobMock = vi.fn(() => {});
const unlistenMock = vi.fn();
const listenHandlers = new Map<string, Array<(event: { payload: unknown }) => void>>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invokeMock(cmd, args),
  isTauri: () => false,
  convertFileSrc: (p: string) => p,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (e: { payload: unknown }) => void) => {
    const handlers = listenHandlers.get(event) ?? [];
    handlers.push(handler);
    listenHandlers.set(event, handlers);
    return Promise.resolve(() => unlistenMock(event));
  },
}));

vi.mock("./components/server-list/types", () => ({
  recordRecentGame: (...args: unknown[]) => recordRecentGameMock(...(args as [])),
  addRecentJob: (...args: unknown[]) => addRecentJobMock(...(args as [])),
}));

import { StoreProvider, useStore, type StoreValue } from "./store";

function account(overrides: Partial<Account> & { UserID: number }): Account {
  return {
    Valid: true,
    SecurityToken: "token",
    Username: `user${overrides.UserID}`,
    LastUse: new Date().toISOString(),
    Alias: "",
    Description: "",
    Password: "",
    Group: "",
    Fields: {},
    LastAttemptedRefresh: new Date().toISOString(),
    BrowserTrackerID: "",
    ...overrides,
  };
}

function caps(overrides: Partial<PlatformCapabilities> = {}): PlatformCapabilities {
  return {
    os: "windows",
    sessionType: "",
    preferredRunner: "",
    detectedRunner: "",
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
    supportsMemoryTrim: false,
    reasons: [],
    warnings: [],
    ...overrides,
  };
}

let accountsData: Account[] = [];
let settingsData: Record<string, Record<string, string>> = {};
let capabilitiesData: PlatformCapabilities = caps();
let presenceRows: unknown[] = [];
let runningInstances: unknown[] = [];
let needsPasswordValue = false;
let updateResult: unknown = null;
const failures = new Map<string, unknown>();
const results = new Map<string, unknown>();

function defaultInvoke(cmd: string): unknown {
  switch (cmd) {
    case "needs_password":
      return needsPasswordValue;
    case "get_accounts":
      return accountsData;
    case "get_all_settings":
      return settingsData;
    case "get_platform_capabilities":
      return capabilitiesData;
    case "is_accounts_encrypted":
      return false;
    case "batched_get_avatar_headshots":
      return [];
    case "get_presence":
      return presenceRows;
    case "get_running_instances":
      return runningInstances;
    case "check_for_updates_with_channels":
      return updateResult;
    case "cmd_kill_all_roblox":
      return 0;
    case "get_theme":
      return { accounts_background: "#101010" };
    default:
      return null;
  }
}

function invokeCalls(cmd: string) {
  return invokeMock.mock.calls.filter((c) => c[0] === cmd);
}

function lastArgs(cmd: string): Record<string, unknown> {
  const calls = invokeCalls(cmd);
  if (calls.length === 0) throw new Error(`no invoke("${cmd}") calls`);
  return calls[calls.length - 1][1] as Record<string, unknown>;
}

function emit(event: string, payload: unknown) {
  const handlers = listenHandlers.get(event) ?? [];
  for (const handler of handlers) handler({ payload });
}

async function renderStore() {
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(StoreProvider, null, children);
  const view = renderHook(() => useStore(), { wrapper });
  await waitFor(() => expect(view.result.current.initialized).toBe(true));
  return view;
}

beforeEach(() => {
  invokeMock.mockReset();
  recordRecentGameMock.mockClear();
  // `mockReset` (e não `mockClear`): um teste que faz a gravação dos recentes
  // explodir não pode deixar a implementação quebrada para o teste seguinte.
  addRecentJobMock.mockReset();
  unlistenMock.mockClear();
  listenHandlers.clear();
  failures.clear();
  results.clear();
  accountsData = [];
  settingsData = {};
  capabilitiesData = caps();
  presenceRows = [];
  runningInstances = [];
  needsPasswordValue = false;
  updateResult = null;
  localStorage.clear();
  invokeMock.mockImplementation(async (cmd: string) => {
    if (failures.has(cmd)) throw failures.get(cmd);
    if (results.has(cmd)) return results.get(cmd);
    return defaultInvoke(cmd);
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("useStore", () => {
  it("throws when used outside the provider", () => {
    expect(() => renderHook(() => useStore())).toThrow(/StoreProvider/);
  });
});

describe("store bootstrap", () => {
  it("loads accounts, settings and saved launch fields", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 })];
    settingsData = {
      General: {
        SavedPlaceId: "111",
        SavedJobId: "job-1",
        SavedLaunchData: "ld",
        ShuffleJobId: "true",
        HideUsernames: "true",
        HiddenNameLetters: "3",
        ShowAvatarsWhenHidden: "true",
        HideRobuxWhenHidden: "true",
      },
      Developer: { DevMode: "true" },
    };

    const { result } = await renderStore();

    expect(result.current.accounts).toHaveLength(2);
    expect(result.current.placeId).toBe("111");
    expect(result.current.jobId).toBe("job-1");
    expect(result.current.launchData).toBe("ld");
    expect(result.current.shuffleJobId).toBe(true);
    expect(result.current.hideUsernames).toBe(true);
    expect(result.current.hiddenNameLetters).toBe(3);
    expect(result.current.showAvatarsWhenHidden).toBe(true);
    expect(result.current.hideRobuxWhenHidden).toBe(true);
    expect(result.current.devMode).toBe(true);
  });

  it("falls back to 0 hidden letters for a non-numeric setting", async () => {
    settingsData = { General: { HiddenNameLetters: "abc" } };
    const { result } = await renderStore();
    expect(result.current.hiddenNameLetters).toBe(0);
  });

  it("keeps accounts empty and surfaces needsPassword when locked", async () => {
    needsPasswordValue = true;
    accountsData = [account({ UserID: 1 })];

    const { result } = await renderStore();

    expect(result.current.needsPassword).toBe(true);
    expect(result.current.accounts).toEqual([]);
    expect(invokeCalls("get_accounts")).toHaveLength(0);
  });

  it("opens the encryption onboarding on a fresh install", async () => {
    settingsData = { General: { EncryptionOnboardingState: "pending" } };
    const { result } = await renderStore();
    expect(result.current.encryptionSetupOpen).toBe(true);
    expect(result.current.encryptionSetupMode).toBe("firstRun");
    // firstRun mode cannot be dismissed
    act(() => result.current.closeEncryptionSetup());
    expect(result.current.encryptionSetupOpen).toBe(true);
  });

  it("opens the first-run walkthrough once encryption onboarding is done", async () => {
    settingsData = {
      General: { FirstRunWalkthroughState: "pending", EncryptionOnboardingState: "completed" },
    };
    const { result } = await renderStore();
    expect(result.current.firstRunWalkthroughOpen).toBe(true);
    expect(result.current.firstRunWalkthroughMode).toBe("firstRun");
  });

  it("records an error when loading accounts fails", async () => {
    failures.set("get_accounts", "boom");
    const { result } = await renderStore();
    expect(result.current.error).toBe("boom");
  });
});

describe("groups and filtering", () => {
  it("collapses a single Default group into a flat list", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2, Group: "Default" })];
    const { result } = await renderStore();

    expect(result.current.groups).toHaveLength(1);
    expect(result.current.groups[0].key).toBe("__all__");
    expect(result.current.groups[0].accounts).toHaveLength(2);
  });

  it("sorts named groups by their numeric prefix and strips it from the label", async () => {
    accountsData = [
      account({ UserID: 1, Group: "20 Bots" }),
      account({ UserID: 2, Group: "5 Mains" }),
      account({ UserID: 3, Group: "Zeta" }),
    ];
    const { result } = await renderStore();

    expect(result.current.groups.map((g) => g.key)).toEqual(["5 Mains", "20 Bots", "Zeta"]);
    expect(result.current.groups.map((g) => g.displayName)).toEqual(["Mains", "Bots", "Zeta"]);
    expect(result.current.groups[2].sortKey).toBe(999999);
  });

  /**
   * Arrastar conta por conta era inviável; reordenar **grupos** é o que o dono
   * pediu. A ordem vive em `General.GroupOrder` (JSON, porque nome de grupo
   * pode ter vírgula) e tem que sobreviver a fechar e reabrir o app.
   */
  it("respeita a ordem manual dos grupos guardada nas settings", async () => {
    accountsData = [
      account({ UserID: 1, Group: "20 Bots" }),
      account({ UserID: 2, Group: "5 Mains" }),
      account({ UserID: 3, Group: "Zeta" }),
    ];
    settingsData = { General: { GroupOrder: '["Zeta","20 Bots"]' } };
    const { result } = await renderStore();

    // Zeta e Bots foram arrastados; Mains, que ninguém tocou, fica no fim.
    expect(result.current.groups.map((g) => g.key)).toEqual(["Zeta", "20 Bots", "5 Mains"]);
  });

  it("ordem estragada no INI não quebra a lista: cai na ordem automática", async () => {
    accountsData = [
      account({ UserID: 1, Group: "Zeta" }),
      account({ UserID: 2, Group: "5 Mains" }),
    ];
    settingsData = { General: { GroupOrder: "Zeta,5 Mains" } };
    const { result } = await renderStore();

    expect(result.current.groups.map((g) => g.key)).toEqual(["5 Mains", "Zeta"]);
  });

  it("arrastar um grupo grava a ordem inteira e reordena na hora", async () => {
    accountsData = [
      account({ UserID: 1, Group: "5 Mains" }),
      account({ UserID: 2, Group: "20 Bots" }),
      account({ UserID: 3, Group: "Zeta" }),
    ];
    const { result } = await renderStore();
    expect(result.current.groups.map((g) => g.key)).toEqual(["5 Mains", "20 Bots", "Zeta"]);

    await act(async () => {
      await result.current.reorderGroups("Zeta", "5 Mains");
    });

    const gravado = invokeMock.mock.calls.filter(
      (c) => c[0] === "update_setting" && (c[1] as { key?: string })?.key === "GroupOrder"
    );
    expect(gravado).toHaveLength(1);
    // A lista inteira é gravada: um arrasto congela a ordem de todos os grupos,
    // senão o próximo grupo novo se enfiaria no meio.
    expect(JSON.parse(String((gravado[0][1] as { value?: string }).value))).toEqual([
      "Zeta",
      "5 Mains",
      "20 Bots",
    ]);
    expect(result.current.groups.map((g) => g.key)).toEqual(["Zeta", "5 Mains", "20 Bots"]);
  });

  /**
   * Com busca ativa a lista mostra só os grupos que casam. Gravar "a ordem
   * visível" apagaria da ordem os grupos escondidos pelo filtro.
   */
  it("não perde grupo escondido pela busca ao reordenar", async () => {
    accountsData = [
      account({ UserID: 1, Username: "needle-a", Group: "Mains" }),
      account({ UserID: 2, Username: "needle-b", Group: "Bots" }),
      account({ UserID: 3, Username: "outro", Group: "Escondido" }),
    ];
    const { result } = await renderStore();
    act(() => result.current.setSearchQuery("needle"));
    expect(result.current.groups.map((g) => g.key)).toEqual(["Bots", "Mains"]);

    await act(async () => {
      await result.current.reorderGroups("Mains", "Bots");
    });

    const gravado = invokeMock.mock.calls.filter(
      (c) => c[0] === "update_setting" && (c[1] as { key?: string })?.key === "GroupOrder"
    );
    expect(JSON.parse(String((gravado[0][1] as { value?: string }).value))).toEqual([
      "Mains",
      "Bots",
      "Escondido",
    ]);
  });

  it("arrastar grupo para o próprio lugar não grava nada", async () => {
    accountsData = [account({ UserID: 1, Group: "A" }), account({ UserID: 2, Group: "B" })];
    const { result } = await renderStore();

    await act(async () => {
      await result.current.reorderGroups("A", "A");
      await result.current.reorderGroups("A", "__all__");
    });

    expect(
      invokeMock.mock.calls.filter(
        (c) => c[0] === "update_setting" && (c[1] as { key?: string })?.key === "GroupOrder"
      )
    ).toHaveLength(0);
  });

  it("returns one synthetic group when grouping is disabled", async () => {
    accountsData = [account({ UserID: 1, Group: "A" }), account({ UserID: 2, Group: "B" })];
    const { result } = await renderStore();

    act(() => result.current.setShowGroups(false));

    expect(result.current.groups).toHaveLength(1);
    expect(result.current.groups[0].key).toBe("__all__");
    expect(result.current.orderedUserIds).toEqual([1, 2]);
  });

  it("filters on username, alias, description and group", async () => {
    accountsData = [
      account({ UserID: 1, Username: "alpha" }),
      account({ UserID: 2, Username: "beta", Alias: "NEEDLE" }),
      account({ UserID: 3, Username: "gamma", Description: "has needle inside" }),
      account({ UserID: 4, Username: "delta", Group: "Needles" }),
      account({ UserID: 5, Username: "epsilon" }),
    ];
    const { result } = await renderStore();

    act(() => result.current.setSearchQuery("needle"));

    const ids = result.current.groups.flatMap((g) => g.accounts.map((a) => a.UserID));
    expect(ids.sort()).toEqual([2, 3, 4]);
  });

  it("omits collapsed groups from orderedUserIds", async () => {
    accountsData = [
      account({ UserID: 1, Group: "A" }),
      account({ UserID: 2, Group: "B" }),
    ];
    const { result } = await renderStore();

    expect(result.current.orderedUserIds).toEqual([1, 2]);
    act(() => result.current.toggleGroup("A"));
    expect(result.current.collapsedGroups.has("A")).toBe(true);
    expect(result.current.orderedUserIds).toEqual([2]);
    act(() => result.current.toggleGroup("A"));
    expect(result.current.orderedUserIds).toEqual([1, 2]);
  });
});

describe("selection", () => {
  async function withFive() {
    accountsData = [1, 2, 3, 4, 5].map((id) => account({ UserID: id }));
    return renderStore();
  }

  function mouse(init: Partial<React.MouseEvent> = {}): React.MouseEvent {
    return { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...init } as React.MouseEvent;
  }

  it("selects a single account on a plain click", async () => {
    const { result } = await withFive();
    act(() => result.current.handleSelect(3, mouse()));
    expect([...result.current.selectedIds]).toEqual([3]);
    expect(result.current.selectedAccount?.UserID).toBe(3);
  });

  it("toggles with ctrl/alt/meta and reports multi selection", async () => {
    const { result } = await withFive();
    act(() => result.current.handleSelect(1, mouse()));
    act(() => result.current.handleSelect(3, mouse({ ctrlKey: true })));
    expect([...result.current.selectedIds].sort()).toEqual([1, 3]);
    // selectedAccount is only set for exactly one selected row
    expect(result.current.selectedAccount).toBeNull();
    expect(result.current.selectedAccounts.map((a) => a.UserID)).toEqual([1, 3]);

    act(() => result.current.handleSelect(3, mouse({ altKey: true })));
    expect([...result.current.selectedIds]).toEqual([1]);
  });

  it("selects a range with shift and extends it when combined with ctrl", async () => {
    const { result } = await withFive();
    act(() => result.current.handleSelect(2, mouse()));
    act(() => result.current.handleSelect(4, mouse({ shiftKey: true })));
    expect([...result.current.selectedIds].sort()).toEqual([2, 3, 4]);

    act(() => result.current.handleSelect(1, mouse({ shiftKey: true, ctrlKey: true })));
    expect([...result.current.selectedIds].sort()).toEqual([1, 2, 3, 4]);
  });

  it("selects all, deselects all and toggles", async () => {
    const { result } = await withFive();
    act(() => result.current.selectAll());
    expect(result.current.selectedIds.size).toBe(5);
    act(() => result.current.deselectAll());
    expect(result.current.selectedIds.size).toBe(0);
    act(() => result.current.toggleSelectAll());
    expect(result.current.selectedIds.size).toBe(5);
    act(() => result.current.toggleSelectAll());
    expect(result.current.selectedIds.size).toBe(0);
  });

  it("selectAll only covers the filtered accounts", async () => {
    accountsData = [
      account({ UserID: 1, Username: "keep-me" }),
      account({ UserID: 2, Username: "other" }),
    ];
    const { result } = await renderStore();
    act(() => result.current.setSearchQuery("keep"));
    act(() => result.current.selectAll());
    expect([...result.current.selectedIds]).toEqual([1]);
  });

  /**
   * Ação em lote não pode atingir conta invisível: com o filtro escondendo
   * linhas selecionadas, a barra inferior e o menu de contexto ainda agiriam
   * sobre elas. A seleção escondida é descartada.
   */
  it("drops from the selection every account the filter hides", async () => {
    accountsData = [
      account({ UserID: 1, Username: "alpha" }),
      account({ UserID: 2, Username: "beta" }),
      account({ UserID: 3, Username: "alphabet" }),
    ];
    const { result } = await renderStore();

    act(() => result.current.selectAll());
    expect(result.current.selectedIds.size).toBe(3);

    act(() => result.current.setSearchQuery("alpha"));
    expect([...result.current.selectedIds].sort()).toEqual([1, 3]);
    expect(result.current.selectedAccounts.map((a) => a.UserID)).toEqual([1, 3]);

    act(() => result.current.setSearchQuery("nothing-matches-this"));
    expect(result.current.selectedIds.size).toBe(0);
    expect(result.current.selectedAccounts).toEqual([]);
  });

  it("keeps the surviving selection when the filter is cleared", async () => {
    accountsData = [account({ UserID: 1, Username: "alpha" }), account({ UserID: 2, Username: "beta" })];
    const { result } = await renderStore();

    act(() => result.current.selectAll());
    act(() => result.current.setSearchQuery("alpha"));
    act(() => result.current.setSearchQuery(""));

    expect([...result.current.selectedIds]).toEqual([1]);
  });

  it("navigates the selection with and without shift and clamps at the edges", async () => {
    const { result } = await withFive();
    act(() => result.current.selectSingle(1));
    act(() => result.current.navigateSelection("up", false));
    expect([...result.current.selectedIds]).toEqual([1]);

    act(() => result.current.navigateSelection("down", false));
    expect([...result.current.selectedIds]).toEqual([2]);

    act(() => result.current.navigateSelection("down", true));
    expect([...result.current.selectedIds].sort()).toEqual([2, 3]);

    act(() => result.current.selectSingle(5));
    act(() => result.current.navigateSelection("down", false));
    expect([...result.current.selectedIds]).toEqual([5]);
  });

  it("starts from the first row when nothing was clicked yet", async () => {
    const { result } = await withFive();
    act(() => result.current.navigateSelection("down", false));
    expect([...result.current.selectedIds]).toEqual([2]);
  });
});

describe("joinServer", () => {
  async function setup(general: Record<string, string> = {}) {
    accountsData = [account({ UserID: 1, Alias: "Main" }), account({ UserID: 2 })];
    settingsData = { General: { ...general } };
    return renderStore();
  }

  it("sends the store's place/job/launchData and shuffle flag", async () => {
    const { result } = await setup({
      SavedPlaceId: "606849621",
      SavedJobId: "job-abc",
      SavedLaunchData: "payload",
      ShuffleJobId: "true",
    });

    await act(async () => {
      await result.current.joinServer(1);
    });

    expect(lastArgs("launch_roblox")).toEqual({
      userId: 1,
      placeId: 606849621,
      jobId: "job-abc",
      launchData: "payload",
      followUser: false,
      joinVip: false,
      linkCode: "",
      shuffleJob: true,
    });
  });

  it("prefers the explicit target over store state", async () => {
    const { result } = await setup({ SavedPlaceId: "1", SavedJobId: "stale", SavedLaunchData: "old" });

    await act(async () => {
      await result.current.joinServer(1, { placeId: "222", jobId: "fresh", launchData: "new" });
    });

    expect(lastArgs("launch_roblox")).toMatchObject({
      placeId: 222,
      jobId: "fresh",
      launchData: "new",
    });
  });

  it("falls back to placeId 5315046213 when place is empty or not a number", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.joinServer(1);
    });
    expect(lastArgs("launch_roblox")).toMatchObject({ placeId: 5315046213 });

    await act(async () => {
      await result.current.joinServer(1, { placeId: "not-a-number" });
    });
    expect(lastArgs("launch_roblox")).toMatchObject({ placeId: 5315046213 });

    // parseInt("0") is falsy, so an explicit 0 also falls back
    await act(async () => {
      await result.current.joinServer(1, { placeId: "0" });
    });
    expect(lastArgs("launch_roblox")).toMatchObject({ placeId: 5315046213 });
  });

  it("parses the vip: job prefix into joinVip + linkCode", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.joinServer(1, { jobId: "vip:  abc-123  " });
    });

    expect(lastArgs("launch_roblox")).toMatchObject({
      jobId: "",
      joinVip: true,
      linkCode: "abc-123",
    });
  });

  it("accepts the vip: prefix case-insensitively", async () => {
    const { result } = await setup();
    await act(async () => {
      await result.current.joinServer(1, { jobId: "VIP:Code42" });
    });
    expect(lastArgs("launch_roblox")).toMatchObject({ joinVip: true, linkCode: "Code42" });
  });

  it("extracts linkCode from a pasted private-server URL and decodes it", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.joinServer(1, {
        jobId: "https://www.roblox.com/games/123/x?privateServerLinkCode=a%2Fb",
      });
    });

    expect(lastArgs("launch_roblox")).toMatchObject({
      jobId: "",
      linkCode: "a/b",
      // NOTE: joinVip stays false for this branch; the backend resolves by link code.
      joinVip: false,
    });
  });

  it("keeps the raw link code when it is not valid percent-encoding", async () => {
    const { result } = await setup();
    await act(async () => {
      await result.current.joinServer(1, { jobId: "?linkCode=100%bad" });
    });
    expect(lastArgs("launch_roblox")).toMatchObject({ linkCode: "100%bad", jobId: "" });
  });

  it("lets an explicit target.joinVip/linkCode win over the parsed job string", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.joinServer(1, {
        jobId: "plain-job-id",
        joinVip: true,
        linkCode: "resolved-code",
      });
    });

    expect(lastArgs("launch_roblox")).toMatchObject({
      jobId: "",
      joinVip: true,
      linkCode: "resolved-code",
    });
  });

  it("lets target.joinVip=false override a vip: prefix while keeping the parsed code", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.joinServer(1, { jobId: "vip:code", joinVip: false });
    });

    expect(lastArgs("launch_roblox")).toMatchObject({
      joinVip: false,
      linkCode: "code",
      jobId: "",
    });
  });

  it("trims the job id before sending it", async () => {
    const { result } = await setup();
    await act(async () => {
      await result.current.joinServer(1, { jobId: "   job-xyz   " });
    });
    expect(lastArgs("launch_roblox")).toMatchObject({ jobId: "job-xyz" });
  });

  it("uses an empty target.launchData instead of the store value", async () => {
    const { result } = await setup({ SavedLaunchData: "store-data" });
    await act(async () => {
      await result.current.joinServer(1, { launchData: "" });
    });
    expect(lastArgs("launch_roblox")).toMatchObject({ launchData: "" });
  });

  it("records the launched place as a recent game with the configured cap", async () => {
    const { result } = await setup({ SavedPlaceId: "42", MaxRecentGames: "3" });
    await act(async () => {
      await result.current.joinServer(1);
    });
    expect(recordRecentGameMock).toHaveBeenCalledWith(42, 1, 3);
  });

  it("defaults the recent-games cap to 8 when the setting is missing or invalid", async () => {
    const { result } = await setup({ SavedPlaceId: "42", MaxRecentGames: "zero" });
    await act(async () => {
      await result.current.joinServer(1);
    });
    expect(recordRecentGameMock).toHaveBeenCalledWith(42, 1, 8);
  });

  /**
   * O servidor entra nos recentes junto com o jogo: voltar ao mesmo servidor
   * era impossível sem ter copiado o Job ID antes.
   */
  it("records the job id as a recent server, with the configured cap", async () => {
    const { result } = await setup({ SavedPlaceId: "42", MaxRecentJobs: "4" });
    await act(async () => {
      await result.current.joinServer(1, { jobId: "job-xyz" });
    });
    expect(addRecentJobMock).toHaveBeenCalledWith("job-xyz", 42, 4, [1]);
  });

  it("defaults the recent-servers cap to 12", async () => {
    const { result } = await setup({ SavedPlaceId: "42" });
    await act(async () => {
      await result.current.joinServer(1, { jobId: "job-xyz" });
    });
    expect(addRecentJobMock).toHaveBeenCalledWith("job-xyz", 42, 12, [1]);
  });

  /**
   * Num alvo VIP o Job ID vai vazio e o código viaja em `linkCode` — guardar o
   * Job ID cru perderia o servidor. O que se guarda é o `vip:<código>` que o
   * campo de Job ID sabe reabrir.
   */
  it("records a VIP target as vip:<code>, not as an empty job", async () => {
    const { result } = await setup({ SavedPlaceId: "42" });
    await act(async () => {
      await result.current.joinServer(1, { jobId: "", joinVip: true, linkCode: "abc123" });
    });
    expect(addRecentJobMock).toHaveBeenCalledWith("vip:abc123", 42, 12, [1]);
  });

  it("does not record a recent server when there is no job id at all", async () => {
    const { result } = await setup({ SavedPlaceId: "42" });
    await act(async () => {
      await result.current.joinServer(1);
    });
    expect(addRecentJobMock).not.toHaveBeenCalled();
  });

  /**
   * `addRecentJob` grava no `localStorage`, que o WebView **recusa** com a cota
   * cheia ou com o perfil sem storage. Isso acontece **depois** de o
   * `launch_roblox` ter voltado com sucesso: o cliente do Roblox já subiu. Se a
   * exceção cair no `catch` do launch, a tela diz "Launch failed" sobre um
   * launch que deu certo. Guardar recentes é conveniência; não derruba launch.
   */
  it("keeps the launch successful when recording the recent server throws", async () => {
    addRecentJobMock.mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    const { result } = await setup({ SavedPlaceId: "42" });

    await act(async () => {
      await result.current.joinServer(1, { jobId: "job-xyz" });
    });

    expect(result.current.error).toBeNull();
    expect(result.current.actionStatus?.tone).not.toBe("error");
    expect([...result.current.joiningAccounts]).toEqual([1]);
  });

  it("tracks joining state and progress, then clears it after 7s", async () => {
    const { result } = await setup({ SavedPlaceId: "42" });
    vi.useFakeTimers();

    await act(async () => {
      await result.current.joinServer(1);
    });

    expect([...result.current.joiningAccounts]).toEqual([1]);
    expect(result.current.launchProgress).toMatchObject({
      mode: "single",
      current: 1,
      total: 1,
      userId: 1,
    });

    await act(async () => {
      vi.advanceTimersByTime(7000);
    });

    expect(result.current.joiningAccounts.size).toBe(0);
    expect(result.current.launchProgress).toBeNull();
  });

  it("reports a launch failure without throwing and clears the joining state", async () => {
    const { result } = await setup();
    failures.set("launch_roblox", "backend exploded");

    await act(async () => {
      await result.current.joinServer(1);
    });

    expect(result.current.error).toBe("backend exploded");
    expect(result.current.joiningAccounts.size).toBe(0);
    expect(result.current.launchProgress).toBeNull();
    expect(result.current.actionStatus?.tone).toBe("error");
    expect(recordRecentGameMock).not.toHaveBeenCalled();
  });

  it("traduz a recusa do backend quando já há um launch em andamento", async () => {
    // O backend recusa com um código; despejá-lo na tela ("Launch failed:
    // launch-already-active") não diz nada a quem clicou duas vezes.
    const { result } = await setup();
    failures.set("launch_roblox", "launch-already-active");

    // E diz que não começou: quem chamou precisa saber, senão a tela mostra
    // "seguindo com 1 conta..." em cima do aviso de recusa.
    await act(async () => {
      await expect(result.current.joinServer(1)).resolves.toBe("refused");
    });

    expect(result.current.toasts.map((toast) => toast.message)).toContain(
      "A launch is already in progress"
    );
    // Uma frase de uma linha não precisa de três lugares: o toast basta aqui, e a
    // tela que disparou o launch ainda mostra a sua linha inline. A linha de
    // status do rodapé seria uma terceira cópia simultânea.
    expect(result.current.actionStatus?.message).not.toBe("A launch is already in progress");
    // Recusa não é falha do app: a faixa vermelha de erro não aparece.
    expect(result.current.error).toBeNull();
    expect(result.current.joiningAccounts.size).toBe(0);
    expect(result.current.launchProgress).toBeNull();
  });

  /**
   * O launch escreve "Launching X…" no rodapé, com 5 s de duração, antes do
   * `invoke`. A recusa volta na hora e só vira toast (o rodapé ficou de fora de
   * propósito, ver `reportLaunchAlreadyActive`) — e a linha seguia 5 s dizendo
   * que lançava a conta que acabou de ser recusada. A recusa retira a linha.
   */
  it("depois da recusa o rodapé não segue dizendo que está lançando", async () => {
    const { result } = await setup();
    failures.set("launch_roblox", "launch-already-active");

    await act(async () => {
      await result.current.joinServer(1);
    });

    expect(result.current.actionStatus).toBeNull();
  });

  it("a recusa só retira a própria linha, não a que outra ação escreveu no meio", async () => {
    const { result } = await setup();
    let recusar = () => {};
    results.set(
      "launch_roblox",
      new Promise((_resolve, reject) => {
        recusar = () => reject("launch-already-active");
      })
    );

    let pending: Promise<unknown> | null = null;
    await act(async () => {
      pending = result.current.joinServer(1);
      await Promise.resolve();
    });
    // Enquanto o launch espera o backend, outra tela escreve no rodapé.
    act(() => {
      window.dispatchEvent(
        new CustomEvent("ram-action-status", { detail: { message: "Settings saved" } })
      );
    });
    await act(async () => {
      recusar();
      await pending;
    });

    expect(result.current.actionStatus?.message).toBe("Settings saved");
  });

  it("uma falha comum de launch não vira exceção, mas também não vira sucesso", async () => {
    // O irmão do bug da recusa: a tela anunciava "seguindo com 1 conta..." em
    // cima da faixa vermelha de erro porque o launch de uma conta engolia a
    // falha e quem chamou não tinha como saber.
    const { result } = await setup();
    failures.set("launch_roblox", "version-conflict");

    await act(async () => {
      await expect(result.current.joinServer(1)).resolves.toBe("failed");
    });

    expect(result.current.error).toBe("version-conflict");
    expect(result.current.actionStatus?.tone).toBe("error");
  });

  it("diz que começou quando o backend aceitou o launch", async () => {
    const { result } = await setup();

    await act(async () => {
      await expect(result.current.joinServer(1)).resolves.toBe("started");
    });
  });

  it("announces the account alias in the action status while launching", async () => {
    const { result } = await setup();
    let releaseLaunch = () => {};
    results.set(
      "launch_roblox",
      new Promise<null>((resolve) => {
        releaseLaunch = () => resolve(null);
      })
    );

    let pending: Promise<unknown> | null = null;
    await act(async () => {
      pending = result.current.joinServer(1);
      await Promise.resolve();
    });

    expect(result.current.actionStatus?.message).toContain("Main");

    await act(async () => {
      releaseLaunch();
      await pending;
    });

    // O launch que voltou é um fato: vira toast. `actionStatus` continua
    // mostrando o que está em curso (esta conta) até o próprio timeout dele.
    expect(result.current.toasts.map((toast) => toast.message)).toContain("Launching game...");
    expect(result.current.actionStatus?.message).toContain("Main");
  });
});

describe("launchMultiple", () => {
  async function setup(general: Record<string, string> = {}) {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 }), account({ UserID: 3 })];
    settingsData = { General: { ...general } };
    return renderStore();
  }

  it("does nothing for an empty selection", async () => {
    const { result } = await setup();
    await act(async () => {
      await result.current.launchMultiple([]);
    });
    expect(invokeCalls("launch_multiple")).toHaveLength(0);
  });

  it("sends userIds, place, job and launchData", async () => {
    const { result } = await setup({
      SavedPlaceId: "777",
      SavedJobId: "job-1",
      SavedLaunchData: "data",
    });

    await act(async () => {
      await result.current.launchMultiple([1, 2]);
    });

    expect(lastArgs("launch_multiple")).toEqual({
      userIds: [1, 2],
      placeId: 777,
      jobId: "job-1",
      launchData: "data",
      shuffleJob: false,
    });
  });

  it("encodes a VIP target as a vip:<code> job id", async () => {
    const { result } = await setup({ SavedPlaceId: "777" });

    await act(async () => {
      await result.current.launchMultiple([1, 2], {
        joinVip: true,
        linkCode: "  code-9  ",
        jobId: "ignored",
      });
    });

    expect(lastArgs("launch_multiple")).toMatchObject({ jobId: "vip:code-9" });
  });

  it("falls back to the job field's own code when joinVip has no link code", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.launchMultiple([1], {
        joinVip: true,
        linkCode: "",
        jobId: "vip:fallback-code",
      });
    });

    expect(lastArgs("launch_multiple")).toMatchObject({ jobId: "vip:fallback-code" });
  });

  it("sends the plain job when joinVip has no code anywhere", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.launchMultiple([1], { joinVip: true, linkCode: "", jobId: "raw-job" });
    });

    // Nothing to join privately with; a plain job is the only sane request.
    expect(lastArgs("launch_multiple")).toMatchObject({ jobId: "raw-job" });
  });

  it("parses a private-server link pasted into the job field, like joinServer", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.launchMultiple([1], { jobId: "?linkCode=abc" });
    });

    expect(lastArgs("launch_multiple")).toMatchObject({ jobId: "vip:abc" });
  });

  it("parses a full private-server URL pasted into the job field", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.launchMultiple([1], {
        jobId: "https://www.roblox.com/games/606849621/X?privateServerLinkCode=99887766",
      });
    });

    expect(lastArgs("launch_multiple")).toMatchObject({ jobId: "vip:99887766" });
  });

  it("keeps a plain job id untouched", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.launchMultiple([1], { jobId: "abc-123-def" });
    });

    expect(lastArgs("launch_multiple")).toMatchObject({ jobId: "abc-123-def" });
  });

  it("passa o shuffleJob para o backend sortear um servidor por conta", async () => {
    const { result } = await setup({ ShuffleJobId: "true" });

    await act(async () => {
      await result.current.launchMultiple([1, 2]);
    });

    expect(lastArgs("launch_multiple")).toMatchObject({ shuffleJob: true });
  });

  it("manda shuffleJob desligado quando a opção está desmarcada", async () => {
    const { result } = await setup();

    await act(async () => {
      await result.current.launchMultiple([1]);
    });

    expect(lastArgs("launch_multiple")).toMatchObject({ shuffleJob: false });
  });

  it("falls back to placeId 5315046213", async () => {
    const { result } = await setup();
    await act(async () => {
      await result.current.launchMultiple([1]);
    });
    expect(lastArgs("launch_multiple")).toMatchObject({ placeId: 5315046213 });
  });

  it("records the recent game for the first account", async () => {
    const { result } = await setup({ SavedPlaceId: "99", MaxRecentGames: "5" });
    await act(async () => {
      await result.current.launchMultiple([3, 1]);
    });
    expect(recordRecentGameMock).toHaveBeenCalledWith(99, 3, 5);
  });

  /**
   * Num launch em lote o alvo privado é das contas **todas** que entraram: é o
   * que decide para quem ele volta a aparecer na lista de recentes.
   */
  it("records the recent server for every account that launched", async () => {
    const { result } = await setup({ SavedPlaceId: "99", MaxRecentJobs: "6" });
    await act(async () => {
      await result.current.launchMultiple([3, 1], { jobId: "vip:abc123" });
    });
    expect(addRecentJobMock).toHaveBeenCalledWith("vip:abc123", 99, 6, [3, 1]);
  });

  /**
   * Mesmo caso do launch único, e aqui é pior: o `catch` do `launchMultiple`
   * **relança**, então uma gravação recusada pelo `localStorage` interromperia
   * o que a Choose Game faz depois de um lote que já subiu os clientes.
   */
  it("does not fail or rethrow when recording the recent server throws", async () => {
    addRecentJobMock.mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    const { result } = await setup({ SavedPlaceId: "99" });

    await act(async () => {
      await expect(
        result.current.launchMultiple([3, 1], { jobId: "job-xyz" })
      ).resolves.toBeUndefined();
    });

    expect(result.current.error).toBeNull();
  });

  it("refuses multi-launch on an unsupported Linux runner and reports the reason", async () => {
    capabilitiesData = caps({
      os: "linux",
      supportsMultiLaunch: false,
      reasons: ["runner incompatible"],
    });
    const { result } = await setup();

    await act(async () => {
      await expect(result.current.launchMultiple([1, 2])).rejects.toThrow("runner incompatible");
    });

    expect(invokeCalls("launch_multiple")).toHaveLength(0);
    expect(result.current.error).toBe("runner incompatible");
  });

  it("still allows a single account on that Linux runner", async () => {
    capabilitiesData = caps({ os: "linux", supportsMultiLaunch: false, reasons: ["nope"] });
    const { result } = await setup();

    await act(async () => {
      await result.current.launchMultiple([1]);
    });

    expect(invokeCalls("launch_multiple")).toHaveLength(1);
  });

  it("rethrows backend failures and clears progress", async () => {
    const { result } = await setup();
    failures.set("launch_multiple", "multi failed");

    await act(async () => {
      await expect(result.current.launchMultiple([1, 2])).rejects.toBeTruthy();
    });

    expect(result.current.error).toBe("multi failed");
    expect(result.current.joiningAccounts.size).toBe(0);
    expect(result.current.launchProgress).toBeNull();
  });

  it("traduz a recusa do backend quando já há um launch em andamento", async () => {
    const { result } = await setup();
    failures.set("launch_multiple", "launch-already-active");

    await act(async () => {
      await expect(result.current.launchMultiple([1, 2])).rejects.toBeTruthy();
    });

    expect(result.current.toasts.map((toast) => toast.message)).toContain(
      "A launch is already in progress"
    );
    // Ver o launch de uma conta: a recusa sai no toast, sem repetir no rodapé.
    expect(result.current.actionStatus?.message).not.toBe("A launch is already in progress");
    expect(result.current.error).toBeNull();
    expect(result.current.joiningAccounts.size).toBe(0);
    expect(result.current.launchProgress).toBeNull();
  });

  it("depois da recusa o rodapé não segue dizendo que está lançando", async () => {
    // O mesmo do launch de uma conta: "Launching 2 accounts..." ficava 5 s no
    // rodapé em cima de um lote que o backend recusou.
    const { result } = await setup();
    failures.set("launch_multiple", "launch-already-active");

    await act(async () => {
      await expect(result.current.launchMultiple([1, 2])).rejects.toBeTruthy();
    });

    expect(result.current.actionStatus).toBeNull();
  });
});

describe("restartRobloxClients", () => {
  it("does nothing when no selected account was launched by the app", async () => {
    accountsData = [account({ UserID: 1 })];
    const { result } = await renderStore();

    await act(async () => {
      await result.current.restartRobloxClients([1]);
    });

    expect(invokeCalls("cmd_kill_roblox")).toHaveLength(0);
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/No launched Roblox clients/i);
  });

  it("closes and relaunches the launched clients", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 })];
    runningInstances = [{ userId: 1 }, { user_id: 2 }];
    const { result } = await renderStore();

    await waitFor(() => expect(result.current.launchedByProgram.size).toBe(2));

    await act(async () => {
      await result.current.restartRobloxClients([1, 2, 2]);
    });

    expect(invokeCalls("cmd_kill_roblox")).toHaveLength(2);
    expect(invokeCalls("launch_multiple")).toHaveLength(1);
    expect(lastArgs("launch_multiple")).toMatchObject({ userIds: [1, 2] });
  });

  it("uses joinServer for a single launched client", async () => {
    accountsData = [account({ UserID: 1 })];
    runningInstances = [{ userId: 1 }];
    const { result } = await renderStore();
    await waitFor(() => expect(result.current.launchedByProgram.size).toBe(1));

    await act(async () => {
      await result.current.restartRobloxClients([1]);
    });

    expect(invokeCalls("launch_roblox")).toHaveLength(1);
  });
});

describe("clients opened outside the app", () => {
  it("polls the unidentified clients and marks the adopted ones", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 })];
    runningInstances = [
      { user_id: 1, pid: 10, adopted: false },
      { user_id: 2, pid: 20, adopted: true },
    ];
    results.set("get_unidentified_clients", [
      { pid: 30, reason: "waitingForGame", userId: null, placeId: null, jobId: null, startedAtMs: 1 },
    ]);
    const { result } = await renderStore();

    await waitFor(() => expect(result.current.unidentifiedClients).toHaveLength(1));
    expect(result.current.unidentifiedClients[0].pid).toBe(30);
    expect([...result.current.adoptedClients]).toEqual([2]);
    expect(result.current.launchedByProgram.has(2)).toBe(true);
  });

  it("an old backend without the command leaves the list empty", async () => {
    failures.set("get_unidentified_clients", "unknown command");
    const { result } = await renderStore();
    await waitFor(() => expect(invokeCalls("get_unidentified_clients").length).toBeGreaterThan(0));
    expect(result.current.unidentifiedClients).toEqual([]);
  });

  it("identifying a client sends the pid and the account, then refreshes", async () => {
    accountsData = [account({ UserID: 1 })];
    results.set("identify_external_client", true);
    const { result } = await renderStore();
    const before = invokeCalls("get_running_instances").length;

    await act(async () => {
      await result.current.identifyExternalClient(30, 1);
    });

    expect(lastArgs("identify_external_client")).toEqual({ pid: 30, userId: 1 });
    expect(invokeCalls("get_running_instances").length).toBeGreaterThan(before);
  });

  it("showing a client's window goes by pid", async () => {
    results.set("focus_client_window", true);
    const { result } = await renderStore();
    let ok = false;
    await act(async () => {
      ok = await result.current.focusClientWindow(30);
    });
    expect(ok).toBe(true);
    expect(lastArgs("focus_client_window")).toEqual({ pid: 30 });
  });
});

describe("account mutations", () => {
  it("adds an account by cookie and reports whether it was new", async () => {
    results.set("validate_cookie", { user_id: 7, name: "Cookie" });
    const { result } = await renderStore();

    await act(async () => {
      await result.current.addAccountByCookie("_|WARNING:-token");
    });

    expect(lastArgs("add_account")).toEqual({
      securityToken: "_|WARNING:-token",
      username: "Cookie",
      userId: 7,
    });
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("Added Cookie");
  });

  /**
   * O Quick Add passou a aceitar a linha do import (`username:password:cookie`):
   * a senha vai para o `add_account` separada, como no import — nunca dentro
   * do cookie. Sem senha, a chamada fica exatamente como era.
   */
  it("guarda a senha junto quando o cookie veio de uma linha user:pass:cookie", async () => {
    results.set("validate_cookie", { user_id: 7, name: "Cookie" });
    const { result } = await renderStore();

    await act(async () => {
      await result.current.addAccountByCookie("_|WARNING:-token", "hunter2");
    });

    expect(lastArgs("validate_cookie")).toEqual({ cookie: "_|WARNING:-token" });
    expect(lastArgs("add_account")).toEqual({
      securityToken: "_|WARNING:-token",
      username: "Cookie",
      userId: 7,
      password: "hunter2",
    });
  });

  it("says 'Updated' when the account already exists", async () => {
    accountsData = [account({ UserID: 7 })];
    results.set("validate_cookie", { user_id: 7, name: "Cookie" });
    const { result } = await renderStore();

    await act(async () => {
      await result.current.addAccountByCookie("token");
    });

    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("Updated Cookie");
  });

  it("removes accounts and drops them from the selection", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 })];
    const { result } = await renderStore();
    act(() => result.current.setSelectedIds(new Set([1, 2])));

    accountsData = [account({ UserID: 2 })];
    await act(async () => {
      await result.current.removeAccounts([1]);
    });

    expect(lastArgs("remove_account")).toEqual({ userId: 1 });
    expect([...result.current.selectedIds]).toEqual([2]);
    expect(result.current.accounts.map((a) => a.UserID)).toEqual([2]);
  });

  it("updates an account in place", async () => {
    accountsData = [account({ UserID: 1, Alias: "old" })];
    const { result } = await renderStore();

    await act(async () => {
      await result.current.updateAccount({ ...result.current.accounts[0], Alias: "new" });
    });

    expect(result.current.accounts[0].Alias).toBe("new");
    expect(invokeCalls("update_account")).toHaveLength(1);
  });

  it("moves accounts to a group and persists each one", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 })];
    const { result } = await renderStore();

    await act(async () => {
      await result.current.moveToGroup([1], "10 Bots");
    });

    expect(result.current.accounts[0].Group).toBe("10 Bots");
    expect(invokeCalls("update_account")).toHaveLength(1);
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("Bots");
  });

  it("sorts a group alphabetically by alias or username and persists the order", async () => {
    accountsData = [
      account({ UserID: 1, Username: "zeta", Group: "G" }),
      account({ UserID: 2, Username: "alpha", Group: "G" }),
      account({ UserID: 3, Username: "mid", Group: "Other" }),
    ];
    const { result } = await renderStore();

    act(() => result.current.sortGroupAlphabetically("G"));

    expect(result.current.accounts.map((a) => a.UserID)).toEqual([2, 1, 3]);
    expect(lastArgs("reorder_accounts")).toEqual({ userIds: [2, 1, 3] });
  });

  it("leaves a one-account group untouched", async () => {
    accountsData = [account({ UserID: 1, Group: "G" })];
    const { result } = await renderStore();

    act(() => result.current.sortGroupAlphabetically("G"));

    expect(invokeCalls("reorder_accounts")).toHaveLength(0);
  });

  it("reorders accounts by drag and drop", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 }), account({ UserID: 3 })];
    const { result } = await renderStore();

    await act(async () => {
      await result.current.reorderAccounts(3, 1);
    });

    expect(result.current.accounts.map((a) => a.UserID)).toEqual([3, 1, 2]);
    expect(lastArgs("reorder_accounts")).toEqual({ userIds: [3, 1, 2] });
  });

  it("ignores a reorder onto itself or onto an unknown account", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 })];
    const { result } = await renderStore();

    await act(async () => {
      await result.current.reorderAccounts(1, 1);
      await result.current.reorderAccounts(1, 99);
    });

    expect(invokeCalls("reorder_accounts")).toHaveLength(0);
  });
});

describe("saved launch fields", () => {
  it("persists placeId, jobId and launchData as they change", async () => {
    const { result } = await renderStore();

    act(() => result.current.setPlaceId("123"));
    act(() => result.current.setJobId("job"));
    act(() => result.current.setLaunchData("data"));
    act(() => result.current.setHideUsernames(true));

    const saved = invokeCalls("update_setting").map((c) => c[1] as Record<string, string>);
    expect(saved).toEqual(
      expect.arrayContaining([
        { section: "General", key: "SavedPlaceId", value: "123" },
        { section: "General", key: "SavedJobId", value: "job" },
        { section: "General", key: "SavedLaunchData", value: "data" },
        { section: "General", key: "HideUsernames", value: "true" },
      ])
    );
    expect(result.current.placeId).toBe("123");
  });
});

describe("toasts and action status", () => {
  /** Tom de cada toast da fila, na ordem em que entraram. */
  const tones = (result: { current: StoreValue }) => result.current.toasts.map((toast) => toast.tone);

  it("derives an error tone from the message and clears after the timeout", async () => {
    const { result } = await renderStore();
    vi.useFakeTimers();

    act(() => result.current.addToast("Something failed"));
    expect(result.current.toasts).toMatchObject([{ message: "Something failed", tone: "error" }]);
    // O toast diz "isto acabou de acontecer"; `actionStatus` é só progresso, e
    // um toast não escreve mais nele (a frase apareceria duas vezes na tela).
    expect(result.current.actionStatus).toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(2500);
    });
    expect(result.current.toasts).toEqual([]);
  });

  it("gives every toast an id of its own so the queue keeps its identity", async () => {
    const { result } = await renderStore();

    act(() => result.current.addToast("Accounts saved"));
    act(() => result.current.addToast("Alias updated"));

    const ids = result.current.toasts.map((toast) => toast.id);
    expect(new Set(ids).size).toBe(2);
    expect(result.current.toasts.map((toast) => toast.message)).toEqual([
      "Accounts saved",
      "Alias updated",
    ]);
  });

  /**
   * Trava o tempo de vida por toast: cada um sai no seu proprio timeout, sem
   * levar o vizinho. Vale dizer o que este teste **nao** prova: com vidas iguais
   * e fila FIFO, remover por `slice(1)` da no mesmo resultado aqui — o ganho real
   * do `id` e a estabilidade da `key` no React, e quem guarda isso e
   * "keeps a toast mounted when an older one leaves the queue" em App.test.tsx.
   */
  it("o toast que expira leva embora so ele, e o vizinho vivo continua", async () => {
    const { result } = await renderStore();
    vi.useFakeTimers();

    act(() => result.current.addToast("Accounts saved"));
    await act(async () => {
      vi.advanceTimersByTime(1500);
    });
    act(() => result.current.addToast("Alias updated"));

    // 1000 ms a mais: o primeiro completa 2500 ms e sai; o segundo esta na metade.
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(result.current.toasts.map((toast) => toast.message)).toEqual(["Alias updated"]);

    await act(async () => {
      vi.advanceTimersByTime(1500);
    });
    expect(result.current.toasts).toEqual([]);
  });

  /**
   * O heuristico existe para os ~200 call sites que nao se importam com o tom.
   * Quem sabe o tom da propria mensagem tem de poder dizer: "Aviso de copia de
   * credencial desligado" e confirmacao de ajuste, mas cai em `warn` so porque a
   * frase contem a palavra "aviso".
   */
  it("aceita tom explicito, que vence o heuristico do texto", async () => {
    const { result } = await renderStore();

    const last = () => result.current.toasts[result.current.toasts.length - 1];

    act(() => result.current.addToast("Credential copy warning disabled", "info"));
    expect(last()).toMatchObject({ tone: "info" });

    act(() => result.current.addToast("Some warning here"));
    expect(last()).toMatchObject({ tone: "warn" });
  });

  it("uses a success tone for saved/updated/launched and a warn tone for warnings", async () => {
    const { result } = await renderStore();

    act(() => result.current.addToast("Accounts saved"));
    act(() => result.current.addToast("Some warning here"));
    act(() => result.current.addToast("Neutral message"));

    expect(tones(result)).toEqual(["success", "warn", "info"]);
  });

  it("keeps the tone when the call site already localized the message", async () => {
    // 162 call sites chamam `addToast(tr("..."))`, ou seja entregam a frase já
    // traduzida. Se o tom fosse deduzido só de palavra inglesa, todo erro em
    // português cairia como `info` — o toast de falha ficaria igual ao de sucesso.
    const { result } = await renderStore();

    act(() => result.current.addToast("Não foi possível fechar o Roblox: acesso negado"));
    act(() => result.current.addToast("A busca falhou: tempo esgotado"));
    act(() => result.current.addToast("Configurações salvas"));
    act(() => result.current.addToast("Apelido atualizado"));
    act(() => result.current.addToast("Aviso de otimização: memória baixa"));
    act(() => result.current.addToast("Iniciando o jogo..."));

    expect(tones(result)).toEqual(["error", "error", "success", "success", "warn", "info"]);
  });

  it("reacts to the ram-action-status window event", async () => {
    const { result } = await renderStore();

    act(() => {
      window.dispatchEvent(
        new CustomEvent("ram-action-status", { detail: { message: "Settings saved" } })
      );
    });

    expect(result.current.actionStatus).toMatchObject({ message: "Settings saved", tone: "success" });
  });

  it("ignores a ram-action-status event without a message", async () => {
    const { result } = await renderStore();
    act(() => {
      window.dispatchEvent(new CustomEvent("ram-action-status", { detail: {} }));
    });
    expect(result.current.actionStatus).toBeNull();
  });

  it("opens and closes the modal", async () => {
    const { result } = await renderStore();
    act(() => result.current.showModal("Title", "Body"));
    expect(result.current.modal).toEqual({ title: "Title", content: "Body" });
    act(() => result.current.closeModal());
    expect(result.current.modal).toBeNull();
  });

  it("opens and closes the context menu", async () => {
    const { result } = await renderStore();
    act(() => result.current.openContextMenu(12, 34));
    expect(result.current.contextMenu).toEqual({ x: 12, y: 34 });
    act(() => result.current.closeContextMenu());
    expect(result.current.contextMenu).toBeNull();
  });

  /**
   * O Auto Rejoin pode ser aberto por um jogo (clique direito na lista) ou pela
   * barra. Abrir pela barra tem que **limpar** o jogo da abertura anterior,
   * senão o place escolhido num clique direito continuaria carimbando a tela.
   */
  it("carrega o jogo escolhido ao abrir o Auto Rejoin, e o limpa quando não há jogo", async () => {
    const { result } = await renderStore();

    act(() => result.current.openBottingDialog("606849621"));
    expect(result.current.afkModeDialog).toEqual({ tab: "rejoin", placeId: "606849621" });

    act(() => result.current.closeAfkMode());
    expect(result.current.afkModeDialog).toBeNull();
    act(() => result.current.openBottingDialog());
    expect(result.current.afkModeDialog).toEqual({ tab: "rejoin", placeId: null });
  });

  it("place em branco na abertura conta como sem jogo", async () => {
    const { result } = await renderStore();
    act(() => result.current.openBottingDialog("   "));
    expect(result.current.afkModeDialog?.placeId).toBeNull();
  });

  /**
   * Auto Rejoin e cliques AFK moram na mesma janela (Modo AFK). Quem abre pela
   * barra (o atalho de AFK) cai na aba de cliques; quem abre pelo "Em jogo" leva
   * as contas e o caminho de adoção junto.
   */
  it("o Modo AFK abre na aba pedida e com as contas de quem abriu", async () => {
    const { result } = await renderStore();

    act(() => result.current.setAfkDialogOpen(true));
    expect(result.current.afkModeDialog).toEqual({ tab: "clicks" });
    act(() => result.current.setAfkDialogOpen(false));
    expect(result.current.afkModeDialog).toBeNull();

    act(() =>
      result.current.openAfkMode({ tab: "rejoin", targetUserIds: [1, 2], adoptRunning: true })
    );
    expect(result.current.afkModeDialog).toEqual({
      tab: "rejoin",
      targetUserIds: [1, 2],
      adoptRunning: true,
    });

    // Sem aba pedida, abre nos cliques AFK (a aba padrão).
    act(() => result.current.openAfkMode({ targetUserIds: [1] }));
    expect(result.current.afkModeDialog).toEqual({ tab: "clicks", targetUserIds: [1] });
  });
});

/**
 * Ligar o Auto Rejoin numa conta que ja esta jogando: o caminho antigo (abrir o
 * dialogo e dar Start) fecha e relanca todo mundo, tirando as contas do
 * servidor em que estavam.
 */
describe("adotar contas em jogo no Botting", () => {
  it("com sessao ativa, so entra nela — sem place nem relancamento", async () => {
    results.set("get_botting_mode_status", {
      active: true,
      startedAtMs: 1,
      placeId: 606849621,
      jobId: "",
      intervalMinutes: 19,
      launchDelaySeconds: 20,
      playerGraceMinutes: 15,
      userIds: [1],
      accounts: [],
    });
    results.set("add_botting_accounts", {
      active: true,
      startedAtMs: 1,
      placeId: 606849621,
      jobId: "",
      intervalMinutes: 19,
      launchDelaySeconds: 20,
      playerGraceMinutes: 15,
      userIds: [1, 2],
      accounts: [],
    });
    const { result } = await renderStore();
    await waitFor(() => expect(result.current.bottingStatus?.active).toBe(true));

    await act(async () => {
      await result.current.adoptRunningIntoBotting([2]);
    });

    expect(invokeCalls("add_botting_accounts")).toHaveLength(1);
    expect(invokeCalls("start_botting_mode")).toHaveLength(0);
    // Nao precisa perguntar a presenca: a sessao ja tem o place dela.
    expect(invokeCalls("get_account_game_location")).toHaveLength(0);
  });

  it("sem sessao, o place vem da presenca da conta e nada e relancado", async () => {
    results.set("get_account_game_location", {
      userId: 1,
      inGame: true,
      placeId: 606849621,
      jobId: "job-abc",
    });
    const { result } = await renderStore();

    await act(async () => {
      await result.current.adoptRunningIntoBotting([1, 2]);
    });

    const [, args] = invokeCalls("start_botting_mode")[0];
    expect(args).toMatchObject({
      userIds: [1, 2],
      placeId: 606849621,
      adoptRunning: true,
      // O job fica de fora: o ciclo relanca no place, e fixar o servidor atual
      // mandaria todo reinicio para um servidor que pode nem existir mais.
      jobId: "",
    });
  });

  it("sem saber onde a conta esta, avisa em vez de chutar um place", async () => {
    results.set("get_account_game_location", {
      userId: 1,
      inGame: false,
      placeId: null,
      jobId: null,
    });
    const { result } = await renderStore();

    await expect(
      act(async () => {
        await result.current.adoptRunningIntoBotting([1, 2]);
      })
    ).rejects.toThrow(/which game/i);
    expect(invokeCalls("start_botting_mode")).toHaveLength(0);
  });

  /**
   * Pelo Modo AFK, quem adota vê e ajusta o tempo do ciclo e as contas main
   * antes do Start: o que foi escolhido na tela vale, e o place que a tela
   * mostrou (detectado da presença ou digitado) também.
   */
  it("com a configuracao da tela, usa o tempo, as mains e o place escolhidos", async () => {
    const { result } = await renderStore();

    await act(async () => {
      await result.current.adoptRunningIntoBotting([1, 2], {
        placeId: 1818,
        intervalMinutes: 25,
        launchDelaySeconds: 12,
        playerGraceMinutes: 7,
        playerUserIds: [1],
      });
    });

    const [, args] = invokeCalls("start_botting_mode")[0];
    expect(args).toMatchObject({
      userIds: [1, 2],
      placeId: 1818,
      intervalMinutes: 25,
      launchDelaySeconds: 12,
      playerGraceMinutes: 7,
      playerUserIds: [1],
      adoptRunning: true,
      jobId: "",
    });
    // O place veio da tela: não pergunta a presença de novo.
    expect(invokeCalls("get_account_game_location")).toHaveLength(0);
  });

  it("detecta o jogo das contas em jogo pela presença", async () => {
    results.set("get_account_game_location", {
      userId: 1,
      inGame: true,
      placeId: 606849621,
      jobId: null,
    });
    const { result } = await renderStore();

    let place: number | null = null;
    await act(async () => {
      place = await result.current.detectRunningGamePlace([1, 2]);
    });
    expect(place).toBe(606849621);
  });

  it("uma conta so, sem sessao, explica o minimo em vez de falhar no backend", async () => {
    const { result } = await renderStore();

    await expect(
      act(async () => {
        await result.current.adoptRunningIntoBotting([1]);
      })
    ).rejects.toThrow(/two accounts/i);
    expect(invokeCalls("get_account_game_location")).toHaveLength(0);
  });
});

describe("backend events", () => {
  /**
   * Make Friends acompanhado como a fila de launch: retrato inicial pelo
   * comando (para a tela que abre no meio da operação) e evento com o payload
   * completo a cada mudança. Antes o progresso vivia em `useState` de dois
   * componentes, e remontar significava perder tudo.
   */
  it("carrega e acompanha o estado do Make Friends", async () => {
    results.set("get_friend_link_state", {
      active: true,
      phase: "checking",
      processed: 0,
      total: 2,
      accounts: [
        { userId: 1, state: "processing", error: null },
        { userId: 2, state: "pending", error: null },
      ],
      mode: "mesh",
      mainUserId: null,
    });
    const { result } = await renderStore();

    await waitFor(() => expect(result.current.friendLinkState?.total).toBe(2));
    expect(result.current.friendLinkState?.phase).toBe("checking");

    await waitFor(() => expect(listenHandlers.has("friend-link-state")).toBe(true));
    act(() =>
      emit("friend-link-state", {
        active: false,
        phase: "done",
        processed: 2,
        total: 2,
        accounts: [
          { userId: 1, state: "done", error: null },
          { userId: 2, state: "failed", error: "cookie inválido" },
        ],
        mode: "mesh",
        mainUserId: null,
      })
    );

    // O payload substitui o anterior inteiro: nada de mesclar deltas.
    expect(result.current.friendLinkState?.active).toBe(false);
    expect(result.current.friendLinkState?.accounts[1]).toEqual({
      userId: 2,
      state: "failed",
      error: "cookie inválido",
    });
  });

  it("appends launch logs and caps the buffer at 500 entries", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("launch-log")).toBe(true));

    act(() => {
      emit("launch-log", { userId: 1, level: "warn", step: "prep", message: "hello" });
    });
    expect(result.current.launchLogs).toHaveLength(1);
    expect(result.current.launchLogs[0]).toMatchObject({
      userId: 1,
      level: "warn",
      step: "prep",
      message: "hello",
    });

    act(() => {
      for (let i = 0; i < 520; i++) emit("launch-log", { message: `m${i}` });
    });
    expect(result.current.launchLogs).toHaveLength(500);
    expect(result.current.launchLogs[499].message).toBe("m519");
    // defaults for a payload without level/step/userId
    expect(result.current.launchLogs[499]).toMatchObject({ level: "info", step: "", userId: null });

    act(() => result.current.clearLaunchLogs());
    expect(result.current.launchLogs).toEqual([]);
  });

  it("tracks multi-launch progress and completion", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("launch-progress")).toBe(true));

    act(() => emit("launch-progress", { userId: 5, index: 1, total: 3 }));
    expect(result.current.launchProgress).toEqual({
      mode: "multi",
      current: 2,
      total: 3,
      userId: 5,
    });
    expect([...result.current.joiningAccounts]).toEqual([5]);

    act(() => emit("launch-complete", {}));
    expect(result.current.joiningAccounts.size).toBe(0);
    expect(result.current.launchProgress).toMatchObject({ current: 3, total: 3 });
    expect(result.current.actionStatus?.tone).toBe("success");
  });

  it("clamps the reported progress index to the total", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("launch-progress")).toBe(true));
    act(() => emit("launch-progress", { userId: 1, index: 9, total: 2 }));
    expect(result.current.launchProgress).toMatchObject({ current: 2, total: 2 });
  });

  it("reloads accounts and warns when an account gets moderated", async () => {
    accountsData = [account({ UserID: 1, Alias: "Alpha" })];
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("account-moderated")).toBe(true));

    await act(async () => {
      emit("account-moderated", { userId: 1 });
    });

    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("Alpha");
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("moderadas");
  });

  /**
   * Os toasts que nomeiam uma conta saíam com o nome real com "Names hidden"
   * ligado — inclusive o deste listener, montado uma vez só no boot.
   */
  it("masks the account name in toasts while names are hidden", async () => {
    accountsData = [account({ UserID: 1, Alias: "Alpha" })];
    settingsData = { General: { HideUsernames: "true", HiddenNameLetters: "0" } };
    results.set("validate_cookie", { user_id: 7, name: "SecretCookie" });
    const { result } = await renderStore();
    await waitFor(() => expect(result.current.hideUsernames).toBe(true));
    await waitFor(() => expect(listenHandlers.has("account-moderated")).toBe(true));

    await act(async () => {
      emit("account-moderated", { userId: 1 });
    });
    await act(async () => {
      await result.current.addAccountByCookie("_|WARNING:-token");
    });
    const shown = result.current.toasts.map((toast) => toast.message).join(" | ");
    expect(shown).toContain("************ is moderated");
    expect(shown).toContain("Added ************");
    expect(shown).not.toMatch(/Alpha|SecretCookie/);

    // A linha "Launching <conta>..." do rodapé, com o launch ainda em curso.
    results.set("launch_roblox", new Promise(() => {}));
    act(() => {
      void result.current.joinServer(1);
    });
    await waitFor(() => expect(result.current.actionStatus?.message).toBe("Launching ************..."));
    // O backend continua recebendo o nome de verdade.
    expect(lastArgs("add_account")).toMatchObject({ username: "SecretCookie" });
  });

  /**
   * O login pelo navegador termina sem cookie quando o Roblox não devolveu a
   * sessão — é falha, e a frase antiga não tinha marcador nenhum: o toast saía
   * cinza de `info`, igual a um "Iniciando o jogo...".
   */
  it("reports a browser login that produced no cookie as a failure", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("browser-login-detected")).toBe(true));
    results.set("extract_browser_cookie", "");

    vi.useFakeTimers();
    await act(async () => {
      emit("browser-login-detected", {});
      // O handler tenta 8 vezes, 350ms entre elas, antes de desistir.
      await vi.advanceTimersByTimeAsync(4000);
    });

    const toast = result.current.toasts[result.current.toasts.length - 1];
    expect(toast?.message).toMatch(/cookie/i);
    expect(toast?.tone).toBe("error");
  });

  it("surfaces roblox build install progress", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("roblox-build-install")).toBe(true));

    act(() => emit("roblox-build-install", { stage: "starting", current: 0, total: 0 }));
    expect(result.current.actionStatus?.message).toMatch(/Downloading the new Roblox version/);

    act(() => emit("roblox-build-install", { stage: "ready", current: 1, total: 1 }));
    expect(result.current.actionStatus).toMatchObject({ tone: "success" });

    act(() => emit("roblox-build-install", { stage: "error", current: 0, total: 0, message: "nope" }));
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("nope");
  });

  it("turns a chromium download event into a percentage status", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("chromium-download-progress")).toBe(true));

    act(() => emit("chromium-download-progress", { stage: "downloading", downloaded: 50, total: 200 }));
    expect(result.current.actionStatus?.message).toContain("25");

    act(() => emit("chromium-download-progress", { stage: "ready", downloaded: 0, total: 0 }));
    expect(result.current.actionStatus).toMatchObject({ tone: "success" });
  });

  /**
   * `browserDownload` é o que a Settings > General "Bundled Browser" usa para
   * desenhar a barra de progresso e o botão Download/Reinstall — trilha o
   * mesmo evento do teste acima, mas guarda estado em vez de só mostrar toast.
   */
  it("tracks the bundled-browser download stage in browserDownload", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("chromium-download-progress")).toBe(true));

    act(() => emit("chromium-download-progress", { stage: "resolving", downloaded: 0, total: 0 }));
    expect(result.current.browserDownload).toMatchObject({ active: true, stage: "resolving" });

    act(() => emit("chromium-download-progress", { stage: "downloading", downloaded: 50, total: 200 }));
    expect(result.current.browserDownload).toMatchObject({ active: true, stage: "downloading", percent: 25 });

    act(() => emit("chromium-download-progress", { stage: "extracting", downloaded: 0, total: 0 }));
    expect(result.current.browserDownload).toMatchObject({ active: true, stage: "extracting" });

    act(() => emit("chromium-download-progress", { stage: "ready", downloaded: 0, total: 0 }));
    expect(result.current.browserDownload).toMatchObject({ active: false, stage: "ready", percent: 100 });
  });

  it("repeating the same download percentage does not re-render browserDownload", async () => {
    // Um evento por ~2MB baixados vira várias mensagens com o mesmo
    // percentual arredondado; sem o dedupe a barra "tremia" em vez de andar.
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("chromium-download-progress")).toBe(true));

    act(() => emit("chromium-download-progress", { stage: "downloading", downloaded: 50, total: 200 }));
    const first = result.current.browserDownload;
    // Dois blocos de ~2MB podem arredondar para o mesmo percentual inteiro; o
    // dedupe olha o percentual final, não os bytes brutos de cada evento.
    act(() => emit("chromium-download-progress", { stage: "downloading", downloaded: 50.4, total: 200 }));
    expect(result.current.browserDownload).toBe(first);
  });

  it("shows a toast when the backend falls back to the system browser", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("chromium-fallback")).toBe(true));

    act(() => emit("chromium-fallback", { browser: "Microsoft Edge", error: "network down" }));

    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("Microsoft Edge");
  });

  it("ensureBrowserDownload reports success and forwards force to the backend", async () => {
    const { result } = await renderStore();

    await act(async () => {
      await result.current.ensureBrowserDownload(true);
    });

    expect(invokeMock).toHaveBeenCalledWith("ensure_browser", { force: true });
    expect(result.current.browserDownload).toMatchObject({ active: false, stage: "ready" });
  });

  it("ensureBrowserDownload surfaces a backend failure instead of throwing", async () => {
    failures.set("ensure_browser", "Could not reach browser download service");
    const { result } = await renderStore();

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.ensureBrowserDownload();
    });

    expect(ok).toBe(false);
    expect(result.current.browserDownload).toMatchObject({ active: false, stage: "error" });
    expect(result.current.browserDownload?.error).toContain("Could not reach browser download service");
  });

  it("marks botting as inactive when the backend stops it without a prior status", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("botting-stopped")).toBe(true));

    act(() => emit("botting-stopped", {}));

    expect(result.current.bottingStatus).toMatchObject({
      active: false,
      intervalMinutes: 19,
      launchDelaySeconds: 20,
      playerGraceMinutes: 15,
    });
  });

  it("stores botting and generator status payloads", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("botting-status")).toBe(true));

    act(() => emit("botting-status", { active: true, userIds: [1] }));
    expect(result.current.bottingStatus).toMatchObject({ active: true });

    act(() => emit("generator-status", { active: true, totalGenerated: 3 }));
    expect(result.current.generatorStatus).toMatchObject({ totalGenerated: 3 });

    act(() => emit("generator-stopped", {}));
    expect(result.current.generatorStatus).toMatchObject({ active: false });
  });

  it("warns when a botting rejoin cycle fails", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("botting-account-cycle")).toBe(true));

    act(() => emit("botting-account-cycle", { userId: 4, ok: false, error: "timeout" }));
    expect(result.current.actionStatus).toMatchObject({ tone: "warn" });
    expect(result.current.actionStatus?.message).toContain("timeout");

    const before = result.current.actionStatus;
    act(() => emit("botting-account-cycle", { userId: 4, ok: true }));
    expect(result.current.actionStatus).toBe(before);
  });

  it("formats optimization warnings with and without a pid", async () => {
    const { result } = await renderStore();
    await waitFor(() => expect(listenHandlers.has("roblox-optimization-warning")).toBe(true));

    act(() => emit("roblox-optimization-warning", { pid: 4242, message: "high cpu" }));
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("4242");

    act(() => emit("roblox-optimization-warning", { message: "  " }));
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("Unknown");
  });
});

describe("presence and running instances", () => {
  it("stays empty while ShowPresence is off", async () => {
    accountsData = [account({ UserID: 1 })];
    presenceRows = [{ userId: 1, userPresenceType: 2 }];
    const { result } = await renderStore();

    expect(invokeCalls("get_presence")).toHaveLength(0);
    expect(result.current.presenceByUserId.size).toBe(0);
  });

  it("maps camelCase and snake_case presence rows", async () => {
    accountsData = [account({ UserID: 1 }), account({ UserID: 2 }), account({ UserID: 3 })];
    settingsData = { General: { ShowPresence: "true" } };
    presenceRows = [
      { userId: 1, userPresenceType: 2 },
      { user_id: 2, user_presence_type: 1 },
      { userId: 3 },
    ];
    const { result } = await renderStore();

    await waitFor(() => expect(result.current.presenceByUserId.size).toBe(3));
    expect(result.current.presenceByUserId.get(1)).toBe(2);
    expect(result.current.presenceByUserId.get(2)).toBe(1);
    expect(result.current.presenceByUserId.get(3)).toBe(0);
  });

  it("tracks program-launched clients from both key spellings", async () => {
    accountsData = [account({ UserID: 1 })];
    runningInstances = [{ userId: 1 }, { user_id: 2 }, { pid: 3 }];
    const { result } = await renderStore();

    await waitFor(() => expect(result.current.launchedByProgram.size).toBe(2));
    expect([...result.current.launchedByProgram].sort()).toEqual([1, 2]);
  });
});

describe("AFK mode", () => {
  const RUNNING = { active: true, startedAtMs: 1, intervalSeconds: 600, key: "Space", accounts: [] };

  /** "Modo AFK iniciado em 1 contas" era o toast de quem liga o modo numa conta só. */
  it("o toast de início fala de uma conta no singular", async () => {
    const { result } = await renderStore();
    results.set("start_afk_mode", RUNNING);

    await act(async () => {
      await result.current.startAfkMode({
        userIds: [11],
        intervalSeconds: 600,
        key: "Space",
        mode: "key",
        clickX: 50,
        clickY: 50,
      });
    });

    expect(result.current.toasts.map((toast) => toast.message)).toContain(
      "AFK mode started for 1 account"
    );
  });

  it("e no plural com mais de uma", async () => {
    const { result } = await renderStore();
    results.set("start_afk_mode", RUNNING);

    await act(async () => {
      await result.current.startAfkMode({
        userIds: [11, 22],
        intervalSeconds: 600,
        key: "Space",
        mode: "key",
        clickX: 50,
        clickY: 50,
      });
    });

    expect(result.current.toasts.map((toast) => toast.message)).toContain(
      "AFK mode started for 2 accounts"
    );
  });

  it("o start leva o modo e o ponto padrão do clique ao backend", async () => {
    const { result } = await renderStore();
    results.set("start_afk_mode", RUNNING);

    await act(async () => {
      await result.current.startAfkMode({
        userIds: [11],
        intervalSeconds: 10,
        key: "",
        mode: "click",
        clickX: 37.5,
        clickY: 62.5,
      });
    });

    expect(lastArgs("start_afk_mode")).toEqual({
      userIds: [11],
      intervalSeconds: 10,
      key: "",
      mode: "click",
      clickX: 37.5,
      clickY: 62.5,
    });
  });

  /** Tecla ou clique é o da sessão ligada: a tela não manda outro no envio manual. */
  it("o envio manual só leva as contas", async () => {
    const { result } = await renderStore();
    results.set("afk_trigger_now", 1);

    await act(async () => {
      await result.current.afkTriggerNow([11, 22]);
    });

    expect(lastArgs("afk_trigger_now")).toEqual({ userIds: [11, 22] });
  });

  it("o Marcar devolve o ponto, e o erro chega à tela como código", async () => {
    const { result } = await renderStore();
    results.set("afk_capture_point", { userId: 11, xPct: 40, yPct: 60 });

    let captured: unknown;
    await act(async () => {
      captured = await result.current.captureAfkPoint();
    });
    expect(captured).toEqual({ userId: 11, xPct: 40, yPct: 60 });

    failures.set("afk_capture_point", "notAnAccountWindow");
    await act(async () => {
      await expect(result.current.captureAfkPoint()).rejects.toBe("notAnAccountWindow");
    });
    // O código cru não vira faixa de erro: a tela escreve a frase.
    expect(result.current.error).not.toBe("notAnAccountWindow");
  });
});

describe("botting and generator commands", () => {
  it("maps the botting start config onto the backend arguments", async () => {
    const { result } = await renderStore();
    results.set("start_botting_mode", { active: true });

    await act(async () => {
      await result.current.startBottingMode({
        userIds: [1, 2],
        placeId: 5,
        jobId: "job",
        launchData: "ld",
        playerUserIds: [1],
        intervalMinutes: 10,
        launchDelaySeconds: 20,
        playerGraceMinutes: 30,
      });
    });

    expect(lastArgs("start_botting_mode")).toEqual({
      userIds: [1, 2],
      placeId: 5,
      jobId: "job",
      launchData: "ld",
      playerUserIds: [1],
      intervalMinutes: 10,
      launchDelaySeconds: 20,
      playerGraceMinutes: 30,
      // O Start normal fecha e relança tudo; só a adoção de contas que já
      // estão em jogo liga esta bandeira.
      adoptRunning: false,
    });
    expect(result.current.bottingStatus).toMatchObject({ active: true });
  });

  it("refuses to start botting on an unsupported Linux runner", async () => {
    capabilitiesData = caps({ os: "linux", supportsBotting: false, warnings: ["no botting"] });
    const { result } = await renderStore();

    await act(async () => {
      await expect(
        result.current.startBottingMode({
          userIds: [1],
          placeId: 1,
          jobId: "",
          launchData: "",
          playerUserIds: [],
          intervalMinutes: 1,
          launchDelaySeconds: 1,
          playerGraceMinutes: 1,
        })
      ).rejects.toThrow("no botting");
    });
    expect(invokeCalls("start_botting_mode")).toHaveLength(0);
  });

  it("stops botting and refreshes the status", async () => {
    const { result } = await renderStore();
    await act(async () => {
      await result.current.stopBottingMode(true);
    });
    expect(lastArgs("stop_botting_mode")).toEqual({ closeBotAccounts: true });
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/alt accounts closed/i);
  });

  it("adds botting accounts and ignores an empty list", async () => {
    const { result } = await renderStore();
    await act(async () => {
      await result.current.addBottingAccounts([]);
    });
    expect(invokeCalls("add_botting_accounts")).toHaveLength(0);

    await act(async () => {
      await result.current.addBottingAccounts([1, 2]);
    });
    expect(lastArgs("add_botting_accounts")).toEqual({ userIds: [1, 2] });
  });

  it("sets the player accounts and reports cleared vs updated", async () => {
    const { result } = await renderStore();

    await act(async () => {
      await result.current.setBottingPlayerAccounts([]);
    });
    expect(lastArgs("set_botting_player_accounts")).toEqual({ playerUserIds: [] });
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/cleared/i);

    await act(async () => {
      await result.current.setBottingPlayerAccounts([1]);
    });
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/updated/i);
  });

  it("forwards per-account botting actions", async () => {
    const { result } = await renderStore();
    await act(async () => {
      await result.current.bottingAccountAction(3, "restartLoop");
    });
    expect(lastArgs("botting_account_action")).toEqual({ userId: 3, action: "restartLoop" });
  });

  it("starts and stops the account generator", async () => {
    results.set("start_generator", { active: true, totalGenerated: 0 });
    const { result } = await renderStore();

    await act(async () => {
      await result.current.startGenerator({
        provider: "p",
        endpoint: "e",
        apiKey: "secret",
        accountType: "t",
        extraDelaySeconds: 2,
        targetGroup: "g",
        maxAccounts: 4,
      });
    });

    expect(lastArgs("start_generator")).toEqual({
      provider: "p",
      endpoint: "e",
      apiKey: "secret",
      accountType: "t",
      extraDelaySeconds: 2,
      targetGroup: "g",
      maxAccounts: 4,
    });

    await act(async () => {
      await result.current.stopGenerator();
    });
    expect(invokeCalls("stop_generator")).toHaveLength(1);
  });

  it("propagates generator failures", async () => {
    failures.set("start_generator", "gen boom");
    const { result } = await renderStore();

    await act(async () => {
      await expect(
        result.current.startGenerator({
          provider: "p",
          endpoint: "",
          apiKey: "",
          accountType: "",
          extraDelaySeconds: 0,
          targetGroup: "",
          maxAccounts: 1,
        })
      ).rejects.toBeTruthy();
    });
    expect(result.current.error).toBe("gen boom");
  });
});

describe("process control", () => {
  it("reports how many Roblox processes were closed", async () => {
    results.set("cmd_kill_all_roblox", 2);
    const { result } = await renderStore();

    await act(async () => {
      await result.current.killAllRobloxProcesses();
    });

    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toContain("2");
    expect(result.current.error).toBeNull();
  });

  it("reports when nothing was open", async () => {
    results.set("cmd_kill_all_roblox", 0);
    const { result } = await renderStore();

    await act(async () => {
      await result.current.killAllRobloxProcesses();
    });

    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/No open Roblox processes/i);
  });

  it("surfaces focus failures", async () => {
    failures.set("focus_roblox_window", "no window");
    const { result } = await renderStore();

    await act(async () => {
      await expect(result.current.focusRobloxClient(1)).rejects.toBeTruthy();
    });
    expect(result.current.error).toBe("no window");
  });
});

describe("updates", () => {
  it("skips the automatic check when updates are disabled", async () => {
    settingsData = { General: { CheckForUpdates: "false" } };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.checkForUpdates();
    });

    expect(invokeCalls("check_for_updates_with_channels")).toHaveLength(0);
  });

  it("still runs a manual check with normalized channels", async () => {
    settingsData = {
      General: {
        CheckForUpdates: "false",
        UpdaterReleaseChannel: "STABLE",
        UpdaterFeatureChannel: "nexus",
      },
    };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.checkForUpdates(true);
    });

    expect(lastArgs("check_for_updates_with_channels")).toEqual({
      releaseChannel: "stable",
      featureChannel: "nexus-ws",
      allowEditionSwitch: true,
    });
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/No updates available/i);
  });

  it("opens the update dialog when an update is returned", async () => {
    updateResult = {
      version: "4.3.0",
      currentVersion: "4.2.0",
      date: "",
      body: "",
      releaseChannel: "garbage",
      featureChannel: "full",
    };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.checkForUpdates(true);
    });

    expect(result.current.updateDialogOpen).toBe(true);
    expect(result.current.updateInfo).toMatchObject({
      version: "4.3.0",
      releaseChannel: "beta",
      featureChannel: "nexus-ws",
    });
  });

  it("honours a skipped version for automatic checks only", async () => {
    updateResult = {
      version: "4.3.0",
      currentVersion: "4.2.0",
      date: "",
      body: "",
      releaseChannel: "beta",
      featureChannel: "standard",
    };
    localStorage.setItem("skipped-update-version:beta:standard", "4.3.0");
    const { result } = await renderStore();

    await act(async () => {
      await result.current.checkForUpdates();
    });
    expect(result.current.updateDialogOpen).toBe(false);

    await act(async () => {
      await result.current.checkForUpdates(true);
    });
    expect(result.current.updateDialogOpen).toBe(true);
  });

  it("reports a failed manual check", async () => {
    failures.set("check_for_updates_with_channels", "network down");
    const { result } = await renderStore();

    await act(async () => {
      await result.current.checkForUpdates(true);
    });

    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/Update check failed/i);
  });

  it("the automatic check never asks for an edition switch", async () => {
    settingsData = { General: { CheckForUpdates: "true", UpdaterFeatureChannel: "nexus-ws" } };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.checkForUpdates();
    });

    expect(lastArgs("check_for_updates_with_channels")).toMatchObject({
      featureChannel: "nexus-ws",
      allowEditionSwitch: false,
    });
  });

  it("switching to the complete edition saves the channel, then downloads it in the app", async () => {
    settingsData = { General: { UpdaterReleaseChannel: "stable", UpdaterFeatureChannel: "standard" } };
    // Mesma versão nas duas edições: o backend devolve o instalador completo.
    updateResult = {
      version: "1.4.0",
      currentVersion: "1.4.0",
      date: "",
      body: "",
      releaseChannel: "stable",
      featureChannel: "nexus-ws",
    };
    const { result } = await renderStore();

    let opened = false;
    await act(async () => {
      opened = await result.current.switchToCompleteEdition();
    });

    expect(opened).toBe(true);
    expect(lastArgs("update_setting")).toEqual({
      section: "General",
      key: "UpdaterFeatureChannel",
      value: "nexus-ws",
    });
    expect(lastArgs("check_for_updates_with_channels")).toEqual({
      releaseChannel: "stable",
      featureChannel: "nexus-ws",
      allowEditionSwitch: true,
    });
    // A setting é gravada antes da checagem.
    const order = invokeMock.mock.calls.map((c) => c[0]);
    expect(order.lastIndexOf("update_setting")).toBeLessThan(order.lastIndexOf("check_for_updates_with_channels"));
    expect(result.current.updateDialogOpen).toBe(true);
    expect(result.current.updateInfo).toMatchObject({ version: "1.4.0", featureChannel: "nexus-ws", autoInstall: true });
  });

  it("after the switch, the next automatic check stays on the complete edition", async () => {
    settingsData = { General: { CheckForUpdates: "true", UpdaterFeatureChannel: "standard" } };
    const { result } = await renderStore();
    await act(async () => {
      await result.current.switchToCompleteEdition();
    });
    expect(result.current.settings?.General?.UpdaterFeatureChannel).toBe("nexus-ws");

    // Versão nova saindo: a checagem do boot olha o manifesto completo.
    updateResult = {
      version: "1.5.0",
      currentVersion: "1.4.0",
      date: "",
      body: "",
      releaseChannel: "beta",
      featureChannel: "nexus-ws",
    };
    await act(async () => {
      await result.current.checkForUpdates();
    });
    expect(lastArgs("check_for_updates_with_channels")).toMatchObject({
      featureChannel: "nexus-ws",
      allowEditionSwitch: false,
    });
    expect(result.current.updateInfo).toMatchObject({ version: "1.5.0", featureChannel: "nexus-ws", autoInstall: false });
  });

  it("says so in the app when the complete edition cannot be found", async () => {
    updateResult = null;
    const { result } = await renderStore();
    let opened = true;
    await act(async () => {
      opened = await result.current.switchToCompleteEdition();
    });
    expect(opened).toBe(false);
    const toasts = result.current.toasts.map((toast) => toast.message).join(" ");
    expect(toasts).toMatch(/complete edition is not available right now/i);
    expect(toasts).not.toMatch(/github|browser/i);
  });

  it("fills a preview release for the update dialog", async () => {
    const { result } = await renderStore();
    act(() => result.current.openUpdatePreviewDialog());
    expect(result.current.updateDialogOpen).toBe(true);
    expect(result.current.updateInfo?.version).toBe("4.2.6-beta");
  });
});

describe("versions, encryption and walkthrough", () => {
  it("applies the default version optimistically", async () => {
    settingsData = { Versions: { DefaultVersion: "old" } };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.setDefaultVersion("new");
    });

    expect(lastArgs("versions_set_default")).toEqual({ versionId: "new" });
    expect(result.current.settings?.Versions?.DefaultVersion).toBe("new");
  });

  it("rolls the default version back when the backend refuses", async () => {
    settingsData = { Versions: { DefaultVersion: "old" } };
    failures.set("versions_set_default", "nope");
    const { result } = await renderStore();

    await act(async () => {
      await result.current.setDefaultVersion("new");
    });

    expect(result.current.settings?.Versions?.DefaultVersion).toBe("old");
    expect(result.current.toasts.map((toast) => toast.message).join(" ")).toMatch(/Failed to set version/i);
  });

  it("stores an empty string when clearing the default version", async () => {
    settingsData = { Versions: { DefaultVersion: "old" } };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.setDefaultVersion(null);
    });

    expect(result.current.settings?.Versions?.DefaultVersion).toBe("");
    expect(lastArgs("versions_set_default")).toEqual({ versionId: null });
  });

  it("applies password encryption and persists the onboarding state", async () => {
    const { result } = await renderStore();

    await act(async () => {
      await result.current.applyEncryptionMethod("password", "hunter2");
    });

    expect(lastArgs("set_encryption_password")).toEqual({ password: "hunter2" });
    const settingWrites = invokeCalls("update_setting").map((c) => c[1] as Record<string, string>);
    expect(settingWrites).toEqual(
      expect.arrayContaining([
        { section: "General", key: "EncryptionOnboardingState", value: "completed" },
        { section: "General", key: "EncryptionMethod", value: "password" },
      ])
    );
    expect(result.current.encryptionSetupOpen).toBe(false);
    expect(result.current.applyingEncryption).toBe(false);
  });

  it("sends a null password for default encryption", async () => {
    const { result } = await renderStore();
    await act(async () => {
      await result.current.applyEncryptionMethod("default");
    });
    expect(lastArgs("set_encryption_password")).toEqual({ password: null });
  });

  it("keeps the dialog open and records the error when encryption fails", async () => {
    failures.set("set_encryption_password", "bad password");
    const { result } = await renderStore();

    act(() => result.current.openEncryptionSetupFromSettings());
    await act(async () => {
      await expect(result.current.applyEncryptionMethod("password", "x")).rejects.toBeTruthy();
    });

    expect(result.current.encryptionSetupError).toBe("bad password");
    expect(result.current.encryptionSetupOpen).toBe(true);
  });

  it("unlocks with a password and reloads the accounts", async () => {
    needsPasswordValue = true;
    const { result } = await renderStore();
    expect(result.current.needsPassword).toBe(true);

    accountsData = [account({ UserID: 1 })];
    await act(async () => {
      await result.current.unlock("secret");
    });

    expect(lastArgs("unlock_accounts")).toEqual({ password: "secret", rememberHours: null });
    expect(result.current.needsPassword).toBe(false);
    expect(result.current.accounts).toHaveLength(1);
    expect(result.current.unlocking).toBe(false);
  });

  it("keeps the lock screen up on a wrong password", async () => {
    needsPasswordValue = true;
    failures.set("unlock_accounts", "wrong password");
    const { result } = await renderStore();

    await act(async () => {
      await result.current.unlock("nope");
    });

    expect(result.current.needsPassword).toBe(true);
    expect(result.current.error).toBe("wrong password");
  });

  it("persists the walkthrough state only once it was pending", async () => {
    settingsData = { General: { FirstRunWalkthroughState: "completed" } };
    const { result } = await renderStore();

    act(() => result.current.openFirstRunWalkthroughFromSettings());
    await waitFor(() => expect(result.current.firstRunWalkthroughOpen).toBe(true));

    await act(async () => {
      await result.current.completeFirstRunWalkthrough();
    });

    expect(result.current.firstRunWalkthroughOpen).toBe(false);
    const writes = invokeCalls("update_setting").map((c) => c[1] as Record<string, string>);
    expect(writes.some((w) => w.key === "FirstRunWalkthroughState")).toBe(false);
  });

  it("persists completion for a pending first run", async () => {
    settingsData = {
      General: { FirstRunWalkthroughState: "pending", EncryptionOnboardingState: "completed" },
    };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.completeFirstRunWalkthrough();
    });

    const writes = invokeCalls("update_setting").map((c) => c[1] as Record<string, string>);
    expect(writes).toEqual(
      expect.arrayContaining([
        { section: "General", key: "FirstRunWalkthroughState", value: "completed" },
      ])
    );
    expect(result.current.settings?.General?.FirstRunWalkthroughState).toBe("completed");
  });

  it("persists a skip for a pending first run", async () => {
    settingsData = {
      General: { FirstRunWalkthroughState: "pending", EncryptionOnboardingState: "completed" },
    };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.skipFirstRunWalkthrough();
    });

    const writes = invokeCalls("update_setting").map((c) => c[1] as Record<string, string>);
    expect(writes).toEqual(
      expect.arrayContaining([
        { section: "General", key: "FirstRunWalkthroughState", value: "skipped" },
      ])
    );
  });

  it("closes the walkthrough without persisting anything", async () => {
    settingsData = { General: { FirstRunWalkthroughState: "skipped" } };
    const { result } = await renderStore();

    act(() => result.current.openFirstRunWalkthroughFromSettings());
    await waitFor(() => expect(result.current.firstRunWalkthroughOpen).toBe(true));
    act(() => result.current.closeFirstRunWalkthrough());

    expect(result.current.firstRunWalkthroughOpen).toBe(false);
    expect(result.current.firstRunWalkthroughMode).toBe("manual");
  });
});

describe("theme and settings reload", () => {
  it("normalizes a theme preview and writes CSS variables", async () => {
    const { result } = await renderStore();

    act(() =>
      result.current.applyThemePreview({
        ...(result.current.theme as StoreValue["theme"])!,
        accounts_background: "not-a-color",
        buttons_background: "#123456",
      })
    );

    expect(result.current.theme?.buttons_background).toBe("#123456");
    // invalid colors fall back to the default theme value
    expect(result.current.theme?.accounts_background).toBe("#09090B");
    expect(document.documentElement.style.getPropertyValue("--buttons-bg")).toBe("#123456");
  });

  it("saves a normalized theme to the backend", async () => {
    const { result } = await renderStore();

    await act(async () => {
      await result.current.saveTheme({
        ...(result.current.theme as StoreValue["theme"])!,
        button_style: "Popup",
      });
    });

    const args = lastArgs("update_theme") as { theme: Record<string, unknown> };
    expect(args.theme.button_style).toBe("Popup");
    expect(result.current.theme?.button_style).toBe("Popup");
  });

  it("reloads settings and platform capabilities on demand", async () => {
    const { result } = await renderStore();
    settingsData = { General: { Language: "de-DE" } };

    await act(async () => {
      await result.current.reloadSettings();
    });

    expect(result.current.settings?.General?.Language).toBe("de-DE");
  });

  /**
   * Uma setting mudada fora da página Settings (o padrão de reconexão na página
   * Session): a tela vê o valor novo na hora e o INI recebe a gravação.
   */
  it("updates one setting right away and saves it", async () => {
    settingsData = { General: { AutoReconnect: "false", Language: "en" } };
    const { result } = await renderStore();

    await act(async () => {
      await result.current.updateSetting("General", "AutoReconnect", "true");
    });

    expect(result.current.settings?.General?.AutoReconnect).toBe("true");
    expect(result.current.settings?.General?.Language).toBe("en");
    expect(lastArgs("update_setting")).toEqual({ section: "General", key: "AutoReconnect", value: "true" });
  });

  it("puts the old value back and says so when saving a setting fails", async () => {
    settingsData = { General: { AutoReconnect: "false" } };
    const { result } = await renderStore();
    failures.set("update_setting", new Error("disk full"));

    await act(async () => {
      await result.current.updateSetting("General", "AutoReconnect", "true");
    });

    expect(result.current.settings?.General?.AutoReconnect).toBe("false");
    expect(result.current.toasts.some((toast) => toast.message.includes("disk full"))).toBe(true);
  });

  it("themes the titlebar from the forms colors for a light top bar", async () => {
    settingsData = { General: { ThemeWindowsNavbar: "false" } };
    results.set("get_theme", { forms_background: "#123456", dark_top_bar: false });
    await renderStore();
    expect(document.documentElement.style.getPropertyValue("--titlebar-bg")).toBe("#123456");
  });

  it("uses the dark titlebar colors when dark_top_bar is set", async () => {
    settingsData = { General: { ThemeWindowsNavbar: "false" } };
    results.set("get_theme", { forms_background: "#123456", dark_top_bar: true });
    await renderStore();
    expect(document.documentElement.style.getPropertyValue("--titlebar-bg")).toBe("#09090b");
    expect(document.documentElement.style.getPropertyValue("--titlebar-fg")).toBe("#a1a1aa");
  });

  it("uses the theme colors for the titlebar when the navbar option is on", async () => {
    settingsData = { General: { ThemeWindowsNavbar: "true" } };
    results.set("get_theme", { forms_background: "#123456", forms_foreground: "#abcdef", dark_top_bar: true });
    await renderStore();
    expect(document.documentElement.style.getPropertyValue("--titlebar-bg")).toBe("#123456");
    expect(document.documentElement.style.getPropertyValue("--titlebar-fg")).toBe("#abcdef");
  });
});

describe("browser helpers", () => {
  it("opens the login and per-account browsers", async () => {
    const { result } = await renderStore();

    await act(async () => {
      await result.current.openLoginBrowser();
      await result.current.openAccountBrowser(9);
    });

    expect(invokeCalls("open_login_browser")).toHaveLength(1);
    expect(lastArgs("open_account_browser")).toEqual({ userId: 9 });
  });

  it("records an error when the login browser cannot open", async () => {
    failures.set("open_login_browser", "no chromium");
    const { result } = await renderStore();

    await act(async () => {
      await result.current.openLoginBrowser();
    });

    expect(result.current.error).toBe("no chromium");
  });

  it("refreshes a cookie and reloads the accounts", async () => {
    results.set("refresh_cookie", true);
    const { result } = await renderStore();

    let ok = false;
    await act(async () => {
      ok = await result.current.refreshCookie(3);
    });

    expect(ok).toBe(true);
    expect(lastArgs("refresh_cookie")).toEqual({ userId: 3 });
  });
  // Quebra 1 da re-revisao: o aviso do AccountData.key e a UNICA rede contra o
  // lockout de quem usa a chave do aparelho, e o boot e o unico momento em que o
  // backend o descobre. O efeito de inicializacao NAO passa por `loadAccounts`
  // (chama `get_accounts` direto), entao sem uma chamada explicita o aviso nunca
  // aparecia: quem esta no modo device key pode passar a sessao inteira sem fazer
  // nenhuma mutacao e sem nunca destrancar por senha.
  describe("aviso do arquivo de chave", () => {
    it("consulta o backend no boot, sem depender de nenhuma mutacao", async () => {
      results.set("vault_key_warning", {
        code: "writeFailed",
        path: "C:\dados\AccountData.key",
        detail: "acesso negado",
      });

      const { result } = await renderStore();

      expect(invokeCalls("vault_key_warning").length).toBeGreaterThan(0);
      expect(result.current.vaultKeyWarning).toEqual({
        code: "writeFailed",
        path: "C:\dados\AccountData.key",
        detail: "acesso negado",
      });
    });

    it("fica limpo quando o backend nao tem nada a dizer", async () => {
      const { result } = await renderStore();
      expect(invokeCalls("vault_key_warning").length).toBeGreaterThan(0);
      expect(result.current.vaultKeyWarning).toBeNull();
    });

    // A primeira versao nunca limpava: aviso resolvido ficava na tela para sempre.
    it("limpa o aviso quando o problema e resolvido", async () => {
      results.set("vault_key_warning", {
        code: "weakWrapper",
        path: "C:\dados\AccountData.key",
      });
      const { result } = await renderStore();
      expect(result.current.vaultKeyWarning?.code).toBe("weakWrapper");

      results.set("vault_key_warning", null);
      await act(async () => {
        await result.current.loadAccounts();
      });

      expect(result.current.vaultKeyWarning).toBeNull();
    });

    it("nao derruba o boot quando o comando falha", async () => {
      failures.set("vault_key_warning", "comando ausente");
      const { result } = await renderStore();
      expect(result.current.initialized).toBe(true);
      expect(result.current.vaultKeyWarning).toBeNull();
    });

    // O caminho que falha e justamente o que pode ter deixado um aviso novo (a
    // chave que nao pode ser criada). Lendo so no sucesso, o aviso aparecia
    // somente no proximo boot — depois de o dono ja ter fechado o app achando que
    // era "deu erro, tento outra vez".
    it("le o aviso mesmo quando trocar o metodo de criptografia falha", async () => {
      const { result } = await renderStore();
      const before = invokeCalls("vault_key_warning").length;

      failures.set("set_encryption_password", "nao deu");
      results.set("vault_key_warning", {
        code: "writeFailed",
        path: "C:\dados\AccountData.key",
      });

      await act(async () => {
        await expect(result.current.applyEncryptionMethod("default")).rejects.toBeTruthy();
      });

      expect(invokeCalls("vault_key_warning").length).toBeGreaterThan(before);
      expect(result.current.vaultKeyWarning?.code).toBe("writeFailed");
    });

    // A2 do checkup: o aviso tambem nasce em gravacao de fundo (Auto Rejoin a
    // cada ciclo, Watcher, servidor HTTP), que nao passa por nenhuma das leituras
    // acima. Com o Auto Rejoin a noite inteira a faixa nunca aparecia, e o boot
    // seguinte caia em lockout. O backend publica cada mudanca num evento.
    it("mostra na hora o aviso que uma gravacao de fundo levantou, e o tira quando some", async () => {
      const { result } = await renderStore();
      expect(result.current.vaultKeyWarning).toBeNull();

      act(() => {
        emit(VAULT_KEY_WARNING_EVENT, { code: "writeFailed", path: "C:\dados\AccountData.key" });
      });
      expect(result.current.vaultKeyWarning).toEqual({
        code: "writeFailed",
        path: "C:\dados\AccountData.key",
      });

      act(() => {
        emit(VAULT_KEY_WARNING_EVENT, null);
      });
      expect(result.current.vaultKeyWarning).toBeNull();
    });

    // A leitura que ja estava a caminho quando o evento chegou e mais velha que
    // ele: aplica-la por cima apagaria a faixa que acabou de aparecer — ate a
    // proxima mudanca, que pode nunca vir.
    it("uma leitura que ja estava a caminho nao apaga o aviso que chegou por evento", async () => {
      const { result } = await renderStore();

      let answer: (warning: unknown) => void = () => {};
      const slowRead = new Promise((done) => {
        answer = done;
      });
      const fallback = invokeMock.getMockImplementation();
      invokeMock.mockImplementation(async (cmd: string, args?: unknown) =>
        cmd === "vault_key_warning" ? slowRead : fallback?.(cmd, args)
      );

      // Uma recarga de contas pede o aviso antes de a gravacao de fundo mudar.
      await act(async () => {
        await result.current.loadAccounts();
      });
      // A gravacao de fundo levanta o aviso enquanto a resposta ainda vinha.
      act(() => {
        emit(VAULT_KEY_WARNING_EVENT, { code: "writeFailed", path: "C:\dados\AccountData.key" });
      });
      // A resposta velha chega depois.
      await act(async () => {
        answer(null);
        await slowRead;
        await new Promise((done) => setTimeout(done, 0));
      });

      expect(result.current.vaultKeyWarning?.code).toBe("writeFailed");
    });

    // O evento e contrato entre dois arquivos em linguagens diferentes: um typo
    // de um lado nao quebra compilacao nenhuma — a faixa so para de aparecer.
    it("ouve o evento pelo mesmo nome que o backend publica", () => {
      const rust = readFileSync(
        resolve(process.cwd(), "src-tauri/src/data/accounts/commands.rs"),
        "utf8"
      );
      const published = rust.match(/VAULT_KEY_WARNING_EVENT: &str = "([^"]+)"/)?.[1];
      expect(published).toBe(VAULT_KEY_WARNING_EVENT);
    });
  });
});

/**
 * Depois do lote de avatares o Roblox costuma responder o headshot novo como
 * "Pending" (`imageUrl: null`) por alguns segundos. A foto velha fica na tela
 * até a nova chegar — apagar antes deixava a conta sem foto até recarregar.
 */
describe("refreshAvatarHeadshots", () => {
  it("keeps the old picture while Roblox answers Pending and swaps it when the retry returns the new one", async () => {
    accountsData = [account({ UserID: 1 })];
    let headshotCalls = 0;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "batched_get_avatar_headshots") {
        headshotCalls += 1;
        if (headshotCalls === 1) return [{ targetId: 1, imageUrl: "old.png" }];
        if (headshotCalls === 2) return [{ targetId: 1, imageUrl: null }];
        return [{ targetId: 1, imageUrl: "new.png" }];
      }
      return defaultInvoke(cmd);
    });

    const { result } = await renderStore();
    await waitFor(() => expect(result.current.avatarUrls.get(1)).toBe("old.png"));

    vi.useFakeTimers();
    let done: Promise<void> = Promise.resolve();
    await act(async () => {
      done = result.current.refreshAvatarHeadshots([1]);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(invokeCalls("invalidate_avatar_headshots")[0][1]).toEqual({ userIds: [1] });
    expect(headshotCalls).toBe(2);
    // Pending: a foto antiga continua.
    expect(result.current.avatarUrls.get(1)).toBe("old.png");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
      await done;
    });
    expect(headshotCalls).toBe(3);
    expect(result.current.avatarUrls.get(1)).toBe("new.png");
  });

  it("gives up after a few attempts and keeps the old picture", async () => {
    accountsData = [account({ UserID: 1 })];
    let headshotCalls = 0;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "batched_get_avatar_headshots") {
        headshotCalls += 1;
        return [{ targetId: 1, imageUrl: headshotCalls === 1 ? "old.png" : null }];
      }
      return defaultInvoke(cmd);
    });

    const { result } = await renderStore();
    await waitFor(() => expect(result.current.avatarUrls.get(1)).toBe("old.png"));

    vi.useFakeTimers();
    await act(async () => {
      const done = result.current.refreshAvatarHeadshots([1]);
      await vi.advanceTimersByTimeAsync(30_000);
      await done;
    });
    // 1 carga inicial + 3 tentativas.
    expect(headshotCalls).toBe(4);
    expect(result.current.avatarUrls.get(1)).toBe("old.png");
  });
});

/**
 * Navegação por páginas: a barra lateral troca a área principal em vez de abrir
 * modal. Os setters antigos (`setSettingsOpen` e cia.) continuam valendo — há
 * chamadas espalhadas (barra de ações, gerador, walkthrough, Choose Game) — e
 * viram navegação.
 */
describe("activePage", () => {
  it("starts on the account list", async () => {
    const { result } = await renderStore();
    expect(result.current.activePage).toBe("accounts");
  });

  it("navigates with setActivePage", async () => {
    const { result } = await renderStore();
    act(() => result.current.setActivePage("avatars"));
    expect(result.current.activePage).toBe("avatars");
    act(() => result.current.setActivePage("accounts"));
    expect(result.current.activePage).toBe("accounts");
  });

  it.each([
    ["setSettingsOpen", "settings"],
    ["setThemeEditorOpen", "theme"],
    ["setAvatarsDialogOpen", "avatars"],
    ["setSessionDialogOpen", "session"],
    ["setNexusOpen", "nexus"],
    ["setScriptsOpen", "scripts"],
  ] as const)("maps %s(true) to the %s page", async (setter, page) => {
    const { result } = await renderStore();
    act(() => result.current[setter](true));
    expect(result.current.activePage).toBe(page);
  });

  it("closing the open page goes back to the account list", async () => {
    const { result } = await renderStore();
    act(() => result.current.setScriptsOpen(true));
    act(() => result.current.setScriptsOpen(false));
    expect(result.current.activePage).toBe("accounts");
  });

  /**
   * O walkthrough "fecha tudo" no último passo chamando cada setter com
   * `false`: fechar uma página que não é a aberta não pode tirar o usuário de
   * onde ele está.
   */
  it("closing a page that is not open leaves the current page alone", async () => {
    const { result } = await renderStore();
    act(() => result.current.setActivePage("avatars"));
    act(() => result.current.setSettingsOpen(false));
    expect(result.current.activePage).toBe("avatars");
  });

  it("reopening the walkthrough from Settings leaves the Settings page", async () => {
    const { result } = await renderStore();
    act(() => result.current.setSettingsOpen(true));
    act(() => result.current.openFirstRunWalkthroughFromSettings());
    expect(result.current.activePage).toBe("accounts");
  });
});

/**
 * Reconexão automática (commands/reconnect.rs): a lista vem inteira do
 * backend, e o aviso sai na mudança (desistiu / parou / voltou ao jogo).
 */
describe("auto-reconnect", () => {
  function entry(userId: number, phase: string, attempt = 1) {
    return {
      userId,
      phase,
      attempt,
      maxAttempts: 5,
      nextAttemptAtMs: null,
      reason: null,
      error: null,
      drop: { kind: "disconnected", reason: "connectionLost", code: 277, message: null, sinceMs: 0 },
    };
  }

  /**
   * Achado no harness: um `[]` no lugar do objeto tem `.entries` (o método do
   * Array), e a função no setState virava updater do React — a tela inteira
   * quebrava.
   */
  it("an array in place of the payload leaves the list empty instead of breaking", async () => {
    results.set("get_auto_reconnect_status", []);
    const { result } = await renderStore();
    await waitFor(() => expect(invokeCalls("get_auto_reconnect_status")).toHaveLength(1));
    expect(result.current.autoReconnect).toEqual([]);
    act(() => emit("auto-reconnect", []));
    expect(result.current.autoReconnect).toEqual([]);
  });

  it("follows the event and warns once when an account gives up", async () => {
    accountsData = [account({ UserID: 1 })];
    const { result } = await renderStore();
    act(() => emit("auto-reconnect", { entries: [entry(1, "waiting")] }));
    expect(result.current.autoReconnect.map((e) => e.phase)).toEqual(["waiting"]);
    expect(result.current.toasts.some((t) => t.message.includes("Auto-reconnect"))).toBe(false);

    act(() => emit("auto-reconnect", { entries: [entry(1, "gaveUp", 5)] }));
    act(() => emit("auto-reconnect", { entries: [entry(1, "gaveUp", 5)] }));
    const warnings = result.current.toasts.filter((t) => t.message.includes("Gave up after 5 tries"));
    expect(warnings.map((t) => t.message)).toEqual(["Auto-reconnect (user1) — Gave up after 5 tries"]);
  });

  /** "… — user1: Not reconnecting: …" tinha dois-pontos duas vezes. */
  it("warns about a stop without a double colon", async () => {
    accountsData = [account({ UserID: 1 })];
    const { result } = await renderStore();
    act(() =>
      emit("auto-reconnect", { entries: [{ ...entry(1, "stopped"), reason: "closedByUser" }] })
    );
    expect(result.current.toasts.map((t) => t.message)).toContain(
      "Auto-reconnect (user1) — Not reconnecting: the client was closed"
    );
  });
});

/** Queda lida do log do Roblox: o aviso dizia "user1 in Roblox: Disconnected: lost connection". */
describe("client drop toast", () => {
  it("names the account and the drop without a double colon", async () => {
    accountsData = [account({ UserID: 1 })];
    const { result } = await renderStore();
    act(() =>
      emit("roblox-client-health", {
        userId: 1,
        drop: { kind: "disconnected", reason: "connectionLost", code: 277, message: null, sinceMs: 0 },
      })
    );
    const messages = result.current.toasts.map((t) => t.message);
    expect(messages).toContain("user1 in Roblox — Disconnected: lost connection");
    for (const message of messages) expect(message.split(":").length, message).toBeLessThanOrEqual(2);
  });

  it("says when a reopened account stayed in the game", async () => {
    accountsData = [account({ UserID: 1 })];
    const { result } = await renderStore();
    act(() => emit("auto-reconnect", { entries: [], reconnected: [1] }));
    expect(result.current.toasts.map((t) => t.message)).toContain("user1 is back in the game");
  });
});
