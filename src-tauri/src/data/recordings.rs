//! Gravações: sequências de passos (tecla, clique num ponto relativo da janela,
//! espera) que o app toca na janela do Roblox de uma conta, uma janela por vez.
//! Ver docs/features/recordings.md.
//!
//! O arquivo `RAMRecordings.json`, na pasta de dados, é a única cópia (está em
//! `DATA_FILES`, então backup, restauração e migração o levam). Mesmas garantias
//! do `RAMLaunchPresets.json`:
//!
//! - gravação atômica (temporário + troca), com o conteúdo anterior em
//!   `RAMRecordings.json.bak`;
//! - arquivo ilegível **trava** a gravação em vez de ser sobrescrito;
//! - o store **não guarda nada em memória**: cada leitura vai ao disco, então
//!   restaurar um backup vale na hora e o ciclo do Modo AFK sempre toca o que
//!   está salvo.
//!
//! Aqui mora também a parte pura que não depende de janela: a lista fechada de
//! teclas, a validação dos passos e qual gravação vale para cada conta.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const RECORDINGS_FILE_NAME: &str = "RAMRecordings.json";
const MAX_RECORDINGS_FILE_BYTES: u64 = 4 * 1024 * 1024;
const RECORDINGS_FILE_VERSION: u32 = 1;
pub const MAX_RECORDINGS: usize = 100;
pub const MAX_RECORDING_STEPS: usize = 500;
pub const MAX_RECORDING_NAME_CHARS: usize = 60;
/// Uma espera sozinha: até 10 minutos.
pub const MAX_WAIT_MS: u64 = 600_000;
/// Toque de tecla: o mesmo piso do Modo AFK é 40 ms; abaixo de 10 ms alguns
/// jogos não registram a tecla.
pub const MIN_HOLD_MS: u64 = 10;
pub const MAX_HOLD_MS: u64 = 10_000;
pub const DEFAULT_HOLD_MS: u64 = 40;
/// A gravação inteira, estimada: até 10 minutos. O foco fica fora da janela do
/// usuário enquanto ela toca, e uma gravação sem fim prenderia o PC.
pub const MAX_RECORDING_TOTAL_MS: u64 = 600_000;
/// Quanto um clique custa na estimativa (a receita do clique do Modo AFK leva
/// ~0,55 s sem o clique de foco e ~0,8 s com ele).
pub const CLICK_ESTIMATE_MS: u64 = 800;

/// Lista **fechada** de teclas que uma gravação pode tocar: `(nome, virtual key,
/// tecla estendida)`. Começa com as 14 do Modo AFK, na mesma ordem, e acrescenta
/// com cuidado o que um percurso no jogo pede — Shift, setas, as outras letras e
/// os outros números.
///
/// Ficam fora, de propósito, as mesmas do Modo AFK e pelo mesmo motivo: Enter (abre
/// o chat), Tab, Escape (menu do Roblox; Esc + L sai do jogo, Esc + R reseta o
/// personagem), F-keys (F4 com Alt fecha o cliente, F9 abre o console), Alt,
/// Ctrl, Windows, Backspace/Delete, `/` (chat) e `` ` ``.
pub const RECORDING_KEYS: &[(&str, u16, bool)] = &[
    ("Space", 0x20, false),
    ("W", 0x57, false),
    ("A", 0x41, false),
    ("S", 0x53, false),
    ("D", 0x44, false),
    ("E", 0x45, false),
    ("F", 0x46, false),
    ("R", 0x52, false),
    ("Q", 0x51, false),
    ("1", 0x31, false),
    ("2", 0x32, false),
    ("3", 0x33, false),
    ("4", 0x34, false),
    ("5", 0x35, false),
    // Shift esquerdo: correr ou o shift lock, conforme o jogo.
    ("Shift", 0xA0, false),
    // Setas: teclas estendidas (o scan code sozinho seria o do teclado numérico).
    ("Up", 0x26, true),
    ("Down", 0x28, true),
    ("Left", 0x25, true),
    ("Right", 0x27, true),
    ("B", 0x42, false),
    ("C", 0x43, false),
    ("G", 0x47, false),
    ("H", 0x48, false),
    ("I", 0x49, false),
    ("J", 0x4A, false),
    ("K", 0x4B, false),
    ("L", 0x4C, false),
    ("M", 0x4D, false),
    ("N", 0x4E, false),
    ("O", 0x4F, false),
    ("P", 0x50, false),
    ("T", 0x54, false),
    ("U", 0x55, false),
    ("V", 0x56, false),
    ("X", 0x58, false),
    ("Y", 0x59, false),
    ("Z", 0x5A, false),
    ("6", 0x36, false),
    ("7", 0x37, false),
    ("8", 0x38, false),
    ("9", 0x39, false),
    ("0", 0x30, false),
];

