import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/event", async () => (await import("../../test-utils/tauriMocks")).tauriEventMock());
vi.mock("../../hooks/usePrompt", async () => (await import("../../test-utils/promptMocks")).promptModuleMock());

import { SessionPanel } from "./SessionPanel";
import {
  makeAccount,
  makeBottingStatus,
  makePlatformCapabilities,
  renderWithStore,
  setStore,
} from "../../test-utils/renderWithStore";
import { confirmMock, promptAnswers, resetPromptMocks } from "../../test-utils/promptMocks";
import { emitTauriEvent, invokeMock, resetTauriMocks, setInvokeMap } from "../../test-utils/tauriMocks";
import { clearGameIdentityCache } from "../../hooks/useGameIdentity";
import type {
  Account,
  AutoReconnectEntry,
  ClientDrop,
  ClientHealth,
  CurrentSession,
  FriendLinkState,
  LaunchQueueEntry,
  LaunchQueuePayload,
  LaunchQueueState,
  UnidentifiedClient,
  UnidentifiedReason,
} from "../../types";
import type { StoreValue } from "../../store";

const ACCOUNTS = [
  makeAccount({ UserID: 1, Username: "alpha" }),
  makeAccount({ UserID: 2, Username: "bravo", Alias: "Bravo Alt" }),
  makeAccount({ UserID: 3, Username: "charlie" }),
];

function entry(userId: number, state: LaunchQueueState, error: string | null = null): LaunchQueueEntry {
  return { userId, state, error, updatedAtMs: 1_700_000_000_000 };
}

function queue(entries: LaunchQueueEntry[], active = true): LaunchQueuePayload {
  return { entries, active, placeId: 5315046213, jobId: "" };
}

/**
 * A store é substituída por um mock, então as ações são reimplementadas aqui
 * exatamente como em `store.tsx`. Assim os testes travam o **contrato com o
 * backend** (nome do comando e argumentos que chegam ao `invoke`) e não apenas
 * o fato de o componente ter chamado a store.
 */
function storeActions(): Partial<StoreValue> {
  return {
    cancelAccountLaunch: vi.fn(
      async (userId: number) => (await invokeMock("cancel_account_launch", { userId })) as boolean
    ),
    stopLaunchQueue: vi.fn(async () => (await invokeMock("stop_launch_queue")) as number),
    focusRobloxClient: vi.fn(
      async (userId: number) => (await invokeMock("focus_roblox_window", { userId })) as boolean
    ),
    identifyExternalClient: vi.fn(
      async (pid: number, userId: number) =>
        (await invokeMock("identify_external_client", { pid, userId })) as boolean
    ),
    focusClientWindow: vi.fn(
      async (pid: number) => (await invokeMock("focus_client_window", { pid })) as boolean
    ),
    closeRobloxClients: vi.fn(async (userIds: number[]) => {
      let closed = 0;
      for (const userId of userIds) {
        if (await invokeMock("cmd_kill_roblox", { userId })) closed += 1;
      }
      return closed;
    }),
  };
}

function renderPanel(overrides: Partial<StoreValue> = {}) {
  return renderWithStore(<SessionPanel />, {
    accounts: ACCOUNTS,
    ...storeActions(),
    ...overrides,
  });
}

/** Chamadas do `invoke` mockado para um comando. */
function callsFor(cmd: string) {
  return invokeMock.mock.calls.filter((c) => c[0] === cmd);
}

beforeEach(() => {
  resetTauriMocks();
  resetPromptMocks();
  setInvokeMap({
    cancel_account_launch: true,
    stop_launch_queue: 3,
    focus_roblox_window: true,
    cmd_kill_roblox: true,
    identify_external_client: true,
    focus_client_window: true,
  });
});

function unidentified(
  pid: number,
  reason: UnidentifiedReason,
  userId: number | null = null
): UnidentifiedClient {
  return { pid, reason, userId, placeId: null, jobId: null, startedAtMs: 1_700_000_000_000 };
}

afterEach(cleanup);

describe("SessionPanel — rendering", () => {
  it("lists the launch queue and the running clients", () => {
    renderPanel({
      launchQueue: queue([entry(1, "launching"), entry(2, "queued"), entry(3, "done")]),
      launchedByProgram: new Set([1, 3]),
    });

    // Fila: uma linha por conta, com o estado de cada uma.
    expect(within(screen.getByTestId("session-queue-1")).getByText("Joining")).toBeInTheDocument();
    expect(within(screen.getByTestId("session-queue-2")).getByText("Queued")).toBeInTheDocument();
    expect(within(screen.getByTestId("session-queue-3")).getByText("Joined")).toBeInTheDocument();

    // Em jogo: só as contas com cliente rodando.
    expect(screen.getByTestId("session-running-1")).toBeInTheDocument();
    expect(screen.getByTestId("session-running-3")).toBeInTheDocument();
    expect(screen.queryByTestId("session-running-2")).not.toBeInTheDocument();
    expect(screen.getByText("2 running")).toBeInTheDocument();
  });

  it("shows the alias and masks names when the app hides usernames", () => {
    renderPanel({
      launchQueue: queue([entry(2, "queued")]),
      launchedByProgram: new Set<number>(),
    });
    expect(within(screen.getByTestId("session-queue-2")).getByText("Bravo Alt")).toBeInTheDocument();

    cleanup();
    renderPanel({
      launchQueue: queue([entry(2, "queued")]),
      launchedByProgram: new Set<number>(),
      hideUsernames: true,
      hiddenNameLetters: 2,
    });
    const row = screen.getByTestId("session-queue-2");
    expect(within(row).queryByText("Bravo Alt")).not.toBeInTheDocument();
    expect(within(row).getByText("Br********")).toBeInTheDocument();
  });

  it("surfaces a failed entry's backend error", () => {
    // Fixture com frase, não com código: esta linha desenha `entry.error` cru, e
    // o backend manda frase justamente por isso (`version_conflict_message`).
    const erro =
      "A Roblox client is already running on a different Roblox version. Open now: system install.";
    renderPanel({
      launchQueue: queue([entry(1, "failed", erro)]),
      launchedByProgram: new Set<number>(),
    });
    const row = screen.getByTestId("session-queue-1");
    expect(within(row).getByText("Failed")).toBeInTheDocument();
    expect(within(row).getByText(erro)).toBeInTheDocument();
  });

  it("explains both empty states and disables Stop queue", () => {
    renderPanel({ launchQueue: null, launchedByProgram: new Set<number>() });

    expect(
      screen.getByText("Nothing in the launch queue. Pick a game to start joining accounts.")
    ).toBeInTheDocument();
    expect(
      screen.getByText("No Roblox client is running. Accounts you launch show up here.")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Stop queue/ })).toBeDisabled();
  });
});

