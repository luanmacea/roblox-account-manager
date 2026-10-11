//! Importar uma gravação do TinyTask (`.rec`). Ver docs/features/recordings.md
//! ("Importar do TinyTask").
//!
//! Só lê bytes de um arquivo que a pessoa escolheu: nada aqui captura teclado
//! ou mouse, nada envia entrada. O resultado é um rascunho de passos que a tela
//! abre no editor para revisar antes de salvar.
//!
//! **O formato.** O TinyTask grava com o gancho de diário do Windows
//! (`WH_JOURNALRECORD`) e o `.rec` é a sequência crua das estruturas `EVENTMSG`
//! que o gancho entrega, sem cabeçalho: `message`, `paramL`, `paramH`, `time`,
//! `hwnd`, cada um com 4 bytes little-endian (o TinyTask é um programa de 32
//! bits) — 20 bytes por evento. Um gravador de 64 bits teria `hwnd` com 8
//! bytes (24 por evento); os dois tamanhos são aceitos, e o que vale é o que
//! deixa todos os eventos com uma mensagem de teclado ou mouse.
//!
//! - teclado (`WM_KEYDOWN`/`WM_KEYUP`, `WM_SYSKEYDOWN`/`WM_SYSKEYUP`):
//!   `paramL` = código virtual no byte baixo e scan code no byte seguinte;
//!   `paramH` = repetição, com o bit 15 para tecla estendida;
//! - mouse (`WM_MOUSEMOVE`, botões, roda): `paramL` = x e `paramH` = y, em
//!   coordenadas de **tela** (monitor à esquerda do principal dá x negativo);
//! - `time` = o relógio do Windows em ms (`GetTickCount`), que dá a volta em
//!   ~49 dias.
//!
//! **A conversão** (`convert_tinytask_events`): tecla apertada/solta vira passo
//! de tecla (só as da lista fechada das gravações; o resto é pulado e listado),
//! botão esquerdo vira clique no ponto relativo à área interna da janela de
//! referência, e o tempo entre eles vira espera — intervalos minúsculos somam
//! até a próxima espera, intervalos longos têm teto.

use super::recordings::{
    RecordingStep, DEFAULT_HOLD_MS, MAX_HOLD_MS, MAX_RECORDING_STEPS, MIN_HOLD_MS, RECORDING_KEYS,
};
use serde::{Deserialize, Serialize};

/// Arquivo maior que isto não é aberto (~400 mil eventos, horas de gravação).
pub const MAX_TINYTASK_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Espera menor que isto não vira passo: soma com a próxima.
pub const TINYTASK_MIN_WAIT_MS: u64 = 30;
/// Botão solto a mais que isto de onde desceu conta como arrasto.
pub const DRAG_PX: i32 = 10;
/// Teto de cada espera importada (a pessoa parada antes de parar de gravar).
pub const TINYTASK_MAX_WAIT_MS: u64 = 60_000;

const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_LSHIFT: u16 = 0xA0;
const VK_RSHIFT: u16 = 0xA1;
const VK_LCONTROL: u16 = 0xA2;
const VK_RCONTROL: u16 = 0xA3;
const VK_LMENU: u16 = 0xA4;
const VK_RMENU: u16 = 0xA5;
const VK_LWIN: u16 = 0x5B;
const VK_RWIN: u16 = 0x5C;

/// Um `EVENTMSG` lido do arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TinyTaskEvent {
    pub message: u32,
    pub param_l: u32,
    pub param_h: u32,
    pub time: u32,
}

/// Por que o arquivo não pôde ser lido. A tela traduz pelo código.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TinyTaskError {
    /// Arquivo vazio.
    Empty,
    /// Maior que `MAX_TINYTASK_FILE_BYTES`.
    TooLarge,
    /// O tamanho não fecha com 20 nem 24 bytes por evento, ou há mensagem que
    /// não é de teclado nem de mouse: não é um `.rec` do TinyTask.
    NotTinyTask,
}

impl TinyTaskError {
    pub fn code(&self) -> &'static str {
        match self {
            TinyTaskError::Empty => "empty",
            TinyTaskError::TooLarge => "tooLarge",
            TinyTaskError::NotTinyTask => "notTinyTask",
        }
    }
}

/// Mensagem que o gancho de diário entrega: teclado (0x100–0x10F) ou mouse
/// (0x200–0x20F).
fn is_input_message(message: u32) -> bool {
    (0x0100..=0x010F).contains(&message) || (0x0200..=0x020F).contains(&message)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn parse_with_stride(bytes: &[u8], stride: usize) -> Option<Vec<TinyTaskEvent>> {
    if bytes.len() % stride != 0 {
        return None;
    }
    let events: Vec<TinyTaskEvent> = bytes
        .chunks_exact(stride)
        .map(|c| TinyTaskEvent {
            message: u32_at(c, 0),
            param_l: u32_at(c, 4),
            param_h: u32_at(c, 8),
            time: u32_at(c, 12),
        })
        .collect();
    events.iter().all(|e| is_input_message(e.message)).then_some(events)
}

/// Lê o `.rec`: 20 bytes por evento (o TinyTask de 32 bits) ou, se esse não
/// servir, 24.
pub fn parse_tinytask(bytes: &[u8]) -> Result<Vec<TinyTaskEvent>, TinyTaskError> {
    if bytes.is_empty() {
        return Err(TinyTaskError::Empty);
    }
    if bytes.len() > MAX_TINYTASK_FILE_BYTES {
        return Err(TinyTaskError::TooLarge);
    }
    parse_with_stride(bytes, 20)
        .or_else(|| parse_with_stride(bytes, 24))
        .ok_or(TinyTaskError::NotTinyTask)
}

/// A área interna da janela de referência, em pixels de tela: onde estava a
/// janela do Roblox quando a gravação foi feita.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TinyTaskArea {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// Uma tecla pulada e quantas vezes ela foi apertada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedKey {
    pub key: String,
    pub count: usize,
}

