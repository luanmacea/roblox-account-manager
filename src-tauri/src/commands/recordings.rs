// Gravações — sequências de teclas, cliques e esperas que o app toca na janela
// do Roblox de uma conta, uma janela por vez. Ver docs/features/recordings.md.
//
// O cliente do Roblox só aceita entrada na janela em primeiro plano, então
// tocar uma gravação é o mesmo ciclo do Modo AFK (`run_afk_cycle_blocking`):
// traz a janela da conta, confere que ela chegou à frente, toca, devolve o foco
// no fim. Quem toca:
//
// - o **Modo AFK** no modo "gravação", com o intervalo dele;
// - o botão **Tocar agora** da aba Recordings, nas contas que o usuário marcou;
// - a **reconexão automática**: a conta que ela devolveu ao jogo, depois de
//   ficar o tempo configurado no jogo (`Recordings.AfterReconnect`).
//
// Este arquivo só **toca** o que o usuário escreveu no editor: a tecla e o
// clique saem pelas portas do módulo de entrada (`press_recording_key`,
// `click_recording_point`), e ler teclado ou mouse aqui é proibido como em todo
// o resto do Modo AFK — a trava é o `afk_input_safety_tests`.

use data::recordings::{recording_for_account, Recording, RecordingStep, RecordingsFile};

/// Quanto o botão fica pressionado num clique de gravação (o mesmo do AFK).
#[cfg(target_os = "windows")]
const RECORDING_CLICK_HOLD_MS: u64 = 40;
/// De quanto em quanto tempo uma espera olha o "parar": parar interrompe na
/// hora, inclusive no meio de uma espera de 10 minutos.
const RECORDING_WAIT_SLICE_MS: u64 = 25;

/// O que a gravação faz, já sem o "toque com tempo": `Key` vira apertar,
/// esperar, soltar.
#[derive(Debug, Clone, PartialEq)]
enum RecordingAction {
    Press(String),
    Release(String),
    Click(AfkPoint),
    Wait(u64),
}

fn recording_actions(steps: &[RecordingStep]) -> Vec<RecordingAction> {
    let mut out = Vec::with_capacity(steps.len() * 2);
    for step in steps {
        match step {
            RecordingStep::Key { key, hold_ms } => {
                out.push(RecordingAction::Press(key.clone()));
                out.push(RecordingAction::Wait(*hold_ms));
                out.push(RecordingAction::Release(key.clone()));
            }
            RecordingStep::KeyDown { key } => out.push(RecordingAction::Press(key.clone())),
            RecordingStep::KeyUp { key } => out.push(RecordingAction::Release(key.clone())),
            RecordingStep::Click { x_pct, y_pct } => {
                out.push(RecordingAction::Click(AfkPoint::clamped(*x_pct, *y_pct)))
            }
            RecordingStep::Wait { ms } => out.push(RecordingAction::Wait(*ms)),
        }
    }
    out
}

/// Por que uma gravação parou no meio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingPlayError {
    /// O usuário mandou parar.
    Stopped,
    /// Outra janela veio para frente: o resto cairia nela.
    FocusLost,
    KeyRefused,
    ClickRefused,
}

/// O erro da gravação nos termos do ciclo do Modo AFK (o status por conta).
fn afk_error_from_playback(error: RecordingPlayError) -> AfkSendError {
    match error {
        RecordingPlayError::Stopped => AfkSendError::Stopped,
        RecordingPlayError::FocusLost => AfkSendError::FocusLost,
        RecordingPlayError::KeyRefused => AfkSendError::KeyRefused,
        RecordingPlayError::ClickRefused => AfkSendError::ClickRefused,
    }
}

/// O que a reprodução usa do Windows — trocável nos testes.
trait RecordingDesk {
    fn stopped(&self) -> bool;
    /// A janela da conta continua em primeiro plano?
    fn target_in_front(&self) -> bool;
    /// Aperta (`up = false`) ou solta uma tecla da lista.
    fn key(&mut self, key: &str, up: bool) -> bool;
    fn click(&mut self, point: AfkPoint, focus_click: bool) -> bool;
    fn now_ms(&self) -> u64;
    fn sleep_ms(&mut self, ms: u64);
}

/// Toca as ações. Regras:
///
/// - antes de **cada** tecla apertada e de cada clique, confere que a janela da
///   conta continua na frente; se não, para (`FocusLost`) — o resto cairia na
///   janela em que o usuário acabou de clicar;
/// - espera em fatias de 25 ms, olhando o "parar" a cada fatia, com prazo
///   próprio (a espera não acumula o atraso das fatias);
/// - o primeiro clique leva o clique de foco da receita do AFK; os outros não;
/// - **no fim, sempre**, solta toda tecla que ficou apertada — parada, erro,
///   foco perdido ou `keyDown` sem `keyUp` na gravação. Tecla presa faz o
///   personagem andar sozinho.
fn play_recording_with(
    desk: &mut impl RecordingDesk,
    actions: &[RecordingAction],
) -> Result<(), RecordingPlayError> {
    let mut held: Vec<String> = Vec::new();
    let result = play_recording_actions(desk, actions, &mut held);
    for key in held.iter().rev() {
        desk.key(key, true);
    }
    result
}

