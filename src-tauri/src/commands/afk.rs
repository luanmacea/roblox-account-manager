// AFK mode — manda uma tecla, de tempo em tempo, para a janela de cada conta
// que o usuário colocou no modo. Ver docs/features/afk-mode.md.
//
// Não é detecção de interação: a API do Windows para isso responde pela sessão
// inteira do usuário, nunca por uma janela, então "só envia se aquela conta
// estiver parada" não é uma pergunta que este app possa responder. O que existe
// aqui é envio periódico, e só. (O nome dessa API está na lista de proibições do
// `afk_input_safety_tests`, no fim deste arquivo.)
//
// Duas regras mandam no desenho:
//
// 1. `SendInput` entrega na janela em **primeiro plano**. Para acertar o cliente
//    de uma conta, o ciclo traz aquela janela para frente, manda a tecla, passa
//    para a conta seguinte e só no fim devolve o foco para onde estava. O foco
//    fica fora da janela do usuário o ciclo inteiro — ~0,44 s por conta —, e a
//    tela diz isso com esses números.
// 2. O módulo só **envia** entrada. Ler teclado do usuário é proibido — a trava
//    é o `afk_input_safety_tests`, no fim deste arquivo.

/// Lista **fechada** de teclas que o AFK mode pode enviar, com a virtual key de
/// cada uma. O usuário escolhe de dentro dela; não existe campo para digitar
/// tecla arbitrária, e o backend recusa qualquer nome que não esteja aqui.
///
/// São teclas de movimento e de ação comuns em jogo do Roblox. Ficaram fora, de
/// propósito, as que fazem outra coisa na tela: Enter (abre o chat), Tab (troca
/// de janela), Escape (menu do Roblox) e F4 (fecha o cliente junto com Alt).
const AFK_KEYS: &[(&str, u16)] = &[
    ("Space", 0x20),
    ("W", 0x57),
    ("A", 0x41),
    ("S", 0x53),
    ("D", 0x44),
    ("E", 0x45),
    ("F", 0x46),
    ("R", 0x52),
    ("Q", 0x51),
    ("1", 0x31),
    ("2", 0x32),
    ("3", 0x33),
    ("4", 0x34),
    ("5", 0x35),
];

/// Os nomes da lista fechada, na ordem em que a tela os oferece.
fn afk_key_names() -> Vec<String> {
    AFK_KEYS.iter().map(|(name, _)| (*name).to_string()).collect()
}

/// Virtual key de uma tecla da lista. `None` para qualquer outro nome — é este
/// `None` que impede tecla de fora de chegar ao `SendInput`.
fn afk_virtual_key(key: &str) -> Option<u16> {
    AFK_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, vk)| *vk)
}

/// Intervalo entre envios da mesma conta, em segundos (a tela manda minutos e
/// segundos somados). O teto de 2 h é o mesmo da tela (120 min); o piso de 5 s
/// evita sessão que rouba o foco sem parar.
fn clamp_afk_interval_seconds(seconds: i64) -> u64 {
    seconds.clamp(5, 7_200) as u64
}

/// O intervalo em milissegundos, que é a unidade do relógio das contas.
fn afk_interval_ms(interval_seconds: u64) -> i64 {
    (interval_seconds as i64).saturating_mul(1_000)
}

/// O que o AFK mode manda para cada conta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfkMode {
    /// Toca uma tecla da lista fechada.
    Key,
    /// Clique esquerdo num ponto relativo da janela da conta. Existe para quem
    /// quer o personagem **parado**: toda tecla da lista mexe nele.
    Click,
}

impl AfkMode {
    /// Qualquer valor desconhecido vira `Key`: é o modo que já existia.
    fn parse(raw: &str) -> AfkMode {
        if raw.trim().eq_ignore_ascii_case("click") {
            AfkMode::Click
        } else {
            AfkMode::Key
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            AfkMode::Key => "key",
            AfkMode::Click => "click",
        }
    }
}

/// Ponto do clique em porcentagem da área interna da janela (0–100 nos dois
/// eixos). Relativo de propósito: cai no mesmo lugar com a janela pequena,
/// grande ou maximizada, e nenhuma janela precisa ser redimensionada.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AfkPoint {
    x_pct: f64,
    y_pct: f64,
}

const AFK_DEFAULT_POINT: AfkPoint = AfkPoint {
    x_pct: 50.0,
    y_pct: 50.0,
};

/// Porcentagem travada em 0–100; valor que não é número vira o meio.
fn clamp_afk_percent(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        50.0
    }
}

impl AfkPoint {
    fn clamped(x: f64, y: f64) -> Self {
        Self {
            x_pct: clamp_afk_percent(x),
            y_pct: clamp_afk_percent(y),
        }
    }
}

/// Área interna (cliente) de uma janela, em coordenadas de tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AfkClientRect {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

/// Pixel de tela do ponto, **sempre dentro** da área interna: 0% é a primeira
/// coluna/linha e 100% a última, nunca a borda de fora.
fn afk_point_to_pixel(rect: AfkClientRect, point: AfkPoint) -> Option<(i32, i32)> {
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let p = AfkPoint::clamped(point.x_pct, point.y_pct);
    let dx = ((p.x_pct / 100.0) * f64::from(rect.width - 1)).round() as i32;
    let dy = ((p.y_pct / 100.0) * f64::from(rect.height - 1)).round() as i32;
    Some((rect.left + dx, rect.top + dy))
}

/// O inverso, para o Marcar. `None` quando o cursor não está dentro da área
/// interna (em cima da borda, da barra de título ou fora da janela).
fn afk_pixel_to_point(rect: AfkClientRect, x: i32, y: i32) -> Option<AfkPoint> {
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let (dx, dy) = (x - rect.left, y - rect.top);
    if dx < 0 || dy < 0 || dx >= rect.width || dy >= rect.height {
        return None;
    }
    let span_x = f64::from((rect.width - 1).max(1));
    let span_y = f64::from((rect.height - 1).max(1));
    // Duas casas: o bastante para a ida e volta cair no mesmo pixel até em 4K.
    let round2 = |v: f64| (v * 100.0).round() / 100.0;
    Some(AfkPoint::clamped(
        round2(f64::from(dx) / span_x * 100.0),
        round2(f64::from(dy) / span_y * 100.0),
    ))
}

/// Ponto próprio da conta, dos campos `AfkClickX`/`AfkClickY`. Só vale com os
/// dois números: um sem o outro cai no padrão.
fn afk_point_from_fields(fields: &HashMap<String, String>) -> Option<AfkPoint> {
    let x = fields.get("AfkClickX")?.trim().parse::<f64>().ok()?;
    let y = fields.get("AfkClickY")?.trim().parse::<f64>().ok()?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some(AfkPoint::clamped(x, y))
}

/// O ponto de cada alvo: o próprio, se a conta tiver, senão o padrão.
fn afk_points_for_targets(
    targets: &[i64],
    default: AfkPoint,
    overrides: &HashMap<i64, AfkPoint>,
) -> HashMap<i64, AfkPoint> {
    targets
        .iter()
        .map(|uid| (*uid, overrides.get(uid).copied().unwrap_or(default)))
        .collect()
}

/// Resultado do Marcar: de que conta é a janela e onde, em porcentagem.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AfkCapturedPoint {
    user_id: i64,
    x_pct: f64,
    y_pct: f64,
}

/// O Marcar a partir das partes, para o teste não precisar de janela. `tracked`
/// é `(conta, PID)` dos clientes abertos por este app. O erro é um **código**; a
/// tela escreve a frase.
fn afk_capture_from_parts(
    cursor: (i32, i32),
    window_pid: Option<u32>,
    tracked: &[(i64, u32)],
    rect: Option<AfkClientRect>,
) -> Result<AfkCapturedPoint, String> {
    let pid = window_pid.ok_or("noWindow")?;
    let user_id = tracked
        .iter()
        .find(|(_, p)| *p == pid)
        .map(|(uid, _)| *uid)
        .ok_or("notAnAccountWindow")?;
    let rect = rect.ok_or("outsideGameArea")?;
    let point = afk_pixel_to_point(rect, cursor.0, cursor.1).ok_or("outsideGameArea")?;
    Ok(AfkCapturedPoint {
        user_id,
        x_pct: point.x_pct,
        y_pct: point.y_pct,
    })
}

/// Um passo da receita do clique. `MoveTo` é posição de tela absoluta, `Nudge`
/// é movimento relativo em pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfkMouseStep {
    MoveTo(i32, i32),
    Nudge(i32, i32),
    Wait(u64),
    Press,
    Release,
}

/// Tamanho do tremor, em pixels.
const AFK_CLICK_JITTER_PX: i32 = 2;
/// Espera depois de chegar ao ponto, para o jogo assentar o mouse lá.
const AFK_CLICK_SETTLE_MS: u64 = 500;
/// Espera entre o clique de foco e o clique de verdade.
const AFK_CLICK_FOCUS_SETTLE_MS: u64 = 200;
/// Pausa de ~1 quadro entre a posição final e o botão descer.
const AFK_CLICK_FINAL_PAUSE_MS: u64 = 12;

/// A receita do clique no ponto `point` da área interna `rect`.
///
/// O Roblox lê o mouse pelo caminho de entrada crua do sistema: pôr o cursor no
/// lugar sem gerar movimento não chega ao jogo, e um clique parado cai onde o
/// jogo acha que o mouse estava. A receita é a que funcionou no bot de Robeats
/// do dono:
///
/// 1. chega ao ponto por movimento de entrada, com um micro-desvio de 1 px e
///    volta, e espera o jogo assentar;
/// 2. treme ±2 px em movimento relativo e reafirma a posição exata;
/// 3. clica — e esse primeiro clique pode só focar o jogo;
/// 4. espera, treme de novo e dá o clique de verdade.
///
/// Todo desvio vai para **dentro** da janela, inclusive nas bordas. `None` para
/// janela sem área.
fn afk_click_plan(rect: AfkClientRect, point: AfkPoint, hold_ms: u64) -> Option<Vec<AfkMouseStep>> {
    use AfkMouseStep::*;
    let (x, y) = afk_point_to_pixel(rect, point)?;
    // Desvio para o lado de dentro: na última coluna, para a esquerda.
    let inward = |at: i32, start: i32, len: i32, step: i32| {
        if at + step <= start + len - 1 {
            step
        } else {
            -step
        }
    };
    let micro = inward(x, rect.left, rect.width, 1);
    let jitter = inward(x, rect.left, rect.width, AFK_CLICK_JITTER_PX);
    // Janela de 1 px de largura não tem para onde tremer.
    let (micro, jitter) = if rect.width < 3 { (0, 0) } else { (micro, jitter) };

    let click = |steps: &mut Vec<AfkMouseStep>, settle: bool| {
        steps.extend([MoveTo(x, y), MoveTo(x + micro, y), MoveTo(x, y)]);
        if settle {
            steps.push(Wait(AFK_CLICK_SETTLE_MS));
        }
        steps.extend([
            Nudge(jitter, 0),
            Nudge(-jitter, 0),
            MoveTo(x, y),
            Wait(AFK_CLICK_FINAL_PAUSE_MS),
            Press,
            Wait(hold_ms),
            Release,
        ]);
    };

    let mut steps = Vec::new();
    click(&mut steps, true);
    steps.push(Wait(AFK_CLICK_FOCUS_SETTLE_MS));
    click(&mut steps, false);
    Some(steps)
}

/// Pixel de tela na escala do envio absoluto (0..65535 sobre a área de trabalho
/// virtual, todos os monitores). Fora da área é travado.
fn afk_absolute_input(x: i32, y: i32, desktop: AfkClientRect) -> (i32, i32) {
    let scale = |at: i32, start: i32, len: i32| {
        let span = f64::from((len - 1).max(1));
        ((f64::from(at - start) * 65535.0 / span).round() as i32).clamp(0, 65535)
    };
    (scale(x, desktop.left, desktop.width), scale(y, desktop.top, desktop.height))
}

/// O que o ciclo faz em cada conta. No clique o ponto já vem resolvido por
/// conta (o próprio ou o padrão), então o corpo bloqueante não lê store nenhum.
#[derive(Debug, Clone)]
enum AfkCycleAction {
    Key(String),
    Click(HashMap<i64, AfkPoint>),
}

/// Por que um start não pode acontecer. No modo tecla, sem tecla escolhida o
/// modo **não liga**: inventar uma tecla padrão seria mexer no personagem sem o
/// usuário pedir. O modo clique não usa tecla.
fn validate_afk_start(mode: AfkMode, key: &str, user_ids: &[i64]) -> Result<(), String> {
    if mode == AfkMode::Key && afk_virtual_key(key).is_none() {
        return Err("Choose one of the AFK mode keys before starting".into());
    }
    if user_ids.is_empty() {
        return Err("Put at least one account in AFK mode before starting".into());
    }
    Ok(())
}

/// Por que uma conta não recebeu a tecla. O código vai para a tela, que escreve
/// a frase traduzida — a mensagem em inglês fica para log e para caso novo.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AfkSendError {
    /// A conta não tem janela de cliente de pé.
    NoWindow,
    /// O Windows recusou trazer a janela para o primeiro plano. **Nada foi
    /// enviado**: a tecla cairia na janela que o usuário está usando.
    FocusDenied,
    /// O `SendInput` foi recusado (ou o "solta a tecla" não passou).
    KeyRefused,
    /// O clique foi recusado (ou o "solta o botão" não passou).
    ClickRefused,
    /// Falha inesperada do ciclo.
    Internal(String),
}

