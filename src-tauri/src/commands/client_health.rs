// Saúde de cada cliente que o app acompanha: a queda lida do log do Roblox,
// com o motivo, e o nome da conta no título da janela. Ver
// docs/features/watcher.md ("Quedas" e "Nome da conta na janela").
//
// Tudo aqui só **lê**: o log (aberto só para leitura) e a lista de processos.
// Quem fecha cliente continua sendo só o Watcher, e só os que o app abriu
// (`only_launched_by_app`). Cliente adotado do site só ganha o aviso na tela.
//
// O classificador e a máquina de estados são puros (testados abaixo); o laço
// do Windows só junta as peças: acha o log de cada PID pelo mesmo casamento da
// varredura de clientes de fora (`platform::windows::locate_logs_for_pids`) e
// lê só os bytes novos a cada passada.

/// Carência depois de um teleporte: a desconexão do servidor antigo não é
/// queda. Se a conta entrar no jogo novo dentro dela, nada é mostrado.
const TELEPORT_GRACE_MS: i64 = 8_000;
/// Teto de leitura do log por cliente por passada (o resto fica para a próxima).
const CLIENT_LOG_READ_MAX: u64 = 8 * 1024 * 1024;
/// Sem log achado, tenta de novo depois disto (listar a pasta tem custo).
const CLIENT_LOG_RETRY_MS: i64 = 10_000;
/// Mensagem de kick vem do jogo: corta para não virar parágrafo na tela.
const KICK_MESSAGE_MAX_CHARS: usize = 200;

/// Por que a conta caiu, quando o código diz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DropReason {
    /// 277, 279, 266, 260–262: a conexão com o servidor caiu.
    ConnectionLost,
    /// 264, 273 (e 276, só pelos concorrentes): a mesma conta entrou em outro lugar.
    JoinedElsewhere,
    /// 278: parada tempo demais (kick de inatividade do próprio Roblox).
    Idle,
    Other,
}

/// O que uma linha do log diz sobre a sessão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientLogEvent {
    /// `! Joining game '<job>' place <place> at <ip>`.
    JoinedGame {
        place_id: Option<i64>,
        job_id: Option<String>,
    },
    /// `doTeleport:` / `finishTeleportWithJoinScriptPayload` / `Teleported.`.
    TeleportStarted,
    /// A pessoa saiu do jogo (fechou a janela, voltou para a home).
    LeftVoluntarily,
    Disconnected { code: u32, reason: DropReason },
    Kicked {
        code: Option<u32>,
        message: Option<String>,
    },
    ServerShutdown { code: u32 },
}

/// Texto depois de `marker`, se houver.
fn text_after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.find(marker).map(|i| &line[i + marker.len()..])
}

/// Primeiro número logo depois de `marker` (pula espaços e um rótulo curto,
/// como em `for reason: Player: 285`).
fn number_after(line: &str, marker: &str) -> Option<u32> {
    let rest = text_after(line, marker)?;
    let start = rest.find(|c: char| c.is_ascii_digit())?;
    // O número tem que estar perto do marcador: `Reason: 285`, `Player: 285`.
    if start > 16 {
        return None;
    }
    let digits: String = rest[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn drop_reason_for_code(code: u32) -> DropReason {
    match code {
        264 | 273 | 276 => DropReason::JoinedElsewhere,
        260..=262 | 266 | 277 | 279 => DropReason::ConnectionLost,
        278 => DropReason::Idle,
        _ => DropReason::Other,
    }
}

/// O evento de um código de desconexão (`Enum.ConnectionError` do Roblox).
/// 285 é a saída pedida pelo próprio cliente: aparece em **toda** saída e em
/// **todo** teleporte (no 0.742, depois até do join do servidor novo), então
/// não diz nada sozinho — a saída é reconhecida pelas linhas de saída.
pub fn event_for_disconnect_code(code: u32) -> Option<ClientLogEvent> {
    match code {
        285 => None,
        267 => Some(ClientLogEvent::Kicked {
            code: Some(code),
            message: None,
        }),
        274 | 275 => Some(ClientLogEvent::ServerShutdown { code }),
        _ => Some(ClientLogEvent::Disconnected {
            code,
            reason: drop_reason_for_code(code),
        }),
    }
}

fn disconnect_code(line: &str) -> Option<u32> {
    // Cliente 0.740/0.741 (transporte antigo).
    for marker in [
        "Disconnection Notification. Reason: ",
        "Sending disconnect with reason: ",
    ] {
        if let Some(code) = number_after(line, marker) {
            return Some(code);
        }
    }
    // 0.742 (RbxTransport): `Disconnected from server for reason: Player: 285 (DisconnectClientInitiated)`.
    if let Some(code) = number_after(line, "Disconnected from server for reason: ") {
        return Some(code);
    }
    // Texto do aviso de erro (`Error Code: 277`), visto nos concorrentes. Só a
    // faixa das desconexões: "error code: 403" de um asset não é queda.
    let lower = line.to_ascii_lowercase();
    number_after(&lower, "error code: ").filter(|code| (256..=299).contains(code))
}

/// `You were kicked from this experience: <mensagem> (Error Code: 267)`.
fn kick_message(line: &str) -> Option<Option<String>> {
    let lower = line.to_ascii_lowercase();
    let marker = "kicked from this experience";
    let at = lower.find(marker)?;
    let rest = line[at + marker.len()..].trim_start_matches([':', ' ']);
    let rest = match rest.to_ascii_lowercase().find("(error code") {
        Some(end) => &rest[..end],
        None => rest,
    };
    let message: String = rest.trim().chars().take(KICK_MESSAGE_MAX_CHARS).collect();
    Some((!message.is_empty()).then_some(message))
}

/// Classifica uma linha do log do cliente. Puro: nada aqui guarda a linha
/// (ela pode trazer ticket, IP e Job ID).
pub fn classify_log_line(line: &str) -> Option<ClientLogEvent> {
    if let Some(rest) = text_after(line, "! Joining game '") {
        let end = rest.find('\'')?;
        let job = &rest[..end];
        let place_id = text_after(&rest[end..], "' place ").and_then(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        });
        return Some(ClientLogEvent::JoinedGame {
            place_id,
            job_id: (!job.is_empty()).then(|| job.to_string()),
        });
    }
    if line.contains("UgcExperienceController: doTeleport:")
        || line.contains("finishTeleportWithJoinScriptPayload")
        || line.contains("[FLog::SessionTransitionFSM] Teleported.")
    {
        return Some(ClientLogEvent::TeleportStarted);
    }
    if line.contains("[FLog::SingleSurfaceApp] leaveUGCGameInternal")
        || line.contains("[FLog::SessionTransitionFSM] Tearing down.")
        || line.contains("[FLog::SingleSurfaceApp] returnToLuaApp")
    {
        return Some(ClientLogEvent::LeftVoluntarily);
    }
    if let Some(message) = kick_message(line) {
        return Some(ClientLogEvent::Kicked {
            code: Some(267),
            message,
        });
    }
    event_for_disconnect_code(disconnect_code(line)?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DropKind {
    Disconnected,
    Kicked,
    ServerShutdown,
    /// O processo terminou sem a conta sair do jogo (e sem o app fechá-lo).
    Crashed,
}

/// A conta caiu: o que a Sessão mostra.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDrop {
    pub kind: DropKind,
    pub reason: Option<DropReason>,
    pub code: Option<u32>,
    pub message: Option<String>,
    pub since_ms: i64,
}

impl ClientDrop {
    fn same_drop(&self, other: &ClientDrop) -> bool {
        self.kind == other.kind && self.code == other.code
    }
}

/// Onde a conta entrou por último (`! Joining game '<job>' place <place>`).
/// A reconexão (commands/reconnect.rs) volta para cá; nunca vai para a tela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinedDestination {
    pub place_id: i64,
    pub job_id: Option<String>,
}

/// O que o log disse até agora sobre a sessão de um cliente.
#[derive(Debug, Clone, Default)]
pub struct ClientLogSession {
    /// Queda vista, esperando a carência do teleporte.
    pending: Option<ClientDrop>,
    drop: Option<ClientDrop>,
    last_teleport_ms: Option<i64>,
    left: bool,
    /// Já entrou num jogo alguma vez (cliente parado no mutex nunca entra).
    joined: bool,
    /// Place e servidor do último `Joining game` (histórico de sessões).
    place_id: Option<i64>,
    job_id: Option<String>,
    destination: Option<JoinedDestination>,
}

impl ClientLogSession {
    pub fn apply(&mut self, event: &ClientLogEvent, now_ms: i64) {
        match event {
            ClientLogEvent::JoinedGame { place_id, job_id } => {
                // Entrou num jogo (de novo): o que caiu antes já não vale.
                self.place_id = *place_id;
                self.job_id = job_id.clone();
                self.pending = None;
                self.drop = None;
                self.left = false;
                self.joined = true;
                self.last_teleport_ms = None;
                if let Some(place_id) = place_id {
                    self.destination = Some(JoinedDestination {
                        place_id: *place_id,
                        job_id: job_id.clone(),
                    });
                }
            }
            ClientLogEvent::TeleportStarted => self.last_teleport_ms = Some(now_ms),
            ClientLogEvent::LeftVoluntarily => self.left = true,
            ClientLogEvent::Disconnected { code, reason } => self.note_drop(ClientDrop {
                kind: DropKind::Disconnected,
                reason: Some(*reason),
                code: Some(*code),
                message: None,
                since_ms: now_ms,
            }),
            ClientLogEvent::Kicked { code, message } => self.note_drop(ClientDrop {
                kind: DropKind::Kicked,
                reason: None,
                code: *code,
                message: message.clone(),
                since_ms: now_ms,
            }),
            ClientLogEvent::ServerShutdown { code } => self.note_drop(ClientDrop {
                kind: DropKind::ServerShutdown,
                reason: None,
                code: Some(*code),
                message: None,
                since_ms: now_ms,
            }),
        }
    }

    fn note_drop(&mut self, drop: ClientDrop) {
        // Depois que a pessoa saiu, o que vier é a desmontagem da sessão.
        if self.left {
            return;
        }
        let existing = match (&mut self.pending, &mut self.drop) {
            (Some(pending), _) => pending,
            (None, Some(current)) => current,
            (None, None) => {
                self.pending = Some(drop);
                return;
            }
        };
        // A mensagem do kick pode vir numa linha separada do código.
        if drop.kind == DropKind::Kicked && existing.message.is_none() && drop.message.is_some() {
            existing.kind = DropKind::Kicked;
            existing.reason = None;
            existing.message = drop.message;
        }
    }

    /// Confirma a queda pendente: na hora, ou depois da carência se houve
    /// teleporte perto dela.
    pub fn settle(&mut self, now_ms: i64) {
        let Some(pending) = &self.pending else {
            return;
        };
        let teleport = self
            .last_teleport_ms
            .filter(|t| (pending.since_ms - t).abs() <= TELEPORT_GRACE_MS);
        let ready = match teleport {
            Some(t) => now_ms - t.max(pending.since_ms) >= TELEPORT_GRACE_MS,
            None => true,
        };
        if ready {
            self.drop = self.pending.take();
        }
    }

    pub fn current_drop(&self) -> Option<&ClientDrop> {
        self.drop.as_ref()
    }

    /// Está num jogo agora: entrou, e depois disso não caiu nem saiu. É o
    /// sinal que a fila de launch espera antes da próxima conta (launch.rs).
    pub fn in_game(&self) -> bool {
        self.joined && !self.left && self.drop.is_none() && self.pending.is_none()
    }

    /// Onde a conta entrou por último, se o log disse.
    pub fn destination(&self) -> Option<&JoinedDestination> {
        self.destination.as_ref()
    }

    /// O processo terminou dentro de um jogo, sem sair e sem queda: fechou
    /// sozinho. Cliente que nunca entrou num jogo (preso no "só um Roblox
    /// aberto", por exemplo) não conta.
    pub fn ended_without_leaving(&self) -> bool {
        self.joined && !self.left && self.drop.is_none() && self.pending.is_none()
    }

    pub fn absorb(&mut self, text: &str, now_ms: i64) {
        for line in text.lines() {
            if let Some(event) = classify_log_line(line) {
                self.apply(&event, now_ms);
            }
        }
    }
}

/// Quantos bytes de `bytes` formam linhas completas (até o último `\n`). Um
/// pedaço cheio sem quebra de linha segue assim mesmo, para não travar.
fn complete_line_bytes(bytes: &[u8], max: u64) -> usize {
    match bytes.iter().rposition(|b| *b == b'\n') {
        Some(last_newline) => last_newline + 1,
        None if bytes.len() as u64 >= max => bytes.len(),
        None => 0,
    }
}

/// Um cliente que o app acompanha, como o tracker diz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedClient {
    pub user_id: i64,
    pub pid: u32,
    /// Aberto pelo site e reconhecido: só aviso na tela, nunca ação.
    pub adopted: bool,
    /// O nome que vai no título da janela (já mascarado com os nomes
    /// ocultos). `None`: opção desligada — o título volta a ser "Roblox".
    pub window_label: Option<String>,
}

