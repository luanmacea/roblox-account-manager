import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect, type ReactNode } from "react";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/event", async () => (await import("../../test-utils/tauriMocks")).tauriEventMock());
vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(async () => {}),
  disable: vi.fn(async () => {}),
}));
// O gerador pago (BloxGen) vem desligado (`ENABLE_ACCOUNT_GENERATOR`); estes
// testes cobrem o gerador ligado. Desligado: accountGeneratorHidden.test.tsx.
vi.mock("../../featureFlags", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../featureFlags")>()),
  ENABLE_ACCOUNT_GENERATOR: true,
}));

import { GeneralTab } from "./GeneralTab";
import { DeveloperTab } from "./DeveloperTab";
import { SettingsPage } from "../pages/SettingsPage";
import { IsolationTab } from "./IsolationTab";
import { WebServerTab } from "./WebServerTab";
import { WatcherTab } from "./WatcherTab";
import { OptimizationTab } from "./OptimizationTab";
import { VersionsTab } from "./VersionsTab";
import { useSettings, type UseSettingsReturn } from "../../hooks/useSettings";
import { setStore } from "../../test-utils/renderWithStore";
import { invokeMock, resetTauriMocks, setInvokeHandler } from "../../test-utils/tauriMocks";
import { ENABLE_WEBSERVER } from "../../featureFlags";
import type { PlatformCapabilities } from "../../types";
import i18n from "../../i18n";
import { walkTour } from "../../test-utils/tourHelpers";

/**
 * Settings tabs receive the real `useSettings()` object, so every assertion
 * below goes through the same debounce + `update_setting` path the app uses.
 */
