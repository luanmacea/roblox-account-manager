// O que este SO consegue fazer, do ponto de vista do frontend.
//
// `src/store.tsx` chama `get_platform_capabilities` no boot (e de novo quando
// uma chave de `[Linux]` muda) e usa o resultado para bloquear multi launch e
// botting fora do Windows, além de `isWindowsPlatform` (`src/utils/platform.ts`),
// que sem o campo `os` caía num palpite pelo user agent. O formato é o tipo
// `PlatformCapabilities` em `src/types.ts` — camelCase, sem campos opcionais.

/// Runner nativo: o app lança `RobloxPlayerBeta.exe` (ou o Roblox.app) direto.
const RUNNER_NATIVE: &str = "native";
/// Nenhum runner utilizável nesta plataforma.
const RUNNER_NONE: &str = "none";

/// Espelho de `PlatformCapabilities` em `src/types.ts`.
///
/// Todo campo é obrigatório do lado do TypeScript, então nada aqui pode ser
/// `#[serde(skip_serializing_if = ...)]`: um campo ausente vira `undefined` e
/// quebra as guardas do frontend em silêncio.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformCapabilities {
    os: String,
    session_type: String,
    preferred_runner: String,
    detected_runner: String,
    runner_path: Option<String>,
    supports_single_launch: bool,
    supports_multi_launch: bool,
    supports_watcher: bool,
    supports_watcher_memory: bool,
    supports_window_controls: bool,
    supports_botting: bool,
    supports_updater: bool,
    supports_client_settings: bool,
    /// Volume ao vivo por cliente: só no Windows e só com a feature
    /// `live-audio` no binário (nas duas edições, via `standard`).
    supports_live_audio: bool,
    /// Teto de memória que libera RAM antes de fechar: só no Windows e só com
    /// a feature `memory-trim` (nas duas edições).
    supports_memory_trim: bool,
    reasons: Vec<String>,
    warnings: Vec<String>,
}

/// Monta as capacidades para um SO qualquer — função pura, é o que os testes
/// exercitam (o comando só coleta o ambiente e delega para cá).
///
/// - `os`: `std::env::consts::OS` (`windows`, `macos`, `linux`, ...), que é
///   exatamente o vocabulário que o frontend compara.
/// - `session_type`: `desktop` no Windows/macOS, `$XDG_SESSION_TYPE` no resto.
/// - `preferred_runner`: `[Linux] PreferredRunner` do INI (default `sober`);
///   ignorado onde o launch é nativo.
///
/// `reasons` explica por que algo está desligado (o frontend mostra
/// `reasons[0]` no toast de bloqueio); `warnings` guarda o que funciona só
/// parcialmente. No Windows ambos ficam vazios.
fn build_platform_capabilities(
    os: &str,
    session_type: &str,
    preferred_runner: &str,
) -> PlatformCapabilities {
    match os {
        // Plataforma primária: tudo suportado.
        "windows" => PlatformCapabilities {
            os: os.to_string(),
            session_type: session_type.to_string(),
            preferred_runner: RUNNER_NATIVE.to_string(),
            detected_runner: RUNNER_NATIVE.to_string(),
            runner_path: None,
            supports_single_launch: true,
            supports_multi_launch: true,
            supports_watcher: true,
            supports_watcher_memory: true,
            supports_window_controls: true,
            supports_botting: true,
            supports_updater: true,
            supports_client_settings: true,
            supports_live_audio: cfg!(feature = "live-audio"),
            supports_memory_trim: cfg!(feature = "memory-trim"),
            reasons: Vec::new(),
            warnings: Vec::new(),
        },
        // Suporte parcial: lança e vigia, mas sem memória do processo, sem
        // grid de janelas e sem botting (tudo isso é Win32).
        "macos" => PlatformCapabilities {
            os: os.to_string(),
            session_type: session_type.to_string(),
            preferred_runner: RUNNER_NATIVE.to_string(),
            detected_runner: RUNNER_NATIVE.to_string(),
            runner_path: None,
            supports_single_launch: true,
            supports_multi_launch: true,
            supports_watcher: true,
            supports_watcher_memory: false,
            supports_window_controls: false,
            supports_botting: false,
            supports_updater: true,
            supports_client_settings: true,
            supports_live_audio: false,
            supports_memory_trim: false,
            reasons: vec!["Auto Rejoin is only supported on Windows".to_string()],
            warnings: vec![
                "macOS support is partial: no per-client memory watch, no window grid and no pre-launch isolation".to_string(),
            ],
        },
        // Linux e qualquer outro alvo: não há backend de launch.
        _ => PlatformCapabilities {
            os: os.to_string(),
            session_type: session_type.to_string(),
            preferred_runner: preferred_runner.to_string(),
            detected_runner: RUNNER_NONE.to_string(),
            runner_path: None,
            supports_single_launch: false,
            supports_multi_launch: false,
            supports_watcher: false,
            supports_watcher_memory: false,
            supports_window_controls: false,
            supports_botting: false,
            supports_updater: true,
            supports_client_settings: false,
            supports_live_audio: false,
            supports_memory_trim: false,
            reasons: vec!["Launching Roblox is only supported on Windows and macOS".to_string()],
            warnings: vec![
                "Account management works, but launching, the watcher and Auto Rejoin are unavailable on this platform".to_string(),
            ],
        },
    }
}

