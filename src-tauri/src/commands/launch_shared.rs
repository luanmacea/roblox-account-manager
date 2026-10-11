/// Emit one structured line to the launch console (frontend listens on the
/// "launch-log" event). `level` is one of "info" | "success" | "warn" | "error";
/// `step` is a short machine code (start, isolation, auth, target, pid, spawn,
/// wait, done) the UI can use for coloring/icons.
pub(crate) fn emit_launch_log(
    app: &tauri::AppHandle,
    user_id: i64,
    level: &str,
    step: &str,
    message: impl Into<String>,
) {
    let _ = app.emit(
        "launch-log",
        serde_json::json!({
            "userId": user_id,
            "level": level,
            "step": step,
            "message": message.into(),
        }),
    );
}

/// Linha de console que nao pertence a uma conta: inicio/fim de uma sessao de
/// Botting, por exemplo. O frontend ja aceita `userId: null` e desenha "—";
/// passar `0` faria o console imprimir "0" no lugar do nome.
pub(crate) fn emit_session_log(
    app: &tauri::AppHandle,
    level: &str,
    step: &str,
    message: impl Into<String>,
) {
    let _ = app.emit(
        "launch-log",
        serde_json::json!({
            "userId": serde_json::Value::Null,
            "level": level,
            "step": step,
            "message": message.into(),
        }),
    );
}

/// Group name used to bucket accounts that failed to launch because Roblox
/// reports them as moderated/banned.
pub(crate) const MODERATED_GROUP: &str = "moderadas";

/// Returns true if an auth-ticket error string indicates the account is
/// moderated/banned (Roblox returns 403 with `"User is moderated"`).
pub(crate) fn is_moderated_error(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("moderated") || e.contains("is banned") || e.contains("account has been")
}

/// Move an account into the "moderadas" group and persist it, then notify the
/// frontend so it can refresh and surface a toast. No-op if already grouped.
pub(crate) fn mark_account_moderated(store: &AccountStore, app: &tauri::AppHandle, user_id: i64) {
    if let Ok(accounts) = store.get_all() {
        if let Some(mut account) = accounts.into_iter().find(|a| a.user_id == user_id) {
            if account.group != MODERATED_GROUP {
                account.group = MODERATED_GROUP.to_string();
                let _ = store.update(account);
                let _ = app.emit(
                    "account-moderated",
                    serde_json::json!({ "userId": user_id, "group": MODERATED_GROUP }),
                );
                record_moderated_history(app, user_id);
            }
        }
    }
}

/// Quais grupos de campos de uma abertura vieram da exceção da conta (e não do
/// perfil global). É o que o registro de exceções do lado Windows guarda para
/// desfazer na próxima conta sem exceção (`OverrideLedger`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccountSourced {
    pub fps: bool,
    pub volume: bool,
    pub graphics: bool,
    /// Tela cheia e/ou tamanho de janela.
    pub window: bool,
}

#[derive(Clone, Default)]
struct WindowsClientOverrides {
    max_fps: Option<u32>,
    master_volume: Option<f32>,
    graphics: Option<GraphicsQuality>,
    /// `Some(true)` abre em tela cheia, `Some(false)` em janela, `None` deixa
    /// como o Roblox tiver gravado.
    fullscreen: Option<bool>,
    window_size: Option<(u32, u32)>,
    fast_flags: Option<serde_json::Map<String, serde_json::Value>>,
    /// O que veio da exceção da conta.
    from_account: AccountSourced,
}

/// Nível de qualidade gráfica pedido ao cliente. Existe como enum porque
/// "automático" **não é** um nível: é o Roblox decidindo sozinho, gravado num
/// campo diferente do XML. Passar `0` como se fosse um nível fazia o valor
/// cair no `clamp(1, 10)` e virar 1 — a pior qualidade, não a automática.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphicsQuality {
    Automatic,
    /// Nível fixo de 1 a 10 (o `clamp` é feito na escrita do XML).
    Level(u32),
}

/// Exceções de cliente de **uma conta só**, por cima do perfil global.
///
/// Ficam em `Account.fields` (portanto no `AccountData.json`, junto de
/// `RobloxVersion`), e são aplicadas no instante em que aquela conta vai abrir.
///
/// Ressalva que precisa ficar escrita: `ClientAppSettings.json` é por pasta de
/// versão do Roblox e `GlobalBasicSettings_13.xml` é por usuário do Windows —
/// os dois são **globais**. "Por conta" funciona porque a fila de launch é
/// sequencial e o patch roda imediatamente antes de cada spawn; não é
/// isolamento de verdade. Um cliente aberto ainda pode reescrever o XML antes
/// de o novo o ler — por isso o tamanho da janela é conferido pelo PID depois
/// do spawn (`spawn_client_window_enforcement`).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AccountClientOverrides {
    pub max_fps: Option<u32>,
    pub master_volume: Option<f32>,
    pub graphics: Option<GraphicsQuality>,
    pub fullscreen: Option<bool>,
    pub window_size: Option<(u32, u32)>,
    pub start_minimized: Option<bool>,
}

/// Chave de `Account.fields` que liga as exceções. Sem ela em `"true"`, os
/// outros campos são ignorados — dá para guardar uma configuração desligada.
pub(crate) const ACCOUNT_OVERRIDES_ENABLED_FIELD: &str = "ClientOverridesEnabled";

fn field_str<'a>(fields: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    fields
        .get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
}

fn field_bool(fields: &HashMap<String, String>, key: &str) -> Option<bool> {
    match field_str(fields, key)?.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn field_u32(fields: &HashMap<String, String>, key: &str) -> Option<u32> {
    field_str(fields, key)?.parse::<u32>().ok().filter(|v| *v > 0)
}

/// Lê as exceções de uma conta. `None` quando o interruptor está desligado ou
/// quando nenhum campo tem valor útil — assim o resto do launch continua
/// falando "esta conta não tem exceção" em vez de aplicar um struct vazio.
pub(crate) fn account_client_overrides(
    fields: &HashMap<String, String>,
) -> Option<AccountClientOverrides> {
    if field_bool(fields, ACCOUNT_OVERRIDES_ENABLED_FIELD) != Some(true) {
        return None;
    }

    let graphics = field_str(fields, "ClientOverrideGraphics").and_then(|raw| {
        if raw.eq_ignore_ascii_case("auto") || raw.eq_ignore_ascii_case("automatic") {
            Some(GraphicsQuality::Automatic)
        } else {
            raw.parse::<u32>()
                .ok()
                .filter(|lvl| (1..=10).contains(lvl))
                .map(GraphicsQuality::Level)
        }
    });

    let master_volume = field_str(fields, "ClientOverrideVolume")
        .and_then(|raw| raw.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 1.0));

    let width = field_u32(fields, "ClientOverrideWindowWidth");
    let height = field_u32(fields, "ClientOverrideWindowHeight");
    // Meia janela não é janela: largura sem altura é configuração incompleta,
    // e aplicar só uma das duas deixaria o cliente num tamanho que ninguém
    // pediu.
    let window_size = match (width, height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };

    let overrides = AccountClientOverrides {
        max_fps: field_u32(fields, "ClientOverrideMaxFPS"),
        master_volume,
        graphics,
        fullscreen: field_bool(fields, "ClientOverrideFullscreen"),
        window_size,
        start_minimized: field_bool(fields, "ClientOverrideStartMinimized"),
    };

    if overrides == AccountClientOverrides::default() {
        return None;
    }
    Some(overrides)
}

// ── a janela do cliente, conferida pelo PID ──────────────────────────────
//
// O `StartScreenSize` do `GlobalBasicSettings_13.xml` é **um arquivo para todos
// os clientes**. O patch roda logo antes do spawn, mas um cliente já aberto
// (a conta principal com a exceção dela, por exemplo) pode reescrever o XML
// entre o patch e o instante em que o cliente novo o lê — e a alt abria com o
// tamanho da principal (03/10/2026). O XML continua sendo gravado (é o que faz
// o cliente já nascer no tamanho certo na maioria das vezes), mas quem garante
// é a conferência depois do spawn: achada a janela do PID novo, ela recebe o
// tamanho resolvido para ESTA conta.

/// O que o launch resolveu para a janela desta conta — a mesma coisa que foi
/// gravada no XML.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ClientWindowInputs {
    /// `Some(true)` = tela cheia (exceção da conta); `Some(false)` = em janela
    /// (exceção, ou o valor do jogador que o registro das exceções pôs de volta
    /// depois da tela cheia de outra conta).
    pub fullscreen: Option<bool>,
    /// O tamanho gravado no `StartScreenSize` (exceção da conta, senão o
    /// perfil global com `OverrideClientWindowSize`).
    pub window_size: Option<(u32, u32)>,
    /// A conta tem janela própria (exceção com tamanho ou tela cheia) — fica
    /// fora da grade (`account_keeps_own_window`).
    pub keeps_own_window: bool,
    pub start_minimized: bool,
    /// `General.AutoArrangeGrid`.
    pub auto_arrange_grid: bool,
    /// O retângulo que o Watcher salvou (`SaveWindowPositions`); só o launch
    /// avulso o restaura.
    pub saved_rect: Option<(i32, i32, i32, i32)>,
}

/// O que fazer com a janela quando ela aparecer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ClientWindowPlan {
    /// Tamanho a impor, no mesmo sentido do `StartScreenSize`.
    pub size: Option<(u32, u32)>,
    /// Pôr a janela na primeira célula livre da grade.
    pub grid: bool,
    /// Posição (e tamanho, se `size` não vier) a restaurar.
    pub saved_rect: Option<(i32, i32, i32, i32)>,
    /// A conta abre em janela (tamanho no plano, ou `Fullscreen=false`
    /// resolvido): se a janela aparecer em tela cheia, essa tela cheia veio do
    /// XML regravado por outro cliente, e é tirada — uma vez.
    pub leave_fullscreen: bool,
}

impl ClientWindowPlan {
    pub fn is_noop(&self) -> bool {
        self.size.is_none() && !self.grid && self.saved_rect.is_none() && !self.leave_fullscreen
    }
}

/// O piso que `rewrite_global_basic_settings` aplica ao `StartScreenSize`: o
/// tamanho imposto na janela é o mesmo que o XML recebeu.
const MIN_CLIENT_WINDOW: (u32, u32) = (320, 240);

pub(crate) fn client_window_plan(inputs: ClientWindowInputs) -> ClientWindowPlan {
    // Tela cheia e começar minimizado são pedidos do usuário que mexer na
    // janela desfaria.
    if inputs.start_minimized || inputs.fullscreen == Some(true) {
        return ClientWindowPlan::default();
    }
    // Conta com janela própria fica fora da grade, com o tamanho e a posição
    // dela; para as outras, a grade vence a posição salva pelo Watcher.
    let grid = inputs.auto_arrange_grid && !inputs.keeps_own_window;
    ClientWindowPlan {
        size: inputs
            .window_size
            .map(|(w, h)| (w.max(MIN_CLIENT_WINDOW.0), h.max(MIN_CLIENT_WINDOW.1))),
        grid,
        saved_rect: if grid { None } else { inputs.saved_rect },
        // Com tamanho no plano a conta é "em janela" por definição (o XML
        // grava `Fullscreen=false` junto do `StartScreenSize`). Sem tamanho e
        // sem `Some(false)`, a tela cheia é preferência do próprio jogador.
        leave_fullscreen: inputs.window_size.is_some() || inputs.fullscreen == Some(false),
    }
}

/// A conta tem uma janela que é dela: exceção de launch com tamanho próprio
/// ou com tela cheia. Essas ficam fora da grade (automática e manual) — a
/// grade encolheria a conta principal ao tamanho das alts.
pub(crate) fn account_keeps_own_window(fields: &HashMap<String, String>) -> bool {
    account_client_overrides(fields)
        .map(|o| o.keeps_own_window())
        .unwrap_or(false)
}

impl AccountClientOverrides {
    /// Ver `account_keeps_own_window`.
    pub(crate) fn keeps_own_window(&self) -> bool {
        self.window_size.is_some() || self.fullscreen == Some(true)
    }
}

/// Os PIDs rastreados cujas contas têm janela própria. `tracked` são pares
/// `(user_id, pid)`; conta que não está mais na lista entra na grade.
pub(crate) fn pids_keeping_own_window(
    tracked: &[(i64, u32)],
    fields_by_user: &HashMap<i64, HashMap<String, String>>,
) -> HashSet<u32> {
    tracked
        .iter()
        .filter(|(user_id, _)| {
            fields_by_user
                .get(user_id)
                .map(account_keeps_own_window)
                .unwrap_or(false)
        })
        .map(|(_, pid)| *pid)
        .collect()
}

/// `General.AutoArrangeGrid`, ligado por padrão (também quando a chave sumiu
/// do INI).
pub(crate) fn auto_arrange_grid_enabled(settings: &SettingsStore) -> bool {
    settings
        .get("General", "AutoArrangeGrid")
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true)
}

