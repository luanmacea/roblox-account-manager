import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("./store", async () => (await import("./test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("./test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/event", async () => (await import("./test-utils/tauriMocks")).tauriEventMock());
vi.mock("@tauri-apps/api/window", async () => (await import("./test-utils/tauriMocks")).tauriWindowMock());
vi.mock("@tauri-apps/api/webview", async () => (await import("./test-utils/tauriMocks")).tauriWebviewMock());
vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(async () => {}),
  disable: vi.fn(async () => {}),
}));

import App from "./App";
import { defaultSettings, makeAccount, setStore } from "./test-utils/renderWithStore";
import { invokeMock, monitorMock, resetTauriMocks, setInvokeHandler, webviewMock } from "./test-utils/tauriMocks";
import { walkTour } from "./test-utils/tourHelpers";
import { closeTour } from "./components/tour/tourState";
import { TONE_STYLES } from "./utils/toastTone";
import type { StoreValue } from "./store";

const A = makeAccount({ UserID: 1, Username: "ann" });
const B = makeAccount({ UserID: 2, Username: "bob" });

function renderApp(overrides: Partial<StoreValue> = {}) {
  const store = setStore({ accounts: [A, B], ...overrides });
  render(<App />);
  return store;
}

function selecting(ids: number[]) {
  const selectedAccounts = [A, B].filter((a) => ids.includes(a.UserID));
  return { selectedIds: new Set(ids), selectedAccounts };
}

beforeEach(() => {
  resetTauriMocks();
  localStorage.clear();
  setInvokeHandler((cmd) => {
    switch (cmd) {
      case "get_all_settings":
        return {};
      case "get_setting":
        return "";
      case "versions_list_installed":
        return [];
      default:
        return undefined;
    }
  });
});

afterEach(() => {
  cleanup();
  closeTour();
});

describe("App — tamanho da interface", () => {
  it("aplica o tamanho salvo em General.InterfaceScale", async () => {
    const settings = defaultSettings();
    renderApp({ settings: { ...settings, General: { ...settings.General, InterfaceScale: "90" } } });
    await waitFor(() => expect(webviewMock.setZoom).toHaveBeenCalledWith(0.9));
    expect(webviewMock.setZoom).toHaveBeenCalledTimes(1);
  });

  it("automático vale já na tela de senha (monitor lógico 1280x720 -> 85%)", async () => {
    monitorMock.mockResolvedValue({ size: { width: 1920, height: 1080 }, scaleFactor: 1.5 });
    try {
      renderApp({ needsPassword: true });
      await waitFor(() => expect(webviewMock.setZoom).toHaveBeenCalledWith(0.85));
    } finally {
      monitorMock.mockResolvedValue({ size: { width: 1440, height: 800 }, scaleFactor: 1 });
    }
  });
});

