// Reconexão automática: quando o monitor de quedas (client_health.rs) diz que
// a conta caiu, relança **só aquela conta**, no mesmo destino, com espera
// crescente. Ver docs/features/watcher.md ("Reconexão automática").
//
// Regras que não podem se perder:
// - Só cliente que o app abriu. Cliente do site (adotado) nunca é fechado nem
//   relançado; o app também nunca fecha cliente de outra conta: o único
//   cliente fechado aqui é o da própria conta que caiu, e só se ele não
//   estiver em jogo.
// - Para (e não tenta de novo) quando a conta entrou em outro lugar (as duas
//   sessões se derrubariam em laço), quando a pessoa fechou o cliente, quando
//   a conta está banida e quando a sessão expirou.
// - Cada tentativa pega um ticket novo pelo launch normal, **sem** renovar a
//   sessão (`allow_session_refresh = false`): o refresh desloga a conta de
//   todo lugar, e aqui ninguém está olhando.
// - Auto Rejoin manda na conta que ele gerencia: a reconexão fica de fora.
// - Nada é gravado em disco: ao reabrir o app, nada é retomado sozinho.
//
// A máquina de estados (`ReconnectBook`) é pura e testada abaixo; o laço do
// Windows só junta as peças, a cada passada do monitor de quedas.

/// Espera antes de cada tentativa: a 1ª depois de 10 s, a 5ª (e o teto) 5 min.
pub const RECONNECT_BACKOFF_SECS: [i64; 5] = [10, 30, 60, 120, 300];
/// Tentativas seguidas que não ficam no jogo antes de desistir.
pub const RECONNECT_MAX_ATTEMPTS: u32 = 5;
/// Tempo no jogo para a reconexão contar como bem-sucedida (zera as tentativas).
pub const RECONNECT_STABLE_MS: i64 = 120_000;
/// Relançou e não entrou no jogo neste tempo (com o log achado): tentativa falhou.
pub const RECONNECT_JOIN_TIMEOUT_MS: i64 = 120_000;
/// Sem internet: confere de novo depois disto, sem gastar tentativa.
pub const RECONNECT_OFFLINE_RETRY_MS: i64 = 10_000;
/// A fila de launch está ocupada (outro launch rodando): tenta de novo depois disto.
pub const RECONNECT_BUSY_RETRY_MS: i64 = 5_000;

// O erro de uma tentativa, como vai para a tela (`error` da entrada). Em inglês
// porque a tradução é do front (`reconnectErrorText` em
// src/utils/autoReconnect.ts, que lê esta lista no teste): frase nova aqui
// precisa entrar lá, senão aparece crua na tela em português.
/// O cliente relançado foi fechado pelo próprio app antes de ficar no jogo.
pub const RECONNECT_ERROR_CLIENT_CLOSED: &str = "The client was closed";
/// Relançado, não entrou no jogo em `RECONNECT_JOIN_TIMEOUT_MS`.
pub const RECONNECT_ERROR_NOT_IN_GAME: &str = "It did not get into the game in 2 minutes";
/// O cliente caído da própria conta não fechou antes do relaunch.
#[allow(dead_code)]
pub const RECONNECT_ERROR_OLD_CLIENT_OPEN: &str = "The old client did not close";
/// O launch terminou sem cliente do app aberto.
#[allow(dead_code)]
pub const RECONNECT_ERROR_DID_NOT_START: &str = "The Roblox client did not start";

/// Espera antes da tentativa `attempt` (1, 2, ...).
pub fn reconnect_backoff_ms(attempt: u32) -> i64 {
    let index = (attempt.max(1) - 1).min(RECONNECT_BACKOFF_SECS.len() as u32 - 1) as usize;
    RECONNECT_BACKOFF_SECS[index] * 1_000
}

/// Por que a reconexão parou de vez.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReconnectStopReason {
    /// A mesma conta entrou em outro lugar: relançar faria as duas sessões se
    /// derrubarem em laço.
    JoinedElsewhere,
    /// A pessoa fechou o cliente (ou saiu do jogo) por conta própria.
    ClosedByUser,
    /// O Roblox baniu ou encerrou a conta.
    Banned,
    /// O Roblox invalidou a sessão: precisa entrar de novo na conta.
    SessionExpired,
    /// Nem o log nem o launch dizem para onde voltar.
    NoDestination,
    /// A conta está aberta num cliente de fora do app (site): o app não o fecha.
    OpenedOutsideApp,
}

/// Para onde o app mandou a conta no último launch (launch.rs grava).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedTarget {
    pub place_id: i64,
    /// O Job ID como foi pedido (pode ser `vip:<código>` ou um link privado).
    pub job_id: String,
    pub launch_data: String,
    pub join_vip: bool,
    pub link_code: String,
    /// O launch foi para um servidor privado/VIP.
    pub private: bool,
}

/// O destino da reconexão, nos termos do launch normal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectTarget {
    pub place_id: i64,
    pub job_id: String,
    pub launch_data: String,
    pub join_vip: bool,
    pub link_code: String,
}

/// Para onde voltar. Servidor privado: os mesmos dados que o app usou no
/// launch (nunca um link inventado). Público: o place e o Job ID do último
/// "Joining game" do log — sem o Job ID quando o servidor fechou (ele não
/// existe mais). Sem log, o que o app pediu no launch.
pub fn reconnect_target(
    launched: Option<&LaunchedTarget>,
    seen: Option<&JoinedDestination>,
    kind: DropKind,
) -> Option<ReconnectTarget> {
    let server_gone = kind == DropKind::ServerShutdown;
    if let Some(l) = launched.filter(|l| l.private) {
        return Some(ReconnectTarget {
            place_id: l.place_id,
            job_id: l.job_id.clone(),
            launch_data: l.launch_data.clone(),
            join_vip: l.join_vip,
            link_code: l.link_code.clone(),
        });
    }
    if let Some(seen) = seen {
        // O launch data é do jogo que o app abriu: só vale no mesmo place.
        let launch_data = launched
            .filter(|l| l.place_id == seen.place_id)
            .map(|l| l.launch_data.clone())
            .unwrap_or_default();
        return Some(ReconnectTarget {
            place_id: seen.place_id,
            job_id: if server_gone {
                String::new()
            } else {
                seen.job_id.clone().unwrap_or_default()
            },
            launch_data,
            join_vip: false,
            link_code: String::new(),
        });
    }
    let l = launched?;
    Some(ReconnectTarget {
        place_id: l.place_id,
        job_id: if server_gone { String::new() } else { l.job_id.clone() },
        launch_data: l.launch_data.clone(),
        join_vip: false,
        link_code: String::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReconnectPhase {
    /// Esperando a hora da próxima tentativa.
    Waiting { at_ms: i64 },
    /// Sem internet: confere de novo em `at_ms`, sem gastar tentativa.
    WaitingForInternet { at_ms: i64 },
    /// Tentativa em andamento (checagens + launch).
    Launching,
    /// Relançou: conferindo se fica no jogo por 2 min.
    Checking {
        launched_at_ms: i64,
        in_game_since_ms: Option<i64>,
    },
    GaveUp,
    Stopped(ReconnectStopReason),
}

impl ReconnectPhase {
    fn is_final(&self) -> bool {
        matches!(self, Self::GaveUp | Self::Stopped(_))
    }
}

#[derive(Debug, Clone)]
struct ReconnectEntry {
    phase: ReconnectPhase,
    /// A tentativa que espera, roda ou está sendo conferida (1, 2, ...).
    attempt: u32,
    /// A queda que começou (ou recomeçou) a reconexão.
    drop: ClientDrop,
    /// O cliente que o estado acompanha: o que caiu, enquanto espera; o
    /// relançado, enquanto confere.
    watched_pid: Option<u32>,
    /// O cliente que caiu continuou aberto (queda de rede, kick): se a pessoa
    /// o fechar enquanto espera, a reconexão para.
    watched_was_open: bool,
    target: Option<ReconnectTarget>,
    error: Option<String>,
}

/// Uma queda que o monitor reportou, com o que o laço sabe da conta.
#[derive(Debug, Clone)]
pub struct ReconnectDrop {
    pub user_id: i64,
    pub pid: Option<u32>,
    pub adopted: bool,
    pub drop: ClientDrop,
    /// A opção está ligada para esta conta.
    pub enabled: bool,
    /// O Auto Rejoin gerencia esta conta agora.
    pub auto_rejoin: bool,
    pub target: Option<ReconnectTarget>,
}

/// O que o laço vê da conta numa passada (dublê nos testes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconnectObservation {
    pub enabled: bool,
    pub auto_rejoin: bool,
    /// O cliente que o tracker tem para a conta agora.
    pub tracked_pid: Option<u32>,
    pub tracked_adopted: bool,
    /// Esse cliente está num jogo (log).
    pub tracked_in_game: bool,
    /// O cliente acompanhado terminou.
    pub watched_exited: bool,
    /// Foi o app que o fechou (Fechar, Watcher, a própria reconexão…).
    pub watched_closed_by_app: bool,
    /// O cliente acompanhado está num jogo (log).
    pub watched_in_game: bool,
    /// O log do cliente acompanhado foi achado.
    pub watched_log_found: bool,
    /// A queda que o monitor mostra agora para o cliente acompanhado (reserva
    /// para um aviso que chegou enquanto a tentativa rodava).
    pub watched_drop: Option<ClientDrop>,
}

/// O que fazer agora.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectAction {
    Attempt {
        user_id: i64,
        attempt: u32,
        target: ReconnectTarget,
    },
}

/// Como terminou uma tentativa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// O cliente novo abriu (PID detectado).
    Launched { pid: u32 },
    /// Sem internet: espera ela voltar sem gastar tentativa.
    NoInternet,
    /// Outro launch está rodando: tenta logo depois.
    Busy,
    /// O cliente já voltou ao jogo sozinho (o Roblox reconectou).
    AlreadyInGame,
    Failed(String),
    Stop(ReconnectStopReason, Option<String>),
}

