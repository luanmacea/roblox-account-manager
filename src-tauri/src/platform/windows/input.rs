// Entrada sintética para a janela de um cliente Roblox (AFK mode).
//
// Por que `SendInput` e não `PostMessage`/`SendMessage`: o cliente do Roblox lê
// teclado pelo caminho de entrada do sistema, e mensagem postada na fila da
// janela não move o personagem. O preço do `SendInput` é o foco — ele entrega a
// tecla na janela que está em **primeiro plano**, então quem envia precisa
// trazer a janela do Roblox para frente, **confirmar que ela chegou lá** e
// devolver o foco depois (o ciclo mora em `commands/afk.rs`).
//
// Este módulo só **envia**, e por portas estreitas:
// - `tap_afk_key` recebe **nome** de tecla e o resolve pela lista fechada
//   (`send_key` é privado): não existe chamador com virtual key cru;
// - `click_afk_point` recebe janela + **porcentagem** e clica com o botão
//   esquerdo dentro da área interna dela (`send_mouse` é privado): não existe
//   chamador com coordenada de tela crua. O cursor volta para onde estava;
// - as Gravações (docs/features/recordings.md) usam as mesmas duas formas:
//   `press_recording_key` recebe **nome** de tecla da lista fechada das
//   gravações (`RECORDING_KEYS`) e `click_recording_point` é o clique do AFK,
//   com o clique de foco opcional.
// Ler teclado ou botão do usuário é proibido aqui; a posição do cursor é lida só
// para devolvê-lo e para o Marcar. A trava é o `afk_input_safety_tests` (em
// `commands/afk.rs`), que varre este arquivo e só aqui aceita injeção de mouse.

use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, GetSystemMetrics, IsWindow, SetCursorPos, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Quantas vezes o "solta a tecla" é tentado. Tecla que fica logicamente
/// pressionada no cliente faz o personagem **andar** — exatamente o que o AFK
/// mode existe para evitar.
const KEY_UP_ATTEMPTS: u32 = 3;
/// Respiro entre duas tentativas de soltar a tecla.
const KEY_UP_RETRY_MS: u64 = 15;

/// `(virtual key, scan code)` de uma tecla da lista fechada do AFK mode, ou
/// `None` para qualquer outro nome.
///
/// A lista fica no raiz do crate (`commands/afk.rs`) porque a tela também a
/// consulta; aqui só se acrescenta o scan code, que depende do layout de teclado
/// ativo. `MapVirtualKeyW` **traduz** um código de tecla pelo layout: não lê
/// tecla pressionada nem estado de teclado.
fn vk_and_scan(key: &str) -> Option<(u16, u16)> {
    let vk = crate::afk_virtual_key(key)?;
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    Some((vk, scan))
}

/// A janela ainda existe? O cliente pode ter fechado entre um ciclo e o
/// seguinte, e aí não há para onde mandar tecla.
pub fn window_exists(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }
    unsafe { IsWindow(hwnd) != 0 }
}

/// Um evento de tecla para a janela em primeiro plano: `up = false` pressiona,
/// `up = true` solta. Devolve `false` quando o Windows recusou o envio.
///
/// Privado: quem chama passa **nome** de tecla por `tap_afk_key` ou
/// `press_recording_key`, nunca um virtual key cru.
fn send_key(vk: u16, scan: u16, up: bool) -> bool {
    send_key_flags(vk, scan, up, false)
}

/// `send_key` com a marca de tecla estendida (as setas da lista das gravações).
fn send_key_flags(vk: u16, scan: u16, up: bool, extended: bool) -> bool {
    let mut flags = if up { KEYEVENTF_KEYUP } else { 0 };
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) == 1 }
}

/// Um toque na tecla `key` (da lista fechada) na janela que está em primeiro
/// plano: pressiona, espera `hold_ms` e solta.
///
/// O "solta" é tentado até três vezes: se ele falhar, a tecla fica logicamente
/// pressionada dentro do cliente e o personagem sai andando.
pub fn tap_afk_key(key: &str, hold_ms: u64) -> Result<(), String> {
    let (vk, scan) =
        vk_and_scan(key).ok_or_else(|| format!("Key not allowed in AFK mode: {}", key))?;

    let down = send_key(vk, scan, false);
    std::thread::sleep(std::time::Duration::from_millis(hold_ms));

    let mut up = false;
    for attempt in 0..KEY_UP_ATTEMPTS {
        if send_key(vk, scan, true) {
            up = true;
            break;
        }
        if attempt + 1 < KEY_UP_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(KEY_UP_RETRY_MS));
        }
    }

    if !down {
        return Err("Windows refused the synthetic key".into());
    }
    if !up {
        return Err("Windows refused to release the key".into());
    }
    Ok(())
}

