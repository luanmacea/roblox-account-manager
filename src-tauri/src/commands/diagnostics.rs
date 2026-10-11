#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MutexDiagnosis {
    holder: &'static str,
    roblox_pids: Vec<u32>,
    legacy_ram_pids: Vec<u32>,
    this_process_holds: bool,
}

#[tauri::command]
fn kill_legacy_ram_processes() -> Result<u32, String> {
    #[cfg(target_os = "windows")]
    {
        let pids = platform::windows::find_legacy_ram_pids();
        let mut killed = 0u32;
        for pid in pids {
            if platform::windows::kill_process(pid).is_ok() {
                killed += 1;
            }
        }
        if killed > 0 {
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        Ok(killed)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(0)
    }
}

/// Who owns the Roblox singleton mutex, in priority order: this app first (it
/// holds the handle itself), then a running client, then the legacy manager.
fn mutex_holder_label(
    this_process_holds: bool,
    roblox_pids: &[u32],
    legacy_ram_pids: &[u32],
) -> &'static str {
    if this_process_holds {
        "thisProcess"
    } else if !roblox_pids.is_empty() {
        "roblox"
    } else if !legacy_ram_pids.is_empty() {
        "legacyRam"
    } else {
        "free"
    }
}

#[tauri::command]
fn diagnose_mutex_holder() -> Result<MutexDiagnosis, String> {
    #[cfg(target_os = "windows")]
    {
        let roblox_pids = platform::windows::get_roblox_pids();
        let legacy_ram_pids = platform::windows::find_legacy_ram_pids();
        let this_process_holds = platform::windows::this_process_holds_multi_roblox();

        let holder = mutex_holder_label(this_process_holds, &roblox_pids, &legacy_ram_pids);

        Ok(MutexDiagnosis {
            holder,
            roblox_pids,
            legacy_ram_pids,
            this_process_holds,
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(MutexDiagnosis {
            holder: "free",
            roblox_pids: Vec::new(),
            legacy_ram_pids: Vec::new(),
            this_process_holds: false,
        })
    }
}

// ── Diagnóstico "o launch não faz nada" (ideia 16) ─────────────────────────
//
// Uma lista de checagens para quem relata "clico e nada acontece". **Só lê**:
// nunca fecha cliente, nunca mexe em registro, nunca baixa build. A única
// escrita é o arquivo de prova da checagem de pasta, criado e apagado na hora.
//
// O backend devolve só `id` + `reason` (+ um número quando faz sentido); a
// frase que a pessoa lê sai do frontend (`src/utils/diagnostics.ts`), traduzida.
// Assim nada de caminho, nome de conta ou PID vai para a tela — e o resumo do
// "Reportar problema" (ideia 28) pode levar o resultado sem anonimizar nada.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum CheckStatus {
    Ok,
    Warn,
    Problem,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticCheck {
    id: &'static str,
    status: CheckStatus,
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<u32>,
}

impl DiagnosticCheck {
    fn new(id: &'static str, status: CheckStatus, reason: &'static str) -> Self {
        Self { id, status, reason, count: None }
    }

    fn with_count(mut self, count: u32) -> Self {
        self.count = Some(count);
        self
    }
}

/// Um processo do Roblox sem janela há mais que isto conta como "preso": um
/// cliente subindo leva alguns segundos para mostrar a janela; 150 s é o limite
/// que o RobloxKeeper (ideia 16) usa, folgado para máquina lenta.
const STUCK_PROCESS_MIN_AGE_SECS: u64 = 150;

/// Hosts consultados na checagem de internet: pedidos sem conta, pequenos.
/// Qualquer resposta HTTP (até 404) prova que o Roblox é alcançável.
const REACHABILITY_PROBES: &[(&str, &str)] = &[("users", "/v1/users/1"), ("auth", "/v2/metadata")];

fn install_check(build_found: bool) -> DiagnosticCheck {
    if build_found {
        DiagnosticCheck::new("robloxInstall", CheckStatus::Ok, "found")
    } else {
        // Não é defeito por si: o launch baixa a build de produção sozinho.
        // Vira problema quando a internet também falha — a frase diz isso.
        DiagnosticCheck::new("robloxInstall", CheckStatus::Warn, "missing")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderProbe {
    Writable,
    /// A pasta ainda não existe, mas a mais próxima que existe aceita escrita
    /// (o app a cria quando precisar).
    CanBeCreated,
    NotWritable,
    /// Nem dá para saber onde fica (variável de ambiente faltando).
    Unknown,
}

fn folder_check(id: &'static str, probe: FolderProbe) -> DiagnosticCheck {
    match probe {
        FolderProbe::Writable | FolderProbe::CanBeCreated => DiagnosticCheck::new(id, CheckStatus::Ok, "writable"),
        FolderProbe::NotWritable => DiagnosticCheck::new(id, CheckStatus::Problem, "notWritable"),
        FolderProbe::Unknown => DiagnosticCheck::new(id, CheckStatus::Warn, "unknown"),
    }
}

/// Tenta criar e apagar um arquivo de prova em `dir` (ou na pasta mais
/// próxima que existir acima dela). Não cria a pasta: se ela não existe, só
/// diz se daria para criar.
fn probe_folder_writable(dir: &std::path::Path) -> FolderProbe {
    let mut target = dir.to_path_buf();
    let mut exists = target.is_dir();
    while !exists {
        match target.parent() {
            Some(parent) if parent != target.as_path() => {
                target = parent.to_path_buf();
                exists = target.is_dir();
            }
            _ => return FolderProbe::NotWritable,
        }
    }
    let probe = target.join(format!(".multialt-write-check-{}.tmp", std::process::id()));
    let writable = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    match (writable, target.as_path() == dir) {
        (false, _) => FolderProbe::NotWritable,
        (true, true) => FolderProbe::Writable,
        (true, false) => FolderProbe::CanBeCreated,
    }
}

fn internet_check(reached: usize, total: usize) -> DiagnosticCheck {
    if total == 0 || reached == total {
        DiagnosticCheck::new("internet", CheckStatus::Ok, "reachable")
    } else if reached == 0 {
        DiagnosticCheck::new("internet", CheckStatus::Problem, "unreachable")
    } else {
        DiagnosticCheck::new("internet", CheckStatus::Warn, "partial")
    }
}

/// Quantos hosts do Roblox responderam (com qualquer status HTTP). Timeout,
/// DNS ou TLS quebrado contam como "não alcançou".
async fn probe_roblox_hosts() -> (usize, usize) {
    let client = match api::http_client::builder_with(
        std::time::Duration::from_secs(6),
        std::time::Duration::from_secs(10),
    )
    .build()
    {
        Ok(client) => client,
        Err(_) => return (0, REACHABILITY_PROBES.len()),
    };
    let mut reached = 0;
    for (sub, path) in REACHABILITY_PROBES {
        let url = format!("{}{}", api::endpoints::host(sub), path);
        if client.get(url).send().await.is_ok() {
            reached += 1;
        }
    }
    (reached, REACHABILITY_PROBES.len())
}

/// Um processo do Roblox visto pela checagem de presos.
#[derive(Debug, Clone, Copy)]
struct RobloxProcessView {
    has_window: bool,
    /// Há quanto tempo o processo existe; `None` = não deu para ler.
    age_secs: Option<u64>,
}

/// Quantos processos estão **sem janela há tempo demais**. Sem idade conhecida
/// o processo não entra: melhor não acusar um cliente que acabou de abrir.
fn count_stuck_processes(processes: &[RobloxProcessView], min_age_secs: u64) -> usize {
    processes
        .iter()
        .filter(|p| !p.has_window && p.age_secs.is_some_and(|age| age >= min_age_secs))
        .count()
}

fn stuck_processes_check(stuck: usize) -> DiagnosticCheck {
    if stuck == 0 {
        DiagnosticCheck::new("stuckProcesses", CheckStatus::Ok, "none")
    } else {
        DiagnosticCheck::new("stuckProcesses", CheckStatus::Warn, "stuck").with_count(stuck as u32)
    }
}

/// Estado do Multi Roblox para quem vai abrir mais uma conta.
fn multi_roblox_check(enabled: bool, holder: &str, roblox_running: bool) -> DiagnosticCheck {
    if !enabled {
        return if roblox_running {
            // Com um cliente aberto e o Multi Roblox desligado, a conta nova
            // derruba a que está aberta (ou não sobe) — o "não faz nada" clássico.
            DiagnosticCheck::new("multiRoblox", CheckStatus::Warn, "offWithClients")
        } else {
            DiagnosticCheck::new("multiRoblox", CheckStatus::Ok, "off")
        };
    }
    match holder {
        "legacyRam" => DiagnosticCheck::new("multiRoblox", CheckStatus::Problem, "legacyRam"),
        "thisProcess" => DiagnosticCheck::new("multiRoblox", CheckStatus::Ok, "held"),
        "roblox" => DiagnosticCheck::new("multiRoblox", CheckStatus::Ok, "clientOpen"),
        _ => DiagnosticCheck::new("multiRoblox", CheckStatus::Ok, "free"),
    }
}

/// A reserva experimental do nome do singleton (ideia 3). Só aparece com a
/// opção ligada: desligada, não há o que dizer.
fn singleton_reservation_check(enabled: bool, held: bool) -> Option<DiagnosticCheck> {
    if !enabled {
        return None;
    }
    Some(if held {
        DiagnosticCheck::new("singletonReservation", CheckStatus::Ok, "reserved")
    } else {
        // Ainda não houve launch com a opção ligada, ou um cliente segura o
        // Event e ele não pôde ser fechado. O método atual continua valendo.
        DiagnosticCheck::new("singletonReservation", CheckStatus::Warn, "notReserved")
    })
}

#[cfg(target_os = "windows")]
fn collect_roblox_process_views() -> Vec<RobloxProcessView> {
    use platform::windows::ExternalClientOs;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let os = platform::windows::WindowsExternalClientOs;
    platform::windows::get_roblox_pids()
        .into_iter()
        .map(|pid| RobloxProcessView {
            has_window: platform::windows::find_main_window(pid).is_some(),
            age_secs: os
                .process_created_ms(pid)
                .map(|created| (now_ms.saturating_sub(created).max(0) / 1000) as u64),
        })
        .collect()
}

/// Roda todas as checagens. Read-only: nunca fecha nada.
#[tauri::command]
async fn run_launch_diagnostics(
    settings: tauri::State<'_, SettingsStore>,
) -> Result<Vec<DiagnosticCheck>, String> {
    let mut checks = Vec::new();

    #[cfg(target_os = "windows")]
    checks.push(install_check(platform::windows::get_roblox_path().is_ok()));

    checks.push(folder_check(
        "dataFolder",
        probe_folder_writable(&data::settings::get_runtime_data_dir()),
    ));
    checks.push(folder_check(
        "versionsFolder",
        match data::versions::ram_managed_versions_root() {
            Some(dir) => probe_folder_writable(&dir),
            None => FolderProbe::Unknown,
        },
    ));

    let (reached, total) = probe_roblox_hosts().await;
    checks.push(internet_check(reached, total));

    #[cfg(target_os = "windows")]
    {
        let views = tauri::async_runtime::spawn_blocking(collect_roblox_process_views)
            .await
            .unwrap_or_default();
        checks.push(stuck_processes_check(count_stuck_processes(
            &views,
            STUCK_PROCESS_MIN_AGE_SECS,
        )));

        let roblox_pids = platform::windows::get_roblox_pids();
        let legacy_ram_pids = platform::windows::find_legacy_ram_pids();
        let holder = mutex_holder_label(
            platform::windows::this_process_holds_multi_roblox(),
            &roblox_pids,
            &legacy_ram_pids,
        );
        checks.push(multi_roblox_check(
            settings.get_bool("General", "EnableMultiRbx"),
            holder,
            !roblox_pids.is_empty(),
        ));
        if let Some(check) = singleton_reservation_check(
            settings.get_bool("General", "EnableMultiRbx")
                && settings.get_bool("General", "ReserveSingletonEvent"),
            platform::windows::singleton_reservation_held(),
        ) {
            checks.push(check);
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = &settings;

    Ok(checks)
}

#[cfg(test)]
mod launch_diagnostics_tests {
    use super::*;

    #[test]
    fn a_missing_build_is_a_warning_because_the_launch_downloads_it() {
        assert_eq!(install_check(true).status, CheckStatus::Ok);
        let missing = install_check(false);
        assert_eq!(missing.status, CheckStatus::Warn);
        assert_eq!(missing.reason, "missing");
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_a_problem() {
        assert_eq!(folder_check("dataFolder", FolderProbe::Writable).status, CheckStatus::Ok);
        assert_eq!(folder_check("dataFolder", FolderProbe::CanBeCreated).status, CheckStatus::Ok);
        let blocked = folder_check("versionsFolder", FolderProbe::NotWritable);
        assert_eq!(blocked.status, CheckStatus::Problem);
        assert_eq!(blocked.id, "versionsFolder");
        assert_eq!(folder_check("versionsFolder", FolderProbe::Unknown).status, CheckStatus::Warn);
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "multialt-diag-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn the_folder_probe_writes_and_leaves_nothing_behind() {
        let dir = temp_dir("writable");
        assert_eq!(probe_folder_writable(&dir), FolderProbe::Writable);
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert!(leftovers.is_empty(), "the probe file was not removed: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_does_not_exist_yet_is_not_created_by_the_probe() {
        let base = temp_dir("missing");
        let missing = base.join("not").join("there");
        assert_eq!(probe_folder_writable(&missing), FolderProbe::CanBeCreated);
        assert!(!missing.exists(), "the check must not create the folder");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn internet_is_judged_by_how_many_hosts_answered() {
        assert_eq!(internet_check(2, 2).status, CheckStatus::Ok);
        assert_eq!(internet_check(1, 2).status, CheckStatus::Warn);
        let down = internet_check(0, 2);
        assert_eq!(down.status, CheckStatus::Problem);
        assert_eq!(down.reason, "unreachable");
    }

    #[tokio::test]
    async fn the_internet_probe_counts_any_http_answer_as_reachable() {
        // The shared mock answers unmatched paths with 404: still an answer.
        let _server = api::endpoints::test_support::mock_server().await;
        let (reached, total) = probe_roblox_hosts().await;
        assert_eq!(total, REACHABILITY_PROBES.len());
        assert_eq!(reached, total);
    }

    #[test]
    fn the_internet_probe_only_uses_roblox_hosts_from_endpoints() {
        for (sub, path) in REACHABILITY_PROBES {
            assert!(!sub.contains('.'), "{sub} must be a subdomain label for endpoints::host");
            assert!(path.starts_with('/'));
        }
    }

    #[test]
    fn only_old_windowless_processes_count_as_stuck() {
        let views = [
            RobloxProcessView { has_window: true, age_secs: Some(9_999) },
            // Acabou de abrir: ainda carregando, não é preso.
            RobloxProcessView { has_window: false, age_secs: Some(20) },
            RobloxProcessView { has_window: false, age_secs: Some(STUCK_PROCESS_MIN_AGE_SECS) },
            RobloxProcessView { has_window: false, age_secs: Some(3_600) },
            // Idade desconhecida: não acusa.
            RobloxProcessView { has_window: false, age_secs: None },
        ];
        assert_eq!(count_stuck_processes(&views, STUCK_PROCESS_MIN_AGE_SECS), 2);
        assert_eq!(count_stuck_processes(&[], STUCK_PROCESS_MIN_AGE_SECS), 0);
    }

    #[test]
    fn stuck_processes_are_a_warning_with_the_count() {
        assert_eq!(stuck_processes_check(0).status, CheckStatus::Ok);
        let stuck = stuck_processes_check(3);
        assert_eq!(stuck.status, CheckStatus::Warn);
        assert_eq!(stuck.count, Some(3));
    }

    #[test]
    fn multi_roblox_off_only_warns_when_a_client_is_open() {
        assert_eq!(multi_roblox_check(false, "free", false).status, CheckStatus::Ok);
        let warn = multi_roblox_check(false, "roblox", true);
        assert_eq!(warn.status, CheckStatus::Warn);
        assert_eq!(warn.reason, "offWithClients");
    }

    #[test]
    fn the_legacy_manager_holding_the_lock_is_the_only_multi_roblox_problem() {
        assert_eq!(multi_roblox_check(true, "legacyRam", false).status, CheckStatus::Problem);
        for holder in ["thisProcess", "roblox", "free"] {
            assert_eq!(multi_roblox_check(true, holder, true).status, CheckStatus::Ok, "{holder}");
        }
    }

    #[test]
    fn the_experimental_reservation_only_shows_up_when_it_is_on() {
        assert_eq!(singleton_reservation_check(false, false), None);
        assert_eq!(singleton_reservation_check(false, true), None);
        assert_eq!(singleton_reservation_check(true, true).unwrap().status, CheckStatus::Ok);
        let waiting = singleton_reservation_check(true, false).unwrap();
        assert_eq!(waiting.status, CheckStatus::Warn);
        assert_eq!(waiting.reason, "notReserved");
    }

    #[test]
    fn a_check_serializes_with_the_keys_the_ui_reads() {
        let json = serde_json::to_value(stuck_processes_check(2)).unwrap();
        assert_eq!(json["id"], "stuckProcesses");
        assert_eq!(json["status"], "warn");
        assert_eq!(json["reason"], "stuck");
        assert_eq!(json["count"], 2);
        let without = serde_json::to_value(internet_check(1, 1)).unwrap();
        assert!(without.get("count").is_none(), "count only when it means something");
    }

    #[test]
    fn the_diagnostics_never_close_or_kill_anything() {
        // Read-only é contrato: a checagem não pode alcançar kill/terminate.
        let source = include_str!("diagnostics.rs");
        let body = source
            .split("// ── Diagnóstico \"o launch não faz nada\"")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("diagnostics section");
        for forbidden in ["kill_process", "kill_all_roblox", "TerminateProcess", "close_roblox_singleton_handles"] {
            assert!(!body.contains(forbidden), "diagnostics must not call {forbidden}");
        }
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn mutex_holder_label_reports_free_when_nothing_holds_it() {
        assert_eq!(mutex_holder_label(false, &[], &[]), "free");
    }

    #[test]
    fn mutex_holder_label_reports_this_process_first() {
        // This app holding the handle wins over every other signal, otherwise
        // the UI would tell the user to close a client that is not the blocker.
        assert_eq!(mutex_holder_label(true, &[], &[]), "thisProcess");
        assert_eq!(mutex_holder_label(true, &[1234], &[5678]), "thisProcess");
    }

    #[test]
    fn mutex_holder_label_prefers_a_running_client_over_the_legacy_manager() {
        assert_eq!(mutex_holder_label(false, &[1234], &[5678]), "roblox");
        assert_eq!(mutex_holder_label(false, &[1234], &[]), "roblox");
    }

    #[test]
    fn mutex_holder_label_falls_back_to_the_legacy_manager() {
        assert_eq!(mutex_holder_label(false, &[], &[5678]), "legacyRam");
    }

    #[test]
    fn mutex_holder_label_treats_many_pids_the_same_as_one() {
        let many: Vec<u32> = (1..=500).collect();
        assert_eq!(mutex_holder_label(false, &many, &[]), "roblox");
        assert_eq!(mutex_holder_label(false, &[], &many), "legacyRam");
    }

    #[test]
    fn mutex_diagnosis_serializes_with_the_camel_case_keys_the_ui_reads() {
        let diagnosis = MutexDiagnosis {
            holder: "legacyRam",
            roblox_pids: vec![1, 2],
            legacy_ram_pids: vec![3],
            this_process_holds: false,
        };
        let json = serde_json::to_value(&diagnosis).unwrap();
        assert_eq!(json["holder"], "legacyRam");
        assert_eq!(json["robloxPids"], serde_json::json!([1, 2]));
        assert_eq!(json["legacyRamPids"], serde_json::json!([3]));
        assert_eq!(json["thisProcessHolds"], false);
    }

    #[test]
    fn diagnose_mutex_holder_is_self_consistent_on_this_machine() {
        // Read-only probe: whatever the machine state, the reported holder must
        // match the pid lists in the same payload.
        let diagnosis = diagnose_mutex_holder().expect("diagnosis should succeed");
        assert_eq!(
            diagnosis.holder,
            mutex_holder_label(
                diagnosis.this_process_holds,
                &diagnosis.roblox_pids,
                &diagnosis.legacy_ram_pids,
            )
        );
    }
}