/// Monitores (índices de 1 em diante; vazio = todos) e gap da grade — os
/// mesmos da aba Windows da Choose Game (`GridMonitors`, `GridGap`).
pub(crate) fn grid_layout_settings(settings: &SettingsStore) -> (Vec<usize>, i32) {
    let monitors = settings
        .get_string("General", "GridMonitors")
        .split(',')
        .filter_map(|s| s.trim().parse::<usize>().ok())
        .filter(|i| *i > 0)
        .collect();
    let gap = settings
        .get_int("General", "GridGap")
        .unwrap_or(20)
        .clamp(0, 200) as i32;
    (monitors, gap)
}

/// Célula menor que o mínimo do Roblox (`General.GridAllowSmallWindows`) e
/// janelas sem moldura (`General.GridBorderless`) — as duas desligadas por
/// padrão, só para clientes que o app abriu. Ver docs/features/performance.md.
#[cfg(target_os = "windows")]
pub(crate) fn grid_window_style(settings: &SettingsStore) -> platform::windows::GridWindowStyle {
    platform::windows::GridWindowStyle {
        allow_small: settings.get_bool("General", "GridAllowSmallWindows"),
        borderless: settings.get_bool("General", "GridBorderless"),
    }
}

/// O tamanho de janela do perfil global (`OverrideClientWindowSize`), ou
/// `None` com ele desligado.
pub(crate) fn global_window_size(
    settings: &SettingsStore,
    profile: LaunchClientProfile,
) -> Option<(u32, u32)> {
    let override_window_key = profile_key(
        profile,
        "OverrideClientWindowSize",
        "BottingPlayerOverrideClientWindowSize",
        "BottingBotOverrideClientWindowSize",
    );
    let window_width_key = profile_key(
        profile,
        "ClientWindowWidth",
        "BottingPlayerClientWindowWidth",
        "BottingBotClientWindowWidth",
    );
    let window_height_key = profile_key(
        profile,
        "ClientWindowHeight",
        "BottingPlayerClientWindowHeight",
        "BottingBotClientWindowHeight",
    );

    if !settings.get_bool("General", override_window_key) {
        return None;
    }
    let w = settings.get_int("General", window_width_key).unwrap_or(1280);
    let h = settings.get_int("General", window_height_key).unwrap_or(720);
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

/// Os PIDs dos clientes rastreados cujas contas têm janela própria — os que a
/// grade não pode mexer nem contar como ocupando célula.
#[cfg(target_os = "windows")]
pub(crate) fn grid_excluded_pids(accounts: &AccountStore) -> HashSet<u32> {
    let tracked: Vec<(i64, u32)> = platform::windows::tracker()
        .get_all()
        .into_iter()
        .map(|p| (p.user_id, p.pid))
        .collect();
    let fields_by_user: HashMap<i64, HashMap<String, String>> = accounts
        .get_all()
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.user_id, a.fields))
        .collect();
    pids_keeping_own_window(&tracked, &fields_by_user)
}

/// O que o patch resolveu para a janela, devolvido para a conferência depois
/// do spawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ResolvedClientWindow {
    pub fullscreen: Option<bool>,
    pub window_size: Option<(u32, u32)>,
}

/// Espera a janela principal do PID aparecer e aplica o plano, sem prender o
/// launch (roda numa task à parte). A janela é procurada por até 45 s — o
/// mesmo teto da restauração de posição do Watcher.
///
/// Com `plan.grid`, a janela vai para a primeira célula livre da grade
/// (monitores, gap e as janelas fora da grade são lidos na hora de pôr, não no
/// launch: a fila pode levar minutos).
#[cfg(target_os = "windows")]
pub(crate) fn spawn_client_window_enforcement(
    app: &tauri::AppHandle,
    pid: u32,
    plan: ClientWindowPlan,
) {
    if plan.is_noop() {
        return;
    }
    let app = app.clone();
    tokio::spawn(async move {
        use platform::windows;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        let found = loop {
            if windows::find_main_window(pid).is_some() {
                break true;
            }
            if std::time::Instant::now() >= deadline || !windows::is_roblox_pid_running(pid) {
                break false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        };
        if !found {
            return;
        }
        // O cliente ainda se dimensiona logo depois de mostrar a janela; medir
        // antes disso confunde a borda com o tamanho.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        // Uma aplicação e duas conferências: se o cliente redimensionar a
        // janela por conta própria logo depois, ela volta ao lugar escolhido
        // (a célula não é escolhida de novo).
        let mut chosen: Option<(i32, i32, i32, i32)> = None;
        // A tela cheia herdada é tirada uma vez só (`leave_fullscreen`): se o
        // Roblox voltar para ela, a conferência seguinte a vê e para.
        let mut fullscreen_exit_tried = false;
        // Lido uma vez: a conferência segura a célula escolhida com os mesmos
        // flags com que ela foi posta.
        let grid_style = plan
            .grid
            .then(|| grid_window_style(&app.state::<SettingsStore>()))
            .unwrap_or_default();
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            }
            let Some(hwnd) = windows::find_main_window(pid) else {
                return;
            };
            let mut first_look = chosen.is_none();
            let mode = windows::window_mode_of(hwnd).unwrap_or(windows::WindowMode::Normal);
            if windows::should_leave_fullscreen(plan.leave_fullscreen, mode, fullscreen_exit_tried) {
                match windows::leave_fullscreen(hwnd, plan.size) {
                    windows::FullscreenExit::NotFullscreen => {}
                    windows::FullscreenExit::Left(_) => {
                        fullscreen_exit_tried = true;
                        first_look = false;
                        eprintln!("[window] PID {pid}: tela cheia herdada desfeita");
                    }
                    windows::FullscreenExit::Refused => {
                        fullscreen_exit_tried = true;
                        eprintln!("[window] PID {pid}: não foi possível sair da tela cheia");
                    }
                }
            }
            let outcome = match chosen {
                Some(rect) => windows::hold_window_rect(hwnd, rect, grid_style.allow_small),
                None if plan.grid => {
                    let request = {
                        let settings = app.state::<SettingsStore>();
                        let (monitor_indices, gap) = grid_layout_settings(&settings);
                        windows::GridRequest {
                            monitor_indices,
                            gap,
                            excluded_pids: grid_excluded_pids(app.state::<AccountStore>().inner()),
                            style: grid_style,
                        }
                    };
                    windows::place_in_grid(hwnd, pid, plan.size, &request, first_look)
                }
                None => windows::enforce_client_window(hwnd, plan.size, plan.saved_rect, first_look),
            };
            match outcome {
                windows::WindowEnforcement::Skipped => return,
                windows::WindowEnforcement::Placed(rect) => chosen = Some(rect),
            }
        }
    });
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LaunchClientProfile {
    Normal,
    BottingPlayer,
    BottingBot,
}

pub(crate) fn profile_key(
    profile: LaunchClientProfile,
    normal: &'static str,
    player: &'static str,
    bot: &'static str,
) -> &'static str {
    match profile {
        LaunchClientProfile::Normal => normal,
        LaunchClientProfile::BottingPlayer => player,
        LaunchClientProfile::BottingBot => bot,
    }
}

pub(crate) fn effective_launch_profile(
    settings: &SettingsStore,
    profile: LaunchClientProfile,
) -> LaunchClientProfile {
    if matches!(profile, LaunchClientProfile::Normal) || !botting_uses_shared_client_profile(settings)
    {
        profile
    } else {
        LaunchClientProfile::Normal
    }
}

fn custom_client_settings_path(settings: &SettingsStore, profile: LaunchClientProfile) -> String {
    let key = profile_key(
        profile,
        "CustomClientSettings",
        "BottingPlayerCustomClientSettings",
        "BottingBotCustomClientSettings",
    );
    settings.get_string("General", key)
}

pub(crate) fn start_minimized_for_profile(
    settings: &SettingsStore,
    profile: LaunchClientProfile,
) -> bool {
    let key = profile_key(
        profile,
        "StartRobloxMinimized",
        "BottingPlayerStartRobloxMinimized",
        "BottingBotStartRobloxMinimized",
    );
    settings.get_bool("General", key)
}

pub(crate) fn botting_uses_shared_client_profile(settings: &SettingsStore) -> bool {
    settings
        .get("General", "BottingUseSharedClientProfile")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(true)
}

#[cfg(target_os = "windows")]
fn windows_client_overrides(
    settings: &SettingsStore,
    allow_fps_override: bool,
    profile: LaunchClientProfile,
    account: Option<&AccountClientOverrides>,
) -> WindowsClientOverrides {
    let unlock_fps_key = profile_key(
        profile,
        "UnlockFPS",
        "BottingPlayerUnlockFPS",
        "BottingBotUnlockFPS",
    );
    let max_fps_key = profile_key(
        profile,
        "MaxFPSValue",
        "BottingPlayerMaxFPSValue",
        "BottingBotMaxFPSValue",
    );

    let max_fps = if allow_fps_override && settings.get_bool("General", unlock_fps_key) {
        let fps = settings.get_int("General", max_fps_key).unwrap_or(120);
        if fps > 0 {
            Some(fps as u32)
        } else {
            None
        }
    } else {
        None
    };

    let override_volume_key = profile_key(
        profile,
        "OverrideClientVolume",
        "BottingPlayerOverrideClientVolume",
        "BottingBotOverrideClientVolume",
    );
    let client_volume_key = profile_key(
        profile,
        "ClientVolume",
        "BottingPlayerClientVolume",
        "BottingBotClientVolume",
    );

    let master_volume = if settings.get_bool("General", override_volume_key) {
        Some(
            settings
                .get_float("General", client_volume_key)
                .unwrap_or(0.5)
                .clamp(0.0, 1.0) as f32,
        )
    } else {
        None
    };

    let override_graphics_key = profile_key(
        profile,
        "OverrideClientGraphics",
        "BottingPlayerOverrideClientGraphics",
        "BottingBotOverrideClientGraphics",
    );
    let graphics_level_key = profile_key(
        profile,
        "ClientGraphicsLevel",
        "BottingPlayerClientGraphicsLevel",
        "BottingBotClientGraphicsLevel",
    );

    let graphics = if settings.get_bool("General", override_graphics_key) {
        let lvl = settings
            .get_int("General", graphics_level_key)
            .unwrap_or(10);
        if lvl > 0 {
            Some(GraphicsQuality::Level(lvl.clamp(1, 10) as u32))
        } else {
            None
        }
    } else {
        None
    };

    let window_size = global_window_size(settings, profile);

    let optimization_profile = platform::windows::load_optimization_profile(settings, profile);
    let fast_flags = if optimization_profile.experimental.enable_fast_flags {
        match platform::windows::parse_allowlisted_fast_flags_json(
            &optimization_profile.experimental.fast_flags_json,
        ) {
            Ok(flags) => Some(flags),
            Err(err) => {
                eprintln!("Skipped allowlisted fast flags for {:?}: {}", profile, err);
                None
            }
        }
    } else {
        None
    };

    // A conta entra **por cima**: cada campo que ela define substitui o global,
    // e o que ela deixa vazio continua vindo do perfil. O FPS respeita o
    // `allow_fps_override` igual ao global — quando o usuário aponta um
    // ClientAppSettings.json próprio, ninguém mexe no FPS dele, nem a exceção.
    let mut resolved = WindowsClientOverrides {
        max_fps,
        master_volume,
        graphics,
        fullscreen: None,
        window_size,
        fast_flags,
        from_account: AccountSourced::default(),
    };

    if let Some(acc) = account {
        if allow_fps_override {
            if let Some(fps) = acc.max_fps {
                resolved.max_fps = Some(fps);
                resolved.from_account.fps = true;
            }
        }
        if let Some(volume) = acc.master_volume {
            resolved.master_volume = Some(volume);
            resolved.from_account.volume = true;
        }
        if let Some(graphics) = acc.graphics {
            resolved.graphics = Some(graphics);
            resolved.from_account.graphics = true;
        }
        resolved.from_account.window = acc.fullscreen.is_some() || acc.window_size.is_some();
        if let Some(fullscreen) = acc.fullscreen {
            resolved.fullscreen = Some(fullscreen);
            // Tela cheia com um tamanho de janela ao lado é contraditório: o XML
            // grava `Fullscreen=false` junto de `StartScreenSize`. Quem pediu
            // tela cheia e não pediu tamanho fica só com a tela cheia.
            if fullscreen && acc.window_size.is_none() {
                resolved.window_size = None;
            }
        }
        if let Some(size) = acc.window_size {
            resolved.window_size = Some(size);
        }
    }

    resolved
}