/// `(virtual key, scan code, estendida)` de uma tecla da lista fechada das
/// Gravações, ou `None` para qualquer outro nome.
fn recording_vk_and_scan(key: &str) -> Option<(u16, u16, bool)> {
    let (vk, extended) = crate::data::recordings::recording_key(key)?;
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    Some((vk, scan, extended))
}

/// Aperta (`up = false`) ou solta (`up = true`) uma tecla da lista das
/// Gravações na janela em primeiro plano. Tecla fora da lista: `false`, sem
/// enviar nada. O "solta" é tentado até três vezes, como no AFK: tecla presa
/// faz o personagem andar sozinho.
pub fn press_recording_key(key: &str, up: bool) -> bool {
    let Some((vk, scan, extended)) = recording_vk_and_scan(key) else {
        return false;
    };
    if !up {
        return send_key_flags(vk, scan, false, extended);
    }
    for attempt in 0..KEY_UP_ATTEMPTS {
        if send_key_flags(vk, scan, true, extended) {
            return true;
        }
        if attempt + 1 < KEY_UP_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(KEY_UP_RETRY_MS));
        }
    }
    false
}

/// Área interna (cliente) da janela, em coordenadas de tela. `None` para janela
/// nula, fechada ou sem área.
pub fn client_rect_on_screen(hwnd: HWND) -> Option<crate::AfkClientRect> {
    if !window_exists(hwnd) {
        return None;
    }
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    let mut origin = POINT { x: 0, y: 0 };
    if unsafe { ClientToScreen(hwnd, &mut origin) } == 0 {
        return None;
    }
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(crate::AfkClientRect {
        left: origin.x,
        top: origin.y,
        width,
        height,
    })
}

/// Posição do cursor. **Só a posição**: nunca botão, nunca tecla.
pub fn cursor_position() -> Option<(i32, i32)> {
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        None
    } else {
        Some((point.x, point.y))
    }
}

/// Um evento de mouse cru: movimento (`dx`, `dy`) e/ou botão, conforme `flags`.
/// Privado: as portas são `click_afk_point`, que decide tudo a partir da
/// janela, da porcentagem e da receita do clique, e `nudge_for_focus_back`.
fn send_mouse(dx: i32, dy: i32, flags: u32) -> bool {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) == 1 }
}

/// Um movimento de mouse de **zero** pixel, para o AFK mode poder trazer a janela
/// da conta e, no fim do ciclo, devolver o foco (issue #23).
///
/// O Windows só aceita o `SetForegroundWindow` de quem gerou a última entrada.
/// No ciclo normal essa entrada é a tecla ou o clique do AFK; se o usuário
/// mexe em outra janela depois dela (o outro monitor), a volta do foco é
/// recusada e o Roblox fica na frente. Este evento não move o cursor, não
/// aperta nada e não lê nada: só faz deste processo, de novo, o da última
/// entrada. Medido em 10/10/2026: recusado sem ele, aceito com ele (4 de 4).
pub fn nudge_for_focus_back() -> bool {
    send_mouse(0, 0, MOUSEEVENTF_MOVE)
}

