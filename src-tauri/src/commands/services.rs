const WEBSERVER_DISABLED_ERR: &str = "Web server is disabled in this build";
const NEXUS_DISABLED_ERR: &str = "Nexus is disabled in this build";

#[derive(Debug, Clone, serde::Serialize)]
struct WebServerStatusResponse {
    running: bool,
    port: u16,
}

#[cfg(feature = "nexus")]
type NexusStatusResponse = nexus::websocket::NexusStatus;

#[cfg(feature = "nexus")]
type NexusAccountViewResponse = nexus::websocket::AccountView;

#[cfg(feature = "nexus")]
type NexusElementResponse = nexus::websocket::CustomElement;

#[cfg(not(feature = "nexus"))]
#[derive(Debug, Clone, serde::Serialize)]
struct NexusStatusResponse {
    running: bool,
    port: Option<u16>,
    connected_count: usize,
}

#[cfg(not(feature = "nexus"))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NexusAccountViewResponse {
    username: String,
    auto_execute: String,
    place_id: i64,
    job_id: String,
    relaunch_delay: f64,
    auto_relaunch: bool,
    is_checked: bool,
    status: String,
    in_game_job_id: String,
}

#[cfg(not(feature = "nexus"))]
#[derive(Debug, Clone, serde::Serialize)]
struct NexusElementResponse {
    name: String,
    element_type: String,
    content: String,
    size: Option<(i32, i32)>,
    margin: Option<(i32, i32, i32, i32)>,
    decimal_places: Option<i32>,
    increment: Option<String>,
    value: String,
    is_newline: bool,
}

