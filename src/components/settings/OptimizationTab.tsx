import { useEffect, useMemo, useState } from "react";
import type { UseSettingsReturn } from "../../hooks/useSettings";
import { useStore } from "../../store";
import { useTr } from "../../i18n/text";
import { Divider } from "../ui/Divider";
import { NumberField } from "../ui/NumberField";
import { SectionLabel } from "../ui/SectionLabel";
import { Select } from "../ui/Select";
import { TextAreaField } from "../ui/TextAreaField";
import { TextField } from "../ui/TextField";
import { Toggle } from "../ui/Toggle";
import { WarningBadge } from "../ui/WarningBadge";
import { isWindowsPlatform } from "../../utils/platform";

type OptimizationProfileId = "Normal" | "BottingPlayer" | "BottingBot";

/**
 * Espelho de `WINDOWS_FASTFLAG_ALLOWLIST`
 * (`src-tauri/src/platform/windows/optimization.rs`).
 *
 * Por que espelhar em vez de expor por comando Tauri: a validacao precisa
 * acontecer enquanto o usuario digita, e a tela de Settings ja monta sem
 * depender de ida ao backend; um comando novo traria estado assincrono (carga,
 * falha, lista vazia no primeiro render) para uma lista de 15 strings que so
 * muda quando alguem edita o .rs. O preco do espelho e pago por
 * `fastFlagAllowlist.test.ts`, que le o proprio .rs e quebra a suite se as duas
 * listas divergirem.
 */
export const WINDOWS_FASTFLAG_ALLOWLIST = [
  "DFFlagTextureQualityOverrideEnabled",
  "DFIntTextureQualityOverride",
  "DFFlagDebugRenderForceTechnologyVoxel",
  "DFFlagDebugRenderForceTechnologyFuture",
  "DFFlagRenderForceLowQualityLightmaps",
  "DFFlagDisableDPIScale",
  "FFlagDebugGraphicsDisableDirect3D11",
  "FFlagDebugGraphicsPreferD3D11FL10",
  "FIntDebugForceMSAASamples",
  "FFlagDebugSkyGray",
  "DFFlagDebugPauseVoxelizer",
  "DFFlagDebugRenderForceMoonAngularSize",
  "DFIntDebugRenderForceMoonTextureSize",
  "DFIntDebugFRMQualityLevelOverride",
  "DFIntRenderShadowIntensity",
] as const;

function generalKey(profile: OptimizationProfileId, suffix: string): string {
  if (profile === "Normal") return suffix;
  return `${profile}${suffix}`;
}

function optimizationKey(profile: OptimizationProfileId, suffix: string): string {
  return `${profile}${suffix}`;
}

function optimizationJsonError(raw: string, enabled: boolean, t: (text: string) => string) {
  if (!enabled) return null;
  const trimmed = raw.trim();
  if (!trimmed) return t("Allowlisted fast flags JSON cannot be empty while enabled");

  try {
    const parsed = JSON.parse(trimmed);
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      return t("Allowlisted fast flags JSON must be a JSON object");
    }
    // O backend recusa o JSON INTEIRO por uma chave fora da allowlist e
    // `launch_shared.rs` engole o erro num `eprintln!` — sem esta checagem o
    // toggle ficava ligado, o JSON salvo, e nada era aplicado.
    const invalidKeys = Object.keys(parsed).filter(
      (key) => !(WINDOWS_FASTFLAG_ALLOWLIST as readonly string[]).includes(key)
    );
    if (invalidKeys.length > 0) {
      return `${t("Only Roblox allowlisted keys are accepted")}: ${invalidKeys.join(", ")}`;
    }
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    return `${t("Invalid allowlisted fast flags JSON")}: ${detail}`;
  }

  return null;
}

function AppliesBadge({ text }: { text: string }) {
  return (
    <span className="inline-flex items-center gap-1 rounded border border-sky-500/20 bg-sky-500/10 px-1.5 py-0.5 text-[11px] text-sky-400/80">
      {text}
    </span>
  );
}

