pub fn get_account_data_path() -> PathBuf {
    crate::data::settings::get_runtime_data_dir().join("AccountData.json")
}

#[tauri::command]
pub fn get_accounts(state: tauri::State<'_, AccountStore>) -> Result<Vec<Account>, String> {
    state.get_all()
}

#[tauri::command]
pub fn save_accounts(state: tauri::State<'_, AccountStore>) -> Result<(), String> {
    state.save()
}

#[tauri::command]
pub fn add_account(
    state: tauri::State<'_, AccountStore>,
    security_token: String,
    username: String,
    user_id: i64,
    password: Option<String>,
) -> Result<(), String> {
    let mut account = Account::new(security_token, username, user_id);
    if let Some(password) = password {
        account.password = password;
    }
    state.add(account)
}

#[tauri::command]
pub fn remove_account(state: tauri::State<'_, AccountStore>, user_id: i64) -> Result<bool, String> {
    state.remove(user_id)
}

#[tauri::command]
pub fn update_account(
    state: tauri::State<'_, AccountStore>,
    mut account: Account,
) -> Result<bool, String> {
    // The webview holds a snapshot that goes stale whenever the backend
    // rotates a cookie (session refresh, webserver SetField...). Editing an
    // alias/group from that snapshot must not write the old, invalidated
    // cookie back, so credentials always come from the store.
    if let Some(stored) = state
        .get_all()?
        .into_iter()
        .find(|a| a.user_id == account.user_id)
    {
        account.security_token = stored.security_token;
        account.password = stored.password;
    }
    state.update(account)
}

#[tauri::command]
pub fn unlock_accounts(
    state: tauri::State<'_, AccountStore>,
    password: String,
    remember_hours: Option<u64>,
) -> Result<(), String> {
    state.load_with_password(&password)?;

    // Só depois de destrancar: guardar uma senha que não abre nada seria pior
    // que não guardar nada.
    match remember_hours {
        Some(hours) if hours > 0 => {
            if let Err(e) = remember(&password, hours) {
                // O unlock valeu; o lembrete é conveniência.
                eprintln!("Não foi possível lembrar a senha: {}", e);
            }
        }
        _ => forget(),
    }
    Ok(())
}

/// Destranca com a senha lembrada, se houver uma válida.
///
/// Devolve `false` quando não há lembrete — a UI mostra a tela de senha. Um
/// lembrete que não destranca mais (senha trocada por fora, arquivo de outra
/// instalação) é **apagado**, para não ficar tentando para sempre.
#[tauri::command]
pub fn try_remembered_unlock(state: tauri::State<'_, AccountStore>) -> Result<bool, String> {
    let Some(password) = remembered_password() else {
        return Ok(false);
    };
    match state.load_with_password(&password) {
        Ok(()) => Ok(true),
        Err(_) => {
            forget();
            Ok(false)
        }
    }
}

/// Estado da caixa "lembrar de mim" para a tela de senha.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RememberState {
    /// `false` fora do Windows: sem proteção do SO, a caixa não aparece.
    pub supported: bool,
    /// Há um lembrete guardado agora.
    pub active: bool,
    pub default_hours: u64,
}

#[tauri::command]
pub fn remembered_unlock_state() -> RememberState {
    RememberState {
        supported: can_remember(),
        active: can_remember() && remembered_unlock_path().exists(),
        default_hours: REMEMBER_DEFAULT_HOURS,
    }
}

#[tauri::command]
pub fn forget_remembered_unlock() -> Result<(), String> {
    forget();
    Ok(())
}

/// Para a UI, "encrypted" sempre quis dizer **"protegido por senha"** — é isso
/// que a tela de criptografia mostra e o que o usuário decide ali. Desde que o
/// vault sem senha também é cifrado (pela chave do aparelho), `is_encrypted()`
/// deixou de responder essa pergunta: ela é verdadeira nos dois casos. Então o
/// comando passou a devolver `has_user_password()`; trocar para os bytes do
/// arquivo faria a tela dizer "Pass Lock" para quem não tem senha nenhuma.
#[tauri::command]
pub fn is_accounts_encrypted(state: tauri::State<'_, AccountStore>) -> Result<bool, String> {
    state.has_user_password()
}

#[tauri::command]
pub fn needs_password(state: tauri::State<'_, AccountStore>) -> Result<bool, String> {
    state.needs_password()
}