/// Tipo de sessão gráfica. No Windows/macOS é sempre `desktop`; no resto vem de
/// `XDG_SESSION_TYPE` (`wayland`, `x11`, `tty`) e cai em `unknown` sem a var.
fn detect_session_type(os: &str) -> String {
    match os {
        "windows" | "macos" => "desktop".to_string(),
        _ => std::env::var("XDG_SESSION_TYPE")
            .ok()
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "unknown".to_string()),
    }
}

/// Runner preferido do INI, com fallback para o default de `[Linux]`.
fn preferred_runner_setting(settings: &SettingsStore) -> String {
    let configured = settings.get_string("Linux", "PreferredRunner");
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        "sober".to_string()
    } else {
        trimmed.to_string()
    }
}

#[tauri::command]
fn get_platform_capabilities(
    settings: tauri::State<'_, SettingsStore>,
) -> Result<PlatformCapabilities, String> {
    let os = std::env::consts::OS;
    Ok(build_platform_capabilities(
        os,
        &detect_session_type(os),
        &preferred_runner_setting(settings.inner()),
    ))
}

#[cfg(test)]
mod platform_info_tests {
    use super::*;

    /// Os campos que o TypeScript declara como obrigatórios.
    const REQUIRED_KEYS: &[&str] = &[
        "os",
        "sessionType",
        "preferredRunner",
        "detectedRunner",
        "runnerPath",
        "supportsSingleLaunch",
        "supportsMultiLaunch",
        "supportsWatcher",
        "supportsWatcherMemory",
        "supportsWindowControls",
        "supportsBotting",
        "supportsUpdater",
        "supportsClientSettings",
        "supportsLiveAudio",
        "supportsMemoryTrim",
        "reasons",
        "warnings",
    ];

    #[test]
    fn windows_supports_everything_without_reasons_or_warnings() {
        let caps = build_platform_capabilities("windows", "desktop", "sober");
        assert!(caps.supports_single_launch);
        assert!(caps.supports_multi_launch);
        assert!(caps.supports_watcher);
        assert!(caps.supports_watcher_memory);
        assert!(caps.supports_window_controls);
        assert!(caps.supports_botting);
        assert!(caps.supports_updater);
        assert!(caps.supports_client_settings);
        assert!(caps.reasons.is_empty());
        assert!(caps.warnings.is_empty());
        // `PreferredRunner` é uma chave de `[Linux]`; no Windows o launch é
        // sempre nativo e a configuração não deve vazar para a UI.
        assert_eq!(caps.preferred_runner, RUNNER_NATIVE);
        assert_eq!(caps.detected_runner, RUNNER_NATIVE);
        assert_eq!(caps.runner_path, None);
    }