describe("SessionPanel — queue actions", () => {
  it("cancels only the clicked account", async () => {
    const user = userEvent.setup();
    renderPanel({
      launchQueue: queue([entry(1, "queued"), entry(2, "queued")]),
      launchedByProgram: new Set<number>(),
    });

    await user.click(within(screen.getByTestId("session-queue-2")).getByRole("button"));

    await waitFor(() => expect(callsFor("cancel_account_launch")).toHaveLength(1));
    expect(callsFor("cancel_account_launch")[0][1]).toEqual({ userId: 2 });
  });

  it("never closes a client when cancelling (cancel !== kill)", async () => {
    const user = userEvent.setup();
    // Conta 1 já está com cliente aberto E ainda na fila: cancelar não pode
    // encostar nesse cliente.
    renderPanel({
      launchQueue: queue([entry(1, "launching")]),
      launchedByProgram: new Set([1]),
    });

    await user.click(within(screen.getByTestId("session-queue-1")).getByRole("button"));
    await waitFor(() => expect(callsFor("cancel_account_launch")).toHaveLength(1));

    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
    expect(callsFor("cmd_kill_all_roblox")).toHaveLength(0);
  });

  it("offers no cancel button for finished entries", () => {
    renderPanel({
      launchQueue: queue([entry(1, "done"), entry(2, "failed", "boom"), entry(3, "cancelled")]),
      launchedByProgram: new Set<number>(),
    });
    for (const id of [1, 2, 3]) {
      expect(within(screen.getByTestId(`session-queue-${id}`)).queryByRole("button")).toBeNull();
    }
  });

  it("stops the whole queue from the header", async () => {
    const user = userEvent.setup();
    renderPanel({
      launchQueue: queue([entry(1, "queued"), entry(2, "queued")]),
      launchedByProgram: new Set<number>(),
    });

    await user.click(screen.getByRole("button", { name: /Stop queue/ }));

    await waitFor(() => expect(callsFor("stop_launch_queue")).toHaveLength(1));
    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
  });

  it("shows a backend failure instead of swallowing it", async () => {
    const user = userEvent.setup();
    setInvokeMap({
      stop_launch_queue: () => {
        throw new Error("queue is gone");
      },
    });
    renderPanel({
      launchQueue: queue([entry(1, "queued")]),
      launchedByProgram: new Set<number>(),
    });

    await user.click(screen.getByRole("button", { name: /Stop queue/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent("queue is gone");
  });
});

describe("SessionPanel — running clients", () => {
  it("focuses the clicked client's window", async () => {
    const user = userEvent.setup();
    renderPanel({ launchQueue: null, launchedByProgram: new Set([1, 3]) });

    await user.click(within(screen.getByTestId("session-running-3")).getByRole("button", { name: "Focus" }));

    await waitFor(() => expect(callsFor("focus_roblox_window")).toHaveLength(1));
    expect(callsFor("focus_roblox_window")[0][1]).toEqual({ userId: 3 });
  });

  it("reports a window that could not be raised", async () => {
    const user = userEvent.setup();
    setInvokeMap({ focus_roblox_window: false });
    renderPanel({ launchQueue: null, launchedByProgram: new Set([1]) });

    await user.click(within(screen.getByTestId("session-running-1")).getByRole("button", { name: "Focus" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not bring that Roblox window to the front."
    );
  });

  it("closes a single client without asking for confirmation", async () => {
    const user = userEvent.setup();
    renderPanel({ launchQueue: null, launchedByProgram: new Set([1, 2]) });

    await user.click(within(screen.getByTestId("session-running-2")).getByRole("button", { name: "Close" }));

    await waitFor(() => expect(callsFor("cmd_kill_roblox")).toHaveLength(1));
    expect(callsFor("cmd_kill_roblox")[0][1]).toEqual({ userId: 2 });
    expect(confirmMock).not.toHaveBeenCalled();
  });

  it("closes a multi-selection after a single confirmation", async () => {
    const user = userEvent.setup();
    promptAnswers.confirm = true;
    renderPanel({ launchQueue: null, launchedByProgram: new Set([1, 2, 3]) });

    await user.click(screen.getByRole("checkbox", { name: "Select alpha" }));
    await user.click(screen.getByRole("checkbox", { name: "Select Bravo Alt" }));
    await user.click(screen.getByRole("checkbox", { name: "Select charlie" }));

    await user.click(screen.getByRole("button", { name: /Close accounts \(3\)/ }));

    await waitFor(() => expect(callsFor("cmd_kill_roblox")).toHaveLength(3));
    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(confirmMock.mock.calls[0][0]).toContain("3");
    expect(callsFor("cmd_kill_roblox").map((c) => c[1])).toEqual([
      { userId: 1 },
      { userId: 2 },
      { userId: 3 },
    ]);
  });

  it("closes nothing when the confirmation is declined", async () => {
    const user = userEvent.setup();
    promptAnswers.confirm = false;
    renderPanel({ launchQueue: null, launchedByProgram: new Set([1, 2]) });

    await user.click(screen.getByRole("checkbox", { name: "Select all running clients" }));
    await user.click(screen.getByRole("button", { name: /Close accounts \(2\)/ }));

    await waitFor(() => expect(confirmMock).toHaveBeenCalledTimes(1));
    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
  });
});

describe("SessionPanel — live updates", () => {
  it("follows the store's queue state without a reload", async () => {
    // O evento `launch-queue` é ouvido pela store; o painel só reflete o
    // estado. Re-renderizar com o payload novo é o que o listener faz.
    const { rerender } = renderPanel({
      launchQueue: queue([entry(1, "queued"), entry(2, "queued")]),
      launchedByProgram: new Set<number>(),
    });
    expect(within(screen.getByTestId("session-queue-1")).getByText("Queued")).toBeInTheDocument();

    setStore({
      accounts: ACCOUNTS,
      ...storeActions(),
      launchQueue: queue([entry(1, "done"), entry(2, "launching")]),
      launchedByProgram: new Set([1]),
    });
    rerender(<SessionPanel />);

    await waitFor(() =>
      expect(within(screen.getByTestId("session-queue-1")).getByText("Joined")).toBeInTheDocument()
    );
    expect(within(screen.getByTestId("session-queue-2")).getByText("Joining")).toBeInTheDocument();
    expect(screen.getByTestId("session-running-1")).toBeInTheDocument();
  });

  it("drops a client that stopped running from the selection", async () => {
    const user = userEvent.setup();
    const { rerender } = renderPanel({ launchQueue: null, launchedByProgram: new Set([1, 2]) });

    await user.click(screen.getByRole("checkbox", { name: "Select all running clients" }));
    expect(screen.getByRole("button", { name: /Close accounts \(2\)/ })).toBeInTheDocument();

    // O polling deixa de ver a conta 2: ela some da lista e do lote.
    setStore({
      accounts: ACCOUNTS,
      ...storeActions(),
      launchQueue: null,
      launchedByProgram: new Set([1]),
    });
    rerender(<SessionPanel />);

    expect(screen.queryByTestId("session-running-2")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Close accounts \(1\)/ })).toBeInTheDocument();
  });
});

/**
 * Fechar as contas em jogo: sem marcação, o botão vale para **todas as que
 * estão na lista** — nunca para clientes fora dela (`closeRobloxClients`, não o
 * `killAllRobloxProcesses`). Mais de uma sempre pergunta antes.
 */
describe("SessionPanel — fechar contas", () => {
  it("sem marcar ninguém, fecha todas as da lista depois de uma confirmação", async () => {
    const user = userEvent.setup();
    promptAnswers.confirm = true;
    const { store } = renderPanel({ launchQueue: null, launchedByProgram: new Set([1, 2]) });

    await user.click(screen.getByRole("button", { name: "Close accounts" }));

    await waitFor(() => expect(callsFor("cmd_kill_roblox")).toHaveLength(2));
    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(confirmMock.mock.calls[0][0]).toContain("2");
    expect(store.killAllRobloxProcesses).not.toHaveBeenCalled();
  });

  it("recusada a confirmação, nada fecha", async () => {
    const user = userEvent.setup();
    promptAnswers.confirm = false;
    renderPanel({ launchQueue: null, launchedByProgram: new Set([1, 2]) });

    await user.click(screen.getByRole("button", { name: "Close accounts" }));

    await waitFor(() => expect(confirmMock).toHaveBeenCalledTimes(1));
    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
  });

  it("com uma conta só em jogo, fecha sem perguntar", async () => {
    const user = userEvent.setup();
    renderPanel({ launchQueue: null, launchedByProgram: new Set([3]) });

    await user.click(screen.getByRole("button", { name: "Close accounts" }));

    await waitFor(() => expect(callsFor("cmd_kill_roblox")).toHaveLength(1));
    expect(callsFor("cmd_kill_roblox")[0][1]).toEqual({ userId: 3 });
    expect(confirmMock).not.toHaveBeenCalled();
  });

  it("não aparece sem cliente rodando", () => {
    renderPanel({ launchQueue: null, launchedByProgram: new Set() });
    expect(screen.queryByRole("button", { name: /Close accounts/ })).not.toBeInTheDocument();
  });
});

/**
 * O botão de Auto Rejoin daqui ligava o ciclo na hora, sem tela: quem clicava
 * não via o tempo do ciclo, nem as contas main, nem onde parar. Agora ele abre o
 * Modo AFK com as contas em jogo — o Start de lá é que adota, sem fechar nada.
 */
describe("SessionPanel — Modo AFK com as contas em jogo", () => {
  function comRodando(overrides: Partial<StoreValue> = {}) {
    return renderPanel({
      launchedByProgram: new Set([1, 2]),
      launchQueue: queue([]),
      ...overrides,
    });
  }

  it("abre o Modo AFK com as contas marcadas, sem ligar nada nem fechar cliente", async () => {
    const { store } = comRodando({ launchedByProgram: new Set([1, 2, 3]) });

    await userEvent.click(screen.getByRole("checkbox", { name: "Select alpha" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Select charlie" }));
    await userEvent.click(screen.getByRole("button", { name: /AFK Mode/ }));

    expect(store.openAfkMode).toHaveBeenCalledWith({
      tab: "clicks",
      targetUserIds: [1, 3],
      adoptRunning: true,
    });
    expect(store.adoptRunningIntoBotting).not.toHaveBeenCalled();
    expect(store.startBottingMode).not.toHaveBeenCalled();
    expect(invokeMock).not.toHaveBeenCalledWith("cmd_kill_roblox", expect.anything());
  });

  it("sem marcar ninguém, leva todas as que estão em jogo", async () => {
    const { store } = comRodando();

    await userEvent.click(screen.getByRole("button", { name: /AFK Mode/ }));

    expect(store.openAfkMode).toHaveBeenCalledWith({
      tab: "clicks",
      targetUserIds: [1, 2],
      adoptRunning: true,
    });
  });

  it("com só os cliques AFK ligados, abre na aba deles", async () => {
    const { store } = comRodando({
      afkStatus: {
        active: true,
        startedAtMs: 1,
        intervalSeconds: 600,
        key: "Space",
        mode: "key",
        clickX: 50,
        clickY: 50,
        accounts: [],
      },
    });

    await userEvent.click(screen.getByRole("button", { name: /AFK Mode/ }));

    expect(store.openAfkMode).toHaveBeenCalledWith(expect.objectContaining({ tab: "clicks" }));
  });

  /** Os cliques AFK são o padrão (pedido do dono, 03/10/2026); o Auto Rejoin só
   * abre direto quando é ele que está rodando. */
  it("com só o Auto Rejoin ligado, abre na aba dele", async () => {
    const { store } = comRodando({ bottingStatus: makeBottingStatus({ active: true, userIds: [1, 2] }) });

    await userEvent.click(screen.getByRole("button", { name: /AFK Mode/ }));

    expect(store.openAfkMode).toHaveBeenCalledWith(expect.objectContaining({ tab: "rejoin" }));
  });

  it("não oferece o botão quando não há cliente rodando", () => {
    renderPanel({ launchedByProgram: new Set(), launchQueue: queue([]) });
    expect(screen.queryByRole("button", { name: /AFK Mode/ })).not.toBeInTheDocument();
  });
});

/**
 * Make Friends não tinha como ser acompanhado: o progresso era `{phase, done,
 * total}` num `useState` de dois componentes, e na fase de envio o `done`
 * contava **pares**. Agora o painel mostra conta por conta, no mesmo lugar em
 * que se acompanha a fila de launch.
 */
describe("SessionPanel — Make Friends", () => {
  function friendLink(overrides: Partial<FriendLinkState> = {}): FriendLinkState {
    return {
      active: true,
      phase: "linking",
      processed: 1,
      total: 3,
      mode: "star",
      mainUserId: 1,
      accounts: [
        { userId: 1, state: "processing", error: null },
        { userId: 2, state: "done", error: null },
        { userId: 3, state: "pending", error: null },
      ],
      ...overrides,
    };
  }

  it("não ocupa espaço no painel enquanto ninguém rodou Make Friends", () => {
    renderPanel({ launchQueue: queue([entry(1, "queued")]) });
    expect(screen.queryByTestId("friend-link-panel")).not.toBeInTheDocument();
  });

  it("diz quantas contas já foram processadas e o estado de cada uma", () => {
    renderPanel({ friendLinkState: friendLink() });

    const painel = within(screen.getByTestId("friend-link-panel"));
    expect(painel.getByText("1 / 3 accounts processed")).toBeInTheDocument();
    expect(painel.getByText("Sending friend requests")).toBeInTheDocument();

    expect(within(screen.getByTestId("friend-link-1")).getByText("Processing")).toBeInTheDocument();
    expect(within(screen.getByTestId("friend-link-2")).getByText("Linked")).toBeInTheDocument();
    expect(within(screen.getByTestId("friend-link-3")).getByText("Waiting")).toBeInTheDocument();
  });

  it("marca qual é a conta principal do modo star", () => {
    renderPanel({ friendLinkState: friendLink() });
    expect(within(screen.getByTestId("friend-link-1")).getByText("main")).toBeInTheDocument();
    expect(within(screen.getByTestId("friend-link-2")).queryByText("main")).not.toBeInTheDocument();
  });

  it("mostra o erro na conta que falhou, e não num texto agregado", () => {
    renderPanel({
      friendLinkState: friendLink({
        accounts: [
          { userId: 1, state: "failed", error: "cookie inválido" },
          { userId: 2, state: "done", error: null },
          { userId: 3, state: "done", error: null },
        ],
      }),
    });

    const linha = within(screen.getByTestId("friend-link-1"));
    expect(linha.getByText("Failed")).toBeInTheDocument();
    expect(linha.getByText("cookie inválido")).toBeInTheDocument();
  });

  it("continua mostrando o resultado depois que a operação termina", () => {
    renderPanel({
      friendLinkState: friendLink({ active: false, phase: "done", processed: 3 }),
    });

    const painel = within(screen.getByTestId("friend-link-panel"));
    expect(painel.getByText("3 / 3 accounts processed")).toBeInTheDocument();
    expect(painel.getByText("Finished")).toBeInTheDocument();
  });

  it("usa o alias mascarado, como o resto do painel", () => {
    renderPanel({
      friendLinkState: friendLink(),
      hideUsernames: true,
      hiddenNameLetters: 2,
    });
    expect(within(screen.getByTestId("friend-link-2")).getByText("Br********")).toBeInTheDocument();
  });
});

describe("SessionPanel — clientes abertos fora do app", () => {
  it("marks a running client that was opened from the website", () => {
    renderPanel({
      launchedByProgram: new Set([1, 3]),
      adoptedClients: new Set([3]),
    });
    expect(within(screen.getByTestId("session-running-3")).getByText("Opened outside the app")).toBeInTheDocument();
    expect(within(screen.getByTestId("session-running-1")).queryByText("Opened outside the app")).not.toBeInTheDocument();
  });

  it("lists unidentified clients apart from the running accounts, with why", () => {
    renderPanel({
      launchedByProgram: new Set([1]),
      unidentifiedClients: [
        unidentified(4100, "waitingForGame"),
        unidentified(4200, "noLog"),
        unidentified(4300, "unknownAccount", 999),
        unidentified(4400, "accountBusy", 1),
      ],
    });

    const waiting = screen.getByTestId("session-unidentified-4100");
    expect(within(waiting).getByText("Unidentified client")).toBeInTheDocument();
    expect(within(waiting).getByText(/PID 4100/)).toBeInTheDocument();
    expect(within(waiting).getByText(/hasn't joined a game yet/)).toBeInTheDocument();
    expect(within(screen.getByTestId("session-unidentified-4200")).getByText(/No Roblox log matched/)).toBeInTheDocument();
    expect(within(screen.getByTestId("session-unidentified-4300")).getByText(/not in your list/)).toBeInTheDocument();
    expect(within(screen.getByTestId("session-unidentified-4400")).getByText(/alpha already has a client open/)).toBeInTheDocument();

    // Não entram na contagem nem no lote de fechar: só as contas.
    expect(screen.getByText("1 running")).toBeInTheDocument();
    expect(screen.getByText("4 unidentified")).toBeInTheDocument();
    // Cliente não identificado nunca ganha botão de fechar.
    expect(within(waiting).queryByRole("button", { name: /Close/ })).not.toBeInTheDocument();
  });

  it("does not show the empty state when only unidentified clients are open", () => {
    renderPanel({ unidentifiedClients: [unidentified(4100, "noLog")] });
    expect(
      screen.queryByText("No Roblox client is running. Accounts you launch show up here.")
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("session-unidentified-4100")).toBeInTheDocument();
  });

  it("Show window focuses the client by its pid", async () => {
    const user = userEvent.setup();
    renderPanel({ unidentifiedClients: [unidentified(4100, "noLog")] });

    await user.click(
      within(screen.getByTestId("session-unidentified-4100")).getByRole("button", { name: /Show window/ })
    );
    expect(callsFor("focus_client_window")).toEqual([["focus_client_window", { pid: 4100 }]]);
  });

  it("explains when the window could not be shown", async () => {
    const user = userEvent.setup();
    setInvokeMap({ focus_client_window: false });
    renderPanel({ unidentifiedClients: [unidentified(4100, "noLog")] });

    await user.click(screen.getByRole("button", { name: /Show window/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not bring that Roblox window to the front."
    );
  });

  it("Identify assigns the picked account to that pid, marking accounts already in game", async () => {
    const user = userEvent.setup();
    renderPanel({
      launchedByProgram: new Set([1]),
      unidentifiedClients: [unidentified(4100, "waitingForGame")],
    });
    const row = screen.getByTestId("session-unidentified-4100");

    await user.click(within(row).getByRole("button", { name: /Identify/ }));
    const picker = within(row).getByRole("combobox", { name: "Account for this client" });
    expect(within(picker).getByRole("option", { name: "alpha (already in game)" })).toBeInTheDocument();
    expect(within(picker).getByRole("option", { name: "Bravo Alt" })).toBeInTheDocument();

    // Sem escolher, confirmar não faz nada.
    expect(within(row).getByRole("button", { name: /Confirm/ })).toBeDisabled();
    await user.selectOptions(picker, "2");
    await user.click(within(row).getByRole("button", { name: /Confirm/ }));

    expect(callsFor("identify_external_client")).toEqual([
      ["identify_external_client", { pid: 4100, userId: 2 }],
    ]);
  });

  it("starts the picker on the account the log named", async () => {
    const user = userEvent.setup();
    renderPanel({
      launchedByProgram: new Set([1]),
      unidentifiedClients: [unidentified(4400, "accountBusy", 1)],
    });
    await user.click(screen.getByRole("button", { name: /Identify/ }));
    expect(screen.getByRole("combobox", { name: "Account for this client" })).toHaveValue("1");
  });

  it("surfaces the backend refusal when identifying fails", async () => {
    const user = userEvent.setup();
    setInvokeMap({ identify_external_client: () => Promise.reject("That Roblox client is no longer running.") });
    renderPanel({ unidentifiedClients: [unidentified(4100, "noLog")] });

    await user.click(screen.getByRole("button", { name: /Identify/ }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Account for this client" }), "3");
    await user.click(screen.getByRole("button", { name: /Confirm/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("That Roblox client is no longer running.");
  });

  it("masks account names in the picker when names are hidden", async () => {
    const user = userEvent.setup();
    renderPanel({
      unidentifiedClients: [unidentified(4100, "noLog")],
      hideUsernames: true,
      hiddenNameLetters: 2,
    });
    await user.click(screen.getByRole("button", { name: /Identify/ }));
    const picker = screen.getByRole("combobox", { name: "Account for this client" });
    expect(within(picker).queryByRole("option", { name: "Bravo Alt" })).not.toBeInTheDocument();
    expect(within(picker).getByRole("option", { name: "Br********" })).toBeInTheDocument();
  });
});

describe("SessionPanel — quedas lidas do log", () => {
  function health(drop: Partial<ClientDrop> | null): ClientHealth {
    return {
      pid: 4242,
      logFound: true,
      drop: drop
        ? { kind: "disconnected", reason: null, code: null, message: null, sinceMs: 1_700_000_000_000, ...drop }
        : null,
    };
  }

  it("shows why each account dropped, next to its name", () => {
    renderPanel({
      launchedByProgram: new Set([1, 2, 3]),
      clientHealth: new Map([
        [1, health({ kind: "disconnected", reason: "connectionLost", code: 277 })],
        [2, health({ kind: "kicked", code: 267, message: "Server restarting" })],
        [3, health({ kind: "serverShutdown", code: 274 })],
      ]),
    });
    const lost = within(screen.getByTestId("session-running-1")).getByTestId("client-health-note");
    expect(lost).toHaveTextContent("Disconnected: lost connection");
    // O código fica no tooltip, para quem quiser procurar.
    expect(lost).toHaveAttribute("title", expect.stringContaining("277"));
    expect(within(screen.getByTestId("session-running-2")).getByText("Kicked: Server restarting")).toBeInTheDocument();
    expect(within(screen.getByTestId("session-running-3")).getByText("The server shut down")).toBeInTheDocument();
  });

  it("says nothing for an account that is fine", () => {
    renderPanel({
      launchedByProgram: new Set([1, 2]),
      clientHealth: new Map([[1, health(null)]]),
    });
    expect(screen.queryByTestId("client-health-note")).not.toBeInTheDocument();
  });

  it("a website client also shows its drop (display only, it keeps the same buttons)", () => {
    renderPanel({
      launchedByProgram: new Set([3]),
      adoptedClients: new Set([3]),
      clientHealth: new Map([[3, health({ kind: "disconnected", reason: "joinedElsewhere", code: 273 })]]),
    });
    const row = screen.getByTestId("session-running-3");
    expect(within(row).getByText("Disconnected: the account joined somewhere else")).toBeInTheDocument();
    expect(within(row).getByText("Opened outside the app")).toBeInTheDocument();
  });

  it("shows a hung window as not responding, in its own color", () => {
    renderPanel({
      launchedByProgram: new Set([1]),
      clientHealth: new Map([[1, { ...health(null), notResponding: true }]]),
    });
    const note = within(screen.getByTestId("session-running-1")).getByTestId("client-health-note");
    expect(note).toHaveTextContent("Not responding");
    expect(note).toHaveAttribute("data-tone", "hung");
  });
});

describe("SessionPanel — reconexão automática", () => {
  const NOW = Date.now();

  function reconnect(userId: number, partial: Partial<AutoReconnectEntry>): AutoReconnectEntry {
    return {
      userId,
      phase: "waiting",
      attempt: 1,
      maxAttempts: 5,
      nextAttemptAtMs: NOW + 30_000,
      reason: null,
      error: null,
      drop: { kind: "disconnected", reason: "connectionLost", code: 277, message: null, sinceMs: NOW },
      ...partial,
    };
  }

  function reconnectActions(): Partial<StoreValue> {
    return {
      stopAutoReconnect: vi.fn(async (userId: number) => (await invokeMock("stop_auto_reconnect", { userId })) as boolean),
      retryAutoReconnect: vi.fn(
        async (userId: number) => (await invokeMock("retry_auto_reconnect", { userId })) as boolean
      ),
    };
  }

  beforeEach(() => {
    setInvokeMap({ stop_auto_reconnect: true, retry_auto_reconnect: true });
  });

  it("has no section while nothing is reconnecting", () => {
    renderPanel();
    expect(screen.queryByTestId("auto-reconnect-panel")).not.toBeInTheDocument();
  });

  it("shows the countdown and the attempt, with Try now and Stop", async () => {
    renderPanel({
      ...reconnectActions(),
      autoReconnect: [reconnect(1, { attempt: 2, nextAttemptAtMs: Date.now() + 30_000 })],
    });
    const row = screen.getByTestId("session-reconnect-1");
    expect(within(row).getByText("alpha")).toBeInTheDocument();
    expect(within(row).getByText(/^Reconnecting in (29|30) s \(attempt 2\/5\)$/)).toBeInTheDocument();

    await userEvent.click(within(row).getByRole("button", { name: /Try now/ }));
    expect(callsFor("retry_auto_reconnect")).toEqual([["retry_auto_reconnect", { userId: 1 }]]);
    await userEvent.click(within(row).getByRole("button", { name: /Stop/ }));
    expect(callsFor("stop_auto_reconnect")).toEqual([["stop_auto_reconnect", { userId: 1 }]]);
    // Parar a reconexão nunca fecha cliente.
    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
  });

  it("says when it gave up, and offers to try again or hide the line", async () => {
    renderPanel({
      ...reconnectActions(),
      autoReconnect: [reconnect(2, { phase: "gaveUp", attempt: 5, error: "PID not detected" })],
    });
    const row = screen.getByTestId("session-reconnect-2");
    expect(within(row).getByText("Gave up after 5 tries")).toBeInTheDocument();
    expect(within(row).getByText("PID not detected")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: /Try again/ })).toBeInTheDocument();
    await userEvent.click(within(row).getByRole("button", { name: "Dismiss Bravo Alt" }));
    expect(callsFor("stop_auto_reconnect")).toEqual([["stop_auto_reconnect", { userId: 2 }]]);
  });

  it("explains why it will not reconnect, without a retry for a banned account", () => {
    renderPanel({
      autoReconnect: [
        reconnect(1, { phase: "stopped", reason: "joinedElsewhere" }),
        reconnect(3, { phase: "stopped", reason: "banned" }),
      ],
    });
    expect(
      within(screen.getByTestId("session-reconnect-1")).getByText("Not reconnecting: the account joined somewhere else")
    ).toBeInTheDocument();
    const banned = screen.getByTestId("session-reconnect-3");
    expect(within(banned).getByText("Not reconnecting: the account is banned")).toBeInTheDocument();
    expect(within(banned).queryByRole("button", { name: /Try/ })).not.toBeInTheDocument();
  });

  it("shows the attempt in progress without a Try now", () => {
    renderPanel({ autoReconnect: [reconnect(1, { phase: "launching", attempt: 3 })] });
    const row = screen.getByTestId("session-reconnect-1");
    expect(within(row).getByText("Reconnecting now (attempt 3/5)")).toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: /Try/ })).not.toBeInTheDocument();
    expect(within(row).getByRole("button", { name: /Stop/ })).toBeInTheDocument();
  });

  /** A causa ficava só no tooltip: agora é uma segunda linha, à vista. */
  it("shows the cause under the line, for a pending reconnect too", () => {
    renderPanel({
      autoReconnect: [
        reconnect(2, { phase: "gaveUp", attempt: 5, error: "PID not detected" }),
        reconnect(1, { phase: "waiting", attempt: 2, error: "It did not get into the game in 2 minutes" }),
      ],
    });
    expect(screen.getByTestId("session-reconnect-detail-2")).toHaveTextContent("PID not detected");
    expect(screen.getByTestId("session-reconnect-detail-1")).toHaveTextContent(
      "It did not get into the game in 2 minutes"
    );
  });

  it("has no cause line when there is no error", () => {
    renderPanel({ autoReconnect: [reconnect(1, { phase: "waiting" })] });
    expect(screen.queryByTestId("session-reconnect-detail-1")).not.toBeInTheDocument();
  });

  /** Sem retorno, o clique parecia não ter feito nada (e dava para clicar de novo). */
  it("disables the line's buttons while the command runs", async () => {
    let finish: (value: boolean) => void = () => {};
    setInvokeMap({ retry_auto_reconnect: () => new Promise<boolean>((resolve) => (finish = resolve)) });
    renderPanel({ ...reconnectActions(), autoReconnect: [reconnect(1, { phase: "waiting" })] });
    const row = screen.getByTestId("session-reconnect-1");
    await userEvent.click(within(row).getByRole("button", { name: /Try now/ }));
    expect(within(row).getByRole("button", { name: /Try now/ })).toBeDisabled();
    expect(within(row).getByRole("button", { name: /Stop/ })).toBeDisabled();
    finish(true);
    await waitFor(() => expect(within(row).getByRole("button", { name: /Try now/ })).not.toBeDisabled());
  });

  it("hides a dismissed line right away, before the backend answers", async () => {
    setInvokeMap({ stop_auto_reconnect: () => new Promise<boolean>(() => {}) });
    renderPanel({ ...reconnectActions(), autoReconnect: [reconnect(2, { phase: "gaveUp", attempt: 5 })] });
    await userEvent.click(screen.getByRole("button", { name: "Dismiss Bravo Alt" }));
    expect(screen.queryByTestId("session-reconnect-2")).not.toBeInTheDocument();
  });

  it("says so when the account is no longer reconnecting", async () => {
    setInvokeMap({ stop_auto_reconnect: false, retry_auto_reconnect: false });
    const { store } = renderPanel({
      ...reconnectActions(),
      autoReconnect: [reconnect(2, { phase: "gaveUp", attempt: 5 }), reconnect(1, { phase: "waiting" })],
    });
    await userEvent.click(within(screen.getByTestId("session-reconnect-1")).getByRole("button", { name: /Try now/ }));
    await waitFor(() =>
      expect(store.addToast).toHaveBeenCalledWith("alpha is no longer being reconnected.", "info")
    );
    // Dispensar uma que o backend já não tem: some do mesmo jeito, com o aviso.
    await userEvent.click(screen.getByRole("button", { name: "Dismiss Bravo Alt" }));
    await waitFor(() =>
      expect(store.addToast).toHaveBeenCalledWith("Bravo Alt is no longer being reconnected.", "info")
    );
    expect(screen.queryByTestId("session-reconnect-2")).not.toBeInTheDocument();
  });

  it("brings a dismissed line back when the command fails", async () => {
    setInvokeMap({
      stop_auto_reconnect: () => {
        throw new Error("boom");
      },
    });
    renderPanel({ ...reconnectActions(), autoReconnect: [reconnect(2, { phase: "gaveUp", attempt: 5 })] });
    await userEvent.click(screen.getByRole("button", { name: "Dismiss Bravo Alt" }));
    await waitFor(() => expect(screen.getByTestId("session-reconnect-2")).toBeInTheDocument());
    expect(screen.getByText(/boom/)).toBeInTheDocument();
  });

  it("shows a new drop of a dismissed account again", async () => {
    setInvokeMap({ stop_auto_reconnect: () => new Promise<boolean>(() => {}) });
    const actions = reconnectActions();
    const { rerender } = renderPanel({ ...actions, autoReconnect: [reconnect(2, { phase: "gaveUp", attempt: 5 })] });
    await userEvent.click(screen.getByRole("button", { name: "Dismiss Bravo Alt" }));
    expect(screen.queryByTestId("session-reconnect-2")).not.toBeInTheDocument();
    // Caiu de novo: outra queda (outro `sinceMs`) é outra linha.
    const again = reconnect(2, { phase: "waiting" });
    setStore({
      accounts: ACCOUNTS,
      ...storeActions(),
      ...actions,
      autoReconnect: [{ ...again, drop: { ...again.drop, sinceMs: NOW + 60_000 } }],
    });
    rerender(<SessionPanel />);
    expect(screen.getByTestId("session-reconnect-2")).toBeInTheDocument();
  });
});

/**
 * Reconexão automática por conta, na lista "Em jogo" (antes ficava no painel
 * de uma conta, que quase ninguém abre com muitas contas). O campo da conta
 * (`Fields.AutoReconnect`) manda; sem ele vale `General.AutoReconnect`; o
 * AutoRelaunch do Nexus liga por cima (`reconnect_enabled`, commands/reconnect.rs).
 */
describe("SessionPanel — reconexão por conta", () => {
  function renderRunning(overrides: Partial<StoreValue> = {}, accounts: Account[] = ACCOUNTS) {
    return renderWithStore(<SessionPanel />, {
      accounts,
      launchedByProgram: new Set(accounts.map((a) => a.UserID)),
      ...storeActions(),
      ...overrides,
    });
  }

  function savedAccounts(store: StoreValue): Account[] {
    return (store.updateAccount as unknown as { mock: { calls: [Account][] } }).mock.calls.map((c) => c[0]);
  }

  function clearSaved(store: StoreValue) {
    (store.updateAccount as unknown as { mockClear: () => void }).mockClear();
  }

  function rowSwitch(userId: number) {
    return within(screen.getByTestId(`session-running-${userId}`)).getByRole("switch");
  }

  beforeEach(() => {
    setInvokeMap({ get_nexus_accounts: [] });
  });

  it("each row follows the default until the account changes it, and says so", () => {
    renderRunning({ settings: { General: { AutoReconnect: "true" } } });
    const row = screen.getByTestId("session-running-1");
    const toggle = within(row).getByRole("switch", { name: "Auto-reconnect for alpha" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(within(row).getByText("default")).toBeInTheDocument();
    expect(toggle).toHaveAttribute("title", "Following the default from Settings › General (on).");
  });

  it("turning a row off writes the account field and keeps the other fields", async () => {
    const accounts = [makeAccount({ UserID: 1, Username: "alpha", Fields: { RobloxVersion: "LIVE:abc" } })];
    const { store } = renderRunning({ settings: { General: { AutoReconnect: "true" } } }, accounts);
    await userEvent.click(rowSwitch(1));
    const [saved] = savedAccounts(store);
    expect(saved.Fields.AutoReconnect).toBe("false");
    expect(saved.Fields.RobloxVersion).toBe("LIVE:abc");
  });

  it("the account's own choice wins over the default, and can go back to it", async () => {
    const accounts = [makeAccount({ UserID: 1, Username: "alpha", Fields: { AutoReconnect: "false" } })];
    const { store } = renderRunning({ settings: { General: { AutoReconnect: "true" } } }, accounts);
    const row = screen.getByTestId("session-running-1");
    expect(rowSwitch(1)).toHaveAttribute("aria-checked", "false");
    expect(within(row).queryByText("default")).not.toBeInTheDocument();
    await userEvent.click(within(row).getByRole("button", { name: "Use the default from Settings › General (on)" }));
    expect(savedAccounts(store)[0].Fields).not.toHaveProperty("AutoReconnect");
  });

  it("shows the row locked on when Nexus AutoRelaunch keeps it on", async () => {
    setInvokeMap({ get_nexus_accounts: [{ username: "ALPHA", auto_relaunch: true }] });
    const accounts = [makeAccount({ UserID: 1, Username: "alpha", Fields: { AutoReconnect: "false" } })];
    renderRunning({}, accounts);
    await waitFor(() => expect(rowSwitch(1)).toHaveAttribute("aria-checked", "true"));
    expect(rowSwitch(1)).toHaveAttribute("aria-disabled", "true");
    expect(rowSwitch(1)).toHaveAttribute(
      "title",
      "Nexus AutoRelaunch is on for this account, so it reconnects even with this off."
    );
  });

  it("does not blame Nexus when its AutoRelaunch is off for the account", async () => {
    setInvokeMap({ get_nexus_accounts: [{ username: "alpha", auto_relaunch: false }] });
    renderRunning({}, [makeAccount({ UserID: 1, Username: "alpha" })]);
    await waitFor(() => expect(callsFor("get_nexus_accounts")).toHaveLength(1));
    expect(rowSwitch(1)).toHaveAttribute("aria-checked", "false");
    expect(rowSwitch(1)).not.toHaveAttribute("aria-disabled");
  });

  it("bulk: the checked rows turn on, off, or go back to the default", async () => {
    const accounts = [
      makeAccount({ UserID: 1, Username: "alpha", Fields: { AutoReconnect: "false" } }),
      makeAccount({ UserID: 2, Username: "bravo" }),
      makeAccount({ UserID: 3, Username: "charlie" }),
    ];
    const { store } = renderRunning({}, accounts);
    // Nada marcado: as ações de lote não aparecem.
    expect(screen.queryByRole("button", { name: "Reconnect on" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: "Select alpha" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Select bravo" }));

    await userEvent.click(screen.getByRole("button", { name: "Reconnect on" }));
    expect(savedAccounts(store).map((a) => [a.UserID, a.Fields.AutoReconnect])).toEqual([
      [1, "true"],
      [2, "true"],
    ]);

    clearSaved(store);
    await userEvent.click(screen.getByRole("button", { name: "Reconnect off" }));
    expect(savedAccounts(store).map((a) => [a.UserID, a.Fields.AutoReconnect])).toEqual([
      [1, "false"],
      [2, "false"],
    ]);

    clearSaved(store);
    await userEvent.click(screen.getByRole("button", { name: "Use default" }));
    const saved = savedAccounts(store);
    expect(saved.map((a) => a.UserID)).toEqual([1, 2]);
    expect(saved.every((a) => !("AutoReconnect" in a.Fields))).toBe(true);
    // Mudar a reconexão nunca fecha cliente.
    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
  });

  it("explains a reconnect still running after the account was turned off", () => {
    renderRunning(
      {
        autoReconnect: [
          {
            userId: 1,
            phase: "launching",
            attempt: 2,
            maxAttempts: 5,
            nextAttemptAtMs: null,
            reason: null,
            error: null,
            drop: { kind: "crashed", reason: null, code: null, message: null, sinceMs: 0 },
          },
        ],
      },
      [makeAccount({ UserID: 1, Username: "alpha", Fields: { AutoReconnect: "false" } })]
    );
    expect(
      within(screen.getByTestId("session-reconnect-1")).getByText(
        "Turned off: it stops after the attempt in progress."
      )
    ).toBeInTheDocument();
  });

  it("has no reconnect controls outside Windows, where the Roblox log is not read", async () => {
    renderRunning({ platformCapabilities: makePlatformCapabilities({ os: "macos" }) });
    expect(within(screen.getByTestId("session-running-1")).queryByRole("switch")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: "Select alpha" }));
    expect(screen.queryByRole("button", { name: "Reconnect on" })).not.toBeInTheDocument();
  });
});

/**
 * Teto de memória por cliente, na lista "Em jogo" (commands/memory_ceiling.rs).
 * A linha mostra a memória do cliente (do polling de 2,5 s) e um seletor de
 * limite que grava o campo `MemoryLimit` da conta (vale na hora e nos próximos
 * launches); sem campo, vale `Optimization.MemoryLimit`. Só cliente que o app
 * abriu, e só com a feature `memory-trim` no binário.
 */
describe("SessionPanel — limite de memória por cliente", () => {
  const TRIM = makePlatformCapabilities({ supportsMemoryTrim: true });

  function renderRunning(overrides: Partial<StoreValue> = {}, accounts: Account[] = ACCOUNTS) {
    return renderWithStore(<SessionPanel />, {
      accounts,
      launchedByProgram: new Set(accounts.map((a) => a.UserID)),
      platformCapabilities: TRIM,
      ...storeActions(),
      ...overrides,
    });
  }

  function savedAccounts(store: StoreValue): Account[] {
    return (store.updateAccount as unknown as { mock: { calls: [Account][] } }).mock.calls.map((c) => c[0]);
  }

  function clearSaved(store: StoreValue) {
    (store.updateAccount as unknown as { mockClear: () => void }).mockClear();
  }

  function limitSelect(name: string) {
    return screen.getByRole("combobox", { name: `Memory limit for ${name}` }) as HTMLSelectElement;
  }

  beforeEach(() => {
    setInvokeMap({ get_nexus_accounts: [] });
  });

  it("shows the client's memory and follows the default until the account picks its own", () => {
    renderRunning({
      settings: { Optimization: { MemoryLimit: "2048" } },
      clientMemory: new Map([[1, { memoryMb: 1250, limitMb: 2048, over: false, trimmedAtMs: null }]]),
    });
    const row = screen.getByTestId("session-running-1");
    expect(within(row).getByText("1.2 GB")).toBeInTheDocument();
    const select = limitSelect("alpha");
    expect(select.value).toBe("default");
    expect(within(select).getByRole("option", { name: "Default (2 GB)" })).toBeInTheDocument();
    expect(select).toHaveAttribute("title", expect.stringMatching(/Following the default/));
  });

  it("says when there is no default limit", () => {
    renderRunning();
    expect(within(limitSelect("alpha")).getByRole("option", { name: "Default (no limit)" })).toBeInTheDocument();
  });

  it("picking a size writes the account field and keeps the other fields, closing nothing", async () => {
    const accounts = [makeAccount({ UserID: 1, Username: "alpha", Fields: { RobloxVersion: "LIVE:abc" } })];
    const { store } = renderRunning({}, accounts);
    await userEvent.selectOptions(limitSelect("alpha"), "2048");
    const [saved] = savedAccounts(store);
    expect(saved.Fields.MemoryLimit).toBe("2048");
    expect(saved.Fields.RobloxVersion).toBe("LIVE:abc");
    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
  });

  it("no limit is its own choice, and default removes the field", async () => {
    const accounts = [makeAccount({ UserID: 1, Username: "alpha", Fields: { MemoryLimit: "1536" } })];
    const { store } = renderRunning({ settings: { Optimization: { MemoryLimit: "2048" } } }, accounts);
    expect(limitSelect("alpha").value).toBe("1536");
    await userEvent.selectOptions(limitSelect("alpha"), "0");
    expect(savedAccounts(store)[0].Fields.MemoryLimit).toBe("0");
    clearSaved(store);
    await userEvent.selectOptions(limitSelect("alpha"), "default");
    expect(savedAccounts(store)[0].Fields).not.toHaveProperty("MemoryLimit");
  });

  it("a custom size is asked for, in MB or GB", async () => {
    const accounts = [makeAccount({ UserID: 1, Username: "alpha" })];
    const { store } = renderRunning({}, accounts);
    promptAnswers.prompt = "2.5 GB";
    await userEvent.selectOptions(limitSelect("alpha"), "custom");
    expect(savedAccounts(store)[0].Fields.MemoryLimit).toBe("2560");
  });

  it("a cancelled or unreadable custom size saves nothing", async () => {
    const accounts = [makeAccount({ UserID: 1, Username: "alpha" })];
    const { store } = renderRunning({}, accounts);
    promptAnswers.prompt = null;
    await userEvent.selectOptions(limitSelect("alpha"), "custom");
    promptAnswers.prompt = "lots";
    await userEvent.selectOptions(limitSelect("alpha"), "custom");
    expect(savedAccounts(store)).toHaveLength(0);
    expect(limitSelect("alpha").value).toBe("default");
  });

  it("a custom size already saved shows up as its own option", () => {
    renderRunning({}, [makeAccount({ UserID: 1, Username: "alpha", Fields: { MemoryLimit: "1800" } })]);
    const select = limitSelect("alpha");
    expect(select.value).toBe("1800");
    expect(within(select).getByRole("option", { name: "1.8 GB" })).toBeInTheDocument();
  });

  it("over the limit, the memory is highlighted and says the memory was freed", () => {
    renderRunning({
      clientMemory: new Map([[1, { memoryMb: 2500, limitMb: 2048, over: true, trimmedAtMs: 1 }]]),
    });
    const reading = within(screen.getByTestId("session-running-1")).getByText("2.4 GB");
    expect(reading.className).toMatch(/amber/);
    expect(reading).toHaveAttribute(
      "title",
      "Over its limit of 2 GB: MultiAlt asked Windows to free this client's memory."
    );
  });

  it("a client opened from the website has no limit control", () => {
    renderRunning({ adoptedClients: new Set([3]) });
    expect(screen.queryByRole("combobox", { name: "Memory limit for charlie" })).not.toBeInTheDocument();
    expect(limitSelect("alpha")).toBeInTheDocument();
  });

  it("without the memory-trim build there is no memory control at all", async () => {
    renderRunning({
      platformCapabilities: makePlatformCapabilities({ supportsMemoryTrim: false }),
      clientMemory: new Map([[1, { memoryMb: 1250, limitMb: null, over: false, trimmedAtMs: null }]]),
    });
    expect(screen.queryByRole("combobox", { name: /Memory limit for/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: "Select alpha" }));
    expect(screen.queryByTestId("session-memory-bulk")).not.toBeInTheDocument();
  });

  it("bulk: the checked rows get the same limit, or go back to the default", async () => {
    const accounts = [
      makeAccount({ UserID: 1, Username: "alpha", Fields: { MemoryLimit: "1024" } }),
      makeAccount({ UserID: 2, Username: "bravo" }),
      makeAccount({ UserID: 3, Username: "charlie" }),
    ];
    const { store } = renderRunning({}, accounts);
    expect(screen.queryByTestId("session-memory-bulk")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: "Select alpha" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Select bravo" }));

    const bulk = screen.getByRole("combobox", { name: "Memory limit for 2 selected" });
    await userEvent.selectOptions(bulk, "3072");
    expect(savedAccounts(store).map((a) => [a.UserID, a.Fields.MemoryLimit])).toEqual([
      [1, "3072"],
      [2, "3072"],
    ]);

    clearSaved(store);
    await userEvent.selectOptions(bulk, "default");
    expect(savedAccounts(store).every((a) => !("MemoryLimit" in a.Fields))).toBe(true);
    expect(callsFor("cmd_kill_roblox")).toHaveLength(0);
  });

  it("bulk skips the clients opened from the website", async () => {
    const { store } = renderRunning({ adoptedClients: new Set([3]) });
    await userEvent.click(screen.getByRole("checkbox", { name: "Select all running clients" }));
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Memory limit for 2 selected" }), "2048");
    expect(savedAccounts(store).map((a) => a.UserID)).toEqual([1, 2]);
  });
});

/**
 * Sessão de agora em cada linha do "Em jogo": jogo, tipo de servidor, tempo em
 * jogo (andando) e estado. O início é o mesmo que o histórico grava
 * (`get_current_sessions`, commands/session_history.rs); a tela relê no
 * `session-history-changed`, sem polling.
 */
describe("SessionPanel — sessão atual", () => {
  const MIN = 60_000;

  function current(userId: number, partial: Partial<CurrentSession> = {}): CurrentSession {
    return {
      userId,
      placeId: 606849621,
      jobId: "job-1",
      sinceMs: Date.now() - 12 * MIN,
      privateServer: false,
      ...partial,
    };
  }

  function renderRunning(sessions: CurrentSession[], overrides: Partial<StoreValue> = {}) {
    setInvokeMap({
      get_nexus_accounts: [],
      get_current_sessions: () => sessions,
      batched_get_game_info: (args: Record<string, unknown> | undefined) => ({
        placeId: Number(args?.placeId),
        universeId: 1,
        name: Number(args?.placeId) === 606849621 ? "Jailbreak" : null,
        iconUrl: null,
      }),
    });
    return renderWithStore(<SessionPanel />, {
      accounts: ACCOUNTS,
      launchedByProgram: new Set([1, 2, 3]),
      ...storeActions(),
      ...overrides,
    });
  }

  beforeEach(() => {
    clearGameIdentityCache();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the game, the server type and the time in game of each account", async () => {
    renderRunning([current(1), current(2, { privateServer: true, sinceMs: Date.now() - 65 * MIN })]);
    const first = within(screen.getByTestId("session-running-1"));
    expect(await first.findByText("Jailbreak")).toBeInTheDocument();
    expect(first.getByText("Public server")).toBeInTheDocument();
    expect(first.getByTestId("session-time-1")).toHaveTextContent("12m");
    expect(first.getByText("Playing")).toBeInTheDocument();

    const second = within(screen.getByTestId("session-running-2"));
    expect(await second.findByText("Jailbreak")).toBeInTheDocument();
    expect(second.getByText("Private server")).toBeInTheDocument();
    expect(second.getByTestId("session-time-2")).toHaveTextContent("1h 05m");
  });

  it("falls back to the Place ID while the game name is unknown, with the full name in a tooltip", async () => {
    renderRunning([current(1, { placeId: 123456 })]);
    const row = within(screen.getByTestId("session-running-1"));
    const game = await row.findByText("Place 123456");
    expect(game).toHaveAttribute("title", "Place 123456");
    await waitFor(() => expect(callsFor("batched_get_game_info").length).toBeGreaterThan(0));
    expect(row.getByText("Place 123456")).toBeInTheDocument();
  });

  it("says nothing about the server when it is not known (opened from the website)", async () => {
    renderRunning([current(1, { privateServer: null })]);
    const row = within(screen.getByTestId("session-running-1"));
    await row.findByText("Jailbreak");
    expect(row.queryByText("Public server")).not.toBeInTheDocument();
    expect(row.queryByText("Private server")).not.toBeInTheDocument();
  });

  it("the time in game ticks on its own", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    renderRunning([current(1, { sinceMs: Date.now() - 58_000 })]);
    const time = await screen.findByTestId("session-time-1");
    expect(time).toHaveTextContent("58s");
    await act(async () => {
      vi.advanceTimersByTime(3_000);
    });
    expect(screen.getByTestId("session-time-1")).toHaveTextContent("1m");
  });

  it("reads the sessions again when the history changes, and does not poll", async () => {
    let sessions: CurrentSession[] = [];
    setInvokeMap({ get_nexus_accounts: [], get_current_sessions: () => sessions });
    renderWithStore(<SessionPanel />, {
      accounts: ACCOUNTS,
      launchedByProgram: new Set([1]),
      ...storeActions(),
    });
    await waitFor(() => expect(callsFor("get_current_sessions")).toHaveLength(1));
    expect(screen.queryByTestId("session-time-1")).not.toBeInTheDocument();

    sessions = [current(1, { sinceMs: Date.now() - 5 * MIN })];
    await act(async () => {
      emitTauriEvent("session-history-changed", { userIds: [1] });
    });
    expect(await screen.findByTestId("session-time-1")).toHaveTextContent("5m");
    expect(callsFor("get_current_sessions")).toHaveLength(2);
  });

  it("an account that is not in a game yet has no time and says so", async () => {
    renderRunning([], {
      clientHealth: new Map([[1, { pid: 1, logFound: true, drop: null, inGame: false }]]),
    });
    const row = within(screen.getByTestId("session-running-1"));
    expect(await row.findByText("Not in a game yet")).toBeInTheDocument();
    expect(row.queryByTestId("session-time-1")).not.toBeInTheDocument();
  });

  it("a dropped account shows why, not Playing nor the time", async () => {
    renderRunning([], {
      clientHealth: new Map([
        [
          1,
          {
            pid: 1,
            logFound: true,
            drop: { kind: "disconnected", reason: "connectionLost", code: 277, message: null, sinceMs: 0 },
            inGame: false,
          },
        ],
      ]),
    });
    const row = within(screen.getByTestId("session-running-1"));
    expect(await row.findByText("Disconnected: lost connection")).toBeInTheDocument();
    expect(row.queryByText("Playing")).not.toBeInTheDocument();
    expect(row.queryByText("Not in a game yet")).not.toBeInTheDocument();
  });

  it("an account being reconnected says so in its row", async () => {
    renderRunning([], {
      autoReconnect: [
        {
          userId: 1,
          phase: "launching",
          attempt: 1,
          maxAttempts: 5,
          nextAttemptAtMs: null,
          reason: null,
          error: null,
          drop: { kind: "crashed", reason: null, code: null, message: null, sinceMs: 0 },
        },
      ],
    });
    const row = within(screen.getByTestId("session-running-1"));
    expect(await row.findByText("Reconnecting")).toBeInTheDocument();
  });
});
