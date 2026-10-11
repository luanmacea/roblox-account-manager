struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}

/// Release signal for the dedicated thread that owns `ROBLOX_singletonMutex`.
/// Win32 mutex ownership is per-thread: ReleaseMutex only works on the thread
/// that acquired it, and the mutex is abandoned (and can be taken by a Roblox
/// client, re-enabling single-instance mode) if that thread exits. Tokio worker
/// and blocking threads give neither guarantee, so one long-lived thread
/// acquires, holds and releases it.
static MULTI_ROBLOX_HANDLE: Mutex<Option<std::sync::mpsc::Sender<()>>> = Mutex::new(None);
static COOKIES_LOCK_HANDLE: Mutex<Option<SendHandle>> = Mutex::new(None);
/// Reserva experimental do nome `ROBLOX_singletonEvent` (ideia 3): um Mutex
/// **nosso** com esse nome. Enquanto ele existe, nenhum cliente consegue criar
/// o Event de instância única, e um teleporte não derruba outro cliente. Só
/// guardamos o handle — não é preciso ser dono do mutex, então qualquer thread
/// pode criar e fechar.
static SINGLETON_RESERVATION: Mutex<Option<SendHandle>> = Mutex::new(None);
/// A opção "Experimental: keep clients open across teleports"
/// (`General.ReserveSingletonEvent`), desligada por padrão.
static RESERVE_SINGLETON_EVENT: AtomicBool = AtomicBool::new(false);
static TRACKER: LazyLock<ProcessTracker> = LazyLock::new(ProcessTracker::new);

fn encode_wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

pub fn tracker() -> &'static ProcessTracker {
    &TRACKER
}

pub fn generate_browser_tracker_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let a = (now % 75000 + 100000) as u64;
    let b = ((now / 31) % 800000 + 100000) as u64;
    format!("{}{}", a, b)
}

/// Sobe a thread dedicada e tenta tomar posse de `ROBLOX_singletonMutex`.
/// `Ok(None)` = o mutex está com outro processo (cliente aberto, RAM legado,
/// outra ferramenta). A thread só sobrevive se a posse for conquistada.
fn spawn_singleton_mutex_owner() -> Result<Option<std::sync::mpsc::Sender<()>>, String> {
    let (acquired_tx, acquired_rx) = std::sync::mpsc::channel::<Result<bool, String>>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("multi-roblox-mutex".into())
        .spawn(move || {
            let name = encode_wide("ROBLOX_singletonMutex");
            let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
            if h.is_null() {
                let _ = acquired_tx.send(Err("Failed to create mutex".into()));
                return;
            }
            let result = unsafe { WaitForSingleObject(h, 0) };
            if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED_0 {
                unsafe { CloseHandle(h) };
                let _ = acquired_tx.send(Ok(false));
                return;
            }
            let _ = acquired_tx.send(Ok(true));
            // Hold ownership until asked to release (or the sender is dropped).
            let _ = release_rx.recv();
            unsafe {
                ReleaseMutex(h);
                CloseHandle(h);
            }
        })
        .map_err(|e| format!("Failed to spawn mutex thread: {}", e))?;

    match acquired_rx.recv() {
        Ok(Ok(true)) => Ok(Some(release_tx)),
        Ok(Ok(false)) => Ok(None),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Mutex thread exited unexpectedly".into()),
    }
}

