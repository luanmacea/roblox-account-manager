/// Keeps the first occurrence of every id, in the order the user selected them.
/// Botting launches follow this order, so it must stay stable.
fn dedupe_preserving_order(ids: Vec<i64>) -> Vec<i64> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for id in ids {
        if seen.insert(id) {
            out.push(id);
        }
    }
    out
}

/// Player accounts must be part of the botting selection; anything else is a
/// UI bug that would silently never be launched.
#[cfg(target_os = "windows")]
fn botting_player_set(selected: &[i64], player_user_ids: Vec<i64>) -> Result<HashSet<i64>, String> {
    let mut player_set = HashSet::new();
    for uid in player_user_ids {
        if !selected.contains(&uid) {
            return Err("Player Account must be one of the selected accounts".into());
        }
        player_set.insert(uid);
    }
    Ok(player_set)
}

/// `(retry_max, retry_base_seconds, default_player_grace_minutes)` read from
/// settings and clamped to the ranges the cycle loop expects.
#[cfg(target_os = "windows")]
fn botting_retry_config(settings: &SettingsStore) -> (u32, u64, i64) {
    let retry_max = settings
        .get_int("General", "BottingRetryMax")
        .unwrap_or(6)
        .clamp(1, 20) as u32;
    let retry_base_seconds = settings
        .get_int("General", "BottingRetryBaseSeconds")
        .unwrap_or(8)
        .clamp(5, 120) as u64;
    let default_player_grace_minutes = settings
        .get_int("General", "BottingPlayerGraceMinutes")
        .unwrap_or(15)
        .clamp(1, 90);
    (retry_max, retry_base_seconds, default_player_grace_minutes)
}

/// Rejoin interval, in minutes. Too short and Roblox rate-limits the account.
#[cfg(target_os = "windows")]
fn clamp_botting_interval_minutes(interval_minutes: i64) -> u64 {
    interval_minutes.clamp(10, 480) as u64
}

/// Spacing between two launches of the session, in seconds.
#[cfg(target_os = "windows")]
fn clamp_botting_launch_delay_seconds(launch_delay_seconds: i64) -> u64 {
    launch_delay_seconds.clamp(5, 120) as u64
}

/// Grace period a demoted player account keeps its client before the loop
/// takes it over. Zero or negative means "use the configured default".
#[cfg(target_os = "windows")]
fn resolve_player_grace_minutes(requested: i64, default_minutes: i64) -> u64 {
    if requested <= 0 {
        default_minutes as u64
    } else {
        requested.clamp(1, 90) as u64
    }
}

/// Backoff after a 429: never shorter than 45s, and never shorter than two
/// launch slots, so the retry does not walk straight back into the rate limit.
#[cfg(target_os = "windows")]
fn botting_429_delay_seconds(delay: i64, launch_delay_seconds: u64) -> i64 {
    delay
        .max(45)
        .max((launch_delay_seconds as i64).saturating_mul(2))
}

/// Backoff after the previous client refused to close: at least two launch
/// slots, and always within 6..=300 seconds.
#[cfg(target_os = "windows")]
fn botting_close_failure_delay_seconds(
    retry_base_seconds: u64,
    retry_count: u32,
    retry_max: u32,
    launch_delay_seconds: u64,
) -> u64 {
    backoff_delay_seconds(retry_base_seconds, retry_count, retry_max)
        .max(launch_delay_seconds.saturating_mul(2))
        .clamp(6, 300)
}

/// Phase and next-restart time for an account added to a running session: one
/// already running waits a full interval, a queued one only a launch slot.
#[cfg(target_os = "windows")]
fn botting_add_schedule(
    has_running_client: bool,
    now: i64,
    interval_ms: i64,
    launch_delay_ms: i64,
) -> (&'static str, Option<i64>) {
    if has_running_client {
        ("running", Some(now.saturating_add(interval_ms)))
    } else {
        ("queued", Some(now.saturating_add(launch_delay_ms)))
    }
}

/// O que a primeira passagem da sessao faz com uma conta.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottingFirstPass {
    /// Conta marcada como desconectada: fica de fora do ciclo.
    Disconnected,
    /// Ja esta em jogo e a sessao foi aberta em modo adocao: **nao** relanca.
    /// O cliente que o usuario abriu continua de pe e so entra no ciclo no
    /// primeiro vencimento do intervalo.
    Adopt,
    /// O caminho de sempre: fecha o que houver e lanca.
    Launch,
}

/// Decide entre adotar o cliente que ja esta aberto e relancar do zero.
///
/// Sem a adocao, ligar o Botting em contas que ja estavam jogando derrubava
/// todas elas: a primeira passagem fecha o cliente e abre outro no place da
/// sessao. Quem ja esta no lugar certo nao precisa disso.
#[cfg(target_os = "windows")]
fn botting_first_pass(
    adopt_running: bool,
    has_running_client: bool,
    disconnected: bool,
) -> BottingFirstPass {
    if disconnected {
        return BottingFirstPass::Disconnected;
    }
    if adopt_running && has_running_client {
        return BottingFirstPass::Adopt;
    }
    BottingFirstPass::Launch
}

/// `(disconnect, close, restart_client, restart_loop)` for a context-menu
/// action on one botting account.
#[cfg(target_os = "windows")]
fn botting_action_flags(action: &BottingAccountAction) -> (bool, bool, bool, bool) {
    let should_disconnect = matches!(
        action,
        BottingAccountAction::Disconnect | BottingAccountAction::CloseDisconnect
    );
    let should_close = matches!(
        action,
        BottingAccountAction::Close
            | BottingAccountAction::CloseDisconnect
            | BottingAccountAction::RestartClient
            | BottingAccountAction::RestartLoop
    );
    let should_restart_client = matches!(action, BottingAccountAction::RestartClient);
    let should_restart_loop = matches!(action, BottingAccountAction::RestartLoop);
    (
        should_disconnect,
        should_close,
        should_restart_client,
        should_restart_loop,
    )
}

/// Versão a reportar ao `ProcessTracker` para o cliente que este ciclo do
/// Botting acabou de abrir (Task de sync com o upstream: sem isto,
/// `has_version_conflict` — a guarda que a fila de launch usa para recusar
/// abrir numa versão diferente da que já está rodando — não enxergava os
/// clientes do Botting, porque o ciclo só chamava `tracker.track` (sem
/// versão). Duas contas em versões diferentes (uma pelo Botting, outra pela
/// fila) abriam lado a lado sem aviso nenhum, que é exatamente o que essa
/// guarda existe para impedir.
///
/// Só vale no old join: pelo protocolo (`launch_url`) o cliente sempre abre a
/// build de **produção** — o `channel:` vazio da URL vence o registro
/// (`CLAUDE.md`, `docs/features/launch.md`) — então reportar
/// `resolved_version_id` nesse ramo mentiria para a guarda. Na prática
/// `resolve_use_old_join` já garante `resolved_version_id.is_none()` sempre
/// que `use_old_join` for falso (a mesma invariante de que depende
/// `windows::client_source`), mas a função fica segura por conta própria
/// em vez de confiar nisso silenciosamente.
#[cfg(target_os = "windows")]
fn botting_tracked_version(
    use_old_join: bool,
    resolved_version_id: Option<String>,
) -> Option<String> {
    if use_old_join {
        resolved_version_id
    } else {
        None
    }
}