/// Problema com o `AccountData.key` que o usuário precisa ver **hoje**.
///
/// `eprintln!` numa build GUI não vai a lugar nenhum, e um `.key` ilegível é
/// justamente o defeito que passa o dia inteiro invisível — a chave mestra está
/// em memória, tudo funciona — para virar lockout no boot seguinte. Este comando
/// é o que leva isso à tela.
#[tauri::command]
pub fn vault_key_warning(state: tauri::State<'_, AccountStore>) -> Option<VaultKeyWarning> {
    state.vault_key_warning()
}

/// Evento com cada mudança do aviso do `.key`. Payload: o aviso, ou `null`
/// quando ele sumiu. O frontend ouve pelo mesmo nome (`src/types.ts`).
pub const VAULT_KEY_WARNING_EVENT: &str = "vault-key-warning-changed";

/// Leva à janela, **na hora**, cada mudança do aviso do `.key`.
///
/// O comando `vault_key_warning` só responde quando a UI pergunta, e ela só
/// pergunta no boot, ao recarregar contas e quando trocar a criptografia falha.
/// Gravador de fundo não passa por nada disso: com o Auto Rejoin a noite inteira,
/// o `.key` que ficava ruim de madrugada virava aviso só no backend, e o dono
/// descobria no boot seguinte — o lockout que a faixa existe para evitar.
///
/// A thread é dela, e fala com o Tauri **fora** de qualquer lock do store: o
/// aviso muda dentro de `save_locked`, que roda segurando o lock de contas, e um
/// comando síncrono na thread principal (restaurar backup) pode estar esperando
/// justamente esse lock. O store só enfileira no canal, o que nunca bloqueia.
pub fn forward_vault_key_warning(app: &tauri::AppHandle, store: &AccountStore) {
    use tauri::Emitter;

    let changes = store.watch_key_warning();
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vault-key-warning".to_string())
        .spawn(move || {
            for warning in changes {
                let _ = app.emit(VAULT_KEY_WARNING_EVENT, warning);
            }
        });
    if let Err(e) = spawned {
        // Sem a thread a faixa volta a ser lida só quando a UI pergunta; o app
        // segue funcionando.
        eprintln!("Não foi possível acompanhar o aviso da chave do vault: {}", e);
    }
}

#[tauri::command]
pub fn set_encryption_password(
    state: tauri::State<'_, AccountStore>,
    password: Option<String>,
) -> Result<(), String> {
    state.set_password(password.as_deref())?;
    // A senha guardada não abre mais nada (ou não é mais necessária): guardá-la
    // só deixaria uma senha antiga em disco.
    forget();
    Ok(())
}

/// Tentativas erradas seguidas na tela "trancado por inatividade" (ideia 27)
/// e até quando a próxima fica recusada.
struct AppLockAttempts {
    failures: u32,
    blocked_until: Option<std::time::Instant>,
}

static APP_LOCK_ATTEMPTS: Mutex<AppLockAttempts> = Mutex::new(AppLockAttempts {
    failures: 0,
    blocked_until: None,
});

/// Espera depois de `failures` senhas erradas seguidas: as três primeiras são
/// livres (dedo errado), depois 5 s dobrando até 60 s.
fn app_lock_retry_delay_secs(failures: u32) -> u64 {
    if failures < 3 {
        return 0;
    }
    let doublings = (failures - 3).min(4);
    (5u64 << doublings).min(60)
}

/// Destranca a tela de inatividade. **Só confere a senha** — não relê as
/// contas, não troca a sessão, não para nada que esteja rodando.
#[tauri::command]
pub fn verify_app_password(state: tauri::State<'_, AccountStore>, password: String) -> Result<(), String> {
    check_app_password(&APP_LOCK_ATTEMPTS, std::time::Instant::now(), |p| state.verify_password(p), &password)
}

fn check_app_password(
    attempts: &Mutex<AppLockAttempts>,
    now: std::time::Instant,
    verify: impl Fn(&str) -> Result<bool, String>,
    password: &str,
) -> Result<(), String> {
    let mut slot = attempts.lock().map_err(|e| e.to_string())?;
    if let Some(until) = slot.blocked_until {
        if now < until {
            let wait = until.duration_since(now).as_secs().max(1);
            return Err(format!("Too many wrong passwords. Wait {wait} seconds and try again."));
        }
    }
    if verify(password)? {
        slot.failures = 0;
        slot.blocked_until = None;
        return Ok(());
    }
    slot.failures = slot.failures.saturating_add(1);
    let delay = app_lock_retry_delay_secs(slot.failures);
    slot.blocked_until = (delay > 0).then(|| now + std::time::Duration::from_secs(delay));
    Err("Wrong password.".to_string())
}