function SettingsHarness({ children }: { children: (s: UseSettingsReturn) => ReactNode }) {
  const s = useSettings();
  useEffect(() => {
    void s.load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // SettingsPage mounts the tabs only once settings are loaded, and some tabs
  // (IsolationTab) read settings in a mount effect — mirror that here.
  if (!s.loaded) return <div>Loading settings...</div>;
  return <>{children(s)}</>;
}

let stored: Record<string, Record<string, string>> = {};

function renderTab(children: (s: UseSettingsReturn) => ReactNode) {
  render(<SettingsHarness>{children}</SettingsHarness>);
}

/** Waits for the debounced save and asserts the exact backend payload. */
async function expectSaved(section: string, key: string, value: string) {
  await waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("update_setting", { section, key, value })
  );
}

beforeEach(async () => {
  resetTauriMocks();
  // The GeneralTab language picker switches i18n globally; reset it per test.
  await i18n.changeLanguage("en");
  stored = {};
  setStore({});
  setInvokeHandler((cmd) => {
    switch (cmd) {
      case "get_all_settings":
        return stored;
      case "get_web_server_status":
        return { running: false, port: 0 };
      case "isolation_list_adapters":
        return [];
      // Usados so pela SettingsPage inteira, que monta todas as abas de uma vez.
      case "versions_list_installed":
        return [];
      case "remembered_unlock_state":
        return { remembered: false, expiresAt: null };
      case "list_backups":
        return [];
      case "backups_info":
        return { dir: "C:/data", portable: false, totalBytes: 0, count: 0 };
      default:
        return undefined;
    }
  });
});

afterEach(cleanup);

describe("WebServerTab", () => {
  function renderWebServer(initial: Record<string, Record<string, string>> = { Developer: { DevMode: "true" } }) {
    stored = initial;
    renderTab((s) => <WebServerTab s={s} />);
  }

  it("stays locked until Developer Mode or the web server is enabled", async () => {
    renderWebServer({});
    expect(await screen.findByText("Web Server is off")).toBeInTheDocument();
    expect(screen.queryByText("Allow GetCookie")).not.toBeInTheDocument();
  });

  /**
   * A aba deixou de ser escondida (SettingsPage), entao o estado bloqueado e
   * a unica coisa que explica a funcionalidade: tem que dizer o QUE o servidor
   * faz e ONDE se liga, senao trocamos um recurso invisivel por uma tela muda.
   */
  it("says what the web server does and where to turn it on while locked", async () => {
    renderWebServer({});
    expect(
      await screen.findByText(
        "It serves a local HTTP API so external tools and scripts can list your accounts, read their cookies and launch them."
      )
    ).toBeInTheDocument();
    expect(
      screen.getByText("Turn on Enable Web Server in the Developer tab to unlock these settings.")
    ).toBeInTheDocument();
  });

  it("unlocks from the EnableWebServer flag alone", async () => {
    renderWebServer({ Developer: { EnableWebServer: "true" } });
    expect(await screen.findByText("Allow GetCookie")).toBeInTheDocument();
  });

  it.each([
    ["Every Request Requires Password", "EveryRequestRequiresPassword"],
    ["Allow GetCookie", "AllowGetCookie"],
    ["Allow GetAccounts", "AllowGetAccounts"],
    ["Allow LaunchAccount", "AllowLaunchAccount"],
    ["Allow Account Editing", "AllowAccountEditing"],
    ["Allow External Connections", "AllowExternalConnections"],
  ])("saves WebServer.%s as a boolean", async (label, key) => {
    renderWebServer();
    await userEvent.click(await screen.findByText(label));
    await expectSaved("WebServer", key, "true");
  });

  it("turns a permission back off", async () => {
    renderWebServer({ Developer: { DevMode: "true" }, WebServer: { AllowGetCookie: "true" } });
    await userEvent.click(await screen.findByText("Allow GetCookie"));
    await expectSaved("WebServer", "AllowGetCookie", "false");
  });

  it("strips non-alphanumeric characters from the password", async () => {
    renderWebServer();
    const field = (await screen.findByText("Password")).parentElement?.querySelector("input");
    await userEvent.type(field as HTMLInputElement, "a");
    await expectSaved("WebServer", "Password", "a");

    await userEvent.type(field as HTMLInputElement, "!");
    // The invalid character never reaches the backend.
    expect(invokeMock).not.toHaveBeenCalledWith("update_setting", {
      section: "WebServer",
      key: "Password",
      value: "a!",
    });
  });

  /**
   * O middleware devolve 401 para QUALQUER requisicao quando a senha tem menos
   * de 6 caracteres (api/server/middleware.rs:53). A tela deixava salvar "a" e
   * o usuario ficava com um servidor que recusa tudo, sem pista do motivo.
   */
  it("warns that a password under 6 characters blocks every request", async () => {
    renderWebServer({ Developer: { DevMode: "true" }, WebServer: { Password: "abc" } });
    expect(
      await screen.findByText("Too short: the server answers 401 to everything until it has 6 characters.")
    ).toBeInTheDocument();
  });

  it("drops the warning once the password is long enough", async () => {
    renderWebServer({ Developer: { DevMode: "true" }, WebServer: { Password: "abcdef" } });
    await screen.findByText("Password");
    expect(
      screen.queryByText("Too short: the server answers 401 to everything until it has 6 characters.")
    ).not.toBeInTheDocument();
  });

  it("starts and stops the server", async () => {
    renderWebServer();
    await userEvent.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("start_web_server"));
  });

  it("reports the running port", async () => {
    setInvokeHandler((cmd) => {
      if (cmd === "get_all_settings") return { Developer: { DevMode: "true" } };
      if (cmd === "get_web_server_status") return { running: true, port: 7963 };
      return undefined;
    });
    renderTab((s) => <WebServerTab s={s} />);
    expect(await screen.findByText("Running on port 7963")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
  });
});

describe("IsolationTab", () => {
  function renderIsolation(initial: Record<string, Record<string, string>> = {}) {
    stored = initial;
    renderTab((s) => <IsolationTab s={s} />);
  }

  it("switches the isolation mode to Full", async () => {
    renderIsolation();
    await userEvent.click(await screen.findByText("Full"));
    await expectSaved("Isolation", "Mode", "Full");
  });

  it("switches the isolation mode back to Off", async () => {
    renderIsolation({ Isolation: { Mode: "Full" } });
    await userEvent.click(await screen.findByText("Off"));
    await expectSaved("Isolation", "Mode", "Off");
  });

  it("migrates a legacy light/medium mode to Full on mount", async () => {
    renderIsolation({ Isolation: { Mode: "medium" } });
    await expectSaved("Isolation", "Mode", "Full");
  });

  it("leaves a valid mode untouched on mount", async () => {
    renderIsolation({ Isolation: { Mode: "Off" } });
    await screen.findByText("Full");
    expect(invokeMock).not.toHaveBeenCalledWith("update_setting", expect.anything());
  });

  it.each([
    ["Rotate MachineGuid", "SpoofMachineGuid"],
    ["Rotate MAC address", "SpoofMacAddress"],
  ])("saves Isolation.%s", async (label, key) => {
    renderIsolation();
    await userEvent.click(await screen.findByText(label));
    await expectSaved("Isolation", key, "true");
  });

  /**
   * O modo Full apaga arquivos e chaves do registro antes de cada launch. A
   * previa (dry-run) e a restauracao dos identificadores sao as duas redes de
   * seguranca do usuario: nenhuma pode depender de descobrir um "Advanced".
   */
  it("offers the wipe preview without expanding Advanced", async () => {
    renderIsolation({ Isolation: { Mode: "Full" } });
    await userEvent.click(
      await screen.findByRole("button", { name: "Preview what gets wiped" })
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("isolation_dry_run", expect.anything())
    );
  });

  it("offers the identifier restore without expanding Advanced", async () => {
    renderIsolation({ Isolation: { BackupMachineGuid: "{original-guid}" } });
    expect(
      await screen.findByRole("button", { name: "Restore original network identifiers" })
    ).toBeEnabled();
  });

  it("keeps the preservation switches behind Advanced", async () => {
    renderIsolation();
    expect(screen.queryByText("Preserve fast flags")).not.toBeInTheDocument();

    await userEvent.click(await screen.findByRole("button", { name: "Advanced" }));
    // Defaults to on, so the first click turns it off.
    await userEvent.click(await screen.findByText("Preserve fast flags"));
    await expectSaved("Isolation", "PreserveFastFlags", "false");
  });
});

/**
 * A aba WebServer sumia inteira sem Dev Mode: quem nao sabia que existe uma API
 * HTTP local nunca ia descobrir. A capacidade tem que ser descobrivel — o que
 * continua trancado e o conteudo, nao a aba.
 */
describe("SettingsPage sections", () => {
  it.runIf(ENABLE_WEBSERVER)("lists the WebServer tab even without Developer Mode", async () => {
    stored = {};
    render(<SettingsPage active onLeave={() => {}} />);
    expect(await screen.findByRole("button", { name: "WebServer" })).toBeInTheDocument();
  });

  it.runIf(ENABLE_WEBSERVER)("opens the WebServer tab on its locked explanation", async () => {
    stored = {};
    render(<SettingsPage active onLeave={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "WebServer" }));
    expect(await screen.findByText("Web Server is off")).toBeVisible();
  });

  /**
   * "Generator" sozinho nao diz qual das duas funcoes de criar conta e esta.
   * O nome canonico da paga, usado no menu Add e no dialogo, e
   * `Account Generator` — a aba tem que bater com ele.
   */
  it("names the generator tab like the rest of the app does", async () => {
    stored = {};
    render(<SettingsPage active onLeave={() => {}} />);
    expect(await screen.findByRole("button", { name: "Account Generator" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Generator" })).not.toBeInTheDocument();
  });

  /**
   * As nove abas mal cabiam numa linha do modal de 780 px. Na página elas são
   * uma lista vertical à esquerda, com a seção atual marcada.
   */
  /**
   * Backup era um diálogo aberto por um botão "Manage" escondido em Misc > Data.
   * Agora é uma seção própria da página, com o conteúdo inline.
   */
  it("has a Backups section that shows the backups inline, without a dialog", async () => {
    stored = {};
    render(<SettingsPage active onLeave={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Backups" }));

    expect(screen.getByRole("heading", { level: 2, name: "Backups" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Create backup" })).toBeVisible();
    expect(screen.getByText("C:/data")).toBeVisible();
    expect(document.querySelector(".fixed.inset-0")).toBeNull();
  });

  it("does not keep the old Manage button for backups in Misc", async () => {
    stored = {};
    render(<SettingsPage active onLeave={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Misc" }));
    expect(screen.queryByRole("button", { name: "Manage" })).not.toBeInTheDocument();
  });

  it("lists the sections in a vertical side navigation", async () => {
    stored = {};
    render(<SettingsPage active onLeave={() => {}} />);
    const sections = screen.getByRole("navigation", { name: "Settings sections" });
    const general = await screen.findByRole("button", { name: "General" });
    expect(sections).toContainElement(general);
    expect(general).toHaveAttribute("aria-current", "true");

    await userEvent.click(screen.getByRole("button", { name: "Watcher" }));
    expect(screen.getByRole("button", { name: "Watcher" })).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("heading", { level: 2, name: "Watcher" })).toBeInTheDocument();
  });

  it("renders nothing while another page is open", () => {
    render(<SettingsPage active={false} onLeave={() => {}} />);
    expect(screen.queryByRole("heading", { name: "Settings" })).not.toBeInTheDocument();
  });

  /** O "Done" do modal recarregava as settings na store; sair da página faz o mesmo. */
  it("tells the store the settings changed when the page is left", async () => {
    stored = {};
    const changed = vi.fn();
    const view = render(<SettingsPage active onLeave={() => {}} onSettingsChanged={changed} />);
    await screen.findByRole("button", { name: "General" });
    expect(changed).not.toHaveBeenCalled();
    view.rerender(<SettingsPage active={false} onLeave={() => {}} onSettingsChanged={changed} />);
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it("leaves the page on Escape", async () => {
    stored = {};
    const leave = vi.fn();
    render(<SettingsPage active onLeave={leave} />);
    await screen.findByRole("button", { name: "General" });
    await userEvent.keyboard("{Escape}");
    expect(leave).toHaveBeenCalledTimes(1);
  });
});

describe("GeneralTab", () => {
  const WAIT_FOR_GAME = "Start the next account once the previous one is in the game";
  const DISABLED_BY_SERIAL =
    "Not used while “Launch one account at a time” is on. It applies again when you turn that off.";

  function renderGeneral(initial: Record<string, Record<string, string>> = {}) {
    stored = initial;
    renderTab((s) => <GeneralTab s={s} />);
  }

  it.each([
    ["Auto Check for Updates", "General", "CheckForUpdates"],
    ["Launch one account at a time", "General", "AsyncJoin"],
    ["Wrap Long Names", "General", "WrapLongNames"],
    ["Disable Image Loading", "General", "DisableImages"],
    ["Multi Roblox", "General", "EnableMultiRbx"],
    ["Auto Rejoin", "General", "BottingEnabled"],
    ["Show Presence", "General", "ShowPresence"],
    ["Auto Cookie Refresh", "General", "AutoCookieRefresh"],
    ["Minimize to Tray", "General", "MinimizeToTray"],
  ])("saves the %s toggle", async (label, section, key) => {
    renderGeneral();
    await userEvent.click(await screen.findByText(label));
    await expectSaved(section, key, "true");
  });

  it.each([
    ["Persistent login profile", "PersistentProfile"],
    ["Reduce automation signals", "StealthMode"],
  ])("turns the %s login option off (it defaults to on)", async (label, key) => {
    renderGeneral();
    await userEvent.click(await screen.findByText(label));
    await expectSaved("Login", key, "false");
  });

  it("turns the aging alert off and on", async () => {
    renderGeneral({ General: { DisableAgingAlert: "true" } });
    await userEvent.click(await screen.findByText("Disable Aging Alert"));
    await expectSaved("General", "DisableAgingAlert", "false");
  });

  /**
   * O aviso antes de copiar credencial nasce **ligado**: o toggle existe para
   * quem quer desligar, entao o clique grava "false".
   */
  it("lets the credential-copy warning be turned off", async () => {
    renderGeneral();
    await userEvent.click(await screen.findByText("Warn Before Copying Credentials"));
    await expectSaved("General", "WarnOnCopyCredential", "false");
  });

  /**
   * Tamanho da interface (uiScale.ts): nasce em Automatic; a escolha grava
   * `General.InterfaceScale` e avisa o App na hora (`ram-ui-scale`), sem
   * esperar a store reler as settings ao sair da página.
   */
  it("saves the interface size and applies it right away", async () => {
    const announced: string[] = [];
    const onAnnounce = (e: Event) => announced.push(String((e as CustomEvent).detail));
    window.addEventListener("ram-ui-scale", onAnnounce);
    try {
      renderGeneral();
      expect(await screen.findByText("Auto shrinks the interface on smaller screens")).toBeInTheDocument();
      await userEvent.click(screen.getByRole("button", { name: "Interface size" }));
      await userEvent.click(await screen.findByText("90%"));
      await expectSaved("General", "InterfaceScale", "90");
      expect(announced).toEqual(["90"]);
    } finally {
      window.removeEventListener("ram-ui-scale", onAnnounce);
    }
  });

  it("shows Automatic when the interface size was never set", async () => {
    renderGeneral();
    expect(await screen.findByRole("button", { name: "Interface size" })).toHaveTextContent("Automatic");
  });

  it("saves the picked language", async () => {
    renderGeneral();
    await userEvent.click(await screen.findByText("English"));
    await userEvent.click(await screen.findByText("German"));
    await expectSaved("General", "Language", "de");
  });

  it("offers Brazilian Portuguese and saves it as pt", async () => {
    renderGeneral();
    await userEvent.click(await screen.findByText("English"));
    await userEvent.click(await screen.findByText("Portuguese (Brazil)"));
    await expectSaved("General", "Language", "pt");
  });

  it("offers Spanish and saves it as es", async () => {
    renderGeneral();
    await userEvent.click(await screen.findByText("English"));
    await userEvent.click(await screen.findByText("Spanish"));
    await expectSaved("General", "Language", "es");
  });

  it("saves the updater release channel", async () => {
    renderGeneral();
    await userEvent.click(await screen.findByText("Beta"));
    await userEvent.click(await screen.findByText("Stable"));
    await expectSaved("General", "UpdaterReleaseChannel", "stable");
  });

  it("saves the updater feature channel", async () => {
    renderGeneral();
    await userEvent.click(await screen.findByText("Standard (Non-Nexus/WebServer)"));
    await userEvent.click(await screen.findByText("Nexus + WebServer"));
    await expectSaved("General", "UpdaterFeatureChannel", "nexus-ws");
  });

  /**
   * `AsyncJoin` serializa a fila (launch.rs espera a conta anterior). O rotulo
   * "Async Launching" prometia o contrario e a descricao dizia o certo — duas
   * frases brigando na mesma linha.
   */
  it("names the serial launch toggle after what it does", async () => {
    renderGeneral();
    expect(await screen.findByText("Launch one account at a time")).toBeInTheDocument();
    expect(screen.queryByText("Async Launching")).not.toBeInTheDocument();
  });

  /** O backend nunca desce de MIN_JOIN_GAP_SECS = 8; o campo aceitava 0. */
  it("does not save a join delay the backend will ignore", async () => {
    renderGeneral({ General: { AccountJoinDelay: "20" } });
    const delay = await screen.findByLabelText("Account Join Delay");
    await userEvent.clear(delay);
    await userEvent.type(delay, "3");
    await userEvent.tab();
    await expectSaved("General", "AccountJoinDelay", "8");
  });

  /** Com o lote em serie o delay nem e lido: o campo tem que dizer isso. */
  it("disables the join delay while accounts launch one at a time", async () => {
    renderGeneral({ General: { AsyncJoin: "true" } });
    expect(await screen.findByLabelText("Account Join Delay")).toBeDisabled();
    expect(screen.getByText(DISABLED_BY_SERIAL)).toBeInTheDocument();
  });

  /**
   * "Esperar cada conta abrir" e "Esperar cada conta entrar no jogo" pareciam
   * a mesma coisa. O `AsyncJoin` (launch.rs) espera o sinal `next_account`, que
   * nada na tela manda: na prática são 2 minutos entre contas. A descrição diz
   * isso, e o outro interruptor diz que só encurta a espera do delay.
   */
  it("tells the two ways of waiting between accounts apart", async () => {
    renderGeneral();
    expect(
      await screen.findByText(
        "Leaves 2 minutes between accounts, so each one has time to load. The slowest option. Off: accounts start spaced by the delay below."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/Waits for each account to open/)).not.toBeInTheDocument();
  });

  /**
   * launch.rs (`wait_for_game_join`): a fila segue quando o log diz que a
   * conta entrou. Só no Windows, onde o log é lido; nasce ligado.
   */
  describe("Start the next account once the previous one is in the game", () => {
    let userAgent: { mockRestore: () => void } | null = null;
    beforeEach(() => {
      userAgent = vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    });
    afterEach(() => {
      userAgent?.mockRestore();
    });

    it("is on by default and turning it off saves false", async () => {
      renderGeneral();
      const toggle = (await screen.findByText(WAIT_FOR_GAME)).closest("[role=switch]");
      expect(toggle).toHaveAttribute("aria-checked", "true");
      await userEvent.click(toggle as HTMLElement);
      await expectSaved("General", "WaitForGameJoin", "false");
    });

    it("says it only shortens the delay above", async () => {
      renderGeneral();
      expect(
        await screen.findByText(
          "Doesn't wait out the whole delay above: never sooner than 8 seconds, at most 20 (or the delay, if longer)."
        )
      ).toBeInTheDocument();
    });

    /**
     * Cinza e ainda "ligado" ao lado de "não usado": a dica diz qual opção
     * manda e que volta a valer quando ela for desligada.
     */
    it("is not used while accounts launch one at a time, and says when it applies again", async () => {
      renderGeneral({ General: { AsyncJoin: "true" } });
      const toggle = (await screen.findByText(WAIT_FOR_GAME)).closest("[role=switch]");
      expect(toggle).toHaveAttribute("aria-disabled", "true");
      expect(screen.getAllByText(DISABLED_BY_SERIAL)).toHaveLength(2);
    });

    /** commands/reconnect.rs: padrão de todas as contas, nasce desligado. */
    it("has the auto-reconnect default off, and turning it on saves true", async () => {
      renderGeneral();
      const toggle = (await screen.findByText("Reconnect accounts that drop")).closest("[role=switch]");
      expect(toggle).toHaveAttribute("aria-checked", "false");
      await userEvent.click(toggle as HTMLElement);
      await expectSaved("General", "AutoReconnect", "true");
    });

    /** Descrição curta, sem "cliente", e dizendo que janela aberta pelo site fica de fora. */
    it("describes auto-reconnect in plain words", async () => {
      renderGeneral();
      expect(
        await screen.findByText(
          "Reopens an account in the same game after a lost connection, a kick or a crash. Only windows MultiAlt opened, never ones opened from the website. Each account can change this in the In game list of the Session page."
        )
      ).toBeInTheDocument();
    });

    /** commands/keep_awake.rs: nasce ligado; desligar grava false. O rótulo cobre a reconexão também. */
    it("keeps the PC awake by default, and turning it off saves false", async () => {
      renderGeneral();
      const toggle = (await screen.findByText("Keep the PC awake while accounts are kept in game")).closest(
        "[role=switch]"
      );
      expect(toggle).toHaveAttribute("aria-checked", "true");
      expect(
        screen.getByText(
          "Windows won't go to sleep while AFK Mode, Auto Rejoin or auto-reconnect is running. The screen can still turn off."
        )
      ).toBeInTheDocument();
      await userEvent.click(toggle as HTMLElement);
      await expectSaved("General", "KeepPcAwake", "false");
    });

    /** Ideia 3: experimental, nasce desligado e só vale com o Multi Roblox. */
    it("has the experimental teleport protection off and locked until Multi Roblox is on", async () => {
      renderGeneral();
      const label = "Experimental: keep clients open across teleports";
      const off = (await screen.findByText(label)).closest("[role=switch]");
      expect(off).toHaveAttribute("aria-checked", "false");
      expect(off).toHaveAttribute("aria-disabled", "true");
      cleanup();

      renderGeneral({ General: { EnableMultiRbx: "true" } });
      const toggle = (await screen.findByText(label)).closest("[role=switch]");
      expect(toggle).toHaveAttribute("aria-checked", "false");
      await userEvent.click(toggle as HTMLElement);
      await expectSaved("General", "ReserveSingletonEvent", "true");
    });
  });

  it("registers the app with the OS autostart when Run on Windows Startup is turned on", async () => {
    const autostart = await import("@tauri-apps/plugin-autostart");
    renderGeneral();
    await userEvent.click(await screen.findByText("Run on Windows Startup"));
    await expectSaved("General", "StartOnPCStartup", "true");
    expect(autostart.enable).toHaveBeenCalledTimes(1);
  });

  it("saves a custom browser executable path", async () => {
    renderGeneral();
    const field = await screen.findByLabelText("Custom browser executable");
    await userEvent.type(field, "C:\\browsers\\chrome.exe");
    await userEvent.tab();
    await expectSaved("Login", "ManualBinaryPath", "C:\\browsers\\chrome.exe");
  });

  /**
   * O botão do navegador embutido chama `store.ensureBrowserDownload`
   * (`chromium/download.rs` → `ensure_browser`), não `update_setting`: é o
   * único controle desta aba que dispara um download em vez de gravar settings.
   */
  it("downloads the bundled browser through the store", async () => {
    const store = setStore({});
    renderGeneral();
    await userEvent.click(await screen.findByRole("button", { name: "Download" }));
    expect(store.ensureBrowserDownload).toHaveBeenCalledTimes(1);
  });

  it("labels the button Reinstall once the bundled browser is already installed and forces a fresh download", async () => {
    setInvokeHandler((cmd) => {
      if (cmd === "is_browser_ready") return true;
      if (cmd === "get_all_settings") return stored;
      return undefined;
    });
    const store = setStore({});
    renderGeneral();
    const button = await screen.findByRole("button", { name: "Reinstall" });
    await userEvent.click(button);
    expect(store.ensureBrowserDownload).toHaveBeenCalledWith(true);
  });

  it("shows the download progress and disables the button while it runs", async () => {
    setStore({
      browserDownload: { active: true, stage: "downloading", percent: 42, error: null },
    });
    renderGeneral();
    expect(await screen.findByText("Downloading browser (42%)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Downloading..." })).toBeDisabled();
  });

  it("shows the backend error message when the bundled browser download fails", async () => {
    setStore({
      browserDownload: { active: false, stage: "error", percent: null, error: "network down" },
    });
    renderGeneral();
    expect(await screen.findByText("network down")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry Download" })).toBeInTheDocument();
  });
});

describe("VersionsTab", () => {
  /**
   * A guarda de versão recusa abrir uma conta numa versão diferente das que
   * já estão abertas; com duas versões abertas nenhum launch passa. O toggle
   * libera abrir numa versão que já tem cliente aberto — desligado por padrão.
   */
  it("liga a opção de abrir numa versão que já está aberta", async () => {
    stored = {};
    renderTab((s) => <VersionsTab s={s} />);
    await userEvent.click(await screen.findByText("Allow launching on an already open version"));
    await expectSaved("Versions", "AllowLaunchOnOpenVersion", "true");
  });
});

describe("WatcherTab", () => {
  function renderWatcher(os: string) {
    stored = {};
    setStore({ platformCapabilities: { os } as PlatformCapabilities });
    renderTab((s) => <WatcherTab s={s} />);
  }

  /**
   * `ReadInterval` so e lido dentro de `#[cfg(target_os = "macos")]`
   * (commands/watcher.rs). No Windows o campo era decoracao.
   */
  it("hides Read Interval on Windows, where nothing reads it", async () => {
    renderWatcher("windows");
    expect(await screen.findByText("Scan Interval")).toBeInTheDocument();
    expect(screen.queryByText("Read Interval")).not.toBeInTheDocument();
  });

  it("keeps Read Interval on macOS", async () => {
    renderWatcher("macos");
    expect(await screen.findByText("Read Interval")).toBeInTheDocument();
  });
});

describe("DeveloperTab", () => {
  function renderDeveloper(initial: Record<string, Record<string, string>> = {}) {
    stored = initial;
    renderTab((s) => <DeveloperTab s={s} />);
  }

  /**
   * O bloco da previa do modal era texto solto no JSX: nao passava por `t()`,
   * entao ficava em ingles com o catalogo inteiro traduzido.
   */
  it("mostra o bloco da prévia do modal traduzido", async () => {
    await i18n.changeLanguage("pt");
    try {
      renderDeveloper();
      expect(await screen.findByText("Prévia do modal de atualização")).toBeInTheDocument();
      expect(screen.getByText("Abrir a prévia")).toBeInTheDocument();
      expect(screen.queryByText("Update Modal Preview")).not.toBeInTheDocument();
    } finally {
      await i18n.changeLanguage("en");
    }
  });

  it("saves the Developer Mode toggle", async () => {
    renderDeveloper();
    await userEvent.click(await screen.findByText("Enable Developer Mode"));
    await expectSaved("Developer", "DevMode", "true");
  });

  it.runIf(ENABLE_WEBSERVER)("saves the web server toggle", async () => {
    renderDeveloper();
    await userEvent.click(await screen.findByText("Enable Web Server"));
    await expectSaved("Developer", "EnableWebServer", "true");
  });

  it("opens the update preview from the store", async () => {
    const store = setStore({});
    renderDeveloper();
    await userEvent.click(await screen.findByRole("button", { name: "Open Preview" }));
    expect(store.openUpdatePreviewDialog).toHaveBeenCalledTimes(1);
  });

  it.each([
    [{ holder: "free", robloxPids: [], legacyRamPids: [] }, "Free (no process holds the mutex)"],
    [
      { holder: "thisProcess", robloxPids: [], legacyRamPids: [] },
      "This Account Manager (Multi-Roblox active)",
    ],
    [
      { holder: "roblox", robloxPids: [11, 22], legacyRamPids: [] },
      "Running Roblox client(s): 11, 22",
    ],
    [
      { holder: "legacyRam", robloxPids: [], legacyRamPids: [99] },
      "Legacy Roblox Account Manager: PID 99",
    ],
  ])("explains who holds the Roblox mutex (%#)", async (diagnosis, expected) => {
    setInvokeHandler((cmd) => {
      if (cmd === "get_all_settings") return {};
      if (cmd === "diagnose_mutex_holder") return { ...diagnosis, thisProcessHolds: false };
      return undefined;
    });
    renderTab((s) => <DeveloperTab s={s} />);

    await userEvent.click(await screen.findByRole("button", { name: "Diagnose mutex holder" }));
    expect(await screen.findByText(expected)).toBeInTheDocument();
  });

  it("offers to close a legacy Account Manager that holds the mutex", async () => {
    let killed = false;
    setInvokeHandler((cmd) => {
      if (cmd === "get_all_settings") return {};
      if (cmd === "diagnose_mutex_holder")
        return killed
          ? { holder: "free", robloxPids: [], legacyRamPids: [], thisProcessHolds: false }
          : { holder: "legacyRam", robloxPids: [], legacyRamPids: [42], thisProcessHolds: false };
      if (cmd === "kill_legacy_ram_processes") {
        killed = true;
        return 1;
      }
      return undefined;
    });
    renderTab((s) => <DeveloperTab s={s} />);

    await userEvent.click(await screen.findByRole("button", { name: "Diagnose mutex holder" }));
    await userEvent.click(
      await screen.findByRole("button", { name: "Close legacy Account Manager" })
    );

    expect(await screen.findByText("Free (no process holds the mutex)")).toBeInTheDocument();
  });
});

/**
 * A allowlist de fast flags mora no Rust (`WINDOWS_FASTFLAG_ALLOWLIST`,
 * platform/windows/optimization.rs). O backend recusa o JSON inteiro se
 * qualquer chave estiver fora dela — e `launch_shared.rs` engole o erro num
 * `eprintln!`. A tela precisa recusar na hora o que o launch vai recusar
 * depois, senao o toggle fica ligado e nada e aplicado, em silencio.
 */
describe("OptimizationTab", () => {
  function renderOptimization(
    initial: Record<string, Record<string, string>> = {},
    os = "windows"
  ) {
    stored = initial;
    setStore({ platformCapabilities: { os } as PlatformCapabilities });
    renderTab((s) => <OptimizationTab s={s} />);
  }

  const FAST_FLAGS_ON = {
    Optimization: { NormalEnableFastFlags: "true" },
  } as Record<string, Record<string, string>>;

  /** A grade automática fica junto do tamanho de janela global, que é o da célula. */
  it("turns the automatic window grid off next to the window size", async () => {
    renderOptimization({ General: { AutoArrangeGrid: "true" } });
    const toggle = await screen.findByRole("switch", { name: /Arrange in grid on launch/ });
    expect(toggle).toHaveAttribute("aria-checked", "true");

    await userEvent.click(toggle);
    await expectSaved("General", "AutoArrangeGrid", "false");
  });

  it("shows the automatic window grid on when it was never saved", async () => {
    renderOptimization({});
    const toggle = await screen.findByRole("switch", { name: /Arrange in grid on launch/ });
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  /** Ideia 22: janelas da grade menores que o mínimo e sem moldura, opcionais. */
  it.each([
    ["Allow smaller windows in the grid", "GridAllowSmallWindows"],
    ["Remove window borders in the grid", "GridBorderless"],
  ])("offers %s off by default and saves it", async (label, key) => {
    renderOptimization({});
    const toggle = await screen.findByRole("switch", { name: new RegExp(label) });
    expect(toggle).toHaveAttribute("aria-checked", "false");

    await userEvent.click(toggle);
    await expectSaved("General", key, "true");
  });

  it("hides the grid window options outside Windows", async () => {
    renderOptimization({}, "macos");
    await screen.findByText("Override Window Size");
    expect(
      screen.queryByRole("switch", { name: /Allow smaller windows in the grid/ })
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("switch", { name: /Remove window borders in the grid/ })
    ).not.toBeInTheDocument();
  });

  it("hides the automatic window grid outside Windows", async () => {
    renderOptimization({}, "macos");
    await screen.findByText("Override Window Size");
    expect(screen.queryByRole("switch", { name: /Arrange in grid on launch/ })).not.toBeInTheDocument();
  });

  /** Ideia 18: a janela em uso a toda velocidade, as outras no fundo. Opcional. */
  it("offers the focus-following optimization off by default and saves it", async () => {
    renderOptimization({});
    const toggle = await screen.findByRole("switch", { name: /Follow the window in use/ });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(
      screen.getByText(/The one you're playing runs at full speed, the others slow down/)
    ).toBeInTheDocument();

    await userEvent.click(toggle);
    await expectSaved("Optimization", "FollowFocus", "true");
  });

  /** Ideia 20: só aparece quando o binário traz a feature `live-audio`. */
  it("offers muting the windows not in use only when the build has live audio", async () => {
    stored = {};
    setStore({
      platformCapabilities: { os: "windows", supportsLiveAudio: true } as PlatformCapabilities,
    });
    renderTab((s) => <OptimizationTab s={s} />);
    const toggle = await screen.findByRole("switch", {
      name: /Mute the Roblox windows you're not using/,
    });
    expect(toggle).toHaveAttribute("aria-checked", "false");

    await userEvent.click(toggle);
    await expectSaved("Optimization", "MuteBackgroundClients", "true");
  });

  /** Teto de memória: o padrão de todos os clientes, só com a feature `memory-trim`. */
  it("offers the memory limit per client only when the build can free memory", async () => {
    stored = {};
    setStore({
      platformCapabilities: { os: "windows", supportsMemoryTrim: true } as PlatformCapabilities,
    });
    renderTab((s) => <OptimizationTab s={s} />);
    const field = await screen.findByLabelText("Memory limit per client");
    expect(field).toHaveValue("0");
    expect(screen.getByText(/asks Windows to free the client's memory first/)).toBeInTheDocument();
  });

  it("hides the memory limit when the build cannot free memory", async () => {
    renderOptimization({});
    await screen.findByRole("switch", { name: /Follow the window in use/ });
    expect(screen.queryByLabelText("Memory limit per client")).not.toBeInTheDocument();
  });

  it("hides the mute option when the build has no live audio", async () => {
    renderOptimization({});
    await screen.findByRole("switch", { name: /Follow the window in use/ });
    expect(
      screen.queryByRole("switch", { name: /Mute the Roblox windows you're not using/ })
    ).not.toBeInTheDocument();
  });

  /**
   * Revisão no harness (10/10/2026): "While you play" (vale na hora) ficava
   * entre o cabeçalho dos perfis (vale no próximo launch) e o card do perfil,
   * separando o seletor de perfil das opções que ele controla.
   */
  it("puts the 'While you play' box above the per-profile settings", async () => {
    renderOptimization({});
    const whilePlaying = await screen.findByText("While you play");
    const profiles = screen.getByText("Optimization Profiles");
    expect(
      whilePlaying.compareDocumentPosition(profiles) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  /** "35 s" quebrava entre o número e a unidade na janela estreita. */
  it("keeps '35 s' on one line", async () => {
    renderOptimization({});
    // O normalizador padrão do testing-library troca o espaço rígido por um
    // comum: confere o texto cru.
    const description = await screen.findByText(/to load first/);
    expect(description.textContent).toContain("35 s");
  });

  /** As duas opções também valem para o botão Arrange in grid, mesmo sem a grade no launch. */
  it.each([
    "Allow smaller windows in the grid",
    "Remove window borders in the grid",
  ])("says %s also applies to the Arrange in grid button", async (label) => {
    renderOptimization({});
    const toggle = await screen.findByRole("switch", { name: new RegExp(label) });
    expect(toggle.closest("label") ?? toggle.parentElement?.parentElement).toHaveTextContent(
      /Arrange in grid button/
    );
  });

  /** A grade é uma só para todos os perfis: com perfis separados, o card diz isso. */
  it("says the grid options are shared when Auto Rejoin profiles are split", async () => {
    renderOptimization({
      General: { BottingEnabled: "true", BottingUseSharedClientProfile: "false" },
    });
    await screen.findByRole("switch", { name: /Arrange in grid on launch/ });
    expect(screen.getByText(/Grid options are the same for every profile/)).toBeInTheDocument();
  });

  it("does not mention profiles in the grid when there is only one", async () => {
    renderOptimization({});
    await screen.findByRole("switch", { name: /Arrange in grid on launch/ });
    expect(screen.queryByText(/Grid options are the same for every profile/)).not.toBeInTheDocument();
  });

  /** Ideia 21: devolver ao fechar o que o launch mudou nos arquivos do Roblox. */
  it("offers giving the Roblox settings back on close, off by default, and saves it", async () => {
    renderOptimization({});
    const toggle = await screen.findByRole("switch", {
      name: /Restore Roblox settings when MultiAlt closes/,
    });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(/the game you open from the website/)).toBeInTheDocument();

    await userEvent.click(toggle);
    await expectSaved("General", "RestoreRobloxSettingsOnExit", "true");
  });

  it("hides giving the Roblox settings back outside Windows", async () => {
    renderOptimization({}, "macos");
    await screen.findByText("Override Window Size");
    expect(
      screen.queryByRole("switch", { name: /Restore Roblox settings when MultiAlt closes/ })
    ).not.toBeInTheDocument();
  });

  it("hides the focus-following optimization outside Windows", async () => {
    renderOptimization({}, "macos");
    await screen.findByText("Override Window Size");
    expect(screen.queryByRole("switch", { name: /Follow the window in use/ })).not.toBeInTheDocument();
  });

  it("rejects a fast flag key that is not on the backend allowlist", async () => {
    renderOptimization({
      Optimization: {
        NormalEnableFastFlags: "true",
        NormalFastFlagsJson: '{"FFlagMadeUpByTheUser": true}',
      },
    });
    expect(
      await screen.findByText(
        "Only Roblox allowlisted keys are accepted: FFlagMadeUpByTheUser"
      )
    ).toBeInTheDocument();
  });

  it("accepts a JSON whose keys are all on the allowlist", async () => {
    renderOptimization({
      Optimization: {
        NormalEnableFastFlags: "true",
        NormalFastFlagsJson: '{"DFIntTextureQualityOverride": 0}',
      },
    });
    await screen.findByLabelText("Allowlisted fast flags JSON");
    expect(
      screen.queryByText(/Only Roblox allowlisted keys are accepted:/)
    ).not.toBeInTheDocument();
  });

  it("shows which keys the backend accepts instead of making the user guess", async () => {
    renderOptimization(FAST_FLAGS_ON);
    expect(await screen.findByText("DFFlagTextureQualityOverrideEnabled")).toBeInTheDocument();
    expect(screen.getByText("DFIntRenderShadowIntensity")).toBeInTheDocument();
  });

  /**
   * Campo numerico/texto editavel com o interruptor que o governa desligado:
   * o valor e salvo e o backend nunca o le.
   */
  it.each([
    ["Max FPS", "NormalUnlockFPS"],
    ["Client Volume", "NormalOverrideClientVolume"],
    ["Graphics Level", "NormalOverrideClientGraphics"],
    ["Window Width", "NormalOverrideClientWindowSize"],
    ["Window Height", "NormalOverrideClientWindowSize"],
  ])("disables %s while its General switch is off", async (label) => {
    renderOptimization();
    expect(await screen.findByLabelText(label)).toBeDisabled();
  });

  it.each([
    ["Max FPS", "UnlockFPS"],
    ["Client Volume", "OverrideClientVolume"],
    ["Graphics Level", "OverrideClientGraphics"],
    ["Window Width", "OverrideClientWindowSize"],
    ["Window Height", "OverrideClientWindowSize"],
  ])("enables %s once its General switch is on", async (label, key) => {
    renderOptimization({ General: { [key]: "true" } });
    expect(await screen.findByLabelText(label)).toBeEnabled();
  });

  it.each([
    ["Apply delay", "NormalEnableProcessPolicy"],
    ["CPU limit", "NormalEnableJobCpuLimit"],
    ["Process memory limit", "NormalEnableJobMemoryLimit"],
  ])("disables %s while its Optimization switch is off", async (label, key) => {
    renderOptimization();
    expect(await screen.findByLabelText(label)).toBeDisabled();

    cleanup();
    renderOptimization({ Optimization: { [key]: "true" } });
    expect(await screen.findByLabelText(label)).toBeEnabled();
  });

  it.each([["Priority Class"], ["Memory Priority"]])(
    "disables the %s picker while process optimization is off",
    async (label) => {
      renderOptimization();
      expect(await screen.findByRole("button", { name: label })).toBeDisabled();

      cleanup();
      renderOptimization({ Optimization: { NormalEnableProcessPolicy: "true" } });
      expect(await screen.findByRole("button", { name: label })).toBeEnabled();
    }
  );

  it.each([["Background Mode"], ["EcoQoS"], ["Ignore Timer Resolution"]])(
    "ignores a click on %s while process optimization is off",
    async (label) => {
      renderOptimization();
      await userEvent.click(await screen.findByText(label));
      expect(invokeMock).not.toHaveBeenCalledWith("update_setting", expect.anything());
    }
  );

  it("disables the fast flags editor while the fast flags switch is off", async () => {
    renderOptimization();
    expect(await screen.findByLabelText("Allowlisted fast flags JSON")).toBeDisabled();
  });

  it("enables the fast flags editor once the switch is on", async () => {
    renderOptimization(FAST_FLAGS_ON);
    expect(await screen.findByLabelText("Allowlisted fast flags JSON")).toBeEnabled();
  });

  /**
   * Antes desta mudanca, Auto Rejoin ligado + perfis separados montava as 3
   * secoes de uma vez: 6357px de scroll, `Unlock FPS` 3x, e 12 aria-label
   * triplicados (Max FPS, Client Volume, Priority Class...). Um perfil por
   * vez elimina isso — so a secao escolhida existe no DOM.
   */
  describe("profile selector", () => {
    const SEPARATE_PROFILES = { General: { BottingEnabled: "true", BottingUseSharedClientProfile: "false" } };

    it("hides the selector when there is only one profile", async () => {
      renderOptimization();
      expect(screen.queryByRole("radio", { name: "Auto Rejoin Main" })).not.toBeInTheDocument();
      expect(screen.queryByRole("radio", { name: "Normal" })).not.toBeInTheDocument();
    });

    it("shows one radio per profile once Botting uses separate profiles", async () => {
      renderOptimization(SEPARATE_PROFILES);
      expect(await screen.findByRole("radio", { name: "Normal" })).toBeInTheDocument();
      expect(screen.getByRole("radio", { name: "Auto Rejoin Main" })).toBeInTheDocument();
      expect(screen.getByRole("radio", { name: "Auto Rejoin Alt" })).toBeInTheDocument();
    });

    it("mounts only the selected profile's section, never more than one", async () => {
      renderOptimization(SEPARATE_PROFILES);
      await screen.findByRole("radio", { name: "Normal" });
      // Um so "Max FPS" no DOM — com as 3 secoes montadas de uma vez isso dava 3.
      expect(screen.getAllByLabelText("Max FPS")).toHaveLength(1);
      expect(screen.getAllByText("Unlock FPS")).toHaveLength(1);
    });

    it("switches the mounted section when another profile is picked", async () => {
      renderOptimization(SEPARATE_PROFILES);
      await userEvent.click(await screen.findByRole("radio", { name: "Auto Rejoin Alt" }));
      expect(screen.getByRole("radio", { name: "Auto Rejoin Alt" })).toHaveAttribute("aria-checked", "true");
      // Continua havendo so uma secao montada apos trocar de perfil.
      expect(screen.getAllByLabelText("Max FPS")).toHaveLength(1);
    });

    /**
     * O titulo do perfil ativo nao pode depender de rolagem: o seletor mora
     * fora do fluxo que rola (`sticky`), entao ele sempre esta visivel junto
     * com o nome do perfil escolhido.
     */
    it("keeps the profile picker out of the scrolling flow", async () => {
      renderOptimization(SEPARATE_PROFILES);
      const group = await screen.findByRole("radiogroup");
      expect(group.closest(".sticky")).not.toBeNull();
    });

    it("falls back to Normal when Botting is turned back off while another profile is selected", async () => {
      renderOptimization(SEPARATE_PROFILES);
      await userEvent.click(await screen.findByRole("radio", { name: "Auto Rejoin Alt" }));
      cleanup();
      renderOptimization();
      expect(screen.queryByRole("radiogroup")).not.toBeInTheDocument();
      expect(await screen.findByLabelText("Max FPS")).toBeInTheDocument();
    });
  });
});

describe("WatcherTab dependent fields", () => {
  function renderWatcher(initial: Record<string, Record<string, string>> = {}) {
    stored = initial;
    setStore({ platformCapabilities: { os: "windows" } as PlatformCapabilities });
    renderTab((s) => <WatcherTab s={s} />);
  }

  it.each([
    ["No Connection Timeout", "ExitIfNoConnection"],
    ["Memory Threshold", "CloseRbxMemory"],
    ["Expected Title", "CloseRbxWindowTitle"],
  ])("disables %s while its switch is off", async (label, key) => {
    renderWatcher();
    expect(await screen.findByLabelText(label)).toBeDisabled();

    cleanup();
    renderWatcher({ Watcher: { [key]: "true" } });
    expect(await screen.findByLabelText(label)).toBeEnabled();
  });
});

describe("SettingsPage — tutorial", () => {
  it("walks the Settings tutorial and ends on Backups without changing a setting", async () => {
    stored = {};
    render(<SettingsPage active onLeave={() => {}} />);
    await screen.findByRole("navigation", { name: "Settings sections" });
    await walkTour("settings", { invoke: invokeMock });
    expect(screen.getByRole("heading", { level: 2, name: "Backups" })).toBeInTheDocument();
  });
});