// ── Nome da conta no título da janela ──────────────────────────────────────

/// O título que o Roblox põe na janela do cliente em jogo.
pub const ROBLOX_WINDOW_TITLE: &str = "Roblox";
/// O que a tela mostra no lugar de um nome escondido por inteiro (igual a
/// `HIDDEN_NAME` em src/utils/accountName.ts).
const HIDDEN_ACCOUNT_NAME: &str = "************";

/// Espelho de `maskAccountName` (src/utils/accountName.ts), com a mesma conta
/// de letras do JavaScript (unidades UTF-16): com os nomes ocultos, a barra de
/// tarefas não pode mostrar mais do que a tela do app. Os dois lados leem os
/// mesmos casos de `src/utils/accountNameCases.json`.
pub fn mask_account_name(name: &str, hidden: bool, preview_letters: i64) -> String {
    if !hidden {
        return name.to_string();
    }
    let units: Vec<u16> = name.encode_utf16().collect();
    if preview_letters > 0 && (preview_letters as usize) < units.len() {
        let mut shown = String::from_utf16_lossy(&units[..preview_letters as usize]);
        shown.push_str("********");
        return shown;
    }
    HIDDEN_ACCOUNT_NAME.to_string()
}

/// Alias || Username, mascarado como a tela do app mostra.
pub fn window_account_label(alias: &str, username: &str, hidden: bool, preview_letters: i64) -> String {
    let raw = if alias.is_empty() { username } else { alias };
    mask_account_name(raw, hidden, preview_letters)
}

/// Entre o nome da conta e "Roblox" no título.
const WINDOW_TITLE_SEPARATOR: &str = " — ";

/// `<conta> — Roblox`. O nome vem primeiro (decisão do dono, 10/10/2026): ao
/// passar o mouse na barra de tarefas, o Windows mostra o começo do título, e
/// o nome aparece inteiro.
pub fn client_window_title(label: &str) -> String {
    format!("{label}{WINDOW_TITLE_SEPARATOR}{ROBLOX_WINDOW_TITLE}")
}

/// O mesmo título na ordem que a versão anterior punha (`Roblox — <conta>`).
/// Uma janela renomeada por ela e ainda aberta depois da atualização tem que
/// ser reconhecida como nossa (e passar para a ordem nova), não como título
/// estranho para as regras do Watcher.
fn previous_order_title(desired: &str) -> Option<String> {
    let label = desired.strip_suffix(&format!("{WINDOW_TITLE_SEPARATOR}{ROBLOX_WINDOW_TITLE}"))?;
    Some(format!("{ROBLOX_WINDOW_TITLE}{WINDOW_TITLE_SEPARATOR}{label}"))
}

/// O título como o Roblox o pôs: se o atual é o que o app pôs (ou poria agora,
/// depois de reabrir, inclusive na ordem da versão anterior), vale "Roblox". As
/// regras do Watcher que olham o título (título esperado, beta, sem conexão)
/// comparam isto, nunca o nome da conta.
pub fn effective_client_title<'a>(current: &'a str, applied: Option<&str>, desired: Option<&str>) -> &'a str {
    let ours = !current.is_empty()
        && (applied == Some(current)
            || desired == Some(current)
            || desired.and_then(previous_order_title).as_deref() == Some(current));
    if ours {
        ROBLOX_WINDOW_TITLE
    } else {
        current
    }
}

/// O título a pôr agora, se precisar mudar. Só mexe numa janela que está com o
/// título normal do Roblox (ou o nosso): se o Roblox mostra outra coisa (erro,
/// beta), deixa como está, para as regras do Watcher continuarem vendo.
pub fn plan_window_title(current: &str, applied: Option<&str>, desired: Option<&str>) -> Option<String> {
    if effective_client_title(current, applied, desired) != ROBLOX_WINDOW_TITLE {
        return None;
    }
    let want = desired.unwrap_or(ROBLOX_WINDOW_TITLE);
    (current != want).then(|| want.to_string())
}

/// O que o monitor precisa do sistema (dublê nos testes).
pub trait ClientHealthOs {
    fn alive_pids(&self) -> HashSet<u32>;
    /// PID → log, para os PIDs sem log. `taken`: logs que já têm dono.
    fn locate_logs(&self, pids: &[u32], taken: &HashSet<std::path::PathBuf>) -> HashMap<u32, std::path::PathBuf>;
    fn read_log(&self, path: &std::path::Path, offset: u64, max: u64) -> Option<Vec<u8>>;
    /// O próprio app fechou este PID (Fechar, Auto Rejoin, Watcher…).
    fn terminated_by_app(&self, pid: u32) -> bool;
    /// Janela principal do cliente (HWND como número).
    fn main_window(&self, pid: u32) -> Option<isize>;
    fn window_title(&self, hwnd: isize) -> String;
    fn set_window_title(&self, hwnd: isize, title: &str) -> bool;
    /// A janela está "Não respondendo" (`IsHungAppWindow`).
    fn is_hung(&self, hwnd: isize) -> bool;
}