impl AfkSendError {
    fn code(&self) -> &'static str {
        match self {
            AfkSendError::NoWindow => "noWindow",
            AfkSendError::FocusDenied => "focusDenied",
            AfkSendError::KeyRefused => "keyRefused",
            AfkSendError::ClickRefused => "clickRefused",
            AfkSendError::Internal(_) => "internal",
        }
    }

    fn message(&self) -> String {
        match self {
            AfkSendError::NoWindow => "No Roblox window for this account".into(),
            AfkSendError::FocusDenied => {
                "Windows did not bring this account's Roblox window to the front, so nothing was sent"
                    .into()
            }
            AfkSendError::KeyRefused => "Windows refused the synthetic key".into(),
            AfkSendError::ClickRefused => "Windows refused the synthetic click".into(),
            AfkSendError::Internal(message) => message.clone(),
        }
    }
}

/// Estado de uma conta que **está** no AFK mode.
#[derive(Debug, Clone)]
struct AfkAccountRuntime {
    user_id: i64,
    /// Último envio (ou a entrada no modo, enquanto não houve envio). É daqui
    /// que sai o "está na hora desta conta?".
    last_send_at_ms: i64,
    sends: u64,
    last_error: Option<AfkSendError>,
}

impl AfkAccountRuntime {
    /// Conta entrando no modo agora: o relógio dela começa aqui, então o
    /// primeiro envio só sai depois de um intervalo inteiro.
    fn joined(user_id: i64, at_ms: i64) -> Self {
        Self {
            user_id,
            last_send_at_ms: at_ms,
            sends: 0,
            last_error: None,
        }
    }
}

/// Está na hora desta conta? Só o tempo decorrido desde o último envio decide.
/// Relógio do sistema andando para trás dá diferença negativa — e aí não é hora.
fn afk_is_due(last_send_at_ms: i64, interval_ms: i64, now_ms: i64) -> bool {
    interval_ms > 0 && now_ms.saturating_sub(last_send_at_ms) >= interval_ms
}

fn afk_next_send_at_ms(last_send_at_ms: i64, interval_ms: i64) -> i64 {
    last_send_at_ms.saturating_add(interval_ms)
}

/// As contas que o ciclo deve visitar agora, em ordem estável de user id.
///
/// O mapa é o da sessão: conta que **não** está no AFK mode não tem entrada
/// nele e portanto nunca vira alvo, por mais tempo que passe.
fn afk_due_targets(
    accounts: &HashMap<i64, AfkAccountRuntime>,
    interval_ms: i64,
    now_ms: i64,
) -> Vec<i64> {
    let mut due: Vec<i64> = accounts
        .values()
        .filter(|entry| afk_is_due(entry.last_send_at_ms, interval_ms, now_ms))
        .map(|entry| entry.user_id)
        .collect();
    due.sort_unstable();
    due
}

/// Remarca o relógio de quem o ciclo visitou com `finished_at_ms`, o **fim** do
/// ciclo: a espera até o próximo envio conta de quando o ciclo acabou ("10
/// segundos depois que um ciclo acabar"), então o intervalo efetivo de uma conta
/// é o intervalo mais a duração do ciclo.
///
/// Só quem o ciclo visitou é remarcado: uma parada no meio deixa o resto vencido,
/// como estava. Ciclo que falhou inteiro remarca todos os alvos com o erro.
fn afk_record_cycle(
    accounts: &mut HashMap<i64, AfkAccountRuntime>,
    targets: &[i64],
    result: &Result<Vec<(i64, Option<AfkSendError>)>, String>,
    finished_at_ms: i64,
) {
    match result {
        Ok(outcome) => {
            for (user_id, error) in outcome {
                if let Some(entry) = accounts.get_mut(user_id) {
                    entry.last_send_at_ms = finished_at_ms;
                    if error.is_none() {
                        entry.sends += 1;
                    }
                    entry.last_error = error.clone();
                }
            }
        }
        Err(error) => {
            for user_id in targets {
                if let Some(entry) = accounts.get_mut(user_id) {
                    entry.last_send_at_ms = finished_at_ms;
                    entry.last_error = Some(AfkSendError::Internal(error.clone()));
                }
            }
        }
    }
}

/// O que o ciclo faz com o próximo alvo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfkCycleStep {
    /// Sessão parando: o ciclo em andamento é abandonado onde está, sem enviar
    /// o que faltava.
    Abort,
    /// A conta não tem janela de Roblox de pé: pula sem enviar nada e sem mexer
    /// em janela de ninguém.
    Skip,
    Send,
}

fn afk_cycle_step(stopping: bool, window_alive: bool) -> AfkCycleStep {
    if stopping {
        AfkCycleStep::Abort
    } else if !window_alive {
        AfkCycleStep::Skip
    } else {
        AfkCycleStep::Send
    }
}

/// A janela do alvo chegou de fato ao primeiro plano?
///
/// `SendInput` entrega na janela em **primeiro plano**, e o Windows recusa
/// `SetForegroundWindow` de processo que não está em primeiro plano nem recebeu o
/// último evento de entrada — que é o caso normal do AFK mode, com o app em
/// segundo plano. Sem esta conferência a tecla ia, **todo ciclo**, para a janela
/// em que o usuário está digitando, e o ciclo se declarava bem-sucedido.
///
/// `foreground` e `target` são handles de janela em forma de número; `0` é
/// "nenhuma janela", e alvo nulo nunca está pronto.
fn afk_window_is_ready(focus_requested: bool, foreground: isize, target: isize) -> bool {
    focus_requested && target != 0 && foreground == target
}

/// A janela volta a ser minimizada? Trazer para frente desminimiza (o
/// `focus_window` faz `SW_RESTORE` em janela minimizada — e só nela: maximizada
/// continua maximizada); quem trabalha com os clientes minimizados
/// não pediu para vê-los na tela. Só vale para janela que o ciclo mexeu, e só se
/// o usuário a tinha minimizado.
fn afk_should_reminimize(was_minimized: bool, focus_attempted: bool) -> bool {
    was_minimized && focus_attempted
}

/// As contas de um envio manual: **interseção** com quem está no AFK mode, na
/// ordem que o usuário pediu.
///
/// Sem isso o botão "enviar agora" puxava para frente e teclava cliente de conta
/// que nunca entrou no modo — o único ponto que contrariava a regra de não mexer
/// em cliente de conta fora do modo.
fn afk_manual_targets(requested: &[i64], accounts: &HashMap<i64, AfkAccountRuntime>) -> Vec<i64> {
    requested
        .iter()
        .copied()
        .filter(|user_id| accounts.contains_key(user_id))
        .collect()
}

/// Devolver o foco é coisa de ciclo que terminou: quem está parando não mexe em
/// foco nenhum, e ciclo que não roubou o foco não tem o que devolver.
fn afk_should_restore_focus(stopping: bool, focus_taken: bool) -> bool {
    !stopping && focus_taken
}

// ── tela cheia na frente (ideia 25) ─────────────────────────────────────────
//
// Com um vídeo ou outro jogo em tela cheia na frente, trazer a janela do Roblox
// tiraria a pessoa do que ela está vendo. O ciclo espera (confere de novo a cada
// tique) até a tela cheia sair — ou até o teto, para a conta não cair por
// inatividade. Só geometria de janela: qual está na frente, se ela cobre o
// monitor dela inteiro e de que processo ela é. Nada de entrada é lido.

/// Quanto o ciclo espera, no máximo, depois da hora de uma conta. O Roblox
/// derruba quem fica parado 20 min; com o intervalo padrão de 10 min, 5 min de
/// espera ainda deixam folga.
const AFK_FULLSCREEN_MAX_WAIT_MS: i64 = 5 * 60_000;

/// `Afk.WaitForFullscreen`: ligada por padrão (protege quem está vendo algo em
/// tela cheia); só `"false"` desliga.
fn afk_wait_for_fullscreen_enabled(raw: &str) -> bool {
    raw.trim() != "false"
}