describe("App — blocking screens", () => {
  it("waits while the store initialises", () => {
    renderApp({ initialized: false });
    expect(screen.getByText("Loading...")).toBeInTheDocument();
    expect(screen.queryByText("MultiAlt")).not.toBeInTheDocument();
  });

  it("asks for the password before anything else", () => {
    renderApp({ needsPassword: true, encryptionSetupOpen: true });
    expect(screen.getByText("Restricted Access")).toBeInTheDocument();
    expect(screen.queryByText("Set Up Encryption")).not.toBeInTheDocument();
  });

  it("shows the encryption setup once unlocked", () => {
    renderApp({ encryptionSetupOpen: true });
    expect(screen.getByText("Set Up Encryption")).toBeInTheDocument();
    expect(screen.queryByText("Restricted Access")).not.toBeInTheDocument();
  });

  it("checks for updates once the app is usable", () => {
    const store = renderApp();
    expect(store.checkForUpdates).toHaveBeenCalledTimes(1);
  });

  it("does not check for updates behind the password screen", () => {
    const store = renderApp({ needsPassword: true });
    expect(store.checkForUpdates).not.toHaveBeenCalled();
  });

  // A faixa da chave do vault e a unica rede contra o lockout de quem nao tem
  // senha, e estas duas telas sao exatamente as do momento de panico: a de senha e
  // onde cai quem nao conseguiu abrir o vault, e a de criptografia e onde o backend
  // manda o usuario olhar ("See the warning on screen"). Os `return` antecipados do
  // App deixavam a faixa atras deles — ponteiro quebrado na hora que importa.
  const keyWarning = {
    code: "writeFailed" as const,
    path: "C:\dados\AccountData.key",
  };

  it("shows the vault key warning on the password screen", () => {
    renderApp({ needsPassword: true, vaultKeyWarning: keyWarning });
    expect(screen.getByText("Restricted Access")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("C:\dados\AccountData.key");
  });

  it("shows the vault key warning on the encryption setup screen", () => {
    renderApp({ encryptionSetupOpen: true, vaultKeyWarning: keyWarning });
    expect(screen.getByText("Set Up Encryption")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("C:\dados\AccountData.key");
  });

  it("shows the vault key warning on the main layout", () => {
    renderApp({ vaultKeyWarning: keyWarning });
    expect(screen.getByRole("alert")).toHaveTextContent("C:\dados\AccountData.key");
  });

  it("draws no alert when there is nothing wrong with the key file", () => {
    renderApp();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("App — main layout", () => {
  it("renders the window chrome, account list and status bar", () => {
    renderApp();
    expect(screen.getByText("MultiAlt")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Filter accounts...")).toBeInTheDocument();
    expect(screen.getByText("ann")).toBeInTheDocument();
    expect(screen.getByText("Legend:")).toBeInTheDocument();
  });

  it("swaps the account list for the Choose Game screen", () => {
    renderApp({ chooseGameOpen: true, ...selecting([1]) });
    expect(screen.getByText("Choose Game")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Back/ })).toBeInTheDocument();
  });

  it("shows the batch action bar only with a selection and no Choose Game", () => {
    renderApp();
    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();

    cleanup();
    renderApp(selecting([1, 2]));
    expect(screen.getByRole("button", { name: "Clear" })).toBeInTheDocument();

    cleanup();
    renderApp({ chooseGameOpen: true, ...selecting([1, 2]) });
    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
  });

  it("opens the detail sidebar only for exactly one selected account", () => {
    renderApp({ sidebarOpen: true, ...selecting([1, 2]) });
    expect(screen.queryByText("Account Details")).not.toBeInTheDocument();

    cleanup();
    renderApp({ sidebarOpen: false, ...selecting([1]) });
    expect(screen.queryByText("Account Details")).not.toBeInTheDocument();
  });
});

describe("App — error banner, toasts and the generic modal", () => {
  it("shows a dismissible error banner", async () => {
    const store = renderApp({ error: "Something went wrong" });
    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close Roblox" })).not.toBeInTheDocument();

    const banner = screen.getByText("Something went wrong").parentElement as HTMLElement;
    await userEvent.click(banner.querySelectorAll("button")[0]);
    expect(store.setError).toHaveBeenCalledWith(null);
  });

  it("offers Close Roblox for a multi-Roblox failure", async () => {
    const store = renderApp({ error: "Failed to enable Multi Roblox" });
    await userEvent.click(screen.getByRole("button", { name: "Close Roblox" }));
    expect(store.killAllRobloxProcesses).toHaveBeenCalledTimes(1);
  });

  /**
   * A faixa cortava a mensagem com `truncate`, e é justamente o fim dela que
   * explica o erro. O texto inteiro tem que ficar legível — quebrando linha,
   * não virando painel.
   */
  it("shows the whole error message instead of cutting it off", () => {
    const long =
      "Failed to launch account 3: the production build is missing and the download was refused by the server";
    renderApp({ error: long });

    const message = screen.getByText(long);
    expect(message).not.toHaveClass("truncate");
    expect(message.className).toContain("whitespace-pre-wrap");
  });

  /**
   * O porquê da falha fica no log de lançamento, em outra tela. A faixa tem que
   * dizer onde ele está e levar até lá.
   */
  it("points at the launch log when there is one", async () => {
    const store = renderApp({
      error: "Launch failed",
      launchLogs: [
        { id: 1, userId: 1, level: "error", step: "launch", message: "boom", ts: Date.now() },
      ],
    });

    expect(screen.getByText(/Choose Game/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open launch log" }));
    expect(store.setChooseGameOpen).toHaveBeenCalledWith(true);
  });

  it("offers no launch log when nothing was logged", () => {
    renderApp({ error: "Launch failed", launchLogs: [] });
    expect(screen.queryByRole("button", { name: "Open launch log" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Choose Game/)).not.toBeInTheDocument();
  });

  it("stacks the toasts", () => {
    renderApp({
      toasts: [
        { id: 1, message: "Copied 2 cookies", tone: "success" },
        { id: 2, message: "Added roboduck", tone: "info" },
      ],
    });
    expect(screen.getByText("Copied 2 cookies")).toBeInTheDocument();
    expect(screen.getByText("Added roboduck")).toBeInTheDocument();
  });

  /**
   * Toda a fila saía com `theme-panel theme-border` e mais nada, então "Launch
   * failed" e "Accounts saved" eram visualmente a mesma coisa. A cor vem do
   * mesmo mapa do Console de launch (`TONE_STYLES`).
   */
  it("paints each toast with the colour of its tone", () => {
    renderApp({
      toasts: [
        { id: 1, message: "Launch failed", tone: "error" },
        { id: 2, message: "Accounts saved", tone: "success" },
        { id: 3, message: "Low memory warning", tone: "warn" },
        { id: 4, message: "Launching game...", tone: "info" },
      ],
    });

    const toastOf = (text: string) => screen.getByText(text).closest("div") as HTMLElement;
    expect(toastOf("Launch failed").className).toContain(TONE_STYLES.error.text);
    expect(toastOf("Accounts saved").className).toContain(TONE_STYLES.success.text);
    expect(toastOf("Low memory warning").className).toContain(TONE_STYLES.warn.text);
    expect(toastOf("Launching game...").className).toContain(TONE_STYLES.info.text);

    // A bolinha repete o padrão do rodapé e do Console.
    expect(toastOf("Launch failed").querySelector(`.${CSS.escape(TONE_STYLES.error.dot)}`)).not.toBeNull();
  });

  /**
   * Com `key={i}` a saída do primeiro toast renumerava os que sobraram e o
   * React remontava cada um — a animação de entrada reiniciava sozinha. O `id`
   * do toast é a chave estável.
   */
  it("keeps a toast mounted when an older one leaves the queue", () => {
    const second = { id: 2, message: "second toast", tone: "info" as const };
    setStore({
      accounts: [A, B],
      toasts: [{ id: 1, message: "first toast", tone: "info" }, second],
    });
    const { rerender } = render(<App />);
    const survivor = screen.getByText("second toast");

    setStore({ accounts: [A, B], toasts: [second] });
    rerender(<App />);

    // Com `key={i}` o React reaproveitaria o nó do toast que saiu e descartaria
    // este, reiniciando a animação de entrada do que ficou.
    expect(survivor).toBeInTheDocument();
    expect(screen.queryByText("first toast")).not.toBeInTheDocument();
  });

  it("renders the generic text modal and closes it", async () => {
    const store = renderApp({ modal: { title: "Auth ticket", content: "ticket-body" } });
    expect(screen.getByText("Auth ticket")).toBeInTheDocument();
    expect(screen.getByText("ticket-body")).toBeInTheDocument();

    await userEvent.click(screen.getByText("Auth ticket").parentElement!.querySelector("button")!);
    expect(store.closeModal).toHaveBeenCalledTimes(1);
  });
});

describe("App — dialog routing", () => {
  it("keeps every dialog closed by default", () => {
    renderApp();
    expect(screen.queryByRole("heading", { name: "Settings" })).not.toBeInTheDocument();
    expect(screen.queryByText("Roblox Versions")).not.toBeInTheDocument();
  });

  it("opens the versions dialog from store state", async () => {
    renderApp({ versionsDialogOpen: true });
    expect(await screen.findByText("Roblox Versions")).toBeInTheDocument();
  });

  it("opens the first-run walkthrough from store state", () => {
    renderApp({ firstRunWalkthroughOpen: true });
    // The walkthrough owns the screen, so the update check is skipped.
    expect(screen.queryByText("Loading...")).not.toBeInTheDocument();
  });
});

/**
 * A barra de ícones do topo virou barra lateral com rótulo, e cada item abre
 * uma página na área principal em vez de modal. A barra de cima ficou só com o
 * que é da lista de contas — e só aparece na página de contas.
 */
describe("App — pages", () => {
  it("shows the side navigation with the account list as the current page", () => {
    renderApp();
    const nav = screen.getByRole("navigation", { name: "Main navigation" });
    expect(nav).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Accounts/ })).toHaveAttribute("aria-current", "page");
  });

  it("keeps the account top bar on the account page only", () => {
    renderApp();
    expect(screen.getByPlaceholderText("Filter accounts...")).toBeInTheDocument();

    cleanup();
    renderApp({ activePage: "session" });
    expect(screen.queryByPlaceholderText("Filter accounts...")).not.toBeInTheDocument();
    expect(screen.queryByText("ann")).not.toBeInTheDocument();
  });

  it.each([
    ["session", "Session"],
    ["avatars", "Avatars"],
    // "groups" fica de fora: a página está atrás de ENABLE_GROUPS (desligada,
    // branch feature/groups) e o item não aparece na barra lateral.
    ["scripts", "Scripts"],
    ["theme", "Theme"],
    ["settings", "Settings"],
    ["afk", "AFK Mode"],
  ] as const)("renders the %s page with its own header", (page, title) => {
    renderApp({ activePage: page });
    expect(screen.getByRole("heading", { level: 1, name: title })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: new RegExp(`^${title}`) })).toHaveAttribute("aria-current", "page");
  });

  /**
   * A página de novidades lê as releases do GitHub (60 pedidos/hora sem login).
   * Só quando é aberta: abrir o app não gasta pedido nenhum.
   */
  it("does not ask GitHub for the update history on start", () => {
    const fetchMock = vi.fn(async () => ({ ok: true, status: 200, headers: new Headers(), json: async () => [] }));
    vi.stubGlobal("fetch", fetchMock);
    try {
      renderApp();
      expect(fetchMock).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("renders the What's new page with its own header", async () => {
    const fetchMock = vi.fn(async () => ({ ok: true, status: 200, headers: new Headers(), json: async () => [] }));
    vi.stubGlobal("fetch", fetchMock);
    try {
      renderApp({ activePage: "changelog" });
      expect(screen.getByRole("heading", { level: 1, name: "What's new" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /^What's new/ })).toHaveAttribute("aria-current", "page");
      expect(await screen.findByText("No updates to show yet.")).toBeInTheDocument();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("goes back to the account list on Escape", async () => {
    const store = renderApp({ activePage: "settings" });
    await userEvent.keyboard("{Escape}");
    expect(store.setActivePage).toHaveBeenCalledWith("accounts");
  });

  /**
   * Página não é modal: os botões da janela (minimizar, fechar) continuam na
   * barra de título. Escondê-los era o que o `anyModalOpen` fazia com os modais.
   */
  it("keeps the window controls in the title bar on a page", () => {
    renderApp({ activePage: "scripts" });
    // A pílula de controles dos modais fica escondida, e a barra de título não
    // recolhe os seus.
    const pill = screen.getByRole("button", { name: "Move window" }).parentElement as HTMLElement;
    expect(pill.className).toContain("pointer-events-none");
    const titleBarControls = screen
      .getAllByRole("button", { name: "Minimize" })
      .map((b) => b.closest("div") as HTMLElement)
      .filter((div) => div !== pill);
    expect(titleBarControls).toHaveLength(1);
    expect(titleBarControls[0].className).not.toContain("max-w-0");
  });

  it("hides the batch action bar outside the account page", () => {
    renderApp({ activePage: "avatars", ...selecting([1]) });
    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
  });
});

/**
 * Tutoriais de tela: um botão Tutorial em cada tela, nenhum abre sozinho, e o
 * da lista de contas aponta partes que existem sem selecionar nem lançar nada.
 */
describe("App — screen tutorials", () => {
  it("never opens a tutorial by itself", () => {
    renderApp(selecting([1]));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("walks the Accounts tutorial with every part on screen", async () => {
    const store = renderApp({ sidebarOpen: true, ...selecting([1]) });
    await walkTour("accounts", { withHost: false, invoke: invokeMock });
    expect(store.setChooseGameOpen).not.toHaveBeenCalled();
    expect(store.setHideUsernames).not.toHaveBeenCalled();
    expect(store.selectSingle).not.toHaveBeenCalled();
  });

  it("falls back gracefully on an empty account list", async () => {
    renderApp({ accounts: [] });
    await walkTour("accounts", { withHost: false, allowMissing: ["select", "panel", "choose-game"] });
  });

  it.each([
    ["session", "Session"],
    ["afk", "AFK Mode"],
    ["avatars", "Avatars"],
    ["groups", "Groups"],
    ["scripts", "Scripts"],
    ["theme", "Theme"],
    ["settings", "Settings"],
    ["changelog", "What's new"],
  ] as const)("puts a Tutorial button on the %s page", (page, title) => {
    renderApp({ activePage: page });
    const header = screen.getByRole("heading", { level: 1, name: title }).closest("header") as HTMLElement;
    expect(within(header).getByRole("button", { name: /Tutorial/ })).toBeInTheDocument();
  });

  it("puts a Tutorial button on the Choose Game screen", () => {
    renderApp({ chooseGameOpen: true, ...selecting([1]) });
    expect(screen.getByRole("button", { name: /Tutorial/ })).toBeInTheDocument();
  });

  it("closes the tutorial when the person leaves its screen", async () => {
    renderApp({ activePage: "theme" });
    await userEvent.click(screen.getByRole("button", { name: /Tutorial/ }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    // Foi para a lista de contas (a store de teste não re-renderiza sozinha).
    cleanup();
    renderApp({ activePage: "accounts" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument(), { timeout: 1500 });
  });
});

describe("App — trancado por inatividade (ideia 27)", () => {
  it("covers the app with the lock screen without unmounting what is behind it", () => {
    renderApp({ appLocked: true });
    const overlay = screen.getByRole("dialog", { name: "MultiAlt is locked" });
    expect(overlay).toBeInTheDocument();
    // A lista de contas continua montada (nada para), só fica inerte.
    const behind = screen.getByText("ann").closest("[inert]");
    expect(behind).not.toBeNull();
    expect(overlay.closest("[inert]")).toBeNull();
  });

  it("shows no lock screen while unlocked", () => {
    renderApp();
    expect(screen.queryByRole("dialog", { name: "MultiAlt is locked" })).not.toBeInTheDocument();
    expect(document.querySelector("[inert]")).toBeNull();
  });

  it("locks by itself after the set minutes only with the option on and an app password", () => {
    vi.useFakeTimers();
    try {
      const settings = defaultSettings();
      const on = { ...settings, General: { ...settings.General, LockOnInactivity: "true", LockAfterMinutes: "1" } };
      const store = renderApp({ settings: on, accountsEncrypted: true });
      vi.advanceTimersByTime(70_000);
      expect(store.lockApp).toHaveBeenCalled();
      cleanup();

      const noPassword = renderApp({ settings: on, accountsEncrypted: false });
      vi.advanceTimersByTime(10 * 60_000);
      expect(noPassword.lockApp).not.toHaveBeenCalled();
      cleanup();

      const off = renderApp({ settings, accountsEncrypted: true });
      vi.advanceTimersByTime(10 * 60_000);
      expect(off.lockApp).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});