#[cfg(test)]
mod app_lock_command_tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fresh() -> Mutex<AppLockAttempts> {
        Mutex::new(AppLockAttempts { failures: 0, blocked_until: None })
    }

    #[test]
    fn the_wait_grows_after_three_wrong_passwords_and_has_a_ceiling() {
        assert_eq!(app_lock_retry_delay_secs(0), 0);
        assert_eq!(app_lock_retry_delay_secs(2), 0);
        assert_eq!(app_lock_retry_delay_secs(3), 5);
        assert_eq!(app_lock_retry_delay_secs(4), 10);
        assert_eq!(app_lock_retry_delay_secs(5), 20);
        assert_eq!(app_lock_retry_delay_secs(7), 60);
        assert_eq!(app_lock_retry_delay_secs(500), 60);
    }

    #[test]
    fn a_wrong_password_is_refused_and_the_right_one_unlocks() {
        let attempts = fresh();
        let verify = |p: &str| Ok(p == "certa");
        let now = Instant::now();
        assert_eq!(check_app_password(&attempts, now, verify, "errada"), Err("Wrong password.".into()));
        assert_eq!(check_app_password(&attempts, now, verify, "certa"), Ok(()));
        assert_eq!(attempts.lock().unwrap().failures, 0, "success resets the count");
    }

    #[test]
    fn while_waiting_even_the_right_password_is_refused() {
        let attempts = fresh();
        let verify = |p: &str| Ok(p == "certa");
        let now = Instant::now();
        for _ in 0..3 {
            let _ = check_app_password(&attempts, now, verify, "errada");
        }
        let err = check_app_password(&attempts, now + Duration::from_secs(1), verify, "certa").unwrap_err();
        assert!(err.contains("Wait"), "{err}");
        assert_eq!(
            check_app_password(&attempts, now + Duration::from_secs(6), verify, "certa"),
            Ok(())
        );
    }

    #[test]
    fn a_vault_without_a_password_reports_the_error() {
        let attempts = fresh();
        let err = check_app_password(&attempts, Instant::now(), |_| Err("No app password is set.".into()), "x");
        assert_eq!(err, Err("No app password is set.".into()));
    }
}

#[tauri::command]
pub fn reorder_accounts(
    state: tauri::State<'_, AccountStore>,
    user_ids: Vec<i64>,
) -> Result<(), String> {
    state.reorder(&user_ids)
}

#[tauri::command]
pub fn import_old_account_data(
    state: tauri::State<'_, AccountStore>,
    file_data: Vec<u8>,
    password: Option<String>,
) -> Result<OldAccountImportSummary, String> {
    state.import_old_account_data(&file_data, password.as_deref())
}

#[cfg(test)]
mod vault_key_warning_event_tests {
    /// **A2 do checkup.** O canal do store (`watch_key_warning`) não vale nada se
    /// ninguém o ligar à janela. O `setup` do Tauri é o lugar que tem um
    /// `AppHandle` antes de qualquer gravação de fundo começar (Auto Rejoin,
    /// Watcher e servidor HTTP só existem depois dele).
    #[test]
    fn the_app_setup_forwards_every_warning_change_to_the_window() {
        let lib = include_str!("../../lib.rs");
        let setup = lib
            .split(".setup(|app|")
            .nth(1)
            .expect("lib.rs sem o .setup(|app| ...) do Tauri");
        assert!(
            setup.contains("forward_vault_key_warning("),
            "o aviso de uma gravação de fundo não chega à janela: o setup não liga o canal"
        );
    }
}

#[cfg(test)]
mod account_path_tests {
    use super::*;

    #[test]
    fn get_account_data_path_lives_in_the_runtime_data_dir() {
        // Deixou de ser "ao lado do exe": os dados agora ficam no perfil do
        // usuário, para o executável poder ser movido de pasta.
        let path = get_account_data_path();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("AccountData.json")
        );
        assert!(path.is_absolute(), "{}", path.display());
        assert_eq!(
            path.parent(),
            Some(crate::data::settings::get_runtime_data_dir().as_path())
        );
        assert_eq!(get_account_data_path(), path, "the path is stable");
    }
}