/// A janela da frente segura o ciclo? Só se cobre o monitor inteiro (a tela
/// cheia de um vídeo ou de outro jogo) e não é de um cliente que o app abriu,
/// da área de trabalho (que também cobre o monitor) nem do próprio MultiAlt.
/// Cliente aberto pelo site **segura**: é a pessoa jogando.
fn afk_foreground_blocks(
    foreground_pid: Option<u32>,
    covers_monitor: bool,
    app_client_pids: &HashSet<u32>,
    shell_pids: &HashSet<u32>,
    own_pid: u32,
) -> bool {
    let Some(pid) = foreground_pid else {
        return false;
    };
    covers_monitor && pid != own_pid && !app_client_pids.contains(&pid) && !shell_pids.contains(&pid)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfkGate {
    Send,
    Wait,
}

/// Envia agora ou espera a tela cheia sair? `due_since_ms`: desde quando a
/// conta mais atrasada do ciclo está na hora.
fn afk_fullscreen_gate(
    enabled: bool,
    fullscreen_in_front: bool,
    due_since_ms: i64,
    now_ms: i64,
    max_wait_ms: i64,
) -> AfkGate {
    if !enabled || !fullscreen_in_front {
        return AfkGate::Send;
    }
    if now_ms.saturating_sub(due_since_ms) >= max_wait_ms {
        AfkGate::Send
    } else {
        AfkGate::Wait
    }
}

/// Desde quando o alvo mais atrasado está na hora (último envio + intervalo).
fn afk_due_since(
    accounts: &HashMap<i64, AfkAccountRuntime>,
    targets: &[i64],
    interval_ms: i64,
    now_ms: i64,
) -> i64 {
    targets
        .iter()
        .filter_map(|uid| accounts.get(uid))
        .map(|entry| afk_next_send_at_ms(entry.last_send_at_ms, interval_ms))
        .min()
        .unwrap_or(now_ms)
}

/// A janela da frente agora segura o ciclo? Primeiro o barato (cobre o
/// monitor?); só então a lista de processos.
#[cfg(target_os = "windows")]
fn afk_fullscreen_in_front() -> bool {
    use platform::windows;
    let foreground = windows::get_foreground_hwnd();
    if foreground.is_null() {
        return false;
    }
    let covers = windows::window_mode_of(foreground) == Some(windows::WindowMode::Fullscreen);
    if !covers {
        return false;
    }
    let app_clients: HashSet<u32> = windows::tracker()
        .get_all()
        .into_iter()
        .filter(|process| !process.adopted)
        .map(|process| process.pid)
        .collect();
    let shell: HashSet<u32> = windows::get_shell_pids().into_iter().collect();
    afk_foreground_blocks(
        windows::window_pid(foreground),
        covers,
        &app_clients,
        &shell,
        std::process::id(),
    )
}

#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct AfkAccountStatus {
    user_id: i64,
    last_send_at_ms: i64,
    next_send_at_ms: i64,
    sends: u64,
    last_error: Option<String>,
    /// `noWindow`, `focusDenied`, `keyRefused`, `clickRefused` ou `internal` — a
    /// tela escolhe a frase traduzida por aqui, em vez de casar texto em inglês.
    last_error_code: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AfkStatusPayload {
    active: bool,
    started_at_ms: Option<i64>,
    /// Segundos entre dois envios da mesma conta, contados do fim do ciclo.
    interval_seconds: u64,
    key: String,
    /// `key` ou `click`.
    mode: String,
    /// Ponto padrão do modo clique, em porcentagem.
    click_x: f64,
    click_y: f64,
    accounts: Vec<AfkAccountStatus>,
    /// O ciclo está na hora mas espera: há uma janela em tela cheia na frente
    /// (`afk_fullscreen_gate`).
    waiting_fullscreen: bool,
}

impl Default for AfkStatusPayload {
    /// Sem sessão: o que a tela mostra antes de ligar.
    fn default() -> Self {
        afk_status_from_parts(None, 0, "", AfkMode::Key, AFK_DEFAULT_POINT, &HashMap::new())
    }
}

/// Tempo que a janela fica em frente antes da tecla sair: sem essa folga o
/// `SendInput` chega antes de o Roblox virar a janela em foco e a tecla cai na
/// janela anterior.
#[cfg(target_os = "windows")]
const AFK_FOCUS_SETTLE_MS: u64 = 150;
/// Quanto a tecla fica pressionada. Toque curto: o objetivo é o jogo registrar
/// entrada, não andar com o personagem.
#[cfg(target_os = "windows")]
const AFK_KEY_HOLD_MS: u64 = 40;
/// Respiro entre duas contas do mesmo ciclo.
///
/// Folga + tecla + respiro dão ~0,44 s por conta, e o foco só volta para a
/// janela do usuário no fim do ciclo: é o "cerca de meio segundo cada" e o "uns
/// 4 segundos com 10 contas" do `AfkDialog`
/// (`a_cycle_keeps_the_focus_about_half_a_second_per_account`).
#[cfg(target_os = "windows")]
const AFK_BETWEEN_WINDOWS_MS: u64 = 250;
/// De quanto em quanto tempo o laço olha o relógio das contas.
#[cfg(target_os = "windows")]
const AFK_TICK_MS: i64 = 1_000;

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct AfkConfig {
    /// Segundos entre dois envios da mesma conta (`clamp_afk_interval_seconds`).
    interval_seconds: u64,
    key: String,
    mode: AfkMode,
    /// Ponto de quem não tem ponto próprio (modo clique).
    default_point: AfkPoint,
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct AfkSession {
    id: u64,
    stop_flag: Arc<AtomicBool>,
    /// Ligado pelo laço quando ele realmente saiu. Quem manda parar olha isto
    /// antes de esperar: sem ele, laço que terminou **antes** de o `notified()`
    /// ser registrado fazia o parar esperar os 2 s inteiros.
    stopped: Arc<AtomicBool>,
    stopped_notify: Arc<tokio::sync::Notify>,
    started_at_ms: i64,
    config: Arc<Mutex<AfkConfig>>,
    accounts: Arc<Mutex<HashMap<i64, AfkAccountRuntime>>>,
    /// Ciclo segurado por uma janela em tela cheia na frente.
    waiting_fullscreen: Arc<AtomicBool>,
}

#[cfg(target_os = "windows")]
struct AfkManager {
    session: Mutex<Option<AfkSession>>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    next_id: AtomicU64,
}

#[cfg(target_os = "windows")]
impl AfkManager {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            task: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    fn next_session_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn get_session(&self) -> Option<AfkSession> {
        self.session.lock().ok().and_then(|s| s.as_ref().cloned())
    }

    fn replace_session(&self, session: Option<AfkSession>) {
        if let Ok(mut guard) = self.session.lock() {
            *guard = session;
        }
    }

    fn set_task(&self, handle: tauri::async_runtime::JoinHandle<()>) {
        if let Ok(mut guard) = self.task.lock() {
            if let Some(previous) = guard.replace(handle) {
                previous.abort();
            }
        }
    }

    fn abort_task(&self) {
        if let Ok(mut guard) = self.task.lock() {
            if let Some(previous) = guard.take() {
                previous.abort();
            }
        }
    }
}

#[cfg(target_os = "windows")]
static AFK_MANAGER: LazyLock<AfkManager> = LazyLock::new(AfkManager::new);

/// Um ciclo por vez. O laço da sessão e o "enviar agora" do usuário disputam as
/// mesmas janelas e o mesmo foco: dois ciclos ao mesmo tempo mandariam tecla
/// para a janela que o outro acabou de trazer para frente.
#[cfg(target_os = "windows")]
static AFK_CYCLE_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// O mesmo "um ciclo por vez", mas do lado bloqueante: `abort` numa task de
/// `spawn_blocking` não para a closure que já começou, então o lock assíncrono
/// sozinho não impede dois corpos de ciclo se sobreporem.
#[cfg(target_os = "windows")]
static AFK_CYCLE_SEQ: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Sessão nova. Cada conta entra com o relógio marcando `started_at_ms`, e é o
/// que faz a tela **já saber** quando é o primeiro envio (`started_at` +
/// intervalo) antes de qualquer tecla sair: sem isso, quem liga o modo passa o
/// intervalo inteiro olhando um "--" sem saber se funcionou.
#[cfg(target_os = "windows")]
fn new_afk_session(
    id: u64,
    started_at_ms: i64,
    config: AfkConfig,
    user_ids: &[i64],
) -> AfkSession {
    let accounts: HashMap<i64, AfkAccountRuntime> = user_ids
        .iter()
        .map(|uid| (*uid, AfkAccountRuntime::joined(*uid, started_at_ms)))
        .collect();
    AfkSession {
        id,
        stop_flag: Arc::new(AtomicBool::new(false)),
        stopped: Arc::new(AtomicBool::new(false)),
        stopped_notify: Arc::new(tokio::sync::Notify::new()),
        started_at_ms,
        config: Arc::new(Mutex::new(config)),
        accounts: Arc::new(Mutex::new(accounts)),
        waiting_fullscreen: Arc::new(AtomicBool::new(false)),
    }
}

/// O status que a tela recebe, montado a partir das partes — separado de
/// `afk_status_from` para o teste poder montar um sem sessão viva.
fn afk_status_from_parts(
    started_at_ms: Option<i64>,
    interval_seconds: u64,
    key: &str,
    mode: AfkMode,
    default_point: AfkPoint,
    accounts: &HashMap<i64, AfkAccountRuntime>,
) -> AfkStatusPayload {
    let interval_ms = afk_interval_ms(interval_seconds);
    let mut rows: Vec<AfkAccountStatus> = accounts
        .values()
        .map(|entry| AfkAccountStatus {
            user_id: entry.user_id,
            last_send_at_ms: entry.last_send_at_ms,
            next_send_at_ms: afk_next_send_at_ms(entry.last_send_at_ms, interval_ms),
            sends: entry.sends,
            last_error: entry.last_error.as_ref().map(|e| e.message()),
            last_error_code: entry.last_error.as_ref().map(|e| e.code().to_string()),
        })
        .collect();
    rows.sort_by_key(|a| a.user_id);

    AfkStatusPayload {
        active: started_at_ms.is_some(),
        started_at_ms,
        interval_seconds,
        key: key.to_string(),
        mode: mode.as_str().to_string(),
        click_x: default_point.x_pct,
        click_y: default_point.y_pct,
        accounts: rows,
        waiting_fullscreen: false,
    }
}

#[cfg(target_os = "windows")]
fn afk_status_from(session: &AfkSession) -> AfkStatusPayload {
    let config = session
        .config
        .lock()
        .map(|c| c.clone())
        .unwrap_or_else(|_| AfkConfig {
            interval_seconds: 0,
            key: String::new(),
            mode: AfkMode::Key,
            default_point: AFK_DEFAULT_POINT,
        });
    let accounts = session
        .accounts
        .lock()
        .map(|map| map.clone())
        .unwrap_or_default();

    let mut status = afk_status_from_parts(
        Some(session.started_at_ms),
        config.interval_seconds,
        &config.key,
        config.mode,
        config.default_point,
        &accounts,
    );
    status.waiting_fullscreen = session.waiting_fullscreen.load(Ordering::Relaxed);
    status
}

#[cfg(target_os = "windows")]
fn current_afk_status() -> AfkStatusPayload {
    match AFK_MANAGER.get_session() {
        Some(session) => afk_status_from(&session),
        None => AfkStatusPayload::default(),
    }
}

#[cfg(not(target_os = "windows"))]
fn current_afk_status() -> AfkStatusPayload {
    AfkStatusPayload::default()
}

#[cfg(target_os = "windows")]
fn emit_afk_status(app: &tauri::AppHandle) {
    let _ = app.emit("afk-status", current_afk_status());
}

/// Ciclo concluído, com quantas contas receberam a tecla. É o gancho do aviso
/// sonoro opcional da tela: o usuário está usando o PC e o piscar do foco fica
/// sem explicação se nada avisa que foi o app.
#[cfg(target_os = "windows")]
fn emit_afk_cycle(app: &tauri::AppHandle, sent: u32) {
    let _ = app.emit("afk-cycle", serde_json::json!({ "sent": sent }));
}

/// Uma passada de envio pelas contas vencidas. Devolve, por conta, o erro que
/// houve (ou `None` quando a tecla saiu).
///
/// Só toca em janela de conta que está no AFK mode: o alvo vem do tracker, que
/// sabe qual PID é de qual conta. Nada aqui fecha, mata ou minimiza janela.
#[cfg(target_os = "windows")]
fn run_afk_cycle_blocking(
    action: &AfkCycleAction,
    targets: &[i64],
    stop_flag: &AtomicBool,
) -> Result<Vec<(i64, Option<AfkSendError>)>, String> {
    use platform::windows;

    // Um ciclo por vez **de verdade**: `JoinHandle::abort` não interrompe uma
    // closure de `spawn_blocking` que já começou, então o lock assíncrono lá fora
    // pode ser liberado com este corpo ainda rodando.
    let _serial = AFK_CYCLE_SEQ
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let AfkCycleAction::Key(key) = action {
        if afk_virtual_key(key).is_none() {
            return Err(format!("Key not allowed in AFK mode: {}", key));
        }
    }

    let previous_foreground = windows::get_foreground_hwnd();
    // PID de cliente que já morreu pode ter sido reaproveitado pelo Windows por
    // outro programa qualquer: só vale PID que ainda é um Roblox.
    let alive: HashSet<u32> = windows::get_roblox_pids().into_iter().collect();
    let tracker = windows::tracker();

    let mut focus_taken = false;
    // Janelas que o ciclo trouxe para frente: se no fim a da frente não é
    // nenhuma delas, foi o usuário que escolheu outra (issue #23).
    let mut cycle_windows = Vec::new();
    let mut outcome: Vec<(i64, Option<AfkSendError>)> = Vec::new();

    for &user_id in targets {
        let hwnd = tracker
            .get_pid(user_id)
            .filter(|pid| alive.contains(pid))
            .and_then(windows::find_main_window)
            .filter(|hwnd| windows::window_exists(*hwnd));

        match afk_cycle_step(stop_flag.load(Ordering::Relaxed), hwnd.is_some()) {
            AfkCycleStep::Abort => break,
            AfkCycleStep::Skip => {
                outcome.push((user_id, Some(AfkSendError::NoWindow)));
            }
            AfkCycleStep::Send => {
                let hwnd = match hwnd {
                    Some(hwnd) => hwnd,
                    None => continue,
                };
                // Quem trabalha com os clientes minimizados não pediu para
                // vê-los: o estado é devolvido depois do envio.
                let was_minimized = windows::window_is_minimized(hwnd);
                // Recusado (o usuário mexeu em outra janela), tenta de novo
                // depois de um movimento de mouse de zero pixel — ver
                // `bring_forward_for_cycle`.
                let requested = windows::bring_forward_for_cycle(hwnd);
                focus_taken = true;
                cycle_windows.push(hwnd);
                std::thread::sleep(std::time::Duration::from_millis(AFK_FOCUS_SETTLE_MS));

                let ready = afk_window_is_ready(
                    requested,
                    windows::get_foreground_hwnd() as isize,
                    hwnd as isize,
                );

                let result = if !ready {
                    // O Windows não deixou a janela vir para frente. Tecla ou
                    // clique aqui cairiam na janela do usuário.
                    Some(AfkSendError::FocusDenied)
                } else {
                    match action {
                        AfkCycleAction::Key(key) => windows::tap_afk_key(key, AFK_KEY_HOLD_MS)
                            .err()
                            .map(|_| AfkSendError::KeyRefused),
                        AfkCycleAction::Click(points) => {
                            let point = points.get(&user_id).copied().unwrap_or(AFK_DEFAULT_POINT);
                            windows::click_afk_point(hwnd, point, AFK_KEY_HOLD_MS)
                                .err()
                                .map(|_| AfkSendError::ClickRefused)
                        }
                    }
                };

                if afk_should_reminimize(was_minimized, true) {
                    windows::minimize_window(hwnd);
                }
                outcome.push((user_id, result));
                std::thread::sleep(std::time::Duration::from_millis(AFK_BETWEEN_WINDOWS_MS));
            }
        }
    }

    if afk_should_restore_focus(stop_flag.load(Ordering::Relaxed), focus_taken)
        && windows::window_exists(previous_foreground)
    {
        // Só o primeiro plano volta: a janela do usuário não é restaurada nem
        // desmaximizada (o `focus_window` restauraria uma janela minimizada).
        // A volta é conferida na tela e repetida se o Windows recusar — o que
        // acontece com o usuário mexendo em outra janela durante o ciclo.
        let back = windows::give_focus_back_after_cycle(previous_foreground, &cycle_windows);
        if back == windows::GiveBack::Denied {
            eprintln!("[afk] o Windows recusou devolver o foco à janela anterior");
        }
    }

    Ok(outcome)
}

/// A ação do ciclo. O ponto próprio de cada conta é lido **agora**, dos campos
/// dela: mudar o ponto de uma conta vale no ciclo seguinte, sem religar o modo.
#[cfg(target_os = "windows")]
fn afk_cycle_action(app: &tauri::AppHandle, config: &AfkConfig, targets: &[i64]) -> AfkCycleAction {
    match config.mode {
        AfkMode::Key => AfkCycleAction::Key(config.key.clone()),
        AfkMode::Click => {
            let overrides: HashMap<i64, AfkPoint> = app
                .state::<AccountStore>()
                .get_all()
                .unwrap_or_default()
                .into_iter()
                .filter(|account| targets.contains(&account.user_id))
                .filter_map(|account| {
                    afk_point_from_fields(&account.fields).map(|point| (account.user_id, point))
                })
                .collect();
            AfkCycleAction::Click(afk_points_for_targets(
                targets,
                config.default_point,
                &overrides,
            ))
        }
    }
}

#[cfg(target_os = "windows")]
async fn run_afk_session(app: tauri::AppHandle, session: AfkSession) {
    loop {
        if session.stop_flag.load(Ordering::Relaxed) {
            break;
        }

        let Ok(config) = session.config.lock().map(|c| c.clone()) else {
            break;
        };
        let interval_ms = afk_interval_ms(config.interval_seconds);
        let Ok(targets) = session
            .accounts
            .lock()
            .map(|map| afk_due_targets(&map, interval_ms, now_ms()))
        else {
            break;
        };

        // Tela cheia de outro programa na frente: espera em vez de roubar o
        // foco (ideia 25). Confere de novo a cada tique, até o teto.
        let gate = if targets.is_empty() {
            AfkGate::Send
        } else {
            let enabled = afk_wait_for_fullscreen_enabled(
                &app.state::<SettingsStore>().get_string("Afk", "WaitForFullscreen"),
            );
            let in_front = enabled
                && tokio::task::spawn_blocking(afk_fullscreen_in_front)
                    .await
                    .unwrap_or(false);
            let now = now_ms();
            let due_since = session
                .accounts
                .lock()
                .map(|map| afk_due_since(&map, &targets, interval_ms, now))
                .unwrap_or(now);
            afk_fullscreen_gate(enabled, in_front, due_since, now, AFK_FULLSCREEN_MAX_WAIT_MS)
        };
        let waiting = gate == AfkGate::Wait;
        if session.waiting_fullscreen.swap(waiting, Ordering::Relaxed) != waiting {
            if waiting {
                emit_session_log(
                    &app,
                    "info",
                    "afk",
                    String::from("Modo AFK esperando: há uma janela em tela cheia na frente"),
                );
            }
            emit_afk_status(&app);
        }

        if !targets.is_empty() && !waiting {
            let action = afk_cycle_action(&app, &config, &targets);
            let stop = session.stop_flag.clone();
            let cycle_targets = targets.clone();
            let result = {
                let _guard = AFK_CYCLE_LOCK.lock().await;
                tokio::task::spawn_blocking(move || {
                    run_afk_cycle_blocking(&action, &cycle_targets, &stop)
                })
                .await
                .unwrap_or_else(|e| Err(format!("AFK cycle failed: {}", e)))
            };
            // Lido **depois** do ciclo: a espera até o próximo envio conta do fim
            // do ciclo, que foi o pedido do dono ("10 segundos depois que um
            // ciclo acabar"). O intervalo efetivo de cada conta fica sendo o
            // intervalo mais a duração do ciclo (~0,44 s por conta visitada), e
            // isso é de propósito: com intervalo curto, marcar no começo
            // deixaria quase nenhuma folga entre um ciclo longo e o seguinte.
            let finished_at = now_ms();
            let sent = match &result {
                Ok(outcome) => outcome.iter().filter(|(_, error)| error.is_none()).count() as u32,
                Err(_) => 0,
            };

            if let Ok(mut map) = session.accounts.lock() {
                afk_record_cycle(&mut map, &targets, &result, finished_at);
            }
            emit_afk_status(&app);
            if sent > 0 {
                emit_afk_cycle(&app, sent);
            }
        }

        sleep_interruptible(&session.stop_flag, AFK_TICK_MS).await;
    }

    // Sessão trocada por outra enquanto esta terminava: quem limpa é a nova.
    let owns_session = AFK_MANAGER
        .get_session()
        .map(|s| s.id == session.id)
        .unwrap_or(false);
    if owns_session {
        AFK_MANAGER.replace_session(None);
    }
    session.stopped.store(true, Ordering::SeqCst);
    // `notify_one` guarda a permissão: quem for esperar depois disto não fica
    // preso até o timeout.
    session.stopped_notify.notify_one();
    let _ = app.emit("afk-stopped", ());
    emit_afk_status(&app);
}

/// Manda a sessão parar e espera até 2 s ela confirmar. Depois disso a sessão é
/// descartada de qualquer jeito: sessão parada à força não fica no caminho da
/// próxima.
#[cfg(target_os = "windows")]
async fn stop_afk_session() {
    let Some(session) = AFK_MANAGER.get_session() else {
        AFK_MANAGER.abort_task();
        return;
    };
    session.stop_flag.store(true, Ordering::SeqCst);
    if !session.stopped.load(Ordering::SeqCst) {
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            session.stopped_notify.notified(),
        )
        .await;
    }
    AFK_MANAGER.abort_task();
    AFK_MANAGER.replace_session(None);
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn start_afk_mode(
    app: tauri::AppHandle,
    user_ids: Vec<i64>,
    interval_seconds: i64,
    key: String,
    mode: String,
    click_x: f64,
    click_y: f64,
) -> Result<AfkStatusPayload, String> {
    let user_ids = dedupe_preserving_order(user_ids);
    let mode = AfkMode::parse(&mode);
    validate_afk_start(mode, &key, &user_ids)?;

    stop_afk_session().await;

    let session = new_afk_session(
        AFK_MANAGER.next_session_id(),
        now_ms(),
        AfkConfig {
            interval_seconds: clamp_afk_interval_seconds(interval_seconds),
            key,
            mode,
            default_point: AfkPoint::clamped(click_x, click_y),
        },
        &user_ids,
    );

    AFK_MANAGER.replace_session(Some(session.clone()));
    let handle = tauri::async_runtime::spawn(run_afk_session(app.clone(), session));
    AFK_MANAGER.set_task(handle);

    let status = current_afk_status();
    emit_afk_status(&app);
    Ok(status)
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn stop_afk_mode(app: tauri::AppHandle) -> Result<(), String> {
    stop_afk_session().await;
    emit_afk_status(&app);
    Ok(())
}

/// Liga e desliga contas de uma sessão em andamento. Conta que sai para de
/// receber tecla; o cliente dela **não** é tocado. Lista vazia para a sessão.
#[cfg(target_os = "windows")]
#[tauri::command]
async fn set_afk_accounts(
    app: tauri::AppHandle,
    user_ids: Vec<i64>,
) -> Result<AfkStatusPayload, String> {
    let user_ids = dedupe_preserving_order(user_ids);
    let Some(session) = AFK_MANAGER.get_session() else {
        return Err("AFK mode is not running".into());
    };

    if user_ids.is_empty() {
        stop_afk_session().await;
        let status = current_afk_status();
        emit_afk_status(&app);
        return Ok(status);
    }

    let now = now_ms();
    if let Ok(mut map) = session.accounts.lock() {
        map.retain(|user_id, _| user_ids.contains(user_id));
        for uid in &user_ids {
            map.entry(*uid)
                .or_insert_with(|| AfkAccountRuntime::joined(*uid, now));
        }
    }

    let status = current_afk_status();
    emit_afk_status(&app);
    Ok(status)
}

/// Um ciclo agora, nas contas que o usuário escolheu na tela. Existe para ele
/// conferir que o envio funciona sem esperar o intervalo — e é uma ação dele,
/// explícita, não do agendador.
///
/// Com sessão em andamento, o relógio das contas visitadas é remarcado: senão o
/// envio manual seria seguido de outro logo depois.
#[cfg(target_os = "windows")]
#[tauri::command]
async fn afk_trigger_now(app: tauri::AppHandle, user_ids: Vec<i64>) -> Result<u32, String> {
    let requested = dedupe_preserving_order(user_ids);

    // Envio manual só alcança conta que **está** no modo: fora dele, nem trazer
    // a janela para frente é permitido.
    let Some(session) = AFK_MANAGER.get_session() else {
        return Err("Start AFK mode before sending the key by hand".into());
    };
    let user_ids = session
        .accounts
        .lock()
        .map(|map| afk_manual_targets(&requested, &map))
        .unwrap_or_default();
    if user_ids.is_empty() {
        return Err("None of those accounts is in AFK mode".into());
    }
    // Tecla ou clique: o da sessão ligada, não um que a tela mande agora.
    let config = session
        .config
        .lock()
        .map(|c| c.clone())
        .map_err(|_| "AFK mode is not running".to_string())?;
    validate_afk_start(config.mode, &config.key, &user_ids)?;

    let action = afk_cycle_action(&app, &config, &user_ids);
    let cycle_targets = user_ids.clone();
    let outcome = {
        let _guard = AFK_CYCLE_LOCK.lock().await;
        // Sem stop_flag: este ciclo é o clique do usuário, não o agendador.
        let idle = AtomicBool::new(false);
        tokio::task::spawn_blocking(move || {
            run_afk_cycle_blocking(&action, &cycle_targets, &idle)
        })
        .await
        .unwrap_or_else(|e| Err(format!("AFK cycle failed: {}", e)))?
    };

    let sent = outcome.iter().filter(|(_, error)| error.is_none()).count() as u32;
    // Como no laço: o relógio marca o fim do ciclo.
    let finished_at = now_ms();
    if let Ok(mut map) = session.accounts.lock() {
        afk_record_cycle(&mut map, &user_ids, &Ok(outcome), finished_at);
    }
    emit_afk_status(&app);

    if sent > 0 {
        emit_afk_cycle(&app, sent);
    }
    Ok(sent)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn afk_trigger_now(_user_ids: Vec<i64>) -> Result<u32, String> {
    Err("AFK mode is only available on Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn start_afk_mode(
    _user_ids: Vec<i64>,
    _interval_seconds: i64,
    _key: String,
    _mode: String,
    _click_x: f64,
    _click_y: f64,
) -> Result<AfkStatusPayload, String> {
    Err("AFK mode is only available on Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn stop_afk_mode() -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn set_afk_accounts(_user_ids: Vec<i64>) -> Result<AfkStatusPayload, String> {
    Err("AFK mode is only available on Windows".into())
}

/// O Marcar: lê a posição do cursor **uma vez** (a contagem regressiva é da
/// tela) e diz de que conta é a janela embaixo dele e onde, em porcentagem. Só
/// vale cliente aberto por este app e que ainda é um Roblox — PID reaproveitado
/// pelo Windows não conta. Não lê botão nem tecla.
#[cfg(target_os = "windows")]
#[tauri::command]
fn afk_capture_point() -> Result<AfkCapturedPoint, String> {
    use platform::windows;
    let cursor = windows::cursor_position().ok_or("noCursor")?;
    let hwnd = windows::root_window_at(cursor.0, cursor.1);
    let alive: HashSet<u32> = windows::get_roblox_pids().into_iter().collect();
    let tracked: Vec<(i64, u32)> = windows::tracker()
        .get_all()
        .into_iter()
        .filter(|process| alive.contains(&process.pid))
        .map(|process| (process.user_id, process.pid))
        .collect();
    afk_capture_from_parts(
        cursor,
        windows::window_pid(hwnd),
        &tracked,
        windows::client_rect_on_screen(hwnd),
    )
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn afk_capture_point() -> Result<AfkCapturedPoint, String> {
    Err("AFK mode is only available on Windows".into())
}

#[tauri::command]
fn get_afk_mode_status() -> Result<AfkStatusPayload, String> {
    Ok(current_afk_status())
}

/// A lista fechada de teclas, para a tela oferecer exatamente o que o backend
/// aceita — em vez de manter uma segunda lista no frontend, que sairia do lugar.
#[tauri::command]
fn get_afk_keys() -> Result<Vec<String>, String> {
    Ok(afk_key_names())
}

#[cfg(test)]
mod afk_command_tests {
    use super::*;

    // ── lista fechada de teclas ────────────────────────────────────────────

    #[test]
    fn every_allowed_key_maps_to_a_virtual_key() {
        for name in afk_key_names() {
            assert!(
                afk_virtual_key(&name).is_some(),
                "a tecla oferecida na tela tem de ter virtual key: {name}"
            );
        }
        assert_eq!(afk_key_names().len(), AFK_KEYS.len());
    }

    #[test]
    fn the_allowed_list_is_exactly_the_one_the_feature_documents() {
        assert_eq!(
            afk_key_names(),
            vec!["Space", "W", "A", "S", "D", "E", "F", "R", "Q", "1", "2", "3", "4", "5"]
        );
    }

    #[test]
    fn a_key_outside_the_list_has_no_virtual_key() {
        // Teclas que fazem outra coisa (Enter abre o chat, Tab troca de janela,
        // F4 com Alt fecha o cliente) ficam fora de propósito.
        for outside in [
            "Enter", "Tab", "Escape", "F4", "Delete", "LWin", "Ctrl", "Alt", "Shift", "Z", "0",
            "6", "9", "", " ", "0x20", "spacebar", "Space ",
        ] {
            assert!(
                afk_virtual_key(outside).is_none(),
                "tecla fora da lista foi aceita: {outside:?}"
            );
        }
    }

    #[test]
    fn the_key_name_is_matched_without_case() {
        assert_eq!(afk_virtual_key("space"), afk_virtual_key("Space"));
        assert_eq!(afk_virtual_key("w"), afk_virtual_key("W"));
        assert!(afk_virtual_key("SPACE").is_some());
    }

    #[test]
    fn the_allowed_keys_carry_the_windows_virtual_key_codes() {
        assert_eq!(afk_virtual_key("Space"), Some(0x20));
        assert_eq!(afk_virtual_key("W"), Some(0x57));
        assert_eq!(afk_virtual_key("1"), Some(0x31));
        assert_eq!(afk_virtual_key("5"), Some(0x35));
    }

    // ── recusa de start ────────────────────────────────────────────────────

    #[test]
    fn afk_mode_does_not_start_without_a_key() {
        // Não existe tecla padrão: uma tecla escolhida pelo app mexeria no
        // personagem sem o usuário ter pedido.
        let err = validate_afk_start(AfkMode::Key, "", &[11]).expect_err("sem tecla não liga");
        assert!(err.to_lowercase().contains("key"), "{err}");
    }

    #[test]
    fn afk_mode_does_not_start_with_a_key_outside_the_list() {
        assert!(validate_afk_start(AfkMode::Key, "Enter", &[11]).is_err());
        assert!(validate_afk_start(AfkMode::Key, "F4", &[11]).is_err());
    }

    #[test]
    fn afk_mode_does_not_start_without_an_account() {
        let err = validate_afk_start(AfkMode::Key, "Space", &[]).expect_err("sem conta não liga");
        assert!(err.to_lowercase().contains("account"), "{err}");
    }

    #[test]
    fn afk_mode_starts_with_a_listed_key_and_one_account() {
        assert!(validate_afk_start(AfkMode::Key, "Space", &[11]).is_ok());
        assert!(validate_afk_start(AfkMode::Key, "e", &[11, 22]).is_ok());
    }

    // ── intervalo ──────────────────────────────────────────────────────────

    #[test]
    fn clamp_afk_interval_seconds_keeps_5_seconds_to_2_hours() {
        assert_eq!(clamp_afk_interval_seconds(600), 600);
        assert_eq!(clamp_afk_interval_seconds(10), 10);
        assert_eq!(clamp_afk_interval_seconds(5), 5);
        assert_eq!(clamp_afk_interval_seconds(7_200), 7_200);
        // Abaixo de 5 s o ciclo rouba o foco quase sem parar.
        assert_eq!(clamp_afk_interval_seconds(4), 5);
        assert_eq!(clamp_afk_interval_seconds(0), 5);
        assert_eq!(clamp_afk_interval_seconds(-30), 5);
        assert_eq!(clamp_afk_interval_seconds(i64::MIN), 5);
        // O teto é o de antes: 120 minutos.
        assert_eq!(clamp_afk_interval_seconds(7_201), 7_200);
        assert_eq!(clamp_afk_interval_seconds(i64::MAX), 7_200);
    }

    #[test]
    fn the_interval_in_seconds_becomes_milliseconds_for_the_scheduler() {
        assert_eq!(afk_interval_ms(10), 10_000);
        assert_eq!(afk_interval_ms(600), 600_000);
        assert_eq!(afk_interval_ms(0), 0);
    }

    // ── a espera conta do fim do ciclo ──────────────────────────────────────

    /// O dono pediu "10 segundos depois que um ciclo acabar": o relógio de cada
    /// conta visitada é marcado com o fim do ciclo, não com o começo. Marcado
    /// no começo, um ciclo de 4 s com intervalo de 10 s deixava só 6 s de folga.
    #[test]
    fn the_wait_counts_from_the_end_of_the_cycle() {
        let interval = afk_interval_ms(10);
        let mut accounts = session_with(&[(11, 0), (22, 0)]);
        let cycle_end = 10_000 + 4_400;

        afk_record_cycle(
            &mut accounts,
            &[11, 22],
            &Ok(vec![(11, None), (22, Some(AfkSendError::FocusDenied))]),
            cycle_end,
        );

        for user_id in [11, 22] {
            let entry = &accounts[&user_id];
            assert_eq!(entry.last_send_at_ms, cycle_end, "conta {user_id}");
            assert_eq!(afk_next_send_at_ms(entry.last_send_at_ms, interval), cycle_end + 10_000);
            assert!(!afk_is_due(entry.last_send_at_ms, interval, cycle_end + 9_999));
            assert!(afk_is_due(entry.last_send_at_ms, interval, cycle_end + 10_000));
        }
        assert_eq!(accounts[&11].sends, 1);
        assert!(accounts[&11].last_error.is_none());
        assert_eq!(accounts[&22].sends, 0, "foco negado não conta como envio");
        assert_eq!(accounts[&22].last_error, Some(AfkSendError::FocusDenied));
    }

    #[test]
    fn a_cycle_stopped_halfway_leaves_the_accounts_it_did_not_visit_due() {
        let mut accounts = session_with(&[(11, 0), (22, 0)]);
        // A parada interrompeu o ciclo depois da 11: a 22 não foi visitada.
        afk_record_cycle(&mut accounts, &[11, 22], &Ok(vec![(11, None)]), 5_000);

        assert_eq!(accounts[&11].last_send_at_ms, 5_000);
        assert_eq!(accounts[&22].last_send_at_ms, 0);
        assert_eq!(accounts[&22].sends, 0);
    }

    #[test]
    fn a_cycle_that_failed_marks_every_target_with_the_end_and_the_error() {
        let mut accounts = session_with(&[(11, 0), (22, 0), (33, 0)]);
        afk_record_cycle(&mut accounts, &[11, 22], &Err("boom".into()), 7_000);

        for user_id in [11, 22] {
            assert_eq!(accounts[&user_id].last_send_at_ms, 7_000);
            assert_eq!(
                accounts[&user_id].last_error,
                Some(AfkSendError::Internal("boom".into()))
            );
            assert_eq!(accounts[&user_id].sends, 0);
        }
        // Quem não era alvo não é tocado.
        assert_eq!(accounts[&33].last_send_at_ms, 0);
        assert!(accounts[&33].last_error.is_none());
    }

    /// O laço lê o relógio **depois** do ciclo bloqueante e é com ele que marca
    /// as contas. Ler antes (como era) fazia a espera contar do começo.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_session_loop_marks_the_clock_after_the_cycle_returns() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/afk.rs"),
        )
        .expect("commands/afk.rs tem de existir");
        let body = super::afk_input_safety_tests::production_only(&source);
        let start = body
            .find("async fn run_afk_session")
            .expect("run_afk_session existe");
        let session_loop = &body[start..];
        let cycle = session_loop
            .find("run_afk_cycle_blocking(&action")
            .expect("o laço roda o ciclo");
        let finished = session_loop
            .find("let finished_at = now_ms();")
            .expect("o laço lê o fim do ciclo");
        let record = session_loop
            .find("afk_record_cycle(")
            .expect("o laço marca as contas pelo afk_record_cycle");
        assert!(cycle < finished, "o fim do ciclo é lido depois de o ciclo voltar");
        assert!(finished < record, "e é com ele que as contas são marcadas");
    }

    // ── agendador ──────────────────────────────────────────────────────────

    const MINUTE_MS: i64 = 60_000;

    #[test]
    fn an_account_is_due_only_after_a_whole_interval() {
        let last = 1_000_000;
        let interval = 10 * MINUTE_MS;
        assert!(!afk_is_due(last, interval, last));
        assert!(!afk_is_due(last, interval, last + interval - 1));
        assert!(afk_is_due(last, interval, last + interval));
        assert!(afk_is_due(last, interval, last + interval * 3));
    }

    #[test]
    fn an_account_that_just_entered_afk_mode_waits_its_first_interval() {
        // O relógio da conta começa quando ela entra no modo: ligar o AFK mode
        // não pode mexer no personagem no mesmo segundo.
        let joined = 5_000_000;
        let runtime = AfkAccountRuntime::joined(11, joined);
        assert_eq!(runtime.last_send_at_ms, joined);
        assert!(!afk_is_due(
            runtime.last_send_at_ms,
            10 * MINUTE_MS,
            joined + 1
        ));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_trigger_a_send() {
        // Relógio do sistema andando para trás dá `now` menor que o último
        // envio: isso não é hora de enviar, é hora de esperar.
        let last = 9_000_000;
        assert!(!afk_is_due(last, 10 * MINUTE_MS, last - 60_000));
    }

    #[test]
    fn the_next_send_is_one_interval_after_the_last_one() {
        assert_eq!(afk_next_send_at_ms(1_000, 60_000), 61_000);
        assert_eq!(afk_next_send_at_ms(i64::MAX, 60_000), i64::MAX);
    }

    fn session_with(entries: &[(i64, i64)]) -> HashMap<i64, AfkAccountRuntime> {
        let mut map = HashMap::new();
        for (user_id, last) in entries {
            map.insert(*user_id, AfkAccountRuntime::joined(*user_id, *last));
        }
        map
    }

    #[test]
    fn only_the_accounts_whose_interval_expired_are_targets() {
        let now = 10 * MINUTE_MS;
        let interval = 5 * MINUTE_MS;
        let accounts = session_with(&[
            (11, now - interval),     // venceu agora
            (22, now - interval - 1), // venceu há 1 ms
            (33, now - interval + 1), // falta 1 ms
        ]);

        assert_eq!(afk_due_targets(&accounts, interval, now), vec![11, 22]);
    }

    #[test]
    fn an_account_outside_afk_mode_is_never_a_target() {
        let now = 10 * MINUTE_MS;
        let interval = MINUTE_MS;
        // 99 nunca entrou no modo: nem vencida ela aparece.
        let accounts = session_with(&[(11, 0)]);
        let targets = afk_due_targets(&accounts, interval, now);
        assert_eq!(targets, vec![11]);
        assert!(!targets.contains(&99));

        assert!(
            afk_due_targets(&HashMap::new(), interval, now).is_empty(),
            "sessão sem conta não tem alvo"
        );
    }

    #[test]
    fn the_target_order_is_stable_by_account() {
        let now = 10 * MINUTE_MS;
        let interval = MINUTE_MS;
        let accounts = session_with(&[(33, 0), (11, 0), (22, 0)]);
        assert_eq!(afk_due_targets(&accounts, interval, now), vec![11, 22, 33]);
    }

    // ── parar interrompe o ciclo em andamento ──────────────────────────────

    #[test]
    fn a_stopping_session_aborts_the_cycle_instead_of_sending() {
        assert_eq!(afk_cycle_step(true, true), AfkCycleStep::Abort);
        assert_eq!(afk_cycle_step(true, false), AfkCycleStep::Abort);
    }

    #[test]
    fn a_target_whose_window_is_gone_is_skipped_not_sent() {
        assert_eq!(afk_cycle_step(false, false), AfkCycleStep::Skip);
    }

    #[test]
    fn a_live_window_of_an_afk_account_gets_the_key() {
        assert_eq!(afk_cycle_step(false, true), AfkCycleStep::Send);
    }

    #[test]
    fn a_stopping_session_does_not_restore_the_focus() {
        assert!(!afk_should_restore_focus(true, true));
        assert!(!afk_should_restore_focus(true, false));
    }

    #[test]
    fn the_focus_only_goes_back_when_the_cycle_took_it() {
        assert!(afk_should_restore_focus(false, true));
        assert!(
            !afk_should_restore_focus(false, false),
            "ciclo que não roubou foco não mexe na janela de ninguém"
        );
    }

    // ── status ─────────────────────────────────────────────────────────────

    #[test]
    fn get_afk_mode_status_reports_no_session_by_default() {
        let status = get_afk_mode_status().unwrap();
        assert!(!status.active);
        assert!(status.accounts.is_empty());
        assert!(status.key.is_empty());
    }

    // ── o alvo precisa estar em primeiro plano antes de a tecla sair ────────

    /// O `SendInput` entrega na janela em primeiro plano, e o Windows **recusa**
    /// `SetForegroundWindow` de processo que está em segundo plano — que é o
    /// caso normal do AFK mode. Sem conferir, a tecla ia para a janela em que o
    /// usuário está digitando, e o ciclo dizia que deu tudo certo.
    #[test]
    fn a_window_that_did_not_reach_the_foreground_is_not_ready() {
        // Pedido recusado pelo Windows: nada de tecla.
        assert!(!afk_window_is_ready(false, 10, 10));
        // Pedido aceito, mas quem está na frente é outra janela (a do usuário).
        assert!(!afk_window_is_ready(true, 99, 10));
        // Sem janela em primeiro plano nenhuma.
        assert!(!afk_window_is_ready(true, 0, 10));
    }

    #[test]
    fn a_window_in_the_foreground_is_ready_for_the_key() {
        assert!(afk_window_is_ready(true, 10, 10));
    }

    #[test]
    fn a_null_target_is_never_ready() {
        // Alvo nulo casaria com "nenhuma janela em primeiro plano" e a tecla
        // sairia para o vazio — ou para quem estivesse lá.
        assert!(!afk_window_is_ready(true, 0, 0));
    }

    // ── janela minimizada volta a ser minimizada ────────────────────────────

    #[test]
    fn a_window_the_user_had_minimized_goes_back_to_minimized() {
        assert!(afk_should_reminimize(true, true));
    }

    #[test]
    fn a_window_that_was_not_minimized_is_left_alone() {
        assert!(!afk_should_reminimize(false, true));
        // Ciclo que não chegou a mexer na janela não minimiza nada.
        assert!(!afk_should_reminimize(true, false));
        assert!(!afk_should_reminimize(false, false));
    }

    // ── quanto tempo o foco fica fora ───────────────────────────────────────

    /// O `AfkDialog` diz "cerca de meio segundo cada" e "uns 4 segundos com 10
    /// contas", porque o foco só volta no fim do ciclo. Mexeu nas constantes,
    /// mexe no texto da tela junto — foi o texto ficar para trás do backend que
    /// o checkup achou.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_cycle_keeps_the_focus_about_half_a_second_per_account() {
        let per_account_ms = AFK_FOCUS_SETTLE_MS + AFK_KEY_HOLD_MS + AFK_BETWEEN_WINDOWS_MS;
        assert!(
            (400..=500).contains(&per_account_ms),
            "{per_account_ms} ms por conta: atualize o texto do foco no AfkDialog"
        );
        let ten_accounts_ms = per_account_ms * 10;
        assert!(
            (4_000..5_000).contains(&ten_accounts_ms),
            "{ten_accounts_ms} ms com 10 contas: atualize o texto do foco no AfkDialog"
        );
    }

    // ── devolver o foco não mexe na janela do usuário ───────────────────────

    /// No fim do ciclo o foco volta para a janela que o usuário estava usando.
    /// Pelo `focus_window` isso passava por `SW_RESTORE` e tirava do maximizado
    /// a janela dele a cada ciclo — até quando o foco tinha sido negado e nada
    /// foi enviado. O caminho de volta é o `give_focus_back`, que não mexe no
    /// estado da janela (a decisão está coberta em `win_focus_tests`).
    #[test]
    fn the_cycle_gives_the_focus_back_without_touching_the_window_state() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/afk.rs"),
        )
        .expect("commands/afk.rs tem de existir");
        let body = super::afk_input_safety_tests::production_only(&source);
        assert!(
            body.contains("windows::give_focus_back_after_cycle(previous_foreground"),
            "o ciclo tem de devolver o foco pelo give_focus_back_after_cycle"
        );
        assert!(
            !body.contains("focus_window(previous_foreground)"),
            "focus_window restaura a janela: não serve para devolver o foco"
        );
    }

    // ── envio manual ───────────────────────────────────────────────────────

    #[test]
    fn a_manual_send_only_reaches_accounts_that_are_in_afk_mode() {
        let accounts = session_with(&[(11, 0), (22, 0)]);
        // 99 não está no modo: o envio manual não pode tocar na janela dela.
        assert_eq!(afk_manual_targets(&[99], &accounts), Vec::<i64>::new());
        assert_eq!(afk_manual_targets(&[22, 99, 11], &accounts), vec![22, 11]);
        assert_eq!(afk_manual_targets(&[], &accounts), Vec::<i64>::new());
    }

    #[test]
    fn a_manual_send_keeps_the_order_the_user_asked_for() {
        let accounts = session_with(&[(11, 0), (22, 0), (33, 0)]);
        assert_eq!(afk_manual_targets(&[33, 11], &accounts), vec![33, 11]);
    }

    // ── erro por conta, com código que a tela entende ───────────────────────

    #[test]
    fn every_send_error_carries_a_code_and_a_message() {
        for error in [
            AfkSendError::NoWindow,
            AfkSendError::FocusDenied,
            AfkSendError::KeyRefused,
            AfkSendError::Internal("boom".into()),
        ] {
            assert!(!error.code().is_empty());
            assert!(!error.message().is_empty());
        }
        assert_eq!(AfkSendError::FocusDenied.code(), "focusDenied");
        assert_eq!(AfkSendError::NoWindow.code(), "noWindow");
        assert_eq!(AfkSendError::KeyRefused.code(), "keyRefused");
        assert_eq!(AfkSendError::Internal("boom".into()).message(), "boom");
    }

    #[test]
    fn the_status_tells_the_screen_which_error_it_was() {
        let mut runtime = AfkAccountRuntime::joined(11, 0);
        runtime.last_error = Some(AfkSendError::FocusDenied);
        let mut accounts = HashMap::new();
        accounts.insert(11, runtime);

        let status = afk_status_from_parts(Some(1), 600, "Space", AfkMode::Key, AFK_DEFAULT_POINT, &accounts);
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["accounts"][0]["lastErrorCode"], "focusDenied");
        assert!(json["accounts"][0]["lastError"].is_string());
    }

    // ── tela cheia na frente (ideia 25) ────────────────────────────────────

    fn pids(list: &[u32]) -> HashSet<u32> {
        list.iter().copied().collect()
    }

    #[test]
    fn a_fullscreen_window_of_another_program_holds_the_cycle() {
        // Um vídeo ou outro jogo em tela cheia: PID que não é cliente do app,
        // nem a área de trabalho, nem o próprio MultiAlt.
        assert!(afk_foreground_blocks(Some(30), true, &pids(&[10]), &pids(&[20]), 99));
    }

    #[test]
    fn a_window_that_does_not_cover_its_monitor_never_holds() {
        assert!(!afk_foreground_blocks(Some(30), false, &pids(&[10]), &pids(&[20]), 99));
    }

    #[test]
    fn the_apps_own_clients_the_desktop_and_multialt_never_hold() {
        let clients = pids(&[10]);
        let shell = pids(&[20]);
        assert!(!afk_foreground_blocks(Some(10), true, &clients, &shell, 99), "a client the app opened");
        assert!(!afk_foreground_blocks(Some(20), true, &clients, &shell, 99), "the desktop covers the monitor too");
        assert!(!afk_foreground_blocks(Some(99), true, &clients, &shell, 99), "MultiAlt itself");
        assert!(!afk_foreground_blocks(None, true, &clients, &shell, 99), "no window in front");
    }

    #[test]
    fn the_cycle_waits_for_the_fullscreen_window_up_to_the_cap() {
        let due = 1_000;
        let cap = AFK_FULLSCREEN_MAX_WAIT_MS;
        assert_eq!(afk_fullscreen_gate(true, true, due, due + 60_000, cap), AfkGate::Wait);
        assert_eq!(afk_fullscreen_gate(true, true, due, due + cap - 1, cap), AfkGate::Wait);
        // Passou do teto: a conta não pode cair por inatividade esperando.
        assert_eq!(afk_fullscreen_gate(true, true, due, due + cap, cap), AfkGate::Send);
        // Sem tela cheia, ou com a opção desligada, segue como sempre.
        assert_eq!(afk_fullscreen_gate(true, false, due, due + 60_000, cap), AfkGate::Send);
        assert_eq!(afk_fullscreen_gate(false, true, due, due + 60_000, cap), AfkGate::Send);
    }

    #[test]
    fn the_wait_counts_from_the_account_that_has_been_due_the_longest() {
        let accounts = session_with(&[(11, 5_000), (22, 1_000), (33, 9_000)]);
        assert_eq!(afk_due_since(&accounts, &[11, 22], 10_000, 99_999), 11_000);
        // Sem alvo conhecido, conta de agora.
        assert_eq!(afk_due_since(&accounts, &[44], 10_000, 99_999), 99_999);
    }

    #[test]
    fn waiting_for_a_fullscreen_window_is_on_unless_turned_off() {
        assert!(afk_wait_for_fullscreen_enabled(""));
        assert!(afk_wait_for_fullscreen_enabled("true"));
        assert!(!afk_wait_for_fullscreen_enabled("false"));
    }

    #[test]
    fn the_status_tells_the_screen_it_is_waiting_for_a_fullscreen_window() {
        let mut status = AfkStatusPayload::default();
        assert_eq!(serde_json::to_value(&status).unwrap()["waitingFullscreen"], false);
        status.waiting_fullscreen = true;
        assert_eq!(serde_json::to_value(&status).unwrap()["waitingFullscreen"], true);
    }

    /// Defeito que o upstream teve e que aqui não pode nascer: o prazo do
    /// primeiro envio só aparecia **depois** do primeiro ciclo, e quem ligava o
    /// modo passava o intervalo inteiro olhando um "--", sem saber se pegou.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_first_deadline_is_known_the_moment_the_session_starts() {
        let started_at = 1_000;
        let session = new_afk_session(
            7,
            started_at,
            AfkConfig {
                interval_seconds: 90,
                key: "Space".into(),
                mode: AfkMode::Key,
                default_point: AFK_DEFAULT_POINT,
            },
            &[11, 22],
        );

        let status = afk_status_from(&session);
        assert!(status.active);
        assert_eq!(status.started_at_ms, Some(started_at));
        assert_eq!(status.interval_seconds, 90);
        assert_eq!(status.accounts.len(), 2);
        for account in &status.accounts {
            assert_eq!(
                account.next_send_at_ms,
                started_at + 90_000,
                "a tela tem de saber o primeiro prazo antes de qualquer envio"
            );
            assert_eq!(account.last_send_at_ms, started_at);
            assert_eq!(account.sends, 0);
            assert!(account.last_error.is_none());
        }
    }

    #[test]
    fn get_afk_keys_offers_the_closed_list_and_nothing_else() {
        assert_eq!(get_afk_keys().unwrap(), afk_key_names());
    }

    #[test]
    fn the_status_payload_reaches_the_frontend_in_camel_case() {
        let status = AfkStatusPayload {
            active: true,
            started_at_ms: Some(7),
            interval_seconds: 610,
            key: "Space".into(),
            mode: "click".into(),
            click_x: 25.0,
            click_y: 75.0,
            accounts: vec![AfkAccountStatus {
                user_id: 11,
                last_send_at_ms: 1,
                next_send_at_ms: 2,
                sends: 3,
                last_error: None,
                last_error_code: None,
            }],
            waiting_fullscreen: false,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["active"], true);
        assert_eq!(json["startedAtMs"], 7);
        assert_eq!(json["intervalSeconds"], 610);
        assert!(json.get("intervalMinutes").is_none(), "o campo antigo saiu");
        assert_eq!(json["key"], "Space");
        assert_eq!(json["accounts"][0]["userId"], 11);
        assert_eq!(json["accounts"][0]["nextSendAtMs"], 2);
        assert_eq!(json["accounts"][0]["sends"], 3);
        assert_eq!(json["mode"], "click");
        assert_eq!(json["clickX"], 25.0);
        assert_eq!(json["clickY"], 75.0);
    }

    // ── modo clique: ponto relativo da janela ──────────────────────────────

    const RECT: AfkClientRect = AfkClientRect {
        left: 100,
        top: 200,
        width: 801,
        height: 601,
    };

    #[test]
    fn a_percentage_becomes_the_same_relative_pixel_in_any_window_size() {
        assert_eq!(
            afk_point_to_pixel(RECT, AfkPoint::clamped(50.0, 50.0)),
            Some((500, 500))
        );
        let small = AfkClientRect {
            left: 0,
            top: 0,
            width: 401,
            height: 301,
        };
        assert_eq!(
            afk_point_to_pixel(small, AfkPoint::clamped(50.0, 50.0)),
            Some((200, 150))
        );
    }

    #[test]
    fn the_click_never_leaves_the_game_area() {
        // 0% e 100% são a primeira e a última coluna/linha **de dentro**.
        assert_eq!(
            afk_point_to_pixel(RECT, AfkPoint::clamped(0.0, 0.0)),
            Some((100, 200))
        );
        assert_eq!(
            afk_point_to_pixel(RECT, AfkPoint::clamped(100.0, 100.0)),
            Some((900, 800))
        );
        // Fora de 0–100 é travado, não extrapolado.
        assert_eq!(
            afk_point_to_pixel(
                RECT,
                AfkPoint {
                    x_pct: 150.0,
                    y_pct: -5.0
                }
            ),
            Some((900, 200))
        );
        assert_eq!(clamp_afk_percent(f64::NAN), 50.0);
        assert_eq!(clamp_afk_percent(f64::INFINITY), 50.0);
        let empty = AfkClientRect {
            left: 0,
            top: 0,
            width: 0,
            height: 10,
        };
        assert_eq!(afk_point_to_pixel(empty, AFK_DEFAULT_POINT), None);
    }

    #[test]
    fn marking_turns_the_cursor_into_a_percentage_of_the_window() {
        assert_eq!(
            afk_pixel_to_point(RECT, 500, 500),
            Some(AfkPoint {
                x_pct: 50.0,
                y_pct: 50.0
            })
        );
        assert_eq!(
            afk_pixel_to_point(RECT, 900, 800),
            Some(AfkPoint {
                x_pct: 100.0,
                y_pct: 100.0
            })
        );
        // Na borda de fora (ou fora da janela) não há ponto.
        assert_eq!(afk_pixel_to_point(RECT, 99, 500), None);
        assert_eq!(afk_pixel_to_point(RECT, 901, 500), None);
        assert_eq!(afk_pixel_to_point(RECT, 500, 801), None);
        // Ida e volta cai no mesmo lugar.
        let point = afk_pixel_to_point(RECT, 519, 631).unwrap();
        assert_eq!(afk_point_to_pixel(RECT, point), Some((519, 631)));
    }

    fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn an_account_point_needs_both_numbers() {
        assert_eq!(
            afk_point_from_fields(&fields(&[("AfkClickX", "52.5"), ("AfkClickY", "71")])),
            Some(AfkPoint {
                x_pct: 52.5,
                y_pct: 71.0
            })
        );
        assert_eq!(afk_point_from_fields(&fields(&[("AfkClickX", "52.5")])), None);
        assert_eq!(
            afk_point_from_fields(&fields(&[("AfkClickX", "abc"), ("AfkClickY", "1")])),
            None
        );
        assert_eq!(
            afk_point_from_fields(&fields(&[("AfkClickX", "NaN"), ("AfkClickY", "1")])),
            None
        );
        assert_eq!(
            afk_point_from_fields(&fields(&[("AfkClickX", "150"), ("AfkClickY", "10")])),
            Some(AfkPoint {
                x_pct: 100.0,
                y_pct: 10.0
            })
        );
    }

    #[test]
    fn an_account_without_its_own_point_uses_the_default() {
        let own = AfkPoint::clamped(10.0, 20.0);
        let default = AfkPoint::clamped(50.0, 60.0);
        let overrides: HashMap<i64, AfkPoint> = [(1, own)].into_iter().collect();
        let points = afk_points_for_targets(&[1, 2], default, &overrides);
        assert_eq!(points[&1], own);
        assert_eq!(points[&2], default);
    }

    #[test]
    fn the_click_mode_starts_without_a_key() {
        assert!(validate_afk_start(AfkMode::Click, "", &[1]).is_ok());
        assert!(validate_afk_start(AfkMode::Key, "", &[1]).is_err());
        assert!(validate_afk_start(AfkMode::Click, "", &[]).is_err());
    }

    #[test]
    fn an_unknown_mode_falls_back_to_the_key() {
        assert_eq!(AfkMode::parse("click"), AfkMode::Click);
        assert_eq!(AfkMode::parse(" CLICK "), AfkMode::Click);
        assert_eq!(AfkMode::parse("key"), AfkMode::Key);
        assert_eq!(AfkMode::parse("banana"), AfkMode::Key);
        assert_eq!(AfkMode::Click.as_str(), "click");
        assert_eq!(AfkMode::Key.as_str(), "key");
    }

    #[test]
    fn a_refused_click_has_its_own_code() {
        assert_eq!(AfkSendError::ClickRefused.code(), "clickRefused");
        assert!(!AfkSendError::ClickRefused.message().is_empty());
    }

    #[test]
    fn marking_on_an_account_window_returns_that_account_and_the_percentage() {
        let got =
            afk_capture_from_parts((500, 500), Some(42), &[(7, 41), (9, 42)], Some(RECT)).unwrap();
        assert_eq!((got.user_id, got.x_pct, got.y_pct), (9, 50.0, 50.0));
    }

    #[test]
    fn marking_refuses_what_is_not_an_account_window() {
        assert_eq!(
            afk_capture_from_parts((5, 5), None, &[], None).unwrap_err(),
            "noWindow"
        );
        assert_eq!(
            afk_capture_from_parts((500, 500), Some(99), &[(7, 41)], Some(RECT)).unwrap_err(),
            "notAnAccountWindow"
        );
        assert_eq!(
            afk_capture_from_parts((10, 10), Some(41), &[(7, 41)], Some(RECT)).unwrap_err(),
            "outsideGameArea"
        );
        assert_eq!(
            afk_capture_from_parts((500, 500), Some(41), &[(7, 41)], None).unwrap_err(),
            "outsideGameArea"
        );
    }

    #[test]
    fn the_captured_point_reaches_the_frontend_in_camel_case() {
        let got =
            afk_capture_from_parts((500, 500), Some(41), &[(7, 41)], Some(RECT)).unwrap();
        let json = serde_json::to_value(&got).unwrap();
        assert_eq!(json["userId"], 7);
        assert_eq!(json["xPct"], 50.0);
        assert_eq!(json["yPct"], 50.0);
    }

    // ---- a receita do clique ----------------------------------------------
    //
    // O primeiro modo clique posicionava o cursor e clicava parado, e no Roblox
    // de verdade nada acontecia: o jogo lê o mouse por *raw input*, então para
    // ele o mouse nem tinha chegado lá. A receita abaixo é a que funcionou no
    // bot de Robeats do dono: mover pelo caminho de entrada, tremer, e clicar
    // duas vezes (a primeira pode só focar o jogo).

    fn plan(point: AfkPoint) -> Vec<AfkMouseStep> {
        afk_click_plan(RECT, point, 40).expect("janela com área tem receita")
    }

    #[test]
    fn the_click_moves_the_mouse_through_the_input_path_before_pressing() {
        let steps = plan(AfkPoint::clamped(50.0, 50.0));
        let first_press = steps.iter().position(|s| *s == AfkMouseStep::Press).unwrap();
        assert!(
            steps[..first_press].contains(&AfkMouseStep::MoveTo(500, 500)),
            "o botão não pode descer antes de o jogo ver o mouse no ponto: {steps:?}"
        );
        // O passo imediatamente antes do clique é a posição exata, e não um tremor.
        let before: Vec<_> = steps[..first_press]
            .iter()
            .filter(|s| !matches!(s, AfkMouseStep::Wait(_)))
            .collect();
        assert_eq!(**before.last().unwrap(), AfkMouseStep::MoveTo(500, 500));
    }

    #[test]
    fn the_click_shakes_the_mouse_and_comes_back_to_the_point() {
        let steps = plan(AfkPoint::clamped(50.0, 50.0));
        let nudges: Vec<(i32, i32)> = steps
            .iter()
            .filter_map(|s| match s {
                AfkMouseStep::Nudge(dx, dy) => Some((*dx, *dy)),
                _ => None,
            })
            .collect();
        assert!(!nudges.is_empty(), "sem tremor o Roblox não acorda: {steps:?}");
        let (sx, sy) = nudges.iter().fold((0, 0), |(a, b), (dx, dy)| (a + dx, b + dy));
        assert_eq!((sx, sy), (0, 0), "o tremor tem de voltar ao ponto");
        // Toda posição absoluta cai dentro da área do jogo, inclusive o micro-desvio.
        for step in &steps {
            if let AfkMouseStep::MoveTo(x, y) = step {
                assert!(afk_pixel_to_point(RECT, *x, *y).is_some(), "{step:?} saiu da janela");
            }
        }
    }

    #[test]
    fn the_micro_shake_stays_inside_at_the_edges() {
        for point in [AfkPoint::clamped(100.0, 100.0), AfkPoint::clamped(0.0, 0.0)] {
            let steps = plan(point);
            let mut x = 0;
            let mut y = 0;
            for step in &steps {
                match step {
                    AfkMouseStep::MoveTo(nx, ny) => (x, y) = (*nx, *ny),
                    AfkMouseStep::Nudge(dx, dy) => (x, y) = (x + dx, y + dy),
                    _ => continue,
                }
                assert!(afk_pixel_to_point(RECT, x, y).is_some(), "({x}, {y}) saiu da janela");
            }
        }
    }

    #[test]
    fn the_click_is_a_focus_click_then_the_real_one() {
        let steps = plan(AfkPoint::clamped(30.0, 70.0));
        let presses = steps.iter().filter(|s| **s == AfkMouseStep::Press).count();
        let releases = steps.iter().filter(|s| **s == AfkMouseStep::Release).count();
        assert_eq!((presses, releases), (2, 2), "{steps:?}");
        // Todo botão que desce sobe, e na ordem.
        let mut down = false;
        for step in &steps {
            match step {
                AfkMouseStep::Press => {
                    assert!(!down);
                    down = true;
                }
                AfkMouseStep::Release => {
                    assert!(down);
                    down = false;
                }
                AfkMouseStep::MoveTo(..) | AfkMouseStep::Nudge(..) => {
                    assert!(!down, "mexer com o botão pressionado é arrastar")
                }
                AfkMouseStep::Wait(_) => {}
            }
        }
        assert!(!down);
    }

    #[test]
    fn a_window_without_area_has_no_click_plan() {
        let empty = AfkClientRect { left: 0, top: 0, width: 0, height: 10 };
        assert_eq!(afk_click_plan(empty, AFK_DEFAULT_POINT, 40), None);
    }

    /// `SendInput` absoluto fala em 0..65535 sobre a área de trabalho virtual
    /// (todos os monitores), não em pixels.
    #[test]
    fn a_screen_pixel_becomes_the_absolute_input_scale() {
        let desktop = AfkClientRect { left: -1920, top: 0, width: 3840, height: 1080 };
        assert_eq!(afk_absolute_input(-1920, 0, desktop), (0, 0));
        assert_eq!(afk_absolute_input(1919, 1079, desktop), (65535, 65535));
        assert_eq!(afk_absolute_input(0, 540, desktop), (32776, 32798));
        // Fora da área é travado, não estoura.
        assert_eq!(afk_absolute_input(5000, -10, desktop), (65535, 0));
    }

    /// O texto da tela diz quanto tempo o foco fica fora por conta no modo
    /// clique. Mexeu nas esperas, mexe no texto.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_click_cycle_keeps_the_focus_about_a_second_per_account() {
        let waits: u64 = plan(AFK_DEFAULT_POINT)
            .iter()
            .filter_map(|s| match s {
                AfkMouseStep::Wait(ms) => Some(*ms),
                _ => None,
            })
            .sum();
        let per_account_ms = AFK_FOCUS_SETTLE_MS + waits + AFK_BETWEEN_WINDOWS_MS;
        assert!(
            (1_100..=1_300).contains(&per_account_ms),
            "{per_account_ms} ms por conta no modo clique: atualize o texto do AfkDialog"
        );
    }
}

