// Teto de memória que libera RAM antes de fechar o cliente. Ver
// docs/features/watcher.md ("Teto de memória").
//
// Cada cliente **que o app abriu** pode ter um limite de memória: o padrão de
// todos (`Optimization.MemoryLimit`, MB, 0 = sem limite) ou o da conta (campo
// `MemoryLimit`, que a lista "Em jogo" da página Session grava). Passou do
// limite: primeiro o app pede ao Windows para tirar da RAM o que o cliente não
// está usando (`trim_working_set`, em platform/windows). Só se continuar acima
// um minuto depois **e** a opção de fechar do Watcher estiver ligada
// (`Watcher.Enabled` + `Watcher.CloseRbxMemory`), o cliente é fechado.
// Sem a opção de fechar, o app pede de novo a cada minuto.
//
// Nunca toca cliente aberto pelo site (adotado), nem a janela que a pessoa está
// usando agora. A leitura de memória é a do working set, a mesma do Watcher.
//
// O pedido ao Windows só existe com a feature `memory-trim` (edição completa,
// como o plano das ideias decidiu); sem ela, o teto inteiro fica desligado e a
// tela não mostra a opção.

/// Carência de um cliente recém-aberto: carregando, a memória sobe e desce.
const MEMORY_STARTUP_GRACE_MS: i64 = 30_000;
/// Depois de liberar, quanto esperar antes de conferir de novo.
const MEMORY_TRIM_GRACE_MS: i64 = 60_000;
/// Limite mínimo aceito: abaixo disso o cliente nem joga, e o app ficaria
/// pedindo para liberar memória o tempo todo.
const MEMORY_LIMIT_MIN_MB: u64 = 256;
const MEMORY_LIMIT_MAX_MB: u64 = 65_536;

/// O pedido ao Windows existe neste binário?
fn memory_trim_supported() -> bool {
    cfg!(all(target_os = "windows", feature = "memory-trim"))
}

/// Um valor de limite escrito (INI ou campo da conta): `Some(0)` = sem limite,
/// `Some(mb)` = limite, `None` = vazio ou ilegível.
fn parse_memory_limit(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.eq_ignore_ascii_case("off") {
        return Some(0);
    }
    let mb = raw.parse::<u64>().ok()?;
    Some(if mb == 0 {
        0
    } else {
        mb.clamp(MEMORY_LIMIT_MIN_MB, MEMORY_LIMIT_MAX_MB)
    })
}

/// O limite que vale para a conta: o campo dela vence; sem ele, o padrão.
/// `None` = sem limite.
fn effective_memory_limit(account_field: Option<&str>, default_raw: &str) -> Option<u64> {
    let own = account_field.and_then(parse_memory_limit);
    let mb = own.or_else(|| parse_memory_limit(default_raw)).unwrap_or(0);
    (mb > 0).then_some(mb)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryAction {
    Nothing,
    /// Pedir ao Windows para liberar a memória do cliente.
    Trim,
    /// Continuou acima depois de liberar, e fechar está permitido.
    Close,
}

/// O acompanhamento de um cliente.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MemoryWatch {
    pid: u32,
    first_seen_ms: i64,
    /// Quando o app pediu para liberar pela última vez (enquanto acima).
    trimmed_at_ms: Option<i64>,
}

impl MemoryWatch {
    fn new(pid: u32, now_ms: i64) -> Self {
        Self {
            pid,
            first_seen_ms: now_ms,
            trimmed_at_ms: None,
        }
    }
}

/// A decisão de uma passada para um cliente. `in_use`: é a janela que a
/// pessoa está usando agora (nunca é tocada; o estado fica como estava).
fn memory_ceiling_step(
    watch: &mut MemoryWatch,
    now_ms: i64,
    memory_mb: Option<u64>,
    limit_mb: Option<u64>,
    in_use: bool,
    close_allowed: bool,
) -> MemoryAction {
    let (Some(memory), Some(limit)) = (memory_mb, limit_mb) else {
        watch.trimmed_at_ms = None;
        return MemoryAction::Nothing;
    };
    if now_ms.saturating_sub(watch.first_seen_ms) < MEMORY_STARTUP_GRACE_MS {
        return MemoryAction::Nothing;
    }
    if memory <= limit {
        watch.trimmed_at_ms = None;
        return MemoryAction::Nothing;
    }
    if in_use {
        return MemoryAction::Nothing;
    }
    match watch.trimmed_at_ms {
        None => {
            watch.trimmed_at_ms = Some(now_ms);
            MemoryAction::Trim
        }
        Some(at) if now_ms.saturating_sub(at) < MEMORY_TRIM_GRACE_MS => MemoryAction::Nothing,
        Some(_) if close_allowed => MemoryAction::Close,
        Some(_) => {
            // Sem a opção de fechar: pede de novo.
            watch.trimmed_at_ms = Some(now_ms);
            MemoryAction::Trim
        }
    }
}

