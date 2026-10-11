import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/event", async () => (await import("../../test-utils/tauriMocks")).tauriEventMock());
vi.mock("../../hooks/usePrompt", async () => (await import("../../test-utils/promptMocks")).promptModuleMock());

import { RecordingsTab } from "./RecordingsTab";
import { ClicksTab } from "./ClicksTab";
import type { StoreValue } from "../../store";
import { defaultSettings, makeAccount, setStore } from "../../test-utils/renderWithStore";
import { emitTauriEvent, invokeMock, resetTauriMocks, setInvokeHandler } from "../../test-utils/tauriMocks";
import { confirmMock, promptAnswers, resetPromptMocks } from "../../test-utils/promptMocks";
import type { RecordingsPayload } from "../../recordings";

const ACCOUNTS = [
  makeAccount({ UserID: 11, Username: "alpha" }),
  makeAccount({ UserID: 22, Username: "bravo" }),
];

const KEYS = ["Space", "W", "A", "S", "D", "E"];

function library(overrides: Partial<RecordingsPayload> = {}): RecordingsPayload {
  return {
    recordings: [
      {
        id: "rec-1",
        name: "Walk forward",
        steps: [
          { type: "key", key: "W", holdMs: 800 },
          { type: "wait", ms: 200 },
          { type: "click", xPct: 37.5, yPct: 62.5 },
        ],
        createdAt: 1,
        updatedAt: 1,
      },
      { id: "rec-2", name: "Jump", steps: [{ type: "key", key: "Space", holdMs: 40 }], createdAt: 2, updatedAt: 2 },
    ],
    defaultId: null,
    accountIds: {},
    keys: KEYS,
    ...overrides,
  };
}

let payload: RecordingsPayload;
let playing = false;

function route(extra: Record<string, (args: Record<string, unknown> | undefined) => unknown> = {}) {
  setInvokeHandler((cmd, args) => {
    if (cmd in extra) return extra[cmd](args);
    switch (cmd) {
      case "get_recordings":
        return payload;
      case "get_recording_playback":
        return { active: playing };
      case "save_recording": {
        const rec = (args as { recording: { id: string; name: string } }).recording;
        return { ...rec, id: rec.id || "rec-new", createdAt: 5, updatedAt: 5 };
      }
      default:
        return undefined;
    }
  });
}

function renderTab(overrides: Partial<StoreValue> = {}) {
  const settings = defaultSettings();
  (settings as Record<string, Record<string, string>>).Recordings = {
    AfterReconnect: "false",
    AfterReconnectDelaySeconds: "45",
  };
  const store = setStore({
    accounts: ACCOUNTS,
    launchedByProgram: new Set([11, 22]),
    afkKeys: KEYS,
    settings,
    ...overrides,
  });
  render(<RecordingsTab />);
  return { store };
}

function calls(cmd: string) {
  return invokeMock.mock.calls.filter((c) => c[0] === cmd);
}

beforeEach(() => {
  resetTauriMocks();
  resetPromptMocks();
  payload = library();
  playing = false;
  route();
});

afterEach(cleanup);

