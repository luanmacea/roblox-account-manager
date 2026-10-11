import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/event", async () => (await import("../../test-utils/tauriMocks")).tauriEventMock());

import { ClicksTab, formatAfkElapsed } from "./ClicksTab";
import type { AfkStatus, StoreValue } from "../../store";
import { defaultSettings, makeAccount, setStore, storeRef } from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks } from "../../test-utils/tauriMocks";
import i18n from "../../i18n";
import { writeAfkPoint } from "../../afkClickPoint";

const ACCOUNTS = [
  makeAccount({ UserID: 11, Username: "alpha" }),
  makeAccount({ UserID: 22, Username: "bravo" }),
];

/** A lista fechada que o backend entrega (`get_afk_keys`). */
const KEYS = ["Space", "W", "A", "S", "D", "E", "F", "R", "Q", "1", "2", "3", "4", "5"];

function makeAfkAccount(overrides: Partial<AfkStatus["accounts"][number]> = {}) {
  return {
    userId: 11,
    lastSendAtMs: 1_000,
    nextSendAtMs: 601_000,
    sends: 0,
    lastError: null,
    lastErrorCode: null,
    ...overrides,
  };
}

function makeAfkStatus(overrides: Partial<AfkStatus> = {}): AfkStatus {
  return {
    active: false,
    startedAtMs: null,
    intervalSeconds: 600,
    key: "",
    mode: "key",
    clickX: 50,
    clickY: 50,
    accounts: [],
    ...overrides,
  };
}

function renderDialog(overrides: Partial<StoreValue> = {}) {
  const store = setStore({
    accounts: ACCOUNTS,
    launchedByProgram: new Set([11, 22]),
    afkKeys: KEYS,
    afkStatus: makeAfkStatus(),
    ...overrides,
  });
  render(<ClicksTab />);
  return { store };
}

beforeEach(() => {
  resetTauriMocks();
});

afterEach(cleanup);

/**
 * `SendInput` entrega na janela em **primeiro plano**, então o ciclo traz a
 * janela de cada conta para frente, uma depois da outra, e só devolve o foco
 * depois da última (`run_afk_cycle_blocking`: 150 ms de folga + 40 ms de tecla +
 * 250 ms entre contas, ~0,44 s por conta). Isso tira o foco de quem está usando
 * o PC, e é a primeira coisa que a tela tem de dizer — com os números de
 * verdade: "meio segundo e depois devolve" só valia com uma conta no modo.
 */
describe("ClicksTab — o preço do envio está na tela", () => {
  it("diz que o foco só volta depois da última conta do ciclo, e quanto tempo isso leva", () => {
    renderDialog();
    const aviso = screen.getByText(/takes the focus away from the window you are using/i);
    expect(aviso.textContent).toMatch(/about half a second each/i);
    expect(aviso.textContent).toMatch(/gives the focus back only after the last one/i);
    expect(aviso.textContent).toMatch(/about 4 seconds with 10 accounts/i);
  });

  it("avisa que, nesse meio-tempo, o que você digitar vai para a janela do Roblox", () => {
    renderDialog();
    expect(screen.getByText(/what you type goes to the Roblox window/i)).toBeInTheDocument();
    // A tecla do AFK só sai com a janela certa na frente (`afk_window_is_ready`):
    // "a tecla pode cair na janela errada" apontava o risco que o ciclo já elimina.
    expect(screen.queryByText(/the key can land in the wrong window/i)).not.toBeInTheDocument();
  });
});

/**
 * A lista de teclas é **fechada** e vem do backend (`get_afk_keys`): a tela não
 * pode oferecer tecla que o backend recusa, nem deixar digitar tecla arbitrária.
 */
describe("ClicksTab — só as teclas da lista", () => {
  it("oferece exatamente as teclas que o backend entregou", async () => {
    renderDialog();
    await userEvent.click(screen.getByLabelText("Key to send"));

    for (const key of KEYS) {
      expect(screen.getByRole("button", { name: key })).toBeInTheDocument();
    }
    // Teclas que fazem outra coisa no jogo não aparecem.
    for (const outside of ["Enter", "Tab", "Escape", "F4"]) {
      expect(screen.queryByRole("button", { name: outside })).not.toBeInTheDocument();
    }
  });

  it("não tem campo de texto para digitar uma tecla qualquer", () => {
    renderDialog();
    // A tecla se escolhe numa lista (botão que abre as opções); campo de texto
    // para tecla não existe, senão a lista fechada não seria fechada.
    expect(screen.queryByRole("textbox", { name: /key/i })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Key to send").tagName).toBe("BUTTON");
  });
});

/**
 * Sem tecla escolhida o modo não liga: inventar uma tecla padrão mexeria no
 * personagem sem o usuário ter pedido.
 */
describe("ClicksTab — o que impede o start", () => {
  it("não liga sem tecla escolhida", async () => {
    const { store } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));

    const start = screen.getByRole("button", { name: /Start AFK Mode/i });
    expect(start).toBeDisabled();
    await userEvent.click(start);
    expect(store.startAfkMode).not.toHaveBeenCalled();
  });

  it("não liga sem conta no modo, mesmo com tecla escolhida", async () => {
    const { store } = renderDialog();
    await userEvent.click(screen.getByLabelText("Key to send"));
    await userEvent.click(screen.getByRole("button", { name: "Space" }));

    expect(screen.getByRole("button", { name: /Start AFK Mode/i })).toBeDisabled();
    expect(store.startAfkMode).not.toHaveBeenCalled();
  });

  it("liga com uma tecla da lista e a conta escolhida", async () => {
    const { store } = renderDialog();
    await userEvent.click(screen.getByLabelText("Key to send"));
    await userEvent.click(screen.getByRole("button", { name: "Space" }));
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));
    await userEvent.click(screen.getByRole("button", { name: /Start AFK Mode/i }));

    expect(store.startAfkMode).toHaveBeenCalledWith({
      userIds: [11],
      intervalSeconds: 600,
      key: "Space",
      mode: "key",
      clickX: 50,
      clickY: 50,
    });
  });

  /** Conta que o usuário não marcou não pode entrar no modo por tabela. */
  it("manda só as contas marcadas", async () => {
    const { store } = renderDialog();
    await userEvent.click(screen.getByLabelText("Key to send"));
    await userEvent.click(screen.getByRole("button", { name: "W" }));
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[1].Username }));
    await userEvent.click(screen.getByRole("button", { name: /Start AFK Mode/i }));

    expect(store.startAfkMode).toHaveBeenCalledWith({
      userIds: [22],
      intervalSeconds: 600,
      key: "W",
      mode: "key",
      clickX: 50,
      clickY: 50,
    });
  });
});

