#![cfg_attr(debug_assertions, allow(dead_code))]

mod api;
mod chromium;
mod data;
#[cfg(feature = "nexus")]
mod nexus;
mod platform;
#[cfg(target_os = "windows")]
mod webview_recovery;

use api::batch::ImageCache;
use data::accounts::{get_account_data_path, AccountStore};
use data::avatars::AvatarStore;
use data::crypto;
use data::game_lists::GameListsStore;
use data::launch_presets::LaunchPresetStore;
use data::session_history::SessionHistoryStore;
use data::scripts::ScriptStore;
use data::settings::{
    get_avatars_path, get_game_lists_path, get_launch_presets_path, get_scripts_path, get_session_history_path, get_settings_path, get_theme_path, get_theme_presets_path,
    SettingsStore, ThemePresetStore, ThemeStore,
};
use data::versions::{get_versions_catalog_path, VersionsCatalogStore};
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::sync::{Arc, LazyLock, Mutex};
use tauri::menu::{MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

include!("commands/account_api.rs");
include!("commands/image_cache.rs");
include!("commands/account_helpers.rs");
include!("commands/launch_shared.rs");
include!("commands/botting.rs");
include!("commands/generators.rs");
include!("commands/launch.rs");
include!("commands/launch_presets.rs");
include!("commands/diagnostics.rs");
include!("commands/platform_info.rs");
include!("commands/isolation.rs");
include!("commands/versions.rs");
include!("commands/watcher.rs");
include!("commands/afk.rs");
include!("commands/services.rs");
include!("commands/updater.rs");
include!("commands/backups.rs");
include!("commands/avatars.rs");
include!("commands/groups.rs");
include!("commands/external_clients.rs");
include!("commands/focus_follow.rs");
include!("commands/client_health.rs");
include!("commands/session_history.rs");
include!("commands/clipboard.rs");
include!("commands/moderation.rs");
include!("commands/account_check.rs");
include!("commands/reconnect.rs");
include!("commands/keep_awake.rs");

/// O que o app desfaz do Multi Roblox quando fecha.
#[derive(Debug, PartialEq, Eq)]
enum ExitCleanup {
    /// Multi Roblox desligado: nao ha o que desfazer.
    Nothing,
    /// Solta o mutex do singleton e limpa o rastreamento, deixando os clientes
    /// abertos em paz.
    ReleaseSingleton,
}

/// Separado de [`cleanup_multi_roblox_on_exit`] para poder ser testado: o que
/// estava errado aqui era a decisao, nao a chamada das APIs do Windows.
///
/// `roblox_clients` entra de proposito e e **ignorado**. Era ele que mandava
/// fechar tudo quando havia mais de um cliente aberto — herdado do projeto
/// original, sem teste. Fica no parametro para o teste poder afirmar que
/// nenhuma quantidade volta a mudar a decisao.
fn exit_cleanup_plan(multi_rbx_enabled: bool, roblox_clients: usize) -> ExitCleanup {
    let _ = roblox_clients;
    if !multi_rbx_enabled {
        return ExitCleanup::Nothing;
    }
    ExitCleanup::ReleaseSingleton
}

#[cfg(test)]
mod exit_cleanup_tests {
    use super::*;

    #[test]
    fn closing_the_app_never_closes_a_roblox_client() {
        // Relato do dono (28/09/2026): jogando na conta principal, aberta pelo
        // site, mais 4 alts abertas pelo RAM. Ao fechar o RAM, **todos** os
        // clientes fecharam — inclusive o que o RAM nunca abriu.
        //
        // Fechar o gerenciador nao pode tirar ninguem do jogo. Quem quer fechar
        // tudo tem o comando explicito (`cmd_kill_all_roblox`).
        for clientes in [0, 1, 2, 5, 12] {
            assert_eq!(
                exit_cleanup_plan(true, clientes),
                ExitCleanup::ReleaseSingleton,
                "com {} cliente(s) aberto(s)",
                clientes
            );
        }
    }

    #[test]
    fn with_multi_roblox_off_there_is_nothing_to_undo() {
        for clientes in [0, 1, 5] {
            assert_eq!(exit_cleanup_plan(false, clientes), ExitCleanup::Nothing);
        }
    }
}

/// O que fazer, ao fechar, com o que o launch mudou nos arquivos do Roblox
/// (ideia 21, `General.RestoreRobloxSettingsOnExit`).
#[derive(Debug, PartialEq, Eq)]
enum SettingsOnExit {
    /// Nada anotado, ou nada a fazer agora.
    Nothing,
    /// Devolve os valores do usuário.
    Restore,
    /// Um cliente que o app abriu ainda roda: ele relê e regrava esses
    /// arquivos. A anotação fica para o próximo fechar. Nenhum cliente é
    /// fechado por isso.
    WaitForClients,
    /// A opção foi desligada: esquece o anotado (sem mexer nos arquivos).
    Discard,
}

fn settings_on_exit_plan(enabled: bool, pending: bool, app_clients_running: bool) -> SettingsOnExit {
    if !pending {
        return SettingsOnExit::Nothing;
    }
    if !enabled {
        return SettingsOnExit::Discard;
    }
    if app_clients_running {
        return SettingsOnExit::WaitForClients;
    }
    SettingsOnExit::Restore
}

#[cfg(test)]
mod settings_on_exit_tests {
    use super::*;

    #[test]
    fn with_no_client_of_the_app_running_the_users_settings_come_back() {
        assert_eq!(settings_on_exit_plan(true, true, false), SettingsOnExit::Restore);
    }

    #[test]
    fn a_client_the_app_opened_still_running_postpones_and_closes_nothing() {
        assert_eq!(settings_on_exit_plan(true, true, true), SettingsOnExit::WaitForClients);
    }

    #[test]
    fn nothing_recorded_means_nothing_to_do() {
        for (enabled, running) in [(true, false), (true, true), (false, false)] {
            assert_eq!(settings_on_exit_plan(enabled, false, running), SettingsOnExit::Nothing);
        }
    }

    #[test]
    fn with_the_option_off_what_was_recorded_is_forgotten() {
        assert_eq!(settings_on_exit_plan(false, true, false), SettingsOnExit::Discard);
        assert_eq!(settings_on_exit_plan(false, true, true), SettingsOnExit::Discard);
    }
}

/// Ao fechar: devolve as configurações do Roblox (ideia 21). Antes da limpeza
/// do Multi Roblox, que esvazia o rastreamento. Nunca fecha cliente.
#[cfg(target_os = "windows")]
static SETTINGS_EXIT_DECIDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
fn restore_roblox_settings_on_exit(app: &AppHandle<Wry>) {
    use platform::windows;
    // Decide uma vez só: o `Exit` vem depois do `ExitRequested`, com o
    // rastreamento já esvaziado pela limpeza do Multi Roblox — e aí pareceria
    // que nenhum cliente do app está aberto.
    if SETTINGS_EXIT_DECIDED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let enabled = app
        .state::<SettingsStore>()
        .get_bool("General", "RestoreRobloxSettingsOnExit");
    let pending = windows::has_pending_roblox_settings_restore();
    let alive: std::collections::HashSet<u32> = windows::get_roblox_pids().into_iter().collect();
    let app_clients_running = windows::tracker()
        .get_all()
        .iter()
        .any(|process| !process.adopted && alive.contains(&process.pid));
    match settings_on_exit_plan(enabled, pending, app_clients_running) {
        SettingsOnExit::Nothing | SettingsOnExit::WaitForClients => {}
        SettingsOnExit::Restore => {
            windows::restore_roblox_settings();
        }
        SettingsOnExit::Discard => windows::discard_roblox_settings_restore(),
    }
}

#[cfg(target_os = "windows")]
fn cleanup_multi_roblox_on_exit(app: &AppHandle<Wry>) {
    let settings = app.state::<SettingsStore>();
    let clients = platform::windows::get_roblox_pids().len();
    match exit_cleanup_plan(settings.get_bool("General", "EnableMultiRbx"), clients) {
        ExitCleanup::Nothing => return,
        ExitCleanup::ReleaseSingleton => {}
    }

    let tracker = platform::windows::tracker();
    for process in tracker.get_all() {
        tracker.untrack(process.user_id);
    }

    let _ = platform::windows::disable_multi_roblox();
}

#[cfg(target_os = "macos")]
fn cleanup_multi_roblox_on_exit(app: &AppHandle<Wry>) {
    let settings = app.state::<SettingsStore>();
    let clients = platform::macos::get_roblox_pids().len();
    match exit_cleanup_plan(settings.get_bool("General", "EnableMultiRbx"), clients) {
        ExitCleanup::Nothing => return,
        ExitCleanup::ReleaseSingleton => {}
    }

    let tracker = platform::macos::tracker();
    for process in tracker.get_all() {
        tracker.untrack(process.user_id);
    }

    let _ = platform::macos::disable_multi_roblox();
}

/// O frontend pintou o primeiro quadro.
///
/// Mora aqui, e não em `commands/`, porque não é funcionalidade: é o sinal de
/// vida da casca do app, e ele precisa existir em todo SO mesmo com
/// `webview_recovery` sendo Windows-only.
#[tauri::command]
fn frontend_painted() {
    #[cfg(target_os = "windows")]
    webview_recovery::mark_painted();
}

/// Espelho de `WebviewSafeModeState` em `src/types.ts`. Existe fora do
/// `#[cfg]` porque o frontend é um só: fora do Windows a resposta é
/// simplesmente "não está em safe mode".
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SafeModeReport {
    active: bool,
    sticky: bool,
}

/// O app está com a aceleração de vídeo desligada? A faixa da UI depende disto
/// para o usuário não rodar em modo degradado sem saber.
#[tauri::command]
fn get_webview_safe_mode() -> SafeModeReport {
    #[cfg(target_os = "windows")]
    {
        let state = webview_recovery::current_state();
        SafeModeReport {
            active: state.active,
            sticky: state.sticky,
        }
    }
    #[cfg(not(target_os = "windows"))]
    SafeModeReport {
        active: false,
        sticky: false,
    }
}

/// Apaga o marcador e reabre o app no modo normal.
///
/// O safe mode **deste** boot não dá para desligar: as flags foram entregues ao
/// WebView2 quando a janela foi criada. Por isso a saída é reiniciar.
#[tauri::command]
fn leave_webview_safe_mode(app: AppHandle<Wry>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        webview_recovery::leave_safe_mode()?;
        app.restart();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Ok(())
    }
}