function OptimizationProfileSection({
  s,
  title,
  profile,
  isWindows,
  splitProfiles = false,
}: {
  s: UseSettingsReturn;
  title: string;
  profile: OptimizationProfileId;
  isWindows: boolean;
  /** Auto Rejoin com perfis separados: a grade continua uma só para todos. */
  splitProfiles?: boolean;
}) {
  const t = useTr();
  const customClientSettings = s.get("General", generalKey(profile, "CustomClientSettings"), "").trim();
  const customClientSettingsEnabled = customClientSettings.length > 0;
  const fastFlagsEnabled = s.getBool("Optimization", optimizationKey(profile, "EnableFastFlags"));
  const fastFlagsJson = s.get("Optimization", optimizationKey(profile, "FastFlagsJson"), "");
  const fastFlagsError = useMemo(
    () => optimizationJsonError(fastFlagsJson, fastFlagsEnabled, t),
    [fastFlagsEnabled, fastFlagsJson, t]
  );
  const disableExperimentalEditor = customClientSettingsEnabled || !isWindows;

  // Cada campo abaixo so e lido pelo backend quando o interruptor acima dele
  // esta ligado (`windows_client_overrides` / `load_optimization_profile`).
  // Editavel com o interruptor desligado, o valor era salvo e ignorado.
  const unlockFpsEnabled =
    !customClientSettingsEnabled && s.getBool("General", generalKey(profile, "UnlockFPS"));
  const volumeOverrideEnabled = s.getBool("General", generalKey(profile, "OverrideClientVolume"));
  const graphicsOverrideEnabled = s.getBool(
    "General",
    generalKey(profile, "OverrideClientGraphics")
  );
  const windowSizeOverrideEnabled = s.getBool(
    "General",
    generalKey(profile, "OverrideClientWindowSize")
  );
  const processPolicyEnabled = s.getBool(
    "Optimization",
    optimizationKey(profile, "EnableProcessPolicy")
  );
  const jobCpuLimitEnabled = s.getBool(
    "Optimization",
    optimizationKey(profile, "EnableJobCpuLimit")
  );
  const jobMemoryLimitEnabled = s.getBool(
    "Optimization",
    optimizationKey(profile, "EnableJobMemoryLimit")
  );
  const fastFlagsEditorDisabled = disableExperimentalEditor || !fastFlagsEnabled;

  return (
    <div className="rounded-xl border border-zinc-800/70 bg-zinc-950/35 px-4 py-4">
      <div className="flex items-center justify-between gap-3">
        <SectionLabel>{title}</SectionLabel>
        <AppliesBadge text={t("Applies on next launch")} />
      </div>

      <div className="mt-1 text-[12px] text-zinc-500">
        {t("These settings apply only to Roblox processes launched by MultiAlt")}
      </div>

      <Divider />
      <SectionLabel>{t("Official Roblox Client")}</SectionLabel>

      <Toggle
        checked={s.getBool("General", generalKey(profile, "UnlockFPS"))}
        onChange={(v) => {
          if (customClientSettingsEnabled) return;
          s.setBool("General", generalKey(profile, "UnlockFPS"), v);
        }}
        disabled={customClientSettingsEnabled}
        label="Unlock FPS"
        // O que o interruptor faz so aparecia no Rust: grava
        // `DFIntTaskSchedulerTargetFps` no ClientAppSettings.json e
        // `FramerateCap` no GlobalBasicSettings_13.xml antes do launch
        // (platform/windows/client_settings.rs).
        description={
          customClientSettingsEnabled
            ? "Disabled while Custom ClientAppSettings is set"
            : "Lifts the client's frame cap to the Max FPS below by writing DFIntTaskSchedulerTargetFps before each launch."
        }
      />
      <NumberField
        value={s.getNumber("General", generalKey(profile, "MaxFPSValue"), 120)}
        onChange={(v) => s.setNumber("General", generalKey(profile, "MaxFPSValue"), v)}
        label="Max FPS"
        min={5}
        max={9999}
        disabled={!unlockFpsEnabled}
        description={!unlockFpsEnabled ? "Requires Unlock FPS" : undefined}
      />
      <TextField
        value={s.get("General", generalKey(profile, "CustomClientSettings"), "")}
        onChange={(v) => s.set("General", generalKey(profile, "CustomClientSettings"), v)}
        label="Custom ClientSettings"
        // `{}` para o escape ser processado uma vez: entre aspas o JSX mostraria
        // `C:\\path\\...` e a chave nao casaria com a do catalogo.
        placeholder={"C:\\path\\ClientAppSettings.json"}
      />
      {/* O caminho parecia inofensivo: o arquivo e COPIADO por cima do
          ClientAppSettings.json da instalacao (`copy_custom_client_settings`) e
          desliga o Unlock FPS e as fast flags geradas
          (`patch_client_settings_for_launch`). `TextField` nao tem
          `description`, entao a linha fica ao lado. */}
      <div className="px-1 -mt-1 mb-1 text-[12px] text-zinc-500">
        {t(
          "Your own ClientAppSettings.json: the file is copied over the installed client's one at launch and takes over from Unlock FPS and the fast flags below."
        )}
      </div>
      <Toggle
        checked={s.getBool("General", generalKey(profile, "OverrideClientVolume"))}
        onChange={(v) => s.setBool("General", generalKey(profile, "OverrideClientVolume"), v)}
        label="Override Client Volume"
        description="Apply this volume level before launching Roblox"
      />
      <NumberField
        value={Math.round(s.getNumber("General", generalKey(profile, "ClientVolume"), 0.5) * 100)}
        onChange={(v) =>
          s.setNumber(
            "General",
            generalKey(profile, "ClientVolume"),
            Math.max(0, Math.min(100, v)) / 100
          )
        }
        label="Client Volume"
        min={0}
        max={100}
        suffix="%"
        disabled={!volumeOverrideEnabled}
        description={!volumeOverrideEnabled ? "Requires Override Client Volume" : undefined}
      />
      <Toggle
        checked={s.getBool("General", generalKey(profile, "OverrideClientGraphics"))}
        onChange={(v) => s.setBool("General", generalKey(profile, "OverrideClientGraphics"), v)}
        label="Override Graphics Level"
        description="Forces manual graphics quality at launch"
      />
      <NumberField
        value={s.getNumber("General", generalKey(profile, "ClientGraphicsLevel"), 10)}
        onChange={(v) => s.setNumber("General", generalKey(profile, "ClientGraphicsLevel"), v)}
        label="Graphics Level"
        min={1}
        max={10}
        disabled={!graphicsOverrideEnabled}
        description={!graphicsOverrideEnabled ? "Requires Override Graphics Level" : undefined}
      />
      <Toggle
        checked={s.getBool("General", generalKey(profile, "OverrideClientWindowSize"))}
        onChange={(v) => s.setBool("General", generalKey(profile, "OverrideClientWindowSize"), v)}
        label="Override Window Size"
        description="Optional: start Roblox in windowed mode with this size"
      />
      <NumberField
        value={s.getNumber("General", generalKey(profile, "ClientWindowWidth"), 1280)}
        onChange={(v) => s.setNumber("General", generalKey(profile, "ClientWindowWidth"), v)}
        label="Window Width"
        min={320}
        max={7680}
        disabled={!windowSizeOverrideEnabled}
        description={!windowSizeOverrideEnabled ? "Requires Override Window Size" : undefined}
      />
      <NumberField
        value={s.getNumber("General", generalKey(profile, "ClientWindowHeight"), 720)}
        onChange={(v) => s.setNumber("General", generalKey(profile, "ClientWindowHeight"), v)}
        label="Window Height"
        min={240}
        max={4320}
        disabled={!windowSizeOverrideEnabled}
        description={!windowSizeOverrideEnabled ? "Requires Override Window Size" : undefined}
      />
      <Toggle
        checked={s.getBool("General", generalKey(profile, "StartRobloxMinimized"))}
        onChange={(v) => s.setBool("General", generalKey(profile, "StartRobloxMinimized"), v)}
        label="Start Roblox Minimized"
        description="Launches Roblox and minimizes the client window right after startup"
      />
      {isWindows ? (
        // Uma chave só para todos os perfis (launch e Auto Rejoin): a grade é
        // uma só. Ligada por padrão — só o "false" gravado a desliga.
        <>
          <Toggle
            checked={s.get("General", "AutoArrangeGrid", "true") !== "false"}
            onChange={(v) => s.setBool("General", "AutoArrangeGrid", v)}
            label="Arrange in grid on launch"
            description="Each new Roblox window takes the first free cell of the grid (Choose Game > Windows). Accounts with their own window size keep it."
          />
          {splitProfiles ? (
            <div className="text-[12px] leading-5 text-zinc-500">
              {t("Grid options are the same for every profile.")}
            </div>
          ) : null}
          {/* Ideia 22 (docs/features/performance.md): só nos clientes que o
              app abriu, no launch e no botão Arrange in grid. */}
          <Toggle
            checked={s.getBool("General", "GridAllowSmallWindows")}
            onChange={(v) => s.setBool("General", "GridAllowSmallWindows", v)}
            label="Allow smaller windows in the grid"
            description="Grid cells can be smaller than Roblox's minimum window size, so more windows fit on the screen. Also applies to the Arrange in grid button."
          />
          <Toggle
            checked={s.getBool("General", "GridBorderless")}
            onChange={(v) => s.setBool("General", "GridBorderless", v)}
            label="Remove window borders in the grid"
            description="Grid windows lose their title bar and border so they sit edge to edge. Also applies to the Arrange in grid button; turning this off gives the borders back."
          />
        </>
      ) : null}

      {isWindows ? (
        <>
          <Divider />
          <SectionLabel>{t("Windows Process Policy")}</SectionLabel>

          <Toggle
            checked={s.getBool("Optimization", optimizationKey(profile, "EnableProcessPolicy"))}
            onChange={(v) =>
              s.setBool("Optimization", optimizationKey(profile, "EnableProcessPolicy"), v)
            }
            label="Enable Windows process optimization"
            description="Optimization settings are applied after PID detection and before background minimization completes"
          />
          <NumberField
            value={s.getNumber("Optimization", optimizationKey(profile, "ProcessPolicyDelayMs"), 1500)}
            onChange={(v) =>
              s.setNumber(
                "Optimization",
                optimizationKey(profile, "ProcessPolicyDelayMs"),
                Math.max(0, Math.min(15000, v))
              )
            }
            label="Apply delay"
            min={0}
            max={15000}
            suffix="ms"
            disabled={!processPolicyEnabled}
            // A espera acontece depois de o PID aparecer e antes de aplicar a
            // politica (`apply_windows_post_launch_profile`).
            description={
              !processPolicyEnabled
                ? "Requires Enable Windows process optimization"
                : "Waits this long after the Roblox process shows up before applying the policy to it."
            }
          />

          {/* Vira `SetPriorityClass` no processo do cliente; `BackgroundMode`
              ignora a escolha e forca IDLE (platform/windows/optimization.rs). */}
          <div className="flex items-start gap-3 py-2 px-1">
            <div className="min-w-0">
              <div
                className={`text-[13px] ${processPolicyEnabled ? "text-zinc-300" : "text-zinc-500"}`}
              >
                {t("Priority Class")}
              </div>
              <div className="mt-0.5 text-[12px] text-zinc-500 leading-snug">
                {t(
                  "Windows CPU scheduling priority for the Roblox process. Background Mode overrides this with Idle."
                )}
              </div>
            </div>
            <div className="ml-auto w-[170px] shrink-0">
              <Select
                ariaLabel="Priority Class"
                disabled={!processPolicyEnabled}
                value={s.get("Optimization", optimizationKey(profile, "PriorityClass"), "normal")}
                onChange={(value) =>
                  s.set("Optimization", optimizationKey(profile, "PriorityClass"), value)
                }
                options={[
                  { value: "normal", label: "Normal" },
                  { value: "below_normal", label: "Below Normal" },
                  { value: "idle", label: "Idle" },
                ]}
              />
            </div>
          </div>

          <Toggle
            checked={s.getBool("Optimization", optimizationKey(profile, "BackgroundMode"))}
            onChange={(v) =>
              s.setBool("Optimization", optimizationKey(profile, "BackgroundMode"), v)
            }
            disabled={!processPolicyEnabled}
            label="Background Mode"
            description={
              processPolicyEnabled
                ? "Forces Idle priority on every Roblox client of this profile, even the one in focus"
                : "Requires Enable Windows process optimization"
            }
          />
          <Toggle
            checked={s.getBool("Optimization", optimizationKey(profile, "EcoQos"))}
            onChange={(v) => s.setBool("Optimization", optimizationKey(profile, "EcoQos"), v)}
            disabled={!processPolicyEnabled}
            label="EcoQoS"
            description={
              processPolicyEnabled
                ? "Hints Windows to favor efficiency over burst performance"
                : "Requires Enable Windows process optimization"
            }
          />
          <Toggle
            checked={s.getBool("Optimization", optimizationKey(profile, "IgnoreTimerResolution"))}
            onChange={(v) =>
              s.setBool("Optimization", optimizationKey(profile, "IgnoreTimerResolution"), v)
            }
            disabled={!processPolicyEnabled}
            label="Ignore Timer Resolution"
            description={
              processPolicyEnabled
                ? "Reduces timer-resolution pressure for background Roblox clients"
                : "Requires Enable Windows process optimization"
            }
          />

          {/* Vira `ProcessMemoryPriority` (SetProcessInformation): decide de
              quem o Windows tira memoria primeiro quando a RAM aperta. */}
          <div className="flex items-start gap-3 py-2 px-1">
            <div className="min-w-0">
              <div
                className={`text-[13px] ${processPolicyEnabled ? "text-zinc-300" : "text-zinc-500"}`}
              >
                {t("Memory Priority")}
              </div>
              <div className="mt-0.5 text-[12px] text-zinc-500 leading-snug">
                {t(
                  "How readily Windows takes memory away from this client before other processes when RAM runs short."
                )}
              </div>
            </div>
            <div className="ml-auto w-[170px] shrink-0">
              <Select
                ariaLabel="Memory Priority"
                disabled={!processPolicyEnabled}
                value={s.get("Optimization", optimizationKey(profile, "MemoryPriority"), "normal")}
                onChange={(value) =>
                  s.set("Optimization", optimizationKey(profile, "MemoryPriority"), value)
                }
                options={[
                  { value: "normal", label: "Normal" },
                  { value: "low", label: "Low" },
                  { value: "very_low", label: "Very Low" },
                ]}
              />
            </div>
          </div>

          <Divider />
          <div className="flex items-center px-1 pt-3 pb-1">
            <SectionLabel>Experimental / Danger</SectionLabel>
            <WarningBadge>advanced</WarningBadge>
          </div>

          <div className="px-1 pt-1 text-[12px] text-amber-400/80">
            {t("These settings are experimental and may stop working after Roblox updates")}
          </div>

          <Toggle
            checked={fastFlagsEnabled}
            onChange={(v) =>
              s.setBool("Optimization", optimizationKey(profile, "EnableFastFlags"), v)
            }
            disabled={customClientSettingsEnabled}
            label="Enable allowlisted fast flags"
            description={
              customClientSettingsEnabled
                ? "Custom ClientSettings disables generated fast flags for this profile"
                : "Only Roblox allowlisted keys are accepted"
            }
          />
          <TextAreaField
            value={fastFlagsJson}
            onChange={(value) =>
              s.set("Optimization", optimizationKey(profile, "FastFlagsJson"), value)
            }
            label="Allowlisted fast flags JSON"
            // Idem: entre aspas o `\n` aparecia cru e o exemplo saia numa linha
            // so. Em `{}` viram quebras de linha de verdade, iguais a chave do
            // catalogo.
            placeholder={'{\n  "DFFlagTextureQualityOverrideEnabled": true,\n  "DFIntTextureQualityOverride": 0\n}'}
            rows={6}
            disabled={fastFlagsEditorDisabled}
            error={fastFlagsEditorDisabled ? null : fastFlagsError}
            description={
              customClientSettingsEnabled
                ? "Custom ClientSettings disables generated fast flags for this profile"
                : fastFlagsEnabled
                  ? "Only Roblox allowlisted keys are accepted"
                  : "Requires Enable allowlisted fast flags"
            }
          />
          {/* O usuario tinha que adivinhar as chaves aceitas: a allowlist fica a vista. */}
          <div className="px-1 pb-2">
            <div className="text-[12px] text-zinc-500">{t("Keys accepted by the launcher")}</div>
            <div className="mt-1 flex flex-wrap gap-1">
              {WINDOWS_FASTFLAG_ALLOWLIST.map((key) => (
                <code
                  key={key}
                  className="rounded border border-zinc-800/70 bg-zinc-900/60 px-1.5 py-0.5 text-[11px] text-zinc-400"
                >
                  {key}
                </code>
              ))}
            </div>
          </div>

          <Toggle
            checked={s.getBool("Optimization", optimizationKey(profile, "EnableJobCpuLimit"))}
            onChange={(v) =>
              s.setBool("Optimization", optimizationKey(profile, "EnableJobCpuLimit"), v)
            }
            label="Enable job CPU limit"
            description="Caps this Roblox process with a per-process Windows job object"
          />
          <NumberField
            value={s.getNumber("Optimization", optimizationKey(profile, "JobCpuLimitPercent"), 25)}
            onChange={(v) =>
              s.setNumber(
                "Optimization",
                optimizationKey(profile, "JobCpuLimitPercent"),
                Math.max(5, Math.min(100, v))
              )
            }
            label="CPU limit"
            min={5}
            max={100}
            suffix="%"
            disabled={!jobCpuLimitEnabled}
            description={!jobCpuLimitEnabled ? "Requires Enable job CPU limit" : undefined}
          />
          <Toggle
            checked={s.getBool("Optimization", optimizationKey(profile, "EnableJobMemoryLimit"))}
            onChange={(v) =>
              s.setBool("Optimization", optimizationKey(profile, "EnableJobMemoryLimit"), v)
            }
            label="Enable job memory limit"
            description="Caps the per-process memory budget with a Windows job object"
          />
          <NumberField
            value={s.getNumber("Optimization", optimizationKey(profile, "JobMemoryLimitMb"), 2048)}
            onChange={(v) =>
              s.setNumber(
                "Optimization",
                optimizationKey(profile, "JobMemoryLimitMb"),
                Math.max(256, Math.min(32768, v))
              )
            }
            label="Process memory limit"
            min={256}
            max={32768}
            suffix="MB"
            disabled={!jobMemoryLimitEnabled}
            description={!jobMemoryLimitEnabled ? "Requires Enable job memory limit" : undefined}
          />
        </>
      ) : (
        <>
          <Divider />
          <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/30 px-3 py-2 text-[12px] text-zinc-400">
            {t("Windows-only process policies are unavailable on this platform")}
          </div>
        </>
      )}
    </div>
  );
}