describe("ClicksTab — sessão em andamento", () => {
  const RUNNING = makeAfkStatus({
    active: true,
    startedAtMs: 1_000,
    key: "Space",
    intervalSeconds: 600,
    accounts: [makeAfkAccount({ sends: 2 })],
  });

  it("mostra o botão de parar e não o de ligar", () => {
    renderDialog({ afkStatus: RUNNING });
    expect(screen.getByRole("button", { name: /Stop AFK Mode/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Start AFK Mode/i })).not.toBeInTheDocument();
  });

  it("parar é o comando de parar, e nada mais", async () => {
    const { store } = renderDialog({ afkStatus: RUNNING });
    await userEvent.click(screen.getByRole("button", { name: /Stop AFK Mode/i }));

    expect(store.stopAfkMode).toHaveBeenCalledTimes(1);
    // Parar o AFK mode não fecha nem reinicia cliente de conta nenhuma.
    expect(store.closeRobloxClients).not.toHaveBeenCalled();
    expect(store.killAllRobloxProcesses).not.toHaveBeenCalled();
    expect(store.restartRobloxClients).not.toHaveBeenCalled();
  });

  it("tirar uma conta do modo em andamento vai pelo set_afk_accounts", async () => {
    const { store } = renderDialog({ afkStatus: RUNNING });
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));

    expect(store.setAfkAccounts).toHaveBeenCalledWith([]);
    expect(store.closeRobloxClients).not.toHaveBeenCalled();
  });

  it("acrescentar uma conta ao modo em andamento mantém quem já estava", async () => {
    const { store } = renderDialog({ afkStatus: RUNNING });
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[1].Username }));

    expect(store.setAfkAccounts).toHaveBeenCalledWith([11, 22]);
  });

  it("com sessão ativa o intervalo e a tecla ficam travados", () => {
    renderDialog({ afkStatus: RUNNING });
    expect(screen.getByLabelText("Key to send")).toBeDisabled();
    expect(screen.getByLabelText("Send every: minutes")).toBeDisabled();
    expect(screen.getByLabelText("Send every: seconds")).toBeDisabled();
  });

  /** O intervalo mostrado é o da sessão, quebrado em minutos e segundos. */
  it("com sessão ativa os campos mostram o intervalo da sessão", () => {
    renderDialog({ afkStatus: makeAfkStatus({ ...RUNNING, intervalSeconds: 75 }) });
    expect(screen.getByLabelText("Send every: minutes")).toHaveValue("1");
    expect(screen.getByLabelText("Send every: seconds")).toHaveValue("15");
  });
});

/**
 * Pedido do dono (10/10/2026): "coloca um campo de segundos também, daí consigo
 * configurar tipo só 10 segundos depois que um ciclo acabar". O intervalo é
 * minutos + segundos, com piso de 5 s (`clamp_afk_interval_seconds`), e vai ao
 * backend em segundos.
 */