/// `base_path`: pasta da versão do Roblox que vai realmente abrir (`None` =
/// build padrão/produção, usada pelo servidor HTTP local, que não tem conta no
/// contexto). Sem isso, `ClientAppSettings.json` era sempre escrito na pasta de
/// produção mesmo quando a conta abre numa versão do catálogo — o cliente que
/// de fato abre nunca lia o FPS/volume/qualidade/fullscreen/fast flags
/// aplicados.
#[cfg(target_os = "windows")]
pub(crate) fn patch_client_settings_for_launch(
    settings: &SettingsStore,
    profile: LaunchClientProfile,
    account: Option<&AccountClientOverrides>,
    base_path: Option<&str>,
) -> ResolvedClientWindow {
    use platform::windows;

    let effective_profile = effective_launch_profile(settings, profile);
    let custom_settings = custom_client_settings_path(settings, effective_profile);
    let custom_settings = custom_settings.trim();
    let mut custom_applied = false;

    // Legacy behavior: custom settings file overrides FPS unlock when valid.
    if !custom_settings.is_empty()
        && std::path::Path::new(custom_settings).exists()
        && windows::copy_custom_client_settings(base_path, custom_settings).is_ok()
    {
        custom_applied = true;
    }

    let mut overrides =
        windows_client_overrides(settings, !custom_applied, effective_profile, account);
    if custom_applied {
        overrides.fast_flags = None;
    }
    let fullscreen_from_ledger = windows::apply_runtime_client_settings(
        base_path,
        overrides.max_fps,
        overrides.master_volume,
        overrides.graphics,
        overrides.fullscreen,
        overrides.window_size,
        overrides.fast_flags.as_ref(),
        overrides.from_account,
    )
    .ok()
    .flatten();
    ResolvedClientWindow {
        // Sem pedido desta conta nem do perfil global, vale o que o registro
        // pôs de volta depois da exceção de outra conta (a tela cheia da
        // principal, 03/10/2026) — é ele que diz "esta alt é em janela".
        fullscreen: overrides.fullscreen.or(fullscreen_from_ledger),
        window_size: overrides.window_size,
    }
}

#[cfg(target_os = "macos")]
fn fps_unlock_target(settings: &SettingsStore, profile: LaunchClientProfile) -> Option<u32> {
    let unlock_fps_key = profile_key(
        profile,
        "UnlockFPS",
        "BottingPlayerUnlockFPS",
        "BottingBotUnlockFPS",
    );
    let max_fps_key = profile_key(
        profile,
        "MaxFPSValue",
        "BottingPlayerMaxFPSValue",
        "BottingBotMaxFPSValue",
    );

    if !settings.get_bool("General", unlock_fps_key) {
        return None;
    }
    settings
        .get_int("General", max_fps_key)
        .filter(|fps| *fps > 0)
        .map(|fps| fps as u32)
}

#[cfg(target_os = "macos")]
fn patch_client_settings_for_launch(
    settings: &SettingsStore,
    profile: LaunchClientProfile,
    account: Option<&AccountClientOverrides>,
    base_path: Option<&str>,
) {
    use platform::macos;

    // macOS não tem catálogo de versões instaladas (ver docs/features/launch.md);
    // o parâmetro existe só para manter a mesma assinatura do lado Windows.
    let _ = base_path;

    let custom_settings = custom_client_settings_path(settings, profile);
    let custom_settings = custom_settings.trim();

    // Keep the same override precedence as Windows.
    if !custom_settings.is_empty()
        && std::path::Path::new(custom_settings).exists()
        && macos::copy_custom_client_settings(custom_settings).is_ok()
    {
        return;
    }

    // No macOS só o FPS é aplicável hoje; as outras exceções por conta dependem
    // do `GlobalBasicSettings_13.xml`, que é do lado Windows.
    let fps = account
        .and_then(|a| a.max_fps)
        .or_else(|| fps_unlock_target(settings, profile));
    if let Some(fps) = fps {
        let _ = macos::apply_fps_unlock(fps);
    }
}