/// Garante que a thread dedicada esteja segurando `ROBLOX_singletonMutex`.
/// `Ok(true)` = já segurávamos ou acabamos de conquistar.
fn acquire_multi_roblox_mutex() -> Result<bool, String> {
    let mut handle = MULTI_ROBLOX_HANDLE.lock().map_err(|e| e.to_string())?;
    if handle.is_some() {
        return Ok(true);
    }
    match spawn_singleton_mutex_owner()? {
        Some(release_tx) => {
            *handle = Some(release_tx);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Habilita vários clientes Roblox simultâneos.
///
/// São duas travas diferentes, e elas **não** se substituem:
///
/// 1. `ROBLOX_singletonMutex` — trava legada. Só funciona preventivamente: se o
///    app pegar o mutex **antes** de qualquer cliente, os clientes seguintes
///    sobem sem instalar o modo instância única. Se um cliente já estiver
///    aberto (o usuário entrou pelo site), o mutex é dele e não há como tomá-lo
///    sem matar o processo — era exatamente aí que o app dizia "A Roblox client
///    is already running" ou matava os clientes do usuário.
/// 2. `ROBLOX_singletonEvent` — é o que o cliente **moderno** consulta ao subir:
///    se o nome existe, ele sinaliza a instância antiga e sai. Fechar esse
///    handle de fora (`singleton.rs`) apaga o nome sem tocar no processo: o
///    cliente aberto continua jogando e o próximo sobe normal. Verificado na
///    máquina do usuário, sem elevação.
///
/// Conclusão: para o cliente atual o **Event é o que importa**; o mutex fica
/// porque continua sendo a trava barata e preventiva (e builds antigas ainda
/// dependem dele). Por isso o mutex é mantido como estava e o fechamento do
/// Event entrou como etapa extra — inclusive no caminho de sucesso, para cobrir
/// o caso "app já segurava o mutex e o usuário abriu o jogo pelo site depois".
/// Sem cliente Roblox aberto a etapa nova não custa nada (sai na checagem de
/// pids) e o comportamento antigo é idêntico.
pub fn enable_multi_roblox() -> Result<bool, String> {
    if acquire_multi_roblox_mutex()? {
        // Um cliente aberto fora do app pode ter publicado o Event mesmo com o
        // mutex na nossa mão; limpar é barato e não fecha ninguém.
        let _ = close_roblox_singleton_handles();
        apply_singleton_reservation();
        lock_roblox_cookies()?;
        return Ok(true);
    }

    // Mutex ocupado. Antes de desistir (e antes que o chamador caia no último
    // recurso de matar clientes com `AutoCloseRobloxForMultiRbx`), destrava
    // pelo Event — ou constata que ele já não existe.
    let closed_now = close_roblox_singleton_handles();
    apply_singleton_reservation();
    let roblox_running = !find_roblox_pids_all().is_empty();
    if !can_open_another_client(
        closed_now,
        roblox_running,
        event_blocks_next_client(
            named_event_exists(ROBLOX_SINGLETON_EVENT),
            singleton_reservation_held(),
        ),
    ) {
        // O Event existe e não deu para fechá-lo, ou quem segura o mutex é
        // outra coisa (RAM legado, outra ferramenta). Caminho antigo.
        return Ok(false);
    }

    // O mutex pode ter sido liberado nesse meio tempo; se não foi, seguimos
    // assim mesmo — sem o Event o cliente novo não desiste mais.
    let _ = acquire_multi_roblox_mutex()?;
    lock_roblox_cookies()?;
    Ok(true)
}

/// Com o mutex nas mãos de outro processo: dá para abrir mais um cliente?
///
/// Sim se algum `ROBLOX_singletonEvent` foi fechado agora — **ou** se há
/// cliente aberto e o nome nem existe mais: ele já foi fechado numa leva
/// anterior, e os clientes que o recriaram saíram. Sem o Event o próximo
/// cliente não tem instância antiga para sinalizar e sobe normal; recusar aí
/// era barrar um launch que funcionaria (relato do dono, 28/09/2026: depois de
/// fechar as alts de uma leva, todo launch dava "A Roblox client is already
/// running").
///
/// Sem cliente Roblox aberto nada muda: quem segura o mutex é outra coisa (RAM
/// legado, outra ferramenta) e o chamador segue o caminho antigo.
fn can_open_another_client(closed_now: usize, roblox_running: bool, event_exists: bool) -> bool {
    closed_now > 0 || (roblox_running && !event_exists)
}

/// O nome do Event "existe" também quando é a **nossa** reserva (um Mutex com o
/// mesmo nome: `OpenEventW` falha com tipo errado, não com "não encontrado").
/// Essa não barra o próximo cliente — é justamente o que impede o Event.
fn event_blocks_next_client(name_exists: bool, reserved_by_us: bool) -> bool {
    name_exists && !reserved_by_us
}

/// O que fazer com a reserva do nome `ROBLOX_singletonEvent` agora.
#[derive(Debug, PartialEq, Eq)]
enum ReservationStep {
    /// Ligada e já reservada.
    Keep,
    /// Ligada, sem reserva e o nome livre: criar o Mutex antes que um cliente
    /// crie o Event.
    Create,
    /// Ligada, mas um cliente ainda segura o Event (não deu para fechar): não
    /// há como reservar agora; o método atual segue valendo.
    Wait,
    /// Desligada com reserva feita: soltar o nome.
    Release,
    Nothing,
}

fn reservation_step(enabled: bool, held: bool, event_exists_now: bool) -> ReservationStep {
    match (enabled, held) {
        (true, true) => ReservationStep::Keep,
        (true, false) if !event_exists_now => ReservationStep::Create,
        (true, false) => ReservationStep::Wait,
        (false, true) => ReservationStep::Release,
        (false, false) => ReservationStep::Nothing,
    }
}

/// Liga/desliga a reserva experimental. Desligar solta o nome na hora.
pub fn set_singleton_reservation_enabled(enabled: bool) {
    RESERVE_SINGLETON_EVENT.store(enabled, Ordering::SeqCst);
    if !enabled {
        release_singleton_reservation();
    }
}

pub fn singleton_reservation_enabled() -> bool {
    RESERVE_SINGLETON_EVENT.load(Ordering::SeqCst)
}

pub fn singleton_reservation_held() -> bool {
    SINGLETON_RESERVATION
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

/// Executa o passo de [`reservation_step`]. Nunca fecha cliente nenhum: no
/// máximo cria ou fecha um handle **nosso**.
fn apply_singleton_reservation() {
    let enabled = singleton_reservation_enabled();
    let held = singleton_reservation_held();
    let event_exists_now = enabled && !held && named_event_exists(ROBLOX_SINGLETON_EVENT);
    match reservation_step(enabled, held, event_exists_now) {
        ReservationStep::Create => {
            let _ = reserve_named_object(ROBLOX_SINGLETON_EVENT);
        }
        ReservationStep::Release => release_singleton_reservation(),
        ReservationStep::Keep | ReservationStep::Wait | ReservationStep::Nothing => {}
    }
}

/// Cria um Mutex (sem dono) com `name` e guarda o handle. `false` se o nome já
/// é de outro tipo de objeto (um Event de cliente) ou a API falhou.
fn reserve_named_object(name: &str) -> bool {
    let Ok(mut slot) = SINGLETON_RESERVATION.lock() else {
        return false;
    };
    if slot.is_some() {
        return true;
    }
    let wide = encode_wide(name);
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    *slot = Some(SendHandle(handle));
    eprintln!("Multi Roblox (experimental): {} reservado", name);
    true
}

fn release_singleton_reservation() {
    if let Ok(mut slot) = SINGLETON_RESERVATION.lock() {
        if let Some(SendHandle(h)) = slot.take() {
            unsafe { CloseHandle(h) };
        }
    }
}

fn release_multi_roblox_mutex() {
    if let Ok(mut handle) = MULTI_ROBLOX_HANDLE.lock() {
        if let Some(release) = handle.take() {
            let _ = release.send(());
        }
    }
}

pub fn release_multi_roblox_handle() {
    release_multi_roblox_mutex();
    if let Ok(mut cookie_handle) = COOKIES_LOCK_HANDLE.lock() {
        if let Some(SendHandle(h)) = cookie_handle.take() {
            unsafe {
                CloseHandle(h);
            }
        }
    }
}

pub fn this_process_holds_multi_roblox() -> bool {
    MULTI_ROBLOX_HANDLE
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

pub fn disable_multi_roblox() -> Result<(), String> {
    release_multi_roblox_mutex();
    // Multi Roblox desligado (ou o app fechando): a reserva experimental sai
    // junto. Só fecha o handle nosso; nenhum cliente é tocado.
    release_singleton_reservation();

    lock_roblox_cookies()?;
    Ok(())
}

fn lock_roblox_cookies() -> Result<(), String> {
    let mut handle = COOKIES_LOCK_HANDLE.lock().map_err(|e| e.to_string())?;
    if handle.is_some() || is_773_fix_disabled() {
        return Ok(());
    }

    let Some(path) = get_roblox_cookies_path() else {
        return Ok(());
    };

    let wide = encode_wide(path.as_os_str());
    unsafe {
        let cookie_handle = CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if cookie_handle == INVALID_HANDLE_VALUE {
            eprintln!("Warning: Could not lock RobloxCookies.dat for the 773 fix");
            return Ok(());
        }
        *handle = Some(SendHandle(cookie_handle));
    }

    Ok(())
}

fn get_roblox_cookies_path() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let path = PathBuf::from(local_app_data)
        .join("Roblox")
        .join("LocalStorage")
        .join("RobloxCookies.dat");
    path.exists().then_some(path)
}

fn is_773_fix_disabled() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("no773fix.txt")))
        .map(|path| path.exists())
        .unwrap_or(false)
}

pub fn get_roblox_path() -> Result<String, String> {
    // The registry handler can point at an account test-channel build (see
    // `launch_url`); prefer the production build once it has been resolved.
    if let Some(dir) = cached_production_player_dir() {
        return Ok(dir);
    }

    unsafe {
        let key_name = encode_wide("roblox\\DefaultIcon");
        let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();

        if RegOpenKeyExW(HKEY_CLASSES_ROOT, key_name.as_ptr(), 0, KEY_READ, &mut hkey) == 0 {
            let mut buf = [0u16; 512];
            let mut buf_size = (buf.len() * 2) as u32;
            let mut value_type = 0u32;

            let result = RegQueryValueExW(
                hkey,
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut value_type,
                buf.as_mut_ptr() as *mut u8,
                &mut buf_size,
            );

            RegCloseKey(hkey);

            if result == 0 && value_type == REG_SZ {
                let len = (buf_size as usize / 2).saturating_sub(1);
                let path = String::from_utf16_lossy(&buf[..len]);
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    if parent.exists() {
                        return Ok(parent.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
    for dir in candidate_versions_dirs(&local_app_data) {
        if let Some((_, path)) = scan_versions_dir(&dir) {
            return Ok(path);
        }
    }

    Err("Roblox installation not found".into())
}

/// Varre uma pasta `Versions` (do Roblox oficial ou de um bootstrapper de
/// terceiros) e devolve a instalação válida mais recente nela, se houver.
/// Toca disco — sem teste direto; o que é testado é a lista de pastas que
/// alimenta esta função (`candidate_versions_dirs`).
fn scan_versions_dir(dir: &std::path::Path) -> Option<(SystemTime, String)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(SystemTime, String)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("version-") && entry.path().join("RobloxPlayerBeta.exe").exists() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if best.as_ref().map_or(true, |(t, _)| modified > *t) {
                        best = Some((modified, entry.path().to_string_lossy().into_owned()));
                    }
                }
            }
        }
    }
    best
}

/// Pastas `Versions` candidatas para achar uma instalação do Roblox, na ordem
/// em que `get_roblox_path` deve procurar. `Roblox` vem sempre primeiro — a
/// instalação oficial ganha de qualquer bootstrapper de terceiros presente na
/// mesma máquina — e só depois os bootstrappers mais usados (Bloxstrap,
/// Fishstrap, Voidstrap), que instalam o `RobloxPlayerBeta.exe` na própria
/// pasta em vez de `%LOCALAPPDATA%\Roblox`.
fn candidate_versions_dirs(local_app_data: &str) -> Vec<PathBuf> {
    let local = PathBuf::from(local_app_data);
    ["Roblox", "Bloxstrap", "Fishstrap", "Voidstrap"]
        .iter()
        .map(|root| local.join(root).join("Versions"))
        .collect()
}

fn get_client_settings_file() -> Result<PathBuf, String> {
    get_client_settings_file_in(&get_roblox_path()?)
}

/// Parte pura de `get_client_settings_file`: monta o caminho do
/// `ClientAppSettings.json` dentro de uma pasta base qualquer, em vez de sempre
/// resolver a build de produção. É o que deixa `patch_client_settings_for_launch`
/// (`commands/launch_shared.rs`) escrever no `ClientSettings` da versão que a
/// conta vai de fato abrir — `None` continua significando "a build padrão"
/// (é o caso do servidor HTTP local, que não tem conta no contexto).
fn get_client_settings_file_in(base_path: &str) -> Result<PathBuf, String> {
    let settings_dir = std::path::Path::new(base_path).join("ClientSettings");

    if !settings_dir.exists() {
        std::fs::create_dir_all(&settings_dir)
            .map_err(|e| format!("Failed to create ClientSettings: {}", e))?;
    }

    Ok(settings_dir.join("ClientAppSettings.json"))
}

/// Versão do Windows para o resumo do "Reportar problema" (ideia 28), lida do
/// registro (`HKLM\...\Windows NT\CurrentVersion`) — a mesma leitura que o
/// isolamento já faz, sem API nova no binário.
pub fn os_version_label() -> String {
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let read = |name: &str| read_string_value(HKEY_LOCAL_MACHINE as _, KEY, name);
    format_windows_version(
        read("ProductName").as_deref(),
        read("DisplayVersion").as_deref(),
        read("CurrentBuild").as_deref(),
    )
}

/// Monta "Windows 11 Home (24H2, build 26200)". O `ProductName` do registro diz
/// "Windows 10" também no 11; a build (22000 ou mais) é o que separa os dois.
fn format_windows_version(product: Option<&str>, display: Option<&str>, build: Option<&str>) -> String {
    let build_number = build.and_then(|b| b.trim().parse::<u32>().ok());
    let mut name = product
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("Windows")
        .to_string();
    if build_number.is_some_and(|b| b >= 22_000) && name.starts_with("Windows 10") {
        name = name.replacen("Windows 10", "Windows 11", 1);
    }
    let details: Vec<String> = [
        display.map(str::trim).filter(|d| !d.is_empty()).map(str::to_string),
        build_number.map(|b| format!("build {b}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if details.is_empty() {
        name
    } else {
        format!("{name} ({})", details.join(", "))
    }
}

#[cfg(test)]
mod os_version_label_tests {
    use super::*;

    #[test]
    fn windows_11_is_named_by_its_build_even_when_the_registry_says_10() {
        assert_eq!(
            format_windows_version(Some("Windows 10 Home Single Language"), Some("24H2"), Some("26200")),
            "Windows 11 Home Single Language (24H2, build 26200)"
        );
    }

    #[test]
    fn windows_10_stays_10() {
        assert_eq!(
            format_windows_version(Some("Windows 10 Pro"), Some("22H2"), Some("19045")),
            "Windows 10 Pro (22H2, build 19045)"
        );
    }

    #[test]
    fn missing_values_still_give_a_readable_label() {
        assert_eq!(format_windows_version(None, None, None), "Windows");
        assert_eq!(format_windows_version(Some(" "), None, Some("x")), "Windows");
        assert_eq!(format_windows_version(None, None, Some("22631")), "Windows (build 22631)");
    }

    #[test]
    fn the_label_of_this_machine_starts_with_windows() {
        assert!(os_version_label().starts_with("Windows"));
    }
}

#[cfg(test)]
mod client_settings_file_path_tests {
    use super::*;

    // Pasta temporária própria (sem dependência de `tempfile`), no mesmo
    // padrão usado pelos outros módulos de teste deste arquivo/crate. Nunca
    // chama `get_client_settings_file()`: essa resolve a instalação real do
    // Roblox e criaria pastas no disco do usuário.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("ram4-clientsetfile-{}-{}-{}", tag, nanos, n));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn get_client_settings_file_in_builds_the_path_inside_the_given_base() {
        let temp = TempDir::new("base");
        let base = temp.0.to_string_lossy().into_owned();

        let path = get_client_settings_file_in(&base).expect("path");

        assert_eq!(
            path,
            std::path::Path::new(&base)
                .join("ClientSettings")
                .join("ClientAppSettings.json")
        );
    }

    #[test]
    fn get_client_settings_file_in_gives_a_different_path_for_a_different_base() {
        let temp_a = TempDir::new("base-a");
        let temp_b = TempDir::new("base-b");
        let base_a = temp_a.0.to_string_lossy().into_owned();
        let base_b = temp_b.0.to_string_lossy().into_owned();

        let path_a = get_client_settings_file_in(&base_a).expect("path a");
        let path_b = get_client_settings_file_in(&base_b).expect("path b");

        assert_ne!(path_a, path_b);
    }
}

/// Pastas onde o launcher oficial e os bootstrappers de terceiros mais usados
/// guardam suas builds instaladas do Roblox. Bloxstrap, Fishstrap e Voidstrap
/// instalam o `RobloxPlayerBeta.exe` na própria pasta em vez de
/// `%LOCALAPPDATA%\Roblox`, então sem isso o app não os enxerga e o usuário
/// que só joga por um deles "não tem Roblox instalado".
///
/// A ordem importa: a instalação oficial (`Roblox`) vem sempre primeiro —
/// `get_roblox_path` para no primeiro diretório que tiver uma versão válida,
/// então a oficial ganha de qualquer bootstrapper de terceiros presente na
/// mesma máquina.
#[cfg(test)]
mod roblox_install_candidates_tests {
    use super::*;

    #[test]
    fn candidate_versions_dirs_puts_the_official_install_first() {
        let dirs = candidate_versions_dirs(r"C:\Users\alguem\AppData\Local");
        let names: Vec<String> = dirs
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                r"C:\Users\alguem\AppData\Local\Roblox\Versions",
                r"C:\Users\alguem\AppData\Local\Bloxstrap\Versions",
                r"C:\Users\alguem\AppData\Local\Fishstrap\Versions",
                r"C:\Users\alguem\AppData\Local\Voidstrap\Versions",
            ]
        );
    }

    #[test]
    fn candidate_versions_dirs_joins_versions_under_each_launcher_root() {
        let dirs = candidate_versions_dirs(r"D:\Local");
        assert_eq!(dirs.len(), 4);
        for dir in &dirs {
            assert!(dir.ends_with("Versions"));
            assert!(dir.starts_with(r"D:\Local"));
        }
    }

    #[test]
    fn candidate_versions_dirs_is_stable_for_an_empty_base() {
        // Nunca deveria acontecer (LOCALAPPDATA vazio), mas a função é pura e
        // não deve entrar em pânico nem mudar a ordem.
        let dirs = candidate_versions_dirs("");
        assert_eq!(dirs.len(), 4);
        assert_eq!(dirs[0], PathBuf::from("Roblox").join("Versions"));
        assert_eq!(dirs[3], PathBuf::from("Voidstrap").join("Versions"));
    }
}