#[derive(Debug, Clone)]
struct ClientHealthEntry {
    pid: u32,
    adopted: bool,
    log: Option<std::path::PathBuf>,
    offset: u64,
    next_locate_ms: i64,
    session: ClientLogSession,
    crashed: Option<ClientDrop>,
    exit_seen: bool,
    reported: Option<ClientDrop>,
    /// O título que o app pôs na janela (para reconhecê-lo depois).
    applied_title: Option<String>,
    hung: HungWatch,
    not_responding: bool,
}

impl ClientHealthEntry {
    fn new(client: &TrackedClient) -> Self {
        Self {
            pid: client.pid,
            adopted: client.adopted,
            log: None,
            offset: 0,
            next_locate_ms: 0,
            session: ClientLogSession::default(),
            crashed: None,
            exit_seen: false,
            reported: None,
            applied_title: None,
            hung: HungWatch::default(),
            not_responding: false,
        }
    }

    fn current_drop(&self) -> Option<&ClientDrop> {
        self.crashed.as_ref().or(self.session.current_drop())
    }
}

/// O que a Sessão e o Watcher leem de um cliente.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientHealthView {
    pub pid: u32,
    /// O log do cliente foi achado: a queda vem dele, não do título da janela.
    pub log_found: bool,
    pub drop: Option<ClientDrop>,
    /// O título que o app pôs na janela (nome da conta), se pôs.
    pub window_title: Option<String>,
    /// A janela está "Não respondendo" há 30 s ou mais (só clientes do app).
    pub not_responding: bool,
    /// Está num jogo agora (o log disse que entrou, e não caiu nem saiu).
    pub in_game: bool,
    /// Onde entrou por último — só para a reconexão, não vai para a tela.
    #[serde(skip)]
    pub destination: Option<JoinedDestination>,
    /// O processo terminou (e o monitor viu).
    #[serde(skip)]
    pub exited: bool,
}

/// Retrato de um cliente para o histórico de sessões: o jogo em que está, se
/// caiu, se a pessoa saiu e se o processo já terminou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSessionSnapshot {
    pub pid: u32,
    pub place_id: Option<i64>,
    pub job_id: Option<String>,
    pub drop: Option<ClientDrop>,
    pub left: bool,
    pub exited: bool,
}

/// Quanto tempo a janela fica "Não respondendo" antes de o app avisar (e de
/// o Watcher poder fechá-la, se a opção estiver ligada).
pub const HUNG_THRESHOLD_MS: i64 = 30_000;

/// Desde quando a janela não responde. O Windows já só chama de travada a
/// janela que passou 5 s sem tratar mensagens (`IsHungAppWindow`); os 30 s
/// daqui tiram do caminho a carga de um jogo pesado.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HungWatch {
    since_ms: Option<i64>,
}

impl HungWatch {
    pub fn update(&mut self, hung: bool, now_ms: i64) {
        if hung {
            self.since_ms.get_or_insert(now_ms);
        } else {
            self.since_ms = None;
        }
    }

    pub fn is_not_responding(&self, now_ms: i64) -> bool {
        self.since_ms
            .is_some_and(|since| now_ms - since >= HUNG_THRESHOLD_MS)
    }
}

/// Mudança que vira evento e linha no Console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthNotice {
    Dropped {
        user_id: i64,
        drop: ClientDrop,
        adopted: bool,
    },
    /// A conta voltou a um jogo depois de uma queda.
    Recovered { user_id: i64 },
    /// A janela ficou (ou deixou de ficar) "Não respondendo" por 30 s.
    NotResponding { user_id: i64, not_responding: bool },
}

#[derive(Debug, Default)]
pub struct ClientHealthMonitor {
    entries: HashMap<i64, ClientHealthEntry>,
}

impl ClientHealthMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(
        &mut self,
        os: &dyn ClientHealthOs,
        tracked: &[TrackedClient],
        now_ms: i64,
    ) -> Vec<HealthNotice> {
        let mut notices = Vec::new();
        // Conta que saiu do tracker, ou que trocou de PID, começa do zero.
        self.entries.retain(|uid, entry| {
            tracked
                .iter()
                .any(|t| t.user_id == *uid && t.pid == entry.pid)
        });
        if tracked.is_empty() {
            return notices;
        }
        for client in tracked {
            let entry = self
                .entries
                .entry(client.user_id)
                .or_insert_with(|| ClientHealthEntry::new(client));
            entry.adopted = client.adopted;
        }

        let alive = os.alive_pids();

        let taken: HashSet<std::path::PathBuf> =
            self.entries.values().filter_map(|e| e.log.clone()).collect();
        let need: Vec<u32> = self
            .entries
            .values()
            .filter(|e| e.log.is_none() && alive.contains(&e.pid) && now_ms >= e.next_locate_ms)
            .map(|e| e.pid)
            .collect();
        if !need.is_empty() {
            let found = os.locate_logs(&need, &taken);
            for entry in self.entries.values_mut() {
                if !need.contains(&entry.pid) {
                    continue;
                }
                match found.get(&entry.pid) {
                    Some(path) => entry.log = Some(path.clone()),
                    None => entry.next_locate_ms = now_ms + CLIENT_LOG_RETRY_MS,
                }
            }
        }

        for client in tracked {
            let Some(entry) = self.entries.get_mut(&client.user_id) else {
                continue;
            };
            if alive.contains(&entry.pid) {
                if let Some(path) = entry.log.clone() {
                    if let Some(bytes) = os.read_log(&path, entry.offset, CLIENT_LOG_READ_MAX) {
                        let consumed = complete_line_bytes(&bytes, CLIENT_LOG_READ_MAX);
                        if consumed > 0 {
                            entry
                                .session
                                .absorb(&String::from_utf8_lossy(&bytes[..consumed]), now_ms);
                            entry.offset += consumed as u64;
                        }
                    }
                }
            } else if !entry.exit_seen {
                entry.exit_seen = true;
                // Sem log não dá para saber se a pessoa fechou a janela: não chuta.
                if entry.log.is_some()
                    && entry.session.ended_without_leaving()
                    && !os.terminated_by_app(entry.pid)
                {
                    entry.crashed = Some(ClientDrop {
                        kind: DropKind::Crashed,
                        reason: None,
                        code: None,
                        message: None,
                        since_ms: now_ms,
                    });
                }
            }
            entry.session.settle(now_ms);

            let window = alive
                .contains(&entry.pid)
                .then(|| os.main_window(entry.pid))
                .flatten();
            let hung = window.is_some_and(|hwnd| os.is_hung(hwnd));
            // "Não respondendo" só nos clientes que o app abriu: o do site é
            // da pessoa, e o app não tem o que fazer com ele.
            let was_not_responding = entry.not_responding;
            entry.hung.update(hung && !entry.adopted, now_ms);
            let not_responding = entry.hung.is_not_responding(now_ms);
            entry.not_responding = not_responding;
            if not_responding != was_not_responding {
                notices.push(HealthNotice::NotResponding {
                    user_id: client.user_id,
                    not_responding,
                });
            }
            // Janela travada: nem o título se mexe (só volta a conferir quando
            // ela responder).
            if let (Some(hwnd), false) = (window, hung) {
                Self::apply_window_title(os, entry, hwnd, client.window_label.as_deref());
            }

            let current = entry.current_drop().cloned();
            let changed = match (&current, &entry.reported) {
                (Some(now), Some(before)) => !now.same_drop(before),
                (None, None) => false,
                _ => true,
            };
            if changed {
                match &current {
                    Some(drop) => notices.push(HealthNotice::Dropped {
                        user_id: client.user_id,
                        drop: drop.clone(),
                        adopted: entry.adopted,
                    }),
                    None => notices.push(HealthNotice::Recovered {
                        user_id: client.user_id,
                    }),
                }
            }
            entry.reported = current;
        }
        notices
    }

    /// Põe (ou tira) o nome da conta no título. O Roblox pode trocar o título
    /// de volta (teleporte): cada passada confere e põe de novo — ler o título
    /// não custa nada.
    fn apply_window_title(
        os: &dyn ClientHealthOs,
        entry: &mut ClientHealthEntry,
        hwnd: isize,
        label: Option<&str>,
    ) {
        let desired = label
            .filter(|l| !l.trim().is_empty())
            .map(client_window_title);
        let current = os.window_title(hwnd);
        if current != ROBLOX_WINDOW_TITLE
            && effective_client_title(&current, entry.applied_title.as_deref(), desired.as_deref())
                == ROBLOX_WINDOW_TITLE
        {
            // Já é nosso (inclusive o que ficou de antes de o app reabrir).
            entry.applied_title = Some(current.clone());
        }
        if let Some(next) =
            plan_window_title(&current, entry.applied_title.as_deref(), desired.as_deref())
        {
            if os.set_window_title(hwnd, &next) {
                entry.applied_title = (next != ROBLOX_WINDOW_TITLE).then_some(next);
            }
        }
    }

    /// Onde cada cliente está e como a sessão dele terminou, para o histórico
    /// de sessões (commands/session_history.rs). Só leitura do que o monitor já
    /// sabe: nada novo é lido do log por causa disto.
    pub fn session_snapshots(&self) -> HashMap<i64, ClientSessionSnapshot> {
        self.entries
            .iter()
            .map(|(uid, entry)| {
                (
                    *uid,
                    ClientSessionSnapshot {
                        pid: entry.pid,
                        place_id: entry.session.place_id,
                        job_id: entry.session.job_id.clone(),
                        drop: entry.current_drop().cloned(),
                        left: entry.session.left,
                        exited: entry.exit_seen,
                    },
                )
            })
            .collect()
    }

    pub fn views(&self) -> HashMap<i64, ClientHealthView> {
        self.entries
            .iter()
            .map(|(uid, entry)| {
                (
                    *uid,
                    ClientHealthView {
                        pid: entry.pid,
                        log_found: entry.log.is_some(),
                        drop: entry.current_drop().cloned(),
                        window_title: entry.applied_title.clone(),
                        not_responding: entry.not_responding,
                        in_game: !entry.exit_seen && entry.session.in_game(),
                        destination: entry.session.destination().cloned(),
                        exited: entry.exit_seen,
                    },
                )
            })
            .collect()
    }
}