fn save_browser_tracker_id(
    state: &AccountStore,
    user_id: i64,
    browser_tracker_id: &str,
) -> Result<(), String> {
    let accounts = state.get_all()?;
    if let Some(mut account) = accounts.into_iter().find(|a| a.user_id == user_id) {
        account.browser_tracker_id = browser_tracker_id.to_string();
        state.update(account)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn get_or_create_browser_tracker_id(
    state: &AccountStore,
    user_id: i64,
) -> Result<String, String> {
    let accounts = state.get_all()?;
    if let Some(existing) = accounts
        .iter()
        .find(|a| a.user_id == user_id)
        .map(|a| a.browser_tracker_id.trim().to_string())
        .filter(|id| !id.is_empty())
    {
        return Ok(existing);
    }

    let generated = platform::windows::generate_browser_tracker_id();
    save_browser_tracker_id(state, user_id, &generated)?;
    Ok(generated)
}

#[cfg(target_os = "macos")]
fn get_or_create_browser_tracker_id(state: &AccountStore, user_id: i64) -> Result<String, String> {
    let accounts = state.get_all()?;
    if let Some(existing) = accounts
        .iter()
        .find(|a| a.user_id == user_id)
        .map(|a| a.browser_tracker_id.trim().to_string())
        .filter(|id| !id.is_empty())
    {
        return Ok(existing);
    }

    let generated = platform::macos::generate_browser_tracker_id();
    save_browser_tracker_id(state, user_id, &generated)?;
    Ok(generated)
}

#[cfg(target_os = "windows")]
pub(crate) async fn wait_for_new_roblox_pid(
    pids_before: &[u32],
    timeout: std::time::Duration,
) -> Option<u32> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let pids_after = platform::windows::get_roblox_pids();
        if let Some(pid) = pids_after
            .iter()
            .find(|p| !pids_before.contains(p))
            .copied()
        {
            return Some(pid);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
}

#[cfg(target_os = "macos")]
async fn wait_for_new_roblox_pid(pids_before: &[u32], timeout: std::time::Duration) -> Option<u32> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let pids_after = platform::macos::get_roblox_pids();
        if let Some(pid) = pids_after
            .iter()
            .find(|p| !pids_before.contains(p))
            .copied()
        {
            return Some(pid);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
}

#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct BottingAccountStatusPayload {
    user_id: i64,
    is_player: bool,
    disconnected: bool,
    phase: String,
    retry_count: u32,
    next_restart_at_ms: Option<i64>,
    player_grace_until_ms: Option<i64>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct BottingStatusPayload {
    active: bool,
    started_at_ms: Option<i64>,
    place_id: i64,
    job_id: String,
    launch_data: String,
    interval_minutes: i64,
    launch_delay_seconds: i64,
    player_grace_minutes: i64,
    player_user_ids: Vec<i64>,
    user_ids: Vec<i64>,
    accounts: Vec<BottingAccountStatusPayload>,
}

#[cfg(target_os = "windows")]
async fn minimize_new_roblox_windows(pids_before: Vec<u32>, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    let mut minimized: HashSet<u32> = HashSet::new();
    loop {
        for pid in platform::windows::get_roblox_pids() {
            if pids_before.contains(&pid) || minimized.contains(&pid) {
                continue;
            }
            if let Some(hwnd) = platform::windows::find_main_window(pid) {
                let _ = platform::windows::minimize_window(hwnd);
                minimized.insert(pid);
            }
        }

        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

#[cfg(target_os = "windows")]
pub(crate) async fn apply_windows_post_launch_profile(
    app: Option<&tauri::AppHandle>,
    settings: &SettingsStore,
    profile: LaunchClientProfile,
    pid: u32,
) {
    let effective_profile = effective_launch_profile(settings, profile);
    // A otimização que segue o foco tira daqui a política de fundo deste
    // cliente e o estado a devolver quando ela desliga.
    platform::windows::remember_launch_profile(pid, effective_profile);
    let follow_focus = platform::windows::focus_follow_enabled(settings);
    let optimization_profile = platform::windows::launch_profile_under_focus_follow(
        platform::windows::load_optimization_profile(settings, effective_profile),
        follow_focus,
    );
    let has_process_policy = optimization_profile.process.enabled;
    let has_job_limits = optimization_profile.experimental.enable_job_cpu_limit
        || optimization_profile.experimental.enable_job_memory_limit;
    if !has_process_policy && !has_job_limits {
        return;
    }
    if has_process_policy && optimization_profile.process.delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(
            optimization_profile.process.delay_ms,
        ))
        .await;
    }

    let applied = platform::windows::apply_optimization_to_pid(pid, &optimization_profile);
    if follow_focus {
        // O Job acabou de nascer com o teto ligado: o laço reaplica a
        // velocidade (o cliente novo está na carência, sem teto).
        platform::windows::focus_follow_forget_applied(pid);
    }
    if let Err(err) = applied {
        eprintln!("Failed to apply Windows optimization to pid {}: {}", pid, err);
        if let Some(app) = app {
            let _ = app.emit(
                "roblox-optimization-warning",
                serde_json::json!({
                    "pid": pid,
                    "message": err,
                }),
            );
        }
    }
}

#[cfg(target_os = "windows")]
async fn ensure_multi_roblox_enabled(
    auto_close_conflicts: bool,
    reserve_singleton_event: bool,
) -> Result<(), String> {
    // Experimental e desligado por padrão (ideia 3): com a opção ligada, o
    // `enable_multi_roblox` também reserva o nome `ROBLOX_singletonEvent`.
    platform::windows::set_singleton_reservation_enabled(reserve_singleton_event);
    let enabled = platform::windows::enable_multi_roblox()?;
    if enabled {
        return Ok(());
    }

    let roblox_pids = platform::windows::get_roblox_pids();
    let legacy_pids = platform::windows::find_legacy_ram_pids();

    if !roblox_pids.is_empty() {
        if auto_close_conflicts {
            let killed = platform::windows::kill_all_roblox();
            if killed > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            }
            let _ = platform::windows::tracker().cleanup_dead_processes();
            let enabled_after = platform::windows::enable_multi_roblox()?;
            if enabled_after {
                return Ok(());
            }
        }
        return Err(
            "A Roblox client is already running. Close it or enable Auto-close Roblox for Multi-Roblox in settings.".into(),
        );
    }

    if !legacy_pids.is_empty() {
        return Err(
            "The legacy Roblox Account Manager is running and holds the Roblox singleton mutex. Close it before launching from this app.".into(),
        );
    }

    platform::windows::release_multi_roblox_handle();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let enabled_retry = platform::windows::enable_multi_roblox()?;
    if enabled_retry {
        return Ok(());
    }

    Err(
        "Could not acquire the Roblox singleton mutex. Another program may be holding it. Close any Roblox-related tools and try again.".into(),
    )
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct BottingConfig {
    user_ids: Vec<i64>,
    place_id: i64,
    job_id: String,
    launch_data: String,
    player_user_ids: HashSet<i64>,
    interval_minutes: u64,
    launch_delay_seconds: u64,
    retry_max: u32,
    retry_base_seconds: u64,
    player_grace_minutes: u64,
    /// A sessao foi aberta sobre contas que **ja estavam em jogo**: na primeira
    /// passagem elas nao sao fechadas nem relancadas, so entram no ciclo.
    adopt_running: bool,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct BottingAccountRuntime {
    user_id: i64,
    is_player: bool,
    disconnected: bool,
    manual_restart_pending: bool,
    manual_restart_keep_schedule: bool,
    manual_restart_saved_next_restart_at_ms: Option<i64>,
    phase: &'static str,
    retry_count: u32,
    next_restart_at_ms: Option<i64>,
    player_grace_until_ms: Option<i64>,
    last_error: Option<String>,
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct BottingSession {
    id: u64,
    stop_flag: Arc<AtomicBool>,
    stopped_notify: Arc<tokio::sync::Notify>,
    started_at_ms: i64,
    config: Arc<Mutex<BottingConfig>>,
    accounts: Arc<Mutex<HashMap<i64, BottingAccountRuntime>>>,
}

#[cfg(target_os = "windows")]
struct BottingManager {
    session: Mutex<Option<BottingSession>>,
    next_id: AtomicU64,
}

#[cfg(target_os = "windows")]
impl BottingManager {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    fn next_session_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn get_session(&self) -> Option<BottingSession> {
        self.session.lock().ok().and_then(|s| s.as_ref().cloned())
    }

    fn replace_session(&self, session: Option<BottingSession>) {
        if let Ok(mut guard) = self.session.lock() {
            *guard = session;
        }
    }
}

#[cfg(target_os = "windows")]
static BOTTING_MANAGER: LazyLock<BottingManager> = LazyLock::new(BottingManager::new);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[derive(Debug, Clone, Default)]
struct ResolvedLaunchJob {
    job_id: String,
    join_vip: bool,
    link_code: String,
}

fn decode_url_component(value: &str) -> String {
    urlencoding::decode(value)
        .map(|v| v.into_owned())
        .unwrap_or_else(|_| value.to_string())
}

fn extract_query_param_value(input: &str, key: &str) -> Option<String> {
    for part in input.split(['?', '&']) {
        let pair = part.split('#').next().unwrap_or(part);
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if !k.eq_ignore_ascii_case(key) {
            continue;
        }

        let decoded = decode_url_component(v.trim());
        let value = decoded.trim();
        if value.is_empty()
            || value.eq_ignore_ascii_case("null")
            || value.eq_ignore_ascii_case("undefined")
        {
            continue;
        }

        return Some(value.to_string());
    }
    None
}

fn extract_query_param_value_recursive(input: &str, key: &str) -> Option<String> {
    if let Some(value) = extract_query_param_value(input, key) {
        return Some(value);
    }

    let decoded = decode_url_component(input);
    if decoded != input {
        return extract_query_param_value(&decoded, key);
    }

    None
}

fn strip_ascii_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    value.get(prefix.len()..)
}

fn looks_like_share_link(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("/share?")
        || lower.contains("/share-links")
        || lower.contains("navigation/share_links")
        || lower.contains("type=server")
        || lower.contains("pid=server")
}

fn extract_private_server_link_code(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = strip_ascii_prefix(trimmed, "vip:") {
        let decoded = decode_url_component(rest.trim());
        let code = decoded.trim();
        if !code.is_empty() {
            return Some(code.to_string());
        }
    }

    if let Some(code) = extract_query_param_value_recursive(trimmed, "privateServerLinkCode") {
        return Some(code);
    }
    if let Some(code) = extract_query_param_value_recursive(trimmed, "linkCode") {
        return Some(code);
    }

    let starts_with_code = trimmed
        .get(..5)
        .map(|head| head.eq_ignore_ascii_case("code="))
        .unwrap_or(false);
    if looks_like_share_link(trimmed) || starts_with_code {
        if let Some(code) = extract_query_param_value_recursive(trimmed, "code") {
            return Some(code);
        }
    }

    None
}

fn resolve_launch_job(
    raw_job_id: &str,
    explicit_join_vip: bool,
    explicit_link_code: &str,
) -> ResolvedLaunchJob {
    let trimmed_job = raw_job_id.trim();
    let mut job_id = trimmed_job.to_string();

    let mut link_code = extract_private_server_link_code(explicit_link_code).unwrap_or_else(|| {
        decode_url_component(explicit_link_code.trim())
            .trim()
            .to_string()
    });

    let mut join_vip = explicit_join_vip;
    if let Some(rest) = strip_ascii_prefix(trimmed_job, "vip:") {
        join_vip = true;
        job_id = rest.trim().to_string();
    }

    if link_code.is_empty() {
        if let Some(code) = extract_private_server_link_code(trimmed_job) {
            link_code = code;
        }
    }

    if join_vip && link_code.is_empty() {
        if job_id.trim().is_empty() {
            join_vip = false;
        } else {
            link_code = decode_url_component(job_id.trim()).trim().to_string();
        }
    }

    ResolvedLaunchJob {
        job_id,
        join_vip,
        link_code,
    }
}

fn looks_like_access_code(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }

    let parts: Vec<&str> = trimmed.split('-').collect();
    if parts.len() != 5 {
        return false;
    }

    parts
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn looks_like_share_link_code(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 32
        && trimmed.chars().any(|c| c.is_ascii_alphabetic())
        && trimmed.chars().all(|c| c.is_ascii_hexdigit())
}

fn extract_place_id_from_url(value: &str) -> Option<i64> {
    let lower = value.to_ascii_lowercase();
    let marker = "/games/";
    let start = lower.find(marker)? + marker.len();
    let rest = value.get(start..)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().filter(|id| *id > 0)
}

/// Picks a random public server for **this** account.
///
/// Cada conta chama este helper por conta própria: num launch múltiplo com
/// `shuffleJob` ligado, as contas acabam espalhadas por servidores diferentes
/// (a lista é buscada de novo e o índice é sorteado por conta), em vez de todas
/// caírem no mesmo servidor. Devolve `None` — e o chamador mantém o Job ID
/// vazio, entrando num servidor público qualquer — quando a listagem falha ou
/// vem vazia.
async fn pick_shuffled_public_job(
    accounts: &AccountStore,
    user_id: i64,
    place_id: i64,
) -> Option<String> {
    // Sem `run_with_session_retry` de propósito: o refresh dele chama
    // `signoutfromallsessionsandreauthenticate`, que derruba as sessões abertas
    // da conta. Sortear servidor é leitura opcional — se falhar, o launch segue
    // com o Job vazio (servidor público qualquer).
    let cookie = get_cookie(accounts, user_id).ok()?;
    let response = api::roblox::get_servers(place_id, "Public", None, Some(&cookie))
        .await
        .ok()?;

    if response.data.is_empty() {
        return None;
    }

    let index = shuffle_server_index(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        response.data.len(),
    );
    Some(response.data[index].id.clone())
}

#[derive(Debug, Clone)]
struct ResolvedPrivateJoin {
    place_id: i64,
    link_code: String,
    access_code: String,
    use_private_join: bool,
}

async fn resolve_private_join(
    cookie: &str,
    place_id: i64,
    launch: &ResolvedLaunchJob,
) -> Result<ResolvedPrivateJoin, String> {
    let mut resolved_place_id = place_id;
    let mut resolved_link_code = launch.link_code.trim().to_string();

    if !resolved_link_code.is_empty() {
        if let Some(url_place_id) = extract_place_id_from_url(&launch.job_id) {
            resolved_place_id = url_place_id;
        }
    }

    if !resolved_link_code.is_empty()
        && (looks_like_share_link(&launch.job_id)
            || looks_like_share_link_code(&resolved_link_code))
    {
        let (maybe_place_id, resolved_code) =
            api::roblox::resolve_share_server_link(cookie, &resolved_link_code).await?;
        if let Some(pid) = maybe_place_id {
            resolved_place_id = pid;
        }
        if !resolved_code.trim().is_empty() {
            resolved_link_code = resolved_code.trim().to_string();
        }
    }

    let mut access_code = String::new();
    if looks_like_access_code(&resolved_link_code) {
        access_code = resolved_link_code.clone();
        resolved_link_code.clear();
    }

    let use_private_join =
        launch.join_vip || !resolved_link_code.is_empty() || !access_code.is_empty();

    Ok(ResolvedPrivateJoin {
        place_id: resolved_place_id,
        link_code: resolved_link_code,
        access_code,
        use_private_join,
    })
}

#[cfg(target_os = "windows")]
fn backoff_delay_seconds(base: u64, retry_count: u32, retry_max: u32) -> u64 {
    let exp = retry_count.saturating_sub(1).min(retry_max.max(1));
    let scaled = base.saturating_mul(1_u64 << exp.min(12));
    scaled.clamp(5, 300)
}

#[cfg(target_os = "windows")]
async fn wait_for_launch_slot(
    last_launch_at: &mut Option<std::time::Instant>,
    launch_delay_seconds: u64,
) {
    if let Some(last) = *last_launch_at {
        let required_gap = std::time::Duration::from_secs(launch_delay_seconds);
        let elapsed = last.elapsed();
        if elapsed < required_gap {
            tokio::time::sleep(required_gap - elapsed).await;
        }
    }
    *last_launch_at = Some(std::time::Instant::now());
}

#[cfg(target_os = "windows")]
fn is_429_related_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("429")
        || lower.contains("too many requests")
        || lower.contains("authentifizierung fehlgeschlagen")
        || lower.contains("authentication failed")
}

#[cfg(target_os = "windows")]
fn title_looks_auth_failure(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("authentifizierung fehlgeschlagen")
        || t.contains("authentication failed")
        || t.contains("fehlercode: 429")
        || t.contains("error code: 429")
}

#[cfg(target_os = "windows")]
async fn detect_auth_failure_window(pid: u32) -> bool {
    for _ in 0..20 {
        if let Some(hwnd) = platform::windows::find_main_window(pid) {
            let title = platform::windows::get_window_title(hwnd);
            if !title.is_empty() && title_looks_auth_failure(&title) {
                return true;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    false
}

#[cfg(target_os = "windows")]
fn botting_status_from_session(session: &BottingSession) -> BottingStatusPayload {
    let config = match session.config.lock() {
        Ok(c) => c.clone(),
        Err(_) => {
            return BottingStatusPayload {
                active: false,
                ..BottingStatusPayload::default()
            }
        }
    };
    let mut accounts: Vec<BottingAccountStatusPayload> = match session.accounts.lock() {
        Ok(map) => map
            .values()
            .map(|a| BottingAccountStatusPayload {
                user_id: a.user_id,
                is_player: a.is_player,
                disconnected: a.disconnected,
                phase: a.phase.to_string(),
                retry_count: a.retry_count,
                next_restart_at_ms: a.next_restart_at_ms,
                player_grace_until_ms: a.player_grace_until_ms,
                last_error: a.last_error.clone(),
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    accounts.sort_by_key(|a| a.user_id);
    let mut player_user_ids: Vec<i64> = config.player_user_ids.iter().copied().collect();
    player_user_ids.sort();
    BottingStatusPayload {
        active: !session.stop_flag.load(Ordering::Relaxed),
        started_at_ms: Some(session.started_at_ms),
        place_id: config.place_id,
        job_id: config.job_id,
        launch_data: config.launch_data,
        interval_minutes: config.interval_minutes as i64,
        launch_delay_seconds: config.launch_delay_seconds as i64,
        player_grace_minutes: config.player_grace_minutes as i64,
        player_user_ids,
        user_ids: config.user_ids,
        accounts,
    }
}

#[cfg(target_os = "windows")]
fn current_botting_status() -> BottingStatusPayload {
    if let Some(session) = BOTTING_MANAGER.get_session() {
        botting_status_from_session(&session)
    } else {
        BottingStatusPayload::default()
    }
}

#[cfg(target_os = "windows")]
fn emit_botting_status(app: &tauri::AppHandle) {
    let _ = app.emit("botting-status", current_botting_status());
}

#[cfg(test)]
mod launch_resolve_tests {
    use super::*;

    // ---- resolve_launch_job -------------------------------------------------

    #[test]
    fn vip_prefix_sets_join_vip_and_strips_prefix() {
        let resolved = resolve_launch_job("vip:ABC123", false, "");
        assert!(resolved.join_vip);
        assert_eq!(resolved.job_id, "ABC123");
        assert_eq!(resolved.link_code, "ABC123");
    }

    #[test]
    fn vip_prefix_is_case_insensitive_and_trims() {
        let resolved = resolve_launch_job("  VIP: ABC123  ", false, "");
        assert!(resolved.join_vip);
        assert_eq!(resolved.job_id, "ABC123");
    }

    #[test]
    fn share_url_private_server_link_code_is_extracted() {
        let resolved = resolve_launch_job(
            "https://www.roblox.com/games/606849621/Jailbreak?privateServerLinkCode=1122334455",
            false,
            "",
        );
        assert_eq!(resolved.link_code, "1122334455");
        assert!(!resolved.join_vip);
    }

    #[test]
    fn link_code_query_param_is_extracted_from_job_id() {
        let resolved = resolve_launch_job(
            "https://www.roblox.com/games/start?placeId=1&linkCode=abcdef",
            false,
            "",
        );
        assert_eq!(resolved.link_code, "abcdef");
    }

    #[test]
    fn join_vip_without_code_falls_back_to_job_id_as_link_code() {
        let resolved = resolve_launch_job("SOMECODE", true, "");
        assert!(resolved.join_vip);
        assert_eq!(resolved.job_id, "SOMECODE");
        assert_eq!(resolved.link_code, "SOMECODE");
    }

    #[test]
    fn vip_prefix_with_empty_job_clears_join_vip() {
        let resolved = resolve_launch_job("vip:", false, "");
        assert!(!resolved.join_vip);
        assert_eq!(resolved.job_id, "");
        assert_eq!(resolved.link_code, "");
    }

    #[test]
    fn explicit_link_code_wins_over_job_id_extraction() {
        let resolved = resolve_launch_job(
            "https://www.roblox.com/games/1/x?privateServerLinkCode=fromjob",
            true,
            "vip:explicit",
        );
        assert!(resolved.join_vip);
        assert_eq!(resolved.link_code, "explicit");
    }

    #[test]
    fn plain_job_id_without_vip_stays_untouched() {
        let resolved = resolve_launch_job(
            "  11111111-2222-3333-4444-555555555555  ",
            false,
            "",
        );
        assert!(!resolved.join_vip);
        assert_eq!(resolved.job_id, "11111111-2222-3333-4444-555555555555");
        assert_eq!(resolved.link_code, "");
    }

    // ---- extract_query_param_value -----------------------------------------

    #[test]
    fn extract_query_param_value_reads_simple_pair() {
        assert_eq!(
            extract_query_param_value("https://x/y?code=abc&other=1", "code").as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn extract_query_param_value_is_case_insensitive_on_the_key() {
        assert_eq!(
            extract_query_param_value("?LINKCODE=abc", "linkCode").as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn extract_query_param_value_ignores_null_and_undefined_and_empty() {
        assert_eq!(extract_query_param_value("?code=null", "code"), None);
        assert_eq!(extract_query_param_value("?code=UNDEFINED", "code"), None);
        assert_eq!(extract_query_param_value("?code=", "code"), None);
        // A later, valid occurrence still wins over the null one.
        assert_eq!(
            extract_query_param_value("?code=null&code=real", "code").as_deref(),
            Some("real")
        );
    }

    #[test]
    fn extract_query_param_value_strips_fragment() {
        assert_eq!(
            extract_query_param_value("?code=abc#frag", "code").as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn extract_query_param_value_recursive_handles_double_encoded_urls() {
        let raw = "https://ro.blox.com/Ebh5?af_dp=roblox%3A%2F%2Fnavigation%2Fshare_links%3Fcode%3DDEADBEEF%26type%3DServer";
        // The non-recursive variant cannot see through the encoded inner query.
        assert_eq!(extract_query_param_value(raw, "code"), None);
        assert_eq!(
            extract_query_param_value_recursive(raw, "code").as_deref(),
            Some("DEADBEEF")
        );
    }

    // ---- looks_like_access_code --------------------------------------------

    #[test]
    fn looks_like_access_code_requires_five_non_empty_segments() {
        assert!(looks_like_access_code(
            "11111111-2222-3333-4444-555555555555"
        ));
        assert!(looks_like_access_code("a-b-c-d-e"));
        assert!(!looks_like_access_code("a-b-c-d"));
        assert!(!looks_like_access_code("a-b-c-d-e-f"));
        assert!(!looks_like_access_code("a--c-d-e"));
        assert!(!looks_like_access_code(""));
        assert!(!looks_like_access_code("   "));
        assert!(!looks_like_access_code("a-b-c-d-e!"));
    }

    // ---- looks_like_share_link_code ----------------------------------------

    #[test]
    fn looks_like_share_link_code_requires_32_hex_with_a_letter() {
        assert!(looks_like_share_link_code(
            "0123456789abcdef0123456789abcdef"
        ));
        // 32 hex digits but no letter -> not a share link code.
        assert!(!looks_like_share_link_code(
            "01234567890123456789012345678901"
        ));
        // Wrong length.
        assert!(!looks_like_share_link_code("0123456789abcdef0123456789abcde"));
        // Non hex character.
        assert!(!looks_like_share_link_code(
            "0123456789abcdeg0123456789abcdef"
        ));
        assert!(!looks_like_share_link_code(""));
    }

    // ---- extract_place_id_from_url -----------------------------------------

    #[test]
    fn extract_place_id_from_url_reads_the_games_segment() {
        assert_eq!(
            extract_place_id_from_url("https://www.roblox.com/games/606849621/Jailbreak"),
            Some(606849621)
        );
        assert_eq!(
            extract_place_id_from_url("https://www.roblox.com/GAMES/42?x=1"),
            Some(42)
        );
    }

    #[test]
    fn extract_place_id_from_url_rejects_zero_and_non_digits() {
        assert_eq!(
            extract_place_id_from_url("https://www.roblox.com/games/0/Zero"),
            None
        );
        assert_eq!(
            extract_place_id_from_url("https://www.roblox.com/games/abc"),
            None
        );
        assert_eq!(extract_place_id_from_url("https://www.roblox.com/home"), None);
    }

    // ---- is_moderated_error -------------------------------------------------

    #[test]
    fn is_moderated_error_matches_known_phrases_case_insensitively() {
        assert!(is_moderated_error("User is moderated"));
        assert!(is_moderated_error("USER IS MODERATED"));
        assert!(is_moderated_error("The account is banned"));
        assert!(is_moderated_error("This account has been terminated"));
    }

    #[test]
    fn is_moderated_error_returns_false_for_unrelated_failures() {
        // Regression guard: transient failures must not move accounts into the
        // "moderadas" group.
        assert!(!is_moderated_error("network timeout"));
        assert!(!is_moderated_error("429 Too Many Requests"));
        assert!(!is_moderated_error(""));
    }

    #[test]
    fn moderated_group_name_is_stable() {
        assert_eq!(MODERATED_GROUP, "moderadas");
    }
}

#[cfg(test)]
mod launch_shared_helper_tests {
    use super::*;

    fn temp_settings(tag: &str) -> SettingsStore {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        SettingsStore::new(std::env::temp_dir().join(format!("ram-lshared-{tag}-{nanos}.ini")))
    }

    #[allow(dead_code)]
    fn temp_accounts(tag: &str) -> AccountStore {
        crypto::init();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        AccountStore::new(std::env::temp_dir().join(format!("ram-lshared-{tag}-{nanos}.json")))
    }

    // ---- profile_key --------------------------------------------------------

    #[test]
    fn profile_key_picks_the_setting_key_of_each_profile() {
        assert_eq!(
            profile_key(LaunchClientProfile::Normal, "N", "P", "B"),
            "N"
        );
        assert_eq!(
            profile_key(LaunchClientProfile::BottingPlayer, "N", "P", "B"),
            "P"
        );
        assert_eq!(
            profile_key(LaunchClientProfile::BottingBot, "N", "P", "B"),
            "B"
        );
    }

    // ---- botting_uses_shared_client_profile / effective_launch_profile ------

    #[test]
    fn botting_uses_shared_client_profile_defaults_to_true() {
        let settings = temp_settings("shared-default");
        assert!(botting_uses_shared_client_profile(&settings));
    }

    #[test]
    fn botting_uses_shared_client_profile_is_false_only_for_the_exact_false_value() {
        let settings = temp_settings("shared-off");
        settings
            .set("General", "BottingUseSharedClientProfile", "false")
            .unwrap();
        assert!(!botting_uses_shared_client_profile(&settings));

        settings
            .set("General", "BottingUseSharedClientProfile", "TRUE")
            .unwrap();
        assert!(!botting_uses_shared_client_profile(&settings));

        settings
            .set("General", "BottingUseSharedClientProfile", "true")
            .unwrap();
        assert!(botting_uses_shared_client_profile(&settings));
    }

    #[test]
    fn effective_launch_profile_collapses_botting_profiles_when_sharing() {
        let settings = temp_settings("effective-shared");
        // Default is "share the Normal profile".
        assert!(matches!(
            effective_launch_profile(&settings, LaunchClientProfile::BottingBot),
            LaunchClientProfile::Normal
        ));
        assert!(matches!(
            effective_launch_profile(&settings, LaunchClientProfile::BottingPlayer),
            LaunchClientProfile::Normal
        ));
        assert!(matches!(
            effective_launch_profile(&settings, LaunchClientProfile::Normal),
            LaunchClientProfile::Normal
        ));
    }

    #[test]
    fn effective_launch_profile_keeps_botting_profiles_when_not_sharing() {
        let settings = temp_settings("effective-split");
        settings
            .set("General", "BottingUseSharedClientProfile", "false")
            .unwrap();
        assert!(matches!(
            effective_launch_profile(&settings, LaunchClientProfile::BottingBot),
            LaunchClientProfile::BottingBot
        ));
        assert!(matches!(
            effective_launch_profile(&settings, LaunchClientProfile::BottingPlayer),
            LaunchClientProfile::BottingPlayer
        ));
        assert!(matches!(
            effective_launch_profile(&settings, LaunchClientProfile::Normal),
            LaunchClientProfile::Normal
        ));
    }


    // ---- exceções de cliente por conta ------------------------------------

    fn campos(pares: &[(&str, &str)]) -> HashMap<String, String> {
        pares
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn account_client_overrides_needs_the_switch_turned_on() {
        // Os valores estão lá, mas o interruptor não: guardar uma configuração
        // desligada tem que ser possível sem ela vazar para o launch.
        let fields = campos(&[
            ("ClientOverrideMaxFPS", "240"),
            ("ClientOverrideFullscreen", "true"),
        ]);
        assert_eq!(account_client_overrides(&fields), None);

        let fields = campos(&[
            ("ClientOverridesEnabled", "false"),
            ("ClientOverrideMaxFPS", "240"),
        ]);
        assert_eq!(account_client_overrides(&fields), None);
    }

    #[test]
    fn account_client_overrides_reads_the_whole_exception() {
        let fields = campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideMaxFPS", "240"),
            ("ClientOverrideVolume", "0.2"),
            ("ClientOverrideGraphics", "auto"),
            ("ClientOverrideFullscreen", "true"),
            ("ClientOverrideStartMinimized", "false"),
        ]);
        let o = account_client_overrides(&fields).expect("conta com exceção");
        assert_eq!(o.max_fps, Some(240));
        assert_eq!(o.master_volume, Some(0.2));
        assert_eq!(o.graphics, Some(GraphicsQuality::Automatic));
        assert_eq!(o.fullscreen, Some(true));
        assert_eq!(o.start_minimized, Some(false));
        assert_eq!(o.window_size, None);
    }

    #[test]
    fn account_client_overrides_ignores_an_empty_or_broken_value() {
        let fields = campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideMaxFPS", "   "),
            ("ClientOverrideVolume", "alto"),
            ("ClientOverrideGraphics", "11"),
            ("ClientOverrideFullscreen", "talvez"),
        ]);
        assert_eq!(account_client_overrides(&fields), None);
    }

    #[test]
    fn account_client_overrides_takes_a_fixed_graphics_level() {
        let fields = campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideGraphics", "7"),
        ]);
        let o = account_client_overrides(&fields).expect("nível fixo");
        assert_eq!(o.graphics, Some(GraphicsQuality::Level(7)));
    }

    #[test]
    fn account_client_overrides_clamps_the_volume() {
        let fields = campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideVolume", "5"),
        ]);
        let o = account_client_overrides(&fields).expect("volume");
        assert_eq!(o.master_volume, Some(1.0));
    }

    #[test]
    fn account_client_overrides_needs_both_sides_of_the_window() {
        // Largura sem altura é configuração incompleta: aplicar só uma deixaria
        // o cliente num tamanho que ninguém pediu.
        let fields = campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideWindowWidth", "1920"),
        ]);
        assert_eq!(account_client_overrides(&fields), None);

        let fields = campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideWindowWidth", "1920"),
            ("ClientOverrideWindowHeight", "1080"),
        ]);
        let o = account_client_overrides(&fields).expect("janela");
        assert_eq!(o.window_size, Some((1920, 1080)));
    }

    // ---- windows_client_overrides: global embaixo, conta em cima -----------

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_keeps_the_global_when_the_account_is_silent() {
        let settings = temp_settings("acc-override-silent");
        settings.set("General", "UnlockFPS", "true").unwrap();
        settings.set("General", "MaxFPSValue", "60").unwrap();
        settings
            .set("General", "OverrideClientGraphics", "true")
            .unwrap();
        settings.set("General", "ClientGraphicsLevel", "1").unwrap();

        let conta = AccountClientOverrides {
            master_volume: Some(0.2),
            ..Default::default()
        };
        let o = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, Some(&conta));
        assert_eq!(o.max_fps, Some(60));
        assert_eq!(o.graphics, Some(GraphicsQuality::Level(1)));
        assert_eq!(o.master_volume, Some(0.2));
        // Só o volume veio da conta: é só ele que o registro de exceções guarda
        // para desfazer na próxima conta sem exceção.
        assert_eq!(
            o.from_account,
            AccountSourced { volume: true, ..Default::default() }
        );
        let sem_conta = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);
        assert_eq!(sem_conta.from_account, AccountSourced::default());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_lets_the_account_win() {
        let settings = temp_settings("acc-override-wins");
        settings.set("General", "UnlockFPS", "true").unwrap();
        settings.set("General", "MaxFPSValue", "60").unwrap();
        settings
            .set("General", "OverrideClientGraphics", "true")
            .unwrap();
        settings.set("General", "ClientGraphicsLevel", "1").unwrap();
        settings
            .set("General", "OverrideClientWindowSize", "true")
            .unwrap();
        settings.set("General", "ClientWindowWidth", "320").unwrap();
        settings.set("General", "ClientWindowHeight", "240").unwrap();

        let conta = AccountClientOverrides {
            max_fps: Some(240),
            graphics: Some(GraphicsQuality::Automatic),
            fullscreen: Some(true),
            ..Default::default()
        };
        let o = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, Some(&conta));
        assert_eq!(o.max_fps, Some(240));
        assert_eq!(o.graphics, Some(GraphicsQuality::Automatic));
        assert_eq!(o.fullscreen, Some(true));
        // Tela cheia pedida sem tamanho próprio descarta a janelinha global —
        // senão o XML gravaria Fullscreen=false junto e a tela cheia sumiria.
        assert_eq!(o.window_size, None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_keeps_the_account_window_size_next_to_fullscreen() {
        let settings = temp_settings("acc-override-size");
        let conta = AccountClientOverrides {
            fullscreen: Some(true),
            window_size: Some((1920, 1080)),
            ..Default::default()
        };
        let o = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, Some(&conta));
        assert_eq!(o.fullscreen, Some(true));
        assert_eq!(o.window_size, Some((1920, 1080)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_respects_a_custom_settings_file_over_the_account_fps() {
        // allow_fps_override = false quer dizer "o usuário apontou o próprio
        // ClientAppSettings.json". Nem o global nem a conta mexem no FPS dele.
        let settings = temp_settings("acc-override-custom");
        settings.set("General", "UnlockFPS", "true").unwrap();
        settings.set("General", "MaxFPSValue", "60").unwrap();

        let conta = AccountClientOverrides {
            max_fps: Some(240),
            ..Default::default()
        };
        let o =
            windows_client_overrides(&settings, false, LaunchClientProfile::Normal, Some(&conta));
        assert_eq!(o.max_fps, None);
    }

    // ---- start_minimized_for_profile / custom_client_settings_path ---------

    #[test]
    fn start_minimized_for_profile_reads_the_per_profile_key() {
        let settings = temp_settings("minimized");
        settings
            .set("General", "BottingBotStartRobloxMinimized", "true")
            .unwrap();

        assert!(!start_minimized_for_profile(
            &settings,
            LaunchClientProfile::Normal
        ));
        assert!(!start_minimized_for_profile(
            &settings,
            LaunchClientProfile::BottingPlayer
        ));
        assert!(start_minimized_for_profile(
            &settings,
            LaunchClientProfile::BottingBot
        ));
    }

    #[test]
    fn custom_client_settings_path_reads_the_per_profile_key() {
        let settings = temp_settings("custom-path");
        settings
            .set("General", "CustomClientSettings", "C:/normal.json")
            .unwrap();
        settings
            .set(
                "General",
                "BottingPlayerCustomClientSettings",
                "C:/player.json",
            )
            .unwrap();

        assert_eq!(
            custom_client_settings_path(&settings, LaunchClientProfile::Normal),
            "C:/normal.json"
        );
        assert_eq!(
            custom_client_settings_path(&settings, LaunchClientProfile::BottingPlayer),
            "C:/player.json"
        );
        assert_eq!(
            custom_client_settings_path(&settings, LaunchClientProfile::BottingBot),
            ""
        );
    }

    // ---- windows_client_overrides ------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_are_all_off_by_default() {
        let settings = temp_settings("overrides-default");
        let overrides = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);

        assert_eq!(overrides.max_fps, None);
        assert_eq!(overrides.master_volume, None);
        assert_eq!(overrides.graphics, None);
        assert_eq!(overrides.window_size, None);
        assert!(overrides.fast_flags.is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_reads_every_enabled_override() {
        let settings = temp_settings("overrides-on");
        settings.set("General", "UnlockFPS", "true").unwrap();
        settings.set("General", "MaxFPSValue", "240").unwrap();
        settings.set("General", "OverrideClientVolume", "true").unwrap();
        settings.set("General", "ClientVolume", "0.25").unwrap();
        settings.set("General", "OverrideClientGraphics", "true").unwrap();
        settings.set("General", "ClientGraphicsLevel", "7").unwrap();
        settings.set("General", "OverrideClientWindowSize", "true").unwrap();
        settings.set("General", "ClientWindowWidth", "800").unwrap();
        settings.set("General", "ClientWindowHeight", "600").unwrap();

        let overrides = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);
        assert_eq!(overrides.max_fps, Some(240));
        assert_eq!(overrides.master_volume, Some(0.25));
        assert_eq!(overrides.graphics, Some(GraphicsQuality::Level(7)));
        assert_eq!(overrides.window_size, Some((800, 600)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_skips_fps_when_the_caller_disallows_it() {
        // A valid custom ClientAppSettings file wins over the FPS unlock.
        let settings = temp_settings("overrides-no-fps");
        settings.set("General", "UnlockFPS", "true").unwrap();
        settings.set("General", "MaxFPSValue", "240").unwrap();

        let overrides = windows_client_overrides(&settings, false, LaunchClientProfile::Normal, None);
        assert_eq!(overrides.max_fps, None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_rejects_non_positive_numbers() {
        let settings = temp_settings("overrides-zero");
        settings.set("General", "UnlockFPS", "true").unwrap();
        settings.set("General", "MaxFPSValue", "0").unwrap();
        settings.set("General", "OverrideClientGraphics", "true").unwrap();
        settings.set("General", "ClientGraphicsLevel", "0").unwrap();
        settings.set("General", "OverrideClientWindowSize", "true").unwrap();
        settings.set("General", "ClientWindowWidth", "0").unwrap();
        settings.set("General", "ClientWindowHeight", "600").unwrap();

        let overrides = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);
        assert_eq!(overrides.max_fps, None);
        assert_eq!(overrides.graphics, None);
        assert_eq!(overrides.window_size, None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_clamps_volume_and_graphics_into_range() {
        let settings = temp_settings("overrides-clamp");
        settings.set("General", "OverrideClientVolume", "true").unwrap();
        settings.set("General", "ClientVolume", "9.5").unwrap();
        settings.set("General", "OverrideClientGraphics", "true").unwrap();
        settings.set("General", "ClientGraphicsLevel", "99").unwrap();

        let overrides = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);
        assert_eq!(overrides.master_volume, Some(1.0));
        assert_eq!(overrides.graphics, Some(GraphicsQuality::Level(10)));

        settings.set("General", "ClientVolume", "-3").unwrap();
        let overrides = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);
        assert_eq!(overrides.master_volume, Some(0.0));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_falls_back_to_defaults_for_unparsable_numbers() {
        let settings = temp_settings("overrides-garbage");
        settings.set("General", "UnlockFPS", "true").unwrap();
        settings.set("General", "MaxFPSValue", "not-a-number").unwrap();
        settings.set("General", "OverrideClientVolume", "true").unwrap();
        settings.set("General", "ClientVolume", "loud").unwrap();

        let overrides = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);
        assert_eq!(overrides.max_fps, Some(120));
        assert_eq!(overrides.master_volume, Some(0.5));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_client_overrides_uses_the_bot_profile_keys() {
        let settings = temp_settings("overrides-bot");
        settings.set("General", "BottingBotUnlockFPS", "true").unwrap();
        settings.set("General", "BottingBotMaxFPSValue", "30").unwrap();
        // The Normal keys must not leak into the bot profile.
        settings.set("General", "MaxFPSValue", "240").unwrap();

        let overrides = windows_client_overrides(&settings, true, LaunchClientProfile::BottingBot, None);
        assert_eq!(overrides.max_fps, Some(30));

        let normal = windows_client_overrides(&settings, true, LaunchClientProfile::Normal, None);
        assert_eq!(normal.max_fps, None, "Normal has UnlockFPS off");
    }

    // ---- browser tracker id -------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn get_or_create_browser_tracker_id_keeps_an_existing_id() {
        let store = temp_accounts("btid-existing");
        let mut account = data::accounts::Account::new("TOK".into(), "u".into(), 1);
        account.browser_tracker_id = "  1234567  ".to_string();
        store.add(account).unwrap();
        // `add` only merges a few fields for an existing id, so write the
        // tracker id through `update`.
        let mut stored = store.get_all().unwrap().remove(0);
        stored.browser_tracker_id = "  1234567  ".to_string();
        store.update(stored).unwrap();

        assert_eq!(get_or_create_browser_tracker_id(&store, 1).unwrap(), "1234567");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn get_or_create_browser_tracker_id_generates_and_persists_when_blank() {
        let store = temp_accounts("btid-generate");
        store
            .add(data::accounts::Account::new("TOK".into(), "u".into(), 2))
            .unwrap();

        let generated = get_or_create_browser_tracker_id(&store, 2).unwrap();
        assert!(!generated.trim().is_empty());
        assert!(generated.chars().all(|c| c.is_ascii_digit()));

        let persisted = store.get_all().unwrap()[0].browser_tracker_id.clone();
        assert_eq!(persisted, generated);
        // A second call must reuse the persisted id, not roll a new one.
        assert_eq!(get_or_create_browser_tracker_id(&store, 2).unwrap(), generated);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn get_or_create_browser_tracker_id_for_an_unknown_account_does_not_persist() {
        let store = temp_accounts("btid-unknown");
        let generated = get_or_create_browser_tracker_id(&store, 404).unwrap();
        assert!(!generated.is_empty());
        assert!(store.get_all().unwrap().is_empty());
    }

    #[test]
    fn save_browser_tracker_id_is_a_no_op_for_a_missing_account() {
        let store = temp_accounts("btid-save-missing");
        assert!(save_browser_tracker_id(&store, 999, "1").is_ok());
        assert!(store.get_all().unwrap().is_empty());
    }

    // ---- now_ms -------------------------------------------------------------

    #[test]
    fn now_ms_returns_a_plausible_unix_millisecond_timestamp() {
        let now = now_ms();
        // 2023-01-01 .. 2100-01-01 in milliseconds.
        assert!(now > 1_672_531_200_000, "now_ms looks too small: {now}");
        assert!(now < 4_102_444_800_000, "now_ms looks too large: {now}");
        assert!(now_ms() >= now, "now_ms must be monotonic in wall-clock terms");
    }

    // ---- decode_url_component / strip_ascii_prefix -------------------------

    #[test]
    fn decode_url_component_decodes_percent_escapes() {
        assert_eq!(decode_url_component("a%20b"), "a b");
        assert_eq!(decode_url_component("roblox%3A%2F%2F"), "roblox://");
        assert_eq!(decode_url_component("%C3%A9"), "é");
    }

    #[test]
    fn decode_url_component_returns_invalid_input_unchanged() {
        assert_eq!(decode_url_component("100%"), "100%");
        assert_eq!(decode_url_component("%ZZ"), "%ZZ");
        assert_eq!(decode_url_component(""), "");
        assert_eq!(decode_url_component("já-decodificado"), "já-decodificado");
    }

    #[test]
    fn strip_ascii_prefix_is_case_insensitive_and_bounds_safe() {
        assert_eq!(strip_ascii_prefix("vip:ABC", "vip:"), Some("ABC"));
        assert_eq!(strip_ascii_prefix("VIP:ABC", "vip:"), Some("ABC"));
        assert_eq!(strip_ascii_prefix("vi", "vip:"), None);
        assert_eq!(strip_ascii_prefix("", "vip:"), None);
        assert_eq!(strip_ascii_prefix("nope:ABC", "vip:"), None);
        // Must not panic when the prefix length lands inside a multi-byte char.
        assert_eq!(strip_ascii_prefix("éé", "vip:"), None);
        assert_eq!(strip_ascii_prefix("ép:x", "vip:"), None);
    }

    // ---- looks_like_share_link ---------------------------------------------

    #[test]
    fn looks_like_share_link_matches_every_known_share_shape() {
        assert!(looks_like_share_link("https://www.roblox.com/share?code=x"));
        assert!(looks_like_share_link("https://ro.blox.com/share-links/abc"));
        assert!(looks_like_share_link("roblox://navigation/share_links?code=x"));
        assert!(looks_like_share_link("https://x/y?type=Server"));
        assert!(looks_like_share_link("https://x/y?pid=Server"));
        // Case-insensitive.
        assert!(looks_like_share_link("HTTPS://WWW.ROBLOX.COM/SHARE?CODE=X"));
    }

    #[test]
    fn looks_like_share_link_rejects_plain_game_and_vip_links() {
        assert!(!looks_like_share_link("https://www.roblox.com/games/123/Name"));
        assert!(!looks_like_share_link("vip:ABC"));
        assert!(!looks_like_share_link(""));
    }

    // ---- extract_private_server_link_code ----------------------------------

    #[test]
    fn extract_private_server_link_code_reads_the_vip_prefix() {
        assert_eq!(
            extract_private_server_link_code("vip:ABC123").as_deref(),
            Some("ABC123")
        );
        assert_eq!(
            extract_private_server_link_code("VIP:  ABC%20123 ").as_deref(),
            Some("ABC 123")
        );
        assert_eq!(extract_private_server_link_code("vip:"), None);
        assert_eq!(extract_private_server_link_code("vip:   "), None);
    }

    #[test]
    fn extract_private_server_link_code_prefers_private_server_link_code() {
        assert_eq!(
            extract_private_server_link_code(
                "https://www.roblox.com/games/1/x?privateServerLinkCode=AAA&linkCode=BBB"
            )
            .as_deref(),
            Some("AAA")
        );
    }

    #[test]
    fn extract_private_server_link_code_falls_back_to_link_code() {
        assert_eq!(
            extract_private_server_link_code("https://www.roblox.com/games/1/x?linkCode=BBB")
                .as_deref(),
            Some("BBB")
        );
    }

    #[test]
    fn extract_private_server_link_code_reads_code_only_from_share_shaped_input() {
        // A bare `code=` query on a non-share URL is not a private server code.
        assert_eq!(
            extract_private_server_link_code("https://www.roblox.com/games/1/x?code=CCC"),
            None
        );
        assert_eq!(
            extract_private_server_link_code("code=CCC").as_deref(),
            Some("CCC")
        );
        assert_eq!(
            extract_private_server_link_code("https://www.roblox.com/share?code=CCC").as_deref(),
            Some("CCC")
        );
    }

    #[test]
    fn extract_private_server_link_code_returns_none_for_plain_input() {
        assert_eq!(extract_private_server_link_code(""), None);
        assert_eq!(extract_private_server_link_code("   "), None);
        assert_eq!(
            extract_private_server_link_code("11111111-2222-3333-4444-555555555555"),
            None
        );
    }

    // ---- resolve_private_join (no network on these paths) ------------------

    #[tokio::test]
    async fn resolve_private_join_on_a_public_target_uses_the_requested_place() {
        let launch = resolve_launch_job("", false, "");
        let resolved = resolve_private_join("", 606849621, &launch).await.unwrap();

        assert_eq!(resolved.place_id, 606849621);
        assert!(resolved.link_code.is_empty());
        assert!(resolved.access_code.is_empty());
        assert!(!resolved.use_private_join);
    }

    #[tokio::test]
    async fn resolve_private_join_moves_an_access_code_shaped_value_into_access_code() {
        let launch = resolve_launch_job("vip:11111111-2222-3333-4444-555555555555", false, "");
        let resolved = resolve_private_join("", 1, &launch).await.unwrap();

        assert_eq!(resolved.access_code, "11111111-2222-3333-4444-555555555555");
        assert!(resolved.link_code.is_empty());
        assert!(resolved.use_private_join);
    }

    #[tokio::test]
    async fn resolve_private_join_keeps_a_plain_link_code() {
        let launch = resolve_launch_job("vip:SHORTCODE", false, "");
        let resolved = resolve_private_join("", 42, &launch).await.unwrap();

        assert_eq!(resolved.link_code, "SHORTCODE");
        assert!(resolved.access_code.is_empty());
        assert!(resolved.use_private_join);
        assert_eq!(resolved.place_id, 42);
    }

    #[tokio::test]
    async fn resolve_private_join_takes_the_place_id_from_a_games_url() {
        let launch = resolve_launch_job(
            "https://www.roblox.com/games/606849621/Jailbreak?privateServerLinkCode=SHORT",
            false,
            "",
        );
        let resolved = resolve_private_join("", 1, &launch).await.unwrap();

        assert_eq!(
            resolved.place_id, 606849621,
            "the URL's place id must win over the one passed in"
        );
        assert_eq!(resolved.link_code, "SHORT");
        assert!(resolved.use_private_join);
    }

    #[tokio::test]
    async fn resolve_private_join_marks_join_vip_even_without_a_code() {
        let launch = ResolvedLaunchJob {
            job_id: String::new(),
            join_vip: true,
            link_code: String::new(),
        };
        let resolved = resolve_private_join("", 5, &launch).await.unwrap();
        assert!(resolved.use_private_join);
        assert_eq!(resolved.place_id, 5);
    }

    // ---- backoff / 429 detection -------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    fn backoff_delay_seconds_doubles_and_clamps_to_the_5_to_300_window() {
        assert_eq!(backoff_delay_seconds(8, 1, 6), 8);
        assert_eq!(backoff_delay_seconds(8, 2, 6), 16);
        assert_eq!(backoff_delay_seconds(8, 3, 6), 32);
        assert_eq!(backoff_delay_seconds(8, 4, 6), 64);
        assert_eq!(backoff_delay_seconds(8, 5, 6), 128);
        assert_eq!(backoff_delay_seconds(8, 6, 6), 256);
        // Capped at 300 seconds however many retries pile up.
        assert_eq!(backoff_delay_seconds(8, 7, 6), 300);
        assert_eq!(backoff_delay_seconds(8, u32::MAX, 6), 300);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn backoff_delay_seconds_never_returns_less_than_five_seconds() {
        assert_eq!(backoff_delay_seconds(0, 1, 6), 5);
        assert_eq!(backoff_delay_seconds(1, 1, 6), 5);
        assert_eq!(backoff_delay_seconds(1, 3, 6), 5);
        assert_eq!(backoff_delay_seconds(1, 4, 6), 8);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn backoff_delay_seconds_treats_retry_count_zero_as_the_first_attempt() {
        assert_eq!(backoff_delay_seconds(10, 0, 6), 10);
        assert_eq!(backoff_delay_seconds(10, 1, 6), 10);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn backoff_delay_seconds_handles_degenerate_retry_max_without_overflowing() {
        assert_eq!(backoff_delay_seconds(10, 5, 0), 20);
        assert_eq!(backoff_delay_seconds(u64::MAX, 3, 6), 300);
        assert_eq!(backoff_delay_seconds(10, 40, u32::MAX), 300);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn is_429_related_error_matches_every_rate_limit_phrasing() {
        assert!(is_429_related_error("HTTP 429 Too Many Requests"));
        assert!(is_429_related_error("TOO MANY REQUESTS"));
        assert!(is_429_related_error("Authentifizierung fehlgeschlagen"));
        assert!(is_429_related_error("Authentication Failed"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn is_429_related_error_ignores_unrelated_failures() {
        assert!(!is_429_related_error(""));
        assert!(!is_429_related_error("network timeout"));
        assert!(!is_429_related_error("User is moderated"));
        assert!(!is_429_related_error("status 403"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn title_looks_auth_failure_matches_the_german_and_english_dialogs() {
        assert!(title_looks_auth_failure("Authentifizierung fehlgeschlagen"));
        assert!(title_looks_auth_failure("Authentication failed"));
        assert!(title_looks_auth_failure("Fehlercode: 429"));
        assert!(title_looks_auth_failure("Roblox — Error Code: 429"));
        assert!(!title_looks_auth_failure("Roblox"));
        // O título com o nome da conta (client_health.rs) não é falha.
        assert!(!title_looks_auth_failure("Main — Roblox"));
        assert!(!title_looks_auth_failure("Roblox — Main"));
        assert!(!title_looks_auth_failure(""));
        // A plain 429 in a window title is not enough on its own.
        assert!(!title_looks_auth_failure("429"));
    }

    // ---- wait_for_launch_slot ----------------------------------------------

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn wait_for_launch_slot_does_not_wait_on_the_first_launch() {
        // No previous launch means no spacing to honour, however large the
        // configured delay is.
        let start = std::time::Instant::now();
        let mut last: Option<std::time::Instant> = None;
        wait_for_launch_slot(&mut last, 3600).await;

        assert!(last.is_some(), "the slot must be stamped for the next launch");
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn wait_for_launch_slot_spaces_consecutive_launches_by_the_delay() {
        let mut last: Option<std::time::Instant> = None;
        wait_for_launch_slot(&mut last, 1).await;
        let first_stamp = last.expect("first launch stamps the slot");

        let start = std::time::Instant::now();
        wait_for_launch_slot(&mut last, 1).await;
        let waited = start.elapsed();

        assert!(
            waited >= std::time::Duration::from_millis(900),
            "expected roughly a 1s gap, waited {waited:?}"
        );
        assert!(
            last.expect("second launch re-stamps the slot") > first_stamp,
            "the slot timestamp must move forward"
        );
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn wait_for_launch_slot_with_a_zero_delay_never_blocks() {
        let mut last: Option<std::time::Instant> = None;
        wait_for_launch_slot(&mut last, 0).await;
        let start = std::time::Instant::now();
        wait_for_launch_slot(&mut last, 0).await;
        assert!(start.elapsed() < std::time::Duration::from_millis(500));
    }

    // ---- wait_for_new_roblox_pid -------------------------------------------

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn wait_for_new_roblox_pid_times_out_when_no_new_client_appears() {
        // Baseline = every client already running, so nothing can look "new".
        let baseline = platform::windows::get_roblox_pids();
        let found =
            wait_for_new_roblox_pid(&baseline, std::time::Duration::from_millis(1)).await;
        assert_eq!(found, None);
    }

    // ---- apply_windows_post_launch_profile ---------------------------------

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn apply_windows_post_launch_profile_returns_early_with_no_policy_configured() {
        // With the shipped defaults there is nothing to apply, so the function
        // must not touch the (here: nonexistent) process at all.
        let settings = temp_settings("post-launch-noop");
        let started = std::time::Instant::now();
        apply_windows_post_launch_profile(None, &settings, LaunchClientProfile::Normal, 0).await;
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    // ---- botting status payload --------------------------------------------

    #[cfg(target_os = "windows")]
    fn test_session(stopped: bool) -> BottingSession {
        let mut players = HashSet::new();
        players.insert(20);
        let cfg = BottingConfig {
            user_ids: vec![30, 10, 20],
            place_id: 606849621,
            job_id: "job-1".to_string(),
            launch_data: "data".to_string(),
            player_user_ids: players,
            interval_minutes: 19,
            launch_delay_seconds: 20,
            retry_max: 6,
            retry_base_seconds: 8,
            player_grace_minutes: 15,
            adopt_running: false,
        };
        let mut runtime = HashMap::new();
        for uid in [30_i64, 10, 20] {
            runtime.insert(
                uid,
                BottingAccountRuntime {
                    user_id: uid,
                    is_player: uid == 20,
                    disconnected: false,
                    manual_restart_pending: false,
                    manual_restart_keep_schedule: false,
                    manual_restart_saved_next_restart_at_ms: None,
                    phase: "queued",
                    retry_count: 0,
                    next_restart_at_ms: None,
                    player_grace_until_ms: None,
                    last_error: None,
                },
            );
        }
        BottingSession {
            id: 7,
            stop_flag: Arc::new(AtomicBool::new(stopped)),
            stopped_notify: Arc::new(tokio::sync::Notify::new()),
            started_at_ms: 1_700_000_000_000,
            config: Arc::new(Mutex::new(cfg)),
            accounts: Arc::new(Mutex::new(runtime)),
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_status_from_session_sorts_accounts_and_player_ids() {
        let payload = botting_status_from_session(&test_session(false));

        assert!(payload.active);
        assert_eq!(payload.started_at_ms, Some(1_700_000_000_000));
        assert_eq!(payload.place_id, 606849621);
        assert_eq!(payload.job_id, "job-1");
        assert_eq!(payload.launch_data, "data");
        assert_eq!(payload.interval_minutes, 19);
        assert_eq!(payload.launch_delay_seconds, 20);
        assert_eq!(payload.player_grace_minutes, 15);
        assert_eq!(payload.player_user_ids, vec![20]);
        // user_ids keeps the configured order; accounts are sorted for the UI.
        assert_eq!(payload.user_ids, vec![30, 10, 20]);
        let ids: Vec<i64> = payload.accounts.iter().map(|a| a.user_id).collect();
        assert_eq!(ids, vec![10, 20, 30]);
        assert!(payload.accounts.iter().any(|a| a.user_id == 20 && a.is_player));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_status_from_session_reports_a_stopping_session_as_inactive() {
        let payload = botting_status_from_session(&test_session(true));
        assert!(!payload.active);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn botting_status_payload_serializes_with_camel_case_keys() {
        let payload = botting_status_from_session(&test_session(false));
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["startedAtMs"], 1_700_000_000_000_i64);
        assert_eq!(json["intervalMinutes"], 19);
        assert_eq!(json["launchDelaySeconds"], 20);
        assert_eq!(json["playerGraceMinutes"], 15);
        assert_eq!(json["playerUserIds"], serde_json::json!([20]));
        assert_eq!(json["accounts"][0]["userId"], 10);
        assert_eq!(json["accounts"][0]["retryCount"], 0);
    }

    #[test]
    fn botting_status_payload_default_is_an_inactive_session() {
        let payload = BottingStatusPayload::default();
        assert!(!payload.active);
        assert_eq!(payload.started_at_ms, None);
        assert!(payload.accounts.is_empty());
        assert!(payload.user_ids.is_empty());
    }
}

#[cfg(test)]
mod client_window_plan_tests {
    use super::*;

    fn inputs() -> ClientWindowInputs {
        ClientWindowInputs::default()
    }

    /// O bug de 03/10/2026: a alt abria com o tamanho da conta principal porque
    /// o XML é compartilhado e o cliente da principal o reescrevia antes de a
    /// alt ler. O tamanho resolvido para ESTA conta passa a ser imposto na
    /// janela dela, pelo PID.
    #[test]
    fn the_size_resolved_for_this_account_is_enforced_on_its_window() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((520, 420)),
            ..inputs()
        });
        assert_eq!(plan.size, Some((520, 420)));
        assert!(!plan.is_noop());
    }

    #[test]
    fn the_enforced_size_has_the_same_floor_the_xml_gets() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((100, 50)),
            ..inputs()
        });
        assert_eq!(plan.size, Some((320, 240)));
    }

    #[test]
    fn without_a_size_nothing_is_enforced() {
        let plan = client_window_plan(inputs());
        assert_eq!(plan.size, None);
        assert!(plan.is_noop());
    }

    #[test]
    fn a_fullscreen_launch_is_left_alone() {
        let plan = client_window_plan(ClientWindowInputs {
            fullscreen: Some(true),
            window_size: Some((1920, 1080)),
            saved_rect: Some((0, 0, 800, 600)),
            ..inputs()
        });
        assert!(plan.is_noop(), "{plan:?}");
    }

    #[test]
    fn an_explicitly_windowed_launch_still_gets_its_size() {
        let plan = client_window_plan(ClientWindowInputs {
            fullscreen: Some(false),
            window_size: Some((800, 600)),
            ..inputs()
        });
        assert_eq!(plan.size, Some((800, 600)));
    }

    /// Começar minimizado é pedido do usuário: mexer na janela a traria de
    /// volta, ou brigaria com o `minimize_new_roblox_windows`.
    #[test]
    fn a_start_minimized_launch_is_left_alone() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((520, 420)),
            start_minimized: true,
            saved_rect: Some((10, 10, 520, 420)),
            ..inputs()
        });
        assert!(plan.is_noop(), "{plan:?}");
    }

    #[test]
    fn the_saved_window_rectangle_is_carried_into_the_plan() {
        let plan = client_window_plan(ClientWindowInputs {
            saved_rect: Some((-1910, 20, 800, 600)),
            ..inputs()
        });
        assert_eq!(plan.saved_rect, Some((-1910, 20, 800, 600)));
        assert!(!plan.is_noop());
    }

    // ── grade automática ────────────────────────────────────────────────────

    #[test]
    fn an_account_without_its_own_size_goes_into_the_grid() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((520, 420)),
            auto_arrange_grid: true,
            ..inputs()
        });
        assert!(plan.grid);
        assert_eq!(plan.size, Some((520, 420)));
    }

    /// Sem tamanho global, a janela entra na grade com o tamanho que abriu.
    #[test]
    fn the_grid_alone_is_a_plan() {
        let plan = client_window_plan(ClientWindowInputs {
            auto_arrange_grid: true,
            ..inputs()
        });
        assert!(plan.grid);
        assert_eq!(plan.size, None);
        assert!(!plan.is_noop());
    }

    /// A conta principal com tamanho próprio fica fora da grade: mantém o
    /// tamanho e a posição dela.
    #[test]
    fn an_account_with_its_own_window_size_stays_out_of_the_grid() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((1000, 1000)),
            keeps_own_window: true,
            auto_arrange_grid: true,
            saved_rect: Some((40, 40, 1016, 1039)),
            ..inputs()
        });
        assert!(!plan.grid);
        assert_eq!(plan.size, Some((1000, 1000)));
        assert_eq!(plan.saved_rect, Some((40, 40, 1016, 1039)));
    }

    #[test]
    fn the_grid_switched_off_places_nothing() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((520, 420)),
            auto_arrange_grid: false,
            ..inputs()
        });
        assert!(!plan.grid);
    }

    #[test]
    fn the_grid_wins_over_the_saved_window_position() {
        let plan = client_window_plan(ClientWindowInputs {
            auto_arrange_grid: true,
            saved_rect: Some((40, 40, 536, 459)),
            ..inputs()
        });
        assert!(plan.grid);
        assert_eq!(plan.saved_rect, None);
    }

    #[test]
    fn a_start_minimized_account_is_not_pulled_into_the_grid() {
        let plan = client_window_plan(ClientWindowInputs {
            auto_arrange_grid: true,
            start_minimized: true,
            ..inputs()
        });
        assert!(plan.is_noop(), "{plan:?}");
    }

    #[test]
    fn a_fullscreen_account_is_not_pulled_into_the_grid() {
        let plan = client_window_plan(ClientWindowInputs {
            auto_arrange_grid: true,
            fullscreen: Some(true),
            ..inputs()
        });
        assert!(plan.is_noop(), "{plan:?}");
    }

    // ── tela cheia herdada de outra conta ───────────────────────────────────
    //
    // O bug de 03/10/2026: a principal com a exceção "Tela: cheia" e as alts
    // sem exceção. O cliente da principal regrava `Fullscreen=true` no XML
    // compartilhado, a alt nasce em tela cheia, e a conferência pelo PID
    // pulava janela em tela cheia — a alt ficava assim.

    /// O cenário do dono: perfil global 520x420. Com tamanho no plano, a conta
    /// é "em janela" por definição — tela cheia na janela dela veio de outra.
    #[test]
    fn an_alt_with_the_global_size_leaves_a_fullscreen_it_inherited() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((520, 420)),
            auto_arrange_grid: true,
            ..inputs()
        });
        assert!(plan.leave_fullscreen, "{plan:?}");
        assert!(plan.grid);
    }

    /// Sem tamanho global: o registro das exceções devolveu `Fullscreen=false`
    /// (o valor do jogador) e o launch passa isso adiante como "em janela".
    #[test]
    fn an_alt_resolved_as_windowed_leaves_fullscreen_even_without_a_size() {
        let plan = client_window_plan(ClientWindowInputs {
            fullscreen: Some(false),
            ..inputs()
        });
        assert!(plan.leave_fullscreen, "{plan:?}");
        assert!(!plan.is_noop());
    }

    /// Nada resolvido para a janela: a tela cheia é a preferência do próprio
    /// jogador no Roblox, e o app não mexe nela.
    #[test]
    fn an_alt_with_nothing_resolved_keeps_the_players_own_fullscreen() {
        let plan = client_window_plan(ClientWindowInputs {
            auto_arrange_grid: true,
            ..inputs()
        });
        assert!(!plan.leave_fullscreen, "{plan:?}");
    }

    /// A conta principal com a exceção continua em tela cheia e fora da grade.
    #[test]
    fn the_fullscreen_main_account_still_opens_fullscreen_out_of_the_grid() {
        let plan = client_window_plan(ClientWindowInputs {
            fullscreen: Some(true),
            keeps_own_window: true,
            auto_arrange_grid: true,
            ..inputs()
        });
        assert!(plan.is_noop(), "{plan:?}");
        assert!(!plan.leave_fullscreen);
    }

    #[test]
    fn a_start_minimized_alt_never_leaves_fullscreen() {
        let plan = client_window_plan(ClientWindowInputs {
            window_size: Some((520, 420)),
            start_minimized: true,
            ..inputs()
        });
        assert!(!plan.leave_fullscreen, "{plan:?}");
    }

    // ── quem fica fora da grade ─────────────────────────────────────────────

    fn campos(pares: &[(&str, &str)]) -> HashMap<String, String> {
        pares.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn an_account_window_size_exception_keeps_its_own_window() {
        assert!(account_keeps_own_window(&campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideWindowWidth", "1000"),
            ("ClientOverrideWindowHeight", "1000"),
        ])));
    }

    #[test]
    fn an_account_fullscreen_exception_keeps_its_own_window() {
        assert!(account_keeps_own_window(&campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideFullscreen", "true"),
        ])));
    }

    #[test]
    fn an_exception_that_does_not_touch_the_window_joins_the_grid() {
        assert!(!account_keeps_own_window(&campos(&[
            ("ClientOverridesEnabled", "true"),
            ("ClientOverrideMaxFPS", "240"),
        ])));
        // Exceção desligada não vale, nem para a janela.
        assert!(!account_keeps_own_window(&campos(&[
            ("ClientOverridesEnabled", "false"),
            ("ClientOverrideWindowWidth", "1000"),
            ("ClientOverrideWindowHeight", "1000"),
        ])));
        assert!(!account_keeps_own_window(&HashMap::new()));
    }

    #[test]
    fn only_the_pids_of_accounts_with_their_own_window_are_excluded() {
        let mut fields = HashMap::new();
        fields.insert(
            1_i64,
            campos(&[
                ("ClientOverridesEnabled", "true"),
                ("ClientOverrideWindowWidth", "1000"),
                ("ClientOverrideWindowHeight", "1000"),
            ]),
        );
        fields.insert(2_i64, campos(&[]));
        // A conta 3 está rastreada mas não existe mais na lista: entra na grade.
        let tracked = [(1_i64, 100_u32), (2, 200), (3, 300)];

        let excluded = pids_keeping_own_window(&tracked, &fields);
        assert_eq!(excluded, [100_u32].into_iter().collect());
    }

    // ── configuração da grade ───────────────────────────────────────────────

    fn temp_settings(tag: &str) -> SettingsStore {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        SettingsStore::new(std::env::temp_dir().join(format!("ram-cwplan-{tag}-{nanos}.ini")))
    }

    #[test]
    fn the_automatic_grid_is_on_by_default() {
        let settings = temp_settings("grid-default");
        assert!(auto_arrange_grid_enabled(&settings));
        // Chave apagada do INI: continua o padrão.
        settings.set("General", "AutoArrangeGrid", "").unwrap();
        assert!(auto_arrange_grid_enabled(&settings));
    }

    #[test]
    fn the_automatic_grid_turns_off_only_with_false() {
        let settings = temp_settings("grid-off");
        settings.set("General", "AutoArrangeGrid", "false").unwrap();
        assert!(!auto_arrange_grid_enabled(&settings));
    }

    /// Ideia 22: célula menor que o mínimo e sem moldura, as duas opcionais.
    #[cfg(target_os = "windows")]
    #[test]
    fn small_and_borderless_grid_windows_are_off_by_default() {
        let settings = temp_settings("grid-style");
        assert_eq!(grid_window_style(&settings), platform::windows::GridWindowStyle::default());
        settings.set("General", "GridAllowSmallWindows", "true").unwrap();
        settings.set("General", "GridBorderless", "true").unwrap();
        assert_eq!(
            grid_window_style(&settings),
            platform::windows::GridWindowStyle {
                allow_small: true,
                borderless: true
            }
        );
    }

    #[test]
    fn the_grid_layout_reads_the_monitors_and_the_gap_of_the_windows_tab() {
        let settings = temp_settings("grid-layout");
        settings.set("General", "GridMonitors", "2, 3,x,0").unwrap();
        settings.set("General", "GridGap", "12").unwrap();
        assert_eq!(grid_layout_settings(&settings), (vec![2, 3], 12));
    }

    #[test]
    fn the_grid_layout_defaults_to_every_monitor_and_a_20px_gap() {
        let settings = temp_settings("grid-layout-default");
        assert_eq!(grid_layout_settings(&settings), (vec![], 20));
        settings.set("General", "GridGap", "9999").unwrap();
        assert_eq!(grid_layout_settings(&settings).1, 200);
        settings.set("General", "GridGap", "-5").unwrap();
        assert_eq!(grid_layout_settings(&settings).1, 0);
    }

    #[test]
    fn the_global_window_size_follows_the_profile() {
        let settings = temp_settings("global-size");
        assert_eq!(global_window_size(&settings, LaunchClientProfile::Normal), None);

        settings.set("General", "OverrideClientWindowSize", "true").unwrap();
        settings.set("General", "ClientWindowWidth", "520").unwrap();
        settings.set("General", "ClientWindowHeight", "420").unwrap();
        assert_eq!(
            global_window_size(&settings, LaunchClientProfile::Normal),
            Some((520, 420))
        );
        assert_eq!(global_window_size(&settings, LaunchClientProfile::BottingBot), None);
    }
}

/// A ordem dentro dos três caminhos de launch do Windows. Não dá para rodar o
/// launch num teste (precisa de Roblox, de rede e de conta), então o teste lê o
/// próprio código: o patch do XML compartilhado tem de ser o último passo
/// antes do spawn — depois do fechamento gracioso e das idas à rede, que são
/// justamente o intervalo em que um cliente já aberto reescrevia o arquivo — e
/// a janela tem de ser conferida pelo PID depois que ele aparece.
#[cfg(test)]
mod client_window_order_tests {
    /// Só o código de produção, sem os módulos de teste do fim do arquivo.
    fn producao(tudo: &'static str) -> &'static str {
        let fim = tudo.find("\n#[cfg(test)]").unwrap_or(tudo.len());
        &tudo[..fim]
    }

    /// Da assinatura até a `}` de coluna zero que fecha a função (`\n` ou
    /// `\r\n`: a CI faz checkout com CRLF).
    fn corpo(fonte: &'static str, assinatura: &str) -> &'static str {
        let inicio = fonte
            .find(assinatura)
            .unwrap_or_else(|| panic!("não achei `{assinatura}`"));
        let resto = &fonte[inicio..];
        let fim = resto
            .match_indices("\n}")
            .map(|(i, _)| i)
            .find(|&i| matches!(resto.as_bytes().get(i + 2), Some(b'\n' | b'\r')))
            .unwrap_or(resto.len());
        &resto[..fim]
    }

    fn pos(corpo: &str, trecho: &str, onde: &str) -> usize {
        corpo
            .find(trecho)
            .unwrap_or_else(|| panic!("`{trecho}` não aparece em {onde}"))
    }

    fn caminhos() -> [(&'static str, &'static str); 3] {
        let launch = producao(include_str!("launch.rs"));
        let botting = producao(include_str!("botting.rs"));
        [
            ("launch_roblox_windows", corpo(launch, "async fn launch_roblox_windows(")),
            ("launch_multiple", corpo(launch, "async fn launch_multiple(")),
            ("launch_account_for_cycle", corpo(botting, "async fn launch_account_for_cycle(")),
        ]
    }

    #[test]
    fn the_shared_xml_is_patched_right_before_the_spawn() {
        for (nome, corpo) in caminhos() {
            let patch = pos(corpo, "patch_client_settings_for_launch(", nome);
            for antes in ["kill_for_user_graceful_async", "get_auth_ticket", "resolve_private_join("] {
                assert!(
                    pos(corpo, antes, nome) < patch,
                    "{nome}: o patch do XML vem antes de `{antes}` — um cliente aberto reescreve o arquivo nesse intervalo"
                );
            }
            assert!(
                patch < pos(corpo, "let pids_before", nome),
                "{nome}: o patch do XML tem de vir antes do spawn"
            );
        }
    }

    #[test]
    fn the_window_is_enforced_by_pid_after_it_shows_up() {
        for (nome, corpo) in caminhos() {
            let pid = pos(corpo, "wait_for_new_roblox_pid(", nome);
            let enforce = pos(corpo, "spawn_client_window_enforcement(", nome);
            assert!(pid < enforce, "{nome}: a janela só é conferida depois do PID");
        }
    }
}