fn play_recording_actions(
    desk: &mut impl RecordingDesk,
    actions: &[RecordingAction],
    held: &mut Vec<String>,
) -> Result<(), RecordingPlayError> {
    let mut first_click = true;
    for action in actions {
        if desk.stopped() {
            return Err(RecordingPlayError::Stopped);
        }
        match action {
            RecordingAction::Wait(ms) => {
                let deadline = desk.now_ms().saturating_add(*ms);
                loop {
                    if desk.stopped() {
                        return Err(RecordingPlayError::Stopped);
                    }
                    let now = desk.now_ms();
                    if now >= deadline {
                        break;
                    }
                    desk.sleep_ms((deadline - now).min(RECORDING_WAIT_SLICE_MS));
                }
            }
            RecordingAction::Press(key) => {
                if held.iter().any(|k| k == key) {
                    continue;
                }
                if !desk.target_in_front() {
                    return Err(RecordingPlayError::FocusLost);
                }
                if !desk.key(key, false) {
                    return Err(RecordingPlayError::KeyRefused);
                }
                held.push(key.clone());
            }
            RecordingAction::Release(key) => {
                // Só solta o que esta reprodução apertou.
                let Some(pos) = held.iter().position(|k| k == key) else {
                    continue;
                };
                held.remove(pos);
                if !desk.key(key, true) {
                    return Err(RecordingPlayError::KeyRefused);
                }
            }
            RecordingAction::Click(point) => {
                if !desk.target_in_front() {
                    return Err(RecordingPlayError::FocusLost);
                }
                if !desk.click(*point, first_click) {
                    return Err(RecordingPlayError::ClickRefused);
                }
                first_click = false;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
struct WindowRecordingDesk<'a> {
    hwnd: windows_sys::Win32::Foundation::HWND,
    stop: &'a AtomicBool,
    started: std::time::Instant,
}

#[cfg(target_os = "windows")]
impl RecordingDesk for WindowRecordingDesk<'_> {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    fn target_in_front(&self) -> bool {
        platform::windows::get_foreground_hwnd() == self.hwnd
    }
    fn key(&mut self, key: &str, up: bool) -> bool {
        platform::windows::press_recording_key(key, up)
    }
    fn click(&mut self, point: AfkPoint, focus_click: bool) -> bool {
        platform::windows::click_recording_point(self.hwnd, point, RECORDING_CLICK_HOLD_MS, focus_click)
            .is_ok()
    }
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
    fn sleep_ms(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// Toca a gravação na janela `hwnd`, que o ciclo **já confirmou** estar em
/// primeiro plano.
#[cfg(target_os = "windows")]
fn play_recording_in_window(
    hwnd: windows_sys::Win32::Foundation::HWND,
    steps: &[RecordingStep],
    stop: &AtomicBool,
) -> Result<(), RecordingPlayError> {
    let mut desk = WindowRecordingDesk {
        hwnd,
        stop,
        started: std::time::Instant::now(),
    };
    play_recording_with(&mut desk, &recording_actions(steps))
}

/// Os passos da gravação de cada conta que tem uma (a própria ou a de todas).
/// Gravação vazia não conta: tocá-la só roubaria o foco.
fn recording_plans_for(file: &RecordingsFile, user_ids: &[i64]) -> HashMap<i64, Vec<RecordingStep>> {
    user_ids
        .iter()
        .filter_map(|uid| {
            recording_for_account(file, *uid)
                .filter(|r| !r.steps.is_empty())
                .map(|r| (*uid, r.steps.clone()))
        })
        .collect()
}

/// A mesma gravação para todas as contas pedidas (o "Tocar agora" com uma
/// gravação escolhida no editor).
fn recording_plans_with(recording: &Recording, user_ids: &[i64]) -> HashMap<i64, Vec<RecordingStep>> {
    if recording.steps.is_empty() {
        return HashMap::new();
    }
    user_ids.iter().map(|uid| (*uid, recording.steps.clone())).collect()
}

// ── depois da reconexão ────────────────────────────────────────────────────

/// Conta que a reconexão automática acabou de relançar, esperando ficar o tempo
/// configurado no jogo para a gravação tocar.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AfterReconnectEntry {
    armed_at_ms: i64,
    pid: Option<u32>,
    in_game_since_ms: Option<i64>,
}

/// O que o laço vê da conta numa passada (dublê nos testes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AfterReconnectObservation {
    tracked_pid: Option<u32>,
    adopted: bool,
    in_game: bool,
}

/// Desiste de esperar a conta entrar no jogo depois disto.
const AFTER_RECONNECT_GIVE_UP_MS: i64 = 15 * 60_000;

#[derive(Debug, Default)]
struct AfterReconnectBook {
    entries: HashMap<i64, AfterReconnectEntry>,
}

impl AfterReconnectBook {
    /// A reconexão relançou a conta.
    fn arm(&mut self, user_id: i64, now_ms: i64) {
        self.entries.insert(
            user_id,
            AfterReconnectEntry {
                armed_at_ms: now_ms,
                pid: None,
                in_game_since_ms: None,
            },
        );
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Uma passada: devolve as contas cuja gravação deve tocar agora (e as tira
    /// da espera). Opção desligada esquece todo mundo.
    fn tick(
        &mut self,
        now_ms: i64,
        enabled: bool,
        delay_ms: i64,
        observe: impl Fn(i64) -> AfterReconnectObservation,
    ) -> Vec<i64> {
        if !enabled {
            self.entries.clear();
            return Vec::new();
        }
        let mut fire = Vec::new();
        let mut remove = Vec::new();
        let mut ids: Vec<i64> = self.entries.keys().copied().collect();
        ids.sort_unstable();
        for user_id in ids {
            let Some(entry) = self.entries.get_mut(&user_id) else {
                continue;
            };
            let obs = observe(user_id);
            // Cliente do site nunca recebe nada do app.
            if obs.adopted {
                remove.push(user_id);
                continue;
            }
            match obs.tracked_pid {
                None => entry.in_game_since_ms = None,
                Some(pid) => {
                    // Cliente novo (outra tentativa da reconexão): conta de novo.
                    if entry.pid != Some(pid) {
                        entry.pid = Some(pid);
                        entry.in_game_since_ms = None;
                    }
                    if obs.in_game {
                        entry.in_game_since_ms.get_or_insert(now_ms);
                    } else {
                        entry.in_game_since_ms = None;
                    }
                }
            }
            match entry.in_game_since_ms {
                Some(since) if now_ms - since >= delay_ms => {
                    fire.push(user_id);
                    remove.push(user_id);
                }
                None if now_ms - entry.armed_at_ms >= AFTER_RECONNECT_GIVE_UP_MS => {
                    remove.push(user_id);
                }
                _ => {}
            }
        }
        for user_id in remove {
            self.entries.remove(&user_id);
        }
        fire
    }
}

/// O tempo no jogo antes de tocar, em segundos (5 s a 1 h).
fn clamp_after_reconnect_delay_seconds(seconds: i64) -> i64 {
    seconds.clamp(5, 3_600)
}

static RECORDING_AFTER_RECONNECT: LazyLock<Mutex<AfterReconnectBook>> =
    LazyLock::new(|| Mutex::new(AfterReconnectBook::default()));

fn with_after_reconnect_book<R>(f: impl FnOnce(&mut AfterReconnectBook) -> R) -> R {
    let mut book = match RECORDING_AFTER_RECONNECT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut book)
}

/// A reconexão relançou a conta (`ReconnectNotice::Relaunched`): a gravação
/// dela toca quando ela ficar o tempo configurado no jogo.
pub(crate) fn arm_recording_after_reconnect(user_id: i64, now_ms: i64) {
    with_after_reconnect_book(|book| book.arm(user_id, now_ms));
}

/// Uma passada (a cada 2 s, com a da reconexão): toca a gravação de quem já
/// ficou o tempo no jogo.
#[cfg(target_os = "windows")]
pub(crate) fn recording_after_reconnect_tick(app: &tauri::AppHandle) {
    if with_after_reconnect_book(|book| book.is_empty()) {
        return;
    }
    let settings = app.state::<SettingsStore>();
    let enabled = settings.get_bool("Recordings", "AfterReconnect");
    let delay_s = clamp_after_reconnect_delay_seconds(
        settings
            .get_int("Recordings", "AfterReconnectDelaySeconds")
            .map(|v| v as i64)
            .unwrap_or(30),
    );
    let tracked = platform::windows::tracker().get_all();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let due = with_after_reconnect_book(|book| {
        book.tick(now_ms, enabled, delay_s * 1_000, |user_id| {
            let process = tracked.iter().find(|p| p.user_id == user_id);
            AfterReconnectObservation {
                tracked_pid: process.map(|p| p.pid),
                adopted: process.is_some_and(|p| p.adopted),
                in_game: process
                    .and_then(|p| client_health_of(user_id, p.pid))
                    .is_some_and(|v| v.in_game),
            }
        })
    });
    if due.is_empty() {
        return;
    }
    let file = app
        .state::<data::recordings::RecordingStore>()
        .load()
        .unwrap_or_default();
    for user_id in due {
        let plans = recording_plans_for(&file, &[user_id]);
        if plans.is_empty() {
            emit_launch_log(
                app,
                user_id,
                "info",
                "recording",
                "Voltou ao jogo, mas a conta não tem gravação para tocar",
            );
            continue;
        }
        let name = recording_for_account(&file, user_id)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            emit_launch_log(
                &app,
                user_id,
                "info",
                "recording",
                format!("Tocando a gravação \"{name}\" depois da reconexão"),
            );
            let outcome = run_recording_playback(&app, plans, vec![user_id]).await;
            let (level, line) = match outcome.first().map(|(_, e)| e.clone()) {
                Some(None) => ("success", format!("Gravação \"{name}\" tocada")),
                Some(Some(error)) => ("warn", format!("Gravação \"{name}\" não tocada: {}", error.message())),
                None => ("warn", format!("Gravação \"{name}\" não tocada")),
            };
            emit_launch_log(&app, user_id, level, "recording", line);
        });
    }
}