describe("ClicksTab — intervalo em minutos e segundos", () => {
  const iniWith = (afk: Record<string, string>) => ({
    ...defaultSettings(),
    Afk: { Key: "Space", ...afk },
  });

  async function start() {
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));
    await userEvent.click(screen.getByRole("button", { name: /Start AFK Mode/i }));
  }

  async function typeInto(label: string, value: string) {
    const field = screen.getByLabelText(label);
    await userEvent.clear(field);
    await userEvent.type(field, value);
    await userEvent.tab();
  }

  it("lê minutos e segundos do INI e manda o total em segundos", async () => {
    const { store } = renderDialog({ settings: iniWith({ IntervalMinutes: "1", IntervalSeconds: "30" }) });
    expect(screen.getByLabelText("Send every: minutes")).toHaveValue("1");
    expect(screen.getByLabelText("Send every: seconds")).toHaveValue("30");

    await start();
    expect(store.startAfkMode).toHaveBeenCalledWith(expect.objectContaining({ intervalSeconds: 90 }));
  });

  it("quem só tinha minutos no INI continua com o mesmo intervalo", async () => {
    const { store } = renderDialog({ settings: iniWith({ IntervalMinutes: "10" }) });
    expect(screen.getByLabelText("Send every: seconds")).toHaveValue("0");
    await start();
    expect(store.startAfkMode).toHaveBeenCalledWith(expect.objectContaining({ intervalSeconds: 600 }));
  });

  /** "0" no INI é zero minuto, não "valor ausente": senão 0 min 10 s virava 10 min 10 s. */
  it("zero minuto no INI é zero, e não o padrão de 10", async () => {
    const { store } = renderDialog({ settings: iniWith({ IntervalMinutes: "0", IntervalSeconds: "10" }) });
    expect(screen.getByLabelText("Send every: minutes")).toHaveValue("0");
    await start();
    expect(store.startAfkMode).toHaveBeenCalledWith(expect.objectContaining({ intervalSeconds: 10 }));
  });

  it("dá para ligar com só 10 segundos, e os dois campos ficam gravados", async () => {
    const { store } = renderDialog({ settings: iniWith({ IntervalMinutes: "10", IntervalSeconds: "0" }) });
    await typeInto("Send every: minutes", "0");
    await typeInto("Send every: seconds", "10");

    expect(invokeMock).toHaveBeenCalledWith("update_setting", {
      section: "Afk",
      key: "IntervalMinutes",
      value: "0",
    });
    expect(invokeMock).toHaveBeenCalledWith("update_setting", {
      section: "Afk",
      key: "IntervalSeconds",
      value: "10",
    });

    await start();
    expect(store.startAfkMode).toHaveBeenCalledWith(expect.objectContaining({ intervalSeconds: 10 }));
  });

  it("os segundos vão de 0 a 59 e os minutos de 0 a 120", async () => {
    renderDialog({ settings: iniWith({ IntervalMinutes: "10", IntervalSeconds: "0" }) });
    await typeInto("Send every: seconds", "75");
    expect(screen.getByLabelText("Send every: seconds")).toHaveValue("59");
    await typeInto("Send every: minutes", "500");
    expect(screen.getByLabelText("Send every: minutes")).toHaveValue("120");
  });

  it("abaixo de 5 segundos não liga, e a tela diz por quê", async () => {
    const { store } = renderDialog({ settings: iniWith({ IntervalMinutes: "0", IntervalSeconds: "3" }) });
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));

    expect(screen.getAllByText("At least 5 seconds.").length).toBeGreaterThan(0);
    const startButton = screen.getByRole("button", { name: /Start AFK Mode/i });
    expect(startButton).toBeDisabled();
    await userEvent.click(startButton);
    expect(store.startAfkMode).not.toHaveBeenCalled();

    await typeInto("Send every: seconds", "5");
    expect(screen.queryByText("At least 5 seconds.")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Start AFK Mode/i })).toBeEnabled();
  });

  it("a contagem de cada conta usa o intervalo em segundos", () => {
    vi.useFakeTimers({ now: 10_000_000 });
    try {
      const startedAt = Date.now();
      renderDialog({
        afkStatus: makeAfkStatus({
          active: true,
          startedAtMs: startedAt,
          key: "Space",
          intervalSeconds: 10,
          accounts: [makeAfkAccount({ userId: 11, lastSendAtMs: startedAt, nextSendAtMs: startedAt + 10_000 })],
        }),
      });
      expect(screen.getByRole("button", { name: ACCOUNTS[0].Username }).textContent).toContain("0:10");
    } finally {
      vi.useRealTimers();
    }
  });
});

/**
 * Intervalo e tecla só mudam com o modo parado, então "parar → mudar → ligar"
 * é o caminho normal. Parar não pode esquecer quem estava no modo: a seleção
 * voltava à de antes do start, e uma conta acrescentada com a sessão ligada
 * ficava de fora do próximo start sem aviso — e podia cair por inatividade.
 */
describe("ClicksTab — parar não esquece quem estava no modo", () => {
  const INI_WITH_KEY = { ...defaultSettings(), Afk: { IntervalMinutes: "10", Key: "Space" } };

  function runningWith(userIds: number[]): AfkStatus {
    return makeAfkStatus({
      active: true,
      startedAtMs: 1_000,
      key: "Space",
      accounts: userIds.map((userId) => makeAfkAccount({ userId })),
    });
  }

  /** O backend: `set_afk_accounts` devolve a sessão nova; `stop_afk_mode` a encerra. */
  function backendAnswers(store: StoreValue) {
    vi.mocked(store.setAfkAccounts).mockImplementation(async (userIds: number[]) => {
      storeRef.current = {
        ...storeRef.current,
        afkStatus: userIds.length > 0 ? runningWith(userIds) : makeAfkStatus(),
      };
    });
    vi.mocked(store.stopAfkMode).mockImplementation(async () => {
      storeRef.current = { ...storeRef.current, afkStatus: makeAfkStatus() };
    });
  }

  const row = (name: string) => screen.getByRole("button", { name });

  it("a conta que entrou com a sessão ligada continua marcada depois de parar, e religar a leva junto", async () => {
    const { store } = renderDialog({ afkStatus: runningWith([11]), settings: INI_WITH_KEY });
    backendAnswers(store);

    await userEvent.click(row(ACCOUNTS[1].Username));
    expect(store.setAfkAccounts).toHaveBeenCalledWith([11, 22]);
    await userEvent.click(screen.getByRole("button", { name: /Stop AFK Mode/i }));

    const start = await screen.findByRole("button", { name: /Start AFK Mode/i });
    expect(row(ACCOUNTS[0].Username)).toHaveAttribute("aria-pressed", "true");
    expect(row(ACCOUNTS[1].Username)).toHaveAttribute("aria-pressed", "true");

    await userEvent.click(start);
    expect(store.startAfkMode).toHaveBeenCalledWith({
      userIds: [11, 22],
      intervalSeconds: 600,
      key: "Space",
      mode: "key",
      clickX: 50,
      clickY: 50,
    });
  });

  it("com a tela aberta numa sessão que já rodava, parar deixa marcadas as contas dela", async () => {
    const { store } = renderDialog({ afkStatus: runningWith([11, 22]), settings: INI_WITH_KEY });
    backendAnswers(store);

    await userEvent.click(screen.getByRole("button", { name: /Stop AFK Mode/i }));

    expect(await screen.findByRole("button", { name: /Start AFK Mode/i })).toBeEnabled();
    expect(row(ACCOUNTS[0].Username)).toHaveAttribute("aria-pressed", "true");
    expect(row(ACCOUNTS[1].Username)).toHaveAttribute("aria-pressed", "true");
  });

  it("desmarcar a última conta desliga o modo, e ela fica desmarcada — foi o que o usuário pediu", async () => {
    const { store } = renderDialog({ afkStatus: runningWith([11]), settings: INI_WITH_KEY });
    backendAnswers(store);

    await userEvent.click(row(ACCOUNTS[0].Username));
    expect(store.setAfkAccounts).toHaveBeenCalledWith([]);

    expect(await screen.findByRole("button", { name: /Start AFK Mode/i })).toBeDisabled();
    expect(row(ACCOUNTS[0].Username)).toHaveAttribute("aria-pressed", "false");
  });
});