/// Mudança que vira linha no Console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectNotice {
    Scheduled { user_id: i64, attempt: u32, in_ms: i64 },
    Offline { user_id: i64 },
    Relaunched { user_id: i64, attempt: u32 },
    Reconnected { user_id: i64 },
    BackByItself { user_id: i64 },
    GaveUp { user_id: i64, attempts: u32 },
    Stopped { user_id: i64, reason: ReconnectStopReason },
    /// O Auto Rejoin assumiu a conta, ou a opção foi desligada.
    Cancelled { user_id: i64 },
}

/// O que a Sessão mostra de cada conta.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectEntryView {
    pub user_id: i64,
    /// `waiting`, `waitingForInternet`, `launching`, `checking`, `gaveUp`, `stopped`.
    pub phase: &'static str,
    pub attempt: u32,
    pub max_attempts: u32,
    pub next_attempt_at_ms: Option<i64>,
    pub reason: Option<ReconnectStopReason>,
    pub error: Option<String>,
    pub drop: ClientDrop,
}

#[derive(Debug, Default)]
pub struct ReconnectBook {
    entries: HashMap<i64, ReconnectEntry>,
}

impl ReconnectBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// O monitor disse que a conta caiu.
    pub fn on_drop(&mut self, input: ReconnectDrop, now_ms: i64) -> Option<ReconnectNotice> {
        let user_id = input.user_id;
        // Auto Rejoin gerencia a conta: ele relança, a reconexão fica de fora.
        // Cliente do site: o app não fecha nem relança.
        if input.auto_rejoin || input.adopted {
            return None;
        }
        let joined_elsewhere = input.drop.reason == Some(DropReason::JoinedElsewhere);
        let watched_was_open = input.drop.kind != DropKind::Crashed;

        if let Some(entry) = self.entries.get_mut(&user_id) {
            let same_client = entry.watched_pid == input.pid;
            match entry.phase.clone() {
                // O cliente relançado caiu de novo: a tentativa não ficou.
                ReconnectPhase::Checking { .. } if same_client => {
                    entry.drop = input.drop.clone();
                    entry.watched_was_open = watched_was_open;
                    if joined_elsewhere {
                        entry.phase = ReconnectPhase::Stopped(ReconnectStopReason::JoinedElsewhere);
                        return Some(ReconnectNotice::Stopped {
                            user_id,
                            reason: ReconnectStopReason::JoinedElsewhere,
                        });
                    }
                    return Some(Self::fail_attempt(entry, user_id, None, now_ms));
                }
                // Já está reconectando esta queda: só a "entrou em outro lugar"
                // (que pode chegar depois de outra linha) muda alguma coisa.
                phase if !phase.is_final() => {
                    if joined_elsewhere {
                        entry.drop = input.drop.clone();
                        entry.phase = ReconnectPhase::Stopped(ReconnectStopReason::JoinedElsewhere);
                        return Some(ReconnectNotice::Stopped {
                            user_id,
                            reason: ReconnectStopReason::JoinedElsewhere,
                        });
                    }
                    return None;
                }
                // Estado final do mesmo cliente: nada muda até a pessoa agir.
                _ if same_client => return None,
                // Estado final de um cliente antigo: esta é uma queda nova.
                _ => {}
            }
        }

        if !input.enabled {
            self.entries.remove(&user_id);
            return None;
        }
        let stop = if joined_elsewhere {
            Some(ReconnectStopReason::JoinedElsewhere)
        } else if input.target.is_none() {
            Some(ReconnectStopReason::NoDestination)
        } else {
            None
        };
        let phase = match stop {
            Some(reason) => ReconnectPhase::Stopped(reason),
            None => ReconnectPhase::Waiting {
                at_ms: now_ms + reconnect_backoff_ms(1),
            },
        };
        self.entries.insert(
            user_id,
            ReconnectEntry {
                phase,
                attempt: 1,
                drop: input.drop,
                watched_pid: input.pid,
                watched_was_open,
                target: input.target,
                error: None,
            },
        );
        Some(match stop {
            Some(reason) => ReconnectNotice::Stopped { user_id, reason },
            None => ReconnectNotice::Scheduled {
                user_id,
                attempt: 1,
                in_ms: reconnect_backoff_ms(1),
            },
        })
    }

    /// A tentativa não ficou: agenda a próxima, ou desiste.
    fn fail_attempt(
        entry: &mut ReconnectEntry,
        user_id: i64,
        error: Option<String>,
        now_ms: i64,
    ) -> ReconnectNotice {
        if error.is_some() {
            entry.error = error;
        }
        if entry.attempt >= RECONNECT_MAX_ATTEMPTS {
            entry.phase = ReconnectPhase::GaveUp;
            return ReconnectNotice::GaveUp {
                user_id,
                attempts: entry.attempt,
            };
        }
        entry.attempt += 1;
        let in_ms = reconnect_backoff_ms(entry.attempt);
        entry.phase = ReconnectPhase::Waiting { at_ms: now_ms + in_ms };
        ReconnectNotice::Scheduled {
            user_id,
            attempt: entry.attempt,
            in_ms,
        }
    }

    /// Uma passada: confere cada conta e devolve as tentativas a disparar.
    pub fn tick(
        &mut self,
        now_ms: i64,
        observe: impl Fn(i64, Option<u32>) -> ReconnectObservation,
    ) -> (Vec<ReconnectAction>, Vec<ReconnectNotice>) {
        let mut actions = Vec::new();
        let mut notices = Vec::new();
        let mut remove = Vec::new();
        let mut ids: Vec<i64> = self.entries.keys().copied().collect();
        ids.sort_unstable();
        for user_id in ids {
            let Some(entry) = self.entries.get_mut(&user_id) else {
                continue;
            };
            if entry.phase.is_final() {
                continue;
            }
            let obs = observe(user_id, entry.watched_pid);
            // Auto Rejoin assumiu, ou a pessoa desligou a opção.
            if obs.auto_rejoin || !obs.enabled {
                if entry.phase != ReconnectPhase::Launching {
                    remove.push(user_id);
                    notices.push(ReconnectNotice::Cancelled { user_id });
                }
                continue;
            }
            match entry.phase.clone() {
                ReconnectPhase::Waiting { at_ms } | ReconnectPhase::WaitingForInternet { at_ms } => {
                    if obs.tracked_in_game {
                        // Voltou ao jogo sozinho (o Roblox reconectou), ou a
                        // pessoa abriu a conta de novo.
                        remove.push(user_id);
                        notices.push(ReconnectNotice::BackByItself { user_id });
                    } else if entry.watched_was_open && obs.watched_exited && !obs.watched_closed_by_app {
                        entry.phase = ReconnectPhase::Stopped(ReconnectStopReason::ClosedByUser);
                        notices.push(ReconnectNotice::Stopped {
                            user_id,
                            reason: ReconnectStopReason::ClosedByUser,
                        });
                    } else if obs.tracked_adopted {
                        entry.phase = ReconnectPhase::Stopped(ReconnectStopReason::OpenedOutsideApp);
                        notices.push(ReconnectNotice::Stopped {
                            user_id,
                            reason: ReconnectStopReason::OpenedOutsideApp,
                        });
                    } else if now_ms >= at_ms {
                        match entry.target.clone() {
                            Some(target) => {
                                entry.phase = ReconnectPhase::Launching;
                                actions.push(ReconnectAction::Attempt {
                                    user_id,
                                    attempt: entry.attempt,
                                    target,
                                });
                            }
                            None => {
                                entry.phase = ReconnectPhase::Stopped(ReconnectStopReason::NoDestination);
                                notices.push(ReconnectNotice::Stopped {
                                    user_id,
                                    reason: ReconnectStopReason::NoDestination,
                                });
                            }
                        }
                    }
                }
                ReconnectPhase::Launching => {}
                ReconnectPhase::Checking {
                    launched_at_ms,
                    in_game_since_ms,
                } => {
                    let since = match (in_game_since_ms, obs.watched_in_game) {
                        (Some(t), _) => Some(t),
                        (None, true) => Some(now_ms),
                        (None, false) => None,
                    };
                    entry.phase = ReconnectPhase::Checking {
                        launched_at_ms,
                        in_game_since_ms: since,
                    };
                    if let Some(drop) = obs.watched_drop.clone() {
                        // O relançado caiu e o aviso passou enquanto a
                        // tentativa rodava: vale como se tivesse chegado agora.
                        entry.drop = drop.clone();
                        if drop.reason == Some(DropReason::JoinedElsewhere) {
                            entry.phase = ReconnectPhase::Stopped(ReconnectStopReason::JoinedElsewhere);
                            notices.push(ReconnectNotice::Stopped {
                                user_id,
                                reason: ReconnectStopReason::JoinedElsewhere,
                            });
                        } else {
                            entry.watched_was_open = drop.kind != DropKind::Crashed;
                            notices.push(Self::fail_attempt(entry, user_id, None, now_ms));
                        }
                    } else if since.is_some_and(|t| now_ms - t >= RECONNECT_STABLE_MS) {
                        remove.push(user_id);
                        notices.push(ReconnectNotice::Reconnected { user_id });
                    } else if obs.watched_exited {
                        // A queda (inclusive "fechou sozinho") já chegou por
                        // `on_drop`; terminar sem queda é a pessoa fechando.
                        if obs.watched_closed_by_app {
                            let notice = Self::fail_attempt(
                                entry,
                                user_id,
                                Some(RECONNECT_ERROR_CLIENT_CLOSED.to_string()),
                                now_ms,
                            );
                            notices.push(notice);
                        } else {
                            entry.phase = ReconnectPhase::Stopped(ReconnectStopReason::ClosedByUser);
                            notices.push(ReconnectNotice::Stopped {
                                user_id,
                                reason: ReconnectStopReason::ClosedByUser,
                            });
                        }
                    } else if since.is_none() && now_ms - launched_at_ms >= RECONNECT_JOIN_TIMEOUT_MS {
                        if obs.watched_log_found {
                            let notice = Self::fail_attempt(
                                entry,
                                user_id,
                                Some(RECONNECT_ERROR_NOT_IN_GAME.to_string()),
                                now_ms,
                            );
                            notices.push(notice);
                        } else {
                            // Sem log não dá para ver o jogo: o cliente está
                            // aberto há 2 min, então conta como reconectado.
                            remove.push(user_id);
                            notices.push(ReconnectNotice::Reconnected { user_id });
                        }
                    }
                }
                ReconnectPhase::GaveUp | ReconnectPhase::Stopped(_) => {}
            }
        }
        for user_id in remove {
            self.entries.remove(&user_id);
        }
        (actions, notices)
    }

    /// O resultado de uma tentativa. Ignorado se a pessoa parou a reconexão
    /// enquanto ela rodava.
    pub fn attempt_finished(
        &mut self,
        user_id: i64,
        outcome: AttemptOutcome,
        now_ms: i64,
    ) -> Option<ReconnectNotice> {
        let entry = self.entries.get_mut(&user_id)?;
        if entry.phase != ReconnectPhase::Launching {
            return None;
        }
        match outcome {
            AttemptOutcome::Launched { pid } => {
                entry.watched_pid = Some(pid);
                entry.watched_was_open = true;
                entry.phase = ReconnectPhase::Checking {
                    launched_at_ms: now_ms,
                    in_game_since_ms: None,
                };
                Some(ReconnectNotice::Relaunched {
                    user_id,
                    attempt: entry.attempt,
                })
            }
            AttemptOutcome::NoInternet => {
                entry.phase = ReconnectPhase::WaitingForInternet {
                    at_ms: now_ms + RECONNECT_OFFLINE_RETRY_MS,
                };
                Some(ReconnectNotice::Offline { user_id })
            }
            AttemptOutcome::Busy => {
                entry.phase = ReconnectPhase::Waiting {
                    at_ms: now_ms + RECONNECT_BUSY_RETRY_MS,
                };
                None
            }
            AttemptOutcome::AlreadyInGame => {
                self.entries.remove(&user_id);
                Some(ReconnectNotice::BackByItself { user_id })
            }
            AttemptOutcome::Failed(error) => Some(Self::fail_attempt(entry, user_id, Some(error), now_ms)),
            AttemptOutcome::Stop(reason, error) => {
                entry.error = error;
                entry.phase = ReconnectPhase::Stopped(reason);
                Some(ReconnectNotice::Stopped { user_id, reason })
            }
        }
    }

    /// O monitor viu a conta voltar a um jogo depois da queda (o Roblox
    /// reconectou sozinho, ou a pessoa clicou em "Reconnect").
    pub fn on_recovered(&mut self, user_id: i64) -> Option<ReconnectNotice> {
        let entry = self.entries.get(&user_id)?;
        if matches!(
            entry.phase,
            ReconnectPhase::Waiting { .. } | ReconnectPhase::WaitingForInternet { .. }
        ) {
            self.entries.remove(&user_id);
            return Some(ReconnectNotice::BackByItself { user_id });
        }
        None
    }

    /// "Parar": esquece a reconexão desta conta (o cliente fica como está).
    pub fn stop(&mut self, user_id: i64) -> bool {
        match self.entries.get(&user_id) {
            // A tentativa em andamento termina; o resultado é ignorado.
            Some(_) => {
                self.entries.remove(&user_id);
                true
            }
            None => false,
        }
    }

    /// "Tentar agora" (e "Tentar de novo" depois de desistir). Conta banida
    /// não tem tentativa: o launch nem passaria.
    pub fn try_now(&mut self, user_id: i64, now_ms: i64) -> bool {
        let Some(entry) = self.entries.get_mut(&user_id) else {
            return false;
        };
        match entry.phase {
            ReconnectPhase::Waiting { .. } | ReconnectPhase::WaitingForInternet { .. } => {
                entry.phase = ReconnectPhase::Waiting { at_ms: now_ms };
                true
            }
            ReconnectPhase::GaveUp | ReconnectPhase::Stopped(_) => {
                if entry.phase == ReconnectPhase::Stopped(ReconnectStopReason::Banned) || entry.target.is_none() {
                    return false;
                }
                entry.attempt = 1;
                entry.error = None;
                entry.phase = ReconnectPhase::Waiting { at_ms: now_ms };
                true
            }
            ReconnectPhase::Launching | ReconnectPhase::Checking { .. } => false,
        }
    }

    /// Alguma reconexão em andamento (para manter o PC acordado).
    pub fn is_active(&self) -> bool {
        self.entries.values().any(|e| !e.phase.is_final())
    }

    pub fn views(&self) -> Vec<ReconnectEntryView> {
        let mut views: Vec<ReconnectEntryView> = self
            .entries
            .iter()
            .map(|(user_id, e)| {
                let (phase, next, reason) = match &e.phase {
                    ReconnectPhase::Waiting { at_ms } => ("waiting", Some(*at_ms), None),
                    ReconnectPhase::WaitingForInternet { at_ms } => ("waitingForInternet", Some(*at_ms), None),
                    ReconnectPhase::Launching => ("launching", None, None),
                    ReconnectPhase::Checking { .. } => ("checking", None, None),
                    ReconnectPhase::GaveUp => ("gaveUp", None, None),
                    ReconnectPhase::Stopped(reason) => ("stopped", None, Some(*reason)),
                };
                ReconnectEntryView {
                    user_id: *user_id,
                    phase,
                    attempt: e.attempt,
                    max_attempts: RECONNECT_MAX_ATTEMPTS,
                    next_attempt_at_ms: next,
                    reason,
                    error: e.error.clone(),
                    drop: e.drop.clone(),
                }
            })
            .collect();
        views.sort_by_key(|v| v.user_id);
        views
    }
}

