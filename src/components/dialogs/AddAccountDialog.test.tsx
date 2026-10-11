import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
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

import { AddAccountDialog } from "./AddAccountDialog";
import { setStore } from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks, setInvokeMap } from "../../test-utils/tauriMocks";
import { promptAnswers, promptMock, resetPromptMocks } from "../../test-utils/promptMocks";

const COOKIE =
  "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items.|TOKEN";

function renderDialog(open = true) {
  const store = setStore({});
  const onClose = vi.fn();
  render(<AddAccountDialog open={open} onClose={onClose} />);
  return { store, onClose };
}

beforeEach(() => {
  resetTauriMocks();
  resetPromptMocks();
});

afterEach(cleanup);

describe("AddAccountDialog", () => {
  it("renders nothing while closed", () => {
    const { container } = render(<AddAccountDialog open={false} onClose={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });

  /**
   * O estado vazio da lista abre este diálogo — era a única porta de quem tem
   * zero conta, e escondia justamente as duas entradas que criam conta nova.
   * As duas portas (aqui e o menu `Add` da toolbar) oferecem o mesmo conjunto.
   */
  it("offers the same entries as the toolbar Add menu", () => {
    renderDialog();
    expect(screen.getByText("Add Account")).toBeInTheDocument();
    for (const label of [
      "Quick Add",
      "Browser Login",
      "User:Pass Login",
      "Import Cookie",
      "Import Old Account Data",
      "Create Accounts",
      "Account Generator",
      "Roblox Versions",
    ]) {
      expect(screen.getByRole("button", { name: new RegExp(`^${label}`) })).toBeInTheDocument();
    }
  });

  /**
   * `Create Accounts` cria de graça no navegador embutido (a pessoa resolve o
   * CAPTCHA); `Account Generator` **compra** contas prontas de um serviço pago
   * de terceiro. Ver docs/features/account-creation.md.
   */
  it("says which way is free and which one costs money", () => {
    renderDialog();

    const create = screen.getByRole("button", { name: /^Create Accounts/ });
    expect(create).toHaveTextContent(/free/i);
    expect(create).toHaveTextContent(/CAPTCHA/);

    const generator = screen.getByRole("button", { name: /^Account Generator/ });
    expect(generator).toHaveTextContent(/paid/i);
    expect(generator).toHaveTextContent(/BloxGen/);
  });

  it("closes from the X button", async () => {
    const { onClose } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps the dialog open when the panel itself is clicked", async () => {
    const { onClose } = renderDialog();
    await userEvent.click(screen.getByText("Choose how to add an account"));
    expect(onClose).not.toHaveBeenCalled();
  });

  /**
   * O mesmo defeito do Quick Add da toolbar: `includes(COOKIE_MARKER)` mandava
   * a linha `username:password:cookie` inteira como cookie (senha no cabeçalho
   * de cookie, "Invalid cookie" de volta), e `usuario:senha` ia para a busca de
   * usuário. As duas portas agora leem a linha com `parseImportLine`.
   */
  it("manda só o cookie de uma linha username:password:cookie e guarda a senha", async () => {
    promptAnswers.prompt = `alt_one:hunter2:${COOKIE}`;
    const { store } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() => expect(store.addAccountByCookie).toHaveBeenCalledWith(COOKIE, "hunter2"));
    expect(invokeMock).not.toHaveBeenCalledWith("lookup_user", expect.anything());
  });

  it("não procura usuario:senha como nome de usuário", async () => {
    promptAnswers.prompt = "alt_one:hunter2";
    const { store } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    await waitFor(() => expect(store.addToast).toHaveBeenCalled());
    expect(invokeMock).not.toHaveBeenCalledWith("lookup_user", expect.anything());
    expect(store.addAccountByCookie).not.toHaveBeenCalled();
    const message = (store.addToast as ReturnType<typeof vi.fn>).mock.calls[0][0] as string;
    expect(message).toContain("User:Pass Login");
    expect(message).not.toContain("hunter2");
  });

  it("adds a pasted cookie without touching the user lookup", async () => {
    promptAnswers.prompt = COOKIE;
    const { store, onClose } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    expect(onClose).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(store.addAccountByCookie).toHaveBeenCalledWith(COOKIE));
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("looks a plain username up and registers it without a cookie", async () => {
    promptAnswers.prompt = "  roboduck ";
    setInvokeMap({ lookup_user: { id: 77, name: "roboduck" } });
    const { store } = renderDialog();
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
    // Conta achada por nome entra sem cookie: nao tem sessao e nao lanca.
    // "Added roboduck" fazia parecer sucesso — o mesmo aviso ja corrigido no
    // Quick Add da toolbar tem que valer aqui.
    await waitFor(() =>
      expect(store.addToast).toHaveBeenCalledWith(
        "Added roboduck with no session — paste its cookie or use Browser Login to sign in"
      )
    );
    expect(store.loadAccounts).toHaveBeenCalled();
  });

  /**
   * O Quick Add aceita o cookie `.ROBLOSECURITY`, mas o pedido era só "Cookie
   * or username" — nem o nome do cookie, nem onde achá-lo. É o mesmo texto do
   * Quick Add da toolbar.
   */
  it("tells the Quick Add prompt what the cookie is and where it lives", async () => {
    promptAnswers.prompt = null;
    renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));

    const message = promptMock.mock.calls[0][0] as string;
    expect(message).toContain(".ROBLOSECURITY");
    expect(message).toMatch(/DevTools/);
    expect(message).toMatch(/signs in as/i);
  });

  it("ignores a blank or cancelled Quick Add", async () => {
    promptAnswers.prompt = "   ";
    const { store } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));
    await Promise.resolve();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(store.addAccountByCookie).not.toHaveBeenCalled();
  });

  it("reports a failed lookup as a toast", async () => {
    promptAnswers.prompt = "ghost";
    setInvokeMap({
      lookup_user: () => {
        throw new Error("user not found");
      },
    });
    const { store } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Quick Add" }));
    await waitFor(() =>
      expect(store.addToast).toHaveBeenCalledWith(expect.stringContaining("user not found"))
    );
  });

  it("opens the login browser", async () => {
    const { store, onClose } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Browser Login" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(store.openLoginBrowser).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["User:Pass Login", "userpass"],
    ["Import Cookie", "cookie"],
    ["Import Old Account Data", "legacy"],
  ])("routes %s to the import dialog", async (label, tab) => {
    const { store, onClose } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: label }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(store.setImportDialogTab).toHaveBeenCalledWith(tab);
    expect(store.setImportDialogOpen).toHaveBeenCalledWith(true);
  });

  it.each([
    ["Create Accounts", "signup"],
    ["Account Generator", "provider"],
  ])("opens the generator dialog on the %s tab", async (label, tab) => {
    const { store, onClose } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: new RegExp(`^${label}`) }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(store.openGeneratorDialog).toHaveBeenCalledWith(tab);
  });

  it("opens Quick Login (approve a code on another device)", async () => {
    const { store, onClose } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: /^Quick Login/ }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(store.setQuickLoginOpen).toHaveBeenCalledWith(true);
  });

  it("opens the versions dialog", async () => {
    const { store, onClose } = renderDialog();
    await userEvent.click(screen.getByRole("button", { name: "Roblox Versions" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(store.setVersionsDialogOpen).toHaveBeenCalledWith(true);
  });
});
