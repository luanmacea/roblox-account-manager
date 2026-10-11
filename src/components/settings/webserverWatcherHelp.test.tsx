import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";

vi.mock("../../store", async () => (await import("../../test-utils/renderWithStore")).storeModuleMock());
vi.mock("@tauri-apps/api/core", async () => (await import("../../test-utils/tauriMocks")).tauriCoreMock());
vi.mock("@tauri-apps/api/event", async () => (await import("../../test-utils/tauriMocks")).tauriEventMock());

import { WebServerTab } from "./WebServerTab";
import { WatcherTab } from "./WatcherTab";
import { useSettings, type UseSettingsReturn } from "../../hooks/useSettings";
import { setStore } from "../../test-utils/renderWithStore";
import { resetTauriMocks, setInvokeHandler } from "../../test-utils/tauriMocks";
import type { PlatformCapabilities } from "../../types";
import i18n from "../../i18n";

/**
 * P2 "esta na tela e nao se explica": as duas abas expoem capacidades caras
 * (uma API HTTP que serve cookies; um loop que FECHA clientes) sem dizer o que
 * acontece. Cada asserção aqui e uma frase que o backend comprova.
 */
function SettingsHarness({ children }: { children: (s: UseSettingsReturn) => ReactNode }) {
  const s = useSettings();
  useEffect(() => {
    void s.load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (!s.loaded) return <div>Loading settings...</div>;
  return <>{children(s)}</>;
}

let stored: Record<string, Record<string, string>> = {};

function renderTab(children: (s: UseSettingsReturn) => ReactNode) {
  render(<SettingsHarness>{children}</SettingsHarness>);
}

beforeEach(async () => {
  resetTauriMocks();
  await i18n.changeLanguage("en");
  stored = {};
  setStore({});
  setInvokeHandler((cmd) => {
    switch (cmd) {
      case "get_all_settings":
        return stored;
      case "get_web_server_status":
        return { running: false, port: 0 };
      default:
        return undefined;
    }
  });
});

afterEach(cleanup);

describe("WebServerTab permissions explain what they expose", () => {
  function renderWebServer(initial: Record<string, Record<string, string>> = { Developer: { DevMode: "true" } }) {
    stored = initial;
    renderTab((s) => <WebServerTab s={s} />);
  }

  /** `external_check` (api/server/middleware.rs:36-59) so libera `/Running`. */
  it("says the password switch gates every endpoint but /Running", async () => {
    renderWebServer();
    expect(
      await screen.findByText(
        "Every endpoint except /Running refuses to answer without the password in the URL."
      )
    ).toBeInTheDocument();
  });

  /**
   * `handle_get_cookie` devolve `account.security_token` cru
   * (api/server/handlers_basic.rs:140-167) e `handle_get_accounts_json` embute
   * o mesmo cookie com `IncludeCookies=true` (handlers_basic.rs:50-56).
   * Isto nao pode ser um sussurro cinza igual aos outros.
   */
  it("spells out that GetCookie hands over the account itself", async () => {
    renderWebServer();
    const warning = await screen.findByText(
      "Hands out the .ROBLOSECURITY cookie of any account: whoever reads it is logged into that Roblox account, with no password and no 2FA."
    );
    expect(warning).toBeInTheDocument();
    expect(
      screen.getByText(
        "Covers /GetCookie and GetAccountsJson with IncludeCookies=true; both always demand the web server password."
      )
    ).toBeInTheDocument();
  });

  it("does not paint the GetCookie warning the same grey as the other hints", async () => {
    renderWebServer();
    const warning = await screen.findByText(
      "Hands out the .ROBLOSECURITY cookie of any account: whoever reads it is logged into that Roblox account, with no password and no 2FA."
    );
    // O aviso mora dentro de um bloco de alerta, nao no cinza das descricoes.
    expect(warning.closest('[class*="amber"]')).not.toBeNull();
  });

  /** handlers_basic.rs:9-35/37-108 e handlers_edit.rs:1-95 (alias/desc/field). */
  it("says what GetAccounts leaks", async () => {
    renderWebServer();
    expect(
      await screen.findByText(
        "Lists every account with username, user ID, alias, description, group and custom fields. Cookies are not included."
      )
    ).toBeInTheDocument();
  });

  /** handlers_launch.rs:1-8 (/LaunchAccount) e :166-173 (/FollowUser). */
  it("says LaunchAccount can start any account anywhere", async () => {
    renderWebServer();
    expect(
      await screen.findByText(
        "Starts Roblox on any account and sends it into any place, server or after another player."
      )
    ).toBeInTheDocument();
  });

  /** handlers_edit.rs:105/150/191/231/271: SetField, RemoveField, alias, descricao. */
  it("says what Account Editing rewrites", async () => {
    renderWebServer();
    expect(
      await screen.findByText(
        "Lets callers rewrite the alias, description and custom fields this app stores for an account."
      )
    ).toBeInTheDocument();
  });

  /** runtime.rs:64-68: 0.0.0.0 em vez de 127.0.0.1. */
  it("says External Connections opens the port to the network", async () => {
    renderWebServer();
    expect(
      await screen.findByText(
        "Binds the port to every network interface instead of localhost, so other machines can reach the API."
      )
    ).toBeInTheDocument();
  });

  it("keeps the permissions list from reading as a bare list of switches", async () => {
    renderWebServer();
    expect(
      await screen.findByText(
        "Each switch below opens part of the local HTTP API to anything that can reach the port."
      )
    ).toBeInTheDocument();
  });
});

describe("WatcherTab explains the system it turns on", () => {
  function renderWatcher(
    os = "windows",
    initial: Record<string, Record<string, string>> = {}
  ) {
    stored = initial;
    setStore({ platformCapabilities: { os } as PlatformCapabilities });
    renderTab((s) => <WatcherTab s={s} />);
  }

  /**
   * O loop (commands/watcher.rs:99-312) varre so o que esta no tracker, pula a
   * janela em foreground (:134) e a unica acao e `kill_for_user` — nao relanca.
   */
  it("says what the watcher looks at and what it does about it", async () => {
    renderWatcher();
    expect(
      await screen.findByText(
        "Every few seconds the watcher checks each Roblox client this app launched and closes the ones that match a rule below."
      )
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "It never reopens them, ignores clients you started outside the app, and skips the window you are using right now."
      )
    ).toBeInTheDocument();
  });

  /** `startup_grace_secs: 30` (watcher.rs:61), valido para memoria e titulo. */
  it("says the checks only start after the client has settled", async () => {
    renderWatcher();
    expect(
      await screen.findByText(
        "Memory and window title checks only start 30 seconds after a client opens."
      )
    ).toBeInTheDocument();
  });

  it("says the master switch is what starts the loop", async () => {
    renderWatcher();
    expect(
      await screen.findByText("Starts the scan loop. With it off, nothing below runs.")
    ).toBeInTheDocument();
  });

  /** `scan_interval_ms` (watcher.rs:46) e o intervalo entre varreduras. */
  it("says what the scan interval measures", async () => {
    renderWatcher();
    expect(
      await screen.findByText("How long the watcher waits between checks of every tracked client.")
    ).toBeInTheDocument();
  });

  /** watcher.rs:214-241 (titulo desconectado) e :481-499 (log no macOS). */
  it("says when a disconnected client is closed", async () => {
    renderWatcher();
    expect(
      await screen.findByText(
        "Closes a client that reports a lost connection and stays that way past the timeout below."
      )
    ).toBeInTheDocument();
  });

  it("explains the timeout instead of only naming its switch", async () => {
    renderWatcher("windows", { Watcher: { ExitIfNoConnection: "true" } });
    expect(
      await screen.findByText("How long a client may stay disconnected before it is closed.")
    ).toBeInTheDocument();
  });

  it("keeps saying which switch a disabled timeout needs", async () => {
    renderWatcher();
    expect(await screen.findByText("Requires Exit If No Connection")).toBeInTheDocument();
  });

  /** watcher.rs:74-76 (titulo "roblox beta") e :351-354 (volta para a home). */
  it("says what Exit on Beta reacts to", async () => {
    renderWatcher();
    expect(
      await screen.findByText(
        "Closes a client that lands on the Roblox Beta app instead of staying in the game."
      )
    ).toBeInTheDocument();
  });

  /** watcher.rs:146-162: working set ABAIXO do limite, nao acima. */
  it("says the memory rule fires below the threshold", async () => {
    renderWatcher();
    expect(
      await screen.findByText(
        "Closes a client whose memory drops below the threshold, the usual sign of one that froze."
      )
    ).toBeInTheDocument();
  });

  it("explains the memory threshold instead of only naming its switch", async () => {
    renderWatcher("windows", { Watcher: { CloseRbxMemory: "true" } });
    expect(
      await screen.findByText("A client using less than this is closed.")
    ).toBeInTheDocument();
  });

  /** watcher.rs:176-196: comparacao exata, e so existe no config do Windows. */
  it("says the title rule is an exact match and Windows only", async () => {
    renderWatcher();
    expect(
      await screen.findByText(
        "Closes a client whose window title is not exactly the text below. Windows only."
      )
    ).toBeInTheDocument();
  });

  /** watcher.rs:243-302 grava Window_*; launch.rs:118-126 restaura. */
  it("says where the remembered window positions are used", async () => {
    renderWatcher();
    expect(
      await screen.findByText(
        "Saves each client's window position and size, and the next launch of that account reopens it there."
      )
    ).toBeInTheDocument();
  });

  /**
   * `ReadInterval` so e lido em `#[cfg(target_os = "macos")]` (watcher.rs:338):
   * o texto novo nao pode ter ressuscitado o campo no Windows.
   */
  it("leaves Read Interval macOS-only", async () => {
    renderWatcher();
    expect(await screen.findByText("Scan Interval")).toBeInTheDocument();
    expect(screen.queryByText("Read Interval")).not.toBeInTheDocument();

    cleanup();
    renderWatcher("macos");
    expect(await screen.findByText("Read Interval")).toBeInTheDocument();
    expect(
      screen.getByText("How often the log file is re-read while the watcher is running.")
    ).toBeInTheDocument();
  });
});