/**
 * "Isso está funcionando?" é a pergunta de quem acabou de ligar o modo com
 * intervalo de 10 minutos. Duas respostas na tela: o tempo decorrido da sessão e
 * o botão de enviar agora.
 */
describe("ClicksTab — dá para saber que está funcionando", () => {
  const RUNNING = makeAfkStatus({
    active: true,
    startedAtMs: 1_000,
    key: "Space",
    accounts: [makeAfkAccount()],
  });

  it("formata o tempo decorrido em minutos e horas", () => {
    expect(formatAfkElapsed(null, 10_000)).toBe("--");
    expect(formatAfkElapsed(1_000, 1_000)).toBe("<1m");
    expect(formatAfkElapsed(0, 59_999)).toBe("<1m");
    expect(formatAfkElapsed(0, 60_000)).toBe("1m");
    expect(formatAfkElapsed(0, 12 * 60_000)).toBe("12m");
    expect(formatAfkElapsed(0, 65 * 60_000)).toBe("1h 5m");
    // Relógio para trás não vira tempo negativo.
    expect(formatAfkElapsed(10_000, 0)).toBe("<1m");
  });

  it("mostra há quanto tempo a sessão está rodando", () => {
    vi.setSystemTime(new Date(1_000 + 12 * 60_000));
    try {
      renderDialog({ afkStatus: RUNNING });
      expect(screen.getByText(/Running for 12m/)).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("enviar agora manda a tecla escolhida para as contas do modo", async () => {
    const { store } = renderDialog({ afkStatus: RUNNING });
    await userEvent.click(screen.getByRole("button", { name: /Send the key now/i }));

    expect(store.afkTriggerNow).toHaveBeenCalledWith([11]);
  });

  it("não deixa enviar agora sem tecla escolhida", async () => {
    const { store } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));

    const sendNow = screen.getByRole("button", { name: /Send the key now/i });
    expect(sendNow).toBeDisabled();
    await userEvent.click(sendNow);
    expect(store.afkTriggerNow).not.toHaveBeenCalled();
  });

  /** O som explica o piscar de foco; ligado por quem quer, nunca por padrão. */
  it("o aviso sonoro nasce desligado e grava a escolha no INI", async () => {
    renderDialog();
    const toggle = screen.getByRole("button", { name: "Beep when a cycle finishes" });
    expect(toggle).toHaveAttribute("aria-pressed", "false");

    await userEvent.click(toggle);
    expect(invokeMock).toHaveBeenCalledWith("update_setting", {
      section: "Afk",
      key: "BeepOnCycle",
      value: "true",
    });
  });
});

/**
 * Ideia 25: com um vídeo ou outro jogo em tela cheia na frente, o ciclo espera
 * em vez de roubar o foco (`afk_fullscreen_gate`). Ligado por padrão — protege
 * quem está vendo algo — e a tela diz quando está esperando.
 */
describe("ClicksTab — espera a tela cheia sair", () => {
  it("nasce ligado, explica numa frase e grava a escolha no INI", async () => {
    renderDialog();
    const toggle = screen.getByRole("button", { name: "Wait while a fullscreen window is in front" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/a video or another game in fullscreen/i)).toBeInTheDocument();

    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    expect(invokeMock).toHaveBeenCalledWith("update_setting", {
      section: "Afk",
      key: "WaitForFullscreen",
      value: "false",
    });
  });

  it("lê a escolha desligada do INI", () => {
    const settings = defaultSettings();
    settings.Afk = { ...(settings.Afk ?? {}), WaitForFullscreen: "false" };
    renderDialog({ settings });
    expect(
      screen.getByRole("button", { name: "Wait while a fullscreen window is in front" })
    ).toHaveAttribute("aria-pressed", "false");
  });

  it("diz que está esperando enquanto a tela cheia segura o ciclo", () => {
    renderDialog({
      afkStatus: makeAfkStatus({
        active: true,
        startedAtMs: Date.now(),
        key: "Space",
        accounts: [makeAfkAccount()],
        waitingFullscreen: true,
      }),
    });
    expect(screen.getByText("Waiting: a fullscreen window is in front")).toBeInTheDocument();
  });

  it("não diz nada disso com o ciclo andando normalmente", () => {
    renderDialog({
      afkStatus: makeAfkStatus({
        active: true,
        startedAtMs: Date.now(),
        key: "Space",
        accounts: [makeAfkAccount()],
        waitingFullscreen: false,
      }),
    });
    expect(screen.queryByText("Waiting: a fullscreen window is in front")).not.toBeInTheDocument();
  });
});


/**
 * O `SendInput` só alcança a janela em primeiro plano, e o Windows **recusa**
 * trazer janela para frente a pedido de processo que está em segundo plano — que
 * é o caso normal do AFK mode. Quando isso acontece o ciclo não manda nada, e a
 * tela tem de dizer as duas coisas: que não mandou, e por quê.
 */
describe("ClicksTab — quando o Windows não deixa a janela vir para frente", () => {
  const DENIED = makeAfkStatus({
    active: true,
    startedAtMs: 1_000,
    key: "Space",
    accounts: [
      makeAfkAccount({
        userId: 11,
        lastError: "Windows did not bring this account's Roblox window to the front, so nothing was sent",
        lastErrorCode: "focusDenied",
      }),
    ],
  });

  it("avisa, na configuração, que nada é enviado nesse caso", () => {
    renderDialog();
    expect(screen.getByText(/nothing is sent/i)).toBeInTheDocument();
  });

  it("diz qual conta foi pulada e por quê", () => {
    renderDialog({ afkStatus: DENIED });
    const aviso = screen.getByText(/did not let this account's window come to the front/i);
    expect(aviso).toBeInTheDocument();
    expect(aviso.textContent).toContain(ACCOUNTS[0].Username);
  });

  it("marca a linha da conta como não enviada", () => {
    renderDialog({ afkStatus: DENIED });
    expect(screen.getByText("not sent")).toBeInTheDocument();
  });

  it("explica que o envio manual passa porque o app acabou de receber o clique", () => {
    renderDialog({ afkStatus: DENIED });
    expect(screen.getByRole("button", { name: /Send the key now/i })).toBeEnabled();
    const dica = screen.getByText(/works because you just clicked/i);
    expect(dica.textContent).toMatch(/Send the key now/);
  });

  it("uma conta sem cliente aberto aparece com o motivo dela, não com o do foco", () => {
    renderDialog({
      afkStatus: makeAfkStatus({
        active: true,
        startedAtMs: 1_000,
        key: "Space",
        accounts: [
          makeAfkAccount({
            lastError: "No Roblox window for this account",
            lastErrorCode: "noWindow",
          }),
        ],
      }),
    });
    expect(screen.getByText(/has no Roblox client open/i)).toBeInTheDocument();
    expect(
      screen.queryByText(/did not let this account's window come to the front/i)
    ).not.toBeInTheDocument();
  });
});

/**
 * Envio manual é uma ação do usuário, mas continua valendo a regra de nunca
 * mexer em cliente de conta que não está no modo: sem sessão, não há a quem
 * enviar.
 */
describe("ClicksTab — envio manual exige sessão", () => {
  it("com o modo desligado, enviar agora fica indisponível mesmo com tecla e conta escolhidas", async () => {
    const { store } = renderDialog();
    await userEvent.click(screen.getByLabelText("Key to send"));
    await userEvent.click(screen.getByRole("button", { name: "Space" }));
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));

    const sendNow = screen.getByRole("button", { name: /Send the key now/i });
    expect(sendNow).toBeDisabled();
    await userEvent.click(sendNow);
    expect(store.afkTriggerNow).not.toHaveBeenCalled();
  });
});

/**
 * O AFK mode só alcança cliente que **este app** abriu (é o tracker que liga
 * conta a PID). Conta sem cliente aberto não tem o que receber tecla.
 */
describe("ClicksTab — contas que podem entrar no modo", () => {
  it("lista as contas com cliente aberto", () => {
    renderDialog();
    expect(screen.getByRole("button", { name: ACCOUNTS[0].Username })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: ACCOUNTS[1].Username })).toBeInTheDocument();
  });

  it("não lista conta sem cliente aberto", () => {
    renderDialog({ launchedByProgram: new Set([11]) });
    expect(screen.getByRole("button", { name: ACCOUNTS[0].Username })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: ACCOUNTS[1].Username })).not.toBeInTheDocument();
  });

  it("explica o vazio em vez de mostrar uma lista vazia", () => {
    renderDialog({ launchedByProgram: new Set<number>() });
    expect(screen.getByText(/Open an account first/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Start AFK Mode/i })).toBeDisabled();
  });
});

