import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("../../hooks/usePrompt", async () => (await import("../../test-utils/promptMocks")).promptModuleMock());
// O gerador pago (BloxGen) vem desligado (`ENABLE_ACCOUNT_GENERATOR`); estes
// testes cobrem o gerador ligado. Desligado: accountGeneratorHidden.test.tsx.
vi.mock("../../featureFlags", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../featureFlags")>()),
  ENABLE_ACCOUNT_GENERATOR: true,
}));

import { Toolbar } from "./Toolbar";
import { makeAccount, setStore } from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks, setInvokeMap } from "../../test-utils/tauriMocks";
import { promptAnswers, promptMock, resetPromptMocks } from "../../test-utils/promptMocks";
import type { StoreValue } from "../../store";

const COOKIE =
  "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items.|ABC";

function renderToolbar(overrides: Partial<StoreValue> = {}) {
  const store = setStore({ accounts: [makeAccount({ UserID: 1 })], ...overrides });
  render(<Toolbar />);
  return store;
}

async function openAddMenu() {
  await userEvent.click(screen.getByRole("button", { name: /^Add/ }));
}

/**
 * Todo botão de ícone da toolbar tem que se identificar por nome acessível —
 * tooltip só aparece depois de 350 ms de mouse parado, o que não serve para
 * leitor de tela nem para teste. Os nomes que mudam de estado (selecionar
 * tudo, painel) são casados por trecho estável.
 */
const ICON_BUTTONS = {
  clear: /clear search/i,
  selectAll: /select all/i,
  panel: /panel/i,
} as const;

function iconButton(name: keyof typeof ICON_BUTTONS): HTMLElement {
  return screen.getByRole("button", { name: ICON_BUTTONS[name] });
}

beforeEach(() => {
  resetTauriMocks();
  resetPromptMocks();
});

afterEach(cleanup);

describe("Toolbar — search and toggles", () => {
  it("writes the search query to the store", async () => {
    const store = renderToolbar();
    await userEvent.type(screen.getByPlaceholderText("Filter accounts..."), "a");
    expect(store.setSearchQuery).toHaveBeenLastCalledWith("a");
  });

  it("offers a clear button only while a query is set", async () => {
    renderToolbar();
    const before = screen.getAllByRole("button").length;

    cleanup();
    const store = renderToolbar({ searchQuery: "ann" });
    expect(screen.getAllByRole("button").length).toBe(before + 1);

    await userEvent.click(iconButton("clear"));
    expect(store.setSearchQuery).toHaveBeenCalledWith("");
  });

  it("toggles select-all", async () => {
    const store = renderToolbar();
    await userEvent.click(iconButton("selectAll"));
    expect(store.toggleSelectAll).toHaveBeenCalledTimes(1);
  });

  it("toggles name hiding and shows the current state", async () => {
    const store = renderToolbar({ hideUsernames: false });
    await userEvent.click(screen.getByRole("button", { name: /names shown/i }));
    expect(store.setHideUsernames).toHaveBeenCalledWith(true);

    cleanup();
    const store2 = renderToolbar({ hideUsernames: true });
    await userEvent.click(screen.getByRole("button", { name: /names hidden/i }));
    expect(store2.setHideUsernames).toHaveBeenCalledWith(false);
  });

  /**
   * `Names`/`Hidden` não dizia o que o botão faz — nem no rótulo, nem em
   * tooltip nenhum, porque era o único da toolbar sem. O texto tem que citar
   * os nomes de usuário, que é o que some da lista.
   */
  it("explains that the name toggle masks usernames", async () => {
    renderToolbar({ hideUsernames: false });
    const button = screen.getByRole("button", { name: /names/i });
    fireEvent.focus(button);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(/usernames/i);
  });
});

describe("Toolbar — accessible names", () => {
  it("names every icon-only button", () => {
    renderToolbar({ searchQuery: "ann", selectedIds: new Set([1]) });

    const expected: (keyof typeof ICON_BUTTONS)[] = ["clear", "selectAll", "panel"];
    for (const name of expected) {
      expect(iconButton(name)).toBeInTheDocument();
    }
  });
});

/**
 * Sessão, AFK Mode, Avatars, Scripts, Theme, Nexus, Settings e Ajuda eram
 * ícones aqui que só se explicavam com o mouse parado em cima. Viraram itens
 * com nome na barra lateral; a barra de cima ficou só com o que age na lista.
 */
describe("Toolbar — only list actions", () => {
  it("has no page buttons anymore", () => {
    renderToolbar();
    for (const name of [/session/i, /theme/i, /nexus/i, /scripts/i, /settings/i, /help/i, /afk/i, /avatars/i]) {
      expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
    }
  });
});