/// A área de trabalho virtual (todos os monitores), em pixels de tela.
fn virtual_desktop() -> Option<crate::AfkClientRect> {
    let desktop = unsafe {
        crate::AfkClientRect {
            left: GetSystemMetrics(SM_XVIRTUALSCREEN),
            top: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    };
    (desktop.width > 0 && desktop.height > 0).then_some(desktop)
}

/// Um clique esquerdo no ponto relativo `point` da área interna da janela
/// `hwnd`, que o chamador **já confirmou** estar em primeiro plano.
///
/// Executa a receita de `afk_click_plan`: o movimento sai pelo `SendInput` (o
/// Roblox lê o mouse por entrada crua e não enxerga só o cursor posto no
/// lugar), com tremor e dois cliques — o primeiro pode só focar o jogo. No fim o
/// cursor do usuário volta para onde estava, inclusive quando o clique falha.
///
/// O "solta" é tentado até três vezes: botão que fica pressionado vira arrastar
/// dentro do jogo.
pub fn click_afk_point(hwnd: HWND, point: crate::AfkPoint, hold_ms: u64) -> Result<(), String> {
    click_point_with_plan(hwnd, point, hold_ms, true)
}

/// O clique de uma Gravação: a mesma receita do AFK (`click_afk_point`), e o
/// clique de foco só quando `focus_click` — o primeiro clique depois de trazer
/// a janela. Os seguintes da mesma gravação já acham o jogo focado, e um clique
/// a mais ali apertaria o botão do jogo duas vezes.
pub fn click_recording_point(
    hwnd: HWND,
    point: crate::AfkPoint,
    hold_ms: u64,
    focus_click: bool,
) -> Result<(), String> {
    click_point_with_plan(hwnd, point, hold_ms, focus_click)
}

fn click_point_with_plan(
    hwnd: HWND,
    point: crate::AfkPoint,
    hold_ms: u64,
    focus_click: bool,
) -> Result<(), String> {
    let rect = client_rect_on_screen(hwnd).ok_or("The window has no game area")?;
    let steps = crate::afk_click_plan_with(rect, point, hold_ms, focus_click)
        .ok_or("The window has no game area")?;
    let desktop = virtual_desktop().ok_or("Could not read the screen size")?;
    let back = cursor_position();

    let result = (|| {
        for step in steps {
            match step {
                crate::AfkMouseStep::MoveTo(x, y) => {
                    let (nx, ny) = crate::afk_absolute_input(x, y, desktop);
                    if !send_mouse(
                        nx,
                        ny,
                        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    ) {
                        return Err("Windows refused to move the mouse".to_string());
                    }
                }
                crate::AfkMouseStep::Nudge(dx, dy) => {
                    if !send_mouse(dx, dy, MOUSEEVENTF_MOVE) {
                        return Err("Windows refused to move the mouse".to_string());
                    }
                }
                crate::AfkMouseStep::Wait(ms) => {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                crate::AfkMouseStep::Press => {
                    if !send_mouse(0, 0, MOUSEEVENTF_LEFTDOWN) {
                        return Err("Windows refused the synthetic click".to_string());
                    }
                }
                crate::AfkMouseStep::Release => {
                    let mut released = false;
                    for attempt in 0..KEY_UP_ATTEMPTS {
                        if send_mouse(0, 0, MOUSEEVENTF_LEFTUP) {
                            released = true;
                            break;
                        }
                        if attempt + 1 < KEY_UP_ATTEMPTS {
                            std::thread::sleep(std::time::Duration::from_millis(KEY_UP_RETRY_MS));
                        }
                    }
                    if !released {
                        return Err("Windows refused to release the click".to_string());
                    }
                }
            }
        }
        Ok(())
    })();

    // Devolver o cursor só reposiciona o ponteiro: não gera movimento que o
    // jogo leia, então não mexe em nada dentro dele.
    if let Some((bx, by)) = back {
        unsafe { SetCursorPos(bx, by) };
    }
    result
}

#[cfg(test)]
mod win_input_tests {
    use super::*;

    // Nada aqui envia entrada: `send_key`/`tap_afk_key` e `send_mouse`/
    // `click_afk_point` só são chamados com janela nula, que é recusada antes de
    // qualquer envio. `vk_and_scan` só traduz um código de tecla pelo layout
    // ativo.

    #[test]
    fn a_listed_key_gets_a_virtual_key_and_a_scan_code() {
        let (vk, scan) = vk_and_scan("Space").expect("Space está na lista");
        assert_eq!(vk, 0x20);
        assert_ne!(scan, 0, "o layout tem de dar um scan code para o espaço");

        let (vk_w, _) = vk_and_scan("w").expect("o nome não é sensível a caixa");
        assert_eq!(vk_w, 0x57);
    }

    #[test]
    fn a_key_outside_the_list_has_no_codes() {
        for outside in ["Enter", "F4", "Tab", "LWin", "", "Z"] {
            assert!(
                vk_and_scan(outside).is_none(),
                "tecla fora da lista foi traduzida: {outside:?}"
            );
        }
    }

    #[test]
    fn a_null_window_never_counts_as_existing() {
        assert!(!window_exists(std::ptr::null_mut()));
    }

    #[test]
    fn a_null_window_has_no_game_area() {
        assert_eq!(client_rect_on_screen(std::ptr::null_mut()), None);
    }

    #[test]
    fn a_recording_key_gets_codes_and_arrows_are_extended() {
        let (vk, scan, extended) = recording_vk_and_scan("Up").expect("Up está na lista");
        assert_eq!(vk, 0x26);
        assert!(extended);
        assert_ne!(scan, 0);
        let (vk_w, _, ext_w) = recording_vk_and_scan("w").expect("W está na lista");
        assert_eq!(vk_w, 0x57);
        assert!(!ext_w);
    }

    #[test]
    fn a_key_outside_the_recording_list_is_never_sent() {
        for outside in ["Enter", "Escape", "F4", "Tab", "LWin", ""] {
            assert!(recording_vk_and_scan(outside).is_none(), "{outside:?}");
            // Recusado antes de qualquer envio.
            assert!(!press_recording_key(outside, true), "{outside:?}");
        }
    }

    #[test]
    fn a_null_window_never_gets_a_recording_click() {
        assert!(click_recording_point(std::ptr::null_mut(), crate::AFK_DEFAULT_POINT, 1, false).is_err());
    }

    #[test]
    fn a_null_window_is_never_clicked() {
        // Recusado antes de mover o cursor: nada é enviado.
        assert!(click_afk_point(std::ptr::null_mut(), crate::AFK_DEFAULT_POINT, 1).is_err());
    }
}
