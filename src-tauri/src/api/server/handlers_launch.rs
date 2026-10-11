async fn handle_launch_account(
    Extension(state): Extension<AppState>,
    Query(params): Query<AccountQuery>,
    v2: bool,
) -> Response {
    if !state.settings.get_bool("WebServer", "AllowLaunchAccount") {
        return reply(401, "AllowLaunchAccount is disabled", v2);
    }

    if !check_password(&state, &params.password) {
        return reply(401, "Invalid password", v2);
    }

    let identifier = match params.account {
        Some(ref a) if !a.is_empty() => a,
        _ => return reply(400, "Missing Account parameter", v2),
    };

    let place_id: i64 = match params.place_id.as_deref().and_then(|v| v.parse().ok()) {
        Some(id) => id,
        None => return reply(400, "Missing or invalid PlaceId parameter", v2),
    };

    let job_id = params.job_id.as_deref().unwrap_or("");
    let follow_user = params.follow_user.as_deref().map(|v| v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let join_vip = params.join_vip.as_deref().map(|v| v.eq_ignore_ascii_case("true")).unwrap_or(false);

    let accounts = match state.accounts.get_all() {
        Ok(a) => a,
        Err(e) => return reply(500, &e, v2),
    };

    let account = match find_account(&accounts, identifier) {
        Some(a) => a,
        None => return reply(404, "Account not found", v2),
    };

    #[cfg(target_os = "windows")]
    {
        use crate::platform::windows;

        windows::refresh_production_version().await;
        let is_teleport = state.settings.get_bool("Developer", "IsTeleport");
        let use_old_join = state.settings.get_bool("Developer", "UseOldJoin");
        // A pasta de onde o cliente vai abrir, a mesma para o patch e para o
        // old join (sem versão do catálogo aqui: build do canal do registro no
        // old join, a de produção pelo protocolo).
        let roblox_path = windows::get_roblox_path();
        let client_dir = windows::client_dir(
            windows::client_source(use_old_join, false),
            roblox_path.as_deref().unwrap_or(""),
        )
        .await;
        let client_dir = Some(client_dir).filter(|dir| !dir.trim().is_empty());
        patch_client_settings_for_launch(state.settings, client_dir.as_deref());
        let auto_close_last_process = state.settings.get_bool("General", "AutoCloseLastProcess");
        let multi_rbx = state.settings.get_bool("General", "EnableMultiRbx");

        if multi_rbx {
            windows::set_singleton_reservation_enabled(
                state.settings.get_bool("General", "ReserveSingletonEvent"),
            );
            match windows::enable_multi_roblox() {
                Ok(true) => {}
                Ok(false) => {
                    return reply(500, "Failed to enable Multi Roblox. Close all Roblox processes and try again.", v2);
                }
                Err(e) => return reply(500, &e, v2),
            }
        } else {
            let _ = windows::disable_multi_roblox();
        }

        let tracker = windows::tracker();
        if auto_close_last_process && tracker.get_pid(account.user_id).is_some() {
            tracker.kill_for_user_async(account.user_id).await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        let browser_tracker_id =
            match crate::get_or_create_browser_tracker_id(state.accounts, account.user_id) {
                Ok(id) => id,
                Err(e) => return reply(500, &e, v2),
            };
        let ticket = match auth::get_auth_ticket(&account.security_token).await {
            Ok(t) => t,
            Err(e) => return reply(400, &format!("Failed to get auth ticket: {}", e), v2),
        };

        let pids_before = windows::get_roblox_pids();

        let mut access_code = if join_vip {
            job_id.to_string()
        } else {
            String::new()
        };
        let mut link_code = String::new();

        if join_vip && !job_id.is_empty() {
            let mut extracted = String::new();

            if let Some(code) = job_id.split("privateServerLinkCode=").nth(1) {
                extracted = code.split('&').next().unwrap_or(code).to_string();
            } else if let Some(code) = job_id.split("linkCode=").nth(1) {
                extracted = code.split('&').next().unwrap_or(code).to_string();
            } else if let Some(code) = job_id.split("code=").nth(1) {
                extracted = code.split('&').next().unwrap_or(code).to_string();
            }

            if !extracted.is_empty() {
                link_code = extracted;
                if let Ok(code) = roblox::parse_private_server_link_code(
                    &account.security_token,
                    place_id,
                    &link_code,
                )
                .await
                {
                    access_code = code;
                }
            }
        }

        let launch_result = if use_old_join {
            match client_dir.as_deref() {
                Some(dir) => windows::launch_old_join_from(
                    dir,
                    &ticket,
                    place_id,
                    job_id,
                    "",
                    follow_user,
                    join_vip,
                    &access_code,
                    &link_code,
                    is_teleport,
                ),
                None => Err(roblox_path
                    .err()
                    .unwrap_or_else(|| "Could not find the Roblox installation".to_string())),
            }
        } else {
            let url = windows::build_launch_url(
                &ticket,
                place_id,
                job_id,
                &browser_tracker_id,
                "",
                follow_user,
                join_vip,
                &access_code,
                &link_code,
                is_teleport,
            );
            windows::launch_url(&url).await
        };

        if let Err(e) = launch_result {
            return reply(500, &format!("Failed to launch: {}", e), v2);
        }

        if let Some(pid) =
            crate::wait_for_new_roblox_pid(&pids_before, std::time::Duration::from_secs(12)).await
        {
            tracker.track(account.user_id, pid, browser_tracker_id);
            crate::apply_windows_post_launch_profile(
                None,
                state.settings,
                crate::LaunchClientProfile::Normal,
                pid,
            )
            .await;
        }

        // Launch pelo web server tambem e uso da conta (ele nao passa pela fila do
        // app, entao a marcacao que vive em `launch_queue_mark` nao o alcanca).
        let _ = state.accounts.mark_used(account.user_id);
        reply(200, &format!("Launched {} to {}", account.username, place_id), v2)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (place_id, job_id, follow_user, join_vip, account);
        reply(500, "Launching is only supported on Windows", v2)
    }
}

async fn handle_follow_user(
    Extension(state): Extension<AppState>,
    Query(params): Query<AccountQuery>,
    v2: bool,
) -> Response {
    if !state.settings.get_bool("WebServer", "AllowLaunchAccount") {
        return reply(401, "AllowLaunchAccount is disabled", v2);
    }

    if !check_password(&state, &params.password) {
        return reply(401, "Invalid password", v2);
    }

    let identifier = match params.account {
        Some(ref a) if !a.is_empty() => a,
        _ => return reply(400, "Missing Account parameter", v2),
    };

    let target_username = match params.username {
        Some(ref u) if !u.is_empty() => u.clone(),
        _ => return reply(400, "Missing Username parameter", v2),
    };

    let accounts = match state.accounts.get_all() {
        Ok(a) => a,
        Err(e) => return reply(500, &e, v2),
    };

    let account = match find_account(&accounts, identifier) {
        Some(a) => a,
        None => return reply(404, "Account not found", v2),
    };

    let target = match roblox::get_user_id(None, &target_username).await {
        Ok(u) => u,
        Err(e) => return reply(400, &e, v2),
    };

    #[cfg(target_os = "windows")]
    {
        use crate::platform::windows;

        windows::refresh_production_version().await;
        let is_teleport = state.settings.get_bool("Developer", "IsTeleport");
        let use_old_join = state.settings.get_bool("Developer", "UseOldJoin");
        // A pasta de onde o cliente vai abrir, a mesma para o patch e para o
        // old join (sem versão do catálogo aqui: build do canal do registro no
        // old join, a de produção pelo protocolo).
        let roblox_path = windows::get_roblox_path();
        let client_dir = windows::client_dir(
            windows::client_source(use_old_join, false),
            roblox_path.as_deref().unwrap_or(""),
        )
        .await;
        let client_dir = Some(client_dir).filter(|dir| !dir.trim().is_empty());
        patch_client_settings_for_launch(state.settings, client_dir.as_deref());
        let auto_close_last_process = state.settings.get_bool("General", "AutoCloseLastProcess");
        let multi_rbx = state.settings.get_bool("General", "EnableMultiRbx");

        if multi_rbx {
            windows::set_singleton_reservation_enabled(
                state.settings.get_bool("General", "ReserveSingletonEvent"),
            );
            match windows::enable_multi_roblox() {
                Ok(true) => {}
                Ok(false) => {
                    return reply(500, "Failed to enable Multi Roblox. Close all Roblox processes and try again.", v2);
                }
                Err(e) => return reply(500, &e, v2),
            }
        } else {
            let _ = windows::disable_multi_roblox();
        }

        let tracker = windows::tracker();
        if auto_close_last_process && tracker.get_pid(account.user_id).is_some() {
            tracker.kill_for_user_async(account.user_id).await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        let browser_tracker_id =
            match crate::get_or_create_browser_tracker_id(state.accounts, account.user_id) {
                Ok(id) => id,
                Err(e) => return reply(500, &e, v2),
            };
        let ticket = match auth::get_auth_ticket(&account.security_token).await {
            Ok(t) => t,
            Err(e) => return reply(400, &format!("Failed to get auth ticket: {}", e), v2),
        };

        let pids_before = windows::get_roblox_pids();

        let launch_result = if use_old_join {
            match client_dir.as_deref() {
                Some(dir) => windows::launch_old_join_from(
                    dir,
                    &ticket,
                    target.id,
                    "",
                    "",
                    true,
                    false,
                    "",
                    "",
                    is_teleport,
                ),
                None => Err(roblox_path
                    .err()
                    .unwrap_or_else(|| "Could not find the Roblox installation".to_string())),
            }
        } else {
            let url = windows::build_launch_url(
                &ticket,
                target.id,
                "",
                &browser_tracker_id,
                "",
                true,
                false,
                "",
                "",
                is_teleport,
            );
            windows::launch_url(&url).await
        };

        if let Err(e) = launch_result {
            return reply(500, &format!("Failed to launch: {}", e), v2);
        }

        if let Some(pid) =
            crate::wait_for_new_roblox_pid(&pids_before, std::time::Duration::from_secs(12)).await
        {
            tracker.track(account.user_id, pid, browser_tracker_id);
            crate::apply_windows_post_launch_profile(
                None,
                state.settings,
                crate::LaunchClientProfile::Normal,
                pid,
            )
            .await;
        }

        let _ = state.accounts.mark_used(account.user_id);
        reply(200, &format!("Following {} to {}", account.username, target_username), v2)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (account, target);
        reply(500, "Launching is only supported on Windows", v2)
    }
}

async fn handle_set_server(
    Extension(state): Extension<AppState>,
    Query(params): Query<AccountQuery>,
    v2: bool,
) -> Response {
    if !check_password(&state, &params.password) {
        return reply(401, "Invalid password", v2);
    }

    let identifier = match params.account {
        Some(ref a) if !a.is_empty() => a,
        _ => return reply(400, "Missing Account parameter", v2),
    };

    let place_id: i64 = match params.place_id.as_deref().and_then(|v| v.parse().ok()) {
        Some(id) => id,
        None => return reply(400, "Missing or invalid PlaceId parameter", v2),
    };

    let job_id = match params.job_id {
        Some(ref j) if !j.is_empty() => j.clone(),
        _ => return reply(400, "Missing JobId parameter", v2),
    };

    let accounts = match state.accounts.get_all() {
        Ok(a) => a,
        Err(e) => return reply(500, &e, v2),
    };

    let account = match find_account(&accounts, identifier) {
        Some(a) => a,
        None => return reply(404, "Account not found", v2),
    };

    match roblox::join_game_instance(&account.security_token, place_id, &job_id, false).await {
        Ok(_) => reply(200, "Server set successfully", v2),
        Err(e) => reply(400, &e, v2),
    }
}

async fn handle_set_recommended_server(
    Extension(state): Extension<AppState>,
    Query(params): Query<AccountQuery>,
    v2: bool,
) -> Response {
    if !check_password(&state, &params.password) {
        return reply(401, "Invalid password", v2);
    }

    let identifier = match params.account {
        Some(ref a) if !a.is_empty() => a,
        _ => return reply(400, "Missing Account parameter", v2),
    };

    let place_id: i64 = match params.place_id.as_deref().and_then(|v| v.parse().ok()) {
        Some(id) => id,
        None => return reply(400, "Missing or invalid PlaceId parameter", v2),
    };

    let accounts = match state.accounts.get_all() {
        Ok(a) => a,
        Err(e) => return reply(500, &e, v2),
    };

    let account = match find_account(&accounts, identifier) {
        Some(a) => a,
        None => return reply(404, "Account not found", v2),
    };

    let servers_response = match roblox::get_servers(place_id, "Public", None, Some(&account.security_token)).await {
        Ok(r) => r,
        Err(e) => return reply(400, &format!("Failed to get servers: {}", e), v2),
    };

    if servers_response.data.is_empty() {
        return reply(400, "No servers available", v2);
    }

    for server in servers_response.data.iter().rev() {
        if roblox::join_game_instance(&account.security_token, place_id, &server.id, false)
            .await
            .is_ok()
        {
            return reply(200, "Recommended server set successfully", v2);
        }
    }

    reply(400, "Failed to join any available server", v2)
}

/// `SetServer` and `SetRecommendedServer`: the two endpoints in this file that
/// only reserve a slot through the Roblox API. Every case stops at a guard or
/// at a mocked call — nothing here starts a Roblox process.
///
/// `LaunchAccount` and `FollowUser` are deliberately **not** covered. Naming
/// either handler from a test links tauri's wry runtime into the test binary,
/// and that runtime imports `comctl32!TaskDialogIndirect`, which only resolves
/// through the comctl32 v6 manifest the real app binary carries. A `cargo test`
/// binary has none, so the executable then fails to start with
/// STATUS_ENTRYPOINT_NOT_FOUND before any test runs. Their guard chain
/// (`AllowLaunchAccount` -> password -> Account -> PlaceId -> account lookup)
/// is the same code as the one covered here, minus the launch block.
#[cfg(test)]
mod handlers_launch_tests {
    const PASSWORD: &str = "sup3rsecret";

    use super::server_helpers_tests::TestApp;
    use crate::api::endpoints::test_support::{cookie_of, mock_path, mock_server, mount_csrf};
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, ResponseTemplate};

    #[tokio::test]
    async fn setting_a_server_needs_an_account_a_place_and_a_job() {
        let app = TestApp::new("setserver-params");
        app.password(PASSWORD);
        app.add_account("alt_one", 111, "token-one");

        assert_eq!(
            app.get(&format!("/SetServer?PlaceId=1&JobId=job&Password={}", PASSWORD)).await,
            (400, "Missing Account parameter".to_string())
        );
        assert_eq!(
            app.get(&format!("/SetServer?Account=alt_one&JobId=job&Password={}", PASSWORD)).await,
            (400, "Missing or invalid PlaceId parameter".to_string())
        );
        assert_eq!(
            app.get(&format!("/SetServer?Account=alt_one&PlaceId=1&Password={}", PASSWORD)).await,
            (400, "Missing JobId parameter".to_string())
        );
        assert_eq!(
            app.get(&format!("/SetServer?Account=alt_one&PlaceId=1&JobId=&Password={}", PASSWORD)).await,
            (400, "Missing JobId parameter".to_string())
        );
        assert_eq!(
            app.get(&format!("/SetServer?Account=ghost&PlaceId=1&JobId=job&Password={}", PASSWORD)).await,
            (404, "Account not found".to_string())
        );
    }

    #[tokio::test]
    async fn setting_a_server_reserves_the_instance() {
        let server = mock_server().await;
        mount_csrf("ws-setserver", "csrf-ws-setserver").await;
        Mock::given(method("POST"))
            .and(path(mock_path("gamejoin", "/v1/join-game-instance")))
            .and(header("cookie", cookie_of("ws-setserver")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": 2 })),
            )
            .mount(server)
            .await;

        let app = TestApp::new("setserver-ok");
        app.password(PASSWORD);
        app.add_account("alt_set", 9001, "ws-setserver");

        assert_eq!(
            app.get(&format!("/SetServer?Account=alt_set&PlaceId=9100&JobId=job-set&Password={}", PASSWORD))
                .await,
            (200, "Server set successfully".to_string())
        );
    }

    #[tokio::test]
    async fn a_refused_reservation_is_reported() {
        let server = mock_server().await;
        mount_csrf("ws-setserver-bad", "csrf-ws-setserver-bad").await;
        Mock::given(method("POST"))
            .and(path(mock_path("gamejoin", "/v1/join-game-instance")))
            .and(header("cookie", cookie_of("ws-setserver-bad")))
            .respond_with(ResponseTemplate::new(403).set_body_string("server is full"))
            .mount(server)
            .await;

        let app = TestApp::new("setserver-bad");
        app.password(PASSWORD);
        app.add_account("alt_set_bad", 9002, "ws-setserver-bad");

        let (status, body) = app
            .get("/SetServer?Account=alt_set_bad&PlaceId=9101&JobId=job-bad")
            .await;
        assert_eq!(status, 400);
        assert!(body.contains("server is full"), "body: {}", body);
    }

    #[tokio::test]
    async fn the_recommended_server_needs_an_account_and_a_place() {
        let app = TestApp::new("recommended-params");
        app.password(PASSWORD);
        app.add_account("alt_one", 111, "token-one");

        assert_eq!(
            app.get(&format!("/SetRecommendedServer?PlaceId=1&Password={}", PASSWORD)).await,
            (400, "Missing Account parameter".to_string())
        );
        assert_eq!(
            app.get(&format!("/SetRecommendedServer?Account=alt_one&Password={}", PASSWORD)).await,
            (400, "Missing or invalid PlaceId parameter".to_string())
        );
        assert_eq!(
            app.get(&format!("/SetRecommendedServer?Account=ghost&PlaceId=1&Password={}", PASSWORD)).await,
            (404, "Account not found".to_string())
        );
    }

    #[tokio::test]
    async fn a_failing_server_listing_is_reported() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("games", "/v1/games/9200/servers/Public")))
            .respond_with(ResponseTemplate::new(503))
            .mount(server)
            .await;

        let app = TestApp::new("recommended-fail");
        app.password(PASSWORD);
        app.add_account("alt_rec_fail", 9003, "ws-recommended-fail");

        let (status, body) = app
            .get("/SetRecommendedServer?Account=alt_rec_fail&PlaceId=9200")
            .await;
        assert_eq!(status, 400);
        assert!(body.starts_with("Failed to get servers: "), "body: {}", body);
    }

    #[tokio::test]
    async fn an_empty_server_listing_is_reported() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("games", "/v1/games/9201/servers/Public")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "nextPageCursor": serde_json::Value::Null
            })))
            .mount(server)
            .await;

        let app = TestApp::new("recommended-empty");
        app.password(PASSWORD);
        app.add_account("alt_rec_empty", 9004, "ws-recommended-empty");

        assert_eq!(
            app.get(&format!("/SetRecommendedServer?Account=alt_rec_empty&PlaceId=9201&Password={}", PASSWORD))
                .await,
            (400, "No servers available".to_string())
        );
    }

    /// The listing is walked from the back; the first instance that accepts the
    /// reservation wins.
    #[tokio::test]
    async fn the_recommended_server_joins_the_last_listed_instance() {
        let server = mock_server().await;
        mount_csrf("ws-recommended-ok", "csrf-ws-recommended-ok").await;

        Mock::given(method("GET"))
            .and(path(mock_path("games", "/v1/games/9202/servers/Public")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "id": "job-first", "maxPlayers": 10, "playing": 9 },
                    { "id": "job-last", "maxPlayers": 10, "playing": 1 }
                ],
                "nextPageCursor": serde_json::Value::Null
            })))
            .mount(server)
            .await;

        Mock::given(method("POST"))
            .and(path(mock_path("gamejoin", "/v1/join-game-instance")))
            .and(header("cookie", cookie_of("ws-recommended-ok")))
            .and(body_string_contains("job-last"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": 2 })),
            )
            .mount(server)
            .await;

        let app = TestApp::new("recommended-ok");
        app.password(PASSWORD);
        app.add_account("alt_rec", 9005, "ws-recommended-ok");

        assert_eq!(
            app.get(&format!("/SetRecommendedServer?Account=alt_rec&PlaceId=9202&Password={}", PASSWORD))
                .await,
            (200, "Recommended server set successfully".to_string())
        );
    }

    /// Every instance refusing ends in a single error, not a partial success.
    #[tokio::test]
    async fn every_instance_refusing_is_reported() {
        let server = mock_server().await;
        mount_csrf("ws-recommended-none", "csrf-ws-recommended-none").await;

        Mock::given(method("GET"))
            .and(path(mock_path("games", "/v1/games/9203/servers/Public")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "id": "job-a", "maxPlayers": 10, "playing": 10 },
                    { "id": "job-b", "maxPlayers": 10, "playing": 10 }
                ],
                "nextPageCursor": serde_json::Value::Null
            })))
            .mount(server)
            .await;

        Mock::given(method("POST"))
            .and(path(mock_path("gamejoin", "/v1/join-game-instance")))
            .and(header("cookie", cookie_of("ws-recommended-none")))
            .respond_with(ResponseTemplate::new(403).set_body_string("full"))
            .mount(server)
            .await;

        let app = TestApp::new("recommended-none");
        app.password(PASSWORD);
        app.add_account("alt_rec_none", 9006, "ws-recommended-none");

        assert_eq!(
            app.get(&format!("/SetRecommendedServer?Account=alt_rec_none&PlaceId=9203&Password={}", PASSWORD))
                .await,
            (400, "Failed to join any available server".to_string())
        );
    }
}