/// Os nomes da lista, na ordem em que a tela os oferece.
pub fn recording_key_names() -> Vec<String> {
    RECORDING_KEYS.iter().map(|(name, _, _)| (*name).to_string()).collect()
}

/// `(virtual key, estendida)` de uma tecla da lista, ou `None` para qualquer outro
/// nome — é este `None` que impede tecla de fora de chegar ao envio.
pub fn recording_key(name: &str) -> Option<(u16, bool)> {
    RECORDING_KEYS
        .iter()
        .find(|(key, _, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, vk, extended)| (*vk, *extended))
}

/// O nome canônico (a grafia da lista) de uma tecla.
fn canonical_key_name(name: &str) -> Option<&'static str> {
    RECORDING_KEYS
        .iter()
        .find(|(key, _, _)| key.eq_ignore_ascii_case(name))
        .map(|(key, _, _)| *key)
}

fn default_hold_ms() -> u64 {
    DEFAULT_HOLD_MS
}

/// Um passo da gravação. O JSON usa `type` para dizer qual é:
///
/// - `key` — toca a tecla e a segura `holdMs` (toque ou segurar);
/// - `keyDown` / `keyUp` — aperta / solta, para combinar teclas (segurar W
///   enquanto pula). Tecla que ficar apertada no fim é solta pelo app;
/// - `click` — clique esquerdo no ponto `xPct` × `yPct` (0–100) da área interna
///   da janela, o mesmo ponto relativo do Modo AFK;
/// - `wait` — espera `ms` milissegundos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RecordingStep {
    #[serde(rename_all = "camelCase")]
    Key {
        key: String,
        #[serde(default = "default_hold_ms")]
        hold_ms: u64,
    },
    #[serde(rename_all = "camelCase")]
    KeyDown { key: String },
    #[serde(rename_all = "camelCase")]
    KeyUp { key: String },
    #[serde(rename_all = "camelCase")]
    Click { x_pct: f64, y_pct: f64 },
    #[serde(rename_all = "camelCase")]
    Wait { ms: u64 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub steps: Vec<RecordingStep>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

fn current_file_version() -> u32 {
    RECORDINGS_FILE_VERSION
}

/// O arquivo inteiro: a biblioteca e qual gravação vale para quem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingsFile {
    #[serde(default = "current_file_version")]
    pub version: u32,
    #[serde(default)]
    pub recordings: Vec<Recording>,
    /// A gravação de todas as contas (`None` = nenhuma).
    #[serde(default)]
    pub default_id: Option<String>,
    /// Gravação própria de uma conta, que vence a de todas.
    #[serde(default)]
    pub account_ids: HashMap<i64, String>,
}

impl Default for RecordingsFile {
    fn default() -> Self {
        Self {
            version: RECORDINGS_FILE_VERSION,
            recordings: Vec::new(),
            default_id: None,
            account_ids: HashMap::new(),
        }
    }
}

fn clamp_percent(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        50.0
    }
}

/// Duração estimada da gravação tocando (esperas + teclas seguradas + cliques).
pub fn recording_duration_ms(steps: &[RecordingStep]) -> u64 {
    steps.iter().fold(0u64, |total, step| {
        total.saturating_add(match step {
            RecordingStep::Key { hold_ms, .. } => *hold_ms,
            RecordingStep::Wait { ms } => *ms,
            RecordingStep::Click { .. } => CLICK_ESTIMATE_MS,
            RecordingStep::KeyDown { .. } | RecordingStep::KeyUp { .. } => 0,
        })
    })
}