/// O erro do launch, nos termos da reconexão: ban e sessão expirada param de
/// vez; o resto é uma tentativa que falhou.
pub fn attempt_outcome_for_launch_error(error: &str) -> AttemptOutcome {
    if error == LAUNCH_ALREADY_ACTIVE {
        return AttemptOutcome::Busy;
    }
    // "Skipped: …" é a frase do launch pulado por moderação (moderation.rs).
    if is_moderated_error(error) || error.starts_with("Skipped:") {
        return AttemptOutcome::Stop(ReconnectStopReason::Banned, Some(error.to_string()));
    }
    if is_auth_session_error(error) || error == session_relogin_error() {
        return AttemptOutcome::Stop(ReconnectStopReason::SessionExpired, Some(error.to_string()));
    }
    AttemptOutcome::Failed(error.to_string())
}

/// A conta tem a reconexão ligada? O campo da conta (`AutoReconnect`) manda;
/// sem ele, vale o padrão `General.AutoReconnect`. O `AutoRelaunch` do Nexus
/// liga também.
pub fn reconnect_enabled(account_field: Option<&str>, global_default: bool, nexus_auto_relaunch: bool) -> bool {
    let own = match account_field.map(str::trim) {
        Some("true") => true,
        Some("false") => false,
        _ => global_default,
    };
    own || nexus_auto_relaunch
}