/// O que a conversão deixou de fora ou mudou — a tela mostra à pessoa.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TinyTaskSummary {
    /// Eventos lidos do arquivo.
    pub events: usize,
    /// Teclas fora da lista das gravações (ou apertadas com Ctrl, Alt ou
    /// Windows), na ordem em que apareceram.
    pub skipped_keys: Vec<SkippedKey>,
    /// Cliques esquerdos fora da área da janela de referência.
    pub clicks_outside: usize,
    /// Botão direito, do meio, laterais e roda: as gravações não têm esses passos.
    pub other_mouse: usize,
    /// Esperas que passaram do teto e foram encurtadas.
    pub capped_waits: usize,
    /// Botão esquerdo solto a mais de `DRAG_PX` de onde desceu: virou clique
    /// onde desceu (a gravação não tem arrasto).
    pub drags: usize,
    /// O arquivo tinha mais passos que uma gravação comporta; o resto ficou fora.
    pub truncated: bool,
    /// A tecla apertada no fim e nunca solta — a de parar a gravação do
    /// TinyTask —, que ficou fora (`None` sem ela).
    pub stop_key: Option<String>,
}

/// Rascunho importado: os passos e o resumo.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TinyTaskImport {
    pub steps: Vec<RecordingStep>,
    pub summary: TinyTaskSummary,
    /// Largura ÷ altura da área de referência (4 casas): vai para a gravação
    /// (`sourceAspect`), para a tela avisar quando a janela da conta tem outro formato.
    pub source_aspect: f64,
}

/// O nome da tecla na lista das gravações. Shift esquerdo, direito ou o
/// genérico (o gancho costuma gravar o genérico) viram "Shift".
fn recording_key_for_vk(vk: u16) -> Option<&'static str> {
    if matches!(vk, VK_SHIFT | VK_LSHIFT | VK_RSHIFT) {
        return Some("Shift");
    }
    RECORDING_KEYS
        .iter()
        .find(|(_, code, _)| *code == vk)
        .map(|(name, _, _)| *name)
}

/// Nome legível de uma tecla fora da lista, para a pessoa saber o que ficou de fora.
fn vk_label(vk: u16) -> String {
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => "Ctrl".into(),
        VK_MENU | VK_LMENU | VK_RMENU => "Alt".into(),
        0x13 => "Pause".into(),
        0x14 => "Caps Lock".into(),
        0x1B => "Escape".into(),
        0x21 => "Page Up".into(),
        0x22 => "Page Down".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x2C => "Print Screen".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        VK_LWIN | VK_RWIN => "Windows".into(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        _ => format!("Key 0x{vk:02X}"),
    }
}

fn is_ctrl(vk: u16) -> bool {
    matches!(vk, VK_CONTROL | VK_LCONTROL | VK_RCONTROL)
}
fn is_alt(vk: u16) -> bool {
    matches!(vk, VK_MENU | VK_LMENU | VK_RMENU)
}
fn is_win(vk: u16) -> bool {
    matches!(vk, VK_LWIN | VK_RWIN)
}

/// Ponto de tela → porcentagem da área interna (0% primeira coluna/linha,
/// 100% a última: o mesmo mapa do Marcar do Modo AFK). `None` fora da área.
fn area_percent(area: TinyTaskArea, x: i32, y: i32) -> Option<(f64, f64)> {
    if area.width <= 0 || area.height <= 0 {
        return None;
    }
    let (dx, dy) = (i64::from(x) - i64::from(area.left), i64::from(y) - i64::from(area.top));
    if dx < 0 || dy < 0 || dx >= i64::from(area.width) || dy >= i64::from(area.height) {
        return None;
    }
    let span_x = f64::from((area.width - 1).max(1));
    let span_y = f64::from((area.height - 1).max(1));
    let round2 = |v: f64| ((v * 100.0).round() / 100.0).clamp(0.0, 100.0);
    Some((round2(dx as f64 / span_x * 100.0), round2(dy as f64 / span_y * 100.0)))
}

struct Converter {
    steps: Vec<RecordingStep>,
    summary: TinyTaskSummary,
    /// Tempo acumulado desde o último passo, ainda não escrito como espera.
    pending_ms: u64,
    /// Sobras minúsculas de antes do último passo, para a próxima espera.
    carry_ms: u64,
    /// Teclas da lista apertadas agora (para ignorar a repetição automática).
    held: Vec<&'static str>,
    /// Ctrl / Alt / Windows apertados agora (vk).
    modifiers: Vec<u16>,
    /// Teclas puladas apertadas agora (o soltar delas também é pulado).
    skipped_held: Vec<u16>,
    /// Onde o botão esquerdo desceu (`None` = solto).
    left_down: Option<(i32, i32)>,
    seen_first_step: bool,
}

impl Converter {
    fn full(&self) -> bool {
        self.steps.len() >= MAX_RECORDING_STEPS
    }

    fn push(&mut self, step: RecordingStep) {
        if self.full() {
            self.summary.truncated = true;
            return;
        }
        self.steps.push(step);
    }

    /// Escreve a espera acumulada antes do próximo passo. Antes do primeiro
    /// passo não há espera (o tempo até a pessoa começar não importa).
    fn flush_wait(&mut self) {
        let pending = std::mem::take(&mut self.pending_ms) + std::mem::take(&mut self.carry_ms);
        if !self.seen_first_step {
            return;
        }
        if pending < TINYTASK_MIN_WAIT_MS {
            // Minúscula: fica guardada e soma na próxima espera (não no tempo
            // segurado da tecla que vem agora).
            self.carry_ms = pending;
            return;
        }
        let ms = if pending > TINYTASK_MAX_WAIT_MS {
            self.summary.capped_waits += 1;
            TINYTASK_MAX_WAIT_MS
        } else {
            pending
        };
        self.push(RecordingStep::Wait { ms });
    }

    fn skip_key(&mut self, label: String) {
        if let Some(entry) = self.summary.skipped_keys.iter_mut().find(|s| s.key == label) {
            entry.count += 1;
        } else {
            self.summary.skipped_keys.push(SkippedKey { key: label, count: 1 });
        }
    }