#[cfg(test)]
mod browser_tracker_tests {
    use super::*;

    #[test]
    fn generate_browser_tracker_id_is_digits_only_and_of_the_expected_length() {
        for _ in 0..50 {
            let id = generate_browser_tracker_id();
            assert!(
                id.chars().all(|c| c.is_ascii_digit()),
                "expected digits only, got {id}"
            );
            assert!(
                (11..=13).contains(&id.len()),
                "unexpected length {} for {id}",
                id.len()
            );
            // Both halves are built with a +100000 floor, so neither can be
            // shorter than six digits nor start with a zero.
            assert!(!id.starts_with('0'), "unexpected leading zero in {id}");
        }
    }

    #[test]
    fn generate_browser_tracker_id_varies_between_calls() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..25 {
            seen.insert(generate_browser_tracker_id());
        }
        assert!(seen.len() > 1, "tracker ids should not be constant");
    }
}

#[cfg(test)]
mod singleton_reservation_tests {
    use super::*;

    #[test]
    fn with_the_option_off_nothing_is_reserved() {
        assert_eq!(reservation_step(false, false, false), ReservationStep::Nothing);
        assert_eq!(reservation_step(false, false, true), ReservationStep::Nothing);
    }

    #[test]
    fn on_and_free_creates_on_and_taken_waits() {
        assert_eq!(reservation_step(true, false, false), ReservationStep::Create);
        // Um cliente ainda segura o Event (não deu para fechar): espera.
        assert_eq!(reservation_step(true, false, true), ReservationStep::Wait);
        assert_eq!(reservation_step(true, true, true), ReservationStep::Keep);
        assert_eq!(reservation_step(true, true, false), ReservationStep::Keep);
    }