#[cfg(test)]
mod afk_input_safety_tests {
    // A trava de segurança do AFK mode: ele **só envia** entrada, e só tecla da
    // lista fechada. Este módulo varre o código de verdade em vez de confiar em
    // revisão:
    //
    // 1. caminha por `src-tauri/src` inteiro (arquivo novo entra na varredura
    //    sozinho, e é essa a diferença em relação a listar dois caminhos à mão);
    // 2. considera "arquivo do AFK mode" todo arquivo cujo caminho cita `afk`,
    //    **todo arquivo que envia entrada** (cita `SendInput`) — logo um
    //    `platform/windows/input2.rs` novo cai na rede — e **todo fragmento do
    //    mesmo módulo** que um deles. `include!()` não cria módulo: os
    //    `platform/windows/*.rs` são um módulo só, `windows`, e os
    //    `commands/*.rs` são pedaços da raiz do crate. Fragmento irmão se chama
    //    sem caminho nenhum, então não há fronteira a vigiar entre eles;
    // 3. tira os módulos de teste contando chaves, não cortando no primeiro
    //    `#[cfg(test)]`: código de produção escrito **depois** de um módulo de
    //    teste continua sendo varrido (`the_scan_sees_code_after_the_test_modules`);
    // 4. reprova API de leitura de entrada (gancho global, estado de tecla,
    //    entrada crua, tradução de tecla para caractere, nome de tecla, hook de
    //    evento de UI, `AttachThreadInput` — o truque que alguém acrescentaria
    //    para "consertar" o `SetForegroundWindow` recusado — e
    //    `GetLastInputInfo`, que é a "detecção de interação" que esta
    //    funcionalidade recusa porque responde pela sessão inteira do Windows);
    // 5. reprova injeção fora das portas: `mouse_event`, `keybd_event` e
    //    `KEYEVENTF_UNICODE` mandariam entrada sem citar API proibida nenhuma. A
    //    porta do clique (`INPUT_MOUSE`, `SetCursorPos`, `MOUSEEVENTF_*`) só pode
    //    aparecer no módulo de entrada, atrás de `click_afk_point`;
    // 6. reprova **alcance indireto**: os arquivos do AFK mode não podem citar o
    //    nome de nenhum módulo do backend que leia entrada (hoje o
    //    `webview_recovery`, que importa `GetAsyncKeyState` legitimamente). O
    //    nome é o do **módulo**, não o do fragmento: um `windowing.rs` que
    //    lesse entrada faria do `windows` inteiro um leitor, e é `windows::` que
    //    o `commands/afk.rs` escreve;
    // 7. confere que só o módulo de entrada chama `SendInput`/`send_key`/
    //    `send_mouse`, para o caminho único até o `SendInput` continuar
    //    único amanhã.
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};

    /// Ler entrada: proibido em todo arquivo do AFK mode, sem exceção — inclusive
    /// o módulo de entrada. Ler botão do mouse é `GetAsyncKeyState` também.
    const READS_INPUT: &[&str] = &[
        // gancho global de teclado / de eventos de UI
        "SetWindowsHookEx",
        "SetWinEventHook",
        // estado de tecla
        "GetAsyncKeyState",
        "GetKeyState",
        "GetKeyboardState",
        // entrada crua
        "GetRawInputData",
        "GetRawInputBuffer",
        "RegisterRawInputDevices",
        // tecla -> caractere / nome de tecla
        "ToUnicodeEx",
        "ToUnicode",
        "ToAsciiEx",
        "ToAscii",
        "GetKeyNameText",
        // fila de entrada de outra thread e estado da UI dela
        "AttachThreadInput",
        "GetGUIThreadInfo",
        // "detecção de interação": responde pela sessão inteira, não por janela
        "GetLastInputInfo",
    ];

    /// Injetar sem passar pelas portas (`tap_afk_key`, `click_afk_point`):
    /// proibido em todo arquivo do AFK mode.
    const INJECTS_OUTSIDE_THE_DOORS: &[&str] = &["keybd_event", "mouse_event", "KEYEVENTF_UNICODE"];

    /// A porta do clique: só o módulo de entrada pode citar. `GetCursorPos` fica
    /// de fora de propósito — é posição do ponteiro, não botão nem tecla.
    const CLICK_DOOR_ONLY: &[&str] = &["INPUT_MOUSE", "SetCursorPos", "MOUSEEVENTF_"];

    const INPUT_MODULE: &str = "platform/windows/input.rs";

    /// `(arquivo, API)` de toda citação da porta do clique fora do módulo de
    /// entrada.
    fn click_door_violations(files: &[(String, String)]) -> Vec<(String, &'static str)> {
        let mut out = Vec::new();
        for (path, body) in files {
            if path == INPUT_MODULE {
                continue;
            }
            for api in CLICK_DOOR_ONLY {
                if body.contains(api) {
                    out.push((path.clone(), *api));
                }
            }
        }
        out
    }

    fn backend_src() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    /// Todo `.rs` de `src-tauri/src`, com o conteúdo.
    fn backend_files() -> Vec<(String, String)> {
        fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(e) => panic!("não consegui ler {}: {e}", dir.display()),
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                    let text = std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| panic!("não consegui ler {}: {e}", path.display()));
                    let relative = path
                        .strip_prefix(backend_src())
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((relative, text));
                }
            }
        }
        let mut out = Vec::new();
        walk(&backend_src(), &mut out);
        out.sort();
        out
    }

    /// O arquivo sem os módulos `#[cfg(test)]`, casando chaves — e não cortando
    /// no primeiro atributo, que deixaria de fora tudo que vem depois dele.
    pub(super) fn production_only(source: &str) -> String {
        let mut out = String::new();
        let mut rest = source;
        while let Some(at) = rest.find("#[cfg(test)]") {
            out.push_str(&rest[..at]);
            let tail = &rest[at..];
            let Some(open) = tail.find('{') else {
                // Atributo sem bloco: nada a remover daqui para frente.
                rest = "";
                break;
            };
            let bytes = tail.as_bytes();
            let mut depth = 0i32;
            let mut end = tail.len();
            for (i, byte) in bytes.iter().enumerate().skip(open) {
                if quoted_brace(bytes, i) {
                    // Chave entre aspas (`'{'`, `b'}'`, `"{"`) não abre nem
                    // fecha bloco — e este próprio arquivo tem várias.
                    continue;
                }
                if *byte == b'{' {
                    depth += 1;
                } else if *byte == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
            }
            rest = &tail[end..];
        }
        out.push_str(rest);
        out
    }

    /// A chave nesta posição está entre aspas (literal de caractere ou de texto)?
    fn quoted_brace(bytes: &[u8], at: usize) -> bool {
        if bytes[at] != b'{' && bytes[at] != b'}' {
            return false;
        }
        let before = at.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(at + 1).copied();
        const QUOTE: u8 = b'\'';
        const DQUOTE: u8 = b'"';
        matches!(
            (before, after),
            (Some(QUOTE), Some(QUOTE)) | (Some(DQUOTE), Some(DQUOTE))
        )
    }

    /// `(caminho, corpo de produção)` de todo o backend.
    fn backend_production() -> Vec<(String, String)> {
        backend_files()
            .into_iter()
            .map(|(path, text)| (path, production_only(&text)))
            .collect()
    }

    /// Os arquivos que este puxa por `include!("...")`, resolvidos a partir da
    /// pasta dele — que é como o compilador resolve.
    fn included_paths(path: &str, body: &str) -> Vec<String> {
        const OPEN: &str = "include!(\"";
        let dir = path.rfind('/').map(|at| &path[..at]).unwrap_or("");
        let mut out = Vec::new();
        let mut rest = body;
        while let Some(at) = rest.find(OPEN) {
            let after = &rest[at + OPEN.len()..];
            let Some(end) = after.find('"') else {
                break;
            };
            let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
            for part in after[..end].split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        parts.pop();
                    }
                    other => parts.push(other),
                }
            }
            out.push(parts.join("/"));
            rest = &after[end..];
        }
        out
    }

    /// O arquivo que **é** o módulo de cada caminho. Fragmento `include!()` sobe
    /// até quem o inclui (e assim por diante); arquivo que ninguém inclui é o
    /// próprio módulo.
    fn module_roots(files: &[(String, String)]) -> HashMap<String, String> {
        let mut includer: HashMap<String, String> = HashMap::new();
        for (path, body) in files {
            for included in included_paths(path, body) {
                includer.insert(included, path.clone());
            }
        }
        files
            .iter()
            .map(|(path, _)| {
                let mut root = path.clone();
                // `include!` circular não compila; o teto só impede a varredura
                // de girar para sempre se alguém tentar.
                for _ in 0..32 {
                    match includer.get(&root) {
                        Some(up) => root = up.clone(),
                        None => break,
                    }
                }
                (path.clone(), root)
            })
            .collect()
    }

    /// O nome pelo qual o resto do crate chega ao módulo: o do arquivo raiz
    /// (`platform/windows.rs` → `windows`), ou o da pasta, se o raiz é `mod.rs`.
    fn module_name(root: &str) -> String {
        let path = Path::new(root);
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        if stem != "mod" {
            return stem;
        }
        path.parent()
            .and_then(|dir| dir.file_name())
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Módulos (fora do AFK mode) que leem entrada de propósito — hoje a
    /// recuperação da webview, que usa `GetAsyncKeyState` legitimamente. Um
    /// fragmento que lê entrada entra com o nome do módulo que o inclui.
    fn input_reader_modules(files: &[(String, String)], afk_paths: &[String]) -> Vec<String> {
        let roots = module_roots(files);
        let mut out: Vec<String> = files
            .iter()
            .filter(|(path, _)| !afk_paths.contains(path))
            .filter(|(_, body)| READS_INPUT.iter().any(|api| body.contains(api)))
            .map(|(path, _)| module_name(roots.get(path).unwrap_or(path)))
            // A raiz do crate não se alcança por nome: `lib::` não existe.
            .filter(|name| !matches!(name.as_str(), "" | "lib" | "main"))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// As formas de **chegar** a um módulo pelo nome. Menção solta num
    /// comentário não conta; caminho de chamada conta.
    fn module_needles(stem: &str) -> Vec<String> {
        vec![
            format!("::{stem}"),
            format!("{stem}::"),
            format!("use {stem}"),
            format!("mod {stem}"),
        ]
    }

    /// Arquivo do AFK mode: o caminho cita `afk`, ou o arquivo envia entrada.
    fn is_afk_file(path: &str, body: &str) -> bool {
        path.to_ascii_lowercase().contains("afk") || body.contains("SendInput")
    }

    /// `(caminho, corpo de produção)` dos arquivos que **são** do AFK mode — é
    /// deles que sai a chamada para outro módulo.
    fn afk_seed_files() -> Vec<(String, String)> {
        backend_production()
            .into_iter()
            .filter(|(path, body)| is_afk_file(path, body))
            .collect()
    }

    /// `(caminho, corpo de produção)` de todo fragmento de módulo que tem código
    /// do AFK mode: os arquivos do AFK mode e os irmãos `include!()` deles, que
    /// se alcançam sem caminho nenhum.
    fn afk_files() -> Vec<(String, String)> {
        let files = backend_production();
        let roots = module_roots(&files);
        let afk_modules: HashSet<String> = files
            .iter()
            .filter(|(path, body)| is_afk_file(path, body))
            .filter_map(|(path, _)| roots.get(path).cloned())
            .collect();
        files
            .into_iter()
            .filter(|(path, _)| {
                roots
                    .get(path)
                    .map(|root| afk_modules.contains(root))
                    .unwrap_or(false)
            })
            .collect()
    }

    #[test]
    fn no_afk_file_reads_input_or_sends_outside_the_closed_list() {
        let files = afk_files();
        assert!(
            files.len() >= 2,
            "a varredura tem de achar pelo menos commands/afk.rs e o módulo de envio, achei {:?}",
            files.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
        for (path, body) in &files {
            for api in READS_INPUT.iter().chain(INJECTS_OUTSIDE_THE_DOORS) {
                assert!(
                    !body.contains(api),
                    "{path} usa {api}: o AFK mode só pode enviar tecla da lista fechada, nunca ler entrada. \
                     Fragmento include!() do mesmo módulo que um arquivo do AFK mode conta como AFK mode \
                     (se chama sem caminho); leitura de entrada de outra funcionalidade vai para um módulo \
                     próprio, como o webview_recovery"
                );
            }
        }
    }

    #[test]
    fn no_afk_file_reaches_a_module_that_reads_input() {
        let afk = afk_seed_files();
        let paths: Vec<String> = afk.iter().map(|(path, _)| path.clone()).collect();
        let readers = input_reader_modules(&backend_production(), &paths);

        for (path, body) in &afk {
            for reader in &readers {
                for needle in module_needles(reader) {
                    assert!(
                        !body.contains(&needle),
                        "{path} cita {needle} — o módulo {reader} lê entrada, e o AFK mode não pode chegar lá nem indiretamente"
                    );
                }
            }
        }
    }

    /// A derivação acima só protege se ela **reconhece** um módulo leitor. Este
    /// teste alimenta a mesma função com um caso fabricado: assim ela continua
    /// valendo mesmo quando a árvore não tem (ainda) nenhum arquivo leitor.
    #[test]
    fn the_indirect_reach_check_recognizes_a_reader_module() {
        let files = vec![
            (
                "webview_recovery.rs".to_string(),
                "use windows_sys::...::GetAsyncKeyState;".to_string(),
            ),
            ("commands/afk.rs".to_string(), "nada demais".to_string()),
            ("api/auth.rs".to_string(), "nada demais".to_string()),
        ];
        let readers = input_reader_modules(&files, &["commands/afk.rs".to_string()]);
        assert_eq!(readers, vec!["webview_recovery".to_string()]);

        let needles = module_needles("webview_recovery");
        assert!(needles.iter().any(|n| n == "::webview_recovery"));
        assert!(
            needles
                .iter()
                .any(|n| "let _ = crate::webview_recovery::show();".contains(n)),
            "uma chamada indireta real tem de casar com alguma agulha"
        );
    }

    /// `include!()` não cria módulo. `platform/windows/*.rs` são pedaços de **um**
    /// módulo só, `windows` — e é por esse nome que `commands/afk.rs` chama
    /// (`windows::focus_window`). Um fragmento que lê entrada faz do módulo
    /// inteiro um leitor; tratado como módulo próprio (`windowing`), ele passava
    /// com a suíte verde, porque ninguém escreve `windowing::`.
    #[test]
    fn a_fragment_that_reads_input_makes_the_module_that_includes_it_a_reader() {
        let files = vec![
            ("lib.rs".to_string(), "include!(\"commands/afk.rs\");".to_string()),
            (
                "platform/windows.rs".to_string(),
                "include!(\"windows/windowing.rs\");\ninclude!(\"windows/input.rs\");".to_string(),
            ),
            (
                "platform/windows/windowing.rs".to_string(),
                "unsafe { AttachThreadInput(a, b, 1) };".to_string(),
            ),
            ("platform/windows/input.rs".to_string(), "SendInput(1, &i, n)".to_string()),
            (
                "commands/afk.rs".to_string(),
                "use platform::windows;\nwindows::focus_window(h);".to_string(),
            ),
        ];
        let afk = vec![
            "commands/afk.rs".to_string(),
            "platform/windows/input.rs".to_string(),
        ];

        let readers = input_reader_modules(&files, &afk);
        assert_eq!(
            readers,
            vec!["windows".to_string()],
            "o fragmento é o módulo windows, não um módulo windowing"
        );
        let afk_body = &files[4].1;
        assert!(
            module_needles("windows")
                .iter()
                .any(|needle| afk_body.contains(needle.as_str())),
            "a chamada windows::focus_window tem de casar com alguma agulha"
        );
    }

    /// Na árvore de verdade: fragmento irmão de arquivo do AFK mode se alcança
    /// **sem caminho nenhum** — `input.rs` chama o que está em `windowing.rs` só
    /// pelo nome da função, e `commands/afk.rs` chama o que está em qualquer
    /// `commands/*.rs` do mesmo jeito. Por isso o módulo inteiro entra na
    /// varredura, e não só o arquivo que cita `afk` ou `SendInput`.
    #[test]
    fn every_fragment_of_a_module_with_afk_code_is_scanned() {
        let scanned: Vec<String> = afk_files().into_iter().map(|(path, _)| path).collect();
        for fragment in [
            "platform/windows.rs",
            "platform/windows/windowing.rs",
            "platform/windows/input.rs",
            "platform/windows/tracker.rs",
            "lib.rs",
            "commands/afk.rs",
            "commands/watcher.rs",
        ] {
            assert!(
                scanned.iter().any(|path| path == fragment),
                "{fragment} ficou fora da varredura do AFK mode: {scanned:?}"
            );
        }
        // Módulo de verdade, com fronteira própria, continua fora: é para ele
        // que vale a checagem de alcance indireto.
        assert!(!scanned.iter().any(|path| path == "webview_recovery.rs"));
    }

    #[test]
    fn only_the_input_module_sends_input() {
        let senders: Vec<String> = backend_files()
            .into_iter()
            .map(|(path, text)| (path, production_only(&text)))
            .filter(|(_, body)| {
                body.contains("SendInput(")
                    || body.contains("send_key(")
                    || body.contains("send_mouse(")
            })
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            senders,
            vec!["platform/windows/input.rs".to_string()],
            "o caminho até o SendInput tem de continuar sendo um só"
        );
    }

    /// A porta do clique (`INPUT_MOUSE`, `SetCursorPos`, `MOUSEEVENTF_*`) só existe
    /// no módulo de entrada. Caso fabricado, para a checagem valer mesmo que a
    /// árvore mude.
    #[test]
    fn the_click_door_is_only_open_in_the_input_module() {
        let files = vec![
            (
                INPUT_MODULE.to_string(),
                "INPUT_MOUSE SetCursorPos MOUSEEVENTF_LEFTDOWN".to_string(),
            ),
            ("commands/afk.rs".to_string(), "let x = INPUT_MOUSE;".to_string()),
            (
                "platform/windows/windowing.rs".to_string(),
                "SetCursorPos(1, 2)".to_string(),
            ),
        ];
        assert_eq!(
            click_door_violations(&files),
            vec![
                ("commands/afk.rs".to_string(), "INPUT_MOUSE"),
                ("platform/windows/windowing.rs".to_string(), "SetCursorPos"),
            ]
        );
    }

    #[test]
    fn no_afk_file_opens_the_click_door_outside_the_input_module() {
        let violations = click_door_violations(&afk_files());
        assert!(
            violations.is_empty(),
            "injeção de mouse fora de {INPUT_MODULE}: {violations:?} — o clique tem uma porta só, click_afk_point"
        );
    }

    #[test]
    fn reading_the_mouse_buttons_is_still_forbidden() {
        // Ler botão é `GetAsyncKeyState(VK_LBUTTON)`: continua na lista de leitura,
        // que vale para todo arquivo, inclusive o módulo de entrada.
        assert!(READS_INPUT.contains(&"GetAsyncKeyState"));
        assert!(INJECTS_OUTSIDE_THE_DOORS.contains(&"mouse_event"));
        assert!(INJECTS_OUTSIDE_THE_DOORS.contains(&"keybd_event"));
        assert!(!READS_INPUT.contains(&"INPUT_MOUSE"));
        assert!(CLICK_DOOR_ONLY.contains(&"INPUT_MOUSE"));
    }

    #[test]
    fn the_scan_sees_code_after_the_test_modules() {
        // `AFK_SCAN_TAIL_MARKER` mora no fim de commands/afk.rs, **depois** dos
        // módulos de teste: corte ingênuo no primeiro `#[cfg(test)]` deixaria
        // esse trecho de produção fora da varredura sem ninguém notar.
        let afk = backend_files()
            .into_iter()
            .find(|(path, _)| path == "commands/afk.rs")
            .map(|(_, text)| text)
            .expect("commands/afk.rs tem de existir");
        let body = production_only(&afk);

        assert!(body.contains("fn afk_virtual_key"), "produção do começo sumiu");
        assert!(
            body.contains("AFK_SCAN_TAIL_MARKER"),
            "a varredura não enxerga o código depois dos módulos de teste"
        );
        assert!(
            !body.contains("mod afk_command_tests"),
            "os módulos de teste continuam no texto varrido"
        );
        assert!(
            body.len() > 1_000,
            "li quase nada de commands/afk.rs ({} bytes)",
            body.len()
        );
    }

    #[test]
    fn the_scan_really_walks_the_backend() {
        let files = backend_files();
        assert!(
            files.len() > 40,
            "a caminhada achou {} arquivos, o backend tem muito mais",
            files.len()
        );
        assert!(files.iter().any(|(path, _)| path == "commands/afk.rs"));
        assert!(files
            .iter()
            .any(|(path, _)| path == "platform/windows/input.rs"));
    }
}

/// Marcador de fim de arquivo. Mora **depois** dos módulos de teste de propósito:
/// é ele que prova, em `the_scan_sees_code_after_the_test_modules`, que a
/// varredura de segurança enxerga código de produção escrito abaixo dos testes.
#[allow(dead_code)]
const AFK_SCAN_TAIL_MARKER: &str = "afk-scan-tail";
