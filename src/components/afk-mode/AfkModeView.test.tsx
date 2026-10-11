import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/event", async () => (await import("../../test-utils/tauriMocks")).tauriEventMock());
vi.mock("../../hooks/usePrompt", async () => (await import("../../test-utils/promptMocks")).promptModuleMock());

import { AfkModeView } from "./AfkModeView";
import { AfkModeDialog } from "./AfkModeDialog";
import {
  defaultSettings,
  makeAccount,
  makeBottingStatus,
  setStore,
} from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks, setInvokeHandler } from "../../test-utils/tauriMocks";
import { AfkPage } from "../pages/AfkPage";
import { walkTour } from "../../test-utils/tourHelpers";
import { resetPromptMocks } from "../../test-utils/promptMocks";
import type { AfkStatus, StoreValue } from "../../store";

const A = makeAccount({ UserID: 1, Username: "ann" });
const B = makeAccount({ UserID: 2, Username: "bob" });
const C = makeAccount({ UserID: 3, Username: "cid" });

function afkRunning(): AfkStatus {
  return {
    active: true,
    startedAtMs: 1_000,
    intervalSeconds: 600,
    key: "Space",
    mode: "key",
    clickX: 50,
    clickY: 50,
    accounts: [],
  };
}

function base(overrides: Partial<StoreValue> = {}) {
  const settings = defaultSettings();
  settings.General.EnableMultiRbx = "true";
  return setStore({
    accounts: [A, B, C],
    selectedIds: new Set([1, 2]),
    selectedAccounts: [A, B],
    launchedByProgram: new Set([1, 3]),
    afkKeys: ["Space"],
    settings,
    ...overrides,
  });
}

beforeEach(() => {
  resetTauriMocks();
  resetPromptMocks();
  setInvokeHandler((cmd) => (cmd === "get_all_settings" ? {} : undefined));
});

afterEach(cleanup);