/**
 * Os detalhes baixos do checkup: nenhum quebra o modo, mas cada um faz a tela
 * dizer uma coisa que não é verdade — relógio acima do intervalo, "Enviando"
 * com nada saindo, "1 contas", o modo desligando calado, "Chave" onde é tecla.
 */
describe("ClicksTab — a tela diz a coisa certa", () => {
  const ONE_RUNNING = makeAfkStatus({
    active: true,
    startedAtMs: 1_000,
    key: "Space",
    accounts: [makeAfkAccount({ userId: 11 })],
  });

  /**
   * O prazo chega do backend (agora + intervalo) e era comparado com o relógio
   * da tela do último tique de 1 s — até 1 s atrás. A contagem nascia em
   * "10:01", mais que o intervalo, o que não existe; e de novo depois de cada
   * envio manual.
   */
  it("a contagem nunca começa maior que o intervalo", () => {
    vi.useFakeTimers({ now: 10_000_000 });
    try {
      setStore({ accounts: ACCOUNTS, launchedByProgram: new Set([11, 22]), afkKeys: KEYS, afkStatus: makeAfkStatus() });
      const view = render(<ClicksTab />);

      // 900 ms depois do último tique da tela, o start volta com o prazo do
      // primeiro envio.
      act(() => {
        vi.advanceTimersByTime(900);
      });
      const startedAt = Date.now();
      storeRef.current = {
        ...storeRef.current,
        afkStatus: makeAfkStatus({
          active: true,
          startedAtMs: startedAt,
          key: "Space",
          intervalSeconds: 600,
          accounts: [
            makeAfkAccount({ userId: 11, lastSendAtMs: startedAt, nextSendAtMs: startedAt + 600_000 }),
          ],
        }),
      };
      view.rerender(<ClicksTab />);

      const linha = screen.getByRole("button", { name: ACCOUNTS[0].Username });
      expect(linha.textContent).toContain("10:00");
      expect(linha.textContent).not.toContain("10:01");
    } finally {
      vi.useRealTimers();
    }
  });

  /** "Enviando" com a sessão ligada mentia entre um ciclo e outro, e com o foco negado em todas. */
  it("com a sessão ligada a pílula diz o estado, 'On', e não 'Sending'", () => {
    renderDialog({ afkStatus: ONE_RUNNING });
    expect(screen.getByText("On")).toBeInTheDocument();
    expect(screen.queryByText("Sending")).not.toBeInTheDocument();
  });

  it("desmarcar a última conta avisa que o modo desligou", async () => {
    const { store } = renderDialog({ afkStatus: ONE_RUNNING });
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));

    expect(store.setAfkAccounts).toHaveBeenCalledWith([]);
    expect(store.addToast).toHaveBeenCalledWith("AFK mode off: no account is left in it");
  });

  it("tirar uma conta que não é a última não avisa nada", async () => {
    const { store } = renderDialog({
      afkStatus: makeAfkStatus({
        active: true,
        startedAtMs: 1_000,
        key: "Space",
        accounts: [makeAfkAccount({ userId: 11 }), makeAfkAccount({ userId: 22 })],
      }),
    });
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));

    expect(store.setAfkAccounts).toHaveBeenCalledWith([22]);
    expect(store.addToast).not.toHaveBeenCalled();
  });

  it("enviar para uma conta só diz 'account', no singular", async () => {
    const { store } = renderDialog({ afkStatus: ONE_RUNNING });
    vi.mocked(store.afkTriggerNow).mockResolvedValue(1);
    await userEvent.click(screen.getByRole("button", { name: /Send the key now/i }));

    expect(store.addToast).toHaveBeenCalledWith("Sent Space to 1 account");
  });

  it("enviar para duas contas continua no plural", async () => {
    const { store } = renderDialog({
      afkStatus: makeAfkStatus({
        active: true,
        startedAtMs: 1_000,
        key: "Space",
        accounts: [makeAfkAccount({ userId: 11 }), makeAfkAccount({ userId: 22 })],
      }),
    });
    vi.mocked(store.afkTriggerNow).mockResolvedValue(2);
    await userEvent.click(screen.getByRole("button", { name: /Send the key now/i }));

    expect(store.addToast).toHaveBeenCalledWith("Sent Space to 2 accounts");
  });

  describe("em português", () => {
    beforeEach(async () => {
      await i18n.changeLanguage("pt");
    });
    afterEach(async () => {
      await i18n.changeLanguage("en");
    });

    /**
     * "Key" → "Chave" está certo na tela de campos da conta, que divide a mesma
     * chave do catálogo; aqui é tecla. E o campo do intervalo tinha nome
     * acessível em inglês, porque ia cru para o `NumericInput`.
     */
    it("o rótulo é 'Tecla a enviar', não 'Chave', e o campo do intervalo tem nome em português", () => {
      renderDialog();
      expect(screen.getByText("Tecla a enviar")).toBeInTheDocument();
      expect(screen.queryByText("Chave")).not.toBeInTheDocument();
      expect(screen.getByLabelText("Tecla a enviar").tagName).toBe("BUTTON");
      expect(screen.getByLabelText("Enviar a cada: minutos").tagName).toBe("INPUT");
      expect(screen.getByLabelText("Enviar a cada: segundos").tagName).toBe("INPUT");
    });

    it("o aviso do piso de 5 segundos sai em português", async () => {
      renderDialog({
        settings: { ...defaultSettings(), Afk: { IntervalMinutes: "0", IntervalSeconds: "2", Key: "Space" } },
      });
      expect(screen.getAllByText("No mínimo 5 segundos.").length).toBeGreaterThan(0);
    });

    it("a pílula diz 'Ligado' com a sessão ligada", () => {
      renderDialog({ afkStatus: ONE_RUNNING });
      expect(screen.getByText("Ligado")).toBeInTheDocument();
      expect(screen.queryByText("Enviando")).not.toBeInTheDocument();
    });
  });
});

