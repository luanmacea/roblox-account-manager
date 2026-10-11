import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable } from "@tauri-apps/plugin-autostart";
import type { UseSettingsReturn } from "../../hooks/useSettings";
import { Toggle } from "../ui/Toggle";
import { NumberField } from "../ui/NumberField";
import { TextField } from "../ui/TextField";
import { Divider } from "../ui/Divider";
import { SectionLabel } from "../ui/SectionLabel";
import { WarningBadge } from "../ui/WarningBadge";
import { Select } from "../ui/Select";
import i18n, { LANGUAGE_OPTIONS, normalizeLanguage } from "../../i18n";
import { useTr } from "../../i18n/text";
import { useStore } from "../../store";
import { normalizeUiScaleSetting } from "../../uiScale";
import { announceUiScale } from "../../hooks/useUiScale";
import {
  normalizeUpdaterReleaseChannel,
  normalizeUpdaterFeatureChannel,
} from "../../updaterChannels";

/** Espelha `MIN_JOIN_GAP_SECS` em `commands/launch.rs`: abaixo disso nada muda. */
const MIN_JOIN_DELAY_SECONDS = 8;

export function GeneralTab({ s }: { s: UseSettingsReturn }) {
  const serialLaunch = s.getBool("General", "AsyncJoin");
  const t = useTr();
  const store = useStore();
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [browserReady, setBrowserReady] = useState<boolean | null>(null);
  const browserDownload = store.browserDownload;
  const browserBusy = browserDownload?.active === true;

  useEffect(() => {
    let cancelled = false;
    void invoke<boolean>("is_browser_ready")
      .then((ready) => {
        if (!cancelled) setBrowserReady(ready);
      })
      .catch(() => {
        if (!cancelled) setBrowserReady(null);
      });
    return () => {
      cancelled = true;
    };
    // Reconsulta quando um download termina (sucesso ou erro) para não deixar
    // o botão preso em "Download" depois de uma instalação bem-sucedida.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [browserBusy]);

  const handleBrowserDownload = async () => {
    if (browserBusy) return;
    const ok = await store.ensureBrowserDownload(browserReady === true);
    if (ok) setBrowserReady(true);
  };
  const restrictedBackgroundStyle = (() => {
    const style = s.get("General", "RestrictedBackgroundStyle", "warp");
    if (style === "bubbles" || style === "warp" || style === "warpLegacy" || style === "waves") {
      return style;
    }
    return "warp";
  })();
  const isWindows =
    typeof navigator !== "undefined" && navigator.userAgent.toLowerCase().includes("windows");
  const updaterReleaseChannel = normalizeUpdaterReleaseChannel(
    s.get("General", "UpdaterReleaseChannel", "beta")
  );
  const updaterFeatureChannel = normalizeUpdaterFeatureChannel(
    s.get("General", "UpdaterFeatureChannel", "standard")
  );

  const handleManualUpdateCheck = async () => {
    if (checkingUpdate) return;
    setCheckingUpdate(true);
    try {
      await store.checkForUpdates(true, {
        releaseChannel: updaterReleaseChannel,
        featureChannel: updaterFeatureChannel,
      });
    } finally {
      setCheckingUpdate(false);
    }
  };

  return (
    <div className="space-y-0">
      <div className="flex items-center gap-3 py-2 px-1">
        <span className="text-[13px] text-zinc-300 shrink-0">{t("Language")}</span>
        <div className="ml-auto min-w-[180px]">
          <Select
            value={normalizeLanguage(s.get("General", "Language", "en"))}
            options={LANGUAGE_OPTIONS}
            onChange={(value) => {
              const next = normalizeLanguage(value);
              s.set("General", "Language", next);
              void i18n.changeLanguage(next);
            }}
          />
        </div>
      </div>

      <div className="flex items-center gap-3 py-2 px-1">
        <div className="min-w-0">
          <div className="text-[13px] text-zinc-300">{t("Interface size")}</div>
          <div className="mt-0.5 text-[12px] text-zinc-500">
            {t("Auto shrinks the interface on smaller screens")}
          </div>
        </div>
        <div className="ml-auto min-w-[180px]">
          <Select
            ariaLabel="Interface size"
            value={normalizeUiScaleSetting(s.get("General", "InterfaceScale", "auto"))}
            options={[
              { value: "auto", label: "Automatic" },
              { value: "110", label: "110%" },
              { value: "100", label: "100%" },
              { value: "90", label: "90%" },
              { value: "80", label: "80%" },
            ]}
            onChange={(value) => {
              const next = normalizeUiScaleSetting(value);
              s.set("General", "InterfaceScale", next);
              announceUiScale(next);
            }}
          />
        </div>
      </div>

      <Divider />

      <Toggle
        checked={s.getBool("General", "CheckForUpdates")}
        onChange={(v) => s.setBool("General", "CheckForUpdates", v)}
        label="Auto Check for Updates"
        description="Automatically check for new versions on launch"
      />

      <div className="flex items-center gap-3 py-2 px-1">
        <div className="min-w-0">
          <div className="text-[13px] text-zinc-300">{t("Update Release Channel")}</div>
          <div className="mt-0.5 text-[12px] text-zinc-500">
            {t("Pick which release stream is used by the updater")}
          </div>
        </div>
        <div className="ml-auto min-w-[180px]">
          <Select
            value={updaterReleaseChannel}
            options={[
              { value: "beta", label: "Beta" },
              { value: "stable", label: "Stable" },
            ]}
            onChange={(value) =>
              s.set("General", "UpdaterReleaseChannel", normalizeUpdaterReleaseChannel(value))
            }
          />
        </div>
      </div>

      <div className="flex items-center gap-3 py-2 px-1">
        <div className="min-w-0">
          <div className="text-[13px] text-zinc-300">{t("Update Feature Channel")}</div>
          <div className="mt-0.5 text-[12px] text-zinc-500">
            {t("Choose whether updates use the standard or Nexus/WebServer build")}
          </div>
        </div>
        <div className="ml-auto min-w-[220px]">
          <Select
            value={updaterFeatureChannel}
            options={[
              { value: "standard", label: "Standard (Non-Nexus/WebServer)" },
              { value: "nexus-ws", label: "Nexus + WebServer" },
            ]}
            onChange={(value) =>
              s.set("General", "UpdaterFeatureChannel", normalizeUpdaterFeatureChannel(value))
            }
          />
        </div>
      </div>

      <div className="px-1 py-3">
        <div className="flex items-center justify-between gap-3 rounded-lg border border-zinc-800/70 bg-zinc-900/35 px-3 py-2">
          <div className="min-w-0">
            <div className="text-[13px] text-zinc-200">{t("Manual Update Check")}</div>
            <div className="mt-0.5 text-[12px] text-zinc-500">
              {t("Run an update check immediately")}
            </div>
          </div>
          <button
            type="button"
            onClick={() => {
              void handleManualUpdateCheck();
            }}
            disabled={checkingUpdate}
            className="shrink-0 rounded-lg border border-zinc-700/70 bg-zinc-800 px-3 py-1.5 text-[12px] font-medium text-zinc-200 transition-colors hover:bg-zinc-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {checkingUpdate ? t("Checking...") : t("Check Now")}
          </button>
        </div>
      </div>

      {/* Ideia 16: a mesma checagem que a faixa de erro do launch abre. */}
      <div className="px-1 py-3">
        <div className="flex items-center justify-between gap-3 rounded-lg border border-zinc-800/70 bg-zinc-900/35 px-3 py-2">
          <div className="min-w-0">
            <div className="text-[13px] text-zinc-200">{t("Launch does nothing?")}</div>
            <div className="mt-0.5 text-[12px] text-zinc-500">
              {t("Checks Roblox, folders, internet, stuck Roblox processes and Multi Roblox. It never closes anything.")}
            </div>
          </div>
          <button
            type="button"
            onClick={() => store.setDiagnosticsOpen(true)}
            className="shrink-0 rounded-lg border border-zinc-700/70 bg-zinc-800 px-3 py-1.5 text-[12px] font-medium text-zinc-200 transition-colors hover:bg-zinc-700"
          >
            {t("Run check")}
          </button>
        </div>
      </div>

      <div className="px-1 py-3">
        <div className="flex items-center justify-between gap-3 rounded-lg border border-zinc-800/70 bg-zinc-900/35 px-3 py-2">
          <div className="min-w-0">
            <div className="text-[13px] text-zinc-200">{t("First-Time Walkthrough")}</div>
            <div className="mt-0.5 text-[12px] text-zinc-500">
              {t("Replay the guided setup tour with launch and safety essentials")}
            </div>
          </div>
          <button
            type="button"
            onClick={store.openFirstRunWalkthroughFromSettings}
            className="shrink-0 rounded-lg border border-zinc-700/70 bg-zinc-800 px-3 py-1.5 text-[12px] font-medium text-zinc-200 transition-colors hover:bg-zinc-700"
          >
            {t("Open Walkthrough")}
          </button>
        </div>
      </div>

      <div className="flex items-center gap-3 py-2 px-1">
        <div className="min-w-0">
          <div className="text-[13px] text-zinc-300">{t("Restricted Screen Style")}</div>
          <div className="mt-0.5 text-[12px] text-zinc-500">
            {t("Choose the animated background used on the password screen")}
          </div>
        </div>
        <div className="ml-auto min-w-[220px]">
          <Select
            value={restrictedBackgroundStyle}
            options={[
              { value: "warp", label: t("Fluid Warp") },
              { value: "warpLegacy", label: t("Fluid Warp Legacy") },
              { value: "waves", label: t("Waves & Lines") },
              { value: "bubbles", label: t("Fluid Bubbles") },
            ]}
            onChange={(value) => {
              s.set(
                "General",
                "RestrictedBackgroundStyle",
                value === "bubbles" || value === "warp" || value === "warpLegacy" || value === "waves" ? value : "warp"
              );
            }}
          />
        </div>
      </div>

      {isWindows && (
        <Toggle
          checked={s.getBool("General", "ThemeWindowsNavbar")}
          onChange={(v) => s.setBool("General", "ThemeWindowsNavbar", v)}
          label="Theme Windows window navbar"
          description="Makes the top window navbar follow your active MultiAlt theme"
        />
      )}

      {/* A chave continua `AsyncJoin`, mas ela SERIALIZA a fila (launch.rs
          espera o sinal `next_account`, com teto de 120 s). O rotulo antigo,
          "Async Launching", prometia o contrario do que o codigo faz. Nada na
          tela manda o sinal: na pratica sao 2 minutos entre contas — e e isso
          que a descricao diz, para nao parecer o "Start the next account..." */}
      <Toggle
        checked={serialLaunch}
        onChange={(v) => s.setBool("General", "AsyncJoin", v)}
        label="Launch one account at a time"
        description="Leaves 2 minutes between accounts, so each one has time to load. The slowest option. Off: accounts start spaced by the delay below."
      />
      <NumberField
        value={s.getNumber("General", "AccountJoinDelay", 8)}
        onChange={(v) => s.setNumber("General", "AccountJoinDelay", v)}
        label="Account Join Delay"
        description={
          serialLaunch
            ? "Not used while “Launch one account at a time” is on. It applies again when you turn that off."
            : "Roblox rejects logins that arrive too close together, so 8 seconds is the floor."
        }
        disabled={serialLaunch}
        min={MIN_JOIN_DELAY_SECONDS}
        max={60}
        step={0.5}
        suffix="sec"
      />
      {/* launch.rs (`wait_for_game_join`): segue quando o log do Roblox diz que
          a conta entrou; sem log achado, vale o delay acima. Só no Windows,
          onde o log é lido (client_health.rs). */}
      {isWindows && (
        <Toggle
          checked={s.get("General", "WaitForGameJoin", "true") !== "false"}
          onChange={(v) => s.setBool("General", "WaitForGameJoin", v)}
          label="Start the next account once the previous one is in the game"
          description={
            serialLaunch
              ? "Not used while “Launch one account at a time” is on. It applies again when you turn that off."
              : "Doesn't wait out the whole delay above: never sooner than 8 seconds, at most 20 (or the delay, if longer)."
          }
          disabled={serialLaunch}
        />
      )}
      <Toggle
        checked={s.getBool("General", "WrapLongNames")}
        onChange={(v) => s.setBool("General", "WrapLongNames", v)}
        label="Wrap Long Names"
        description="Show long aliases and usernames in full instead of cutting them off"
      />
      <Toggle
        checked={s.getBool("General", "DisableAgingAlert")}
        onChange={(v) => s.setBool("General", "DisableAgingAlert", v)}
        label="Disable Aging Alert"
        description="Hide the freshness dots on accounts unused for 20+ days"
      />
      <Toggle
        checked={s.getBool("General", "DisableImages")}
        onChange={(v) => s.setBool("General", "DisableImages", v)}
        label="Disable Image Loading"
        description="Reduces memory usage by skipping avatar thumbnails"
      />

      <Divider />
      <SectionLabel>Login Browser</SectionLabel>
      <Toggle
        checked={s.get("Login", "PersistentProfile", "true") !== "false"}
        onChange={(v) => s.setBool("Login", "PersistentProfile", v)}
        label="Persistent login profile"
        description="Reuse the login browser profile so the device builds trust over time, which cuts down on login captchas. Only the Roblox sign-in cookie is cleared before each login"
      />
      <Toggle
        checked={s.get("Login", "StealthMode", "true") !== "false"}
        onChange={(v) => s.setBool("Login", "StealthMode", v)}
        label="Reduce automation signals"
        description="Hide browser automation flags Roblox can detect during login. Helps lower captcha prompts but is not a complete solution"
      />
      <TextField
        value={s.get("Login", "ManualBinaryPath", "")}
        onChange={(v) => s.set("Login", "ManualBinaryPath", v)}
        label="Custom browser executable"
        // `{}` para o escape ser processado uma vez, como no placeholder de
        // Custom ClientSettings logo acima nesta mesma tela.
        placeholder={"C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"}
      />
      {/* `TextField` não tem `description`; o caminho vira `Login.ManualBinaryPath`
          e passa por `resolve_browser_binary` (chromium/download.rs), que exige
          um arquivo comum de verdade — nunca pasta, nunca atalho/link. */}
      <div className="px-1 -mt-1 mb-1 text-[12px] text-zinc-500">
        {t(
          "Use your own Chrome, Edge or Chromium instead of the downloaded copy. Must be a file you picked yourself, not a folder — leave empty to use the downloaded browser."
        )}
      </div>

      <div className="px-1 py-3">
        <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/35 px-3 py-2">
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[13px] text-zinc-200">{t("Bundled Browser")}</div>
              <div className="mt-0.5 text-[11px] text-zinc-500">
                {browserReady === true
                  ? t("Chrome for Testing is installed and used for browser logins")
                  : browserReady === false
                    ? t("Not installed yet. It downloads automatically on the first browser login, or you can download it now")
                    : t("Used for browser logins and the account browser")}
              </div>
            </div>
            <button
              type="button"
              onClick={() => {
                void handleBrowserDownload();
              }}
              disabled={browserBusy}
              className="shrink-0 rounded-lg border border-zinc-700/70 bg-zinc-800 px-3 py-1.5 text-[12px] font-medium text-zinc-200 transition-colors hover:bg-zinc-700 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {browserBusy
                ? t("Downloading...")
                : browserDownload?.stage === "error"
                  ? t("Retry Download")
                  : browserReady === true
                    ? t("Reinstall")
                    : t("Download")}
            </button>
          </div>
          {browserBusy && (
            <div className="mt-2">
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                <div
                  className="h-full rounded-full bg-emerald-500/70 transition-all duration-300"
                  style={{
                    width:
                      browserDownload?.stage === "downloading" && browserDownload.percent !== null
                        ? `${browserDownload.percent}%`
                        : "100%",
                  }}
                />
              </div>
              <div className="mt-1 text-[11px] text-zinc-500">
                {browserDownload?.stage === "downloading" && browserDownload.percent !== null
                  ? t("Downloading browser ({{percent}}%)", { percent: browserDownload.percent })
                  : browserDownload?.stage === "extracting"
                    ? t("Preparing browser...")
                    : t("Contacting download service...")}
              </div>
            </div>
          )}
          {!browserBusy && browserDownload?.stage === "error" && browserDownload.error && (
            <div className="mt-2 text-[11px] leading-snug text-red-400">{browserDownload.error}</div>
          )}
        </div>
      </div>

      <Divider />
      <SectionLabel>Hidden Names</SectionLabel>

      {/* `maskAccountName` (utils/accountName.ts) so preserva este prefixo; 0 troca
          o nome inteiro por asteriscos. "Preview Letters" sozinho nao dizia
          nem de que nome se trata. */}
      <NumberField
        value={s.getNumber("General", "HiddenNameLetters", 0)}
        onChange={(v) => s.setNumber("General", "HiddenNameLetters", v)}
        label="Preview Letters"
        description="First letters kept visible while names are hidden; the rest turns into asterisks. 0 hides the whole name."
        min={0}
        max={20}
        suffix="chars"
      />
      <Toggle
        checked={s.getBool("General", "ShowAvatarsWhenHidden")}
        onChange={(v) => s.setBool("General", "ShowAvatarsWhenHidden", v)}
        label="Show Avatars When Hidden"
        description="Display profile pictures even when names are hidden"
      />
      <Toggle
        checked={s.getBool("General", "HideRobuxWhenHidden")}
        onChange={(v) => s.setBool("General", "HideRobuxWhenHidden", v)}
        label="Hide Robux When Hidden"
        description="Mask the Robux balance in the sidebar when names are hidden"
      />
      {/* commands/client_health.rs: SetWindowText nas janelas que o app acompanha. */}
      {isWindows && (
        <Toggle
          checked={s.get("General", "ShowAccountNameOnWindow", "true") !== "false"}
          onChange={(v) => s.setBool("General", "ShowAccountNameOnWindow", v)}
          label="Show Account Name on Roblox Window"
          description="Titles each Roblox window “account — Roblox”, name first, so you can tell them apart on the taskbar. Hidden names stay hidden."
        />
      )}

      <Divider />

      <Toggle
        checked={s.getBool("General", "EnableMultiRbx")}
        onChange={(v) => s.setBool("General", "EnableMultiRbx", v)}
        label={<>Multi Roblox<WarningBadge>use at own risk</WarningBadge></>}
        description="Allow multiple Roblox instances to run simultaneously"
      />
      {/* Ideia 3 (platform/windows/core.rs, `apply_singleton_reservation`):
          experimental e desligado; o método atual continua o padrão. */}
      {isWindows && (
        <Toggle
          checked={s.getBool("General", "ReserveSingletonEvent")}
          onChange={(v) => s.setBool("General", "ReserveSingletonEvent", v)}
          disabled={!s.getBool("General", "EnableMultiRbx")}
          label={<>Experimental: keep clients open across teleports<WarningBadge>use at own risk</WarningBadge></>}
          description="Reserves the name Roblox uses to allow only one window, so a teleport can't close another account. Needs Multi Roblox. Not tested with every game yet: turn it off if a window stops opening."
        />
      )}
      <Toggle
        checked={s.getBool("General", "BottingEnabled")}
        onChange={(v) => s.setBool("General", "BottingEnabled", v)}
        label={<>Auto Rejoin<WarningBadge>advanced</WarningBadge></>}
        description="Enable account cycling tools to keep selected alts rejoining automatically"
      />
      {/* commands/reconnect.rs: padrão de todas as contas; o campo
          `AutoReconnect` da conta (lista "Em jogo" da página Session) vence. */}
      {isWindows && (
        <Toggle
          checked={s.getBool("General", "AutoReconnect")}
          onChange={(v) => s.setBool("General", "AutoReconnect", v)}
          label="Reconnect accounts that drop"
          description="Reopens an account in the same game after a lost connection, a kick or a crash. Only windows MultiAlt opened, never ones opened from the website. Each account can change this in the In game list of the Session page."
        />
      )}
      {/* commands/keep_awake.rs: SetThreadExecutionState só com o sistema
          exigido — a tela continua apagando. */}
      {isWindows && (
        <Toggle
          checked={s.get("General", "KeepPcAwake", "true") !== "false"}
          onChange={(v) => s.setBool("General", "KeepPcAwake", v)}
          label="Keep the PC awake while accounts are kept in game"
          description="Windows won't go to sleep while AFK Mode, Auto Rejoin or auto-reconnect is running. The screen can still turn off."
        />
      )}
      <Toggle
        checked={s.getBool("General", "ShowPresence")}
        onChange={(v) => s.setBool("General", "ShowPresence", v)}
        label="Show Presence"
        description="Display online status for accounts in the list"
      />
      <Toggle
        checked={s.get("General", "WarnOnOnlineJoin", "true") === "true"}
        onChange={(v) => s.setBool("General", "WarnOnOnlineJoin", v)}
        label="Warn Before Joining Online Accounts"
        description="Show a confirmation if selected accounts are already online/in-game"
      />
      <Toggle
        checked={s.get("General", "WarnOnCopyCredential", "true") === "true"}
        onChange={(v) => s.setBool("General", "WarnOnCopyCredential", v)}
        label="Warn Before Copying Credentials"
        description="Show a confirmation before a cookie or password goes to the clipboard"
      />
      <Toggle
        checked={s.get("General", "CheckModerationBeforeLaunch", "true") === "true"}
        onChange={(v) => s.setBool("General", "CheckModerationBeforeLaunch", v)}
        label="Check Bans Before Launch"
        description="Ask Roblox if an account is banned right before opening it, and skip it if so"
      />
      <Toggle
        checked={s.getBool("General", "AutoCookieRefresh")}
        onChange={(v) => s.setBool("General", "AutoCookieRefresh", v)}
        label="Auto Cookie Refresh"
        description="Periodically refresh account cookies to prevent expiration"
      />
      <Toggle
        checked={s.getBool("General", "StartOnPCStartup")}
        onChange={(v) => {
          s.setBool("General", "StartOnPCStartup", v);
          (v ? enable() : disable()).catch(() => {});
        }}
        label="Run on Windows Startup"
      />
      <Toggle
        checked={s.getBool("General", "MinimizeToTray")}
        onChange={(v) => s.setBool("General", "MinimizeToTray", v)}
        label="Minimize to Tray"
        description="Close button hides to system tray instead of exiting"
      />

      <Divider />

      {/* Corta a lista local de recentes (`addRecentGame`,
          server-list/types.ts) — nada a ver com o historico do Roblox. */}
      <NumberField
        value={s.getNumber("General", "MaxRecentGames", 8)}
        onChange={(v) => s.setNumber("General", "MaxRecentGames", v)}
        label="Max Recent Games"
        description="How many games the Recent list keeps before the oldest one drops off."
        min={1}
        max={30}
      />
      {/* Corta a lista de servidores recentes (`addRecentJob`,
          server-list/types.ts), que fica ao lado dos jogos na aba Recent. */}
      <NumberField
        value={s.getNumber("General", "MaxRecentJobs", 12)}
        onChange={(v) => s.setNumber("General", "MaxRecentJobs", v)}
        label="Max Recent Servers"
        description="How many servers (Job IDs) the Recent list keeps before the oldest one drops off."
        min={1}
        max={50}
      />
      <TextField
        value={s.get("General", "ServerRegionFormat", "<city>, <countryCode>")}
        onChange={(v) => s.set("General", "ServerRegionFormat", v)}
        label="Region Format"
        placeholder="<city>, <countryCode>"
      />
      {/* O campo pedia um template sem dizer que tokens existem. Os cinco
          abaixo sao exatamente os que `format_region`
          (api/roblox/server_regions.rs) substitui; o resto do texto passa
          intacto, e template que nao resolve nada cai no IP cru.
          `TextField` nao tem `description`, entao a ajuda fica ao lado. */}
      <div className="px-1 -mt-1 mb-1 space-y-0.5">
        <div className="text-[12px] text-zinc-500">
          {t("Tokens: <city>, <region>, <country>, <countryCode>, <ip>. Any other text is kept as typed.")}
        </div>
        <div className="text-[12px] text-zinc-500">
          {t(
            'Example: "<city>, <countryCode>" shows as "Ashburn, US" in the server list. A template that fills in empty falls back to the raw IP.'
          )}
        </div>
      </div>
    </div>
  );
}