    #[test]
    fn turning_it_off_releases_the_name() {
        assert_eq!(reservation_step(false, true, false), ReservationStep::Release);
        assert_eq!(reservation_step(false, true, true), ReservationStep::Release);
    }

    #[test]
    fn our_own_reservation_does_not_block_the_next_client() {
        assert!(event_blocks_next_client(true, false));
        assert!(!event_blocks_next_client(true, true));
        assert!(!event_blocks_next_client(false, false));
        assert!(!event_blocks_next_client(false, true));
    }

    /// Com o nome reservado por um Mutex, ninguém consegue criar um Event com
    /// ele — é o que faz o cliente não achar a "instância antiga". Usa um nome
    /// de teste: o de verdade nunca é tocado aqui.
    #[test]
    fn a_reserved_name_cannot_be_created_as_an_event() {
        use windows_sys::Win32::System::Threading::CreateEventW;
        let name = format!("RAMTest_reserved_singletonEvent_{}", std::process::id());
        let wide = encode_wide(&name);
        let reservation = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        assert!(!reservation.is_null());

        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
        assert!(event.is_null(), "an Event was created over the reserved name");
        assert!(named_event_exists(&name), "the name reads as taken");

        unsafe { CloseHandle(reservation) };
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
        assert!(!event.is_null(), "after the release the name is free again");
        unsafe { CloseHandle(event) };
    }