describe("RecordingsTab — biblioteca e editor", () => {
  it("lista as gravações e abre a primeira no editor, com os passos", async () => {
    renderTab();
    expect(await screen.findByLabelText("Recording name")).toHaveValue("Walk forward");
    expect(screen.getByRole("button", { name: /^Walk forward/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("recording-step-1")).toHaveTextContent("W");
    expect(screen.getByTestId("recording-step-3")).toHaveTextContent("37.5% × 62.5%");
    // 800 ms segurada + 200 ms de espera + o clique estimado.
    expect(screen.getByText(/3 steps · about 1.8 s per window/)).toBeInTheDocument();
  });

  it("acrescentar um passo deixa a gravação com mudança, e salvar manda os passos", async () => {
    const { store } = renderTab();
    await screen.findByLabelText("Recording name");
    const save = screen.getByRole("button", { name: "Save recording" });
    expect(save).toBeDisabled();

    await userEvent.click(screen.getByRole("button", { name: "Wait" }));
    expect(screen.getByTestId("recording-step-4")).toBeInTheDocument();
    expect(save).toBeEnabled();
    await userEvent.click(save);

    const sent = calls("save_recording")[0][1] as { recording: { id: string; steps: unknown[] } };
    expect(sent.recording.id).toBe("rec-1");
    expect(sent.recording.steps).toHaveLength(4);
    expect(sent.recording.steps[3]).toEqual({ type: "wait", ms: 500 });
    expect(store.addToast).toHaveBeenCalledWith("Recording saved");
  });

  it("sem nome não salva, e diz por quê", async () => {
    renderTab();
    const name = await screen.findByLabelText("Recording name");
    await userEvent.clear(name);
    expect(screen.getByText("Give the recording a name.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save recording" })).toBeDisabled();
  });

  it("move e remove passos", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");
    await userEvent.click(screen.getByRole("button", { name: "Move step 3 up" }));
    expect(screen.getByTestId("recording-step-2")).toHaveTextContent("37.5% × 62.5%");
    await userEvent.click(screen.getByRole("button", { name: "Remove step 1" }));
    expect(screen.queryByTestId("recording-step-3")).not.toBeInTheDocument();
  });

  it("nova gravação começa vazia e é criada ao salvar", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");
    await userEvent.click(screen.getByRole("button", { name: "New recording" }));
    expect(screen.getByLabelText("Recording name")).toHaveValue("New recording");
    expect(screen.getByText(/No steps yet/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Key press" }));
    await userEvent.click(screen.getByRole("button", { name: "Save recording" }));
    const sent = calls("save_recording")[0][1] as { recording: { id: string; steps: unknown[] } };
    expect(sent.recording.id).toBe("");
    expect(sent.recording.steps).toEqual([{ type: "key", key: "Space", holdMs: 40 }]);
  });

  it("trocar de gravação com mudança pede confirmação", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");
    await userEvent.click(screen.getByRole("button", { name: "Wait" }));
    promptAnswers.confirm = false;
    await userEvent.click(screen.getByRole("button", { name: /^Jump/ }));
    expect(confirmMock).toHaveBeenCalled();
    expect(screen.getByLabelText("Recording name")).toHaveValue("Walk forward");
  });

  it("renomear, duplicar e apagar passam pelo backend; apagar pede confirmação", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");

    promptAnswers.prompt = "Walk back";
    await userEvent.click(screen.getByRole("button", { name: "Rename Walk forward" }));
    expect((calls("save_recording")[0][1] as { recording: { name: string } }).recording.name).toBe("Walk back");

    await userEvent.click(screen.getByRole("button", { name: "Duplicate Jump" }));
    expect(calls("duplicate_recording")[0][1]).toEqual({ id: "rec-2", name: "Jump (copy)" });

    promptAnswers.confirm = false;
    await userEvent.click(screen.getByRole("button", { name: "Delete Jump" }));
    expect(calls("delete_recording")).toHaveLength(0);
    promptAnswers.confirm = true;
    await userEvent.click(screen.getByRole("button", { name: "Delete Jump" }));
    expect(calls("delete_recording")[0][1]).toEqual({ id: "rec-2" });
  });

  it("relê a biblioteca quando o backend avisa que mudou", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");
    payload = library({ recordings: [...library().recordings, { id: "rec-3", name: "Spin", steps: [], createdAt: 3, updatedAt: 3 }] });
    await act(async () => emitTauriEvent("recordings-changed", null));
    expect(await screen.findByRole("button", { name: /^Spin/ })).toBeInTheDocument();
  });
});

describe("RecordingsTab — qual gravação toca", () => {
  it("escolhe a de todas as contas e a própria de uma conta", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");
    await userEvent.click(screen.getByLabelText("Recording for all accounts"));
    await userEvent.click(screen.getByRole("button", { name: "Jump" }));
    expect(calls("set_default_recording")[0][1]).toEqual({ id: "rec-2" });

    await userEvent.click(screen.getByLabelText("Recording for alpha"));
    await userEvent.click(screen.getByRole("button", { name: "Walk forward" }));
    expect(calls("set_account_recording")[0][1]).toEqual({ userId: 11, id: "rec-1" });
  });

  it("voltar a conta para a de todas manda id nulo", async () => {
    payload = library({ accountIds: { "22": "rec-2" } });
    renderTab();
    await screen.findByLabelText("Recording name");
    await userEvent.click(screen.getByLabelText("Recording for bravo"));
    await userEvent.click(screen.getByRole("button", { name: "Same as all accounts" }));
    expect(calls("set_account_recording")[0][1]).toEqual({ userId: 22, id: null });
  });

  it("tocar depois da reconexão grava no INI, com o tempo no jogo", async () => {
    const { store } = renderTab();
    await screen.findByLabelText("Recording name");
    expect(screen.getByLabelText("Seconds in the game before playing")).toHaveValue("45");
    await userEvent.click(screen.getByRole("button", { name: "Play after an automatic reconnect" }));
    expect(store.updateSetting).toHaveBeenCalledWith("Recordings", "AfterReconnect", "true");
  });
});