/// Linha do Console (português, como as do monitor de quedas).
fn reconnect_console_line(notice: &ReconnectNotice) -> (&'static str, String) {
    let max = RECONNECT_MAX_ATTEMPTS;
    match notice {
        ReconnectNotice::Scheduled { attempt, in_ms, .. } => (
            "info",
            format!("Reconexão automática: tentativa {attempt}/{max} em {} s", in_ms / 1_000),
        ),
        ReconnectNotice::Offline { .. } => (
            "warn",
            String::from("Sem internet: a reconexão espera ela voltar"),
        ),
        ReconnectNotice::Relaunched { attempt, .. } => (
            "info",
            format!("Reconectando (tentativa {attempt}/{max}): conferindo se fica no jogo"),
        ),
        ReconnectNotice::Reconnected { .. } => ("success", String::from("Reconectou: ficou no jogo")),
        ReconnectNotice::BackByItself { .. } => (
            "info",
            String::from("Voltou ao jogo sozinha: reconexão dispensada"),
        ),
        ReconnectNotice::GaveUp { attempts, .. } => (
            "error",
            format!("Reconexão desistiu depois de {attempts} tentativas"),
        ),
        ReconnectNotice::Stopped { reason, .. } => (
            "warn",
            match reason {
                ReconnectStopReason::JoinedElsewhere => {
                    "Sem reconexão: a conta entrou em outro lugar".to_string()
                }
                ReconnectStopReason::ClosedByUser => "Sem reconexão: o cliente foi fechado".to_string(),
                ReconnectStopReason::Banned => "Sem reconexão: conta banida".to_string(),
                ReconnectStopReason::SessionExpired => {
                    "Sem reconexão: a sessão expirou, entre na conta de novo".to_string()
                }
                ReconnectStopReason::NoDestination => {
                    "Sem reconexão: não se sabe para onde voltar".to_string()
                }
                ReconnectStopReason::OpenedOutsideApp => {
                    "Sem reconexão: a conta está aberta fora do app".to_string()
                }
            },
        ),
        ReconnectNotice::Cancelled { .. } => ("info", String::from("Reconexão cancelada")),
    }
}

fn reconnect_notice_user(notice: &ReconnectNotice) -> i64 {
    match notice {
        ReconnectNotice::Scheduled { user_id, .. }
        | ReconnectNotice::Offline { user_id }
        | ReconnectNotice::Relaunched { user_id, .. }
        | ReconnectNotice::Reconnected { user_id }
        | ReconnectNotice::BackByItself { user_id }
        | ReconnectNotice::GaveUp { user_id, .. }
        | ReconnectNotice::Stopped { user_id, .. }
        | ReconnectNotice::Cancelled { user_id } => *user_id,
    }
}

/// Onde o app mandou cada conta no último launch (só em memória).
static LAUNCHED_TARGETS: std::sync::LazyLock<std::sync::Mutex<HashMap<i64, LaunchedTarget>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// launch.rs chama isto quando o cliente da conta subiu.
pub(crate) fn remember_launch_target(user_id: i64, target: LaunchedTarget) {
    if let Ok(mut targets) = LAUNCHED_TARGETS.lock() {
        targets.insert(user_id, target);
    }
}

fn launched_target_of(user_id: i64) -> Option<LaunchedTarget> {
    LAUNCHED_TARGETS.lock().ok()?.get(&user_id).cloned()
}

/// Há internet até o Roblox? Qualquer resposta HTTP conta (até um 404): só
/// erro de rede é "sem internet".
async fn reachable_at(client: &reqwest::Client, url: &str) -> bool {
    client.head(url).send().await.is_ok()
}

async fn roblox_reachable() -> bool {
    let Ok(client) = api::http_client::builder_with(
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(8),
    )
    .build() else {
        return true;
    };
    reachable_at(&client, &format!("{}/", api::endpoints::host("www"))).await
}

static RECONNECT_BOOK: std::sync::LazyLock<std::sync::Mutex<ReconnectBook>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(ReconnectBook::new()));

fn with_reconnect_book<R>(f: impl FnOnce(&mut ReconnectBook) -> R) -> R {
    let mut book = match RECONNECT_BOOK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut book)
}

/// Alguma reconexão em andamento (keep_awake.rs).
pub(crate) fn reconnect_is_active() -> bool {
    with_reconnect_book(|book| book.is_active())
}

/// Último retrato enviado à tela (para só emitir quando muda).
static RECONNECT_LAST_VIEWS: std::sync::LazyLock<std::sync::Mutex<Vec<ReconnectEntryView>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

fn publish_reconnect(app: &tauri::AppHandle, notices: &[ReconnectNotice]) {
    for notice in notices {
        // A conta relançada toca a gravação dela quando ficar o tempo
        // configurado no jogo (commands/recordings.rs).
        if let ReconnectNotice::Relaunched { user_id, .. } = notice {
            arm_recording_after_reconnect(*user_id, chrono::Utc::now().timestamp_millis());
        }
        let (level, line) = reconnect_console_line(notice);
        emit_launch_log(app, reconnect_notice_user(notice), level, "reconnect", line);
    }
    let views = with_reconnect_book(|book| book.views());
    let changed = match RECONNECT_LAST_VIEWS.lock() {
        Ok(mut last) => {
            let changed = *last != views;
            if changed {
                *last = views.clone();
            }
            changed
        }
        Err(_) => true,
    };
    let reconnected: Vec<i64> = notices
        .iter()
        .filter_map(|n| match n {
            ReconnectNotice::Reconnected { user_id } => Some(*user_id),
            _ => None,
        })
        .collect();
    if changed || !reconnected.is_empty() {
        let _ = app.emit(
            "auto-reconnect",
            serde_json::json!({ "entries": views, "reconnected": reconnected }),
        );
    }
}

#[tauri::command]
fn get_auto_reconnect_status() -> serde_json::Value {
    serde_json::json!({ "entries": with_reconnect_book(|book| book.views()) })
}

/// "Parar": a conta sai da reconexão; nenhum cliente é fechado.
#[tauri::command]
fn stop_auto_reconnect(app: tauri::AppHandle, user_id: i64) -> bool {
    let stopped = with_reconnect_book(|book| book.stop(user_id));
    if stopped {
        emit_launch_log(&app, user_id, "info", "reconnect", "Reconexão parada pelo usuário");
        publish_reconnect(&app, &[]);
    }
    stopped
}

/// "Tentar agora" / "Tentar de novo": a próxima passada (até 2 s) tenta.
#[tauri::command]
fn retry_auto_reconnect(app: tauri::AppHandle, user_id: i64) -> bool {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let ok = with_reconnect_book(|book| book.try_now(user_id, now_ms));
    if ok {
        publish_reconnect(&app, &[]);
    }
    ok
}

/// A conta é gerenciada pelo Auto Rejoin agora?
#[cfg(target_os = "windows")]
fn managed_by_auto_rejoin(status: &BottingStatusPayload, user_id: i64) -> bool {
    status.active && status.user_ids.contains(&user_id)
}

/// O que decide a opção de cada conta, lido uma vez por passada.
#[cfg(target_os = "windows")]
struct ReconnectSettings {
    global_default: bool,
    /// `AutoReconnect` de cada conta, quando a conta tem o campo.
    fields: HashMap<i64, String>,
    usernames: HashMap<i64, String>,
    nexus_auto_relaunch: HashSet<String>,
}

