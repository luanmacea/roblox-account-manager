/// Pasta de dados do app dentro do perfil do usuário.
pub const APP_DIR_NAME: &str = "Roblox Account Manager";
/// Arquivo vazio ao lado do executável que força o modo portátil (dados na
/// própria pasta do exe, como nas versões antigas).
pub const PORTABLE_MARKER: &str = "portable.txt";
/// Sobrescreve a pasta de dados (usado nos testes e por quem quer pasta própria).
pub const DATA_DIR_ENV: &str = "RAM_DATA_DIR";

/// Arquivos e pastas que pertencem ao usuário e migram junto com ele.
pub const DATA_FILES: &[&str] = &[
    "AccountData.json",
    // A chave que abre o vault sem senha. Tem que viajar **junto** com o
    // `AccountData.json` na migração de pasta e no backup: um vault cifrado sem
    // a chave dele é um vault perdido. O custo é honesto e está na doc — o zip
    // de backup passa a carregar a chave, então o que protege um backup vazado
    // é o embrulho do aparelho, não o DPAPI (que só abre no perfil de origem).
    "AccountData.key",
    "RAMSettings.ini",
    "RAMTheme.ini",
    "RAMThemePresets.json",
    "RAMScripts.json",
    "RAMAvatars.json",
    // Favoritos (com os links dos servidores VIP), jogos e servidores recentes.
    // Moravam só no `localStorage` do WebView e ficavam fora do backup.
    "RAMGameLists.json",
    // Presets de launch (contas → jogo/servidor) e os horários deles.
    "RAMLaunchPresets.json",
    // Gravações (sequências de teclas, cliques e esperas) e qual vale para quem.
    "RAMRecordings.json",
    // Histórico de sessões por conta (uma linha JSON por evento, 90 dias).
    "RAMSessionHistory.jsonl",
    "AccountControlData.json",
];
pub const DATA_DIRS: &[&str] = &["RAMThemeFonts"];

/// Pasta do executável (onde as versões antigas guardavam tudo).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// `%LOCALAPPDATA%\Roblox Account Manager` no Windows (mesma raiz que o
/// catálogo de versões e os backups de isolamento já usam), o equivalente
/// em cada outro SO, ou `None` quando a variável não existe.
pub fn app_data_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);

    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library").join("Application Support"));

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));

    base.map(|base| base.join(APP_DIR_NAME))
}

/// Decide a pasta de dados. Separada da leitura do ambiente para poder ser
/// testada: precedência é variável de ambiente → modo portátil → perfil do
/// usuário → pasta do exe (último recurso, quando não há perfil).
fn decide_data_dir(
    env_override: Option<PathBuf>,
    portable: bool,
    app_root: Option<PathBuf>,
    exe_dir: PathBuf,
) -> PathBuf {
    if let Some(dir) = env_override.filter(|d| !d.as_os_str().is_empty()) {
        return dir;
    }
    if portable {
        return exe_dir;
    }
    app_root.unwrap_or(exe_dir)
}

/// Copia os dados do usuário de `from` para `to` **sem nunca sobrescrever** o
/// que já existe no destino e sem apagar a origem (se algo der errado, basta
/// voltar a versão antiga do app que os arquivos continuam lá).
/// Devolve o que foi migrado.
pub fn migrate_data_files(from: &Path, to: &Path) -> Vec<String> {
    let mut migrated = Vec::new();
    if from == to || !from.exists() {
        return migrated;
    }
    if std::fs::create_dir_all(to).is_err() {
        return migrated;
    }

    for name in DATA_FILES {
        let source = from.join(name);
        let target = to.join(name);
        if source.is_file() && !target.exists() && std::fs::copy(&source, &target).is_ok() {
            migrated.push((*name).to_string());
        }
    }

    for name in DATA_DIRS {
        let source = from.join(name);
        let target = to.join(name);
        if source.is_dir() && !target.exists() && copy_dir_recursive(&source, &target).is_ok() {
            migrated.push((*name).to_string());
        }
    }

    migrated
}