/// Último retrato do monitor, para o `get_running_instances` e o Watcher.
static CLIENT_HEALTH_VIEWS: LazyLock<Mutex<HashMap<i64, ClientHealthView>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A saúde do cliente `pid` da conta (`None` se o monitor ainda não o viu).
pub(crate) fn client_health_of(user_id: i64, pid: u32) -> Option<ClientHealthView> {
    CLIENT_HEALTH_VIEWS
        .lock()
        .ok()?
        .get(&user_id)
        .filter(|view| view.pid == pid)
        .cloned()
}

/// Linha do Console (português, como as do Watcher) para uma queda.
fn drop_console_line(drop: &ClientDrop) -> String {
    let code = drop.code.map(|c| format!(" (código {c})")).unwrap_or_default();
    match drop.kind {
        DropKind::Disconnected => match drop.reason {
            Some(DropReason::ConnectionLost) => format!("Caiu: perdeu a conexão{code}"),
            Some(DropReason::JoinedElsewhere) => {
                format!("Caiu: a conta entrou em outro lugar{code}")
            }
            Some(DropReason::Idle) => format!("Caiu: ficou parada tempo demais{code}"),
            _ => format!("Caiu{code}"),
        },
        DropKind::Kicked => match &drop.message {
            Some(message) => format!("Foi expulso: {message}"),
            None => format!("Foi expulso do jogo{code}"),
        },
        DropKind::ServerShutdown => format!("O servidor fechou{code}"),
        DropKind::Crashed => String::from("O cliente fechou sem sair do jogo"),
    }
}

#[cfg(target_os = "windows")]
const CLIENT_HEALTH_TICK: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(target_os = "windows")]
struct WindowsClientHealthOs;

#[cfg(target_os = "windows")]
impl ClientHealthOs for WindowsClientHealthOs {
    fn alive_pids(&self) -> HashSet<u32> {
        platform::windows::get_roblox_pids().into_iter().collect()
    }
    fn locate_logs(&self, pids: &[u32], taken: &HashSet<std::path::PathBuf>) -> HashMap<u32, std::path::PathBuf> {
        platform::windows::locate_logs_for_pids(pids, taken)
    }
    fn read_log(&self, path: &std::path::Path, offset: u64, max: u64) -> Option<Vec<u8>> {
        platform::windows::read_roblox_log(path, offset, max)
    }
    fn terminated_by_app(&self, pid: u32) -> bool {
        platform::windows::was_terminated_by_app(pid)
    }
    fn main_window(&self, pid: u32) -> Option<isize> {
        platform::windows::find_main_window(pid).map(|hwnd| hwnd as isize)
    }
    fn window_title(&self, hwnd: isize) -> String {
        platform::windows::get_window_title(hwnd as _)
    }
    fn set_window_title(&self, hwnd: isize, title: &str) -> bool {
        platform::windows::set_window_title(hwnd as _, title)
    }
    fn is_hung(&self, hwnd: isize) -> bool {
        platform::windows::is_window_hung(hwnd as _)
    }
}

/// O nome de cada conta para o título da janela, ou nada com a opção
/// desligada. Mesma regra da tela: Alias || Username, mascarado com os nomes
/// ocultos (`General.HideUsernames` + `HiddenNameLetters`, que a tela grava).
#[cfg(target_os = "windows")]
fn window_labels(app: &tauri::AppHandle) -> HashMap<i64, String> {
    let settings = app.state::<SettingsStore>();
    if settings.get_string("General", "ShowAccountNameOnWindow") == "false" {
        return HashMap::new();
    }
    let hidden = settings.get_bool("General", "HideUsernames");
    let letters = settings.get_int("General", "HiddenNameLetters").unwrap_or(0);
    app.state::<AccountStore>()
        .get_all()
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.user_id, window_account_label(&a.alias, &a.username, hidden, letters)))
        .collect()
}

#[cfg(target_os = "windows")]
static CLIENT_HEALTH_MONITOR: LazyLock<Mutex<ClientHealthMonitor>> =
    LazyLock::new(|| Mutex::new(ClientHealthMonitor::new()));

/// Uma passada de verdade: lê o tracker, roda o monitor e publica o retrato.
#[cfg(target_os = "windows")]
fn run_client_health_tick(app: &tauri::AppHandle) -> Vec<HealthNotice> {
    let tracked_processes = platform::windows::tracker().get_all();
    let mut labels = if tracked_processes.is_empty() {
        HashMap::new()
    } else {
        window_labels(app)
    };
    let tracked: Vec<TrackedClient> = tracked_processes
        .into_iter()
        .map(|p| TrackedClient {
            user_id: p.user_id,
            pid: p.pid,
            adopted: p.adopted,
            window_label: labels.remove(&p.user_id),
        })
        .collect();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let Ok(mut monitor) = CLIENT_HEALTH_MONITOR.lock() else {
        return Vec::new();
    };
    let notices = monitor.tick(&WindowsClientHealthOs, &tracked, now_ms);
    if let Ok(mut views) = CLIENT_HEALTH_VIEWS.lock() {
        *views = monitor.views();
    }
    let snapshots = monitor.session_snapshots();
    drop(monitor);
    // Histórico de sessões (ideia 6): grava cada mudança na hora.
    record_session_history(app, &snapshots, now_ms);
    notices
}

/// Liga o monitor em segundo plano (sempre, como a varredura de clientes de
/// fora): sem cliente rastreado, cada passada custa só ler o tracker.
#[cfg(target_os = "windows")]
pub(crate) fn start_client_health_monitor(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let handle = app.clone();
            let notices = tokio::task::spawn_blocking(move || run_client_health_tick(&handle))
                .await
                .unwrap_or_default();
            for notice in &notices {
                emit_health_notice(&app, notice);
            }
            // Reconexão automática da conta que caiu (commands/reconnect.rs).
            reconnect_after_health_tick(&app, &notices);
            // PC acordado durante Modo AFK / Auto Rejoin / reconexão (keep_awake.rs).
            keep_awake_tick(&app);
            // Teto de memória dos clientes do app (commands/memory_ceiling.rs).
            memory_ceiling_tick(&app).await;
            tokio::time::sleep(CLIENT_HEALTH_TICK).await;
        }
    });
}

#[cfg(target_os = "windows")]
fn emit_health_notice(app: &tauri::AppHandle, notice: &HealthNotice) {
    match notice {
        HealthNotice::Dropped {
            user_id,
            drop,
            adopted,
        } => {
            let _ = app.emit(
                "roblox-client-health",
                serde_json::json!({
                    "userId": user_id,
                    "drop": drop,
                    "adopted": adopted,
                }),
            );
            emit_launch_log(app, *user_id, "warn", "client", drop_console_line(drop));
        }
        HealthNotice::NotResponding {
            user_id,
            not_responding,
        } => {
            let _ = app.emit(
                "roblox-client-health",
                serde_json::json!({
                    "userId": user_id,
                    "notResponding": not_responding,
                }),
            );
            let line = if *not_responding {
                "Não respondendo há 30 s"
            } else {
                "Voltou a responder"
            };
            emit_launch_log(app, *user_id, if *not_responding { "warn" } else { "info" }, "client", String::from(line));
        }
        HealthNotice::Recovered { user_id } => {
            let _ = app.emit(
                "roblox-client-health",
                serde_json::json!({
                    "userId": user_id,
                    "drop": serde_json::Value::Null,
                }),
            );
            emit_launch_log(app, *user_id, "info", "client", String::from("Entrou num jogo de novo"));
        }
    }
}