/// Confere e normaliza um passo: tecla da lista (na grafia dela), tempos e
/// porcentagens dentro dos limites.
pub fn normalize_step(step: RecordingStep) -> Result<RecordingStep, String> {
    let key_of = |key: &str| {
        canonical_key_name(key)
            .map(str::to_string)
            .ok_or_else(|| format!("Key not allowed in a recording: {key}"))
    };
    Ok(match step {
        RecordingStep::Key { key, hold_ms } => RecordingStep::Key {
            key: key_of(&key)?,
            hold_ms: hold_ms.clamp(MIN_HOLD_MS, MAX_HOLD_MS),
        },
        RecordingStep::KeyDown { key } => RecordingStep::KeyDown { key: key_of(&key)? },
        RecordingStep::KeyUp { key } => RecordingStep::KeyUp { key: key_of(&key)? },
        RecordingStep::Click { x_pct, y_pct } => RecordingStep::Click {
            x_pct: clamp_percent(x_pct),
            y_pct: clamp_percent(y_pct),
        },
        RecordingStep::Wait { ms } => RecordingStep::Wait {
            ms: ms.min(MAX_WAIT_MS),
        },
    })
}

/// Confere e normaliza uma gravação antes de gravar. Devolve a frase (em
/// inglês; a tela valida antes com as frases dela) do primeiro problema.
pub fn normalize_recording(mut recording: Recording) -> Result<Recording, String> {
    let name = recording.name.trim().to_string();
    if name.is_empty() {
        return Err("Give the recording a name.".into());
    }
    if name.chars().count() > MAX_RECORDING_NAME_CHARS {
        return Err(format!(
            "The name is too long (max {MAX_RECORDING_NAME_CHARS} characters)."
        ));
    }
    recording.name = name;
    if recording.steps.len() > MAX_RECORDING_STEPS {
        return Err(format!("A recording holds up to {MAX_RECORDING_STEPS} steps."));
    }
    recording.steps = recording
        .steps
        .into_iter()
        .map(normalize_step)
        .collect::<Result<Vec<_>, _>>()?;
    if recording_duration_ms(&recording.steps) > MAX_RECORDING_TOTAL_MS {
        return Err("A recording can take up to 10 minutes.".into());
    }
    Ok(recording)
}

/// A gravação que vale para a conta: a própria, se ela tem e a gravação existe;
/// senão a de todas as contas.
pub fn recording_for_account(file: &RecordingsFile, user_id: i64) -> Option<&Recording> {
    let find = |id: &str| file.recordings.iter().find(|r| r.id == id);
    file.account_ids
        .get(&user_id)
        .and_then(|id| find(id))
        .or_else(|| file.default_id.as_deref().and_then(find))
}

// ── o store ────────────────────────────────────────────────────────────────

pub struct RecordingStore {
    file_path: PathBuf,
    lock: Mutex<()>,
}