    fn key_down(&mut self, vk: u16, sys: bool) {
        if is_ctrl(vk) || is_alt(vk) || is_win(vk) {
            if !self.modifiers.contains(&vk) {
                self.modifiers.push(vk);
                self.skip_key(vk_label(vk));
            }
            return;
        }
        if self.skipped_held.contains(&vk) {
            return; // repetição de uma tecla pulada
        }
        let combo = self
            .modifiers
            .iter()
            .map(|m| vk_label(*m))
            .chain(sys.then(|| "Alt".to_string()))
            .fold(Vec::<String>::new(), |mut acc, m| {
                if !acc.contains(&m) {
                    acc.push(m);
                }
                acc
            });
        let name = recording_key_for_vk(vk);
        match name {
            Some(name) if combo.is_empty() => {
                if self.held.contains(&name) {
                    return; // repetição automática de quem já está apertada
                }
                self.held.push(name);
                self.flush_wait();
                self.seen_first_step = true;
                self.push(RecordingStep::KeyDown { key: name.to_string() });
            }
            _ => {
                // Fora da lista, ou junto de Ctrl/Alt/Windows: soltar só o W de
                // um Ctrl+W mudaria o que a gravação faz.
                let label = match name {
                    Some(n) => n.to_string(),
                    None => vk_label(vk),
                };
                let label = if combo.is_empty() { label } else { format!("{}+{}", combo.join("+"), label) };
                self.skipped_held.push(vk);
                self.skip_key(label);
            }
        }
    }

    fn key_up(&mut self, vk: u16) {
        if is_ctrl(vk) || is_alt(vk) || is_win(vk) {
            self.modifiers.retain(|m| *m != vk);
            return;
        }
        if let Some(i) = self.skipped_held.iter().position(|k| *k == vk) {
            self.skipped_held.remove(i);
            return;
        }
        let Some(name) = recording_key_for_vk(vk) else {
            return;
        };
        let Some(i) = self.held.iter().position(|k| *k == name) else {
            return; // soltar sem apertar (a gravação começou com ela apertada)
        };
        self.held.remove(i);
        // Apertar e soltar sem nada no meio vira um toque, com o tempo segurado.
        let hold = self.pending_ms;
        let tap = matches!(self.steps.last(), Some(RecordingStep::KeyDown { key }) if key == name)
            && hold <= MAX_HOLD_MS
            && !self.summary.truncated;
        if tap {
            self.pending_ms = 0;
            let last = self.steps.len() - 1;
            self.steps[last] = RecordingStep::Key {
                key: name.to_string(),
                hold_ms: if hold == 0 { DEFAULT_HOLD_MS } else { hold.max(MIN_HOLD_MS) },
            };
            return;
        }
        self.flush_wait();
        self.push(RecordingStep::KeyUp { key: name.to_string() });
    }

    fn left_down(&mut self, area: TinyTaskArea, x: i32, y: i32) {
        if self.left_down.is_some() {
            return;
        }
        self.left_down = Some((x, y));
        match area_percent(area, x, y) {
            Some((x_pct, y_pct)) => {
                self.flush_wait();
                self.seen_first_step = true;
                self.push(RecordingStep::Click { x_pct, y_pct });
            }
            None => self.summary.clicks_outside += 1,
        }
    }

    /// Soltou longe de onde apertou: foi um arrasto, que a gravação não tem —
    /// fica o clique onde o botão desceu, e o resumo conta.
    fn left_up(&mut self, x: i32, y: i32) {
        if let Some((dx, dy)) = self.left_down.take() {
            if (x - dx).abs() > DRAG_PX || (y - dy).abs() > DRAG_PX {
                self.summary.drags += 1;
            }
        }
    }
}

/// Quantos eventos do fim são a tecla de parar a gravação do próprio
/// TinyTask: o último evento que não é movimento de mouse é tecla apertada
/// que nunca é solta (a gravação para no "apertar"). Pega a sequência inteira
/// de teclas apertadas no fim (um atalho com Ctrl/Shift chega como várias),
/// com os movimentos de mouse no meio.
fn trailing_stop_key(events: &[TinyTaskEvent]) -> usize {
    let mut cut = events.len();
    let mut found = false;
    for (i, e) in events.iter().enumerate().rev() {
        match e.message {
            WM_MOUSEMOVE => {}
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                found = true;
                cut = i;
            }
            _ => break,
        }
    }
    if found {
        cut
    } else {
        events.len()
    }
}

/// Os eventos lidos → passos de gravação, relativos à área `area`.
pub fn convert_tinytask_events(events: &[TinyTaskEvent], area: TinyTaskArea) -> TinyTaskImport {
    let cut = trailing_stop_key(events);
    let stop_keys: Vec<String> = events[cut..]
        .iter()
        .filter(|e| matches!(e.message, WM_KEYDOWN | WM_SYSKEYDOWN))
        .map(|e| {
            let vk = (e.param_l & 0xFF) as u16;
            recording_key_for_vk(vk).map(str::to_string).unwrap_or_else(|| vk_label(vk))
        })
        .fold(Vec::new(), |mut acc, k| {
            if !acc.contains(&k) {
                acc.push(k);
            }
            acc
        });
    let mut out = convert_events(&events[..cut], area);
    out.summary.events = events.len();
    out.summary.stop_key = (!stop_keys.is_empty()).then(|| stop_keys.join("+"));
    out
}