#[cfg(target_os = "windows")]
impl ReconnectSettings {
    fn load(app: &tauri::AppHandle) -> Self {
        let global_default = app.state::<SettingsStore>().get_bool("General", "AutoReconnect");
        let accounts = app.state::<AccountStore>().get_all().unwrap_or_default();
        let mut fields = HashMap::new();
        let mut usernames = HashMap::new();
        for account in accounts {
            if let Some(value) = account.fields.get("AutoReconnect") {
                fields.insert(account.user_id, value.clone());
            }
            usernames.insert(account.user_id, account.username.to_ascii_lowercase());
        }
        Self {
            global_default,
            fields,
            usernames,
            nexus_auto_relaunch: nexus_auto_relaunch_usernames(),
        }
    }

    fn enabled(&self, user_id: i64) -> bool {
        let nexus = self
            .usernames
            .get(&user_id)
            .is_some_and(|name| self.nexus_auto_relaunch.contains(name));
        reconnect_enabled(self.fields.get(&user_id).map(String::as_str), self.global_default, nexus)
    }

    /// Alguma conta com a opção ligada tem um cliente do app aberto: a
    /// reconexão está de guarda (para manter o PC acordado).
    fn guarding(&self, tracked: &[platform::windows::TrackedProcess]) -> bool {
        tracked.iter().any(|p| !p.adopted && self.enabled(p.user_id))
    }
}

/// Contas com `AutoRelaunch` ligado no Nexus (nome em minúsculas).
#[cfg(all(target_os = "windows", feature = "nexus"))]
fn nexus_auto_relaunch_usernames() -> HashSet<String> {
    nexus::websocket::nexus()
        .get_accounts()
        .into_iter()
        .filter(|a| a.auto_relaunch)
        .map(|a| a.username.to_ascii_lowercase())
        .collect()
}

#[cfg(all(target_os = "windows", not(feature = "nexus")))]
fn nexus_auto_relaunch_usernames() -> HashSet<String> {
    HashSet::new()
}

/// A reconexão está de guarda ou em andamento? (keep_awake.rs)
#[cfg(target_os = "windows")]
static RECONNECT_GUARDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
pub(crate) fn reconnect_is_guarding() -> bool {
    RECONNECT_GUARDING.load(std::sync::atomic::Ordering::Relaxed) || reconnect_is_active()
}

/// Uma passada da reconexão, logo depois da do monitor de quedas: as quedas e
/// as voltas que ele avisou, e a conferência de cada conta.
#[cfg(target_os = "windows")]
pub(crate) fn reconnect_after_health_tick(app: &tauri::AppHandle, notices: &[HealthNotice]) {
    // Antes das saídas cedo abaixo: a gravação espera a conta ficar no jogo
    // mesmo depois de a reconexão já ter dado a conta por reconectada.
    recording_after_reconnect_tick(app);
    let tracked = platform::windows::tracker().get_all();
    let has_work = with_reconnect_book(|book| !book.entries.is_empty())
        || notices.iter().any(|n| matches!(n, HealthNotice::Dropped { .. }));
    let any_ours = tracked.iter().any(|p| !p.adopted);
    if !has_work && !any_ours {
        RECONNECT_GUARDING.store(false, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    let settings = ReconnectSettings::load(app);
    RECONNECT_GUARDING.store(settings.guarding(&tracked), std::sync::atomic::Ordering::Relaxed);
    if !has_work {
        return;
    }
    let botting = current_botting_status();
    let views: HashMap<i64, ClientHealthView> = CLIENT_HEALTH_VIEWS
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut out = Vec::new();

    for notice in notices {
        match notice {
            HealthNotice::Dropped {
                user_id,
                drop,
                adopted,
            } => {
                let view = views.get(user_id);
                let target = reconnect_target(
                    launched_target_of(*user_id).as_ref(),
                    view.and_then(|v| v.destination.as_ref()),
                    drop.kind,
                );
                let input = ReconnectDrop {
                    user_id: *user_id,
                    pid: view.map(|v| v.pid),
                    adopted: *adopted,
                    drop: drop.clone(),
                    enabled: settings.enabled(*user_id),
                    auto_rejoin: managed_by_auto_rejoin(&botting, *user_id),
                    target,
                };
                if let Some(n) = with_reconnect_book(|book| book.on_drop(input, now_ms)) {
                    out.push(n);
                }
            }
            HealthNotice::Recovered { user_id } => {
                if let Some(n) = with_reconnect_book(|book| book.on_recovered(*user_id)) {
                    out.push(n);
                }
            }
            HealthNotice::NotResponding { .. } => {}
        }
    }

    let (actions, tick_notices) = with_reconnect_book(|book| {
        book.tick(now_ms, |user_id, watched| {
            let tracked_now = tracked.iter().find(|p| p.user_id == user_id);
            let view = views.get(&user_id);
            let tracked_view = tracked_now.and_then(|p| view.filter(|v| v.pid == p.pid));
            let watched_view = watched.and_then(|pid| view.filter(|v| v.pid == pid));
            let watched_exited = match (watched, watched_view) {
                (None, _) => false,
                (Some(_), Some(v)) => v.exited,
                // Sem retrato: só ainda não visto se for o cliente que o
                // tracker tem agora (acabou de abrir); senão ele já se foi.
                (Some(pid), None) => tracked_now.map(|p| p.pid) != Some(pid),
            };
            ReconnectObservation {
                enabled: settings.enabled(user_id),
                auto_rejoin: managed_by_auto_rejoin(&botting, user_id),
                tracked_pid: tracked_now.map(|p| p.pid),
                tracked_adopted: tracked_now.is_some_and(|p| p.adopted),
                tracked_in_game: tracked_view.is_some_and(|v| v.in_game),
                watched_exited,
                watched_closed_by_app: watched.is_some_and(platform::windows::was_terminated_by_app),
                watched_in_game: watched_view.is_some_and(|v| v.in_game),
                watched_log_found: watched_view.is_some_and(|v| v.log_found),
                watched_drop: watched_view.and_then(|v| v.drop.clone()),
            }
        })
    });
    out.extend(tick_notices);
    publish_reconnect(app, &out);

    for action in actions {
        let ReconnectAction::Attempt {
            user_id,
            attempt,
            target,
        } = action;
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = run_reconnect_attempt(&app, user_id, attempt, &target).await;
            let now_ms = chrono::Utc::now().timestamp_millis();
            let notice = with_reconnect_book(|book| book.attempt_finished(user_id, outcome, now_ms));
            publish_reconnect(&app, &notice.into_iter().collect::<Vec<_>>());
        });
    }
}