describe("AfkModeView — as duas abas", () => {
  it("abre na aba pedida e troca de aba sem perder a outra", async () => {
    base();
    render(<AfkModeView variant="modal" initialTab="clicks" />);

    expect(screen.getByRole("tab", { name: /AFK clicks/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "Start AFK Mode" })).toBeInTheDocument();
    // A aba escondida não aparece para quem navega pela árvore de acessibilidade.
    expect(screen.queryByRole("button", { name: "Start Auto Rejoin" })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("tab", { name: /Auto Rejoin/ }));
    expect(screen.getByRole("button", { name: "Start Auto Rejoin" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start AFK Mode" })).not.toBeInTheDocument();
  });

  /** Os cliques AFK vêm primeiro e são a aba padrão (pedido do dono, 03/10/2026). */
  it("abre nos cliques AFK, que são a primeira aba", () => {
    base();
    render(<AfkModeView variant="page" />);
    const tabs = screen.getAllByRole("tab");
    expect(tabs[0]).toHaveAccessibleName(/AFK clicks/);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
  });

  it("as setas do teclado trocam de aba", async () => {
    base();
    render(<AfkModeView variant="modal" initialTab="rejoin" />);
    screen.getByRole("tab", { name: /Auto Rejoin/ }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: /AFK clicks/ })).toHaveAttribute("aria-selected", "true");
  });

  /** O que cada modo está fazendo, sem precisar abrir a aba dele. */
  it("cada aba diz se o modo dela está rodando", () => {
    base({ bottingStatus: makeBottingStatus({ active: true, userIds: [1, 2] }), afkStatus: null });
    render(<AfkModeView variant="modal" />);
    expect(screen.getByTestId("afk-mode-tab-state-rejoin")).toHaveTextContent("Running");
    expect(screen.getByTestId("afk-mode-tab-state-clicks")).toHaveTextContent("Stopped");

    cleanup();
    base({ bottingStatus: null, afkStatus: afkRunning() });
    render(<AfkModeView variant="modal" />);
    expect(screen.getByTestId("afk-mode-tab-state-rejoin")).toHaveTextContent("Stopped");
    expect(screen.getByTestId("afk-mode-tab-state-clicks")).toHaveTextContent("Running");
  });

  /** O cartão da aba Recordings diz quando a gravação toca, sem abrir a aba. */
  it("o cartão das gravações diz o intervalo do Modo AFK e a reconexão", () => {
    const settings = defaultSettings();
    settings.Afk = { Mode: "recording", IntervalMinutes: "2", IntervalSeconds: "30" };
    (settings as Record<string, Record<string, string>>).Recordings = { AfterReconnect: "true" };
    base({ settings });
    render(<AfkModeView variant="page" />);
    expect(screen.getByRole("tab", { name: /Recordings/ })).toHaveTextContent(
      "Every 2 min 30 s in AFK mode, and after a reconnect"
    );

    cleanup();
    base();
    render(<AfkModeView variant="page" />);
    expect(screen.getByRole("tab", { name: /Recordings/ })).toHaveTextContent(
      "Sequences of keys, clicks and waits played on each window"
    );
  });

  it("a página não tem botão de fechar; o modal tem", async () => {
    base();
    render(<AfkModeView variant="page" />);
    expect(screen.queryByRole("button", { name: "Close" })).not.toBeInTheDocument();
    // O título da página é do PageShell (AfkPage): o view não repete o seu.
    expect(screen.queryByRole("heading", { name: "AFK Mode" })).not.toBeInTheDocument();
    cleanup();

    base();
    const onClose = vi.fn();
    render(<AfkModeView variant="modal" onClose={onClose} />);
    const header = screen.getByRole("heading", { name: "AFK Mode" }).closest("header") as HTMLElement;
    await userEvent.click(within(header).getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("as contas de quem abriu chegam às duas abas", async () => {
    base({ detectRunningGamePlace: vi.fn(async () => 606849621) });
    render(<AfkModeView variant="modal" targetUserIds={[1, 3]} adoptRunning />);

    await userEvent.click(screen.getByRole("tab", { name: /Auto Rejoin/ }));
    // Auto Rejoin: as mesmas contas marcadas, na lista das que têm cliente.
    expect(screen.getByRole("button", { name: "ann" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "cid" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("radio", { name: /The game they are playing now/ })).toBeChecked();

    await userEvent.click(screen.getByRole("tab", { name: /AFK clicks/ }));
    expect(screen.getByRole("button", { name: "ann" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "cid" })).toHaveAttribute("aria-pressed", "true");
  });
});

/** Tutorial da página AFK Mode: passa pelas duas abas sem ligar nada. */
describe("AfkPage — tutorial", () => {
  it("walks every step, opening the Auto Rejoin tab only to show it", async () => {
    base();
    render(<AfkPage active onLeave={vi.fn()} />);
    await walkTour("afk", { invoke: invokeMock });
    // Terminou na aba que o último passo explicou, sem ligar nenhum dos dois.
    expect(screen.getByRole("tab", { name: /Auto Rejoin/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "Start Auto Rejoin" })).toBeInTheDocument();
  });
});

describe("AfkModeDialog", () => {
  it("não desenha nada com o Modo AFK fechado", () => {
    base({ afkModeDialog: null });
    const { container } = render(<AfkModeDialog />);
    expect(container).toBeEmptyDOMElement();
  });

  it("abre na aba que a store pediu e fecha pela store", async () => {
    const store = base({ afkModeDialog: { tab: "clicks" } });
    render(<AfkModeDialog />);

    expect(screen.getByRole("dialog", { name: "AFK Mode" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /AFK clicks/ })).toHaveAttribute("aria-selected", "true");

    const header = screen.getByRole("heading", { name: "AFK Mode" }).closest("header") as HTMLElement;
    await userEvent.click(within(header).getByRole("button", { name: "Close" }));
    expect(store.closeAfkMode).toHaveBeenCalled();
  });

  it("o jogo da abertura chega ao Auto Rejoin", async () => {
    base({ afkModeDialog: { tab: "rejoin", placeId: "606849621" } });
    render(<AfkModeDialog />);
    expect(screen.getByRole("radio", { name: /A game I pick/ })).toBeChecked();
    expect(screen.getByPlaceholderText("Place ID")).toHaveValue("606849621");
  });
});