describe("RecordingsTab — tocar agora e parar", () => {
  it("toca a gravação salva nas contas marcadas", async () => {
    route({ play_recording_now: () => [{ userId: 11, errorCode: null, error: null }] });
    const { store } = renderTab();
    await screen.findByLabelText("Recording name");
    const play = screen.getByRole("button", { name: "Play now" });
    expect(play).toBeDisabled();

    const tryIt = screen.getByText("Try it now").parentElement as HTMLElement;
    await userEvent.click(within(tryIt).getByRole("button", { name: "alpha" }));
    await userEvent.click(play);
    expect(calls("play_recording_now")[0][1]).toEqual({ userIds: [11], recordingId: "rec-1" });
    expect(store.addToast).toHaveBeenCalledWith("Played on 1 account");
  });

  it("com mudança sem salvar, não toca", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");
    const tryIt = screen.getByText("Try it now").parentElement as HTMLElement;
    await userEvent.click(within(tryIt).getByRole("button", { name: "alpha" }));
    await userEvent.click(screen.getByRole("button", { name: "Wait" }));
    expect(screen.getByRole("button", { name: "Play now" })).toBeDisabled();
    expect(screen.getByText("Save before playing.")).toBeInTheDocument();
  });

  it("diz por que uma conta não recebeu a gravação", async () => {
    route({ play_recording_now: () => [{ userId: 11, errorCode: "focusLost", error: "x" }] });
    renderTab();
    await screen.findByLabelText("Recording name");
    const tryIt = screen.getByText("Try it now").parentElement as HTMLElement;
    await userEvent.click(within(tryIt).getByRole("button", { name: "alpha" }));
    await userEvent.click(screen.getByRole("button", { name: "Play now" }));
    expect(
      await screen.findByText("alpha: another window came to the front, so the rest of the recording was not played.")
    ).toBeInTheDocument();
  });

  it("tocando, mostra o estado e o Parar, que não fecha nada", async () => {
    playing = true;
    renderTab();
    const stop = await screen.findByRole("button", { name: "Stop playing" });
    expect(screen.getByTestId("recordings-status")).toHaveAttribute("data-running", "true");
    await userEvent.click(stop);
    expect(calls("stop_recording_playback")).toHaveLength(1);
    expect(calls("cmd_kill_roblox")).toHaveLength(0);

    await act(async () => emitTauriEvent("recording-playback", { active: false }));
    await waitFor(() => expect(screen.getByTestId("recordings-status")).toHaveAttribute("data-running", "false"));
  });

  it("explica o preço: a janela fica na frente pela gravação inteira", async () => {
    renderTab();
    await screen.findByLabelText("Recording name");
    expect(screen.getByText(/plays the whole recording there and gives the focus back only after the last one/)).toBeInTheDocument();
  });
});

describe("ClicksTab — modo gravação", () => {
  function renderClicks(afk: Record<string, string>) {
    const settings = defaultSettings();
    (settings as Record<string, Record<string, string>>).Afk = { ...afk };
    const store = setStore({
      accounts: ACCOUNTS,
      launchedByProgram: new Set([11, 22]),
      afkKeys: KEYS,
      settings,
      afkStatus: null,
    });
    render(<ClicksTab targetUserIds={[11, 22]} />);
    return { store };
  }

  it("liga no modo gravação sem tecla e mostra a gravação de cada conta", async () => {
    payload = library({ defaultId: "rec-1", accountIds: { "22": "rec-2" } });
    const { store } = renderClicks({ Mode: "recording" });
    expect(await screen.findByText("Walk forward")).toBeInTheDocument();
    expect(screen.getByText("Jump")).toBeInTheDocument();
    expect(screen.queryByLabelText("Key to send")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Start AFK Mode" }));
    expect(store.startAfkMode).toHaveBeenCalledWith(expect.objectContaining({ mode: "recording", userIds: [11, 22] }));
  });

  it("sem gravação para nenhuma conta marcada, não liga e diz por quê", async () => {
    renderClicks({ Mode: "recording" });
    expect(await screen.findAllByText("no recording")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Start AFK Mode" })).toBeDisabled();
    expect(screen.getByText("None of the ticked accounts has a recording to play")).toBeInTheDocument();
  });
});