fn convert_events(events: &[TinyTaskEvent], area: TinyTaskArea) -> TinyTaskImport {
    let mut c = Converter {
        steps: Vec::new(),
        summary: TinyTaskSummary {
            events: events.len(),
            ..TinyTaskSummary::default()
        },
        pending_ms: 0,
        carry_ms: 0,
        held: Vec::new(),
        modifiers: Vec::new(),
        skipped_held: Vec::new(),
        left_down: None,
        seen_first_step: false,
    };
    let mut previous_time: Option<u32> = None;
    for event in events {
        if let Some(prev) = previous_time {
            // O relógio dá a volta em ~49 dias: a diferença com volta continua
            // certa. Evento fora de ordem (diferença "negativa") conta zero.
            let delta = event.time.wrapping_sub(prev);
            if delta < 0x8000_0000 {
                c.pending_ms = c.pending_ms.saturating_add(u64::from(delta));
            }
        }
        previous_time = Some(event.time);
        let vk = (event.param_l & 0xFF) as u16;
        let (x, y) = (event.param_l as i32, event.param_h as i32);
        match event.message {
            WM_KEYDOWN => c.key_down(vk, false),
            WM_SYSKEYDOWN => c.key_down(vk, true),
            WM_KEYUP | WM_SYSKEYUP => c.key_up(vk),
            WM_MOUSEMOVE => {}
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => c.left_down(area, x, y),
            WM_LBUTTONUP => c.left_up(x, y),
            _ => {
                // Os "down" do botão direito (0x204), do meio (0x207), laterais
                // (0x20B) e a roda (0x20A, 0x20E) contam uma vez; os "up", não.
                if matches!(event.message, 0x0204 | 0x0207 | 0x020A | 0x020B | 0x020E) {
                    c.summary.other_mouse += 1;
                }
            }
        }
    }
    // Tecla que a gravação deixou apertada: o app solta no fim da reprodução
    // de qualquer jeito; o passo de soltar deixa isso visível no editor.
    let still_held = std::mem::take(&mut c.held);
    for name in still_held {
        c.push(RecordingStep::KeyUp { key: name.to_string() });
    }
    TinyTaskImport {
        steps: c.steps,
        summary: c.summary,
        source_aspect: area_aspect(area),
    }
}

/// Largura ÷ altura, com 4 casas.
fn area_aspect(area: TinyTaskArea) -> f64 {
    if area.width <= 0 || area.height <= 0 {
        return 0.0;
    }
    (f64::from(area.width) / f64::from(area.height) * 10_000.0).round() / 10_000.0
}

/// Lê e converte de uma vez.
pub fn import_tinytask(bytes: &[u8], area: TinyTaskArea) -> Result<TinyTaskImport, TinyTaskError> {
    if area.width <= 0 || area.height <= 0 {
        return Err(TinyTaskError::NotTinyTask);
    }
    Ok(convert_tinytask_events(&parse_tinytask(bytes)?, area))
}

#[cfg(test)]
mod tinytask_import_tests {
    use super::*;

    /// Um `EVENTMSG` de 32 bits (20 bytes), como o TinyTask grava.
    fn ev(message: u32, param_l: u32, param_h: u32, time: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(20);
        for v in [message, param_l, param_h, time, 0x0001_02A4] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// O mesmo, com `hwnd` de 8 bytes (24 bytes por evento).
    fn ev64(message: u32, param_l: u32, param_h: u32, time: u32) -> Vec<u8> {
        let mut out = ev(message, param_l, param_h, time);
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    /// Tecla: vk no byte baixo, scan code no seguinte.
    fn key(message: u32, vk: u32, scan: u32, time: u32) -> Vec<u8> {
        ev(message, vk | (scan << 8), 1, time)
    }

    fn mouse(message: u32, x: i32, y: i32, time: u32) -> Vec<u8> {
        ev(message, x as u32, y as u32, time)
    }

    fn file(parts: Vec<Vec<u8>>) -> Vec<u8> {
        parts.concat()
    }

    const AREA: TinyTaskArea = TinyTaskArea {
        left: 100,
        top: 50,
        width: 801,
        height: 601,
    };

    fn import(parts: Vec<Vec<u8>>) -> TinyTaskImport {
        import_tinytask(&file(parts), AREA).expect("importa")
    }

    // ── leitura do arquivo ─────────────────────────────────────────────────

    #[test]
    fn reads_20_byte_eventmsg_records_little_endian() {
        let bytes = file(vec![key(WM_KEYDOWN, 0x57, 0x11, 1000), mouse(WM_MOUSEMOVE, -5, 300, 1010)]);
        let events = parse_tinytask(&bytes).unwrap();
        assert_eq!(
            events,
            vec![
                TinyTaskEvent { message: WM_KEYDOWN, param_l: 0x1157, param_h: 1, time: 1000 },
                TinyTaskEvent { message: WM_MOUSEMOVE, param_l: (-5i32) as u32, param_h: 300, time: 1010 },
            ]
        );
    }

    #[test]
    fn reads_24_byte_records_of_a_64_bit_recorder() {
        let bytes = file(vec![
            ev64(WM_KEYDOWN, 0x57, 1, 1000),
            ev64(WM_KEYUP, 0x57, 1, 1100),
            ev64(WM_LBUTTONDOWN, 10, 20, 1200),
        ]);
        let events = parse_tinytask(&bytes).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].message, WM_LBUTTONDOWN);
        assert_eq!(events[2].time, 1200);
    }

    #[test]
    fn refuses_what_is_not_a_tinytask_recording() {
        assert_eq!(parse_tinytask(&[]), Err(TinyTaskError::Empty));
        assert_eq!(parse_tinytask(&[0u8; 21]), Err(TinyTaskError::NotTinyTask));
        // Tamanho certo, mas a mensagem não é de teclado nem de mouse (um PNG, um texto...).
        assert_eq!(parse_tinytask(b"\x89PNG\r\n\x1a\n000000000000"), Err(TinyTaskError::NotTinyTask));
        assert_eq!(parse_tinytask(&file(vec![ev(0x0010, 0, 0, 0)])), Err(TinyTaskError::NotTinyTask));
        assert_eq!(
            parse_tinytask(&vec![0u8; MAX_TINYTASK_FILE_BYTES + 20]),
            Err(TinyTaskError::TooLarge)
        );
        assert_eq!(TinyTaskError::NotTinyTask.code(), "notTinyTask");
    }

    // ── teclas ─────────────────────────────────────────────────────────────

    #[test]
    fn a_key_pressed_and_released_becomes_a_tap_with_the_time_held() {
        let out = import(vec![key(WM_KEYDOWN, 0x20, 0x39, 5000), key(WM_KEYUP, 0x20, 0x39, 5080)]);
        assert_eq!(out.steps, vec![RecordingStep::Key { key: "Space".into(), hold_ms: 80 }]);
    }