/// Uma tentativa: internet, ban, o cliente velho da própria conta e o launch
/// normal (pela fila, sem renovar a sessão).
#[cfg(target_os = "windows")]
async fn run_reconnect_attempt(
    app: &tauri::AppHandle,
    user_id: i64,
    attempt: u32,
    target: &ReconnectTarget,
) -> AttemptOutcome {
    if !roblox_reachable().await {
        return AttemptOutcome::NoInternet;
    }

    // Ban novo costuma vir logo depois de um kick: a 1ª tentativa pergunta de
    // novo ao Roblox (leitura, sem refresh). Falha da consulta não bloqueia.
    if attempt == 1 {
        forget_moderation(user_id);
    }
    let status = match cached_moderation(user_id, std::time::Instant::now()) {
        Some(status) => Some(status),
        None => fetch_moderation(
            app.state::<AccountStore>().inner(),
            Some(app),
            user_id,
            api::http_client::client(),
        )
        .await
        .ok(),
    };
    if let Some(message) = status.and_then(|s| moderation_block_message(&s, chrono::Utc::now())) {
        return AttemptOutcome::Stop(ReconnectStopReason::Banned, Some(message));
    }

    let tracker = platform::windows::tracker();
    if let Some(current) = tracker.get_all().into_iter().find(|p| p.user_id == user_id) {
        // Cliente do site nunca é fechado; o que voltou ao jogo sozinho fica.
        if current.adopted {
            return AttemptOutcome::Stop(ReconnectStopReason::OpenedOutsideApp, None);
        }
        if client_health_of(user_id, current.pid).is_some_and(|v| v.in_game) {
            return AttemptOutcome::AlreadyInGame;
        }
    }

    // A fila primeiro: com outro launch rodando, nada é fechado e a tentativa
    // volta logo depois, sem gastar a vez.
    let sequence = match launch_queue_start(app, &[user_id], target.place_id, &target.job_id) {
        Ok(sequence) => sequence,
        Err(_) => return AttemptOutcome::Busy,
    };
    emit_launch_log(
        app,
        user_id,
        "info",
        "reconnect",
        format!("Reconexão automática: tentativa {attempt}/{RECONNECT_MAX_ATTEMPTS}"),
    );

    // O cliente velho é da própria conta e do app (conferido acima): fecha só
    // ele, para o novo não brigar com a sessão caída.
    if let Some(current) = tracker.get_all().into_iter().find(|p| p.user_id == user_id) {
        if current.adopted {
            sequence.finish();
            return AttemptOutcome::Stop(ReconnectStopReason::OpenedOutsideApp, None);
        }
        if !tracker.kill_for_user_graceful_async(user_id, 4500).await {
            let error = RECONNECT_ERROR_OLD_CLIENT_OPEN.to_string();
            sequence.mark(user_id, LaunchQueueState::Failed, Some(error.clone()));
            sequence.finish();
            return AttemptOutcome::Failed(error);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let result = launch_roblox_windows(
        app.clone(),
        &sequence,
        app.state::<AccountStore>(),
        app.state::<SettingsStore>(),
        app.state::<data::versions::VersionsCatalogStore>(),
        user_id,
        target.place_id,
        target.job_id.clone(),
        target.launch_data.clone(),
        false,
        target.join_vip,
        target.link_code.clone(),
        None,
        false,
    )
    .await;
    if let Err(error) = &result {
        sequence.mark(user_id, LaunchQueueState::Failed, Some(error.clone()));
    }
    sequence.finish();
    drop(sequence);
    match result {
        Ok(()) => match tracker.get_all().into_iter().find(|p| p.user_id == user_id) {
            Some(p) if !p.adopted => AttemptOutcome::Launched { pid: p.pid },
            _ => AttemptOutcome::Failed(RECONNECT_ERROR_DID_NOT_START.to_string()),
        },
        Err(error) => attempt_outcome_for_launch_error(&error),
    }
}

#[cfg(test)]
mod auto_reconnect_tests {
    use super::*;

    const T0: i64 = 1_791_000_000_000;

    /// O trecho de `source` que começa em `start` e vai até o fim da função.
    fn function_body<'a>(source: &'a str, start: &str) -> &'a str {
        let from = source.find(start).unwrap_or_else(|| panic!("{start} sumiu"));
        let body = &source[from..];
        &body[..body.find("\n}\n").expect("fim da função")]
    }

    /// Conferido em 11/10/2026: o cliente que a reconexão reabre cai na grade
    /// dos monitores marcados na aba Windows (`General.GridMonitors`), como o
    /// launch normal — não há caminho próprio de abrir cliente aqui. Trava a
    /// corrente inteira: tentativa → `launch_roblox_windows` → plano de janela
    /// com a grade automática → `place_in_grid` com os monitores do INI.
    #[test]
    fn a_reconnected_client_lands_in_the_grid_of_the_ticked_monitors() {
        let attempt = function_body(include_str!("reconnect.rs"), "async fn run_reconnect_attempt(");
        assert!(attempt.contains("launch_roblox_windows("), "a reconexão tem de usar o launch normal");

        let launch = function_body(include_str!("launch.rs"), "async fn launch_roblox_windows(");
        assert!(launch.contains("spawn_client_window_enforcement("));
        assert!(launch.contains("auto_arrange_grid: auto_arrange_grid_enabled(&settings)"));

        let shared = include_str!("launch_shared.rs");
        let enforcement = function_body(shared, "pub(crate) fn spawn_client_window_enforcement(");
        assert!(enforcement.contains("grid_layout_settings(&settings)"));
        assert!(enforcement.contains("monitor_indices,"));
        assert!(enforcement.contains("windows::place_in_grid("));
        let layout = function_body(shared, "pub(crate) fn grid_layout_settings(");
        assert!(layout.contains(r#""GridMonitors""#));

        let windowing = include_str!("../platform/windows/windowing.rs");
        let place = function_body(windowing, "pub fn place_in_grid(");
        assert!(place.contains("selected_monitors(&list_monitors(), &request.monitor_indices)"));
    }

    fn drop_of(kind: DropKind, reason: Option<DropReason>) -> ClientDrop {
        ClientDrop {
            kind,
            reason,
            code: Some(277),
            message: None,
            since_ms: T0,
        }
    }

    fn lost() -> ClientDrop {
        drop_of(DropKind::Disconnected, Some(DropReason::ConnectionLost))
    }

    fn target() -> ReconnectTarget {
        ReconnectTarget {
            place_id: 1,
            job_id: "job".into(),
            launch_data: String::new(),
            join_vip: false,
            link_code: String::new(),
        }
    }

    fn dropped(user_id: i64, pid: u32, drop: ClientDrop) -> ReconnectDrop {
        ReconnectDrop {
            user_id,
            pid: Some(pid),
            adopted: false,
            drop,
            enabled: true,
            auto_rejoin: false,
            target: Some(target()),
        }
    }

    /// A conta ligada, com o cliente que caiu ainda aberto.
    fn open(pid: u32) -> ReconnectObservation {
        ReconnectObservation {
            enabled: true,
            tracked_pid: Some(pid),
            watched_log_found: true,
            ..Default::default()
        }
    }

    fn phase(book: &ReconnectBook, user_id: i64) -> Option<&'static str> {
        book.views().into_iter().find(|v| v.user_id == user_id).map(|v| v.phase)
    }

    /// Leva a conta até a tentativa em andamento e devolve a ação.
    fn run_to_attempt(book: &mut ReconnectBook, now: i64, pid: u32) -> Vec<ReconnectAction> {
        book.tick(now, |_, _| open(pid)).0
    }

    #[test]
    fn the_backoff_grows_to_five_minutes_and_stays() {
        let secs: Vec<i64> = (1..=7).map(|a| reconnect_backoff_ms(a) / 1_000).collect();
        assert_eq!(secs, vec![10, 30, 60, 120, 300, 300, 300]);
    }

    #[test]
    fn a_drop_waits_10_seconds_then_relaunches_to_the_same_place() {
        let mut book = ReconnectBook::new();
        let notice = book.on_drop(dropped(1, 100, lost()), T0);
        assert_eq!(
            notice,
            Some(ReconnectNotice::Scheduled {
                user_id: 1,
                attempt: 1,
                in_ms: 10_000
            })
        );
        assert_eq!(phase(&book, 1), Some("waiting"));
        assert!(run_to_attempt(&mut book, T0 + 9_999, 100).is_empty());
        let actions = run_to_attempt(&mut book, T0 + 10_000, 100);
        assert_eq!(
            actions,
            vec![ReconnectAction::Attempt {
                user_id: 1,
                attempt: 1,
                target: target()
            }]
        );
        assert_eq!(phase(&book, 1), Some("launching"));
        // Uma tentativa por vez: a passada seguinte não dispara outra.
        assert!(run_to_attempt(&mut book, T0 + 12_000, 100).is_empty());
    }

    #[test]
    fn the_option_off_does_nothing() {
        let mut book = ReconnectBook::new();
        let input = ReconnectDrop {
            enabled: false,
            ..dropped(1, 100, lost())
        };
        assert_eq!(book.on_drop(input, T0), None);
        assert!(book.views().is_empty());
    }

    #[test]
    fn a_website_client_is_never_reconnected() {
        let mut book = ReconnectBook::new();
        let input = ReconnectDrop {
            adopted: true,
            ..dropped(1, 100, lost())
        };
        assert_eq!(book.on_drop(input, T0), None);
        assert!(book.views().is_empty());
    }

    #[test]
    fn auto_rejoin_wins() {
        let mut book = ReconnectBook::new();
        let input = ReconnectDrop {
            auto_rejoin: true,
            ..dropped(1, 100, lost())
        };
        assert_eq!(book.on_drop(input, T0), None);

        // Auto Rejoin assumiu no meio da espera: a reconexão sai.
        book.on_drop(dropped(2, 200, lost()), T0);
        let (actions, notices) = book.tick(T0 + 20_000, |_, _| ReconnectObservation {
            auto_rejoin: true,
            ..open(200)
        });
        assert!(actions.is_empty());
        assert_eq!(notices, vec![ReconnectNotice::Cancelled { user_id: 2 }]);
        assert!(book.views().is_empty());
    }

    #[test]
    fn joined_elsewhere_stops_for_good() {
        let mut book = ReconnectBook::new();
        let elsewhere = drop_of(DropKind::Disconnected, Some(DropReason::JoinedElsewhere));
        let notice = book.on_drop(dropped(1, 100, elsewhere.clone()), T0);
        assert_eq!(
            notice,
            Some(ReconnectNotice::Stopped {
                user_id: 1,
                reason: ReconnectStopReason::JoinedElsewhere
            })
        );
        assert!(run_to_attempt(&mut book, T0 + 600_000, 100).is_empty(), "never retries");

        // Chegando depois de outra linha da mesma queda, também para.
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(2, 200, lost()), T0);
        book.on_drop(dropped(2, 200, elsewhere.clone()), T0 + 500);
        assert_eq!(book.views()[0].reason, Some(ReconnectStopReason::JoinedElsewhere));
        assert!(run_to_attempt(&mut book, T0 + 600_000, 200).is_empty());

        // No meio de uma tentativa, também: o resultado dela é ignorado.
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(3, 300, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 300);
        book.on_drop(dropped(3, 301, elsewhere), T0 + 11_000);
        assert_eq!(book.attempt_finished(3, AttemptOutcome::Launched { pid: 301 }, T0 + 12_000), None);
        assert_eq!(book.views()[0].reason, Some(ReconnectStopReason::JoinedElsewhere));
    }

    #[test]
    fn a_drop_of_the_relaunched_client_seen_late_still_counts() {
        // O aviso da queda passou enquanto a tentativa rodava: a passada vê a
        // queda no retrato do monitor.
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        book.attempt_finished(1, AttemptOutcome::Launched { pid: 300 }, T0 + 20_000);
        let (_, notices) = book.tick(T0 + 22_000, |_, _| ReconnectObservation {
            watched_drop: Some(lost()),
            ..open(300)
        });
        assert!(matches!(notices[..], [ReconnectNotice::Scheduled { attempt: 2, .. }]));

        run_to_attempt(&mut book, T0 + 60_000, 300);
        book.attempt_finished(1, AttemptOutcome::Launched { pid: 400 }, T0 + 61_000);
        let (_, notices) = book.tick(T0 + 62_000, |_, _| ReconnectObservation {
            watched_drop: Some(drop_of(DropKind::Disconnected, Some(DropReason::JoinedElsewhere))),
            ..open(400)
        });
        assert_eq!(
            notices,
            vec![ReconnectNotice::Stopped {
                user_id: 1,
                reason: ReconnectStopReason::JoinedElsewhere
            }]
        );
    }

    #[test]
    fn closing_the_dropped_client_stops_it() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        let (actions, notices) = book.tick(T0 + 3_000, |_, _| ReconnectObservation {
            watched_exited: true,
            ..open(100)
        });
        assert!(actions.is_empty());
        assert_eq!(
            notices,
            vec![ReconnectNotice::Stopped {
                user_id: 1,
                reason: ReconnectStopReason::ClosedByUser
            }]
        );
        assert!(run_to_attempt(&mut book, T0 + 600_000, 100).is_empty());
    }

    #[test]
    fn the_app_closing_the_dropped_client_keeps_reconnecting() {
        // O Watcher (Exit If No Connection) fechou o cliente: não foi a pessoa.
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        let observe = |_: i64, _: Option<u32>| ReconnectObservation {
            watched_exited: true,
            watched_closed_by_app: true,
            tracked_pid: None,
            ..open(100)
        };
        assert!(book.tick(T0 + 3_000, observe).1.is_empty());
        assert_eq!(book.tick(T0 + 10_000, observe).0.len(), 1);
    }

    #[test]
    fn a_crash_is_reconnected_even_though_the_process_is_gone() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, drop_of(DropKind::Crashed, None)), T0);
        let actions = book
            .tick(T0 + 10_000, |_, _| ReconnectObservation {
                enabled: true,
                watched_exited: true,
                ..Default::default()
            })
            .0;
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn coming_back_by_itself_cancels_the_reconnect() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        assert_eq!(book.on_recovered(1), Some(ReconnectNotice::BackByItself { user_id: 1 }));
        assert!(book.views().is_empty());

        // Também quando a passada vê o cliente da conta em jogo.
        book.on_drop(dropped(2, 200, lost()), T0);
        let (_, notices) = book.tick(T0 + 1_000, |_, _| ReconnectObservation {
            tracked_in_game: true,
            ..open(200)
        });
        assert_eq!(notices, vec![ReconnectNotice::BackByItself { user_id: 2 }]);
    }

    #[test]
    fn turning_the_option_off_cancels_a_pending_reconnect() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        let (_, notices) = book.tick(T0 + 1_000, |_, _| ReconnectObservation {
            enabled: false,
            ..open(100)
        });
        assert_eq!(notices, vec![ReconnectNotice::Cancelled { user_id: 1 }]);
        assert!(book.views().is_empty());
    }

    #[test]
    fn staying_in_game_for_2_minutes_is_a_success_and_resets() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        book.attempt_finished(1, AttemptOutcome::Launched { pid: 300 }, T0 + 20_000);
        assert_eq!(phase(&book, 1), Some("checking"));

        let in_game = |_: i64, _: Option<u32>| ReconnectObservation {
            watched_in_game: true,
            tracked_in_game: true,
            ..open(300)
        };
        assert!(book.tick(T0 + 40_000, in_game).1.is_empty());
        assert!(book.tick(T0 + 40_000 + RECONNECT_STABLE_MS - 1, in_game).1.is_empty());
        let (_, notices) = book.tick(T0 + 40_000 + RECONNECT_STABLE_MS, in_game);
        assert_eq!(notices, vec![ReconnectNotice::Reconnected { user_id: 1 }]);
        assert!(book.views().is_empty(), "the attempt count starts over on the next drop");
    }

    #[test]
    fn dropping_again_before_2_minutes_is_the_next_attempt_with_more_wait() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        book.attempt_finished(1, AttemptOutcome::Launched { pid: 300 }, T0 + 20_000);
        let notice = book.on_drop(dropped(1, 300, lost()), T0 + 60_000);
        assert_eq!(
            notice,
            Some(ReconnectNotice::Scheduled {
                user_id: 1,
                attempt: 2,
                in_ms: 30_000
            })
        );
        let view = &book.views()[0];
        assert_eq!(view.attempt, 2);
        assert_eq!(view.next_attempt_at_ms, Some(T0 + 90_000));
    }

    #[test]
    fn five_attempts_that_do_not_stay_give_up() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        let mut now = T0;
        for attempt in 1..=RECONNECT_MAX_ATTEMPTS {
            now += reconnect_backoff_ms(attempt);
            let actions = run_to_attempt(&mut book, now, 100);
            assert_eq!(actions.len(), 1, "attempt {attempt}");
            let notice = book.attempt_finished(1, AttemptOutcome::Failed("boom".into()), now);
            if attempt < RECONNECT_MAX_ATTEMPTS {
                assert!(matches!(notice, Some(ReconnectNotice::Scheduled { .. })));
            } else {
                assert_eq!(notice, Some(ReconnectNotice::GaveUp { user_id: 1, attempts: 5 }));
            }
        }
        let view = &book.views()[0];
        assert_eq!(view.phase, "gaveUp");
        assert_eq!(view.error.as_deref(), Some("boom"));
        assert!(run_to_attempt(&mut book, now + 3_600_000, 100).is_empty());
    }

    #[test]
    fn no_internet_waits_without_spending_an_attempt() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        assert_eq!(
            book.attempt_finished(1, AttemptOutcome::NoInternet, T0 + 10_500),
            Some(ReconnectNotice::Offline { user_id: 1 })
        );
        assert_eq!(phase(&book, 1), Some("waitingForInternet"));
        assert!(run_to_attempt(&mut book, T0 + 15_000, 100).is_empty());
        let actions = run_to_attempt(&mut book, T0 + 20_500, 100);
        assert!(matches!(actions[..], [ReconnectAction::Attempt { attempt: 1, .. }]));
    }

    #[test]
    fn a_busy_launch_queue_retries_soon_without_spending_an_attempt() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        assert_eq!(book.attempt_finished(1, AttemptOutcome::Busy, T0 + 10_000), None);
        let actions = run_to_attempt(&mut book, T0 + 15_000, 100);
        assert!(matches!(actions[..], [ReconnectAction::Attempt { attempt: 1, .. }]));
    }

    #[test]
    fn a_ban_or_an_expired_session_stops_for_good() {
        for (error, reason) in [
            ("Skipped: this account is banned.", ReconnectStopReason::Banned),
            ("User is moderated", ReconnectStopReason::Banned),
            ("Request failed with status 401", ReconnectStopReason::SessionExpired),
            ("Roblox invalidated this session. Re-login required.", ReconnectStopReason::SessionExpired),
        ] {
            let outcome = attempt_outcome_for_launch_error(error);
            assert_eq!(outcome, AttemptOutcome::Stop(reason, Some(error.to_string())), "{error}");
            let mut book = ReconnectBook::new();
            book.on_drop(dropped(1, 100, lost()), T0);
            run_to_attempt(&mut book, T0 + 10_000, 100);
            book.attempt_finished(1, outcome, T0 + 11_000);
            assert_eq!(book.views()[0].reason, Some(reason));
            assert!(run_to_attempt(&mut book, T0 + 3_600_000, 100).is_empty());
        }
        assert_eq!(attempt_outcome_for_launch_error(LAUNCH_ALREADY_ACTIVE), AttemptOutcome::Busy);
        assert_eq!(
            attempt_outcome_for_launch_error("PID não detectado"),
            AttemptOutcome::Failed("PID não detectado".into())
        );
    }

    #[test]
    fn a_relaunch_that_never_gets_into_the_game_fails_after_2_minutes() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        book.attempt_finished(1, AttemptOutcome::Launched { pid: 300 }, T0 + 20_000);
        let loading = |_: i64, _: Option<u32>| open(300);
        assert!(book.tick(T0 + 20_000 + RECONNECT_JOIN_TIMEOUT_MS - 1, loading).1.is_empty());
        let (_, notices) = book.tick(T0 + 20_000 + RECONNECT_JOIN_TIMEOUT_MS, loading);
        assert!(matches!(notices[..], [ReconnectNotice::Scheduled { attempt: 2, .. }]));
    }

    #[test]
    fn without_a_log_two_minutes_open_counts_as_reconnected() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        book.attempt_finished(1, AttemptOutcome::Launched { pid: 300 }, T0 + 20_000);
        let no_log = |_: i64, _: Option<u32>| ReconnectObservation {
            watched_log_found: false,
            ..open(300)
        };
        let (_, notices) = book.tick(T0 + 20_000 + RECONNECT_JOIN_TIMEOUT_MS, no_log);
        assert_eq!(notices, vec![ReconnectNotice::Reconnected { user_id: 1 }]);
    }

    #[test]
    fn closing_the_relaunched_client_stops_it() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        book.attempt_finished(1, AttemptOutcome::Launched { pid: 300 }, T0 + 20_000);
        let (_, notices) = book.tick(T0 + 30_000, |_, _| ReconnectObservation {
            watched_exited: true,
            ..open(300)
        });
        assert_eq!(
            notices,
            vec![ReconnectNotice::Stopped {
                user_id: 1,
                reason: ReconnectStopReason::ClosedByUser
            }]
        );
    }

    #[test]
    fn stop_and_try_now_are_the_users_buttons() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        assert!(book.try_now(1, T0 + 1_000));
        assert_eq!(run_to_attempt(&mut book, T0 + 1_000, 100).len(), 1, "no 10 s wait");
        // Parar no meio da tentativa: o resultado dela é ignorado.
        assert!(book.stop(1));
        assert_eq!(book.attempt_finished(1, AttemptOutcome::Launched { pid: 3 }, T0 + 2_000), None);
        assert!(book.views().is_empty());
        assert!(!book.stop(1));

        // Depois de desistir, "Tentar de novo" recomeça a contagem.
        book.on_drop(dropped(2, 200, lost()), T0);
        let mut now = T0;
        for attempt in 1..=RECONNECT_MAX_ATTEMPTS {
            now += reconnect_backoff_ms(attempt);
            run_to_attempt(&mut book, now, 200);
            book.attempt_finished(2, AttemptOutcome::Failed("x".into()), now);
        }
        assert!(book.try_now(2, now));
        let view = &book.views()[0];
        assert_eq!((view.phase, view.attempt, view.error.clone()), ("waiting", 1, None));
    }

    #[test]
    fn a_banned_account_has_no_try_again() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        run_to_attempt(&mut book, T0 + 10_000, 100);
        book.attempt_finished(
            1,
            AttemptOutcome::Stop(ReconnectStopReason::Banned, None),
            T0 + 11_000,
        );
        assert!(!book.try_now(1, T0 + 12_000));
    }

    #[test]
    fn a_new_drop_of_a_new_client_starts_over_after_a_final_state() {
        let mut book = ReconnectBook::new();
        book.on_drop(
            dropped(1, 100, drop_of(DropKind::Disconnected, Some(DropReason::JoinedElsewhere))),
            T0,
        );
        // A mesma queda de novo: nada muda.
        assert_eq!(book.on_drop(dropped(1, 100, lost()), T0 + 1_000), None);
        // A pessoa abriu a conta de novo e ela caiu: reconexão nova.
        let notice = book.on_drop(dropped(1, 400, lost()), T0 + 60_000);
        assert!(matches!(notice, Some(ReconnectNotice::Scheduled { attempt: 1, .. })));
    }

    #[test]
    fn active_means_something_still_in_progress() {
        let mut book = ReconnectBook::new();
        assert!(!book.is_active());
        book.on_drop(dropped(1, 100, lost()), T0);
        assert!(book.is_active());
        book.on_drop(
            dropped(1, 100, drop_of(DropKind::Disconnected, Some(DropReason::JoinedElsewhere))),
            T0,
        );
        assert!(!book.is_active(), "a stopped reconnect is only shown");
    }

    #[test]
    fn the_view_carries_what_the_session_page_shows() {
        let mut book = ReconnectBook::new();
        book.on_drop(dropped(1, 100, lost()), T0);
        let json = serde_json::to_value(book.views()).unwrap();
        assert_eq!(json[0]["userId"], 1);
        assert_eq!(json[0]["phase"], "waiting");
        assert_eq!(json[0]["attempt"], 1);
        assert_eq!(json[0]["maxAttempts"], 5);
        assert_eq!(json[0]["nextAttemptAtMs"], T0 + 10_000);
        assert_eq!(json[0]["drop"]["code"], 277);
        assert!(json[0]["reason"].is_null());
    }

    #[test]
    fn the_option_follows_the_account_then_the_default_and_nexus_turns_it_on() {
        assert!(!reconnect_enabled(None, false, false));
        assert!(reconnect_enabled(None, true, false));
        assert!(reconnect_enabled(Some("true"), false, false));
        assert!(!reconnect_enabled(Some("false"), true, false));
        assert!(reconnect_enabled(Some(""), true, false), "empty field follows the default");
        assert!(reconnect_enabled(Some("false"), false, true), "Nexus AutoRelaunch");
    }
}