fn sibling(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn new_id(file: &RecordingsFile, now_ms: i64) -> String {
    let mut n = 0u32;
    let mut id = format!("rec-{now_ms}");
    while file.recordings.iter().any(|r| r.id == id) {
        n += 1;
        id = format!("rec-{now_ms}-{n}");
    }
    id
}

impl RecordingStore {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            lock: Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn backup_file_path(&self) -> PathBuf {
        sibling(&self.file_path, ".bak")
    }

    /// Arquivo ausente ou vazio = sem gravações. Ilegível = `Err` (nunca "lista
    /// vazia", senão a próxima gravação apagaria o que ele tem).
    fn read_from_disk(&self) -> Result<Option<RecordingsFile>, String> {
        let metadata = match fs::metadata(&self.file_path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("Failed to read the recordings file: {e}")),
        };
        if metadata.len() > MAX_RECORDINGS_FILE_BYTES {
            return Err(format!(
                "The recordings file is too large (max {MAX_RECORDINGS_FILE_BYTES} bytes)"
            ));
        }
        let data = fs::read(&self.file_path)
            .map_err(|e| format!("Failed to read the recordings file: {e}"))?;
        if data.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(None);
        }
        serde_json::from_slice::<RecordingsFile>(&data)
            .map(Some)
            .map_err(|e| format!("Failed to parse the recordings file: {e}"))
    }

    /// A biblioteca e as escolhas, como estão no disco.
    pub fn load(&self) -> Result<RecordingsFile, String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        Ok(self.read_from_disk()?.unwrap_or_default())
    }

    fn write(&self, file: &RecordingsFile, existed: bool) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(file)
            .map_err(|e| format!("Failed to serialize the recordings: {e}"))?;
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create the data folder: {e}"))?;
        }
        if existed {
            fs::copy(&self.file_path, sibling(&self.file_path, ".bak")).map_err(|e| {
                format!("Failed to keep the previous recordings ({e}); nothing was saved.")
            })?;
        }
        let tmp = sibling(&self.file_path, ".tmp");
        match crate::data::versions::write_all_synced(&tmp, &bytes) {
            Ok(true) => {}
            Ok(false) => eprintln!("Warning: recordings written but fsync could not be confirmed"),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(format!("Failed to write the recordings file: {e}"));
            }
        }
        crate::data::versions::atomic_replace(&tmp, &self.file_path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("Failed to write the recordings file: {e}")
        })
    }

    /// Lê, deixa `change` mexer e grava — tudo sob o mesmo lock. Arquivo
    /// ilegível nunca é sobrescrito.
    fn modify<R>(
        &self,
        change: impl FnOnce(&mut RecordingsFile) -> Result<R, String>,
    ) -> Result<R, String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        let current = self.read_from_disk().map_err(|e| {
            format!("{e}; refusing to overwrite it. Fix or restore {RECORDINGS_FILE_NAME} and restart.")
        })?;
        let existed = current.is_some();
        let mut file = current.unwrap_or_default();
        let out = change(&mut file)?;
        file.version = RECORDINGS_FILE_VERSION;
        self.write(&file, existed)?;
        Ok(out)
    }

    /// Cria (id vazio ou desconhecido) ou substitui a gravação com o mesmo id.
    pub fn upsert(&self, recording: Recording, now_ms: i64) -> Result<Recording, String> {
        let mut recording = normalize_recording(recording)?;
        self.modify(|file| {
            match file
                .recordings
                .iter_mut()
                .find(|r| !recording.id.is_empty() && r.id == recording.id)
            {
                Some(slot) => {
                    recording.created_at = slot.created_at;
                    recording.updated_at = now_ms;
                    *slot = recording.clone();
                }
                None => {
                    if file.recordings.len() >= MAX_RECORDINGS {
                        return Err(format!("You can keep up to {MAX_RECORDINGS} recordings."));
                    }
                    recording.id = new_id(file, now_ms);
                    recording.created_at = now_ms;
                    recording.updated_at = now_ms;
                    file.recordings.push(recording.clone());
                }
            }
            Ok(recording)
        })
    }

    /// Cópia com outro nome (a tela manda o nome, já traduzido).
    pub fn duplicate(&self, id: &str, name: &str, now_ms: i64) -> Result<Recording, String> {
        let source = self
            .load()?
            .recordings
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("That recording no longer exists.")?;
        let name: String = name.trim().chars().take(MAX_RECORDING_NAME_CHARS).collect();
        self.upsert(
            Recording {
                id: String::new(),
                name,
                steps: source.steps,
                created_at: 0,
                updated_at: 0,
            },
            now_ms,
        )
    }

    /// Apaga pelo id e tira a gravação de quem a usava (a de todas e as das
    /// contas). `Ok(false)` se não existia.
    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        let Some(mut file) = self.read_from_disk()? else {
            return Ok(false);
        };
        let before = file.recordings.len();
        file.recordings.retain(|r| r.id != id);
        if file.recordings.len() == before {
            return Ok(false);
        }
        if file.default_id.as_deref() == Some(id) {
            file.default_id = None;
        }
        file.account_ids.retain(|_, rec| rec != id);
        self.write(&file, true)?;
        Ok(true)
    }

    /// A gravação de todas as contas (`None` tira).
    pub fn set_default(&self, id: Option<String>) -> Result<(), String> {
        self.modify(|file| {
            if let Some(id) = &id {
                if !file.recordings.iter().any(|r| &r.id == id) {
                    return Err("That recording no longer exists.".into());
                }
            }
            file.default_id = id;
            Ok(())
        })
    }

    /// A gravação própria de uma conta (`None` volta para a de todas).
    pub fn set_for_account(&self, user_id: i64, id: Option<String>) -> Result<(), String> {
        self.modify(|file| {
            match id {
                Some(id) => {
                    if !file.recordings.iter().any(|r| r.id == id) {
                        return Err("That recording no longer exists.".into());
                    }
                    file.account_ids.insert(user_id, id);
                }
                None => {
                    file.account_ids.remove(&user_id);
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod recordings_store_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "ram-recordings-{}-{nanos}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join(RECORDINGS_FILE_NAME)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn recording(name: &str) -> Recording {
        Recording {
            name: name.into(),
            steps: vec![
                RecordingStep::Key {
                    key: "w".into(),
                    hold_ms: 500,
                },
                RecordingStep::Wait { ms: 250 },
                RecordingStep::Click {
                    x_pct: 37.5,
                    y_pct: 62.5,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_new_recording_gets_an_id_and_round_trips_in_camel_case() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        assert!(store.load().unwrap().recordings.is_empty(), "sem arquivo = sem gravações");

        let saved = store.upsert(recording("Farm"), 1_000).unwrap();
        assert_eq!(saved.id, "rec-1000");
        assert_eq!(saved.created_at, 1_000);
        // A tecla volta na grafia da lista.
        assert_eq!(
            saved.steps[0],
            RecordingStep::Key {
                key: "W".into(),
                hold_ms: 500
            }
        );
        assert_eq!(store.load().unwrap().recordings, vec![saved.clone()]);

        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.file()).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        let steps = &raw["recordings"][0]["steps"];
        assert_eq!(steps[0]["type"], "key");
        assert_eq!(steps[0]["holdMs"], 500);
        assert_eq!(steps[1]["type"], "wait");
        assert_eq!(steps[2]["type"], "click");
        assert_eq!(steps[2]["xPct"], 37.5);
        assert!(raw["recordings"][0].get("createdAt").is_some());
    }

    #[test]
    fn the_step_types_parse_from_the_documented_json() {
        let json = r#"{"version":1,"recordings":[{"id":"a","name":"A","steps":[
            {"type":"key","key":"Space"},
            {"type":"keyDown","key":"W"},
            {"type":"wait","ms":800},
            {"type":"keyUp","key":"W"},
            {"type":"click","xPct":10,"yPct":90}
        ]}],"defaultId":"a","accountIds":{"42":"a"}}"#;
        let file: RecordingsFile = serde_json::from_str(json).unwrap();
        let steps = &file.recordings[0].steps;
        assert_eq!(
            steps[0],
            RecordingStep::Key {
                key: "Space".into(),
                hold_ms: DEFAULT_HOLD_MS
            },
            "sem holdMs, o toque padrão"
        );
        assert_eq!(steps[1], RecordingStep::KeyDown { key: "W".into() });
        assert_eq!(steps[2], RecordingStep::Wait { ms: 800 });
        assert_eq!(steps[3], RecordingStep::KeyUp { key: "W".into() });
        assert_eq!(file.account_ids.get(&42).map(String::as_str), Some("a"));
    }

    #[test]
    fn saving_with_an_existing_id_replaces_in_place_and_keeps_the_creation_time() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        let first = store.upsert(recording("Farm"), 1_000).unwrap();
        let mut renamed = first.clone();
        renamed.name = "Night farm".into();
        let saved = store.upsert(renamed, 9_999).unwrap();
        assert_eq!(saved.id, first.id);
        assert_eq!(saved.created_at, 1_000);
        assert_eq!(saved.updated_at, 9_999);
        let all = store.load().unwrap().recordings;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Night farm");
    }

    #[test]
    fn duplicate_copies_the_steps_under_a_new_id_and_name() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        let first = store.upsert(recording("Farm"), 5).unwrap();
        let copy = store.duplicate(&first.id, "Farm (copy)", 5).unwrap();
        assert_ne!(copy.id, first.id);
        assert_eq!(copy.name, "Farm (copy)");
        assert_eq!(copy.steps, first.steps);
        assert_eq!(store.load().unwrap().recordings.len(), 2);
        assert!(store.duplicate("nope", "X", 6).is_err());
    }

    #[test]
    fn deleting_a_recording_clears_whoever_used_it() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        let a = store.upsert(recording("A"), 1).unwrap();
        let b = store.upsert(recording("B"), 2).unwrap();
        store.set_default(Some(a.id.clone())).unwrap();
        store.set_for_account(11, Some(a.id.clone())).unwrap();
        store.set_for_account(22, Some(b.id.clone())).unwrap();

        assert!(store.delete(&a.id).unwrap());
        let file = store.load().unwrap();
        assert_eq!(file.default_id, None);
        assert_eq!(file.account_ids.get(&11), None);
        assert_eq!(file.account_ids.get(&22), Some(&b.id));
        assert!(!store.delete(&a.id).unwrap(), "apagar de novo não acha nada");
    }

    #[test]
    fn assigning_a_recording_that_does_not_exist_is_refused() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        assert!(store.set_default(Some("ghost".into())).is_err());
        assert!(store.set_for_account(1, Some("ghost".into())).is_err());
        store.set_for_account(1, None).unwrap();
    }

    #[test]
    fn every_write_keeps_the_previous_file_in_a_bak() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        store.upsert(recording("A"), 1).unwrap();
        assert!(!store.backup_file_path().exists());
        let before = fs::read(dir.file()).unwrap();
        store.upsert(recording("B"), 2).unwrap();
        assert_eq!(fs::read(store.backup_file_path()).unwrap(), before);
        assert!(
            !sibling(&dir.file(), ".tmp").exists(),
            "o temporário da troca atômica não fica para trás"
        );
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten() {
        let dir = TempDir::new();
        fs::write(dir.file(), b"{ broken").unwrap();
        let store = RecordingStore::new(dir.file());
        assert!(store.load().is_err());
        let err = store.upsert(recording("A"), 1).unwrap_err();
        assert!(err.contains("refusing to overwrite"), "{err}");
        assert!(store.set_default(None).is_err());
        assert_eq!(fs::read(dir.file()).unwrap(), b"{ broken");
    }

    #[test]
    fn the_store_reads_the_disk_every_time_so_a_restored_file_counts_at_once() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        store.upsert(recording("Old"), 1).unwrap();
        let restored = RecordingsFile {
            recordings: vec![Recording {
                id: "rec-9".into(),
                name: "Restored".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        fs::write(dir.file(), serde_json::to_vec(&restored).unwrap()).unwrap();
        assert_eq!(store.load().unwrap().recordings[0].name, "Restored");
    }

    #[test]
    fn the_library_has_a_ceiling() {
        let dir = TempDir::new();
        let store = RecordingStore::new(dir.file());
        let file = RecordingsFile {
            recordings: (0..MAX_RECORDINGS)
                .map(|n| Recording {
                    id: format!("r{n}"),
                    name: format!("R{n}"),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        fs::write(dir.file(), serde_json::to_vec(&file).unwrap()).unwrap();
        assert!(store.upsert(recording("One more"), 1).is_err());
    }
}

#[cfg(test)]
mod recordings_validation_tests {
    use super::*;

    #[test]
    fn the_key_list_starts_with_the_afk_keys_and_leaves_out_the_dangerous_ones() {
        let names = recording_key_names();
        assert_eq!(
            &names[..14],
            &["Space", "W", "A", "S", "D", "E", "F", "R", "Q", "1", "2", "3", "4", "5"]
        );
        for outside in [
            "Enter", "Tab", "Escape", "Esc", "F4", "F9", "Alt", "Ctrl", "LWin", "Backspace",
            "Delete", "/", "`", "", " ", "0x20",
        ] {
            assert!(recording_key(outside).is_none(), "tecla fora da lista: {outside:?}");
        }
        // Todas as letras e números estão na lista, cada um uma vez.
        for c in ('A'..='Z').chain('0'..='9') {
            let name = c.to_string();
            assert_eq!(names.iter().filter(|n| **n == name).count(), 1, "{name}");
        }
    }

    #[test]
    fn arrows_are_extended_keys_and_letters_are_not() {
        assert_eq!(recording_key("Up"), Some((0x26, true)));
        assert_eq!(recording_key("left"), Some((0x25, true)));
        assert_eq!(recording_key("W"), Some((0x57, false)));
        assert_eq!(recording_key("shift"), Some((0xA0, false)));
    }

    #[test]
    fn a_recording_needs_a_name_and_keys_from_the_list() {
        let ok = Recording {
            name: "  Path  ".into(),
            ..Default::default()
        };
        assert_eq!(normalize_recording(ok).unwrap().name, "Path");
        assert!(normalize_recording(Recording::default()).is_err());
        let bad_key = Recording {
            name: "X".into(),
            steps: vec![RecordingStep::Key {
                key: "Enter".into(),
                hold_ms: 40,
            }],
            ..Default::default()
        };
        assert!(normalize_recording(bad_key).unwrap_err().contains("Enter"));
        let long_name = Recording {
            name: "x".repeat(MAX_RECORDING_NAME_CHARS + 1),
            ..Default::default()
        };
        assert!(normalize_recording(long_name).is_err());
    }

    #[test]
    fn times_and_points_are_clamped_into_their_limits() {
        assert_eq!(
            normalize_step(RecordingStep::Key {
                key: "e".into(),
                hold_ms: 0
            })
            .unwrap(),
            RecordingStep::Key {
                key: "E".into(),
                hold_ms: MIN_HOLD_MS
            }
        );
        assert_eq!(
            normalize_step(RecordingStep::Key {
                key: "E".into(),
                hold_ms: 999_999
            })
            .unwrap(),
            RecordingStep::Key {
                key: "E".into(),
                hold_ms: MAX_HOLD_MS
            }
        );
        assert_eq!(
            normalize_step(RecordingStep::Wait { ms: u64::MAX }).unwrap(),
            RecordingStep::Wait { ms: MAX_WAIT_MS }
        );
        assert_eq!(
            normalize_step(RecordingStep::Click {
                x_pct: -5.0,
                y_pct: f64::NAN
            })
            .unwrap(),
            RecordingStep::Click {
                x_pct: 0.0,
                y_pct: 50.0
            }
        );
    }

    #[test]
    fn a_recording_has_a_ceiling_of_steps_and_of_time() {
        let too_many = Recording {
            name: "X".into(),
            steps: vec![RecordingStep::Wait { ms: 1 }; MAX_RECORDING_STEPS + 1],
            ..Default::default()
        };
        assert!(normalize_recording(too_many).is_err());
        let too_long = Recording {
            name: "X".into(),
            steps: vec![RecordingStep::Wait { ms: MAX_WAIT_MS }, RecordingStep::Wait { ms: 1 }],
            ..Default::default()
        };
        assert!(normalize_recording(too_long).unwrap_err().contains("10 minutes"));
    }

    #[test]
    fn the_duration_counts_waits_holds_and_clicks() {
        let steps = vec![
            RecordingStep::Key {
                key: "W".into(),
                hold_ms: 300,
            },
            RecordingStep::Wait { ms: 200 },
            RecordingStep::Click {
                x_pct: 1.0,
                y_pct: 1.0,
            },
            RecordingStep::KeyDown { key: "A".into() },
            RecordingStep::KeyUp { key: "A".into() },
        ];
        assert_eq!(recording_duration_ms(&steps), 300 + 200 + CLICK_ESTIMATE_MS);
    }

    fn file_with(ids: &[&str], default: Option<&str>, own: &[(i64, &str)]) -> RecordingsFile {
        RecordingsFile {
            recordings: ids
                .iter()
                .map(|id| Recording {
                    id: (*id).into(),
                    name: (*id).into(),
                    ..Default::default()
                })
                .collect(),
            default_id: default.map(str::to_string),
            account_ids: own.iter().map(|(u, id)| (*u, (*id).to_string())).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn the_account_recording_wins_over_the_one_for_everyone() {
        let file = file_with(&["all", "mine"], Some("all"), &[(11, "mine")]);
        assert_eq!(recording_for_account(&file, 11).map(|r| r.id.as_str()), Some("mine"));
        assert_eq!(recording_for_account(&file, 22).map(|r| r.id.as_str()), Some("all"));
    }

    #[test]
    fn without_any_choice_an_account_has_no_recording() {
        let file = file_with(&["a"], None, &[]);
        assert!(recording_for_account(&file, 11).is_none());
        // Escolha que aponta para gravação apagada (arquivo editado à mão) cai
        // na de todas.
        let stale = file_with(&["all"], Some("all"), &[(11, "gone")]);
        assert_eq!(recording_for_account(&stale, 11).map(|r| r.id.as_str()), Some("all"));
    }
}