#[cfg(feature = "webserver")]
#[tauri::command]
async fn start_web_server(app: tauri::AppHandle) -> Result<u16, String> {
    let accounts: &'static AccountStore =
        unsafe { &*(app.state::<AccountStore>().inner() as *const AccountStore) };
    let settings: &'static SettingsStore =
        unsafe { &*(app.state::<SettingsStore>().inner() as *const SettingsStore) };
    api::server::start(accounts, settings).await
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
async fn start_web_server(_app: tauri::AppHandle) -> Result<u16, String> {
    Err(WEBSERVER_DISABLED_ERR.into())
}

#[cfg(feature = "webserver")]
#[tauri::command]
fn stop_web_server() -> Result<(), String> {
    api::server::stop()
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
fn stop_web_server() -> Result<(), String> {
    Err(WEBSERVER_DISABLED_ERR.into())
}

#[cfg(feature = "webserver")]
#[tauri::command]
fn get_web_server_status() -> Result<WebServerStatusResponse, String> {
    Ok(WebServerStatusResponse {
        running: api::server::is_running(),
        port: api::server::get_port(),
    })
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
fn get_web_server_status() -> Result<WebServerStatusResponse, String> {
    Ok(WebServerStatusResponse {
        running: false,
        port: 0,
    })
}

#[cfg(feature = "nexus")]
#[tauri::command]
async fn start_nexus_server(
    app: tauri::AppHandle,
    settings: tauri::State<'_, SettingsStore>,
) -> Result<u16, String> {
    let port = settings
        .get_int("AccountControl", "NexusPort")
        .unwrap_or(5242) as u16;
    let allow_external = settings.get_bool("AccountControl", "AllowExternalConnections");
    nexus::websocket::nexus()
        .start(port, allow_external, app)
        .await
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
async fn start_nexus_server(
    _app: tauri::AppHandle,
    _settings: tauri::State<'_, SettingsStore>,
) -> Result<u16, String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn stop_nexus_server() -> Result<(), String> {
    nexus::websocket::nexus().stop();
    Ok(())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn stop_nexus_server() -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn get_nexus_status() -> Result<NexusStatusResponse, String> {
    Ok(nexus::websocket::nexus().get_status())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn get_nexus_status() -> Result<NexusStatusResponse, String> {
    Ok(NexusStatusResponse {
        running: false,
        port: None,
        connected_count: 0,
    })
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn get_nexus_accounts() -> Result<Vec<NexusAccountViewResponse>, String> {
    Ok(nexus::websocket::nexus().get_accounts())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn get_nexus_accounts() -> Result<Vec<NexusAccountViewResponse>, String> {
    Ok(Vec::new())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn add_nexus_account(username: String) -> Result<(), String> {
    nexus::websocket::nexus().add_account(&username)
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn add_nexus_account(_username: String) -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn remove_nexus_accounts(usernames: Vec<String>) -> Result<(), String> {
    nexus::websocket::nexus().remove_accounts(&usernames);
    Ok(())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn remove_nexus_accounts(_usernames: Vec<String>) -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn update_nexus_account(account: NexusAccountViewResponse) -> Result<(), String> {
    nexus::websocket::nexus().update_account(account);
    Ok(())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn update_nexus_account(_account: NexusAccountViewResponse) -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn nexus_send_command(message: String) -> Result<(), String> {
    nexus::websocket::nexus().send_command(&message);
    Ok(())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn nexus_send_command(_message: String) -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn nexus_send_to_all(message: String) -> Result<(), String> {
    nexus::websocket::nexus().send_to_all(&message);
    Ok(())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn nexus_send_to_all(_message: String) -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn get_nexus_log() -> Result<Vec<String>, String> {
    Ok(nexus::websocket::nexus().get_log())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn get_nexus_log() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn clear_nexus_log() -> Result<(), String> {
    nexus::websocket::nexus().clear_log();
    Ok(())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn clear_nexus_log() -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn get_nexus_elements() -> Result<Vec<NexusElementResponse>, String> {
    Ok(nexus::websocket::nexus().get_elements())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn get_nexus_elements() -> Result<Vec<NexusElementResponse>, String> {
    Ok(Vec::new())
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn set_nexus_element_value(name: String, value: String) -> Result<(), String> {
    nexus::websocket::nexus().set_element_value(&name, &value);
    Ok(())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn set_nexus_element_value(_name: String, _value: String) -> Result<(), String> {
    Err(NEXUS_DISABLED_ERR.into())
}

/// The Nexus.lua script shipped inside the binary and written out on demand.
#[cfg(feature = "nexus")]
fn nexus_lua_asset() -> &'static str {
    include_str!("../../assets/Nexus.lua")
}

#[cfg(feature = "nexus")]
#[tauri::command]
fn export_nexus_lua() -> Result<String, String> {
    let out_path = std::env::current_dir()
        .map_err(|e| format!("Failed to read current directory: {}", e))?
        .join("Nexus.lua");
    let content = nexus_lua_asset();
    std::fs::write(&out_path, content).map_err(|e| format!("Failed to write Nexus.lua: {}", e))?;
    Ok(out_path.to_string_lossy().into_owned())
}

#[cfg(not(feature = "nexus"))]
#[tauri::command]
fn export_nexus_lua() -> Result<String, String> {
    Err(NEXUS_DISABLED_ERR.into())
}

/// Pagina do projeto, aberta pelo botao do repositorio na barra de titulo.
///
/// **Este repositorio.** O app foi bifurcado de `niccsprojects/...` e o botao
/// continuava levando para la; o mesmo endereco vivia espalhado no frontend e
/// no updater (que oferecia instalar o binario do outro projeto por cima
/// deste). O lado TS tem o par disto em `src/repo.ts`.
const REPO_URL: &str = "https://github.com/luanmacea/MultiAlt";

#[tauri::command]
fn open_repo_url() -> Result<(), String> {
    open_url_in_browser(REPO_URL)
}

/// Formulário de bug ou sugestão no GitHub (`.github/ISSUE_TEMPLATE/`). O app
/// não envia nada: só abre a página no navegador, e quem escreve e envia é a
/// pessoa, na conta GitHub dela. O frontend escolhe só o **tipo**; o endereço
/// sai daqui, de uma lista fechada.
fn feedback_form_url(kind: &str) -> Option<String> {
    let template = match kind {
        "bug" => "bug_report.yml",
        "idea" => "feature_request.yml",
        _ => return None,
    };
    Some(format!("{REPO_URL}/issues/new?template={template}"))
}

#[tauri::command]
fn open_feedback_form(kind: String) -> Result<(), String> {
    let url = feedback_form_url(&kind).ok_or_else(|| format!("Unknown feedback kind: {kind}"))?;
    open_url_in_browser(&url)
}

/// O que o resumo do "Reportar problema" (ideia 28) diz sobre o app e o PC.
/// Nada aqui identifica a pessoa: versão, edição e versão do sistema.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportEnvironment {
    version: String,
    edition: &'static str,
    os: String,
}

/// O canal de features da build vira o nome que a pessoa vê nas releases.
fn edition_label(feature_channel: &str) -> &'static str {
    if feature_channel == "nexus-ws" {
        "complete"
    } else {
        "standard"
    }
}

#[tauri::command]
fn get_report_environment(app: tauri::AppHandle) -> ReportEnvironment {
    #[cfg(target_os = "windows")]
    let os = platform::windows::os_version_label();
    #[cfg(not(target_os = "windows"))]
    let os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    ReportEnvironment {
        version: app.package_info().version.to_string(),
        edition: edition_label(RUNNING_FEATURE_CHANNEL),
        os,
    }
}

/// Abre um endereço fixo do app no navegador padrão. No Windows vai por
/// `cmd /C start`, que trata `&` como separador de comando — por isso só
/// endereços conferidos nos testes passam por aqui.
fn open_url_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    {
        let _ = url;
        Err("Opening URL is not supported on this platform".into())
    }
}

#[tauri::command]
fn sync_windows_navbar_theme(
    app: tauri::AppHandle,
    settings: tauri::State<'_, SettingsStore>,
    theme_store: tauri::State<'_, ThemeStore>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let enable_theme_sync = settings.get_bool("General", "ThemeWindowsNavbar");
        let target_theme = if enable_theme_sync {
            let theme = theme_store.get()?;
            Some(if theme.dark_top_bar {
                tauri::Theme::Dark
            } else {
                tauri::Theme::Light
            })
        } else {
            None
        };

        for window in app.webview_windows().values() {
            let _ = window.set_theme(target_theme.clone());
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        let _ = settings;
        let _ = theme_store;
    }

    Ok(())
}

#[cfg(test)]
mod services_command_tests {
    use super::*;

    #[test]
    fn disabled_build_messages_name_the_feature_that_is_off() {
        assert_eq!(WEBSERVER_DISABLED_ERR, "Web server is disabled in this build");
        assert_eq!(NEXUS_DISABLED_ERR, "Nexus is disabled in this build");
    }

    #[test]
    fn repo_url_points_at_the_project_over_https() {
        assert_eq!(REPO_URL, "https://github.com/luanmacea/MultiAlt");
        assert!(REPO_URL.starts_with("https://github.com/"));
        // No shell metacharacters: the URL is handed to `cmd /C start`.
        assert!(!REPO_URL.contains(|c: char| c.is_whitespace() || c == '&' || c == '"'));
    }

    #[test]
    fn feedback_forms_open_this_repos_issue_templates() {
        assert_eq!(
            feedback_form_url("bug").as_deref(),
            Some("https://github.com/luanmacea/MultiAlt/issues/new?template=bug_report.yml")
        );
        assert_eq!(
            feedback_form_url("idea").as_deref(),
            Some("https://github.com/luanmacea/MultiAlt/issues/new?template=feature_request.yml")
        );
    }

    #[test]
    fn feedback_form_refuses_anything_but_the_two_known_kinds() {
        // O frontend não escolhe endereço: só o tipo. Nada vira URL arbitrária.
        for kind in ["", "Bug", "https://evil.example", "bug&calc", "../x"] {
            assert_eq!(feedback_form_url(kind), None, "{kind}");
        }
    }

    #[test]
    fn the_report_names_the_edition_like_the_releases_do() {
        assert_eq!(edition_label("nexus-ws"), "complete");
        assert_eq!(edition_label("standard"), "standard");
        assert_eq!(edition_label(""), "standard");
        assert!(["standard", "complete"].contains(&edition_label(RUNNING_FEATURE_CHANNEL)));
    }

    #[test]
    fn the_report_environment_carries_only_version_edition_and_os() {
        let json = serde_json::to_value(ReportEnvironment {
            version: "1.2.3".into(),
            edition: "standard",
            os: "Windows 11".into(),
        })
        .unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["edition", "os", "version"]);
    }

    #[test]
    fn feedback_urls_are_safe_to_hand_to_cmd_start() {
        // `cmd /C start` trata `&` como separador de comando: a URL não pode ter
        // um (por isso nenhum campo vem pré-preenchido pela query string).
        for kind in ["bug", "idea"] {
            let url = feedback_form_url(kind).unwrap();
            assert!(!url.contains(|c: char| c.is_whitespace() || c == '&' || c == '"' || c == '^' || c == '|'));
        }
    }

    #[cfg(feature = "nexus")]
    #[test]
    fn nexus_lua_asset_is_embedded_and_looks_like_the_bridge_script() {
        let asset = nexus_lua_asset();
        assert!(!asset.trim().is_empty(), "Nexus.lua must be embedded");
        let lower = asset.to_ascii_lowercase();
        assert!(
            lower.contains("websocket") || lower.contains("nexus"),
            "Nexus.lua does not look like the bridge script"
        );
    }

    #[test]
    fn web_server_status_serializes_the_fields_the_ui_reads() {
        let json = serde_json::to_value(WebServerStatusResponse {
            running: true,
            port: 7963,
        })
        .unwrap();
        assert_eq!(json["running"], true);
        assert_eq!(json["port"], 7963);
    }

    #[test]
    fn get_web_server_status_answers_without_a_running_app() {
        // The command must be callable even before anything started the server.
        let status = get_web_server_status().expect("status should be readable");
        let _ = status.port;
        assert!(status.port <= u16::MAX);
    }

    #[test]
    fn get_nexus_status_answers_without_a_running_app() {
        let status = get_nexus_status().expect("status should be readable");
        let json = serde_json::to_value(&status).unwrap();
        assert!(json.get("running").is_some(), "status payload: {json}");
    }

    #[test]
    fn nexus_read_only_commands_return_empty_collections_when_idle() {
        assert!(get_nexus_accounts().expect("accounts").is_empty());
        assert!(get_nexus_elements().expect("elements").is_empty());
        // The log may already hold startup lines; it must at least be readable.
        assert!(get_nexus_log().is_ok());
    }
}