fn copy_dir_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Pasta onde o app lê e grava os dados do usuário, resolvida uma vez por
/// processo (com a migração da pasta do exe feita junto, na primeira vez).
pub fn get_runtime_data_dir() -> PathBuf {
    static RESOLVED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let exe = exe_dir();
            let dir = decide_data_dir(
                std::env::var_os(DATA_DIR_ENV).map(PathBuf::from),
                exe.join(PORTABLE_MARKER).exists(),
                app_data_root(),
                exe.clone(),
            );
            let _ = std::fs::create_dir_all(&dir);
            let migrated = migrate_data_files(&exe, &dir);
            if !migrated.is_empty() {
                eprintln!(
                    "Dados migrados da pasta do executável para {}: {}",
                    dir.display(),
                    migrated.join(", ")
                );
            }
            dir
        })
        .clone()
}

pub fn get_settings_path() -> PathBuf {
    get_runtime_data_dir().join("RAMSettings.ini")
}

pub fn get_theme_path() -> PathBuf {
    get_runtime_data_dir().join("RAMTheme.ini")
}

pub fn get_theme_presets_path() -> PathBuf {
    get_runtime_data_dir().join("RAMThemePresets.json")
}

/// Mora aqui, e não em `data/scripts.rs`, porque aquele arquivo é montado
/// isolado pelo teste de integração `security_regression_scripts_store` e não
/// pode referenciar outros módulos do crate.
pub fn get_scripts_path() -> PathBuf {
    get_runtime_data_dir().join("RAMScripts.json")
}

/// Avatares salvos da aba de avatares gratuitos.
pub fn get_avatars_path() -> PathBuf {
    get_runtime_data_dir().join("RAMAvatars.json")
}

/// Favoritos, jogos recentes e servidores recentes (`data/game_lists.rs`).
pub fn get_game_lists_path() -> PathBuf {
    get_runtime_data_dir().join("RAMGameLists.json")
}

/// Presets de launch (`data/launch_presets.rs`).
pub fn get_launch_presets_path() -> PathBuf {
    get_runtime_data_dir().join("RAMLaunchPresets.json")
}

/// Gravações (`data/recordings.rs`).
pub fn get_recordings_path() -> PathBuf {
    get_runtime_data_dir().join(crate::data::recordings::RECORDINGS_FILE_NAME)
}

/// Histórico de sessões (`data/session_history.rs`).
pub fn get_session_history_path() -> PathBuf {
    get_runtime_data_dir().join(crate::data::session_history::SESSION_HISTORY_FILE_NAME)
}

pub fn get_theme_fonts_dir() -> PathBuf {
    get_runtime_data_dir().join("RAMThemeFonts")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeFontAssetImportResult {
    pub file: String,
    pub suggested_family: String,
}

fn sanitize_font_family_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Custom Font")
        .trim();
    if stem.is_empty() {
        return "Custom Font".to_string();
    }
    stem.to_string()
}

fn is_allowed_font_ext(ext: &str) -> bool {
    matches!(ext, "ttf" | "otf" | "woff" | "woff2")
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}


#[cfg(test)]
mod settings_paths_tests {
    use super::*;

    fn exe_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .expect("the test binary has a parent directory")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ram-paths-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn the_runtime_data_dir_is_absolute_and_stable() {
        let dir = get_runtime_data_dir();
        assert!(dir.is_absolute(), "{}", dir.display());
        assert_eq!(get_runtime_data_dir(), dir, "resolvida uma vez por processo");
    }