async fn launch_account_for_cycle(
    app: &tauri::AppHandle,
    user_id: i64,
    place_id: i64,
    job_id: &str,
    launch_data: &str,
    is_player: bool,
) -> Result<(), String> {
    use platform::windows;

    let launch_profile = {
        let settings = app.state::<SettingsStore>();
        if botting_uses_shared_client_profile(&settings) {
            LaunchClientProfile::Normal
        } else if is_player {
            LaunchClientProfile::BottingPlayer
        } else {
            LaunchClientProfile::BottingBot
        }
    };

    let (
        is_teleport,
        configured_old_join,
        auto_close_last_process,
        multi_rbx,
        auto_close_multi_conflicts,
        reserve_singleton_event,
        start_minimized,
    ) = {
        let settings = app.state::<SettingsStore>();
        (
            settings.get_bool("Developer", "IsTeleport"),
            settings.get_bool("Developer", "UseOldJoin"),
            settings.get_bool("General", "AutoCloseLastProcess"),
            settings.get_bool("General", "EnableMultiRbx"),
            settings.get_bool("General", "AutoCloseRobloxForMultiRbx"),
            settings.get_bool("General", "ReserveSingletonEvent"),
            start_minimized_for_profile(&settings, launch_profile),
        )
    };

    // Exceções da conta valem no Botting também: a conta principal continua
    // sendo a mesma conta, esteja ela numa fila de launch ou num ciclo de bot.
    let account_snapshot = app.state::<AccountStore>().get_all().ok();
    let account_overrides = account_snapshot
        .as_ref()
        .and_then(|list| list.iter().find(|a| a.user_id == user_id))
        .and_then(|a| account_client_overrides(&a.fields));
    let start_minimized = account_overrides
        .as_ref()
        .and_then(|o| o.start_minimized)
        .unwrap_or(start_minimized);

    // Versão do catálogo que a conta usa — a mesma resolução que
    // `commands/launch.rs` já faz para o launch avulso e para a fila (Task 3
    // do plano de sync com o upstream, `docs/superpowers/plans/upstream-sync-2026-09.md`).
    // Sem isso, uma conta com `RobloxVersion` própria abria no Botting sempre
    // na build padrão.
    let account_version_override = account_snapshot
        .as_ref()
        .and_then(|list| list.iter().find(|a| a.user_id == user_id))
        .and_then(|a| a.fields.get("RobloxVersion").cloned())
        .filter(|v| !v.trim().is_empty());
    let (resolved_base_path, resolved_version_id) = {
        let settings = app.state::<SettingsStore>();
        let versions = app.state::<data::versions::VersionsCatalogStore>();
        windows::resolve_roblox_install_path(
            account_version_override.as_deref(),
            &settings,
            &versions,
        )?
    };
    // O Botting não roda isolamento pré-launch (`run_pre_launch_isolation` é
    // exclusivo da fila em `commands/launch.rs`), então não há pasta sendo
    // apagada por baixo do old join aqui — `isolation_wipes_install` sempre
    // `false`.
    let use_old_join = resolve_use_old_join(false, configured_old_join, resolved_version_id.as_deref());

    let resolved_launch = resolve_launch_job(job_id, false, "");

    if multi_rbx {
        ensure_multi_roblox_enabled(auto_close_multi_conflicts, reserve_singleton_event).await?;
    } else {
        let _ = windows::disable_multi_roblox();
    }

    // A pasta de onde o cliente vai abrir, a mesma para o patch (FPS, fast
    // flags) e para o spawn do old join: versão do catálogo pelo old join, a
    // build do canal do registro no old join sem catálogo, e a de produção
    // pelo protocolo (`launch_url`) — versão por conta não se aplica nesse
    // caminho (CLAUDE.md). Ver `windows::client_source`.
    let client_dir = windows::client_dir(
        windows::client_source(use_old_join, resolved_version_id.is_some()),
        &resolved_base_path,
    )
    .await;

    windows::refresh_production_version().await;

    let tracker = windows::tracker();
    if auto_close_last_process && tracker.get_pid(user_id).is_some() {
        let closed = tracker.kill_for_user_graceful_async(user_id, 4500).await;
        if !closed {
            return Err("Previous Roblox instance did not close before relaunch".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    let browser_tracker_id = {
        let state = app.state::<AccountStore>();
        get_or_create_browser_tracker_id(&state, user_id)?
    };
    let mut ticket: Option<String> = None;
    let mut last_ticket_err = String::new();
    for attempt in 0..5_u64 {
        let state = app.state::<AccountStore>();
        match run_with_session_retry(state.inner(), user_id, |cookie| async move {
            api::auth::get_auth_ticket(&cookie).await
        })
        .await
        {
            Ok(value) => {
                ticket = Some(value);
                break;
            }
            Err(err) => {
                last_ticket_err = err.clone();
                if !is_429_related_error(&err) || attempt >= 4 {
                    break;
                }
                let delay = 4_u64.saturating_mul(attempt + 1);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }
        }
    }
    let ticket = ticket
        .ok_or_else(|| format!("Failed to get auth ticket for launch: {}", last_ticket_err))?;
    let private_join = {
        let state = app.state::<AccountStore>();
        run_with_session_retry(state.inner(), user_id, |cookie| {
            let resolved_launch = resolved_launch.clone();
            async move { resolve_private_join(&cookie, place_id, &resolved_launch).await }
        })
        .await?
    };

    // Último passo antes do spawn: os clientes do Auto Rejoin (a conta
    // principal inclusive) reescrevem o XML compartilhado enquanto esta conta
    // espera o ticket — que aqui pode levar 4 tentativas com espera de 429.
    let resolved_window = {
        let settings = app.state::<SettingsStore>();
        patch_client_settings_for_launch(
            &settings,
            launch_profile,
            account_overrides.as_ref(),
            Some(&client_dir),
        )
    };

    let pids_before = windows::get_roblox_pids();

    let launch_result = if use_old_join {
        // Sem versão resolvida (nem da conta, nem do catálogo), a pasta do old
        // join é a build do canal do registro, porque é o registro quem decide
        // o canal nesse caminho (CLAUDE.md#regras-críticas) — `client_dir` já
        // resolveu isso, e o patch foi para a mesma pasta.
        windows::launch_old_join_from(
            &client_dir,
            &ticket,
            private_join.place_id,
            &resolved_launch.job_id,
            launch_data,
            false,
            private_join.use_private_join,
            &private_join.access_code,
            &private_join.link_code,
            is_teleport,
        )
    } else {
        let url = windows::build_launch_url(
            &ticket,
            private_join.place_id,
            &resolved_launch.job_id,
            &browser_tracker_id,
            launch_data,
            false,
            private_join.use_private_join,
            &private_join.access_code,
            &private_join.link_code,
            is_teleport,
        );
        windows::launch_url(&url).await
    };

    if let Err(e) = launch_result {
        return Err(format!("Launch failed: {}", e));
    }

    // O ciclo do botting também é uso da conta: sem isto, uma conta que joga o dia
    // inteiro em Botting Mode apareceria como parada há meses na lista.
    let _ = app.state::<AccountStore>().mark_used(user_id);

    let Some(pid) = wait_for_new_roblox_pid(&pids_before, std::time::Duration::from_secs(12)).await
    else {
        return Err("Timed out waiting for Roblox process after launch".into());
    };

    // Registra a versão do jeito que `commands/launch.rs` já faz (Task de sync
    // com o upstream): sem isso `has_version_conflict` da fila de launch não
    // enxergava os clientes que o Botting abre.
    tracker.track_with_version(
        user_id,
        pid,
        browser_tracker_id,
        botting_tracked_version(use_old_join, resolved_version_id.clone()),
    );
    {
        let settings_state = app.state::<SettingsStore>();
        apply_windows_post_launch_profile(Some(app), settings_state.inner(), launch_profile, pid)
            .await;
    }
    if detect_auth_failure_window(pid).await {
        let _ = tracker.kill_for_user_async(user_id).await;
        return Err("Roblox authentication failed (429) while joining".into());
    }

    let auto_arrange_grid = auto_arrange_grid_enabled(&app.state::<SettingsStore>());
    spawn_client_window_enforcement(
        app,
        pid,
        client_window_plan(ClientWindowInputs {
            fullscreen: resolved_window.fullscreen,
            window_size: resolved_window.window_size,
            keeps_own_window: account_overrides
                .as_ref()
                .is_some_and(|o| o.keeps_own_window()),
            start_minimized,
            auto_arrange_grid,
            saved_rect: None,
        }),
    );

    if start_minimized {
        let baseline = pids_before.clone();
        tokio::spawn(async move {
            minimize_new_roblox_windows(baseline, std::time::Duration::from_secs(14)).await;
        });
    }

    Ok(())
}

#[cfg(target_os = "windows")]
/// Publica o fim de um ciclo do Botting: o evento que a UI ja ouvia **e** a
/// linha no console.
///
/// As duas passagens (fila inicial e laco permanente) chamam daqui de
/// proposito. Era exatamente esse o buraco que o console tinha: acao de
/// Botting nao aparecia no historico porque so o `launch.rs` escrevia linha.
#[cfg(target_os = "windows")]
fn emit_botting_cycle(
    app: &tauri::AppHandle,
    user_id: i64,
    ok: bool,
    error: &Option<String>,
    // `(segundos ate a proxima tentativa, foi rate limit, numero da tentativa)`
    falha: Option<(i64, bool, u32)>,
) {
    let _ = app.emit(
        "botting-account-cycle",
        serde_json::json!({
            "userId": user_id,
            "ok": ok,
            "error": error,
        }),
    );

    if ok {
        emit_launch_log(app, user_id, "success", "rejoin", "Entrou no jogo pelo ciclo do Auto Rejoin");
        return;
    }

    let motivo = error
        .clone()
        .unwrap_or_else(|| String::from("erro desconhecido"));
    match falha {
        Some((delay, true, tentativa)) => emit_launch_log(
            app,
            user_id,
            "warn",
            "rejoin-retry",
            format!("Rate limit do Roblox (tentativa {tentativa}) — nova tentativa em {delay}s: {motivo}"),
        ),
        Some((delay, false, tentativa)) => emit_launch_log(
            app,
            user_id,
            "error",
            "rejoin-retry",
            format!("Falha no ciclo (tentativa {tentativa}) — nova tentativa em {delay}s: {motivo}"),
        ),
        None => emit_launch_log(app, user_id, "error", "rejoin", format!("Falha no ciclo: {motivo}")),
    }
}

async fn run_botting_session(
    app: tauri::AppHandle,
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    stopped_notify: Arc<tokio::sync::Notify>,
    config: Arc<Mutex<BottingConfig>>,
    accounts: Arc<Mutex<HashMap<i64, BottingAccountRuntime>>>,
) {
    let initial_user_ids = config
        .lock()
        .map(|c| c.user_ids.clone())
        .unwrap_or_else(|_| Vec::new());
    let mut last_launch_at: Option<std::time::Instant> = None;
    let mut auth429_cooldowns: HashMap<i64, std::time::Instant> = HashMap::new();
    // Quantas vezes cada conta ja foi reiniciada nesta sessao. O usuario pediu
    // esse numero no console, e nao existe em lugar nenhum do estado.
    let mut restarts: HashMap<i64, u32> = HashMap::new();

    if let Ok(cfg) = config.lock() {
        emit_session_log(
            &app,
            "info",
            "rejoin",
            format!(
                "Auto Rejoin iniciado — {} conta(s), place {}, ciclo de {} min, {}s entre launches",
                initial_user_ids.len(),
                cfg.place_id,
                cfg.interval_minutes,
                cfg.launch_delay_seconds
            ),
        );
    }

    for uid in &initial_user_ids {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        let adopt_running = config.lock().map(|c| c.adopt_running).unwrap_or(false);
        let interval_ms = config
            .lock()
            .map(|c| c.interval_minutes as i64 * 60_000)
            .unwrap_or(0);
        let has_client = platform::windows::tracker().get_pid(*uid).is_some();

        let mut skip_launch = false;
        if let Ok(mut map) = accounts.lock() {
            if let Some(entry) = map.get_mut(uid) {
                match botting_first_pass(adopt_running, has_client, entry.disconnected) {
                    BottingFirstPass::Disconnected => {
                        entry.phase = if has_client {
                            "disconnected-running"
                        } else {
                            "disconnected"
                        };
                        entry.next_restart_at_ms = None;
                        entry.player_grace_until_ms = None;
                        skip_launch = true;
                    }
                    BottingFirstPass::Adopt => {
                        // O cliente que o usuario abriu continua de pe. A conta
                        // entra no ciclo valendo um intervalo inteiro a partir
                        // de agora, como quem acabou de ser lancada.
                        entry.last_error = None;
                        entry.retry_count = 0;
                        if entry.is_player {
                            entry.phase = "running-player";
                            entry.next_restart_at_ms = None;
                        } else {
                            entry.phase = "running";
                            entry.next_restart_at_ms = Some(now_ms().saturating_add(interval_ms));
                        }
                        skip_launch = true;
                    }
                    BottingFirstPass::Launch => {
                        entry.phase = "launching";
                        entry.last_error = None;
                    }
                }
            }
        }
        emit_botting_status(&app);
        if skip_launch {
            continue;
        }

        let cfg = match config.lock() {
            Ok(c) => c.clone(),
            Err(_) => break,
        };
        if let Some(until) = auth429_cooldowns.get(uid).copied() {
            let now_instant = std::time::Instant::now();
            if until > now_instant {
                let remaining_ms = (until - now_instant).as_millis().min(i64::MAX as u128) as i64;
                if let Ok(mut map) = accounts.lock() {
                    if let Some(entry) = map.get_mut(uid) {
                        entry.phase = "retry-backoff";
                        entry.next_restart_at_ms = Some(now_ms().saturating_add(remaining_ms));
                    }
                }
                emit_botting_status(&app);
                continue;
            }
            auth429_cooldowns.remove(uid);
        }
        wait_for_launch_slot(&mut last_launch_at, cfg.launch_delay_seconds).await;
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        let is_player = cfg.player_user_ids.contains(uid);
        let launch_result = launch_account_for_cycle(
            &app,
            *uid,
            cfg.place_id,
            &cfg.job_id,
            &cfg.launch_data,
            is_player,
        )
        .await;
        let now = now_ms();
        let launch_ok = launch_result.is_ok();
        let launch_error = launch_result.err();
        // O que o console precisa dizer sobre a falha e calculado dentro do
        // lock (delay, tentativa) e publicado fora dele.
        let mut falha: Option<(i64, bool, u32)> = None;

        if let Ok(mut map) = accounts.lock() {
            if let Some(entry) = map.get_mut(uid) {
                if launch_ok {
                    auth429_cooldowns.remove(uid);
                    entry.retry_count = 0;
                    entry.last_error = None;
                    if entry.is_player {
                        entry.phase = "running-player";
                        entry.next_restart_at_ms = None;
                        entry.player_grace_until_ms = None;
                    } else {
                        entry.phase = "running";
                        entry.next_restart_at_ms =
                            Some(now + (cfg.interval_minutes as i64 * 60_000));
                    }
                } else {
                    entry.retry_count = entry.retry_count.saturating_add(1);
                    let mut delay = backoff_delay_seconds(
                        cfg.retry_base_seconds,
                        entry.retry_count,
                        cfg.retry_max,
                    ) as i64;
                    let is_429 = launch_error
                        .as_ref()
                        .map(|e| is_429_related_error(e))
                        .unwrap_or(false);
                    if is_429 {
                        delay = botting_429_delay_seconds(delay, cfg.launch_delay_seconds);
                        auth429_cooldowns.insert(
                            *uid,
                            std::time::Instant::now()
                                + std::time::Duration::from_secs(delay as u64),
                        );
                    } else {
                        auth429_cooldowns.remove(uid);
                    }
                    entry.phase = "retry-backoff";
                    entry.last_error = launch_error.clone();
                    entry.next_restart_at_ms = Some(now + delay * 1000);
                    falha = Some((delay, is_429, entry.retry_count));
                }
            }
        }
        emit_botting_cycle(&app, *uid, launch_ok, &launch_error, falha);
        emit_botting_status(&app);
    }

    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        let cfg = match config.lock() {
            Ok(c) => c.clone(),
            Err(_) => break,
        };
        let now = now_ms();
        let tracker = platform::windows::tracker();
        let user_ids = cfg.user_ids.clone();

        for uid in user_ids {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let mut should_launch = false;
            let mut skip_for_player = false;
            if let Ok(mut map) = accounts.lock() {
                if let Some(entry) = map.get_mut(&uid) {
                    if entry.disconnected {
                        entry.phase = if tracker.get_pid(uid).is_some() {
                            "disconnected-running"
                        } else {
                            "disconnected"
                        };
                        entry.manual_restart_pending = false;
                        entry.manual_restart_keep_schedule = false;
                        entry.manual_restart_saved_next_restart_at_ms = None;
                        entry.next_restart_at_ms = None;
                        entry.player_grace_until_ms = None;
                        skip_for_player = true;
                    } else if entry.manual_restart_pending {
                        let due = entry.next_restart_at_ms.unwrap_or(now);
                        if now >= due {
                            entry.phase = "restarting";
                            entry.last_error = None;
                            should_launch = true;
                        }
                    } else if entry.is_player {
                        if tracker.get_pid(uid).is_some() {
                            entry.phase = "running-player";
                        } else if entry.phase != "queued-player" && entry.phase != "launching" {
                            entry.phase = "queued-player";
                        }
                        entry.next_restart_at_ms = None;
                        skip_for_player = true;
                    } else if let Some(next_ms) = entry.next_restart_at_ms {
                        if now >= next_ms {
                            entry.phase = "restarting";
                            entry.last_error = None;
                            should_launch = true;
                        }
                    } else {
                        entry.next_restart_at_ms =
                            Some(now + (cfg.interval_minutes as i64 * 60_000));
                    }
                }
            }
            if skip_for_player || !should_launch {
                continue;
            }
            emit_botting_status(&app);

            if let Some(until) = auth429_cooldowns.get(&uid).copied() {
                let now_instant = std::time::Instant::now();
                if until > now_instant {
                    let remaining_ms =
                        (until - now_instant).as_millis().min(i64::MAX as u128) as i64;
                    if let Ok(mut map) = accounts.lock() {
                        if let Some(entry) = map.get_mut(&uid) {
                            entry.phase = "retry-backoff";
                            entry.next_restart_at_ms = Some(now_ms().saturating_add(remaining_ms));
                        }
                    }
                    emit_botting_status(&app);
                    continue;
                }
                auth429_cooldowns.remove(&uid);
            }
            wait_for_launch_slot(&mut last_launch_at, cfg.launch_delay_seconds).await;
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            let numero = restarts.entry(uid).or_insert(0);
            *numero = numero.saturating_add(1);
            let numero = *numero;
            emit_launch_log(
                &app,
                uid,
                "info",
                "rejoin",
                format!("Reiniciando a conta (reinicio #{numero} nesta sessao)"),
            );

            let closed = tracker.kill_for_user_graceful_async(uid, 4500).await;
            if !closed {
                let pid_hint = tracker
                    .get_pid(uid)
                    .map(|pid| format!(" (pid {})", pid))
                    .unwrap_or_default();
                let mut atraso = 0u64;
                if let Ok(mut map) = accounts.lock() {
                    if let Some(entry) = map.get_mut(&uid) {
                        entry.retry_count = entry.retry_count.saturating_add(1);
                        let retry_delay_seconds = botting_close_failure_delay_seconds(
                            cfg.retry_base_seconds,
                            entry.retry_count,
                            cfg.retry_max,
                            cfg.launch_delay_seconds,
                        );
                        entry.phase = "retry-backoff";
                        entry.last_error = Some(format!(
                            "Previous Roblox instance did not close before relaunch{}",
                            pid_hint
                        ));
                        entry.next_restart_at_ms = Some(
                            now_ms()
                                .saturating_add((retry_delay_seconds as i64).saturating_mul(1000)),
                        );
                        atraso = retry_delay_seconds;
                    }
                }
                emit_launch_log(
                    &app,
                    uid,
                    "warn",
                    "rejoin-retry",
                    format!(
                        "O cliente anterior nao fechou a tempo{pid_hint} — nova tentativa em {atraso}s"
                    ),
                );
                emit_botting_status(&app);
                continue;
            }
            tokio::time::sleep(std::time::Duration::from_millis(450)).await;

            let is_player = cfg.player_user_ids.contains(&uid);
            let launch_result = launch_account_for_cycle(
                &app,
                uid,
                cfg.place_id,
                &cfg.job_id,
                &cfg.launch_data,
                is_player,
            )
            .await;
            let now_after = now_ms();
            let launch_ok = launch_result.is_ok();
            let launch_error = launch_result.err();
            let mut falha: Option<(i64, bool, u32)> = None;

            if let Ok(mut map) = accounts.lock() {
                if let Some(entry) = map.get_mut(&uid) {
                    let restart_keep_schedule = entry.manual_restart_keep_schedule;
                    let saved_restart_due = entry.manual_restart_saved_next_restart_at_ms;
                    if launch_ok {
                        auth429_cooldowns.remove(&uid);
                        entry.retry_count = 0;
                        entry.last_error = None;
                        entry.player_grace_until_ms = None;
                        if is_player {
                            entry.is_player = true;
                            entry.disconnected = false;
                            entry.phase = "running-player";
                            entry.next_restart_at_ms = None;
                        } else {
                            entry.is_player = false;
                            entry.disconnected = false;
                            entry.phase = "running";
                            let default_due = now_after + (cfg.interval_minutes as i64 * 60_000);
                            entry.next_restart_at_ms = Some(if restart_keep_schedule {
                                saved_restart_due.unwrap_or(default_due)
                            } else {
                                default_due
                            });
                        }
                        entry.manual_restart_pending = false;
                        entry.manual_restart_keep_schedule = false;
                        entry.manual_restart_saved_next_restart_at_ms = None;
                    } else {
                        entry.retry_count = entry.retry_count.saturating_add(1);
                        let mut delay = backoff_delay_seconds(
                            cfg.retry_base_seconds,
                            entry.retry_count,
                            cfg.retry_max,
                        ) as i64;
                        let is_429 = launch_error
                            .as_ref()
                            .map(|e| is_429_related_error(e))
                            .unwrap_or(false);
                        if is_429 {
                            delay = botting_429_delay_seconds(delay, cfg.launch_delay_seconds);
                            auth429_cooldowns.insert(
                                uid,
                                std::time::Instant::now()
                                    + std::time::Duration::from_secs(delay as u64),
                            );
                        } else {
                            auth429_cooldowns.remove(&uid);
                        }
                        entry.phase = "retry-backoff";
                        entry.last_error = launch_error.clone();
                        entry.next_restart_at_ms = Some(now_after + delay * 1000);
                        entry.is_player = is_player;
                        falha = Some((delay, is_429, entry.retry_count));
                    }
                }
            }

            emit_botting_cycle(&app, uid, launch_ok, &launch_error, falha);
            emit_botting_status(&app);
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    let should_clear = BOTTING_MANAGER
        .get_session()
        .map(|s| s.id == session_id)
        .unwrap_or(false);
    if should_clear {
        BOTTING_MANAGER.replace_session(None);
    }
    stopped_notify.notify_waiters();
    emit_session_log(&app, "info", "rejoin", "Auto Rejoin parado");
    let _ = app.emit("botting-stopped", serde_json::json!({}));
    emit_botting_status(&app);
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn start_botting_mode(
    app: tauri::AppHandle,
    _state: tauri::State<'_, AccountStore>,
    settings: tauri::State<'_, SettingsStore>,
    user_ids: Vec<i64>,
    place_id: i64,
    job_id: String,
    launch_data: String,
    player_user_ids: Vec<i64>,
    interval_minutes: i64,
    launch_delay_seconds: i64,
    player_grace_minutes: i64,
    // `true` quando a sessao nasce de contas que ja estao em jogo: elas nao
    // sao fechadas nem relancadas na primeira passagem.
    adopt_running: Option<bool>,
) -> Result<BottingStatusPayload, String> {
    if user_ids.len() < 2 {
        return Err("Select at least two accounts for Auto Rejoin".into());
    }
    if place_id <= 0 {
        return Err("Place ID must be greater than 0".into());
    }
    if !settings.get_bool("General", "EnableMultiRbx") {
        return Err("Auto Rejoin currently requires Multi Roblox to be enabled".into());
    }

    let dedup = dedupe_preserving_order(user_ids);
    if dedup.len() < 2 {
        return Err("Select at least two unique accounts for Auto Rejoin".into());
    }

    let player_set = botting_player_set(&dedup, player_user_ids)?;

    if let Some(existing) = BOTTING_MANAGER.get_session() {
        existing.stop_flag.store(true, Ordering::Relaxed);
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            existing.stopped_notify.notified(),
        )
        .await;
        let should_clear = BOTTING_MANAGER
            .get_session()
            .map(|s| s.id == existing.id)
            .unwrap_or(false);
        if should_clear {
            BOTTING_MANAGER.replace_session(None);
        }
    }

    let (retry_max, retry_base_seconds, default_player_grace_minutes) =
        botting_retry_config(&settings);

    let interval_minutes = clamp_botting_interval_minutes(interval_minutes);
    let launch_delay_seconds = clamp_botting_launch_delay_seconds(launch_delay_seconds);
    let player_grace_minutes =
        resolve_player_grace_minutes(player_grace_minutes, default_player_grace_minutes);

    let cfg = BottingConfig {
        user_ids: dedup.clone(),
        place_id,
        job_id,
        launch_data,
        player_user_ids: player_set,
        interval_minutes,
        launch_delay_seconds,
        retry_max,
        retry_base_seconds,
        player_grace_minutes,
        adopt_running: adopt_running.unwrap_or(false),
    };

    let mut runtime_map = HashMap::new();
    for uid in &dedup {
        let is_player = cfg.player_user_ids.contains(uid);
        runtime_map.insert(
            *uid,
            BottingAccountRuntime {
                user_id: *uid,
                is_player,
                disconnected: false,
                manual_restart_pending: false,
                manual_restart_keep_schedule: false,
                manual_restart_saved_next_restart_at_ms: None,
                phase: if is_player { "queued-player" } else { "queued" },
                retry_count: 0,
                next_restart_at_ms: None,
                player_grace_until_ms: None,
                last_error: None,
            },
        );
    }

    let session_id = BOTTING_MANAGER.next_session_id();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stopped_notify = Arc::new(tokio::sync::Notify::new());
    let session = BottingSession {
        id: session_id,
        stop_flag: stop_flag.clone(),
        stopped_notify: stopped_notify.clone(),
        started_at_ms: now_ms(),
        config: Arc::new(Mutex::new(cfg)),
        accounts: Arc::new(Mutex::new(runtime_map)),
    };

    BOTTING_MANAGER.replace_session(Some(session.clone()));
    emit_botting_status(&app);

    tokio::spawn(run_botting_session(
        app.clone(),
        session_id,
        stop_flag,
        stopped_notify,
        session.config.clone(),
        session.accounts.clone(),
    ));

    Ok(current_botting_status())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn start_botting_mode(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AccountStore>,
    _settings: tauri::State<'_, SettingsStore>,
    _user_ids: Vec<i64>,
    _place_id: i64,
    _job_id: String,
    _launch_data: String,
    _player_user_ids: Vec<i64>,
    _interval_minutes: i64,
    _launch_delay_seconds: i64,
    _player_grace_minutes: i64,
    _adopt_running: Option<bool>,
) -> Result<BottingStatusPayload, String> {
    Err("Auto Rejoin is only supported on Windows".into())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn stop_botting_mode(app: tauri::AppHandle, close_bot_accounts: bool) -> Result<(), String> {
    let session = BOTTING_MANAGER.get_session();
    if let Some(session) = session {
        session.stop_flag.store(true, Ordering::Relaxed);
        if close_bot_accounts {
            let cfg = session.config.lock().map_err(|e| e.to_string())?.clone();
            let tracker = platform::windows::tracker();
            // Close only this session's bot clients — never players, and never
            // clients the user opened outside the botting session.
            for uid in cfg
                .user_ids
                .iter()
                .filter(|uid| !cfg.player_user_ids.contains(uid))
            {
                let _ = tracker.kill_for_user(*uid);
            }
            let _ = tracker.cleanup_dead_processes();
        }
        BOTTING_MANAGER.replace_session(None);
    }
    let _ = app.emit("botting-stopped", serde_json::json!({}));
    emit_botting_status(&app);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn stop_botting_mode(_app: tauri::AppHandle, _close_bot_accounts: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn get_botting_mode_status() -> Result<BottingStatusPayload, String> {
    Ok(current_botting_status())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn get_botting_mode_status() -> Result<BottingStatusPayload, String> {
    Ok(BottingStatusPayload::default())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn add_botting_accounts(
    app: tauri::AppHandle,
    state: tauri::State<'_, AccountStore>,
    user_ids: Vec<i64>,
) -> Result<BottingStatusPayload, String> {
    let Some(session) = BOTTING_MANAGER.get_session() else {
        return Err("Auto Rejoin is not running".into());
    };
    if user_ids.is_empty() {
        return Err("Select at least one account to add".into());
    }

    let all_accounts = state.get_all()?;
    let known_ids: HashSet<i64> = all_accounts.iter().map(|a| a.user_id).collect();

    let requested = dedupe_preserving_order(user_ids);

    for uid in &requested {
        if !known_ids.contains(uid) {
            return Err(format!("Account {} not found", uid));
        }
    }

    let tracker = platform::windows::tracker();
    let now = now_ms();

    let mut cfg = session.config.lock().map_err(|e| e.to_string())?;
    let launch_delay_ms = (cfg.launch_delay_seconds as i64).saturating_mul(1000);
    let interval_ms = (cfg.interval_minutes as i64).saturating_mul(60_000);
    let mut runtime_map = session.accounts.lock().map_err(|e| e.to_string())?;

    let mut to_add = Vec::new();
    for uid in requested {
        if cfg.user_ids.contains(&uid) || runtime_map.contains_key(&uid) {
            continue;
        }
        to_add.push(uid);
    }

    if to_add.is_empty() {
        return Err("Selected accounts are already in Auto Rejoin".into());
    }

    for uid in to_add {
        cfg.user_ids.push(uid);

        let has_running_client = tracker.get_pid(uid).is_some();
        let (phase, next_restart_at_ms) =
            botting_add_schedule(has_running_client, now, interval_ms, launch_delay_ms);

        runtime_map.insert(
            uid,
            BottingAccountRuntime {
                user_id: uid,
                is_player: false,
                disconnected: false,
                manual_restart_pending: false,
                manual_restart_keep_schedule: false,
                manual_restart_saved_next_restart_at_ms: None,
                phase,
                retry_count: 0,
                next_restart_at_ms,
                player_grace_until_ms: None,
                last_error: None,
            },
        );
    }
    drop(runtime_map);
    drop(cfg);

    emit_botting_status(&app);
    Ok(current_botting_status())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn add_botting_accounts(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AccountStore>,
    _user_ids: Vec<i64>,
) -> Result<BottingStatusPayload, String> {
    Err("Auto Rejoin is only supported on Windows".into())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn set_botting_player_accounts(
    app: tauri::AppHandle,
    player_user_ids: Vec<i64>,
) -> Result<BottingStatusPayload, String> {
    let Some(session) = BOTTING_MANAGER.get_session() else {
        return Err("Auto Rejoin is not running".into());
    };

    let mut cfg = session.config.lock().map_err(|e| e.to_string())?;
    let mut next_set = HashSet::new();
    for uid in player_user_ids {
        if !cfg.user_ids.contains(&uid) {
            return Err("Main Account must be one of the Auto Rejoin accounts".into());
        }
        next_set.insert(uid);
    }

    let old_set = cfg.player_user_ids.clone();
    cfg.player_user_ids = next_set.clone();
    let grace_ms = cfg.player_grace_minutes as i64 * 60_000;
    drop(cfg);

    let tracker = platform::windows::tracker();
    let mut accounts = session.accounts.lock().map_err(|e| e.to_string())?;
    for entry in accounts.values_mut() {
        let was_player = old_set.contains(&entry.user_id);
        let is_player = next_set.contains(&entry.user_id);

        if is_player {
            entry.is_player = true;
            entry.disconnected = false;
            entry.manual_restart_pending = false;
            entry.manual_restart_keep_schedule = false;
            entry.manual_restart_saved_next_restart_at_ms = None;
            entry.retry_count = 0;
            entry.last_error = None;
            entry.player_grace_until_ms = None;
            entry.next_restart_at_ms = None;
            entry.phase = if tracker.get_pid(entry.user_id).is_some() {
                "running-player"
            } else {
                "queued-player"
            };
            continue;
        }

        if was_player && !is_player {
            entry.is_player = false;
            entry.manual_restart_pending = false;
            entry.manual_restart_keep_schedule = false;
            entry.manual_restart_saved_next_restart_at_ms = None;
            entry.retry_count = 0;
            entry.last_error = None;
            if tracker.get_pid(entry.user_id).is_some() {
                let due = now_ms() + grace_ms;
                entry.phase = "player-grace";
                entry.player_grace_until_ms = Some(due);
                entry.next_restart_at_ms = Some(due);
            } else {
                entry.phase = "queued";
                entry.player_grace_until_ms = None;
                entry.next_restart_at_ms = Some(now_ms());
            }
        }
    }
    drop(accounts);

    emit_botting_status(&app);
    Ok(current_botting_status())
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum BottingAccountAction {
    Disconnect,
    Close,
    CloseDisconnect,
    RestartClient,
    RestartLoop,
}

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum BottingAccountAction {
    Disconnect,
    Close,
    CloseDisconnect,
    RestartClient,
    RestartLoop,
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn botting_account_action(
    app: tauri::AppHandle,
    user_id: i64,
    action: BottingAccountAction,
) -> Result<BottingStatusPayload, String> {
    let Some(session) = BOTTING_MANAGER.get_session() else {
        return Err("Auto Rejoin is not running".into());
    };

    let (should_disconnect, should_close, should_restart_client, should_restart_loop) =
        botting_action_flags(&action);

    let tracker = platform::windows::tracker();

    let (is_player_from_config, interval_ms) = {
        let cfg = session.config.lock().map_err(|e| e.to_string())?;
        if !cfg.user_ids.contains(&user_id) {
            return Err("Account is not part of the current Auto Rejoin session".into());
        }
        (
            cfg.player_user_ids.contains(&user_id),
            (cfg.interval_minutes as i64).saturating_mul(60_000),
        )
    };

    if should_disconnect && is_player_from_config {
        return Err(
            "Player accounts cannot be disconnected; remove them from Player Accounts first".into(),
        );
    }

    if should_disconnect {
        let accounts = session.accounts.lock().map_err(|e| e.to_string())?;
        let Some(entry) = accounts.get(&user_id) else {
            return Err("Account runtime is missing for the current Auto Rejoin session".into());
        };
        if entry.is_player {
            return Err(
                "Player accounts cannot be disconnected; remove them from Player Accounts first"
                    .into(),
            );
        }
    }

    if should_close {
        let _ = tracker.kill_for_user(user_id);
    }

    let now = now_ms();
    {
        let mut accounts = session.accounts.lock().map_err(|e| e.to_string())?;
        let Some(entry) = accounts.get_mut(&user_id) else {
            return Err("Account runtime is missing for the current Auto Rejoin session".into());
        };
        let was_disconnected = entry.disconnected;
        let is_player = is_player_from_config || entry.is_player;

        if should_disconnect && is_player {
            return Err(
                "Player accounts cannot be disconnected; remove them from Player Accounts first"
                    .into(),
            );
        }

        entry.retry_count = 0;
        entry.last_error = None;
        entry.player_grace_until_ms = None;
        entry.is_player = is_player;
        entry.disconnected = should_disconnect;
        entry.manual_restart_pending = should_restart_loop || should_restart_client;
        entry.manual_restart_keep_schedule = should_restart_client && !is_player;
        entry.manual_restart_saved_next_restart_at_ms = if entry.manual_restart_keep_schedule {
            entry.next_restart_at_ms
        } else {
            None
        };

        if entry.disconnected {
            entry.next_restart_at_ms = None;
            entry.phase = if !should_close && tracker.get_pid(user_id).is_some() {
                "disconnected-running"
            } else {
                "disconnected"
            };
        } else if should_restart_loop || should_restart_client {
            entry.next_restart_at_ms = Some(now);
            entry.phase = "restarting";
        } else if is_player {
            entry.next_restart_at_ms = None;
            entry.phase = if !should_close && tracker.get_pid(user_id).is_some() {
                "running-player"
            } else {
                "queued-player"
            };
        } else if matches!(action, BottingAccountAction::Close) && was_disconnected {
            entry.next_restart_at_ms = Some(now);
            entry.phase = "restarting";
        } else {
            entry.manual_restart_keep_schedule = false;
            entry.manual_restart_saved_next_restart_at_ms = None;
            let next_due = entry
                .next_restart_at_ms
                .unwrap_or_else(|| now.saturating_add(interval_ms));
            entry.next_restart_at_ms = Some(next_due);
            entry.phase = if next_due <= now {
                "queued"
            } else {
                "waiting-rejoin"
            };
        }
    }

    emit_botting_status(&app);
    Ok(current_botting_status())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn botting_account_action(
    _app: tauri::AppHandle,
    _user_id: i64,
    _action: BottingAccountAction,
) -> Result<BottingStatusPayload, String> {
    Ok(BottingStatusPayload::default())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn set_botting_player_accounts(
    _app: tauri::AppHandle,
    _player_user_ids: Vec<i64>,
) -> Result<BottingStatusPayload, String> {
    Ok(BottingStatusPayload::default())
}

#[cfg(test)]
mod botting_command_tests {
    use super::*;

    #[allow(dead_code)]
    fn temp_settings(tag: &str) -> SettingsStore {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        SettingsStore::new(std::env::temp_dir().join(format!("ram-botting-{tag}-{nanos}.ini")))
    }

    // ---- dedupe_preserving_order -------------------------------------------

    #[test]
    fn dedupe_preserving_order_keeps_the_first_occurrence_in_order() {
        assert_eq!(
            dedupe_preserving_order(vec![30, 10, 30, 20, 10]),
            vec![30, 10, 20]
        );
    }

    #[test]
    fn dedupe_preserving_order_handles_empty_and_single_selections() {
        assert_eq!(dedupe_preserving_order(vec![]), Vec::<i64>::new());
        assert_eq!(dedupe_preserving_order(vec![7]), vec![7]);
        assert_eq!(dedupe_preserving_order(vec![7, 7, 7]), vec![7]);
    }

    #[test]
    fn dedupe_preserving_order_does_not_filter_out_odd_ids() {
        // Filtering non-positive ids is not this helper's job; Botting Mode
        // validates membership separately.
        assert_eq!(
            dedupe_preserving_order(vec![0, -1, i64::MIN, i64::MAX, 0]),
            vec![0, -1, i64::MIN, i64::MAX]
        );
    }

    #[test]
    fn dedupe_preserving_order_scales_to_a_large_selection() {
        let mut input: Vec<i64> = (1..=500).collect();
        input.extend(1..=500);
        let deduped = dedupe_preserving_order(input);
        assert_eq!(deduped.len(), 500);
        assert_eq!(deduped[0], 1);
        assert_eq!(deduped[499], 500);
    }

    // ---- botting_player_set -------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_player_set_accepts_players_inside_the_selection() {
        let set = botting_player_set(&[1, 2, 3], vec![1, 3]).unwrap();
        assert_eq!(set.len(), 2);
        assert!(set.contains(&1));
        assert!(set.contains(&3));
        assert!(!set.contains(&2));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_player_set_is_empty_when_no_player_is_chosen() {
        assert!(botting_player_set(&[1, 2], vec![]).unwrap().is_empty());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_player_set_dedupes_repeated_player_ids() {
        let set = botting_player_set(&[1, 2], vec![2, 2, 2]).unwrap();
        assert_eq!(set.len(), 1);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_player_set_rejects_a_player_outside_the_selection() {
        // Otherwise the account would be marked "player" but never launched.
        assert_eq!(
            botting_player_set(&[1, 2], vec![9]).unwrap_err(),
            "Player Account must be one of the selected accounts"
        );
        assert!(botting_player_set(&[], vec![1]).is_err());
    }

    // ---- botting_retry_config -----------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_retry_config_uses_the_shipped_defaults() {
        let settings = temp_settings("retry-defaults");
        assert_eq!(botting_retry_config(&settings), (6, 8, 15));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_retry_config_clamps_each_value_into_its_range() {
        let settings = temp_settings("retry-clamp-high");
        settings.set("General", "BottingRetryMax", "9999").unwrap();
        settings
            .set("General", "BottingRetryBaseSeconds", "9999")
            .unwrap();
        settings
            .set("General", "BottingPlayerGraceMinutes", "9999")
            .unwrap();
        assert_eq!(botting_retry_config(&settings), (20, 120, 90));

        let settings = temp_settings("retry-clamp-low");
        settings.set("General", "BottingRetryMax", "-5").unwrap();
        settings
            .set("General", "BottingRetryBaseSeconds", "0")
            .unwrap();
        settings
            .set("General", "BottingPlayerGraceMinutes", "-1")
            .unwrap();
        assert_eq!(botting_retry_config(&settings), (1, 5, 1));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_retry_config_falls_back_to_defaults_for_unparsable_values() {
        let settings = temp_settings("retry-garbage");
        settings.set("General", "BottingRetryMax", "many").unwrap();
        settings
            .set("General", "BottingRetryBaseSeconds", "8.5")
            .unwrap();
        assert_eq!(botting_retry_config(&settings), (6, 8, 15));
    }

    // ---- clamps -------------------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn clamp_botting_interval_minutes_keeps_10_to_480() {
        assert_eq!(clamp_botting_interval_minutes(19), 19);
        assert_eq!(clamp_botting_interval_minutes(10), 10);
        assert_eq!(clamp_botting_interval_minutes(120), 120);
        assert_eq!(clamp_botting_interval_minutes(480), 480);
        assert_eq!(clamp_botting_interval_minutes(481), 480);
        assert_eq!(clamp_botting_interval_minutes(9), 10);
        assert_eq!(clamp_botting_interval_minutes(0), 10);
        assert_eq!(clamp_botting_interval_minutes(-100), 10);
        assert_eq!(clamp_botting_interval_minutes(100_000), 480);
        assert_eq!(clamp_botting_interval_minutes(i64::MAX), 480);
        assert_eq!(clamp_botting_interval_minutes(i64::MIN), 10);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn clamp_botting_launch_delay_seconds_keeps_5_to_120() {
        assert_eq!(clamp_botting_launch_delay_seconds(20), 20);
        assert_eq!(clamp_botting_launch_delay_seconds(5), 5);
        assert_eq!(clamp_botting_launch_delay_seconds(120), 120);
        assert_eq!(clamp_botting_launch_delay_seconds(0), 5);
        assert_eq!(clamp_botting_launch_delay_seconds(-1), 5);
        assert_eq!(clamp_botting_launch_delay_seconds(i64::MAX), 120);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolve_player_grace_minutes_uses_the_default_for_zero_or_negative() {
        assert_eq!(resolve_player_grace_minutes(0, 15), 15);
        assert_eq!(resolve_player_grace_minutes(-30, 15), 15);
        assert_eq!(resolve_player_grace_minutes(i64::MIN, 42), 42);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolve_player_grace_minutes_clamps_an_explicit_request() {
        assert_eq!(resolve_player_grace_minutes(30, 15), 30);
        assert_eq!(resolve_player_grace_minutes(1, 15), 1);
        assert_eq!(resolve_player_grace_minutes(90, 15), 90);
        assert_eq!(resolve_player_grace_minutes(9999, 15), 90);
        assert_eq!(resolve_player_grace_minutes(i64::MAX, 15), 90);
    }

    // ---- 429 backoff --------------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_429_delay_seconds_enforces_the_45_second_floor() {
        assert_eq!(botting_429_delay_seconds(5, 20), 45);
        assert_eq!(botting_429_delay_seconds(0, 5), 45);
        assert_eq!(botting_429_delay_seconds(-100, 5), 45);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_429_delay_seconds_also_enforces_two_launch_slots() {
        // With a 120s launch delay the retry must wait at least 240s, well past
        // the 45s floor.
        assert_eq!(botting_429_delay_seconds(45, 120), 240);
        assert_eq!(botting_429_delay_seconds(300, 120), 300);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_429_delay_seconds_does_not_overflow_on_absurd_input() {
        assert_eq!(botting_429_delay_seconds(i64::MAX, 10), i64::MAX);
        // A launch delay past `i64::MAX` wraps negative on the cast and the
        // 45s floor takes over, so the result stays sane instead of panicking.
        // Unreachable in practice: the delay is clamped to 5..=120 on start.
        assert_eq!(botting_429_delay_seconds(1, u64::MAX), 45);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_close_failure_delay_seconds_stays_between_6_and_300() {
        // Small base, small launch delay -> the 6s floor.
        assert_eq!(botting_close_failure_delay_seconds(5, 1, 6, 1), 6);
        // Two launch slots win over the exponential backoff.
        assert_eq!(botting_close_failure_delay_seconds(5, 1, 6, 60), 120);
        // Ceiling holds even with a huge launch delay.
        assert_eq!(botting_close_failure_delay_seconds(120, 20, 20, 120), 300);
        assert_eq!(botting_close_failure_delay_seconds(5, 1, 6, u64::MAX), 300);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_close_failure_delay_seconds_grows_with_the_retry_count() {
        let first = botting_close_failure_delay_seconds(8, 1, 6, 5);
        let third = botting_close_failure_delay_seconds(8, 3, 6, 5);
        assert!(third > first, "{third} should be larger than {first}");
    }

    // ---- botting_add_schedule ----------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_add_schedule_gives_a_running_client_a_full_interval() {
        let (phase, due) = botting_add_schedule(true, 1_000, 60_000, 5_000);
        assert_eq!(phase, "running");
        assert_eq!(due, Some(61_000));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_add_schedule_queues_an_idle_account_after_one_launch_slot() {
        let (phase, due) = botting_add_schedule(false, 1_000, 60_000, 5_000);
        assert_eq!(phase, "queued");
        assert_eq!(due, Some(6_000));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_add_schedule_saturates_instead_of_overflowing() {
        let (_, due) = botting_add_schedule(true, i64::MAX, 60_000, 5_000);
        assert_eq!(due, Some(i64::MAX));
        let (_, due) = botting_add_schedule(false, i64::MAX, 60_000, i64::MAX);
        assert_eq!(due, Some(i64::MAX));
    }

    // ---- adocao de conta ja em jogo -----------------------------------------

    /// Ligar o Botting em contas que ja estavam jogando derrubava todas elas:
    /// a primeira passagem fecha o cliente e abre outro no place da sessao. Em
    /// modo adocao, quem ja esta em jogo fica de pe e so entra no ciclo no
    /// primeiro vencimento do intervalo.
    #[cfg(target_os = "windows")]
    #[test]
    fn adopting_keeps_the_client_that_is_already_running() {
        assert_eq!(botting_first_pass(true, true, false), BottingFirstPass::Adopt);
        // Sem cliente aberto nao ha o que adotar: lanca como sempre.
        assert_eq!(botting_first_pass(true, false, false), BottingFirstPass::Launch);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn without_adoption_the_old_behaviour_is_untouched() {
        assert_eq!(botting_first_pass(false, true, false), BottingFirstPass::Launch);
        assert_eq!(botting_first_pass(false, false, false), BottingFirstPass::Launch);
    }

    /// Conta desconectada fica fora do ciclo, adotando ou nao — senao o modo
    /// adocao ressuscitaria quem o usuario tirou de proposito.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_disconnected_account_is_never_launched_nor_adopted() {
        for adopt in [true, false] {
            for running in [true, false] {
                assert_eq!(
                    botting_first_pass(adopt, running, true),
                    BottingFirstPass::Disconnected,
                    "adopt={adopt} running={running}"
                );
            }
        }
    }

    // ---- botting_action_flags ----------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_action_flags_disconnect_only_disconnects() {
        assert_eq!(
            botting_action_flags(&BottingAccountAction::Disconnect),
            (true, false, false, false)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_action_flags_close_only_closes() {
        assert_eq!(
            botting_action_flags(&BottingAccountAction::Close),
            (false, true, false, false)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_action_flags_close_disconnect_does_both() {
        assert_eq!(
            botting_action_flags(&BottingAccountAction::CloseDisconnect),
            (true, true, false, false)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_action_flags_restart_actions_close_but_never_disconnect() {
        assert_eq!(
            botting_action_flags(&BottingAccountAction::RestartClient),
            (false, true, true, false)
        );
        assert_eq!(
            botting_action_flags(&BottingAccountAction::RestartLoop),
            (false, true, false, true)
        );
    }

    // ---- BottingAccountAction wire format ----------------------------------

    #[test]
    fn botting_account_action_deserializes_the_camel_case_names_the_ui_sends() {
        let parse = |s: &str| serde_json::from_str::<BottingAccountAction>(s).is_ok();
        assert!(parse("\"disconnect\""));
        assert!(parse("\"close\""));
        assert!(parse("\"closeDisconnect\""));
        assert!(parse("\"restartClient\""));
        assert!(parse("\"restartLoop\""));
        // Anything else must be rejected rather than silently mapped.
        assert!(!parse("\"CloseDisconnect\""));
        assert!(!parse("\"close_disconnect\""));
        assert!(!parse("\"\""));
        assert!(!parse("null"));
    }

    // ---- botting_tracked_version ---------------------------------------------

    /// Caso normal: old join realmente abriu a versão resolvida, e é ela que
    /// `has_version_conflict` (na fila de launch) precisa enxergar.
    #[cfg(target_os = "windows")]
    #[test]
    fn botting_tracked_version_reports_the_resolved_version_on_old_join() {
        assert_eq!(
            botting_tracked_version(true, Some("LIVE:version-aaa".to_string())),
            Some("LIVE:version-aaa".to_string())
        );
        // Old join sem versão resolvida (nem da conta, nem do catálogo): o
        // registro decide o canal, e "instalação do sistema" já é a chave
        // `None` que o tracker usa para isso.
        assert_eq!(botting_tracked_version(true, None), None);
    }

    /// A Global Constraint do plano de sync com o upstream (`CLAUDE.md`): pelo
    /// protocolo o cliente sempre abre a build de produção, então reportar uma
    /// versão de conta aqui mentiria para a guarda de conflito. Na prática
    /// `resolve_use_old_join` nunca deixa `resolved_version_id` ser `Some`
    /// junto de `use_old_join = false`, mas a função não confia nisso.
    #[cfg(target_os = "windows")]
    #[test]
    fn botting_tracked_version_never_reports_a_version_when_going_by_url() {
        assert_eq!(
            botting_tracked_version(false, Some("LIVE:version-aaa".to_string())),
            None
        );
        assert_eq!(botting_tracked_version(false, None), None);
    }

    // ---- status payload ------------------------------------------------------

    #[test]
    fn get_botting_mode_status_reports_no_session_by_default() {
        let status = get_botting_mode_status().expect("status should be readable");
        assert!(!status.active);
        assert!(status.user_ids.is_empty());
        assert!(status.accounts.is_empty());
        assert_eq!(status.started_at_ms, None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn current_botting_status_matches_get_botting_mode_status() {
        let direct = current_botting_status();
        let via_command = get_botting_mode_status().unwrap();
        assert_eq!(direct.active, via_command.active);
        assert_eq!(direct.user_ids, via_command.user_ids);
    }
}

/// O console do app e o historico geral das acoes, nao so do launch. Estes
/// testes leem o proprio arquivo porque o que precisa ser garantido e
/// estrutural: existe **um** caminho para publicar o fim de um ciclo, e ele
/// escreve no console. Testar de outro jeito exigiria um `AppHandle` de Tauri.
#[cfg(test)]
mod botting_console_tests {
    /// So o codigo de producao: o proprio arquivo de teste cita os mesmos
    /// trechos, e contar as citacoes daria numero errado.
    fn fonte() -> &'static str {
        const TUDO: &str = include_str!("botting.rs");
        let fim = TUDO.find("
#[cfg(test)]").unwrap_or(TUDO.len());
        &TUDO[..fim]
    }

    /// Corta o corpo de uma funcao a partir da assinatura ate a chave final na
    /// coluna zero.
    fn corpo(assinatura: &str) -> &'static str {
        corpo_em(fonte(), assinatura)
    }

    fn corpo_em<'a>(fonte: &'a str, assinatura: &str) -> &'a str {
        let inicio = fonte
            .find(assinatura)
            .unwrap_or_else(|| panic!("nao achei `{assinatura}` em botting.rs"));
        let resto = &fonte[inicio..];
        let fim = fim_da_funcao(resto).unwrap_or(resto.len());
        &resto[..fim]
    }

    /// Posição do `}` de coluna zero que fecha a função — seguido de `\n` ou de
    /// `\r\n`: a CI (runner Windows) faz checkout com CRLF, e `include_str!`
    /// entrega o arquivo como está no disco.
    fn fim_da_funcao(resto: &str) -> Option<usize> {
        resto
            .match_indices("\n}")
            .map(|(i, _)| i)
            .find(|&i| matches!(resto.as_bytes().get(i + 2), Some(b'\n' | b'\r')))
    }

    /// A CI roda num runner Windows, que faz checkout com CRLF: sem achar o `}`
    /// de coluna zero, o "corpo" virava o resto do arquivo e as checagens abaixo
    /// passavam ou falhavam por acaso.
    #[test]
    fn o_corte_do_corpo_nao_depende_do_fim_de_linha() {
        let lf = "fn a() {\n    um();\n}\n\nfn b() {\n    dois();\n}\n";
        let crlf = lf.replace('\n', "\r\n");
        for fonte in [lf, crlf.as_str()] {
            let corpo = corpo_em(fonte, "fn a(");
            assert!(corpo.contains("um()"), "{corpo:?}");
            assert!(!corpo.contains("dois()"), "o corte passou do fim da funcao: {corpo:?}");
        }
    }

    #[test]
    fn o_evento_de_ciclo_e_publicado_num_lugar_so() {
        // Duas passagens (fila inicial e laco) emitiam o evento por conta
        // propria; uma delas ficaria sem linha de console ao se acrescentar o
        // log em apenas uma. Agora as duas passam pelo mesmo helper.
        assert_eq!(
            fonte().matches("\"botting-account-cycle\"").count(),
            1,
            "so `emit_botting_cycle` pode emitir esse evento"
        );
        assert_eq!(
            fonte().matches("emit_botting_cycle(&app,").count(),
            2,
            "as duas passagens chamam o helper"
        );
    }

    #[test]
    fn o_ciclo_escreve_no_console_no_sucesso_e_na_falha() {
        let helper = corpo("fn emit_botting_cycle(");
        assert!(
            helper.matches("emit_launch_log(").count() >= 4,
            "sucesso, rate limit, falha com retry e falha sem retry"
        );
        assert!(helper.contains("\"success\""));
        assert!(helper.contains("Rate limit"));
    }

    #[test]
    fn o_inicio_e_o_fim_da_sessao_aparecem_no_console() {
        let sessao = corpo("async fn run_botting_session(");
        assert!(
            sessao.contains("Auto Rejoin iniciado"),
            "quem abre o console depois precisa saber que a sessao comecou"
        );
        assert!(sessao.contains("Auto Rejoin parado"));
        // Linha de sessao nao pertence a conta nenhuma: `userId` nulo, senao o
        // console imprime "0" no lugar do nome.
        assert!(sessao.contains("emit_session_log("));
    }

    #[test]
    fn o_reinicio_diz_quantas_vezes_a_conta_ja_reiniciou() {
        let sessao = corpo("async fn run_botting_session(");
        assert!(sessao.contains("reinicio #"), "o numero foi pedido explicitamente");
        assert!(sessao.contains("restarts.entry(uid)"));
    }

    /// Regressão: o ciclo do Botting chamava `tracker.track` sem versão, então
    /// `has_version_conflict` (a guarda que a fila de launch usa para recusar
    /// abrir numa versão diferente da que já está rodando) não enxergava os
    /// clientes abertos pelo Botting — Botting numa versão e um launch avulso
    /// noutra abriam lado a lado sem aviso. O ciclo precisa registrar a versão
    /// resolvida do jeito que `commands/launch.rs` já faz, através da função
    /// pura `botting_tracked_version` (testada à parte).
    #[test]
    fn o_ciclo_registra_a_versao_resolvida_no_tracker() {
        let corpo = corpo("async fn launch_account_for_cycle(");
        assert!(
            corpo.contains("tracker.track_with_version("),
            "sem isso `has_version_conflict` na fila de launch nao ve os clientes do Botting"
        );
        assert!(
            corpo.contains("botting_tracked_version(use_old_join, resolved_version_id"),
            "a versao reportada precisa vir da funcao pura que respeita a regra de canal/build"
        );
        assert!(
            !corpo.contains("tracker.track(user_id, pid, browser_tracker_id);"),
            "o `track` sem versao e exatamente o bug: reporta o PID sem dizer em qual build ele abriu"
        );
    }
}

/// O recurso se chama **Auto Rejoin** na tela. Por dentro tudo continua
/// `botting` — chave do `RAMSettings.ini`, comando Tauri, evento, nome de
/// arquivo, de módulo e de função — porque renomear isso apagaria a
/// configuração de quem já usa o app.
///
/// O backend também escreve texto que o usuário lê: erro que sobe para a tela
/// (`Err("...")`) e linha do console (`emit_launch_log` / `emit_session_log`).
/// É por ali que o nome antigo volta sem ninguém notar, porque nada no
/// frontend cobre string que nasce no Rust. Esta varredura fecha esse lado.
#[cfg(test)]
mod auto_rejoin_naming_tests {
    /// Só o código de produção de cada arquivo: o módulo de teste cita os
    /// nomes de propósito e contaria como vazamento.
    fn producao(fonte: &'static str) -> &'static str {
        let fim = fonte.find("\n#[cfg(test)]").unwrap_or(fonte.len());
        &fonte[..fim]
    }

    /// Strings literais do fonte, com comentário de fora: em comentário o nome
    /// interno é o nome certo, e reprovar por ele forçaria a reescrever a
    /// explicação de código que continua se chamando `botting`.
    fn literais(fonte: &str) -> Vec<String> {
        let cs: Vec<char> = fonte.chars().collect();
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < cs.len() {
            // Comentário de linha.
            if cs[i] == '/' && cs.get(i + 1) == Some(&'/') {
                while i < cs.len() && cs[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            // Comentário de bloco.
            if cs[i] == '/' && cs.get(i + 1) == Some(&'*') {
                i += 2;
                while i + 1 < cs.len() && !(cs[i] == '*' && cs[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(cs.len());
                continue;
            }
            // Literal de caractere (`'"'` enganaria o scanner de string).
            if cs[i] == '\'' && cs.get(i + 2) == Some(&'\'') && cs.get(i + 1) != Some(&'\\') {
                i += 3;
                continue;
            }
            if cs[i] == '"' {
                i += 1;
                let mut s = String::new();
                while i < cs.len() && cs[i] != '"' {
                    if cs[i] == '\\' {
                        i += 1;
                        if i < cs.len() {
                            s.push(cs[i]);
                            i += 1;
                        }
                        continue;
                    }
                    s.push(cs[i]);
                    i += 1;
                }
                i += 1;
                out.push(s);
                continue;
            }
            i += 1;
        }
        out
    }

    /// `needle` como palavra inteira: `BottingPlayer` e `supportsBotting` são
    /// nome interno legítimo e não podem reprovar a varredura.
    fn palavra_inteira(texto: &str, needle: &str) -> bool {
        let mut de = 0usize;
        while let Some(pos) = texto[de..].find(needle) {
            let ini = de + pos;
            let fim = ini + needle.len();
            let colado = |c: char| c.is_alphanumeric() || c == '_';
            let antes_livre = texto[..ini].chars().next_back().map_or(true, |c| !colado(c));
            let depois_livre = texto[fim..].chars().next().map_or(true, |c| !colado(c));
            if antes_livre && depois_livre {
                return true;
            }
            de = fim;
        }
        false
    }

    fn fontes() -> [(&'static str, &'static str); 2] {
        [
            ("botting.rs", producao(include_str!("botting.rs"))),
            ("platform_info.rs", producao(include_str!("platform_info.rs"))),
        ]
    }

    #[test]
    fn nenhuma_frase_do_backend_diz_o_nome_antigo() {
        let mut vazamentos: Vec<String> = Vec::new();
        for (arquivo, fonte) in fontes() {
            for literal in literais(fonte) {
                let frase = literal.contains(' ');
                // Fora de frase o nome minúsculo é identificador (`botting-status`,
                // `start_botting_mode`); dentro dela é texto que o usuário lê.
                let vazou = palavra_inteira(&literal, "Botting")
                    || (frase && palavra_inteira(&literal.to_lowercase(), "botting"));
                if vazou {
                    vazamentos.push(format!("{arquivo}: {literal:?}"));
                }
            }
        }
        assert!(
            vazamentos.is_empty(),
            "texto de tela ainda diz o nome antigo: {vazamentos:#?}"
        );
    }

    #[test]
    fn a_varredura_enxerga_o_fonte_e_poupa_o_nome_interno() {
        let (_, botting) = fontes()[0];
        let achados = literais(botting);
        assert!(
            achados.len() > 50,
            "sem literais a varredura passaria sempre: {}",
            achados.len()
        );
        assert!(palavra_inteira("Start Auto Rejoin", "Rejoin"));
        assert!(!palavra_inteira("BottingPlayerGraceMinutes", "Botting"));
        assert!(!palavra_inteira("supportsBotting", "Botting"));
        // Comentário não conta: é onde o nome interno continua valendo.
        assert!(literais("// o ciclo do Botting\nlet a = \"ok\";") == vec!["ok".to_string()]);
    }
}