    #[test]
    fn auto_repeat_while_a_key_is_held_is_ignored() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x57, 0x11, 0),
            key(WM_KEYDOWN, 0x57, 0x11, 500),
            key(WM_KEYDOWN, 0x57, 0x11, 530),
            key(WM_KEYUP, 0x57, 0x11, 900),
        ]);
        assert_eq!(out.steps, vec![RecordingStep::Key { key: "W".into(), hold_ms: 900 }]);
    }

    #[test]
    fn overlapping_keys_stay_as_down_wait_up() {
        // Segurar W e pular no meio: W para baixo, espera, toque no espaço, espera, W para cima.
        let out = import(vec![
            key(WM_KEYDOWN, 0x57, 0x11, 0),
            key(WM_KEYDOWN, 0x20, 0x39, 400),
            key(WM_KEYUP, 0x20, 0x39, 450),
            key(WM_KEYUP, 0x57, 0x11, 1200),
        ]);
        assert_eq!(
            out.steps,
            vec![
                RecordingStep::KeyDown { key: "W".into() },
                RecordingStep::Wait { ms: 400 },
                RecordingStep::Key { key: "Space".into(), hold_ms: 50 },
                RecordingStep::Wait { ms: 750 },
                RecordingStep::KeyUp { key: "W".into() },
            ]
        );
    }

    #[test]
    fn a_key_held_longer_than_a_tap_allows_stays_down_wait_up() {
        let out = import(vec![key(WM_KEYDOWN, 0x44, 0x20, 0), key(WM_KEYUP, 0x44, 0x20, 15_000)]);
        assert_eq!(
            out.steps,
            vec![
                RecordingStep::KeyDown { key: "D".into() },
                RecordingStep::Wait { ms: 15_000 },
                RecordingStep::KeyUp { key: "D".into() },
            ]
        );
    }

    #[test]
    fn shift_and_arrows_map_to_the_recording_keys() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x10, 0x2A, 0),
            key(WM_KEYUP, 0x10, 0x2A, 40),
            key(WM_KEYDOWN, 0xA1, 0x36, 100),
            key(WM_KEYUP, 0xA1, 0x36, 140),
            ev(WM_KEYDOWN, 0x26 | (0x48 << 8), 1 | 0x8000, 200),
            ev(WM_KEYUP, 0x26 | (0x48 << 8), 1 | 0x8000, 240),
        ]);
        let keys: Vec<&str> = out
            .steps
            .iter()
            .filter_map(|s| match s {
                RecordingStep::Key { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(keys, vec!["Shift", "Shift", "Up"]);
    }

    #[test]
    fn keys_outside_the_list_are_skipped_and_listed_once_each() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x0D, 0x1C, 0), // Enter (chat)
            key(WM_KEYUP, 0x0D, 0x1C, 30),
            key(WM_KEYDOWN, 0x1B, 0x01, 100), // Escape
            key(WM_KEYUP, 0x1B, 0x01, 130),
            key(WM_KEYDOWN, 0x0D, 0x1C, 200),
            key(WM_KEYDOWN, 0x0D, 0x1C, 230), // repetição: não conta de novo
            key(WM_KEYUP, 0x0D, 0x1C, 260),
            key(WM_KEYDOWN, 0x73, 0x3E, 300), // F4
            key(WM_KEYUP, 0x73, 0x3E, 330),
            key(WM_KEYDOWN, 0x45, 0x12, 400), // E entra
            key(WM_KEYUP, 0x45, 0x12, 440),
        ]);
        assert_eq!(out.steps, vec![RecordingStep::Key { key: "E".into(), hold_ms: 40 }]);
        assert_eq!(
            out.summary.skipped_keys,
            vec![
                SkippedKey { key: "Enter".into(), count: 2 },
                SkippedKey { key: "Escape".into(), count: 1 },
                SkippedKey { key: "F4".into(), count: 1 },
            ]
        );
    }

    /// Ctrl+W, Alt+F4 (que chega como tecla de sistema) e Windows+D não viram
    /// W, F4 ou D sozinhos: a combinação inteira é pulada.
    #[test]
    fn keys_pressed_with_ctrl_alt_or_windows_are_skipped_as_combos() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x11, 0x1D, 0),
            key(WM_KEYDOWN, 0x57, 0x11, 50),
            key(WM_KEYUP, 0x57, 0x11, 90),
            key(WM_KEYUP, 0x11, 0x1D, 120),
            key(WM_SYSKEYDOWN, 0x12, 0x38, 200),
            key(WM_SYSKEYDOWN, 0x73, 0x3E, 250),
            key(WM_SYSKEYUP, 0x73, 0x3E, 280),
            key(WM_KEYUP, 0x12, 0x38, 300),
            key(WM_KEYDOWN, 0x5B, 0x5B, 400),
            key(WM_KEYDOWN, 0x44, 0x20, 420),
            key(WM_KEYUP, 0x44, 0x20, 440),
            key(WM_KEYUP, 0x5B, 0x5B, 460),
            key(WM_KEYDOWN, 0x57, 0x11, 600), // W sozinho, depois: entra
            key(WM_KEYUP, 0x57, 0x11, 640),
        ]);
        assert_eq!(out.steps, vec![RecordingStep::Key { key: "W".into(), hold_ms: 40 }]);
        let skipped: Vec<&str> = out.summary.skipped_keys.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(skipped, vec!["Ctrl", "Ctrl+W", "Alt", "Alt+F4", "Windows", "Windows+D"]);
    }

    #[test]
    fn a_key_left_held_at_the_end_gets_a_release_step() {
        // Solta sem apertar (A) não vira passo; W apertado e nunca solto, com um
        // clique depois, ganha o "soltar" no fim.
        let out = import(vec![
            key(WM_KEYUP, 0x41, 0x1E, 0),
            key(WM_KEYDOWN, 0x57, 0x11, 10),
            mouse(WM_LBUTTONDOWN, 500, 350, 300),
            mouse(WM_LBUTTONUP, 500, 350, 340),
        ]);
        assert_eq!(
            out.steps,
            vec![
                RecordingStep::KeyDown { key: "W".into() },
                RecordingStep::Wait { ms: 290 },
                RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 },
                RecordingStep::KeyUp { key: "W".into() },
            ]
        );
        assert_eq!(out.summary.stop_key, None);
    }

    /// A gravação do TinyTask para no "apertar" do atalho de parar: a última
    /// tecla vem apertada e nunca solta. Ela fica fora e o resumo diz qual foi.
    #[test]
    fn the_tinytask_stop_key_at_the_end_is_dropped_and_reported() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x57, 0x11, 0),
            key(WM_KEYUP, 0x57, 0x11, 40),
            mouse(WM_MOUSEMOVE, 1, 1, 500),
            key(WM_KEYDOWN, 0x78, 0x43, 900), // F9
            mouse(WM_MOUSEMOVE, 2, 2, 950),
        ]);
        assert_eq!(out.steps, vec![RecordingStep::Key { key: "W".into(), hold_ms: 40 }]);
        assert_eq!(out.summary.stop_key.as_deref(), Some("F9"));
        assert!(out.summary.skipped_keys.is_empty(), "a tecla de parar não é tecla pulada");

        // Atalho com modificadores: as teclas apertadas no fim vão juntas.
        let combo = import(vec![
            mouse(WM_LBUTTONDOWN, 500, 350, 0),
            mouse(WM_LBUTTONUP, 500, 350, 40),
            key(WM_KEYDOWN, 0x11, 0x1D, 300),
            key(WM_KEYDOWN, 0x52, 0x13, 320),
        ]);
        assert_eq!(combo.steps, vec![RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 }]);
        assert_eq!(combo.summary.stop_key.as_deref(), Some("Ctrl+R"));
    }

    /// O formato exato de um `.rec` real do dono (medido em 11/10/2026, sem
    /// copiar o arquivo): 175 registros de 20 bytes — 162 movimentos de mouse,
    /// 6 pares de botão esquerdo, 1 tecla apertada no fim (a de parar) —, ~4,7 s,
    /// coordenadas absolutas de tela. Aqui o buffer é sintético, com a mesma forma.
    #[test]
    fn a_buffer_shaped_like_a_real_tinytask_file_becomes_six_clicks() {
        let base = 87_000_000u32; // um GetTickCount qualquer
        let points = [(-1500, 200), (-1200, 420), (-900, 300), (-600, 650), (-400, 120), (-200, 900)];
        let area = TinyTaskArea { left: -1920, top: 0, width: 1920, height: 1080 };
        let mut parts = Vec::new();
        let mut t = base;
        let mut moves = 0;
        for (x, y) in points.iter() {
            for m in 0..27 {
                parts.push(mouse(WM_MOUSEMOVE, x - 30 + m, y - 30 + m, t));
                t += 25;
                moves += 1;
            }
            parts.push(mouse(WM_LBUTTONDOWN, *x, *y, t));
            t += 60;
            parts.push(mouse(WM_LBUTTONUP, *x, *y, t));
            t += 20;
        }
        parts.push(key(WM_KEYDOWN, 0x78, 0x43, t + 40)); // a tecla de parar
        let bytes = file(parts);
        assert_eq!(moves, 162);
        assert_eq!(bytes.len(), 3500);
        assert_eq!(bytes.len() / 20, 175);

        let out = import_tinytask(&bytes, area).unwrap();
        let clicks = out.steps.iter().filter(|s| matches!(s, RecordingStep::Click { .. })).count();
        let waits: Vec<u64> = out
            .steps
            .iter()
            .filter_map(|s| match s {
                RecordingStep::Wait { ms } => Some(*ms),
                _ => None,
            })
            .collect();
        assert_eq!(clicks, 6);
        // Entre um clique e o próximo: 60 + 20 + 27 × 25 = 755 ms.
        assert_eq!(waits, vec![755; 5]);
        assert_eq!(out.steps.len(), 11);
        assert_eq!(out.summary.events, 175);
        assert_eq!(out.summary.stop_key.as_deref(), Some("F9"));
        assert!(out.summary.skipped_keys.is_empty());
        assert_eq!(out.summary.clicks_outside, 0);
        match out.steps[0] {
            // (-1500 + 1920) / 1919 ≈ 21,89%; 200 / 1079 ≈ 18,54%
            RecordingStep::Click { x_pct, y_pct } => assert_eq!((x_pct, y_pct), (21.89, 18.54)),
            ref other => panic!("{other:?}"),
        }
    }

    // ── cliques ────────────────────────────────────────────────────────────

    #[test]
    fn a_left_click_becomes_a_click_at_the_point_relative_to_the_window() {
        // Área de 801x601 em (100, 50): o canto, o meio e o último pixel.
        let out = import(vec![
            mouse(WM_MOUSEMOVE, 0, 0, 0),
            mouse(WM_LBUTTONDOWN, 100, 50, 0),
            mouse(WM_LBUTTONUP, 100, 50, 60),
            mouse(WM_LBUTTONDOWN, 500, 350, 1000),
            mouse(WM_LBUTTONUP, 500, 350, 1050),
            mouse(WM_LBUTTONDOWN, 900, 650, 2000),
            mouse(WM_LBUTTONUP, 900, 650, 2040),
        ]);
        assert_eq!(
            out.steps,
            vec![
                RecordingStep::Click { x_pct: 0.0, y_pct: 0.0 },
                RecordingStep::Wait { ms: 1000 },
                RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 },
                RecordingStep::Wait { ms: 1000 },
                RecordingStep::Click { x_pct: 100.0, y_pct: 100.0 },
            ]
        );
    }

    /// O mesmo ponto relativo em janelas de tamanhos diferentes (uma no
    /// monitor da esquerda, x negativo) dá a mesma porcentagem; e a proporção
    /// de cada janela vai junto.
    #[test]
    fn the_same_relative_point_in_windows_of_different_sizes_gives_the_same_percent() {
        let small = TinyTaskArea { left: 100, top: 100, width: 801, height: 601 };
        let big_left = TinyTaskArea { left: -1700, top: 50, width: 1601, height: 901 };
        // 25% × 75% de cada uma.
        let a = import_tinytask(
            &file(vec![mouse(WM_LBUTTONDOWN, 100 + 200, 100 + 450, 0), mouse(WM_LBUTTONUP, 300, 550, 40)]),
            small,
        )
        .unwrap();
        let b = import_tinytask(
            &file(vec![mouse(WM_LBUTTONDOWN, -1700 + 400, 50 + 675, 0), mouse(WM_LBUTTONUP, -1300, 725, 40)]),
            big_left,
        )
        .unwrap();
        assert_eq!(a.steps, vec![RecordingStep::Click { x_pct: 25.0, y_pct: 75.0 }]);
        assert_eq!(b.steps, a.steps);
        assert_eq!(a.source_aspect, 1.3328);
        assert_eq!(b.source_aspect, 1.7769);
        let full_hd = TinyTaskArea { left: 0, top: 0, width: 1920, height: 1080 };
        assert_eq!(area_aspect(full_hd), 1.7778);
    }

    /// Clique fora da janela escolhida nunca é puxado para a borda: fica fora
    /// e é contado — inclusive um pixel à esquerda de uma janela em x negativo.
    #[test]
    fn a_click_just_outside_a_negative_x_window_is_dropped_not_clamped() {
        let area = TinyTaskArea { left: -1920, top: 0, width: 1920, height: 1080 };
        let out = import_tinytask(
            &file(vec![
                mouse(WM_LBUTTONDOWN, -1921, 500, 0),
                mouse(WM_LBUTTONUP, -1921, 500, 40),
                mouse(WM_LBUTTONDOWN, 0, 500, 100), // primeiro pixel do monitor principal
                mouse(WM_LBUTTONUP, 0, 500, 140),
                mouse(WM_LBUTTONDOWN, -1920, 1079, 200), // canto de baixo, dentro
                mouse(WM_LBUTTONUP, -1920, 1079, 240),
            ]),
            area,
        )
        .unwrap();
        assert_eq!(out.steps, vec![RecordingStep::Click { x_pct: 0.0, y_pct: 100.0 }]);
        assert_eq!(out.summary.clicks_outside, 2);
    }

    /// Janela num monitor à esquerda do principal: coordenadas negativas.
    #[test]
    fn clicks_on_a_monitor_left_of_the_primary_use_negative_coordinates() {
        let area = TinyTaskArea { left: -1920, top: 0, width: 1921, height: 1081 };
        let out = import_tinytask(
            &file(vec![mouse(WM_LBUTTONDOWN, -960, 540, 0), mouse(WM_LBUTTONUP, -960, 540, 50)]),
            area,
        )
        .unwrap();
        assert_eq!(out.steps, vec![RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 }]);
    }

    #[test]
    fn clicks_outside_the_window_and_other_buttons_are_counted_not_played() {
        let out = import(vec![
            mouse(WM_LBUTTONDOWN, 10, 10, 0), // fora (à esquerda e acima)
            mouse(WM_LBUTTONUP, 10, 10, 40),
            mouse(WM_LBUTTONDOWN, 901, 300, 100), // um pixel à direita da área
            mouse(WM_LBUTTONUP, 901, 300, 140),
            mouse(0x0204, 300, 300, 200), // botão direito
            mouse(0x0205, 300, 300, 240),
            mouse(0x020A, 300, 300, 300), // roda
            mouse(0x0207, 300, 300, 400), // meio
            mouse(0x0208, 300, 300, 440),
        ]);
        assert!(out.steps.is_empty());
        assert_eq!(out.summary.clicks_outside, 2);
        assert_eq!(out.summary.other_mouse, 3);
        assert_eq!(out.summary.events, 9);
    }

    /// O `.rec` do dono tinha botão solto longe de onde desceu (arrastar). A
    /// gravação não tem arrasto: fica o clique onde desceu, e o resumo conta.
    #[test]
    fn a_drag_becomes_a_click_where_the_button_went_down_and_is_counted() {
        let out = import(vec![
            mouse(WM_LBUTTONDOWN, 500, 350, 0),
            mouse(WM_MOUSEMOVE, 540, 350, 100),
            mouse(WM_LBUTTONUP, 572, 349, 260), // 72 px adiante
            mouse(WM_LBUTTONDOWN, 300, 300, 500),
            mouse(WM_LBUTTONUP, 305, 302, 540), // tremida de 5 px: clique comum
        ]);
        assert_eq!(out.summary.drags, 1);
        assert_eq!(out.steps[0], RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 });
        assert_eq!(out.steps.len(), 3);
    }

    #[test]
    fn a_double_click_is_two_clicks_and_holding_the_button_is_one() {
        let out = import(vec![
            mouse(WM_LBUTTONDOWN, 500, 350, 0),
            mouse(WM_LBUTTONDOWN, 500, 350, 10), // sem o "up" no meio: o mesmo clique
            mouse(WM_LBUTTONUP, 500, 350, 50),
            mouse(WM_LBUTTONDBLCLK, 500, 350, 150),
            mouse(WM_LBUTTONUP, 500, 350, 190),
        ]);
        assert_eq!(
            out.steps,
            vec![
                RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 },
                RecordingStep::Wait { ms: 150 },
                RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 },
            ]
        );
    }

    // ── tempo ──────────────────────────────────────────────────────────────

    #[test]
    fn the_idle_time_before_the_first_step_and_mouse_moves_add_no_steps() {
        let out = import(vec![
            mouse(WM_MOUSEMOVE, 1, 1, 0),
            mouse(WM_MOUSEMOVE, 2, 2, 9_000),
            key(WM_KEYDOWN, 0x57, 0x11, 10_000),
            key(WM_KEYUP, 0x57, 0x11, 10_050),
            mouse(WM_MOUSEMOVE, 3, 3, 10_300),
            mouse(WM_MOUSEMOVE, 4, 4, 10_600),
            key(WM_KEYDOWN, 0x41, 0x1E, 11_050),
            key(WM_KEYUP, 0x41, 0x1E, 11_090),
            mouse(WM_MOUSEMOVE, 5, 5, 40_000), // depois do último passo: não vira espera
        ]);
        assert_eq!(
            out.steps,
            vec![
                RecordingStep::Key { key: "W".into(), hold_ms: 50 },
                RecordingStep::Wait { ms: 1000 },
                RecordingStep::Key { key: "A".into(), hold_ms: 40 },
            ]
        );
    }

    #[test]
    fn tiny_gaps_are_merged_into_the_next_wait() {
        // Dois toques colados (5 ms entre eles) e depois 300 ms: os 5 ms não
        // viram passo, somam na espera seguinte.
        let out = import(vec![
            mouse(WM_LBUTTONDOWN, 500, 350, 0),
            mouse(WM_LBUTTONUP, 500, 350, 0),
            key(WM_KEYDOWN, 0x57, 0x11, 5),
            key(WM_KEYUP, 0x57, 0x11, 45),
            key(WM_KEYDOWN, 0x41, 0x1E, 50),
            key(WM_KEYUP, 0x41, 0x1E, 90),
            mouse(WM_LBUTTONDOWN, 500, 350, 395),
            mouse(WM_LBUTTONUP, 500, 350, 400),
        ]);
        assert_eq!(
            out.steps,
            vec![
                RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 },
                RecordingStep::Key { key: "W".into(), hold_ms: 40 },
                RecordingStep::Key { key: "A".into(), hold_ms: 40 },
                RecordingStep::Wait { ms: 315 },
                RecordingStep::Click { x_pct: 50.0, y_pct: 50.0 },
            ]
        );
    }

    #[test]
    fn long_gaps_are_capped_and_counted() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x57, 0x11, 0),
            key(WM_KEYUP, 0x57, 0x11, 40),
            key(WM_KEYDOWN, 0x41, 0x1E, 40 + 5 * 60_000),
            key(WM_KEYUP, 0x41, 0x1E, 80 + 5 * 60_000),
        ]);
        assert_eq!(out.steps[1], RecordingStep::Wait { ms: TINYTASK_MAX_WAIT_MS });
        assert_eq!(out.summary.capped_waits, 1);
    }

    /// O relógio do Windows (`GetTickCount`) dá a volta em ~49 dias.
    #[test]
    fn the_tick_count_wrapping_around_keeps_the_right_gap() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x57, 0x11, u32::MAX - 99),
            key(WM_KEYUP, 0x57, 0x11, 20), // 120 ms depois, já do outro lado
        ]);
        assert_eq!(out.steps, vec![RecordingStep::Key { key: "W".into(), hold_ms: 120 }]);
    }

    #[test]
    fn a_tap_never_holds_less_than_the_minimum() {
        let out = import(vec![key(WM_KEYDOWN, 0x57, 0x11, 100), key(WM_KEYUP, 0x57, 0x11, 103)]);
        assert_eq!(out.steps, vec![RecordingStep::Key { key: "W".into(), hold_ms: MIN_HOLD_MS }]);
        let same_tick = import(vec![key(WM_KEYDOWN, 0x57, 0x11, 100), key(WM_KEYUP, 0x57, 0x11, 100)]);
        assert_eq!(same_tick.steps, vec![RecordingStep::Key { key: "W".into(), hold_ms: DEFAULT_HOLD_MS }]);
    }

    // ── limites ────────────────────────────────────────────────────────────

    #[test]
    fn more_steps_than_a_recording_holds_are_cut_and_flagged() {
        let mut parts = Vec::new();
        for i in 0..(MAX_RECORDING_STEPS as u32) {
            parts.push(mouse(WM_LBUTTONDOWN, 500, 350, i * 100));
            parts.push(mouse(WM_LBUTTONUP, 500, 350, i * 100 + 10));
        }
        let out = import(parts);
        assert_eq!(out.steps.len(), MAX_RECORDING_STEPS);
        assert!(out.summary.truncated);
    }

    #[test]
    fn an_empty_reference_area_is_refused() {
        let bytes = file(vec![key(WM_KEYDOWN, 0x57, 0x11, 0)]);
        let empty = TinyTaskArea { left: 0, top: 0, width: 0, height: 600 };
        assert_eq!(import_tinytask(&bytes, empty), Err(TinyTaskError::NotTinyTask));
    }

    /// O que sai da importação passa na validação das gravações (o editor
    /// salva sem a pessoa ter de consertar nada).
    #[test]
    fn the_imported_steps_pass_the_recording_validation() {
        let out = import(vec![
            key(WM_KEYDOWN, 0x57, 0x11, 0),
            mouse(WM_LBUTTONDOWN, 500, 350, 300),
            mouse(WM_LBUTTONUP, 500, 350, 340),
            key(WM_KEYDOWN, 0x20, 0x39, 600),
            key(WM_KEYUP, 0x20, 0x39, 640),
            key(WM_KEYUP, 0x57, 0x11, 2000),
        ]);
        let recording = super::super::recordings::Recording {
            name: "From TinyTask".into(),
            steps: out.steps,
            ..Default::default()
        };
        assert!(super::super::recordings::normalize_recording(recording).is_ok());
    }

    /// Converte um `.rec` de verdade e imprime os passos, para conferir à mão:
    /// `TINYTASK_SAMPLE=<arquivo> [TINYTASK_AREA=esq,topo,larg,alt] cargo test
    /// converts_the_file_in_tinytask_sample -- --ignored --nocapture`. Nenhum
    /// arquivo de gravação mora no repositório.
    #[test]
    #[ignore = "lê um .rec de fora do repositório (TINYTASK_SAMPLE)"]
    fn converts_the_file_in_tinytask_sample() {
        let Ok(path) = std::env::var("TINYTASK_SAMPLE") else {
            return;
        };
        let area: Vec<i32> = std::env::var("TINYTASK_AREA")
            .unwrap_or_else(|_| "0,0,2560,1440".into())
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        let area = TinyTaskArea { left: area[0], top: area[1], width: area[2], height: area[3] };
        let bytes = std::fs::read(&path).expect("lê o arquivo");
        let out = import_tinytask(&bytes, area).expect("importa");
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    }

    #[test]
    fn the_summary_reaches_the_screen_in_camel_case() {
        let json = serde_json::to_value(TinyTaskSummary {
            events: 3,
            skipped_keys: vec![SkippedKey { key: "Enter".into(), count: 2 }],
            clicks_outside: 1,
            other_mouse: 0,
            capped_waits: 0,
            drags: 0,
            truncated: false,
            stop_key: Some("F9".into()),
        })
        .unwrap();
        assert_eq!(json["skippedKeys"][0]["key"], "Enter");
        assert_eq!(json["clicksOutside"], 1);
        assert_eq!(json["cappedWaits"], 0);
        assert_eq!(json["stopKey"], "F9");
    }
}