describe("Toolbar — Add menu", () => {
  it("opens and closes the Add menu", async () => {
    renderToolbar();
    expect(screen.queryByRole("button", { name: "Quick Add" })).not.toBeInTheDocument();

    await openAddMenu();
    expect(screen.getByRole("button", { name: "Quick Add" })).toBeInTheDocument();

    await openAddMenu();
    expect(screen.queryByRole("button", { name: "Quick Add" })).not.toBeInTheDocument();
  });

  it("routes Browser Login to the store", async () => {
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Browser Login" }));
    expect(store.openLoginBrowser).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["User:Pass Login", "userpass"],
    ["Import Cookie", "cookie"],
    ["Import Old Account Data", "legacy"],
  ])("opens the import dialog on the %s tab", async (label, tab) => {
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: label }));
    expect(store.setImportDialogTab).toHaveBeenCalledWith(tab);
    expect(store.setImportDialogOpen).toHaveBeenCalledWith(true);
  });

  /**
   * As duas formas de conseguir conta nova vivem no mesmo diálogo, mas cada
   * entrada do menu abre na sua aba — criar no navegador não pode exigir que o
   * usuário descubra uma aba escondida atrás do gerador por provedor.
   */
  it("opens the generator on the tab the menu entry asked for", async () => {
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: /^Create Accounts/ }));
    expect(store.openGeneratorDialog).toHaveBeenCalledWith("signup");

    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: /^Account Generator/ }));
    expect(store.openGeneratorDialog).toHaveBeenCalledWith("provider");
  });

  /**
   * Uma cria conta de graça no navegador embutido (com o CAPTCHA resolvido
   * pela pessoa) e a outra **compra** conta pronta de um serviço pago. Nada na
   * tela dizia isso — ver docs/features/account-creation.md.
   */
  it("says which way is free and which one costs money", async () => {
    renderToolbar();
    await openAddMenu();

    const create = screen.getByRole("button", { name: /^Create Accounts/ });
    expect(create).toHaveTextContent(/free/i);
    expect(create).toHaveTextContent(/CAPTCHA/);

    const generator = screen.getByRole("button", { name: /^Account Generator/ });
    expect(generator).toHaveTextContent(/paid/i);
    expect(generator).toHaveTextContent(/BloxGen/);
  });

  it("offers Quick Login in the Add menu too", async () => {
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Login" }));
    expect(store.setQuickLoginOpen).toHaveBeenCalledWith(true);
  });

  it("opens the versions dialog", async () => {
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Roblox Versions" }));
    expect(store.setVersionsDialogOpen).toHaveBeenCalledWith(true);
  });
});