/**
 * Modo clique: em vez de tecla, um clique esquerdo num ponto relativo (%) da
 * janela de cada conta — para quem quer o personagem parado. Ponto padrão para
 * todas, ponto próprio por conta, e o Marcar lê onde o mouse está depois de 3 s.
 */
describe("ClicksTab — modo clique", () => {
  const CLICK_INI = {
    ...defaultSettings(),
    Afk: { IntervalMinutes: "10", Key: "", Mode: "click", ClickX: "50", ClickY: "50" },
  };

  function clickRunning(overrides: Partial<AfkStatus> = {}): AfkStatus {
    return makeAfkStatus({
      active: true,
      startedAtMs: 1_000,
      mode: "click",
      accounts: [makeAfkAccount({ userId: 11 })],
      ...overrides,
    });
  }

  it("trocar para clique grava o modo no INI e esconde a tecla", async () => {
    renderDialog();
    await userEvent.click(screen.getByLabelText("What to send"));
    await userEvent.click(screen.getByRole("button", { name: "Mouse click" }));

    expect(invokeMock).toHaveBeenCalledWith("update_setting", {
      section: "Afk",
      key: "Mode",
      value: "click",
    });
    expect(screen.queryByLabelText("Key to send")).not.toBeInTheDocument();
    expect(screen.getByText("50% × 50%")).toBeInTheDocument();
  });

  /** O modo clique não usa tecla: exigir uma seria bloquear quem não quer mexer o personagem. */
  it("liga sem tecla, com o modo e o ponto padrão", async () => {
    const { store } = renderDialog({ settings: CLICK_INI });
    await userEvent.click(screen.getByRole("button", { name: ACCOUNTS[0].Username }));
    const start = screen.getByRole("button", { name: /Start AFK Mode/i });
    expect(start).toBeEnabled();
    await userEvent.click(start);

    expect(store.startAfkMode).toHaveBeenCalledWith({
      userIds: [11],
      intervalSeconds: 600,
      key: "",
      mode: "click",
      clickX: 50,
      clickY: 50,
    });
  });

  it("com sessão de clique ligada, o modo vem da sessão e fica travado", () => {
    renderDialog({ afkStatus: clickRunning({ clickX: 30, clickY: 40 }) });
    expect(screen.getByLabelText("What to send")).toBeDisabled();
    expect(screen.getByText("30% × 40%")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Mark the click point for all accounts" })).toBeDisabled();
  });

  it("o Marcar conta 3 segundos, lê o ponto e o grava como padrão", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { store } = renderDialog({ settings: CLICK_INI });
      vi.mocked(store.captureAfkPoint).mockResolvedValue({ userId: 22, xPct: 52.5, yPct: 71 });
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

      await user.click(screen.getByRole("button", { name: "Mark the click point for all accounts" }));
      expect(screen.getByText(/Put the mouse over the spot in a game window: 3/)).toBeInTheDocument();
      expect(store.captureAfkPoint).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3_100);
      });

      expect(store.captureAfkPoint).toHaveBeenCalledTimes(1);
      expect(screen.getByText("52.5% × 71%")).toBeInTheDocument();
      expect(invokeMock).toHaveBeenCalledWith("update_setting", { section: "Afk", key: "ClickX", value: "52.5" });
      expect(invokeMock).toHaveBeenCalledWith("update_setting", { section: "Afk", key: "ClickY", value: "71" });
      expect(store.addToast).toHaveBeenCalledWith("Point marked on bravo's window");
    } finally {
      vi.useRealTimers();
    }
  });

  it("Marcar fora de uma janela de conta diz por quê e não muda o ponto", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { store } = renderDialog({ settings: CLICK_INI });
      vi.mocked(store.captureAfkPoint).mockRejectedValue("notAnAccountWindow");
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

      await user.click(screen.getByRole("button", { name: "Mark the click point for all accounts" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3_100);
      });

      expect(store.addToast).toHaveBeenCalledWith(
        "That window is not a Roblox client this app opened",
        "error"
      );
      expect(screen.getByText("50% × 50%")).toBeInTheDocument();
      expect(invokeMock).not.toHaveBeenCalledWith("update_setting", expect.objectContaining({ key: "ClickX" }));
    } finally {
      vi.useRealTimers();
    }
  });

  it("uma conta pode ter ponto próprio, gravado nos campos dela", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { store } = renderDialog({ settings: CLICK_INI, afkStatus: clickRunning() });
      vi.mocked(store.captureAfkPoint).mockResolvedValue({ userId: 11, xPct: 10, yPct: 90 });
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

      expect(screen.getByText("Default point")).toBeInTheDocument();
      // O ponto de cada conta é lido a cada ciclo: muda com a sessão ligada.
      await user.click(screen.getByRole("button", { name: "Mark the click point for alpha" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3_100);
      });

      expect(store.updateAccount).toHaveBeenCalledWith(
        expect.objectContaining({ UserID: 11, Fields: { AfkClickX: "10", AfkClickY: "90" } })
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("'usar o padrão' apaga o ponto próprio da conta", async () => {
    const own = makeAccount({ UserID: 11, Username: "alpha", Fields: { AfkClickX: "10", AfkClickY: "90", Note: "x" } });
    const { store } = renderDialog({
      settings: CLICK_INI,
      accounts: [own, ACCOUNTS[1]],
      afkStatus: clickRunning(),
    });

    expect(screen.getByText("10% × 90%")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Use the default point for alpha" }));

    expect(store.updateAccount).toHaveBeenCalledWith(
      expect.objectContaining({ UserID: 11, Fields: { Note: "x" } })
    );
  });

  it("clicar agora vai para as contas do modo, e o aviso fala de clique", async () => {
    const { store } = renderDialog({ afkStatus: clickRunning() });
    vi.mocked(store.afkTriggerNow).mockResolvedValue(1);
    await userEvent.click(screen.getByRole("button", { name: /Click now/i }));

    expect(store.afkTriggerNow).toHaveBeenCalledWith([11]);
    expect(store.addToast).toHaveBeenCalledWith("Clicked on 1 account");
  });

  it("avisa que o cursor pula até o ponto e volta", () => {
    renderDialog({ settings: CLICK_INI });
    const aviso = screen.getByText(/the cursor also jumps to the point and comes back/i);
    // A receita do clique (mover, tremer, clique de foco, clique) leva ~1,2 s por
    // conta: `a_click_cycle_keeps_the_focus_about_a_second_per_account`.
    expect(aviso.textContent).toMatch(/about 1\.2 seconds per account/i);
  });

  it("explica o clique recusado de uma conta", () => {
    renderDialog({
      afkStatus: clickRunning({
        accounts: [
          makeAfkAccount({
            userId: 11,
            lastError: "Windows refused the synthetic click",
            lastErrorCode: "clickRefused",
          }),
        ],
      }),
    });
    expect(screen.getByText("alpha: Windows refused the click.")).toBeInTheDocument();
  });
});

/**
 * Aberto pelo "Em jogo" do Painel de Sessão, a aba já chega com as contas que
 * estão em jogo marcadas — com o modo parado. Com sessão ligada, quem manda é a
 * sessão.
 */
describe("ClicksTab — contas de quem abriu", () => {
  it("chega com as contas do Em jogo marcadas", () => {
    setStore({
      accounts: ACCOUNTS,
      launchedByProgram: new Set([11, 22]),
      afkKeys: KEYS,
      afkStatus: makeAfkStatus(),
    });
    render(<ClicksTab targetUserIds={[22]} />);

    expect(screen.getByRole("button", { name: ACCOUNTS[0].Username })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: ACCOUNTS[1].Username })).toHaveAttribute("aria-pressed", "true");
  });

  it("com sessão ligada, as contas da sessão vencem as de quem abriu", () => {
    setStore({
      accounts: ACCOUNTS,
      launchedByProgram: new Set([11, 22]),
      afkKeys: KEYS,
      afkStatus: makeAfkStatus({
        active: true,
        startedAtMs: 1_000,
        key: "Space",
        accounts: [makeAfkAccount({ userId: 11 })],
      }),
    });
    render(<ClicksTab targetUserIds={[22]} />);

    expect(screen.getByRole("button", { name: ACCOUNTS[0].Username })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: ACCOUNTS[1].Username })).toHaveAttribute("aria-pressed", "false");
  });

  it("a barra de estado diz se o modo está ligado", () => {
    setStore({
      accounts: ACCOUNTS,
      launchedByProgram: new Set([11, 22]),
      afkKeys: KEYS,
      afkStatus: makeAfkStatus(),
    });
    render(<ClicksTab />);
    expect(screen.getByTestId("clicks-status")).toHaveTextContent("AFK clicks are off");
  });
});