/// O que a tela recebe de cada cliente (em `get_running_instances`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientMemoryView {
    pub memory_mb: Option<u64>,
    /// O limite que vale agora (`None` = sem limite).
    pub limit_mb: Option<u64>,
    /// Acima do limite nesta passada.
    pub over: bool,
    /// Quando o app pediu para liberar pela última vez, enquanto acima.
    pub trimmed_at_ms: Option<i64>,
}

/// Um cliente do app nesta passada.
#[derive(Debug, Clone)]
struct MemoryClient {
    user_id: i64,
    pid: u32,
    limit_mb: Option<u64>,
}

/// O que o monitor precisa do sistema (dublê nos testes).
trait MemoryOs {
    fn memory_mb(&self, pid: u32) -> Option<u64>;
    fn foreground_pid(&self) -> Option<u32>;
    fn trim(&self, pid: u32) -> bool;
    /// Fecha o cliente da conta (o mesmo `kill_for_user` do Watcher).
    fn close(&self, user_id: i64) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemoryNotice {
    Trimmed { user_id: i64, memory_mb: u64, limit_mb: u64, ok: bool },
    Closed { user_id: i64, memory_mb: u64, limit_mb: u64 },
}

#[derive(Default)]
struct MemoryCeilingMonitor {
    watches: HashMap<i64, MemoryWatch>,
    views: HashMap<i64, ClientMemoryView>,
}

impl MemoryCeilingMonitor {
    fn tick(
        &mut self,
        os: &impl MemoryOs,
        clients: &[MemoryClient],
        close_allowed: bool,
        now_ms: i64,
    ) -> Vec<MemoryNotice> {
        let present: HashSet<i64> = clients.iter().map(|c| c.user_id).collect();
        self.watches.retain(|uid, _| present.contains(uid));
        self.views.retain(|uid, _| present.contains(uid));
        let foreground = os.foreground_pid();
        let mut notices = Vec::new();
        for client in clients {
            let watch = self
                .watches
                .entry(client.user_id)
                .or_insert_with(|| MemoryWatch::new(client.pid, now_ms));
            if watch.pid != client.pid {
                *watch = MemoryWatch::new(client.pid, now_ms);
            }
            let memory = os.memory_mb(client.pid);
            let action = memory_ceiling_step(
                watch,
                now_ms,
                memory,
                client.limit_mb,
                foreground == Some(client.pid),
                close_allowed,
            );
            let (memory_mb, limit_mb) = (memory.unwrap_or(0), client.limit_mb.unwrap_or(0));
            match action {
                MemoryAction::Nothing => {}
                MemoryAction::Trim => {
                    let ok = os.trim(client.pid);
                    notices.push(MemoryNotice::Trimmed {
                        user_id: client.user_id,
                        memory_mb,
                        limit_mb,
                        ok,
                    });
                }
                MemoryAction::Close => {
                    if os.close(client.user_id) {
                        notices.push(MemoryNotice::Closed {
                            user_id: client.user_id,
                            memory_mb,
                            limit_mb,
                        });
                        self.watches.remove(&client.user_id);
                        self.views.remove(&client.user_id);
                        continue;
                    }
                }
            }
            let over = matches!((memory, client.limit_mb), (Some(m), Some(l)) if m > l);
            let trimmed_at_ms = self.watches.get(&client.user_id).and_then(|w| w.trimmed_at_ms);
            self.views.insert(
                client.user_id,
                ClientMemoryView {
                    memory_mb: memory,
                    limit_mb: client.limit_mb,
                    over,
                    trimmed_at_ms,
                },
            );
        }
        notices
    }
}

/// Linha do Console de um aviso (português, como as do Watcher).
fn memory_console_line(notice: &MemoryNotice) -> String {
    match notice {
        MemoryNotice::Trimmed { memory_mb, limit_mb, ok: true, .. } => format!(
            "Memória em {memory_mb} MB, acima do limite de {limit_mb} MB: pedi ao Windows para liberar"
        ),
        MemoryNotice::Trimmed { memory_mb, limit_mb, ok: false, .. } => format!(
            "Memória em {memory_mb} MB, acima do limite de {limit_mb} MB: o Windows recusou liberar"
        ),
        MemoryNotice::Closed { memory_mb, limit_mb, .. } => format!(
            "Cliente fechado: a memória continuou em {memory_mb} MB, acima do limite de {limit_mb} MB, depois de liberar"
        ),
    }
}

/// Último retrato, para o `get_running_instances`.
static CLIENT_MEMORY_VIEWS: LazyLock<Mutex<HashMap<i64, ClientMemoryView>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn client_memory_of(user_id: i64) -> Option<ClientMemoryView> {
    CLIENT_MEMORY_VIEWS.lock().ok()?.get(&user_id).cloned()
}

#[cfg(target_os = "windows")]
struct WindowsMemoryOs;

#[cfg(target_os = "windows")]
impl MemoryOs for WindowsMemoryOs {
    fn memory_mb(&self, pid: u32) -> Option<u64> {
        platform::windows::get_process_memory_mb(pid)
    }
    fn foreground_pid(&self) -> Option<u32> {
        platform::windows::window_pid(platform::windows::get_foreground_hwnd())
    }
    fn trim(&self, pid: u32) -> bool {
        platform::windows::trim_working_set(pid)
    }
    fn close(&self, user_id: i64) -> bool {
        platform::windows::tracker().kill_for_user(user_id)
    }
}

#[cfg(target_os = "windows")]
static MEMORY_CEILING_MONITOR: LazyLock<Mutex<MemoryCeilingMonitor>> =
    LazyLock::new(|| Mutex::new(MemoryCeilingMonitor::default()));

/// Uma passada (no laço do monitor de quedas, a cada 2 s). Só clientes que o
/// app abriu; sem cliente, só lê o tracker.
#[cfg(target_os = "windows")]
fn memory_ceiling_pass(app: &tauri::AppHandle) -> Vec<MemoryNotice> {
    if !memory_trim_supported() {
        return Vec::new();
    }
    let launched: Vec<(i64, u32)> = platform::windows::tracker()
        .get_all()
        .into_iter()
        .filter(|process| !process.adopted)
        .map(|process| (process.user_id, process.pid))
        .collect();
    let settings = app.state::<SettingsStore>();
    let default_raw = settings.get_string("Optimization", "MemoryLimit");
    let close_allowed =
        settings.get_bool("Watcher", "Enabled") && settings.get_bool("Watcher", "CloseRbxMemory");
    let fields: HashMap<i64, String> = if launched.is_empty() {
        HashMap::new()
    } else {
        app.state::<AccountStore>()
            .get_all()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| a.fields.get("MemoryLimit").map(|v| (a.user_id, v.clone())))
            .collect()
    };
    let clients: Vec<MemoryClient> = launched
        .into_iter()
        .map(|(user_id, pid)| MemoryClient {
            user_id,
            pid,
            limit_mb: effective_memory_limit(fields.get(&user_id).map(String::as_str), &default_raw),
        })
        .collect();
    let Ok(mut monitor) = MEMORY_CEILING_MONITOR.lock() else {
        return Vec::new();
    };
    let notices = monitor.tick(&WindowsMemoryOs, &clients, close_allowed, now_ms());
    if let Ok(mut views) = CLIENT_MEMORY_VIEWS.lock() {
        *views = monitor.views.clone();
    }
    notices
}