#[cfg(test)]
mod client_log_classifier_tests {
    use super::*;

    // Linhas no formato do cliente de verdade (calibrado com os logs da
    // máquina do dono em 09/10/2026), com ids, IPs e Job IDs trocados por
    // falsos. Nunca colar linha real aqui: ela traz ticket, IP e Job ID.
    const PREFIX: &str = "2026-10-09T01:02:03.456Z,12.345678,1a2b,6";

    fn line(body: &str) -> String {
        format!("{PREFIX} {body}")
    }

    #[test]
    fn joining_a_game_carries_the_place_and_the_job() {
        let event = classify_log_line(&line(
            "[FLog::Output] ! Joining game '00000000-1111-2222-3333-444444444444' place 1234567 at 10.0.0.1",
        ));
        assert_eq!(
            event,
            Some(ClientLogEvent::JoinedGame {
                place_id: Some(1234567),
                job_id: Some("00000000-1111-2222-3333-444444444444".into()),
            })
        );
    }

    #[test]
    fn the_teleport_lines_are_teleports() {
        for body in [
            "[FLog::UgcExperienceController] UgcExperienceController: doTeleport: url ",
            "[FLog::UgcExperienceController] UgcExperienceController: finishTeleportWithJoinScriptPayload: begin",
            "[FLog::SessionTransitionFSM] Teleported.",
        ] {
            assert_eq!(classify_log_line(&line(body)), Some(ClientLogEvent::TeleportStarted), "{body}");
        }
    }

    #[test]
    fn leaving_the_game_is_voluntary() {
        for body in [
            "[FLog::SingleSurfaceApp] leaveUGCGameInternal",
            "[FLog::SessionTransitionFSM] Tearing down.",
            "[FLog::SingleSurfaceApp] returnToLuaApp: (stage:UGCGame).",
        ] {
            assert_eq!(classify_log_line(&line(body)), Some(ClientLogEvent::LeftVoluntarily), "{body}");
        }
    }

    #[test]
    fn reason_285_alone_says_nothing() {
        // Aparece em toda saída e em todo teleporte, nos três formatos.
        for body in [
            "[FLog::Network] Sending disconnect with reason: 285",
            "[FLog::Network] Disconnection Notification. Reason: 285",
            "[DFLog::RbxTransportDummyClient] Disconnected from server for reason: Player: 285 (DisconnectClientInitiated)",
        ] {
            assert_eq!(classify_log_line(&line(body)), None, "{body}");
        }
    }

    #[test]
    fn a_lost_connection_is_a_disconnect_with_its_reason() {
        assert_eq!(
            classify_log_line(&line("[FLog::Network] Disconnection Notification. Reason: 277")),
            Some(ClientLogEvent::Disconnected {
                code: 277,
                reason: DropReason::ConnectionLost
            })
        );
        assert_eq!(
            classify_log_line(&line(
                "[DFLog::RbxTransportDummyClient] Disconnected from server for reason: Server: 279 (DisconnectRaknetErrors)"
            )),
            Some(ClientLogEvent::Disconnected {
                code: 279,
                reason: DropReason::ConnectionLost
            })
        );
    }

    #[test]
    fn the_same_account_joining_elsewhere_has_its_own_reason() {
        for code in [264, 273] {
            assert_eq!(
                classify_log_line(&line(&format!("[FLog::Network] Disconnection Notification. Reason: {code}"))),
                Some(ClientLogEvent::Disconnected {
                    code,
                    reason: DropReason::JoinedElsewhere
                })
            );
        }
    }

    #[test]
    fn a_kick_is_a_kick_and_carries_the_message_when_there_is_one() {
        assert_eq!(
            classify_log_line(&line("[FLog::Network] Sending disconnect with reason: 267")),
            Some(ClientLogEvent::Kicked {
                code: Some(267),
                message: None
            })
        );
        assert_eq!(
            classify_log_line(&line(
                "[FLog::Output] You were kicked from this experience: Server is restarting (Error Code: 267)"
            )),
            Some(ClientLogEvent::Kicked {
                code: Some(267),
                message: Some("Server is restarting".into())
            })
        );
    }

    #[test]
    fn a_shutdown_for_maintenance_is_a_server_shutdown() {
        assert_eq!(
            classify_log_line(&line("[FLog::Network] Disconnection Notification. Reason: 274")),
            Some(ClientLogEvent::ServerShutdown { code: 274 })
        );
    }

    #[test]
    fn noise_that_looks_like_a_disconnect_is_ignored() {
        for body in [
            // Transporte secundário falhando depois de um teleporte: o jogo seguiu.
            "[DFLog::RbxTransportDummyClient] Failed to establish connection to server at 10.0.0.1:1234, reason IO: 12",
            "[FLog::Network] Connection lost: connectMode: Disconnect ASAP, timeMS:123456, connectionTime 1317",
            "[DFLog::SignalRCoreError] ID: 1 Disconnected - stop() called",
            "[FLog::WndProcessCheck] waitForNewPlayerProcess new waiting for mutex result is 0X102, ERROR Unknown error code 0x102.",
            "[FLog::Error] Asset load failed, error code: 403",
            "[DFLog::NetworkClient] Client:Disconnect",
        ] {
            assert_eq!(classify_log_line(&line(body)), None, "{body}");
        }
    }

    #[test]
    fn the_generic_error_code_text_counts_only_in_the_disconnect_range() {
        assert_eq!(
            classify_log_line("Disconnected (Error Code: 277) lost connection"),
            Some(ClientLogEvent::Disconnected {
                code: 277,
                reason: DropReason::ConnectionLost
            })
        );
        assert_eq!(classify_log_line("error code: 503 while fetching"), None);
    }

    /// Sonda dos logs de verdade da máquina, só leitura: conta os eventos e o
    /// estado final de cada log, sem imprimir linha nenhuma (elas têm ticket,
    /// IP e Job ID). Rode à mão com
    /// `cargo test --all-features client_log_real_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn client_log_real_probe() {
        let Some(dir) = std::env::var_os("LOCALAPPDATA")
            .map(|base| std::path::PathBuf::from(base).join("Roblox").join("logs"))
        else {
            return;
        };
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        let mut finals: std::collections::BTreeMap<String, usize> = Default::default();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.contains("_Player_") || name.contains("CrashHandler") {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            let mut session = ClientLogSession::default();
            for line in text.lines() {
                if let Some(event) = classify_log_line(line) {
                    let key = match &event {
                        ClientLogEvent::JoinedGame { .. } => "joined".to_string(),
                        ClientLogEvent::TeleportStarted => "teleport".to_string(),
                        ClientLogEvent::LeftVoluntarily => "left".to_string(),
                        ClientLogEvent::Disconnected { code, .. } => format!("disconnected {code}"),
                        ClientLogEvent::Kicked { code, .. } => format!("kicked {code:?}"),
                        ClientLogEvent::ServerShutdown { code } => format!("shutdown {code}"),
                    };
                    *counts.entry(key).or_default() += 1;
                    session.apply(&event, 0);
                }
            }
            session.settle(i64::MAX / 2);
            let state = match session.current_drop() {
                Some(drop) => format!("drop {:?} {:?}", drop.kind, drop.code),
                None if session.ended_without_leaving() => "ended without leaving".to_string(),
                None => "left".to_string(),
            };
            *finals.entry(state).or_default() += 1;
        }
        println!("events: {counts:#?}");
        println!("final state per log: {finals:#?}");
    }

    #[test]
    fn a_long_kick_message_is_cut() {
        let long = "x".repeat(500);
        let Some(ClientLogEvent::Kicked { message: Some(m), .. }) =
            classify_log_line(&format!("You were kicked from this experience: {long}"))
        else {
            panic!("expected a kick");
        };
        assert_eq!(m.chars().count(), KICK_MESSAGE_MAX_CHARS);
    }
}

#[cfg(test)]
mod client_log_session_tests {
    use super::*;

    const T0: i64 = 1_791_000_000_000;

    fn joined() -> ClientLogEvent {
        ClientLogEvent::JoinedGame {
            place_id: Some(1),
            job_id: Some("job".into()),
        }
    }

    fn lost() -> ClientLogEvent {
        ClientLogEvent::Disconnected {
            code: 277,
            reason: DropReason::ConnectionLost,
        }
    }