    #[test]
    fn live_audio_follows_the_build_feature_and_is_windows_only() {
        let windows = build_platform_capabilities("windows", "desktop", "sober");
        assert_eq!(windows.supports_live_audio, cfg!(feature = "live-audio"));
        // Nas duas edições: o build da padrão (`--features standard`) também tem.
        if cfg!(feature = "standard") {
            assert!(windows.supports_live_audio);
        }
        assert!(!build_platform_capabilities("macos", "desktop", "sober").supports_live_audio);
        assert!(!build_platform_capabilities("linux", "x11", "sober").supports_live_audio);
    }

    #[test]
    fn the_memory_ceiling_follows_the_build_feature_and_is_windows_only() {
        let windows = build_platform_capabilities("windows", "desktop", "sober");
        assert_eq!(windows.supports_memory_trim, cfg!(feature = "memory-trim"));
        // Só na edição completa: a padrão (`--features standard`) não leva.
        assert!(!build_platform_capabilities("macos", "desktop", "sober").supports_memory_trim);
        assert!(!build_platform_capabilities("linux", "x11", "sober").supports_memory_trim);
    }

    #[test]
    fn macos_launches_but_has_no_botting() {
        let caps = build_platform_capabilities("macos", "desktop", "sober");
        assert!(caps.supports_single_launch);
        assert!(caps.supports_multi_launch);
        assert!(caps.supports_client_settings);
        assert!(!caps.supports_botting);
        assert!(!caps.supports_watcher_memory);
        assert!(!caps.supports_window_controls);
        // O frontend mostra `reasons[0]` quando bloqueia o botting.
        assert!(!caps.reasons.is_empty());
        assert!(!caps.warnings.is_empty());
    }

    #[test]
    fn linux_blocks_launching_and_says_why() {
        let caps = build_platform_capabilities("linux", "wayland", "sober");
        assert_eq!(caps.os, "linux");
        assert_eq!(caps.session_type, "wayland");
        assert!(!caps.supports_single_launch);
        assert!(!caps.supports_multi_launch);
        assert!(!caps.supports_botting);
        assert!(!caps.supports_watcher);
        // As guardas de `launchMultiple`/`startBottingMode` leem `reasons[0]`;
        // uma lista vazia faria o frontend mostrar um texto genérico.
        assert!(!caps.reasons[0].is_empty());
        // O runner configurado é ecoado de volta para a UI poder mostrá-lo.
        assert_eq!(caps.preferred_runner, "sober");
        assert_eq!(caps.detected_runner, RUNNER_NONE);
    }

    #[test]
    fn an_unknown_os_is_treated_like_linux() {
        let caps = build_platform_capabilities("freebsd", "x11", "custom");
        assert_eq!(caps.os, "freebsd");
        assert!(!caps.supports_single_launch);
        assert_eq!(caps.preferred_runner, "custom");
    }

    #[test]
    fn the_payload_carries_every_key_the_frontend_type_declares() {
        for os in ["windows", "macos", "linux"] {
            let json = serde_json::to_value(build_platform_capabilities(os, "desktop", "sober"))
                .expect("capabilities should serialize");
            let object = json.as_object().expect("capabilities should be an object");
            for key in REQUIRED_KEYS {
                assert!(object.contains_key(*key), "{os} is missing {key}");
            }
            assert_eq!(object.len(), REQUIRED_KEYS.len(), "{os} has extra keys");
            // `runnerPath` é `string | null` no TypeScript: nunca pode sumir.
            assert!(object["runnerPath"].is_null() || object["runnerPath"].is_string());
            assert!(object["reasons"].is_array());
            assert!(object["warnings"].is_array());
        }
    }

    #[test]
    fn session_type_is_desktop_on_the_native_platforms() {
        assert_eq!(detect_session_type("windows"), "desktop");
        assert_eq!(detect_session_type("macos"), "desktop");
    }

    #[test]
    fn the_command_describes_the_machine_it_runs_on() {
        let caps = build_platform_capabilities(
            std::env::consts::OS,
            &detect_session_type(std::env::consts::OS),
            "sober",
        );
        assert_eq!(caps.os, std::env::consts::OS);
        assert!(!caps.session_type.is_empty());
        // Suporte de launch e de botting nunca podem discordar para mais:
        // botting lança clientes.
        assert!(caps.supports_single_launch || !caps.supports_botting);
        assert!(caps.supports_single_launch || !caps.supports_multi_launch);
    }
}