describe("Toolbar — Quick Add", () => {
  /**
   * O import anuncia o formato `username:password:cookie`, e o Quick Add
   * decidia com `includes(COOKIE_MARKER)` e mandava a linha **inteira** como
   * cookie: a senha viajava no cabeçalho de cookie e o recurso falhava com
   * "Invalid cookie". Agora a linha passa pelo mesmo leitor do import
   * (`parseImportLine`): só o cookie vai como cookie, e a senha fica guardada,
   * como no import.
   */
  it("manda só o cookie de uma linha username:password:cookie e guarda a senha", async () => {
    promptAnswers.prompt = `alt_one:hunter2:${COOKIE}`;
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() => expect(store.addAccountByCookie).toHaveBeenCalledWith(COOKIE, "hunter2"));
    expect(invokeMock).not.toHaveBeenCalledWith("lookup_user", expect.anything());
  });

  /**
   * Sem cookie, `usuario:senha` ia para o `lookup_user` como se fosse um nome
   * de usuário — a senha saía na busca de usuário. Quick Add não entra com
   * senha; a tela diz qual entrada faz isso.
   */
  it("não procura usuario:senha como nome de usuário", async () => {
    promptAnswers.prompt = "alt_one:hunter2";
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() => expect(store.addToast).toHaveBeenCalled());
    expect(invokeMock).not.toHaveBeenCalledWith("lookup_user", expect.anything());
    expect(store.addAccountByCookie).not.toHaveBeenCalled();
    const message = (store.addToast as ReturnType<typeof vi.fn>).mock.calls[0][0] as string;
    expect(message).toContain("User:Pass Login");
    expect(message).not.toContain("hunter2");
  });

  it("treats a pasted cookie as a cookie add", async () => {
    promptAnswers.prompt = COOKIE;
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() => expect(store.addAccountByCookie).toHaveBeenCalledWith(COOKIE));
    expect(invokeMock).not.toHaveBeenCalledWith("lookup_user", expect.anything());
  });

  it("looks a username up and adds it without a cookie", async () => {
    promptAnswers.prompt = "  roboduck  ";
    setInvokeMap({ lookup_user: { id: 77, name: "roboduck" } });
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("lookup_user", { username: "roboduck" })
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("add_account", {
        securityToken: "",
        username: "roboduck",
        userId: 77,
      })
    );
    expect(store.loadAccounts).toHaveBeenCalled();
  });

  /**
   * Conta criada por nome de usuário não tem cookie: não lança, não entra em
   * lugar nenhum. O aviso de sucesso não pode soar igual ao de um cookie
   * válido — tem que dizer que entrou sem sessão e o que falta fazer.
   */
  it("says the username-only account came in without a session", async () => {
    promptAnswers.prompt = "roboduck";
    setInvokeMap({ lookup_user: { id: 77, name: "roboduck" } });
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() => expect(store.addToast).toHaveBeenCalled());
    const message = (store.addToast as ReturnType<typeof vi.fn>).mock.calls[0][0] as string;
    expect(message).toContain("roboduck");
    expect(message).toContain("no session");
    expect(message).toContain("Browser Login");
  });

  /**
   * O cookie `.ROBLOSECURITY` é pedido aqui sem uma palavra sobre o que é nem
   * onde achá-lo — e ele entra como a conta inteira (`api/auth.rs:45`). O texto
   * é o mesmo do Quick Add do AddAccountDialog.
   */
  it("tells the prompt what the cookie is and where it lives", async () => {
    promptAnswers.prompt = null;
    renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    const message = promptMock.mock.calls[0][0] as string;
    expect(message).toContain(".ROBLOSECURITY");
    expect(message).toMatch(/DevTools/);
    expect(message).toMatch(/signs in as/i);
  });

  it("does nothing when the prompt is cancelled", async () => {
    promptAnswers.prompt = null;
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));
    await Promise.resolve();

    expect(store.addAccountByCookie).not.toHaveBeenCalled();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("reports a lookup failure as a toast", async () => {
    promptAnswers.prompt = "ghost";
    setInvokeMap({
      lookup_user: () => {
        throw new Error("user not found");
      },
    });
    const store = renderToolbar();
    await openAddMenu();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() =>
      expect(store.addToast).toHaveBeenCalledWith(expect.stringContaining("user not found"))
    );
  });
});

describe("Toolbar — detail panel", () => {
  it("toggles the detail sidebar", async () => {
    const store = renderToolbar({ sidebarOpen: false, selectedIds: new Set([1]) });
    await userEvent.click(iconButton("panel"));
    expect(store.setSidebarOpen).toHaveBeenCalledWith(true);
  });

  /**
   * O painel só existe para uma conta (App.tsx). Com 0 ou 2+ selecionadas o
   * botão acendia e nada aparecia — então ele tem que estar desabilitado,
   * dizendo por quê, em vez de mentir que ligou.
   */
  it.each([
    ["nothing selected", [] as number[]],
    ["two accounts selected", [1, 2]],
  ])("disables the panel button with %s", async (_label, ids) => {
    const store = renderToolbar({
      accounts: [makeAccount({ UserID: 1 }), makeAccount({ UserID: 2 })],
      sidebarOpen: false,
      selectedIds: new Set(ids),
    });

    const panel = iconButton("panel");
    expect(panel).toBeDisabled();
    await userEvent.click(panel);
    expect(store.setSidebarOpen).not.toHaveBeenCalled();
  });

  it("keeps the panel button enabled for exactly one selected account", () => {
    renderToolbar({ selectedIds: new Set([1]) });
    expect(iconButton("panel")).toBeEnabled();
  });
});

describe("Toolbar — menu Add cabe na janela", () => {
  /**
   * O menu não tinha teto: na janela mínima do app (750x450) o último item
   * ("Roblox Versions") passava 14 px da borda de baixo em inglês e 30 px em
   * português, meio cortado ou fora da tela (medido no harness, 27/09/2026).
   * O jsdom não mede layout: isto trava o teto e a rolagem de que o conserto
   * depende.
   */
  it("o menu tem teto de altura e rola por dentro", async () => {
    renderToolbar();
    await openAddMenu();
    const painel = screen.getByRole("button", { name: /Quick Add/ }).closest(".rounded-xl") as HTMLElement;
    const classes = painel.className.split(/\s+/);
    expect(classes).toContain("max-h-[calc(100vh-96px)]");
    expect(classes).toContain("overflow-y-auto");
  });
});