/// Autostart com o nome de entrada antigo (ver o comentário em `run`).
fn autostart_plugin() -> tauri::plugin::TauriPlugin<Wry> {
    // No macOS o padrão do builder já é o LaunchAgent, o mesmo de antes.
    tauri_plugin_autostart::Builder::new()
        .app_name("Roblox Account Manager")
        .build()
}

pub fn run() {
    // Antes de tudo: é a última hora de mexer nos argumentos que o WebView2 vai
    // receber (ver webview_recovery.rs).
    #[cfg(target_os = "windows")]
    webview_recovery::prepare_environment();

    crypto::init();

    let account_store = AccountStore::new(get_account_data_path());

    // `load()` é a única porta: ele abre pela chave do aparelho (arquivo `.key`
    // ao lado do vault) e migra um `AccountData.json` em texto puro, deixando
    // `.json.bak` antes. Só quando nada disso abre é que a senha é necessária —
    // e nem falha de criptografia nem chave perdida podem impedir o app de
    // subir, porque é na tela dele que o usuário lê o que aconteceu.
    if let Err(e) = account_store.load() {
        eprintln!("Warning: Failed to load accounts: {}", e);
    }
    match account_store.needs_password() {
        Ok(true) => eprintln!("Encrypted account file detected, password required"),
        Ok(false) => {}
        Err(e) => eprintln!("Warning: Failed to check encryption: {}", e),
    }

    let settings_store = SettingsStore::new(get_settings_path());
    let theme_store = ThemeStore::new(get_theme_path());
    let theme_preset_store = ThemePresetStore::new(get_theme_presets_path());
    let script_store = ScriptStore::new(get_scripts_path());
    let avatar_store = AvatarStore::new(get_avatars_path());
    let game_lists_store = GameListsStore::new(get_game_lists_path());
    let launch_preset_store = LaunchPresetStore::new(get_launch_presets_path());
    let session_history_store = SessionHistoryStore::new(get_session_history_path());
    let versions_catalog = VersionsCatalogStore::new(get_versions_catalog_path());
    let image_cache = ImageCache::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::new().build())
        // O nome da entrada de iniciar com o Windows fica o antigo: o plugin
        // usaria o nome do produto (MultiAlt desde 03/10/2026) e quem tem a opção
        // ligada ficaria com duas entradas. Ver docs/rebrand-multialt.md.
        .plugin(autostart_plugin())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(account_store)
        .manage(settings_store)
        .manage(theme_store)
        .manage(theme_preset_store)
        .manage(script_store)
        .manage(avatar_store)
        .manage(game_lists_store)
        .manage(launch_preset_store)
        .manage(session_history_store)
        .manage(versions_catalog)
        .manage(image_cache)
        .manage(UpdaterRuntimeState::default())
        .manage(chromium::ChromiumManager::new())
        .setup(|app| {
            // A janela já existe: daqui em diante o prazo do primeiro quadro
            // está correndo (ver webview_recovery.rs).
            #[cfg(target_os = "windows")]
            webview_recovery::start_watchdog(app.handle().clone());

            // Lets the launcher report progress while it installs a new Roblox
            // production build by itself (see platform/windows/launch.rs).
            #[cfg(target_os = "windows")]
            platform::windows::set_build_install_app_handle(app.handle().clone());

            // O aviso do `.key` também nasce em gravação de fundo (Auto Rejoin,
            // Watcher, servidor HTTP), que a UI não acompanha: sem isto a faixa
            // só aparecia no boot seguinte — que é justamente o lockout.
            data::accounts::forward_vault_key_warning(
                app.handle(),
                app.state::<AccountStore>().inner(),
            );

            // Histórico de sessões: fecha o que a última execução deixou aberto
            // (antes de o monitor de quedas começar a gravar o novo).
            start_session_history(app.handle());

            // Clientes abertos pelo site (ou antes de o app abrir) entram no
            // "Em jogo" pelo log do Roblox — ver commands/external_clients.rs.
            #[cfg(target_os = "windows")]
            start_external_client_scanner(app.handle().clone());
            // Quedas com motivo, lidas do log de cada cliente rastreado — ver
            // commands/client_health.rs.
            #[cfg(target_os = "windows")]
            start_client_health_monitor(app.handle().clone());

            // Horários dos presets de launch: só com o app aberto, sem
            // recuperar o que passou — ver commands/launch_presets.rs.
            start_preset_scheduler(app.handle().clone());
            // Otimização que segue o foco: o laço só age com a opção ligada.
            #[cfg(target_os = "windows")]
            start_focus_follow_loop(app.handle().clone());

            let show = MenuItemBuilder::with_id("show", "Show").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("MultiAlt")
                .menu(&menu)
                .on_menu_event(
                    |app: &AppHandle<Wry>, event: MenuEvent| match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    },
                )
                .on_tray_icon_event(|tray: &TrayIcon<Wry>, event: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            #[cfg(any(feature = "nexus", feature = "webserver"))]
            let settings = app.state::<SettingsStore>();
            #[cfg(feature = "nexus")]
            if settings.get_bool("AccountControl", "StartOnLaunch") {
                let handle = app.handle().clone();
                let port = settings
                    .get_int("AccountControl", "NexusPort")
                    .unwrap_or(5242) as u16;
                let allow_external =
                    settings.get_bool("AccountControl", "AllowExternalConnections");
                tauri::async_runtime::spawn(async move {
                    match nexus::websocket::nexus()
                        .start(port, allow_external, handle)
                        .await
                    {
                        Ok(port) => eprintln!("Nexus server started on port {}", port),
                        Err(e) => eprintln!("Failed to start Nexus server: {}", e),
                    }
                });
            }

            #[cfg(feature = "webserver")]
            if settings.get_bool("Developer", "EnableWebServer") {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let accounts: &'static AccountStore = unsafe {
                        &*(handle.state::<AccountStore>().inner() as *const AccountStore)
                    };
                    let settings: &'static SettingsStore = unsafe {
                        &*(handle.state::<SettingsStore>().inner() as *const SettingsStore)
                    };
                    match api::server::start(accounts, settings).await {
                        Ok(port) => eprintln!("Web server started on port {}", port),
                        Err(e) => eprintln!("Failed to start web server: {}", e),
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            frontend_painted,
            get_webview_safe_mode,
            leave_webview_safe_mode,
            data::accounts::get_accounts,
            data::accounts::save_accounts,
            data::accounts::add_account,
            data::accounts::remove_account,
            data::accounts::update_account,
            data::accounts::unlock_accounts,
            data::accounts::try_remembered_unlock,
            data::accounts::remembered_unlock_state,
            data::accounts::forget_remembered_unlock,
            data::accounts::is_accounts_encrypted,
            data::accounts::needs_password,
            data::accounts::vault_key_warning,
            data::accounts::set_encryption_password,
            data::accounts::reorder_accounts,
            data::accounts::import_old_account_data,
            data::scripts::get_scripts,
            data::scripts::save_script,
            data::scripts::delete_script,
            data::settings::get_all_settings,
            data::settings::get_setting,
            data::settings::update_setting,
            data::settings::get_theme,
            data::settings::update_theme,
            data::settings::get_theme_presets,
            data::settings::save_theme_preset,
            data::settings::delete_theme_preset,
            data::settings::import_theme_preset_file,
            data::settings::export_theme_preset_file,
            data::settings::import_theme_font_asset,
            data::settings::import_theme_font_bytes,
            data::settings::resolve_theme_font_asset,
            data::settings::import_theme_preset_bytes,
            data::game_lists::get_game_lists,
            data::game_lists::save_game_lists,
            list_backups,
            create_backup,
            restore_backup,
            delete_backup,
            open_backups_folder,
            backups_info,
            check_for_updates_with_channels,
            download_selected_update,
            install_selected_update,
            test_auth,
            validate_cookie,
            get_csrf_token,
            get_auth_ticket,
            check_pin,
            unlock_pin,
            refresh_cookie,
            copy_account_secret,
            check_account_moderation,
            check_accounts,
            get_robux,
            get_user_info,
            lookup_user,
            send_friend_request,
            make_selected_friends,
            get_friend_link_state,
            avatar_free_catalog,
            avatar_list_saved,
            avatar_save,
            avatar_delete,
            avatar_apply_batch,
            avatar_cancel_batch,
            get_avatar_batch_state,
            invalidate_avatar_headshots,
            groups_search,
            groups_icons,
            groups_join_batch,
            groups_cancel_join,
            get_groups_join_state,
            groups_check_membership,
            groups_join_retry,
            groups_popular,
            resolve_join_link,
            block_user,
            unblock_user,
            get_blocked_users,
            unblock_all_users,
            set_follow_privacy,
            get_private_server_invite_privacy,
            set_private_server_invite_privacy,
            set_avatar,
            get_outfits,
            get_outfit_details,
            get_place_details,
            get_servers,
            join_game_instance,
            join_game,
            search_games,
            get_universe_places,
            parse_private_server_link_code,
            join_group,
            get_presence,
            get_account_game_location,
            get_online_friends,
            get_online_friends_for_accounts,
            get_server_regions,
            check_place_access,
            list_servers_ranked,
            start_server_scan,
            stop_server_scan,
            pick_server,
            batch_thumbnails,
            get_avatar_headshots,
            get_asset_thumbnails,
            get_asset_details,
            purchase_product,
            change_password,
            change_email,
            set_display_name,
            quick_login_enter_code,
            quick_login_validate_code,
            batched_get_image,
            batched_get_avatar_headshots,
            batched_get_game_icon,
            batched_get_game_info,
            get_cached_thumbnail,
            clear_image_cache,
            launch_roblox,
            launch_multiple,
            get_launch_presets,
            save_launch_preset,
            delete_launch_preset,
            launch_preset,
            close_preset_clients,
            get_session_history,
            get_current_sessions,
            save_history_export,
            cancel_launch,
            get_launch_queue,
            cancel_account_launch,
            stop_launch_queue,
            next_account,
            start_botting_mode,
            stop_botting_mode,
            get_botting_mode_status,
            add_botting_accounts,
            set_botting_player_accounts,
            botting_account_action,
            start_generator,
            stop_generator,
            get_generator_status,
            generator_test_key,
            cmd_kill_roblox,
            focus_roblox_window,
            list_display_monitors,
            arrange_windows_grid,
            cmd_kill_all_roblox,
            get_running_instances,
            get_unidentified_clients,
            identify_external_client,
            focus_client_window,
            cmd_enable_multi_roblox,
            cmd_disable_multi_roblox,
            cmd_get_roblox_path,
            cmd_apply_fps_unlock,
            kill_legacy_ram_processes,
            diagnose_mutex_holder,
            get_platform_capabilities,
            isolation_get_status,
            isolation_save,
            isolation_list_adapters,
            isolation_restore_network_identifiers,
            isolation_dry_run,
            versions_list_installed,
            versions_list_remote,
            versions_install,
            versions_uninstall,
            versions_set_default,
            versions_set_account_override,
            versions_set_label,
            versions_open_folder,
            start_watcher,
            stop_watcher,
            start_afk_mode,
            stop_afk_mode,
            set_afk_accounts,
            get_afk_mode_status,
            get_afk_keys,
            afk_trigger_now,
            afk_capture_point,
            get_auto_reconnect_status,
            stop_auto_reconnect,
            retry_auto_reconnect,
            chromium::commands::open_login_browser,
            chromium::commands::extract_browser_cookie,
            chromium::commands::close_login_browser,
            chromium::commands::open_account_browser,
            chromium::commands::import_userpass,
            chromium::signup_session::start_signup_session,
            chromium::signup_session::stop_signup_session,
            chromium::signup_session::get_signup_status,
            chromium::commands::is_browser_ready,
            chromium::commands::ensure_browser,
            start_web_server,
            stop_web_server,
            get_web_server_status,
            start_nexus_server,
            stop_nexus_server,
            get_nexus_status,
            get_nexus_accounts,
            add_nexus_account,
            remove_nexus_accounts,
            update_nexus_account,
            nexus_send_command,
            nexus_send_to_all,
            get_nexus_log,
            clear_nexus_log,
            get_nexus_elements,
            set_nexus_element_value,
            export_nexus_lua,
            open_repo_url,
            open_feedback_form,
            sync_windows_navbar_theme,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { .. } => {
                // Antes da limpeza do Multi Roblox, que esvazia o rastreamento.
                #[cfg(target_os = "windows")]
                release_focus_follow_on_exit(app);
                #[cfg(target_os = "windows")]
                restore_roblox_settings_on_exit(app);
                #[cfg(target_os = "windows")]
                cleanup_multi_roblox_on_exit(app);
                // Devolve o PC ao normal (commands/keep_awake.rs).
                #[cfg(target_os = "windows")]
                keep_awake_release_on_exit();
                #[cfg(target_os = "macos")]
                cleanup_multi_roblox_on_exit(app);
            }
            tauri::RunEvent::Exit => {
                #[cfg(target_os = "windows")]
                release_focus_follow_on_exit(app);
                #[cfg(target_os = "windows")]
                restore_roblox_settings_on_exit(app);
                #[cfg(target_os = "windows")]
                cleanup_multi_roblox_on_exit(app);
                #[cfg(target_os = "windows")]
                keep_awake_release_on_exit();
                #[cfg(target_os = "macos")]
                cleanup_multi_roblox_on_exit(app);
                app.state::<chromium::ChromiumManager>().close_login_session();
            }
            _ => {}
        });
}