    #[test]
    fn a_drop_with_no_teleport_shows_at_once() {
        let mut s = ClientLogSession::default();
        s.apply(&joined(), T0);
        s.apply(&lost(), T0 + 1_000);
        s.settle(T0 + 1_000);
        let drop = s.current_drop().expect("dropped");
        assert_eq!(drop.kind, DropKind::Disconnected);
        assert_eq!(drop.reason, Some(DropReason::ConnectionLost));
        assert_eq!(drop.code, Some(277));
    }

    #[test]
    fn a_drop_right_after_a_teleport_waits_the_grace_and_a_join_cancels_it() {
        let mut s = ClientLogSession::default();
        s.apply(&joined(), T0);
        s.apply(&ClientLogEvent::TeleportStarted, T0 + 1_000);
        s.apply(&lost(), T0 + 1_500);
        s.settle(T0 + 3_000);
        assert!(s.current_drop().is_none(), "still inside the teleport grace");
        s.apply(&joined(), T0 + 4_000);
        s.settle(T0 + 20_000);
        assert!(s.current_drop().is_none(), "the teleport landed: not a drop");
    }

    #[test]
    fn a_drop_after_a_teleport_that_never_lands_shows_after_the_grace() {
        let mut s = ClientLogSession::default();
        s.apply(&ClientLogEvent::TeleportStarted, T0);
        s.apply(&lost(), T0 + 500);
        s.settle(T0 + 500 + TELEPORT_GRACE_MS - 1);
        assert!(s.current_drop().is_none());
        s.settle(T0 + 500 + TELEPORT_GRACE_MS);
        assert!(s.current_drop().is_some());
    }

    #[test]
    fn a_client_that_never_joined_a_game_did_not_crash_when_it_ends() {
        let mut s = ClientLogSession::default();
        assert!(!s.ended_without_leaving());
        s.apply(&joined(), T0);
        assert!(s.ended_without_leaving());
    }

    #[test]
    fn joining_again_clears_the_drop() {
        let mut s = ClientLogSession::default();
        s.apply(&lost(), T0);
        s.settle(T0);
        assert!(s.current_drop().is_some());
        s.apply(&joined(), T0 + 30_000);
        assert!(s.current_drop().is_none());
    }

    #[test]
    fn what_comes_after_leaving_is_not_a_drop() {
        let mut s = ClientLogSession::default();
        s.apply(&joined(), T0);
        s.apply(&ClientLogEvent::LeftVoluntarily, T0 + 1_000);
        s.apply(&lost(), T0 + 1_100);
        s.settle(T0 + 2_000);
        assert!(s.current_drop().is_none());
        assert!(!s.ended_without_leaving());
    }

    #[test]
    fn the_kick_message_on_a_later_line_joins_the_kick() {
        let mut s = ClientLogSession::default();
        s.apply(
            &ClientLogEvent::Kicked {
                code: Some(267),
                message: None,
            },
            T0,
        );
        s.settle(T0);
        s.apply(
            &ClientLogEvent::Kicked {
                code: Some(267),
                message: Some("bye".into()),
            },
            T0 + 100,
        );
        let drop = s.current_drop().unwrap();
        assert_eq!(drop.kind, DropKind::Kicked);
        assert_eq!(drop.message.as_deref(), Some("bye"));
        assert_eq!(drop.since_ms, T0, "the first line marks when it dropped");
    }

    #[test]
    fn in_game_means_joined_and_nothing_happened_since() {
        let mut s = ClientLogSession::default();
        assert!(!s.in_game(), "never joined");
        s.apply(&joined(), T0);
        assert!(s.in_game());
        s.apply(&lost(), T0 + 1_000);
        assert!(!s.in_game(), "a pending drop is not in game");
        s.settle(T0 + 1_000);
        assert!(!s.in_game());
        s.apply(&joined(), T0 + 30_000);
        assert!(s.in_game(), "joined again");
        s.apply(&ClientLogEvent::LeftVoluntarily, T0 + 40_000);
        assert!(!s.in_game(), "left");
    }

    #[test]
    fn the_destination_is_the_last_game_joined() {
        let mut s = ClientLogSession::default();
        assert_eq!(s.destination(), None);
        s.apply(
            &ClientLogEvent::JoinedGame {
                place_id: Some(10),
                job_id: Some("a".into()),
            },
            T0,
        );
        s.apply(
            &ClientLogEvent::JoinedGame {
                place_id: Some(20),
                job_id: None,
            },
            T0 + 1_000,
        );
        assert_eq!(
            s.destination(),
            Some(&JoinedDestination {
                place_id: 20,
                job_id: None
            })
        );
        // Uma linha sem place não apaga o destino conhecido.
        s.apply(
            &ClientLogEvent::JoinedGame {
                place_id: None,
                job_id: Some("b".into()),
            },
            T0 + 2_000,
        );
        assert_eq!(s.destination().map(|d| d.place_id), Some(20));
        // A queda também não: é para lá que a reconexão volta.
        s.apply(&lost(), T0 + 3_000);
        s.settle(T0 + 3_000);
        assert_eq!(s.destination().map(|d| d.place_id), Some(20));
    }

    #[test]
    fn replaying_a_whole_log_ends_in_its_last_state() {
        // O monitor lê o log inteiro na 1ª vez (app reaberto com o cliente já
        // em jogo): queda antiga seguida de join não aparece.
        let log = "\
x [FLog::Output] ! Joining game 'a' place 1 at 10.0.0.1
x [FLog::Network] Disconnection Notification. Reason: 277
x [FLog::Output] ! Joining game 'b' place 1 at 10.0.0.2
x [FLog::UgcExperienceController] UgcExperienceController: doTeleport: url
x [DFLog::NetworkClient] Client:Disconnect
x [FLog::SessionTransitionFSM] Teleported.
x [FLog::Output] ! Joining game 'c' place 2 at 10.0.0.3
x [DFLog::RbxTransportDummyClient] Disconnected from server for reason: Player: 285 (DisconnectClientInitiated)
";
        let mut s = ClientLogSession::default();
        s.absorb(log, T0);
        s.settle(T0 + 60_000);
        assert!(s.current_drop().is_none());
        assert!(s.ended_without_leaving());
    }
}