    #[test]
    fn releasing_without_a_reservation_is_harmless() {
        release_singleton_reservation();
        assert!(!singleton_reservation_held());
    }

    /// O padrão do processo é desligado: só o launch, lendo
    /// `General.ReserveSingletonEvent`, liga.
    #[test]
    fn the_process_starts_with_the_reservation_off() {
        let source = include_str!("core.rs");
        assert!(source.contains("static RESERVE_SINGLETON_EVENT: AtomicBool = AtomicBool::new(false);"));
    }
}

#[cfg(test)]
mod multi_roblox_decision_tests {
    use super::*;

    /// Relato do dono (28/09/2026): a principal aberta pelo site, uma leva de
    /// alts entrou (o app fechou o `ROBLOX_singletonEvent` da principal) e,
    /// depois de ele fechar essas alts, todo launch passou a dar "A Roblox
    /// client is already running" — sem evento nenhum para fechar, o app
    /// entendia "não dá", embora o próximo cliente fosse subir normal.
    #[test]
    fn with_a_client_open_and_the_event_already_gone_another_client_can_open() {
        assert!(can_open_another_client(0, true, false));
    }

    #[test]
    fn closing_an_event_now_lets_another_client_open() {
        assert!(can_open_another_client(1, true, true));
        assert!(can_open_another_client(2, true, false));
    }

    /// O Event existe e não foi fechado (acesso negado, por exemplo): o próximo
    /// cliente sinalizaria o antigo e desistiria.
    #[test]
    fn an_event_that_could_not_be_closed_still_blocks() {
        assert!(!can_open_another_client(0, true, true));
    }

    /// Sem cliente Roblox aberto, quem segura o mutex é outra coisa (RAM
    /// legado, outra ferramenta): segue o caminho antigo, com a mensagem dele.
    #[test]
    fn without_a_roblox_client_the_old_path_decides() {
        assert!(!can_open_another_client(0, false, false));
        assert!(!can_open_another_client(0, false, true));
    }
}