/**
 * O que acontece com os clientes enquanto o usuário joga, valendo para todos
 * os perfis (uma chave só): ver docs/features/performance.md. Só clientes
 * que o app abriu — o aberto pelo site fica como está.
 */
function WindowInUseSection({ s }: { s: UseSettingsReturn }) {
  const t = useTr();
  // O volume ao vivo usa COM de áudio e só existe no binário com a feature
  // `live-audio` (nas duas edições, via `standard`): sem ela, nada de opção.
  const capabilities = useStore().platformCapabilities;
  const liveAudio = capabilities?.supportsLiveAudio === true;
  // Teto de memória (commands/memory_ceiling.rs): só com a feature
  // `memory-trim` (nas duas edições). Cada conta muda o seu na página Session.
  const memoryTrim = capabilities?.supportsMemoryTrim === true;
  return (
    <div className="rounded-xl border border-zinc-800/70 bg-zinc-950/35 px-4 py-4">
      <div className="flex items-center justify-between gap-3">
        <SectionLabel>{t("While you play")}</SectionLabel>
        <AppliesBadge text={t("Applies right away")} />
      </div>
      <Toggle
        checked={s.getBool("Optimization", "FollowFocus")}
        onChange={(v) => s.setBool("Optimization", "FollowFocus", v)}
        label="Follow the window in use"
        description="The one you're playing runs at full speed, the others slow down. Only windows opened by MultiAlt; a new one gets 35 s to load first."
      />
      {liveAudio ? (
        <Toggle
          checked={s.getBool("Optimization", "MuteBackgroundClients")}
          onChange={(v) => s.setBool("Optimization", "MuteBackgroundClients", v)}
          label="Mute the Roblox windows you're not using"
          description="Only the window you're playing makes sound. Only windows opened by MultiAlt; turning this off unmutes them."
        />
      ) : null}
      {memoryTrim ? (
        <>
          <NumberField
            value={s.getNumber("Optimization", "MemoryLimit", 0)}
            onChange={(v) => s.setNumber("Optimization", "MemoryLimit", v)}
            label="Memory limit per client"
            description="Above it, MultiAlt asks Windows to free the client's memory first, and again every minute while it stays over. 0 = no limit. Each account can have its own on the Session page."
            min={0}
            max={65536}
            suffix="MB"
          />
          {/* Opção própria (11/10/2026): antes era o "Close If Memory Low" do
              Watcher, que liga também a regra de memória baixa. */}
          <Toggle
            checked={s.getBool("Optimization", "CloseOverMemoryLimit")}
            onChange={(v) => s.setBool("Optimization", "CloseOverMemoryLimit", v)}
            label="Close a client that stays over its limit"
            description="If a client is still over its limit a minute after MultiAlt freed its memory, it is closed. Only that client, only windows MultiAlt opened. Off: MultiAlt only frees memory and never closes."
          />
        </>
      ) : null}
    </div>
  );
}