#[cfg(test)]
mod client_health_monitor_tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    const T0: i64 = 1_791_000_000_000;

    #[derive(Default)]
    struct FakeOs {
        alive: RefCell<HashSet<u32>>,
        logs: RefCell<HashMap<u32, (PathBuf, String)>>,
        terminated: RefCell<HashSet<u32>>,
        locate_calls: RefCell<u32>,
        /// PID → título da janela principal (sem entrada: sem janela).
        titles: RefCell<HashMap<u32, String>>,
        set_calls: RefCell<u32>,
        hung: RefCell<HashSet<u32>>,
    }

    impl FakeOs {
        fn with_client(pid: u32, log: &str) -> Self {
            let os = Self::default();
            os.alive.borrow_mut().insert(pid);
            os.logs
                .borrow_mut()
                .insert(pid, (PathBuf::from(format!("C:/logs/{pid}.log")), log.to_string()));
            os
        }
        fn append(&self, pid: u32, more: &str) {
            self.logs.borrow_mut().get_mut(&pid).unwrap().1.push_str(more);
        }
    }

    impl ClientHealthOs for FakeOs {
        fn alive_pids(&self) -> HashSet<u32> {
            self.alive.borrow().clone()
        }
        fn locate_logs(&self, pids: &[u32], taken: &HashSet<PathBuf>) -> HashMap<u32, PathBuf> {
            *self.locate_calls.borrow_mut() += 1;
            self.logs
                .borrow()
                .iter()
                .filter(|(pid, (path, _))| pids.contains(pid) && !taken.contains(path))
                .map(|(pid, (path, _))| (*pid, path.clone()))
                .collect()
        }
        fn read_log(&self, path: &Path, offset: u64, max: u64) -> Option<Vec<u8>> {
            let logs = self.logs.borrow();
            let (_, content) = logs.values().find(|(p, _)| p == path)?;
            let bytes = content.as_bytes();
            let start = (offset as usize).min(bytes.len());
            let end = (start + max as usize).min(bytes.len());
            Some(bytes[start..end].to_vec())
        }
        fn terminated_by_app(&self, pid: u32) -> bool {
            self.terminated.borrow().contains(&pid)
        }
        fn main_window(&self, pid: u32) -> Option<isize> {
            self.titles.borrow().contains_key(&pid).then_some(pid as isize)
        }
        fn window_title(&self, hwnd: isize) -> String {
            self.titles.borrow().get(&(hwnd as u32)).cloned().unwrap_or_default()
        }
        fn set_window_title(&self, hwnd: isize, title: &str) -> bool {
            *self.set_calls.borrow_mut() += 1;
            self.titles.borrow_mut().insert(hwnd as u32, title.to_string());
            true
        }
        fn is_hung(&self, hwnd: isize) -> bool {
            self.hung.borrow().contains(&(hwnd as u32))
        }
    }

    const JOIN: &str = "x [FLog::Output] ! Joining game 'a' place 1 at 10.0.0.1\n";
    const LOST: &str = "x [FLog::Network] Disconnection Notification. Reason: 277\n";
    const LEAVE: &str = "x [FLog::SingleSurfaceApp] leaveUGCGameInternal\n";

    fn ours(user_id: i64, pid: u32) -> TrackedClient {
        TrackedClient {
            user_id,
            pid,
            adopted: false,
            window_label: None,
        }
    }

    fn named(user_id: i64, pid: u32, label: &str) -> TrackedClient {
        TrackedClient {
            window_label: Some(label.to_string()),
            ..ours(user_id, pid)
        }
    }

    #[test]
    fn a_drop_in_the_log_becomes_one_notice_and_shows_in_the_view() {
        let os = FakeOs::with_client(100, JOIN);
        let mut monitor = ClientHealthMonitor::new();
        assert!(monitor.tick(&os, &[ours(1, 100)], T0).is_empty());

        os.append(100, LOST);
        let notices = monitor.tick(&os, &[ours(1, 100)], T0 + 2_000);
        assert_eq!(notices.len(), 1);
        assert!(matches!(&notices[0], HealthNotice::Dropped { user_id: 1, drop, .. } if drop.code == Some(277)));
        assert!(monitor.tick(&os, &[ours(1, 100)], T0 + 4_000).is_empty(), "reported once");

        let view = monitor.views().remove(&1).unwrap();
        assert!(view.log_found);
        assert_eq!(view.drop.unwrap().reason, Some(DropReason::ConnectionLost));
    }

    #[test]
    fn the_view_says_in_game_and_where_until_the_client_ends() {
        let os = FakeOs::with_client(100, "");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        let view = &monitor.views()[&1];
        assert!(view.log_found);
        assert!(!view.in_game, "still loading");
        assert!(!view.exited);

        os.append(100, JOIN);
        monitor.tick(&os, &[ours(1, 100)], T0 + 2_000);
        let view = &monitor.views()[&1];
        assert!(view.in_game);
        assert_eq!(
            view.destination,
            Some(JoinedDestination {
                place_id: 1,
                job_id: Some("a".into())
            })
        );

        os.alive.borrow_mut().clear();
        monitor.tick(&os, &[ours(1, 100)], T0 + 4_000);
        let view = &monitor.views()[&1];
        assert!(!view.in_game, "the process is gone");
        assert!(view.exited);
    }

    #[test]
    fn rejoining_clears_the_drop_with_a_notice() {
        let os = FakeOs::with_client(100, &format!("{JOIN}{LOST}"));
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        os.append(100, JOIN);
        let notices = monitor.tick(&os, &[ours(1, 100)], T0 + 2_000);
        assert_eq!(notices, vec![HealthNotice::Recovered { user_id: 1 }]);
        assert!(monitor.views()[&1].drop.is_none());
    }

    #[test]
    fn an_adopted_client_is_watched_too_and_flagged_as_adopted() {
        // Só aviso na tela: quem decide fechar é o Watcher, que ignora adotados.
        let os = FakeOs::with_client(100, &format!("{JOIN}{LOST}"));
        let mut monitor = ClientHealthMonitor::new();
        let notices = monitor.tick(
            &os,
            &[TrackedClient {
                user_id: 1,
                pid: 100,
                adopted: true,
                window_label: None,
            }],
            T0,
        );
        assert!(matches!(&notices[0], HealthNotice::Dropped { adopted: true, .. }));
    }

    #[test]
    fn a_process_that_ends_without_leaving_crashed() {
        let os = FakeOs::with_client(100, JOIN);
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        os.alive.borrow_mut().clear();
        let notices = monitor.tick(&os, &[ours(1, 100)], T0 + 2_000);
        assert!(matches!(&notices[0], HealthNotice::Dropped { drop, .. } if drop.kind == DropKind::Crashed));
    }

    #[test]
    fn closing_the_window_or_the_app_closing_it_is_not_a_crash() {
        let os = FakeOs::with_client(100, &format!("{JOIN}{LEAVE}"));
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        os.alive.borrow_mut().clear();
        assert!(monitor.tick(&os, &[ours(1, 100)], T0 + 2_000).is_empty());

        let os = FakeOs::with_client(200, JOIN);
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(2, 200)], T0);
        os.alive.borrow_mut().clear();
        os.terminated.borrow_mut().insert(200);
        assert!(monitor.tick(&os, &[ours(2, 200)], T0 + 2_000).is_empty());
    }

    #[test]
    fn without_a_log_nothing_is_guessed() {
        let os = FakeOs::default();
        os.alive.borrow_mut().insert(100);
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        assert!(!monitor.views()[&1].log_found);
        os.alive.borrow_mut().clear();
        assert!(monitor.tick(&os, &[ours(1, 100)], T0 + 2_000).is_empty());
    }

    #[test]
    fn a_missing_log_is_looked_for_again_only_after_the_retry_delay() {
        let os = FakeOs::default();
        os.alive.borrow_mut().insert(100);
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        monitor.tick(&os, &[ours(1, 100)], T0 + 2_000);
        assert_eq!(*os.locate_calls.borrow(), 1);
        monitor.tick(&os, &[ours(1, 100)], T0 + CLIENT_LOG_RETRY_MS);
        assert_eq!(*os.locate_calls.borrow(), 2);
    }

    #[test]
    fn a_new_pid_for_the_account_starts_clean() {
        let os = FakeOs::with_client(100, &format!("{JOIN}{LOST}"));
        os.alive.borrow_mut().insert(300);
        os.logs
            .borrow_mut()
            .insert(300, (PathBuf::from("C:/logs/300.log"), JOIN.to_string()));
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        assert!(monitor.views()[&1].drop.is_some());
        monitor.tick(&os, &[ours(1, 300)], T0 + 2_000);
        let view = &monitor.views()[&1];
        assert_eq!(view.pid, 300);
        assert!(view.drop.is_none());
    }

    #[test]
    fn only_new_bytes_are_read_and_a_half_line_waits() {
        let os = FakeOs::with_client(100, JOIN);
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[ours(1, 100)], T0);
        // Linha ainda sendo escrita: não conta até ganhar o fim de linha.
        os.append(100, "x [FLog::Network] Disconnection Notification. Reason: 27");
        assert!(monitor.tick(&os, &[ours(1, 100)], T0 + 2_000).is_empty());
        os.append(100, "7\n");
        let notices = monitor.tick(&os, &[ours(1, 100)], T0 + 4_000);
        assert!(matches!(&notices[0], HealthNotice::Dropped { drop, .. } if drop.code == Some(277)));
    }

    #[test]
    fn the_console_line_says_why_in_plain_words() {
        let drop = |kind, reason, code, message: Option<&str>| ClientDrop {
            kind,
            reason,
            code,
            message: message.map(String::from),
            since_ms: T0,
        };
        assert_eq!(
            drop_console_line(&drop(DropKind::Disconnected, Some(DropReason::ConnectionLost), Some(277), None)),
            "Caiu: perdeu a conexão (código 277)"
        );
        assert_eq!(
            drop_console_line(&drop(DropKind::Kicked, None, Some(267), Some("bye"))),
            "Foi expulso: bye"
        );
        assert_eq!(
            drop_console_line(&drop(DropKind::ServerShutdown, None, Some(274), None)),
            "O servidor fechou (código 274)"
        );
    }

    // ---- nome da conta no título ------------------------------------------

    fn with_window(pid: u32, title: &str) -> FakeOs {
        let os = FakeOs::with_client(pid, JOIN);
        os.titles.borrow_mut().insert(pid, title.to_string());
        os
    }

    #[test]
    fn the_window_gets_the_account_name_and_keeps_it() {
        let os = with_window(100, "Roblox");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        // O nome vem primeiro: a barra de tarefas mostra o começo do título.
        assert_eq!(os.titles.borrow()[&100], "Main — Roblox");
        assert_eq!(monitor.views()[&1].window_title.as_deref(), Some("Main — Roblox"));

        // Nada mudou: não escreve de novo.
        monitor.tick(&os, &[named(1, 100, "Main")], T0 + 2_000);
        assert_eq!(*os.set_calls.borrow(), 1);

        // O Roblox pôs "Roblox" de volta (teleporte): põe o nome de novo.
        os.titles.borrow_mut().insert(100, "Roblox".into());
        monitor.tick(&os, &[named(1, 100, "Main")], T0 + 4_000);
        assert_eq!(os.titles.borrow()[&100], "Main — Roblox");
    }

    #[test]
    fn turning_the_option_off_gives_the_title_back() {
        let os = with_window(100, "Roblox");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        monitor.tick(&os, &[ours(1, 100)], T0 + 2_000);
        assert_eq!(os.titles.borrow()[&100], "Roblox");
        assert_eq!(monitor.views()[&1].window_title, None);
    }

    #[test]
    fn hiding_names_swaps_the_title_for_the_masked_one() {
        let os = with_window(100, "Roblox");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        let masked = window_account_label("Main", "annabelle", true, 1);
        monitor.tick(&os, &[named(1, 100, &masked)], T0 + 2_000);
        assert_eq!(os.titles.borrow()[&100], "M******** — Roblox");
    }

    #[test]
    fn a_title_roblox_set_to_something_else_is_left_alone() {
        // Erro ou beta no título: as regras do Watcher precisam ver o texto.
        let os = with_window(100, "Roblox Beta");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        assert_eq!(os.titles.borrow()[&100], "Roblox Beta");
        assert_eq!(*os.set_calls.borrow(), 0);
    }

    #[test]
    fn after_the_app_reopens_its_own_title_is_recognized() {
        // O app fechou e abriu de novo: o título já tem o nome, o monitor
        // nasce sem memória e reconhece o título como seu.
        let os = with_window(100, "Main — Roblox");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        assert_eq!(*os.set_calls.borrow(), 0);
        assert_eq!(monitor.views()[&1].window_title.as_deref(), Some("Main — Roblox"));
    }

    #[test]
    fn a_title_from_the_previous_version_is_switched_to_the_new_order() {
        // A versão anterior punha "Roblox — <conta>". Depois de atualizar, a
        // janela que ficou aberta é reconhecida como nossa (não é erro nem
        // título estranho) e passa para "<conta> — Roblox".
        let os = with_window(100, "Roblox — Main");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        assert_eq!(os.titles.borrow()[&100], "Main — Roblox");
        assert_eq!(*os.set_calls.borrow(), 1);
        assert_eq!(monitor.views()[&1].window_title.as_deref(), Some("Main — Roblox"));
    }

    #[test]
    fn a_previous_version_title_with_another_name_is_left_alone() {
        // Só o nome que o app poria agora é reconhecido; outro texto depois de
        // "Roblox — " pode ser o Roblox mostrando um erro.
        let os = with_window(100, "Roblox — Error Code: 429");
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        assert_eq!(os.titles.borrow()[&100], "Roblox — Error Code: 429");
        assert_eq!(*os.set_calls.borrow(), 0);
    }

    // ---- "Não respondendo" --------------------------------------------------

    #[test]
    fn a_window_hung_for_30_seconds_is_not_responding_once() {
        let os = with_window(100, "Roblox");
        os.hung.borrow_mut().insert(100);
        let mut monitor = ClientHealthMonitor::new();
        assert!(monitor.tick(&os, &[ours(1, 100)], T0).is_empty());
        assert!(monitor.tick(&os, &[ours(1, 100)], T0 + HUNG_THRESHOLD_MS - 1).is_empty());
        assert!(!monitor.views()[&1].not_responding);

        let notices = monitor.tick(&os, &[ours(1, 100)], T0 + HUNG_THRESHOLD_MS);
        assert_eq!(
            notices,
            vec![HealthNotice::NotResponding {
                user_id: 1,
                not_responding: true
            }]
        );
        assert!(monitor.views()[&1].not_responding);
        assert!(monitor.tick(&os, &[ours(1, 100)], T0 + HUNG_THRESHOLD_MS + 2_000).is_empty());

        os.hung.borrow_mut().clear();
        let notices = monitor.tick(&os, &[ours(1, 100)], T0 + HUNG_THRESHOLD_MS + 4_000);
        assert_eq!(
            notices,
            vec![HealthNotice::NotResponding {
                user_id: 1,
                not_responding: false
            }]
        );
        assert!(!monitor.views()[&1].not_responding);
    }

    #[test]
    fn a_short_freeze_resets_the_count() {
        let os = with_window(100, "Roblox");
        let mut monitor = ClientHealthMonitor::new();
        os.hung.borrow_mut().insert(100);
        monitor.tick(&os, &[ours(1, 100)], T0);
        os.hung.borrow_mut().clear();
        monitor.tick(&os, &[ours(1, 100)], T0 + 20_000);
        os.hung.borrow_mut().insert(100);
        monitor.tick(&os, &[ours(1, 100)], T0 + 22_000);
        monitor.tick(&os, &[ours(1, 100)], T0 + 40_000);
        assert!(!monitor.views()[&1].not_responding, "only 18 s since it froze again");
    }

    #[test]
    fn a_website_client_is_never_flagged_as_not_responding() {
        let os = with_window(100, "Roblox");
        os.hung.borrow_mut().insert(100);
        let site = TrackedClient {
            adopted: true,
            ..ours(1, 100)
        };
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[site.clone()], T0);
        assert!(monitor.tick(&os, &[site], T0 + 60_000).is_empty());
        assert!(!monitor.views()[&1].not_responding);
    }

    #[test]
    fn a_hung_window_is_not_renamed() {
        let os = with_window(100, "Roblox");
        os.hung.borrow_mut().insert(100);
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        assert_eq!(*os.set_calls.borrow(), 0);
        os.hung.borrow_mut().clear();
        monitor.tick(&os, &[named(1, 100, "Main")], T0 + 2_000);
        assert_eq!(os.titles.borrow()[&100], "Main — Roblox");
    }

    #[test]
    fn a_client_without_a_window_is_skipped() {
        let os = FakeOs::with_client(100, JOIN);
        let mut monitor = ClientHealthMonitor::new();
        monitor.tick(&os, &[named(1, 100, "Main")], T0);
        assert_eq!(*os.set_calls.borrow(), 0);
    }
}