#[cfg(test)]
mod auto_reconnect_target_tests {
    use super::*;

    fn launched(private: bool) -> LaunchedTarget {
        LaunchedTarget {
            place_id: 10,
            job_id: if private { "vip:CODE".into() } else { "job-a".into() },
            launch_data: "data".into(),
            join_vip: false,
            link_code: String::new(),
            private,
        }
    }

    fn seen(place_id: i64, job: Option<&str>) -> JoinedDestination {
        JoinedDestination {
            place_id,
            job_id: job.map(String::from),
        }
    }

    #[test]
    fn a_public_server_goes_back_to_the_place_and_job_in_the_log() {
        let t = reconnect_target(Some(&launched(false)), Some(&seen(10, Some("job-b"))), DropKind::Disconnected).unwrap();
        assert_eq!((t.place_id, t.job_id.as_str(), t.launch_data.as_str()), (10, "job-b", "data"));
        assert!(!t.join_vip && t.link_code.is_empty());
    }

    #[test]
    fn after_a_teleport_the_log_wins_and_the_launch_data_stays_behind() {
        let t = reconnect_target(Some(&launched(false)), Some(&seen(20, Some("job-c"))), DropKind::Kicked).unwrap();
        assert_eq!((t.place_id, t.job_id.as_str(), t.launch_data.as_str()), (20, "job-c", ""));
    }