#[cfg(target_os = "windows")]
pub(crate) async fn memory_ceiling_tick(app: &tauri::AppHandle) {
    let handle = app.clone();
    let notices = tokio::task::spawn_blocking(move || memory_ceiling_pass(&handle))
        .await
        .unwrap_or_default();
    for notice in &notices {
        match notice {
            MemoryNotice::Trimmed { user_id, .. } => {
                emit_launch_log(app, *user_id, "info", "memory", memory_console_line(notice));
            }
            MemoryNotice::Closed { user_id, memory_mb, limit_mb } => {
                let _ = app.emit(
                    "roblox-memory-limit",
                    serde_json::json!({
                        "userId": user_id,
                        "memoryMb": memory_mb,
                        "limitMb": limit_mb,
                    }),
                );
                emit_launch_log(app, *user_id, "warn", "memory", memory_console_line(notice));
            }
        }
    }
}

#[cfg(test)]
mod memory_ceiling_tests {
    use super::*;
    use std::cell::RefCell;

    const START: i64 = 1_000_000;
    const AFTER_GRACE: i64 = START + MEMORY_STARTUP_GRACE_MS;

    #[test]
    fn the_accounts_own_limit_wins_over_the_default() {
        assert_eq!(effective_memory_limit(Some("2048"), "1024"), Some(2048));
        assert_eq!(effective_memory_limit(None, "1024"), Some(1024));
        assert_eq!(effective_memory_limit(Some(""), "1536"), Some(1536), "empty field follows the default");
    }