#[cfg(test)]
mod client_window_title_tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct MaskCase {
        name: String,
        hidden: bool,
        letters: i64,
        expected: String,
    }

    #[test]
    fn masking_matches_the_app_screen_case_by_case() {
        // Os mesmos casos do src/utils/accountName.test.ts.
        let cases: Vec<MaskCase> =
            serde_json::from_str(include_str!("../../../src/utils/accountNameCases.json")).unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            assert_eq!(
                mask_account_name(&case.name, case.hidden, case.letters),
                case.expected,
                "{:?} hidden={} letters={}",
                case.name,
                case.hidden,
                case.letters
            );
        }
    }

    #[test]
    fn the_label_is_the_alias_or_the_username() {
        assert_eq!(window_account_label("Main", "annabelle", false, 0), "Main");
        assert_eq!(window_account_label("", "annabelle", false, 0), "annabelle");
        assert_eq!(window_account_label("", "annabelle", true, 3), "ann********");
        assert_eq!(window_account_label("", "annabelle", true, 0), "************");
    }

    #[test]
    fn the_account_name_comes_first() {
        assert_eq!(client_window_title("Main"), "Main — Roblox");
        assert_eq!(client_window_title("M********"), "M******** — Roblox");
    }

    #[test]
    fn the_effective_title_also_recognizes_the_previous_order() {
        let ours = client_window_title("Main");
        // O título que a versão anterior punha, com o mesmo nome.
        assert_eq!(effective_client_title("Roblox — Main", None, Some(&ours)), "Roblox");
        assert_eq!(effective_client_title("Roblox — Main", Some("Roblox — Main"), Some(&ours)), "Roblox");
        // Outro nome (ou erro) no formato antigo não é nosso.
        assert_eq!(effective_client_title("Roblox — Other", None, Some(&ours)), "Roblox — Other");
        assert_eq!(effective_client_title("Roblox — Main", None, None), "Roblox — Main");
        // O plano troca o antigo pelo novo.
        assert_eq!(plan_window_title("Roblox — Main", None, Some(&ours)), Some(ours.clone()));
    }

    #[test]
    fn the_effective_title_hides_only_our_own_name() {
        let ours = client_window_title("No Connection Bob");
        // O nome da conta nunca chega às regras do Watcher.
        assert_eq!(effective_client_title(&ours, Some(&ours), None), "Roblox");
        assert_eq!(effective_client_title(&ours, None, Some(&ours)), "Roblox");
        // Título que o app não pôs passa como está.
        assert_eq!(effective_client_title("Roblox — Error Code: 429", None, Some(&ours)), "Roblox — Error Code: 429");
        assert_eq!(effective_client_title("Roblox Beta", None, None), "Roblox Beta");
        assert_eq!(effective_client_title("", None, Some("")), "");
    }

    #[test]
    fn the_plan_only_touches_the_normal_title() {
        let main = client_window_title("Main");
        assert_eq!(plan_window_title("Roblox", None, Some(&main)), Some(main.clone()));
        assert_eq!(plan_window_title(&main, Some(&main), Some(&main)), None);
        assert_eq!(plan_window_title(&main, Some(&main), None), Some("Roblox".into()));
        assert_eq!(plan_window_title("Roblox", None, None), None);
        assert_eq!(plan_window_title("Roblox Beta", None, Some(&main)), None);
        assert_eq!(plan_window_title("Disconnected", None, Some(&main)), None);
    }
}