/**
 * Com "Names hidden" na toolbar, o modo AFK mostrava os nomes reais na lista de
 * contas, no rótulo do "Use default" e nos avisos — quem grava a tela com o modo
 * ligado espera não ver nome nenhum.
 */
describe("ClicksTab — nomes ocultos", () => {
  const SECRET = [
    makeAccount({
      UserID: 11,
      Username: "secretalpha",
      Alias: "AliasAlpha",
      Fields: writeAfkPoint({}, { x: 20, y: 30 }),
    }),
    makeAccount({ UserID: 22, Username: "secretbravo" }),
  ];
  const HIDDEN = { hideUsernames: true, hiddenNameLetters: 0, showAvatarsWhenHidden: false };
  const CLICK_INI = {
    ...defaultSettings(),
    Afk: { IntervalMinutes: "10", Key: "", Mode: "click", ClickX: "50", ClickY: "50" },
  };

  function expectNoRealName() {
    const html = document.body.innerHTML;
    for (const name of ["secretalpha", "AliasAlpha", "secretbravo"]) expect(html).not.toContain(name);
  }

  it("a lista de contas, o rótulo do ponto próprio e os avisos não mostram o nome", () => {
    renderDialog({
      ...HIDDEN,
      accounts: SECRET,
      settings: CLICK_INI,
      afkStatus: makeAfkStatus({
        active: true,
        startedAtMs: 1_000,
        mode: "click",
        accounts: [
          makeAfkAccount({ userId: 11, lastError: "x", lastErrorCode: "noWindow" }),
          makeAfkAccount({ userId: 22, lastError: "x", lastErrorCode: "clickRefused" }),
        ],
      }),
    });
    expect(screen.getAllByRole("button", { name: "************" }).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Use the default point for ************" })).toBeInTheDocument();
    expect(screen.getByText(/has no Roblox client open/i)).toBeInTheDocument();
    expectNoRealName();
  });

  it("o toast do Marcar não diz de quem é a janela", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { store } = renderDialog({ ...HIDDEN, accounts: SECRET, settings: CLICK_INI });
      vi.mocked(store.captureAfkPoint).mockResolvedValue({ userId: 22, xPct: 52.5, yPct: 71 });
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      await user.click(screen.getByRole("button", { name: "Mark the click point for all accounts" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3_100);
      });
      expect(store.addToast).toHaveBeenCalledWith("Point marked on ************'s window");
    } finally {
      vi.useRealTimers();
    }
  });
});