    #[test]
    fn zero_or_off_means_no_limit_even_with_a_default() {
        assert_eq!(effective_memory_limit(Some("0"), "1024"), None);
        assert_eq!(effective_memory_limit(Some("off"), "1024"), None);
        assert_eq!(effective_memory_limit(None, "0"), None);
        assert_eq!(effective_memory_limit(None, ""), None, "off by default");
        assert_eq!(effective_memory_limit(Some("garbage"), "abc"), None);
    }

    #[test]
    fn a_limit_is_kept_between_256_mb_and_64_gb() {
        assert_eq!(effective_memory_limit(Some("10"), ""), Some(MEMORY_LIMIT_MIN_MB));
        assert_eq!(effective_memory_limit(Some("999999"), ""), Some(MEMORY_LIMIT_MAX_MB));
    }

    #[test]
    fn over_the_limit_it_frees_memory_first() {
        let mut watch = MemoryWatch::new(7, START);
        assert_eq!(
            memory_ceiling_step(&mut watch, AFTER_GRACE, Some(2500), Some(2048), false, true),
            MemoryAction::Trim
        );
    }

    #[test]
    fn a_client_that_just_opened_is_left_alone() {
        let mut watch = MemoryWatch::new(7, START);
        assert_eq!(
            memory_ceiling_step(&mut watch, START + 5_000, Some(4000), Some(2048), false, true),
            MemoryAction::Nothing
        );
    }

    #[test]
    fn it_closes_only_if_still_over_a_minute_later_and_closing_is_on() {
        let mut watch = MemoryWatch::new(7, START);
        let t0 = AFTER_GRACE;
        assert_eq!(memory_ceiling_step(&mut watch, t0, Some(2500), Some(2048), false, true), MemoryAction::Trim);
        // Dentro da carência depois de liberar: espera.
        assert_eq!(
            memory_ceiling_step(&mut watch, t0 + 30_000, Some(2500), Some(2048), false, true),
            MemoryAction::Nothing
        );
        assert_eq!(
            memory_ceiling_step(&mut watch, t0 + MEMORY_TRIM_GRACE_MS, Some(2500), Some(2048), false, true),
            MemoryAction::Close
        );
    }

    #[test]
    fn without_the_close_option_it_only_frees_memory_again() {
        let mut watch = MemoryWatch::new(7, START);
        let t0 = AFTER_GRACE;
        memory_ceiling_step(&mut watch, t0, Some(2500), Some(2048), false, false);
        assert_eq!(
            memory_ceiling_step(&mut watch, t0 + MEMORY_TRIM_GRACE_MS, Some(2500), Some(2048), false, false),
            MemoryAction::Trim
        );
    }

    #[test]
    fn going_back_under_the_limit_starts_over() {
        let mut watch = MemoryWatch::new(7, START);
        let t0 = AFTER_GRACE;
        memory_ceiling_step(&mut watch, t0, Some(2500), Some(2048), false, true);
        // Liberou e ficou abaixo: zera.
        assert_eq!(
            memory_ceiling_step(&mut watch, t0 + 2_000, Some(900), Some(2048), false, true),
            MemoryAction::Nothing
        );
        assert_eq!(watch.trimmed_at_ms, None);
        // Subiu de novo mais tarde: libera de novo, não fecha.
        assert_eq!(
            memory_ceiling_step(&mut watch, t0 + 600_000, Some(2500), Some(2048), false, true),
            MemoryAction::Trim
        );
    }

    #[test]
    fn the_window_in_use_is_never_touched() {
        let mut watch = MemoryWatch::new(7, START);
        assert_eq!(
            memory_ceiling_step(&mut watch, AFTER_GRACE, Some(4000), Some(2048), true, true),
            MemoryAction::Nothing
        );
        assert_eq!(watch.trimmed_at_ms, None);
    }

    #[test]
    fn without_a_limit_nothing_happens() {
        let mut watch = MemoryWatch::new(7, START);
        assert_eq!(
            memory_ceiling_step(&mut watch, AFTER_GRACE, Some(9000), None, false, true),
            MemoryAction::Nothing
        );
    }

    /// Dublê do sistema: memória por PID, quem está em uso, e o que foi pedido.
    #[derive(Default)]
    struct FakeOs {
        memory: HashMap<u32, u64>,
        foreground: Option<u32>,
        trimmed: RefCell<Vec<u32>>,
        closed: RefCell<Vec<i64>>,
    }