interface ProfileOption {
  id: OptimizationProfileId;
  label: string;
}

/**
 * Botao de radio custom (mesmo padrao do `ThemedCheckbox` em BottingDialog):
 * `role="radio"` + `aria-checked` porque e um grupo de opcoes mutuamente
 * exclusivas, nao um botao de acao.
 */
function ProfileRadio({
  option,
  active,
  onSelect,
}: {
  option: ProfileOption;
  active: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active}
      onClick={onSelect}
      className={`px-2.5 py-1 rounded-md text-[12px] font-medium whitespace-nowrap transition-colors ${
        active ? "bg-zinc-800 text-zinc-100" : "text-zinc-500 hover:text-zinc-300"
      }`}
    >
      {option.label}
    </button>
  );
}

export function OptimizationTab({ s }: { s: UseSettingsReturn }) {
  const t = useTr();
  const store = useStore();
  const isWindows = isWindowsPlatform(store.platformCapabilities);
  const bottingEnabled = s.getBool("General", "BottingEnabled");
  const sharedProfile = s.get("General", "BottingUseSharedClientProfile", "true") === "true";

  // So existem 2o e 3o perfil quando Auto Rejoin esta ligado com perfis
  // separados; caso contrario so "Normal" existe. Antes disso as 3 secoes
  // eram montadas de uma vez (67 controles, 6357px de scroll) — agora so a
  // escolhida monta.
  const profiles: ProfileOption[] = useMemo(() => {
    const list: ProfileOption[] = [{ id: "Normal", label: t("Normal") }];
    if (bottingEnabled && !sharedProfile) {
      list.push(
        { id: "BottingPlayer", label: t("Auto Rejoin Main") },
        { id: "BottingBot", label: t("Auto Rejoin Alt") }
      );
    }
    return list;
  }, [bottingEnabled, sharedProfile, t]);

  const [selectedProfile, setSelectedProfile] = useState<OptimizationProfileId>("Normal");

  // Se Auto Rejoin for desligado (ou voltar a perfil compartilhado) enquanto um
  // perfil de bot esta selecionado, essa secao deixa de existir — cai de
  // volta em Normal em vez de nao renderizar nada.
  useEffect(() => {
    if (!profiles.some((p) => p.id === selectedProfile)) {
      setSelectedProfile("Normal");
    }
  }, [profiles, selectedProfile]);

  const activeProfile = profiles.find((p) => p.id === selectedProfile) ?? profiles[0];

  return (
    <div className="space-y-4">
      {/* Vale na hora: fica acima dos perfis (que valem no próximo launch),
          para não separar o seletor de perfil das opções dele. */}
      {isWindows ? <WindowInUseSection s={s} /> : null}

      <div className="rounded-xl border border-zinc-800/70 bg-zinc-950/35 px-4 py-4">
        <SectionLabel>{t("Optimization Profiles")}</SectionLabel>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <AppliesBadge text={t("Applies on next launch")} />
          {!isWindows ? <AppliesBadge text={t("Windows only")} /> : null}
        </div>
        <div className="mt-3 text-[12px] leading-5 text-zinc-500">
          {isWindows
            ? t("These settings apply only to Roblox processes launched by MultiAlt")
            : t("Windows-only process policies are unavailable on this platform")}
        </div>
        {isWindows ? (
          // Ideia 21: uma chave só, para todos os perfis. O backend anota o
          // valor de antes da primeira mudança e devolve ao fechar o app
          // (platform/windows/settings_restore.rs).
          <>
            <Divider />
            <Toggle
              checked={s.getBool("General", "RestoreRobloxSettingsOnExit")}
              onChange={(v) => s.setBool("General", "RestoreRobloxSettingsOnExit", v)}
              label="Restore Roblox settings when MultiAlt closes"
              description="Puts back your own FPS, volume, graphics, window and FastFlags when MultiAlt closes and no client it opened is still running, so the game you open from the website is not left with them."
            />
          </>
        ) : null}
        {bottingEnabled ? (
          <>
            <Divider />
            <Toggle
            checked={sharedProfile}
            onChange={(v) => s.setBool("General", "BottingUseSharedClientProfile", v)}
            label="Use same client settings for main and alts"
            description="Main and alt profiles inherit Normal while shared mode is enabled"
          />
          </>
        ) : null}
      </div>

      {profiles.length > 1 ? (
        // `sticky` para o nome do perfil ativo nunca depender de rolagem: o
        // pai que rola e o container de `TabContent` em SettingsDialog, e
        // este bloco vive no topo do conteudo desta aba, entao gruda no topo
        // dele enquanto a secao abaixo rola. `-mx-5`/`px-5` cancelam o
        // padding lateral do container pai para o fundo cobrir a largura toda.
        <div
          role="radiogroup"
          aria-label={t("Optimization profile")}
          className="sticky top-0 z-10 -mx-5 flex items-center gap-1 border-b border-zinc-800/60 bg-zinc-900/95 px-5 py-2 backdrop-blur-sm"
        >
          {profiles.map((option) => (
            <ProfileRadio
              key={option.id}
              option={option}
              active={option.id === activeProfile.id}
              onSelect={() => setSelectedProfile(option.id)}
            />
          ))}
        </div>
      ) : null}

      <OptimizationProfileSection
        s={s}
        title={activeProfile.label}
        profile={activeProfile.id}
        isWindows={isWindows}
        splitProfiles={profiles.length > 1}
      />
    </div>
  );
}