    #[test]
    fn a_private_server_uses_the_same_launch_data_and_never_a_made_up_link() {
        let t = reconnect_target(Some(&launched(true)), Some(&seen(20, Some("reserved"))), DropKind::Disconnected).unwrap();
        assert_eq!(t.place_id, 10);
        assert_eq!(t.job_id, "vip:CODE");
        assert_eq!(t.launch_data, "data");
    }

    #[test]
    fn a_server_that_shut_down_goes_to_any_server_of_the_place() {
        let t = reconnect_target(None, Some(&seen(10, Some("gone"))), DropKind::ServerShutdown).unwrap();
        assert_eq!(t.job_id, "");
        let t = reconnect_target(Some(&launched(false)), None, DropKind::ServerShutdown).unwrap();
        assert_eq!(t.job_id, "");
    }

    #[test]
    fn without_a_log_it_is_what_the_app_launched_and_without_both_nothing() {
        let t = reconnect_target(Some(&launched(false)), None, DropKind::Crashed).unwrap();
        assert_eq!((t.place_id, t.job_id.as_str()), (10, "job-a"));
        assert_eq!(reconnect_target(None, None, DropKind::Crashed), None);
    }

    #[tokio::test]
    async fn any_http_answer_counts_as_internet() {
        let server = crate::api::endpoints::test_support::mock_server().await;
        let client = reqwest::Client::new();
        // Nada montado: o mock responde 404, e mesmo assim há internet.
        assert!(reachable_at(&client, &format!("{}/reconnect-probe/", server.uri())).await);
        // Porta fechada: erro de rede.
        assert!(!reachable_at(&client, "http://127.0.0.1:9/").await);
    }
}