    impl MemoryOs for FakeOs {
        fn memory_mb(&self, pid: u32) -> Option<u64> {
            self.memory.get(&pid).copied()
        }
        fn foreground_pid(&self) -> Option<u32> {
            self.foreground
        }
        fn trim(&self, pid: u32) -> bool {
            self.trimmed.borrow_mut().push(pid);
            true
        }
        fn close(&self, user_id: i64) -> bool {
            self.closed.borrow_mut().push(user_id);
            true
        }
    }

    fn client(user_id: i64, pid: u32, limit: Option<u64>) -> MemoryClient {
        MemoryClient { user_id, pid, limit_mb: limit }
    }

    #[test]
    fn the_monitor_frees_then_closes_only_the_client_over_its_limit() {
        let os = FakeOs {
            memory: HashMap::from([(10, 2500), (20, 900)]),
            ..Default::default()
        };
        let clients = [client(1, 10, Some(2048)), client(2, 20, Some(2048))];
        let mut monitor = MemoryCeilingMonitor::default();
        assert!(monitor.tick(&os, &clients, true, START).is_empty());
        let notices = monitor.tick(&os, &clients, true, AFTER_GRACE);
        assert_eq!(
            notices,
            vec![MemoryNotice::Trimmed { user_id: 1, memory_mb: 2500, limit_mb: 2048, ok: true }]
        );
        assert_eq!(*os.trimmed.borrow(), vec![10]);
        let view = monitor.views.get(&1).cloned().unwrap();
        assert!(view.over);
        assert_eq!(view.memory_mb, Some(2500));
        assert_eq!(view.trimmed_at_ms, Some(AFTER_GRACE));
        assert!(!monitor.views[&2].over);

        let notices = monitor.tick(&os, &clients, true, AFTER_GRACE + MEMORY_TRIM_GRACE_MS);
        assert_eq!(
            notices,
            vec![MemoryNotice::Closed { user_id: 1, memory_mb: 2500, limit_mb: 2048 }]
        );
        assert_eq!(*os.closed.borrow(), vec![1]);
        assert!(!monitor.views.contains_key(&1));
    }

    #[test]
    fn the_monitor_skips_the_window_in_use_and_clients_that_left() {
        let os = FakeOs {
            memory: HashMap::from([(10, 4000)]),
            foreground: Some(10),
            ..Default::default()
        };
        let mut monitor = MemoryCeilingMonitor::default();
        monitor.tick(&os, &[client(1, 10, Some(1024))], true, START);
        assert!(monitor.tick(&os, &[client(1, 10, Some(1024))], true, AFTER_GRACE).is_empty());
        assert!(os.trimmed.borrow().is_empty());
        // A conta fechou: sai do retrato.
        monitor.tick(&os, &[], true, AFTER_GRACE + 2_000);
        assert!(monitor.views.is_empty());
    }

    #[test]
    fn a_new_client_of_the_same_account_gets_a_new_grace() {
        let os = FakeOs {
            memory: HashMap::from([(10, 4000), (11, 4000)]),
            ..Default::default()
        };
        let mut monitor = MemoryCeilingMonitor::default();
        monitor.tick(&os, &[client(1, 10, Some(1024))], true, START);
        assert!(monitor.tick(&os, &[client(1, 11, Some(1024))], true, AFTER_GRACE).is_empty());
    }

    #[test]
    fn the_console_line_says_what_happened() {
        assert_eq!(
            memory_console_line(&MemoryNotice::Trimmed { user_id: 1, memory_mb: 2500, limit_mb: 2048, ok: true }),
            "Memória em 2500 MB, acima do limite de 2048 MB: pedi ao Windows para liberar"
        );
        assert!(memory_console_line(&MemoryNotice::Closed { user_id: 1, memory_mb: 2500, limit_mb: 2048 })
            .starts_with("Cliente fechado"));
    }

    #[test]
    fn the_view_reaches_the_screen_in_camel_case() {
        let view = ClientMemoryView {
            memory_mb: Some(1200),
            limit_mb: Some(2048),
            over: false,
            trimmed_at_ms: None,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["memoryMb"], 1200);
        assert_eq!(json["limitMb"], 2048);
        assert_eq!(json["over"], false);
        assert!(json["trimmedAtMs"].is_null());
    }

    #[test]
    fn the_memory_ceiling_follows_the_build_feature() {
        assert_eq!(
            memory_trim_supported(),
            cfg!(all(target_os = "windows", feature = "memory-trim"))
        );
    }
}