    #[test]
    fn every_settings_path_lives_in_the_runtime_data_dir() {
        let cases = [
            (get_settings_path(), "RAMSettings.ini"),
            (get_scripts_path(), "RAMScripts.json"),
            (get_avatars_path(), "RAMAvatars.json"),
            (get_game_lists_path(), "RAMGameLists.json"),
            (get_launch_presets_path(), "RAMLaunchPresets.json"),
            (get_recordings_path(), "RAMRecordings.json"),
            (get_session_history_path(), "RAMSessionHistory.jsonl"),
            (get_theme_path(), "RAMTheme.ini"),
            (get_theme_presets_path(), "RAMThemePresets.json"),
            (get_theme_fonts_dir(), "RAMThemeFonts"),
        ];
        for (path, expected) in cases {
            assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(expected));
            assert_eq!(path.parent(), Some(get_runtime_data_dir().as_path()));
        }
    }

    // ---- escolha da pasta de dados ------------------------------------------------

    #[test]
    fn the_env_override_wins_over_everything() {
        let dir = decide_data_dir(
            Some(PathBuf::from("D:/custom")),
            true,
            Some(PathBuf::from("C:/users/x/AppData/Local/RAM")),
            PathBuf::from("C:/app"),
        );
        assert_eq!(dir, PathBuf::from("D:/custom"));
    }

    #[test]
    fn an_empty_env_override_is_ignored() {
        let dir = decide_data_dir(
            Some(PathBuf::new()),
            false,
            Some(PathBuf::from("C:/profile/RAM")),
            PathBuf::from("C:/app"),
        );
        assert_eq!(dir, PathBuf::from("C:/profile/RAM"));
    }

    #[test]
    fn the_portable_marker_keeps_the_data_next_to_the_executable() {
        let dir = decide_data_dir(
            None,
            true,
            Some(PathBuf::from("C:/profile/RAM")),
            PathBuf::from("C:/app"),
        );
        assert_eq!(dir, PathBuf::from("C:/app"));
    }

    #[test]
    fn without_a_marker_the_data_goes_to_the_user_profile() {
        let dir = decide_data_dir(
            None,
            false,
            Some(PathBuf::from("C:/profile/RAM")),
            PathBuf::from("C:/app"),
        );
        assert_eq!(dir, PathBuf::from("C:/profile/RAM"));
    }

    #[test]
    fn without_a_user_profile_it_falls_back_to_the_executable_directory() {
        let dir = decide_data_dir(None, false, None, PathBuf::from("C:/app"));
        assert_eq!(dir, PathBuf::from("C:/app"));
    }

    #[test]
    fn the_app_data_root_ends_with_the_app_folder_name() {
        if let Some(root) = app_data_root() {
            assert_eq!(root.file_name().and_then(|n| n.to_str()), Some(APP_DIR_NAME));
            assert!(root.is_absolute(), "{}", root.display());
        }
    }

    // ---- migração ------------------------------------------------------------------

    #[test]
    fn migration_copies_the_user_files_and_keeps_the_originals() {
        let from = temp_dir("mig-from");
        let to = temp_dir("mig-to");
        std::fs::write(from.join("AccountData.json"), b"[]").unwrap();
        std::fs::write(from.join("RAMSettings.ini"), b"[General]\n").unwrap();
        std::fs::create_dir_all(from.join("RAMThemeFonts").join("sub")).unwrap();
        std::fs::write(from.join("RAMThemeFonts").join("sub").join("a.ttf"), b"font").unwrap();

        let migrated = migrate_data_files(&from, &to);

        assert!(migrated.contains(&"AccountData.json".to_string()));
        assert!(migrated.contains(&"RAMSettings.ini".to_string()));
        assert!(migrated.contains(&"RAMThemeFonts".to_string()));
        assert_eq!(std::fs::read(to.join("AccountData.json")).unwrap(), b"[]");
        assert_eq!(
            std::fs::read(to.join("RAMThemeFonts").join("sub").join("a.ttf")).unwrap(),
            b"font"
        );
        // O original continua lá: voltar para uma versão antiga do app não perde nada.
        assert!(from.join("AccountData.json").exists());

        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn migration_never_overwrites_what_is_already_in_the_destination() {
        let from = temp_dir("mig-keep-from");
        let to = temp_dir("mig-keep-to");
        std::fs::write(from.join("AccountData.json"), b"old").unwrap();
        std::fs::write(to.join("AccountData.json"), b"current").unwrap();

        let migrated = migrate_data_files(&from, &to);

        assert!(migrated.is_empty());
        assert_eq!(std::fs::read(to.join("AccountData.json")).unwrap(), b"current");

        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn migration_is_a_noop_when_the_source_is_the_destination() {
        let dir = temp_dir("mig-same");
        std::fs::write(dir.join("AccountData.json"), b"[]").unwrap();

        assert!(migrate_data_files(&dir, &dir).is_empty());
        assert_eq!(std::fs::read(dir.join("AccountData.json")).unwrap(), b"[]");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_of_a_missing_source_does_nothing() {
        let to = temp_dir("mig-missing-to");
        let missing = to.join("nao-existe");

        assert!(migrate_data_files(&missing, &to).is_empty());

        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn the_settings_paths_are_all_distinct() {
        let all = [
            get_settings_path(),
            get_theme_path(),
            get_theme_presets_path(),
            get_theme_fonts_dir(),
        ];
        let mut unique = all.to_vec();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), all.len());
    }

    // ---- font helpers -------------------------------------------------------------

    #[test]
    fn is_allowed_font_ext_accepts_only_the_four_lowercase_web_font_extensions() {
        for ext in ["ttf", "otf", "woff", "woff2"] {
            assert!(is_allowed_font_ext(ext), "{ext} should be allowed");
        }
        for ext in ["", "TTF", "WOFF2", "eot", "ttc", "json", "exe", "ttf2", ".ttf"] {
            assert!(!is_allowed_font_ext(ext), "{ext:?} should be rejected");
        }
    }

    #[test]
    fn sanitize_font_family_from_path_uses_the_trimmed_file_stem() {
        assert_eq!(
            sanitize_font_family_from_path(Path::new("C:\\fonts\\My Font.ttf")),
            "My Font"
        );
        assert_eq!(
            sanitize_font_family_from_path(Path::new("/usr/share/fonts/Inter-Regular.otf")),
            "Inter-Regular"
        );
        // Only the last extension is stripped.
        assert_eq!(
            sanitize_font_family_from_path(Path::new("Family.Bold.woff2")),
            "Family.Bold"
        );
        assert_eq!(sanitize_font_family_from_path(Path::new("NoExtension")), "NoExtension");
    }

    #[test]
    fn sanitize_font_family_from_path_falls_back_to_custom_font() {
        assert_eq!(sanitize_font_family_from_path(Path::new("")), "Custom Font");
        assert_eq!(sanitize_font_family_from_path(Path::new("   .ttf")), "Custom Font");
        assert_eq!(sanitize_font_family_from_path(Path::new("..")), "Custom Font");
    }

    #[test]
    fn to_hex_encodes_bytes_as_lowercase_pairs() {
        assert_eq!(to_hex(&[]), "");
        assert_eq!(to_hex(&[0x00]), "00");
        assert_eq!(to_hex(&[0x0f]), "0f");
        assert_eq!(to_hex(&[0xff]), "ff");
        assert_eq!(to_hex(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(to_hex(&[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]), "0123456789abcdef");

        let all: Vec<u8> = (0u8..=255).collect();
        let hex = to_hex(&all);
        assert_eq!(hex.len(), 512);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn the_font_import_result_serializes_its_fields_verbatim() {
        let result = ThemeFontAssetImportResult {
            file: "abc123.ttf".to_string(),
            suggested_family: "My Font".to_string(),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["file"], "abc123.ttf");
        assert_eq!(
            json["suggested_family"], "My Font",
            "no rename_all: the frontend sees snake_case here"
        );
    }
}