// ── tocar agora ────────────────────────────────────────────────────────────

/// O "parar" da reprodução avulsa (Tocar agora e depois da reconexão). O
/// Modo AFK tem o dele, o da sessão.
static RECORDING_PLAYBACK_STOP: AtomicBool = AtomicBool::new(false);
/// Quantas reproduções avulsas estão rodando (ou esperando a vez).
static RECORDING_PLAYBACKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingPlaybackState {
    active: bool,
}

fn current_recording_playback() -> RecordingPlaybackState {
    RecordingPlaybackState {
        active: RECORDING_PLAYBACKS.load(Ordering::SeqCst) > 0,
    }
}

/// Uma reprodução avulsa: o mesmo ciclo do Modo AFK, um por vez (o
/// `AFK_CYCLE_LOCK` serializa com o Modo AFK ligado), e o resultado por conta.
#[cfg(target_os = "windows")]
async fn run_recording_playback(
    app: &tauri::AppHandle,
    plans: HashMap<i64, Vec<RecordingStep>>,
    targets: Vec<i64>,
) -> Vec<(i64, Option<AfkSendError>)> {
    RECORDING_PLAYBACKS.fetch_add(1, Ordering::SeqCst);
    let _ = app.emit("recording-playback", current_recording_playback());
    let result = {
        let _guard = AFK_CYCLE_LOCK.lock().await;
        // A vez chegou: um "parar" de antes não vale para esta.
        RECORDING_PLAYBACK_STOP.store(false, Ordering::SeqCst);
        let action = AfkCycleAction::Recording(plans);
        let cycle_targets = targets.clone();
        tokio::task::spawn_blocking(move || {
            run_afk_cycle_blocking(&action, &cycle_targets, &RECORDING_PLAYBACK_STOP)
        })
        .await
        .unwrap_or_else(|e| Err(format!("Recording playback failed: {e}")))
    };
    RECORDING_PLAYBACKS.fetch_sub(1, Ordering::SeqCst);
    let _ = app.emit("recording-playback", current_recording_playback());
    match result {
        Ok(outcome) => outcome,
        Err(error) => targets
            .into_iter()
            .map(|uid| (uid, Some(AfkSendError::Internal(error.clone()))))
            .collect(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingPlayResult {
    user_id: i64,
    /// `None` = tocou inteira; senão o código do Modo AFK (`noWindow`,
    /// `focusDenied`, `noRecording`, `focusLost`, `stopped`, ...).
    error_code: Option<String>,
    error: Option<String>,
}

fn recording_play_results(outcome: &[(i64, Option<AfkSendError>)]) -> Vec<RecordingPlayResult> {
    outcome
        .iter()
        .map(|(user_id, error)| RecordingPlayResult {
            user_id: *user_id,
            error_code: error.as_ref().map(|e| e.code().to_string()),
            error: error.as_ref().map(|e| e.message()),
        })
        .collect()
}

/// Toca agora, uma janela por vez, nas contas pedidas: a gravação `recording_id`
/// em todas, ou (sem id) a de cada conta. Só alcança cliente que este app abriu
/// (é o tracker que liga conta a janela). Nada aqui fecha cliente.
#[cfg(target_os = "windows")]
#[tauri::command]
async fn play_recording_now(
    app: tauri::AppHandle,
    user_ids: Vec<i64>,
    recording_id: Option<String>,
) -> Result<Vec<RecordingPlayResult>, String> {
    let user_ids = dedupe_preserving_order(user_ids);
    if user_ids.is_empty() {
        return Err("Pick at least one account".into());
    }
    let file = app.state::<data::recordings::RecordingStore>().load()?;
    let plans = match recording_id {
        Some(id) => {
            let recording = file
                .recordings
                .iter()
                .find(|r| r.id == id)
                .ok_or("That recording no longer exists")?;
            recording_plans_with(recording, &user_ids)
        }
        None => recording_plans_for(&file, &user_ids),
    };
    let outcome = run_recording_playback(&app, plans, user_ids).await;
    Ok(recording_play_results(&outcome))
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn play_recording_now(
    _user_ids: Vec<i64>,
    _recording_id: Option<String>,
) -> Result<Vec<RecordingPlayResult>, String> {
    Err("Recordings are only available on Windows".into())
}

/// Para a reprodução avulsa na hora (a do Modo AFK para com o Parar dele).
/// Tecla apertada é solta; nenhum cliente é fechado.
#[tauri::command]
fn stop_recording_playback() {
    RECORDING_PLAYBACK_STOP.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn get_recording_playback() -> RecordingPlaybackState {
    current_recording_playback()
}

// ── biblioteca ─────────────────────────────────────────────────────────────

/// O que a tela recebe: a biblioteca, as escolhas e a lista de teclas.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingsPayload {
    recordings: Vec<Recording>,
    default_id: Option<String>,
    /// user id (em texto, chave de JSON) → gravação própria.
    account_ids: HashMap<String, String>,
    keys: Vec<String>,
}

fn recordings_payload(file: RecordingsFile) -> RecordingsPayload {
    RecordingsPayload {
        recordings: file.recordings,
        default_id: file.default_id,
        account_ids: file
            .account_ids
            .into_iter()
            .map(|(uid, id)| (uid.to_string(), id))
            .collect(),
        keys: data::recordings::recording_key_names(),
    }
}

fn emit_recordings_changed(app: &tauri::AppHandle) {
    let _ = app.emit("recordings-changed", ());
}

#[tauri::command]
fn get_recordings(
    store: tauri::State<'_, data::recordings::RecordingStore>,
) -> Result<RecordingsPayload, String> {
    Ok(recordings_payload(store.load()?))
}

#[tauri::command]
fn save_recording(
    app: tauri::AppHandle,
    store: tauri::State<'_, data::recordings::RecordingStore>,
    recording: Recording,
) -> Result<Recording, String> {
    let saved = store.upsert(recording, chrono::Utc::now().timestamp_millis())?;
    emit_recordings_changed(&app);
    Ok(saved)
}

#[tauri::command]
fn duplicate_recording(
    app: tauri::AppHandle,
    store: tauri::State<'_, data::recordings::RecordingStore>,
    id: String,
    name: String,
) -> Result<Recording, String> {
    let copy = store.duplicate(&id, &name, chrono::Utc::now().timestamp_millis())?;
    emit_recordings_changed(&app);
    Ok(copy)
}

#[tauri::command]
fn delete_recording(
    app: tauri::AppHandle,
    store: tauri::State<'_, data::recordings::RecordingStore>,
    id: String,
) -> Result<bool, String> {
    let deleted = store.delete(&id)?;
    emit_recordings_changed(&app);
    Ok(deleted)
}

/// A gravação de todas as contas (`None` tira).
#[tauri::command]
fn set_default_recording(
    app: tauri::AppHandle,
    store: tauri::State<'_, data::recordings::RecordingStore>,
    id: Option<String>,
) -> Result<(), String> {
    store.set_default(id.filter(|id| !id.is_empty()))?;
    emit_recordings_changed(&app);
    Ok(())
}

/// A gravação própria de uma conta (`None` volta para a de todas).
#[tauri::command]
fn set_account_recording(
    app: tauri::AppHandle,
    store: tauri::State<'_, data::recordings::RecordingStore>,
    user_id: i64,
    id: Option<String>,
) -> Result<(), String> {
    store.set_for_account(user_id, id.filter(|id| !id.is_empty()))?;
    emit_recordings_changed(&app);
    Ok(())
}

#[cfg(test)]
mod recordings_playback_tests {
    use super::*;

    /// Um Windows de mentira: guarda o que a reprodução fez, com o relógio
    /// andando só quando ela dorme.
    #[derive(Default)]
    struct FakeDesk {
        log: Vec<String>,
        clock: u64,
        /// A janela sai da frente quando o relógio passa disto.
        focus_until: Option<u64>,
        /// Parar quando o relógio passa disto.
        stop_at: Option<u64>,
        refuse_keys: bool,
        refuse_clicks: bool,
    }

    impl RecordingDesk for FakeDesk {
        fn stopped(&self) -> bool {
            self.stop_at.is_some_and(|t| self.clock >= t)
        }
        fn target_in_front(&self) -> bool {
            self.focus_until.map_or(true, |t| self.clock < t)
        }
        fn key(&mut self, key: &str, up: bool) -> bool {
            self.log.push(format!("{}{key}@{}", if up { "up:" } else { "down:" }, self.clock));
            !self.refuse_keys || up
        }
        fn click(&mut self, point: AfkPoint, focus_click: bool) -> bool {
            self.log.push(format!(
                "click:{}x{}{}@{}",
                point.x_pct,
                point.y_pct,
                if focus_click { "+focus" } else { "" },
                self.clock
            ));
            !self.refuse_clicks
        }
        fn now_ms(&self) -> u64 {
            self.clock
        }
        fn sleep_ms(&mut self, ms: u64) {
            self.clock += ms;
        }
    }

    fn key(name: &str, hold_ms: u64) -> RecordingStep {
        RecordingStep::Key {
            key: name.into(),
            hold_ms,
        }
    }

    #[test]
    fn a_key_step_presses_holds_and_releases() {
        let mut desk = FakeDesk::default();
        let steps = vec![key("W", 300), RecordingStep::Wait { ms: 100 }, key("Space", 40)];
        play_recording_with(&mut desk, &recording_actions(&steps)).unwrap();
        assert_eq!(
            desk.log,
            vec!["down:W@0", "up:W@300", "down:Space@400", "up:Space@440"]
        );
    }

    #[test]
    fn only_the_first_click_of_a_run_gets_the_focus_click() {
        let mut desk = FakeDesk::default();
        let steps = vec![
            RecordingStep::Click {
                x_pct: 10.0,
                y_pct: 20.0,
            },
            RecordingStep::Click {
                x_pct: 30.0,
                y_pct: 40.0,
            },
        ];
        play_recording_with(&mut desk, &recording_actions(&steps)).unwrap();
        assert_eq!(desk.log, vec!["click:10x20+focus@0", "click:30x40@0"]);
    }

    #[test]
    fn stopping_interrupts_a_long_wait_at_once_and_releases_held_keys() {
        let mut desk = FakeDesk {
            stop_at: Some(1_000),
            ..Default::default()
        };
        let steps = vec![
            RecordingStep::KeyDown { key: "W".into() },
            RecordingStep::Wait { ms: 600_000 },
            RecordingStep::KeyUp { key: "W".into() },
        ];
        let result = play_recording_with(&mut desk, &recording_actions(&steps));
        assert_eq!(result, Err(RecordingPlayError::Stopped));
        assert!(
            desk.clock <= 1_000 + RECORDING_WAIT_SLICE_MS,
            "a espera de 10 min parou em {} ms",
            desk.clock
        );
        assert_eq!(desk.log.last().map(String::as_str), Some("up:W@1000"));
    }

    #[test]
    fn losing_the_focus_stops_before_the_next_key_and_releases_what_was_held() {
        let mut desk = FakeDesk {
            focus_until: Some(500),
            ..Default::default()
        };
        let steps = vec![
            RecordingStep::KeyDown { key: "W".into() },
            RecordingStep::Wait { ms: 1_000 },
            key("Space", 40),
            RecordingStep::KeyUp { key: "W".into() },
        ];
        let result = play_recording_with(&mut desk, &recording_actions(&steps));
        assert_eq!(result, Err(RecordingPlayError::FocusLost));
        assert_eq!(desk.log, vec!["down:W@0", "up:W@1000"], "o Space não saiu");
    }

    #[test]
    fn losing_the_focus_stops_before_a_click() {
        let mut desk = FakeDesk {
            focus_until: Some(0),
            ..Default::default()
        };
        let steps = vec![RecordingStep::Click {
            x_pct: 50.0,
            y_pct: 50.0,
        }];
        assert_eq!(
            play_recording_with(&mut desk, &recording_actions(&steps)),
            Err(RecordingPlayError::FocusLost)
        );
        assert!(desk.log.is_empty());
    }

    #[test]
    fn a_key_left_down_by_the_recording_is_released_at_the_end() {
        let mut desk = FakeDesk::default();
        let steps = vec![
            RecordingStep::KeyDown { key: "Shift".into() },
            RecordingStep::KeyDown { key: "W".into() },
            RecordingStep::Wait { ms: 50 },
        ];
        play_recording_with(&mut desk, &recording_actions(&steps)).unwrap();
        assert_eq!(
            desk.log,
            vec!["down:Shift@0", "down:W@0", "up:W@50", "up:Shift@50"]
        );
    }

    #[test]
    fn releasing_a_key_that_was_not_pressed_sends_nothing() {
        let mut desk = FakeDesk::default();
        let steps = vec![RecordingStep::KeyUp { key: "W".into() }];
        play_recording_with(&mut desk, &recording_actions(&steps)).unwrap();
        assert!(desk.log.is_empty());
    }

    #[test]
    fn a_refused_key_or_click_stops_with_its_error() {
        let mut desk = FakeDesk {
            refuse_keys: true,
            ..Default::default()
        };
        assert_eq!(
            play_recording_with(&mut desk, &recording_actions(&[key("E", 40)])),
            Err(RecordingPlayError::KeyRefused)
        );
        let mut desk = FakeDesk {
            refuse_clicks: true,
            ..Default::default()
        };
        assert_eq!(
            play_recording_with(
                &mut desk,
                &recording_actions(&[RecordingStep::Click {
                    x_pct: 1.0,
                    y_pct: 1.0
                }])
            ),
            Err(RecordingPlayError::ClickRefused)
        );
    }

    #[test]
    fn a_stop_before_the_start_sends_nothing() {
        let mut desk = FakeDesk {
            stop_at: Some(0),
            ..Default::default()
        };
        assert_eq!(
            play_recording_with(&mut desk, &recording_actions(&[key("E", 40)])),
            Err(RecordingPlayError::Stopped)
        );
        assert!(desk.log.is_empty());
    }

    #[test]
    fn playback_errors_carry_the_afk_status_codes() {
        assert_eq!(afk_error_from_playback(RecordingPlayError::Stopped).code(), "stopped");
        assert_eq!(afk_error_from_playback(RecordingPlayError::FocusLost).code(), "focusLost");
        assert_eq!(afk_error_from_playback(RecordingPlayError::KeyRefused).code(), "keyRefused");
        assert_eq!(
            afk_error_from_playback(RecordingPlayError::ClickRefused).code(),
            "clickRefused"
        );
        assert_eq!(AfkSendError::NoRecording.code(), "noRecording");
    }

    fn file() -> RecordingsFile {
        RecordingsFile {
            recordings: vec![
                Recording {
                    id: "all".into(),
                    name: "All".into(),
                    steps: vec![key("E", 40)],
                    ..Default::default()
                },
                Recording {
                    id: "mine".into(),
                    name: "Mine".into(),
                    steps: vec![key("W", 40)],
                    ..Default::default()
                },
                Recording {
                    id: "empty".into(),
                    name: "Empty".into(),
                    ..Default::default()
                },
            ],
            default_id: Some("all".into()),
            account_ids: [(11, "mine".to_string()), (33, "empty".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn each_account_plays_its_own_recording_or_the_one_for_everyone() {
        let plans = recording_plans_for(&file(), &[11, 22, 33]);
        assert_eq!(plans.get(&11), Some(&vec![key("W", 40)]));
        assert_eq!(plans.get(&22), Some(&vec![key("E", 40)]));
        // Gravação vazia não toca (só roubaria o foco).
        assert!(!plans.contains_key(&33));
    }

    #[test]
    fn without_a_recording_for_everyone_only_accounts_with_their_own_play() {
        let mut f = file();
        f.default_id = None;
        let plans = recording_plans_for(&f, &[11, 22]);
        assert!(plans.contains_key(&11));
        assert!(!plans.contains_key(&22));
    }

    #[test]
    fn the_status_codes_reach_the_screen() {
        let results = recording_play_results(&[
            (1, None),
            (2, Some(AfkSendError::FocusDenied)),
            (3, Some(AfkSendError::NoRecording)),
        ]);
        assert_eq!(results[0].error_code, None);
        assert_eq!(results[1].error_code.as_deref(), Some("focusDenied"));
        assert_eq!(results[2].error_code.as_deref(), Some("noRecording"));
    }

    #[test]
    fn the_screen_gets_the_choices_and_the_key_list() {
        let payload = recordings_payload(file());
        assert_eq!(payload.account_ids.get("11").map(String::as_str), Some("mine"));
        assert_eq!(&payload.keys[..2], &["Space".to_string(), "W".to_string()]);
    }
}

#[cfg(test)]
mod recordings_after_reconnect_tests {
    use super::*;

    fn in_game(pid: u32) -> AfterReconnectObservation {
        AfterReconnectObservation {
            tracked_pid: Some(pid),
            adopted: false,
            in_game: true,
        }
    }

    fn loading(pid: u32) -> AfterReconnectObservation {
        AfterReconnectObservation {
            tracked_pid: Some(pid),
            adopted: false,
            in_game: false,
        }
    }

    #[test]
    fn the_recording_plays_only_after_the_account_stays_the_delay_in_game() {
        let mut book = AfterReconnectBook::default();
        book.arm(11, 0);
        // Ainda carregando: nada.
        assert!(book.tick(2_000, true, 30_000, |_| loading(500)).is_empty());
        // Entrou no jogo aos 4 s.
        assert!(book.tick(4_000, true, 30_000, |_| in_game(500)).is_empty());
        assert!(book.tick(33_999, true, 30_000, |_| in_game(500)).is_empty());
        assert_eq!(book.tick(34_000, true, 30_000, |_| in_game(500)), vec![11]);
        // Toca uma vez só.
        assert!(book.tick(40_000, true, 30_000, |_| in_game(500)).is_empty());
        assert!(book.is_empty());
    }

    #[test]
    fn only_the_account_that_reconnected_plays() {
        let mut book = AfterReconnectBook::default();
        book.arm(11, 0);
        let fired = book.tick(60_000, true, 5_000, |_| in_game(1));
        assert!(fired.is_empty(), "primeira passada só marca a entrada");
        let fired = book.tick(65_000, true, 5_000, |uid| {
            if uid == 11 {
                in_game(1)
            } else {
                in_game(2)
            }
        });
        assert_eq!(fired, vec![11]);
    }

    #[test]
    fn leaving_the_game_restarts_the_count() {
        let mut book = AfterReconnectBook::default();
        book.arm(11, 0);
        book.tick(1_000, true, 10_000, |_| in_game(7));
        book.tick(8_000, true, 10_000, |_| loading(7));
        assert!(book.tick(12_000, true, 10_000, |_| in_game(7)).is_empty());
        assert!(book.tick(21_999, true, 10_000, |_| in_game(7)).is_empty());
        assert_eq!(book.tick(22_000, true, 10_000, |_| in_game(7)), vec![11]);
    }

    #[test]
    fn a_new_client_from_another_attempt_counts_from_zero() {
        let mut book = AfterReconnectBook::default();
        book.arm(11, 0);
        book.tick(1_000, true, 10_000, |_| in_game(7));
        // A reconexão relançou de novo: outro PID.
        assert!(book.tick(10_500, true, 10_000, |_| in_game(8)).is_empty());
        assert_eq!(book.tick(20_500, true, 10_000, |_| in_game(8)), vec![11]);
    }

    #[test]
    fn turning_the_option_off_forgets_everyone() {
        let mut book = AfterReconnectBook::default();
        book.arm(11, 0);
        assert!(book.tick(100_000, false, 1, |_| in_game(1)).is_empty());
        assert!(book.is_empty());
    }

    #[test]
    fn a_website_client_never_gets_the_recording() {
        let mut book = AfterReconnectBook::default();
        book.arm(11, 0);
        let fired = book.tick(100_000, true, 1, |_| AfterReconnectObservation {
            tracked_pid: Some(3),
            adopted: true,
            in_game: true,
        });
        assert!(fired.is_empty());
        assert!(book.is_empty());
    }

    #[test]
    fn an_account_that_never_gets_back_in_game_is_given_up() {
        let mut book = AfterReconnectBook::default();
        book.arm(11, 0);
        book.tick(AFTER_RECONNECT_GIVE_UP_MS - 1, true, 5_000, |_| loading(1));
        assert!(!book.is_empty());
        book.tick(AFTER_RECONNECT_GIVE_UP_MS, true, 5_000, |_| AfterReconnectObservation::default());
        assert!(book.is_empty());
    }

    #[test]
    fn the_delay_is_kept_between_5_seconds_and_an_hour() {
        assert_eq!(clamp_after_reconnect_delay_seconds(30), 30);
        assert_eq!(clamp_after_reconnect_delay_seconds(0), 5);
        assert_eq!(clamp_after_reconnect_delay_seconds(-1), 5);
        assert_eq!(clamp_after_reconnect_delay_seconds(99_999), 3_600);
    }
}

#[cfg(test)]
mod recordings_afk_mode_tests {
    use super::*;

    #[test]
    fn the_afk_mode_knows_the_recording_mode() {
        assert_eq!(AfkMode::parse("recording"), AfkMode::Recording);
        assert_eq!(AfkMode::parse(" Recording "), AfkMode::Recording);
        assert_eq!(AfkMode::Recording.as_str(), "recording");
        // Valor desconhecido continua virando tecla.
        assert_eq!(AfkMode::parse("macro"), AfkMode::Key);
    }

    #[test]
    fn the_recording_mode_starts_without_a_key_but_not_without_an_account() {
        assert!(validate_afk_start(AfkMode::Recording, "", &[11]).is_ok());
        assert!(validate_afk_start(AfkMode::Recording, "", &[]).is_err());
    }

    #[test]
    fn a_recording_click_without_the_focus_click_is_a_single_click() {
        let rect = AfkClientRect {
            left: 100,
            top: 50,
            width: 800,
            height: 600,
        };
        let presses = |plan: &[AfkMouseStep]| plan.iter().filter(|s| matches!(s, AfkMouseStep::Press)).count();
        let with_focus = afk_click_plan_with(rect, AFK_DEFAULT_POINT, 40, true).unwrap();
        let single = afk_click_plan_with(rect, AFK_DEFAULT_POINT, 40, false).unwrap();
        assert_eq!(presses(&with_focus), 2);
        assert_eq!(presses(&single), 1);
        // O AFK continua com a receita de dois cliques.
        assert_eq!(afk_click_plan(rect, AFK_DEFAULT_POINT, 40).unwrap(), with_focus);
        // E o clique único também nunca sai da área interna.
        for step in single {
            if let AfkMouseStep::MoveTo(x, y) = step {
                assert!((100..900).contains(&x) && (50..650).contains(&y));
            }
        }
    }

    #[test]
    fn the_new_status_codes_have_messages() {
        for error in [AfkSendError::NoRecording, AfkSendError::FocusLost, AfkSendError::Stopped] {
            assert!(!error.message().is_empty());
        }
    }
}
