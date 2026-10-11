pub struct SettingsStore {
    ini: Mutex<IniFile>,
    file_path: PathBuf,
    /// Ligado quando o INI existe mas não pôde ser lido. `IniFile::load` engole
    /// o erro de I/O, então sem este latch `apply_defaults` gravaria um arquivo
    /// só com os defaults por cima das settings do usuário — mesmo latch do
    /// `ScriptStore`.
    load_failed: std::sync::atomic::AtomicBool,
}

impl SettingsStore {
    pub fn new(file_path: PathBuf) -> Self {
        let file_existed = file_path.exists();
        let (ini, failed) = if file_existed {
            match fs::read_to_string(&file_path) {
                Ok(raw) => {
                    let mut ini = IniFile::new();
                    ini.parse(&raw);
                    (ini, false)
                }
                Err(_) => (IniFile::new(), true),
            }
        } else {
            (IniFile::new(), false)
        };

        let store = Self {
            ini: Mutex::new(ini),
            file_path,
            load_failed: std::sync::atomic::AtomicBool::new(failed),
        };

        store.apply_defaults(file_existed);
        store
    }

    fn apply_defaults(&self, settings_file_existed: bool) {
        let mut ini = self.ini.lock().unwrap();

        let defaults: &[(&str, &str, Option<&str>)] = &[
            ("CheckForUpdates", "true", None),
            ("UpdaterReleaseChannel", "beta", None),
            // Default = a edição que está rodando: quem instalou a completa
            // continua recebendo a completa. Valor já gravado nunca é trocado.
            ("UpdaterFeatureChannel", crate::RUNNING_FEATURE_CHANNEL, None),
            ("AccountJoinDelay", "8", None),
            ("AsyncJoin", "false", None),
            (
                "WaitForGameJoin",
                "true",
                Some("Multi-launch: start the next account as soon as the previous one is in the game (never before AccountJoinDelay's 8 s floor, at most 20 s)"),
            ),
            ("DisableAgingAlert", "false", None),
            ("HideUsernames", "false", None),
            ("WrapLongNames", "false", None),
            (
                "ServerRegionFormat",
                "<city>, <countryCode>",
                // O comentario apontava ip-api.com, que nao tem nada a ver com os
                // tokens que `format_region` substitui de fato.
                Some("Tokens: <city>, <region>, <country>, <countryCode>, <ip>; other text is kept as typed"),
            ),
            ("MaxRecentGames", "8", None),
            ("MaxRecentJobs", "12", None),
            (
                "GroupOrder",
                "[]",
                // JSON, e nao lista por virgula como as outras chaves de lista:
                // nome de grupo e texto livre do usuario e pode conter virgula.
                Some("Manual order of the account groups, as a JSON array of group names"),
            ),
            ("Language", "en", None),
            ("AutoCookieRefresh", "true", None),
            ("AutoCloseLastProcess", "false", None),
            ("AutoCloseRobloxForMultiRbx", "false", None),
            ("ShowPresence", "true", None),
            (
                "ShowAccountNameOnWindow",
                "true",
                Some("Titles each Roblox window this app tracks \"<account> — Roblox\", name first (masked when names are hidden)"),
            ),
            ("PresenceUpdateRate", "5", None),
            ("WarnOnOnlineJoin", "true", None),
            ("WarnOnCopyCredential", "true", None),
            (
                "CheckModerationBeforeLaunch",
                "true",
                Some("Check if an account is banned right before launching it, and skip it if so"),
            ),
            ("UnlockFPS", "false", None),
            ("MaxFPSValue", "120", None),
            ("CustomClientSettings", "", None),
            ("OverrideClientVolume", "false", None),
            ("ClientVolume", "0.5", None),
            ("OverrideClientGraphics", "false", None),
            ("ClientGraphicsLevel", "10", None),
            ("OverrideClientWindowSize", "false", None),
            ("ClientWindowWidth", "1280", None),
            ("ClientWindowHeight", "720", None),
            ("StartRobloxMinimized", "false", None),
            (
                "AutoArrangeGrid",
                "true",
                Some("New Roblox windows take the first free cell of the window grid (GridMonitors, GridGap)"),
            ),
            (
                "GridAllowSmallWindows",
                "false",
                Some("Grid cells may be smaller than Roblox's minimum window (only clients MultiAlt launched)"),
            ),
            (
                "GridBorderless",
                "false",
                Some("Grid windows lose their title bar and border; turning it off gives them back"),
            ),
            ("StartOnPCStartup", "false", None),
            ("MinimizeToTray", "false", None),
            ("ThemeWindowsNavbar", "true", None),
            ("RestrictedBackgroundStyle", "warp", None),
            ("BottingEnabled", "false", None),
            (
                "KeepPcAwake",
                "true",
                Some("Keep the PC from sleeping (the screen may still turn off) while AFK Mode, Auto Rejoin or auto-reconnect runs"),
            ),
            (
                "AutoReconnect",
                "false",
                Some("Default for every account: reopen a client this app opened in the same game when it drops (each account's AutoReconnect field wins)"),
            ),
            ("BottingUseSharedClientProfile", "true", None),
            ("BottingAutoShareLaunchFields", "true", None),
            ("BottingDualPanelDialog", "true", None),
            ("BottingPlayerUnlockFPS", "false", None),
            ("BottingPlayerMaxFPSValue", "120", None),
            ("BottingPlayerCustomClientSettings", "", None),
            ("BottingPlayerOverrideClientVolume", "false", None),
            ("BottingPlayerClientVolume", "0.5", None),
            ("BottingPlayerOverrideClientGraphics", "false", None),
            ("BottingPlayerClientGraphicsLevel", "10", None),
            ("BottingPlayerOverrideClientWindowSize", "false", None),
            ("BottingPlayerClientWindowWidth", "1280", None),
            ("BottingPlayerClientWindowHeight", "720", None),
            ("BottingPlayerStartRobloxMinimized", "false", None),
            ("BottingBotUnlockFPS", "false", None),
            ("BottingBotMaxFPSValue", "120", None),
            ("BottingBotCustomClientSettings", "", None),
            ("BottingBotOverrideClientVolume", "false", None),
            ("BottingBotClientVolume", "0.5", None),
            ("BottingBotOverrideClientGraphics", "false", None),
            ("BottingBotClientGraphicsLevel", "10", None),
            ("BottingBotOverrideClientWindowSize", "false", None),
            ("BottingBotClientWindowWidth", "1280", None),
            ("BottingBotClientWindowHeight", "720", None),
            ("BottingBotStartRobloxMinimized", "false", None),
            ("BottingDefaultIntervalMinutes", "19", None),
            ("BottingLaunchDelaySeconds", "20", None),
            ("BottingRetryMax", "6", None),
            ("BottingRetryBaseSeconds", "8", None),
            ("BottingPlayerGraceMinutes", "15", None),
            ("BottingDraftPlaceId", "", None),
            ("BottingDraftJobId", "", None),
            ("BottingDraftLaunchData", "", None),
            ("BottingDraftPlayerAccountId", "", None),
            ("BottingDraftPlayerAccountIds", "", None),
            ("BottingDraftSelectedUserIds", "", None),
        ];

        let general = ini.section("General");
        for (key, value, comment) in defaults {
            if !general.exists(key) {
                general.set(key, value, *comment);
            }
        }
        if !general.exists("EncryptionMethod") {
            general.set("EncryptionMethod", "default", None);
        }
        if !general.exists("ThemeWindowsNavbarAutoEnabledV1") {
            general.set("ThemeWindowsNavbar", "true", None);
            general.set("ThemeWindowsNavbarAutoEnabledV1", "true", None);
        }
        if !general.exists("EncryptionOnboardingState") {
            general.set(
                "EncryptionOnboardingState",
                if settings_file_existed {
                    "completed"
                } else {
                    "pending"
                },
                None,
            );
        }
        if !general.exists("FirstRunWalkthroughState") {
            general.set(
                "FirstRunWalkthroughState",
                if settings_file_existed {
                    "completed"
                } else {
                    "pending"
                },
                None,
            );
        }

        let developer = ini.section("Developer");
        if !developer.exists("DevMode") {
            developer.set("DevMode", "false", None);
        }
        if !developer.exists("EnableWebServer") {
            developer.set("EnableWebServer", "false", None);
        }
        if !developer.exists("IsTeleport") {
            developer.set("IsTeleport", "false", None);
        }
        if !developer.exists("UseOldJoin") {
            developer.set("UseOldJoin", "false", None);
        }

        let ws_defaults: &[(&str, &str)] = &[
            ("WebServerPort", "7963"),
            ("AllowGetCookie", "false"),
            ("AllowGetAccounts", "false"),
            ("AllowLaunchAccount", "false"),
            ("AllowAccountEditing", "false"),
            ("EveryRequestRequiresPassword", "false"),
            ("AllowExternalConnections", "false"),
        ];

        let webserver = ini.section("WebServer");
        for (key, value) in ws_defaults {
            if !webserver.exists(key) {
                webserver.set(key, value, None);
            }
        }

        let ac_defaults: &[(&str, &str)] = &[
            ("AllowExternalConnections", "false"),
            ("StartOnLaunch", "false"),
            ("RelaunchDelay", "60"),
            ("LauncherDelay", "9"),
            ("NexusPort", "5242"),
            ("AutoMinimizeEnabled", "false"),
            ("AutoCloseEnabled", "false"),
            ("InternetCheck", "false"),
            ("UsePresence", "false"),
            ("AutoMinimizeInterval", "15"),
            ("AutoCloseInterval", "5"),
            ("MaxInstances", "3"),
            ("AutoCloseType", "0"),
        ];

        let account_control = ini.section("AccountControl");
        for (key, value) in ac_defaults {
            if !account_control.exists(key) {
                account_control.set(key, value, None);
            }
        }

        let watcher_defaults: &[(&str, &str)] = &[
            ("Enabled", "false"),
            ("ScanInterval", "6"),
            ("ReadInterval", "250"),
            ("ExitIfNoConnection", "false"),
            ("NoConnectionTimeout", "60"),
            ("ExitOnBeta", "false"),
            ("CloseIfNotResponding", "false"),
            ("CloseRbxMemory", "false"),
            ("MemoryLowValue", "200"),
            ("CloseRbxWindowTitle", "false"),
            ("ExpectedWindowTitle", "Roblox"),
            ("SaveWindowPositions", "false"),
        ];

        let watcher = ini.section("Watcher");
        for (key, value) in watcher_defaults {
            if !watcher.exists(key) {
                watcher.set(key, value, None);
            }
        }

        let optimization_defaults: &[(&str, &str)] = &[
            ("NormalEnableProcessPolicy", "false"),
            ("NormalProcessPolicyDelayMs", "1500"),
            ("NormalPriorityClass", "normal"),
            ("NormalBackgroundMode", "false"),
            ("NormalEcoQos", "false"),
            ("NormalIgnoreTimerResolution", "false"),
            ("NormalMemoryPriority", "normal"),
            ("NormalEnableFastFlags", "false"),
            ("NormalFastFlagsJson", ""),
            ("NormalEnableJobCpuLimit", "false"),
            ("NormalJobCpuLimitPercent", "25"),
            ("NormalEnableJobMemoryLimit", "false"),
            ("NormalJobMemoryLimitMb", "2048"),
            ("BottingPlayerEnableProcessPolicy", "false"),
            ("BottingPlayerProcessPolicyDelayMs", "1500"),
            ("BottingPlayerPriorityClass", "normal"),
            ("BottingPlayerBackgroundMode", "false"),
            ("BottingPlayerEcoQos", "false"),
            ("BottingPlayerIgnoreTimerResolution", "false"),
            ("BottingPlayerMemoryPriority", "normal"),
            ("BottingPlayerEnableFastFlags", "false"),
            ("BottingPlayerFastFlagsJson", ""),
            ("BottingPlayerEnableJobCpuLimit", "false"),
            ("BottingPlayerJobCpuLimitPercent", "25"),
            ("BottingPlayerEnableJobMemoryLimit", "false"),
            ("BottingPlayerJobMemoryLimitMb", "2048"),
            ("BottingBotEnableProcessPolicy", "false"),
            ("BottingBotProcessPolicyDelayMs", "1500"),
            ("BottingBotPriorityClass", "below_normal"),
            ("BottingBotBackgroundMode", "true"),
            ("BottingBotEcoQos", "true"),
            ("BottingBotIgnoreTimerResolution", "true"),
            ("BottingBotMemoryPriority", "low"),
            ("BottingBotEnableFastFlags", "false"),
            ("BottingBotFastFlagsJson", ""),
            ("BottingBotEnableJobCpuLimit", "false"),
            ("BottingBotJobCpuLimitPercent", "20"),
            ("BottingBotEnableJobMemoryLimit", "false"),
            ("BottingBotJobMemoryLimitMb", "1536"),
            // Otimizacao que segue o foco (docs/features/performance.md):
            // opcional, desligada por padrao.
            ("FollowFocus", "false"),
            // Fundo mudo (feature `live-audio`; sem ela a chave e ignorada).
            ("MuteBackgroundClients", "false"),
        ];

        let optimization = ini.section("Optimization");
        for (key, value) in optimization_defaults {
            if !optimization.exists(key) {
                optimization.set(key, value, None);
            }
        }

        let linux_defaults: &[(&str, &str)] = &[
            ("PreferredRunner", "sober"),
            ("CustomLaunchCommand", ""),
            ("CustomProcessMatch", "sober,flatpak,roblox,robloxplayerbeta"),
            ("CustomLogDir", ""),
            ("EnableExperimentalMultiRbx", "false"),
            ("WindowControlBackend", "auto"),
        ];

        let linux = ini.section("Linux");
        for (key, value) in linux_defaults {
            if !linux.exists(key) {
                linux.set(key, value, None);
            }
        }

        let generator_defaults: &[(&str, &str)] = &[
            ("Provider", "bloxgen"),
            ("ExtraDelaySeconds", "1"),
            ("TargetGroup", "BloxGen"),
            ("MaxAccounts", "0"),
            ("MaxConsecutiveFailures", "3"),
            // Padrao de nome do fluxo gratis (formulario do Roblox): "arvore"
            // gera "arvore_k3p9z". Vazio mantem o nome de palavras de sempre.
            // Nao vale para o BloxGen, cujo nome vem pronto do provedor.
            ("SignupUsernamePrefix", ""),
        ];

        let generator = ini.section("Generator");
        for (key, value) in generator_defaults {
            if !generator.exists(key) {
                generator.set(key, value, None);
            }
        }

        let bloxgen_defaults: &[(&str, &str)] = &[
            ("Endpoint", "https://core.bloxgen.net"),
            ("ApiKey", ""),
            ("AccountType", "alt"),
        ];

        let bloxgen = ini.section("BloxGen");
        for (key, value) in bloxgen_defaults {
            if !bloxgen.exists(key) {
                bloxgen.set(key, value, None);
            }
        }

        let versions_defaults: &[(&str, &str)] = &[
            ("DefaultVersion", ""),
            ("MaxParallelDownloads", "4"),
            ("CatalogCacheMinutes", "10"),
            ("PreferOldJoinForVersioned", "true"),
            ("ShowPreReleaseVersions", "false"),
            ("AllowLaunchOnOpenVersion", "false"),
        ];

        let versions = ini.section("Versions");
        for (key, value) in versions_defaults {
            if !versions.exists(key) {
                versions.set(key, value, None);
            }
        }

        let isolation_defaults: &[(&str, &str)] = &[
            ("Mode", "Off"),
            ("SpoofMachineGuid", "false"),
            ("SpoofMacAddress", "false"),
            ("TargetAdapter", ""),
            ("IncludeStudio", "false"),
            ("PreserveFastFlags", "true"),
            ("PreserveBasicSettings", "true"),
            ("BackupMachineGuid", ""),
            ("BackupNetworkAddress", ""),
            ("BackupAdapterId", ""),
        ];

        let isolation = ini.section("Isolation");
        for (key, value) in isolation_defaults {
            if !isolation.exists(key) {
                isolation.set(key, value, None);
            }
        }

        let login_defaults: &[(&str, &str)] = &[
            ("PersistentProfile", "true"),
            ("StealthMode", "true"),
            // Vazio = nada configurado; `IniSection::set` não grava valor em
            // branco, então esta chave só aparece no INI depois que o usuário
            // digita um caminho (ver EMPTY_STRING_DEFAULTS no teste abaixo).
            ("ManualBinaryPath", ""),
        ];

        let login = ini.section("Login");
        for (key, value) in login_defaults {
            if !login.exists(key) {
                login.set(key, value, None);
            }
        }

        // AFK mode. `Key` nasce **vazia** de proposito: sem tecla escolhida pelo
        // usuario o modo nao liga, e chave vazia nao chega a ser gravada no INI
        // (`IniSection::set` trata valor em branco como remocao). `Mode` nasce
        // `key` (o modo que ja existia) e o ponto do modo clique no meio da janela.
        // O intervalo e `IntervalMinutes` + `IntervalSeconds`; os segundos nascem
        // `0`, entao quem ja usava o modo continua com o mesmo intervalo.
        let afk_defaults: &[(&str, &str)] = &[
            ("IntervalMinutes", "10"),
            ("IntervalSeconds", "0"),
            ("Key", ""),
            ("BeepOnCycle", "false"),
            ("Mode", "key"),
            ("ClickX", "50"),
            ("ClickY", "50"),
        ];

        let afk = ini.section("Afk");
        for (key, value) in afk_defaults {
            if !afk.exists(key) {
                afk.set(key, value, None);
            }
        }

        // Gravações (docs/features/recordings.md): tocar a gravação da conta
        // depois de a reconexão automática devolvê-la ao jogo nasce desligado;
        // ligado, espera a conta ficar 30 s no jogo antes de tocar.
        let recordings_defaults: &[(&str, &str)] = &[
            ("AfterReconnect", "false"),
            ("AfterReconnectDelaySeconds", "30"),
        ];
        let recordings = ini.section("Recordings");
        for (key, value) in recordings_defaults {
            if !recordings.exists(key) {
                recordings.set(key, value, None);
            }
        }

        ini.section("Prompts");

        drop(ini);
        let _ = self.save();
    }

    pub fn save(&self) -> Result<(), String> {
        if self.load_failed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(
                "Settings file could not be read; refusing to overwrite it. Fix or restore RAMSettings.ini and restart.".to_string(),
            );
        }

        let ini = self.ini.lock().map_err(|e| e.to_string())?;
        ini.save(&self.file_path)
    }

    pub fn get_all(&self) -> Result<HashMap<String, HashMap<String, String>>, String> {
        let ini = self.ini.lock().map_err(|e| e.to_string())?;
        Ok(ini.to_map())
    }

    pub fn get(&self, section: &str, key: &str) -> Result<Option<String>, String> {
        let ini = self.ini.lock().map_err(|e| e.to_string())?;
        Ok(ini
            .get_section(section)
            .and_then(|s| s.get(key))
            .map(|v| v.to_string()))
    }

    pub fn get_bool(&self, section: &str, key: &str) -> bool {
        self.get(section, key)
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    pub fn get_int(&self, section: &str, key: &str) -> Option<i64> {
        self.get(section, key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
    }

    pub fn get_float(&self, section: &str, key: &str) -> Option<f64> {
        self.get(section, key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
    }

    pub fn get_string(&self, section: &str, key: &str) -> String {
        self.get(section, key).ok().flatten().unwrap_or_default()
    }

    pub fn set(&self, section: &str, key: &str, value: &str) -> Result<(), String> {
        let mut ini = self.ini.lock().map_err(|e| e.to_string())?;
        ini.section(section).set(key, value, None);
        drop(ini);
        self.save()
    }
}

#[cfg(test)]
mod settings_store_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("ram-settings-{name}-{nanos}.ini"))
    }

    struct TestStore {
        store: SettingsStore,
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.store.file_path);
        }
    }

    impl std::ops::Deref for TestStore {
        type Target = SettingsStore;
        fn deref(&self) -> &SettingsStore {
            &self.store
        }
    }

    fn fresh(name: &str) -> TestStore {
        TestStore {
            store: SettingsStore::new(temp_path(name)),
        }
    }

    fn from_existing(name: &str, contents: &str) -> TestStore {
        let path = temp_path(name);
        fs::write(&path, contents).expect("seed ini");
        TestStore {
            store: SettingsStore::new(path),
        }
    }

    /// Every key documented in `docs/features/settings.md` that has a non-empty
    /// backend default. Keys documented as "—" (no default) or as `""` are
    /// handled by their own tests below.
    fn documented_defaults() -> Vec<(&'static str, String, String)> {
        fn push(
            out: &mut Vec<(&'static str, String, String)>,
            section: &'static str,
            pairs: &[(&str, &str)],
        ) {
            for (key, value) in pairs {
                out.push((section, key.to_string(), value.to_string()));
            }
        }

        let mut out: Vec<(&'static str, String, String)> = Vec::new();

        push(
            &mut out,
            "General",
            &[
                ("CheckForUpdates", "true"),
                ("UpdaterReleaseChannel", "beta"),
                ("UpdaterFeatureChannel", crate::RUNNING_FEATURE_CHANNEL),
                ("AccountJoinDelay", "8"),
                ("AsyncJoin", "false"),
                ("WaitForGameJoin", "true"),
                ("DisableAgingAlert", "false"),
                ("HideUsernames", "false"),
                ("WrapLongNames", "false"),
                ("ServerRegionFormat", "<city>, <countryCode>"),
                ("MaxRecentGames", "8"),
                ("MaxRecentJobs", "12"),
                ("GroupOrder", "[]"),
                ("Language", "en"),
                ("AutoCookieRefresh", "true"),
                ("AutoCloseLastProcess", "false"),
                ("AutoCloseRobloxForMultiRbx", "false"),
                ("ShowPresence", "true"),
                ("ShowAccountNameOnWindow", "true"),
                ("PresenceUpdateRate", "5"),
                ("WarnOnOnlineJoin", "true"),
                ("WarnOnCopyCredential", "true"),
                ("CheckModerationBeforeLaunch", "true"),
                ("UnlockFPS", "false"),
                ("MaxFPSValue", "120"),
                ("OverrideClientVolume", "false"),
                ("ClientVolume", "0.5"),
                ("OverrideClientGraphics", "false"),
                ("ClientGraphicsLevel", "10"),
                ("OverrideClientWindowSize", "false"),
                ("ClientWindowWidth", "1280"),
                ("ClientWindowHeight", "720"),
                ("StartRobloxMinimized", "false"),
                ("AutoArrangeGrid", "true"),
                ("GridAllowSmallWindows", "false"),
                ("GridBorderless", "false"),
                ("StartOnPCStartup", "false"),
                ("MinimizeToTray", "false"),
                ("ThemeWindowsNavbar", "true"),
                ("ThemeWindowsNavbarAutoEnabledV1", "true"),
                ("RestrictedBackgroundStyle", "warp"),
                ("BottingEnabled", "false"),
                ("KeepPcAwake", "true"),
                ("AutoReconnect", "false"),
                ("BottingUseSharedClientProfile", "true"),
                ("BottingAutoShareLaunchFields", "true"),
                ("BottingDualPanelDialog", "true"),
                ("BottingDefaultIntervalMinutes", "19"),
                ("BottingLaunchDelaySeconds", "20"),
                ("BottingRetryMax", "6"),
                ("BottingRetryBaseSeconds", "8"),
                ("BottingPlayerGraceMinutes", "15"),
                ("EncryptionMethod", "default"),
                ("EncryptionOnboardingState", "pending"),
                ("FirstRunWalkthroughState", "pending"),
            ],
        );

        // The Botting client profiles mirror the Normal one key for key.
        let client_profile: &[(&str, &str)] = &[
            ("UnlockFPS", "false"),
            ("MaxFPSValue", "120"),
            ("OverrideClientVolume", "false"),
            ("ClientVolume", "0.5"),
            ("OverrideClientGraphics", "false"),
            ("ClientGraphicsLevel", "10"),
            ("OverrideClientWindowSize", "false"),
            ("ClientWindowWidth", "1280"),
            ("ClientWindowHeight", "720"),
            ("StartRobloxMinimized", "false"),
        ];
        for prefix in ["BottingPlayer", "BottingBot"] {
            for (key, value) in client_profile {
                out.push(("General", format!("{prefix}{key}"), value.to_string()));
            }
        }

        push(
            &mut out,
            "Developer",
            &[
                ("DevMode", "false"),
                ("EnableWebServer", "false"),
                ("IsTeleport", "false"),
                ("UseOldJoin", "false"),
            ],
        );

        push(
            &mut out,
            "WebServer",
            &[
                ("WebServerPort", "7963"),
                ("AllowGetCookie", "false"),
                ("AllowGetAccounts", "false"),
                ("AllowLaunchAccount", "false"),
                ("AllowAccountEditing", "false"),
                ("EveryRequestRequiresPassword", "false"),
                ("AllowExternalConnections", "false"),
            ],
        );

        push(
            &mut out,
            "AccountControl",
            &[
                ("AllowExternalConnections", "false"),
                ("StartOnLaunch", "false"),
                ("RelaunchDelay", "60"),
                ("LauncherDelay", "9"),
                ("NexusPort", "5242"),
                ("AutoMinimizeEnabled", "false"),
                ("AutoCloseEnabled", "false"),
                ("InternetCheck", "false"),
                ("UsePresence", "false"),
                ("AutoMinimizeInterval", "15"),
                ("AutoCloseInterval", "5"),
                ("MaxInstances", "3"),
                ("AutoCloseType", "0"),
            ],
        );

        push(
            &mut out,
            "Watcher",
            &[
                ("Enabled", "false"),
                ("ScanInterval", "6"),
                ("ReadInterval", "250"),
                ("ExitIfNoConnection", "false"),
                ("NoConnectionTimeout", "60"),
                ("ExitOnBeta", "false"),
                ("CloseIfNotResponding", "false"),
                ("CloseRbxMemory", "false"),
                ("MemoryLowValue", "200"),
                ("CloseRbxWindowTitle", "false"),
                ("ExpectedWindowTitle", "Roblox"),
                ("SaveWindowPositions", "false"),
            ],
        );

        // Three optimization profiles sharing the same 13 suffixes.
        let optimization: &[(&str, &str, &str, &str)] = &[
            ("EnableProcessPolicy", "false", "false", "false"),
            ("ProcessPolicyDelayMs", "1500", "1500", "1500"),
            ("PriorityClass", "normal", "normal", "below_normal"),
            ("BackgroundMode", "false", "false", "true"),
            ("EcoQos", "false", "false", "true"),
            ("IgnoreTimerResolution", "false", "false", "true"),
            ("MemoryPriority", "normal", "normal", "low"),
            ("EnableFastFlags", "false", "false", "false"),
            ("EnableJobCpuLimit", "false", "false", "false"),
            ("JobCpuLimitPercent", "25", "25", "20"),
            ("EnableJobMemoryLimit", "false", "false", "false"),
            ("JobMemoryLimitMb", "2048", "2048", "1536"),
        ];
        for (suffix, normal, player, bot) in optimization {
            out.push(("Optimization", format!("Normal{suffix}"), normal.to_string()));
            out.push((
                "Optimization",
                format!("BottingPlayer{suffix}"),
                player.to_string(),
            ));
            out.push(("Optimization", format!("BottingBot{suffix}"), bot.to_string()));
        }
        push(
            &mut out,
            "Optimization",
            &[("FollowFocus", "false"), ("MuteBackgroundClients", "false")],
        );

        push(
            &mut out,
            "Versions",
            &[
                ("MaxParallelDownloads", "4"),
                ("CatalogCacheMinutes", "10"),
                ("PreferOldJoinForVersioned", "true"),
                ("ShowPreReleaseVersions", "false"),
                ("AllowLaunchOnOpenVersion", "false"),
            ],
        );

        push(
            &mut out,
            "Isolation",
            &[
                ("Mode", "Off"),
                ("SpoofMachineGuid", "false"),
                ("SpoofMacAddress", "false"),
                ("IncludeStudio", "false"),
                ("PreserveFastFlags", "true"),
                ("PreserveBasicSettings", "true"),
            ],
        );

        push(
            &mut out,
            "Login",
            &[("PersistentProfile", "true"), ("StealthMode", "true")],
        );

        // AFK mode. `Afk.Key` nasce vazia de proposito (sem tecla escolhida o
        // modo nao liga), entao ela mora em `EMPTY_STRING_DEFAULTS`, nao aqui —
        // ver docs/features/afk-mode.md.
        push(
            &mut out,
            "Afk",
            &[
                ("IntervalMinutes", "10"),
                ("IntervalSeconds", "0"),
                ("BeepOnCycle", "false"),
                ("Mode", "key"),
                ("ClickX", "50"),
                ("ClickY", "50"),
            ],
        );

        push(
            &mut out,
            "Recordings",
            &[("AfterReconnect", "false"), ("AfterReconnectDelaySeconds", "30")],
        );

        push(
            &mut out,
            "Generator",
            &[
                ("Provider", "bloxgen"),
                ("ExtraDelaySeconds", "1"),
                ("TargetGroup", "BloxGen"),
                ("MaxAccounts", "0"),
                ("MaxConsecutiveFailures", "3"),
            ],
        );

        push(
            &mut out,
            "BloxGen",
            &[
                ("Endpoint", "https://core.bloxgen.net"),
                ("AccountType", "alt"),
            ],
        );

        push(
            &mut out,
            "Linux",
            &[
                ("PreferredRunner", "sober"),
                (
                    "CustomProcessMatch",
                    "sober,flatpak,roblox,robloxplayerbeta",
                ),
                ("EnableExperimentalMultiRbx", "false"),
                ("WindowControlBackend", "auto"),
            ],
        );

        out
    }

    /// Keys the docs list with a `""` default. `IniSection::set` removes a key
    /// whose value is blank, so these never reach the file at all.
    const EMPTY_STRING_DEFAULTS: &[(&str, &str)] = &[
        ("Login", "ManualBinaryPath"),
        ("General", "CustomClientSettings"),
        ("General", "BottingPlayerCustomClientSettings"),
        ("General", "BottingBotCustomClientSettings"),
        ("General", "BottingDraftPlaceId"),
        ("General", "BottingDraftJobId"),
        ("General", "BottingDraftLaunchData"),
        ("General", "BottingDraftPlayerAccountId"),
        ("General", "BottingDraftPlayerAccountIds"),
        ("General", "BottingDraftSelectedUserIds"),
        ("Generator", "SignupUsernamePrefix"),
        ("Optimization", "NormalFastFlagsJson"),
        ("Optimization", "BottingPlayerFastFlagsJson"),
        ("Optimization", "BottingBotFastFlagsJson"),
        ("Versions", "DefaultVersion"),
        ("Isolation", "TargetAdapter"),
        ("Isolation", "BackupMachineGuid"),
        ("Isolation", "BackupNetworkAddress"),
        ("Isolation", "BackupAdapterId"),
        ("Afk", "Key"),
        ("BloxGen", "ApiKey"),
        ("Linux", "CustomLaunchCommand"),
        ("Linux", "CustomLogDir"),
    ];

    // ---- defaults --------------------------------------------------------------

    #[test]
    fn every_documented_default_is_applied_on_a_fresh_install() {
        let s = fresh("defaults");
        let all = s.get_all().unwrap();

        let mut missing = Vec::new();
        let mut wrong = Vec::new();
        for (section, key, expected) in documented_defaults() {
            match all.get(section).and_then(|sec| sec.get(&key)) {
                None => missing.push(format!("{section}.{key}")),
                Some(actual) if actual != &expected => {
                    wrong.push(format!("{section}.{key}: {actual:?} != {expected:?}"))
                }
                Some(_) => {}
            }
        }
        assert!(missing.is_empty(), "missing defaults: {missing:?}");
        assert!(wrong.is_empty(), "wrong defaults: {wrong:?}");
    }

    #[test]
    fn the_store_applies_no_defaults_beyond_the_documented_ones() {
        let s = fresh("defaults-extra");
        let all = s.get_all().unwrap();

        let documented: std::collections::HashSet<String> = documented_defaults()
            .into_iter()
            .map(|(section, key, _)| format!("{section}.{key}"))
            .collect();

        let mut undocumented = Vec::new();
        for (section, keys) in &all {
            for key in keys.keys() {
                let id = format!("{section}.{key}");
                if !documented.contains(&id) {
                    undocumented.push(id);
                }
            }
        }
        undocumented.sort();
        assert!(
            undocumented.is_empty(),
            "undocumented defaults (update docs/features/settings.md): {undocumented:?}"
        );
    }

    /// O intervalo do AFK mode ganhou segundos: quem já tinha `IntervalMinutes`
    /// continua com os minutos dele e ganha `IntervalSeconds = 0` — o mesmo
    /// intervalo de antes.
    #[test]
    fn an_existing_afk_interval_keeps_its_minutes_and_gains_zero_seconds() {
        let s = from_existing("afk-seconds", "[Afk]
IntervalMinutes=25
Key=Space
");
        assert_eq!(s.get("Afk", "IntervalMinutes").unwrap().as_deref(), Some("25"));
        assert_eq!(s.get("Afk", "IntervalSeconds").unwrap().as_deref(), Some("0"));
    }

    #[test]
    fn defaults_documented_as_an_empty_string_are_never_written() {
        // `IniSection::set` treats a blank value as a removal, so these keys are
        // absent from a fresh RAMSettings.ini despite being listed with a `""`
        // default in the docs. Consumers must keep their own fallback.
        let s = fresh("defaults-empty");
        let all = s.get_all().unwrap();
        for (section, key) in EMPTY_STRING_DEFAULTS {
            assert_eq!(
                s.get(section, key).unwrap(),
                None,
                "{section}.{key} unexpectedly has a value"
            );
            assert!(
                all.get(*section).map(|sec| !sec.contains_key(*key)).unwrap_or(true),
                "{section}.{key} unexpectedly present in get_all"
            );
            assert_eq!(s.get_string(section, key), "");
        }
    }

    #[test]
    fn an_empty_prompts_section_is_created_but_not_persisted() {
        let s = fresh("prompts");
        // It exists in memory (get_all maps every section)...
        assert_eq!(
            s.get_all().unwrap().get("Prompts").map(|sec| sec.len()),
            Some(0)
        );
        // ...but an empty section is skipped by IniFile::save.
        assert!(!fs::read_to_string(&s.file_path).unwrap().contains("[Prompts]"));

        // Writing a key into it makes the section real.
        s.set("Prompts", "SomePrompt", "seen").unwrap();
        assert_eq!(s.get("Prompts", "SomePrompt").unwrap().as_deref(), Some("seen"));
        assert!(fs::read_to_string(&s.file_path).unwrap().contains("[Prompts]"));
    }

    /// Trocar de edição grava `nexus-ws` (ou `standard`) no INI da pasta de
    /// dados. A versão nova, ao abrir, não pode devolver o default por cima —
    /// senão a próxima checagem voltaria para a outra edição.
    #[test]
    fn the_chosen_update_edition_survives_a_restart_of_either_edition() {
        for chosen in ["nexus-ws", "standard"] {
            let s = from_existing(
                &format!("edition-keep-{chosen}"),
                &format!("[General]
UpdaterFeatureChannel={chosen}
"),
            );
            assert_eq!(s.get_string("General", "UpdaterFeatureChannel"), chosen);
            let reopened = SettingsStore::new(s.file_path.clone());
            assert_eq!(reopened.get_string("General", "UpdaterFeatureChannel"), chosen);
        }
    }

    #[test]
    fn a_fresh_install_follows_the_running_edition_for_updates() {
        let s = fresh("edition-default");
        assert_eq!(
            s.get_string("General", "UpdaterFeatureChannel"),
            crate::RUNNING_FEATURE_CHANNEL
        );
    }

    #[test]
    fn existing_values_are_never_overwritten_by_defaults() {
        let s = from_existing(
            "defaults-keep",
            "[General]\nLanguage=de\nMaxRecentGames=42\nThemeWindowsNavbarAutoEnabledV1=true\n\
             [Watcher]\nEnabled=true\nExpectedWindowTitle=My Window\n\
             [Optimization]\nBottingBotPriorityClass=idle\n",
        );

        assert_eq!(s.get_string("General", "Language"), "de");
        assert_eq!(s.get_string("General", "MaxRecentGames"), "42");
        assert_eq!(s.get_string("Watcher", "Enabled"), "true");
        assert_eq!(s.get_string("Watcher", "ExpectedWindowTitle"), "My Window");
        assert_eq!(s.get_string("Optimization", "BottingBotPriorityClass"), "idle");
        // Untouched keys still receive their defaults.
        assert_eq!(s.get_string("General", "CheckForUpdates"), "true");
        assert_eq!(s.get_string("Watcher", "ScanInterval"), "6");
    }

    #[test]
    fn onboarding_flags_are_pending_on_a_new_install_and_completed_on_an_upgrade() {
        let new_install = fresh("onboarding-new");
        assert_eq!(
            new_install.get_string("General", "EncryptionOnboardingState"),
            "pending"
        );
        assert_eq!(
            new_install.get_string("General", "FirstRunWalkthroughState"),
            "pending"
        );

        // An INI that already existed means the user is upgrading, not installing.
        let upgrade = from_existing("onboarding-upgrade", "[General]\nLanguage=en\n");
        assert_eq!(
            upgrade.get_string("General", "EncryptionOnboardingState"),
            "completed"
        );
        assert_eq!(
            upgrade.get_string("General", "FirstRunWalkthroughState"),
            "completed"
        );

        // A stored value always wins over both branches.
        let stored = from_existing(
            "onboarding-stored",
            "[General]\nEncryptionOnboardingState=pending\nFirstRunWalkthroughState=skipped\n",
        );
        assert_eq!(
            stored.get_string("General", "EncryptionOnboardingState"),
            "pending"
        );
        assert_eq!(
            stored.get_string("General", "FirstRunWalkthroughState"),
            "skipped"
        );
    }

    #[test]
    fn the_navbar_migration_forces_the_flag_on_once_and_then_respects_the_user() {
        // No marker yet: ThemeWindowsNavbar is forced back to true.
        let migrated = from_existing(
            "navbar-migrate",
            "[General]\nThemeWindowsNavbar=false\n",
        );
        assert_eq!(migrated.get_string("General", "ThemeWindowsNavbar"), "true");
        assert_eq!(
            migrated.get_string("General", "ThemeWindowsNavbarAutoEnabledV1"),
            "true"
        );

        // Marker present: the user's choice is kept.
        let respected = from_existing(
            "navbar-respected",
            "[General]\nThemeWindowsNavbar=false\nThemeWindowsNavbarAutoEnabledV1=true\n",
        );
        assert_eq!(respected.get_string("General", "ThemeWindowsNavbar"), "false");
    }

    #[test]
    fn apply_defaults_writes_the_file_immediately_and_it_reloads_identically() {
        let s = fresh("persist-defaults");
        assert!(s.file_path.exists(), "the INI is created on first start");

        let reopened = SettingsStore::new(s.file_path.clone());
        assert_eq!(reopened.get_all().unwrap(), s.get_all().unwrap());
        // Reopening an existing file must not flip the onboarding flags.
        assert_eq!(
            reopened.get_string("General", "EncryptionOnboardingState"),
            "pending"
        );
    }

    #[test]
    fn the_legacy_rbx_alt_manager_section_is_migrated_on_load() {
        let s = from_existing(
            "legacy-section",
            "[RBX Alt Manager]\nLanguage=de\nMaxRecentGames=3\n",
        );
        assert!(!s.get_all().unwrap().contains_key("RBX Alt Manager"));
        assert_eq!(s.get_string("Roblox Account Manager", "Language"), "de");
        // The values did NOT move into [General]; the defaults still apply there.
        assert_eq!(s.get_string("General", "Language"), "en");
    }

    // ---- typed accessors --------------------------------------------------------

    #[test]
    fn get_returns_none_for_unknown_sections_and_keys() {
        let s = fresh("get-missing");
        assert_eq!(s.get("NoSuchSection", "Key").unwrap(), None);
        assert_eq!(s.get("General", "NoSuchKey").unwrap(), None);
        assert_eq!(s.get("General", "language").unwrap(), None, "keys are case sensitive");
        assert_eq!(s.get("general", "Language").unwrap(), None, "sections too");
    }

    #[test]
    fn get_bool_is_true_only_for_the_exact_string_true() {
        let s = fresh("get-bool");
        assert!(s.get_bool("General", "CheckForUpdates"));
        assert!(!s.get_bool("General", "AsyncJoin"));
        assert!(!s.get_bool("General", "NoSuchKey"));
        assert!(!s.get_bool("NoSuchSection", "NoSuchKey"));

        for value in ["True", "TRUE", "1", "yes", "on", " true"] {
            s.set("General", "Probe", value).unwrap();
            assert!(!s.get_bool("General", "Probe"), "{value:?} must not be true");
        }
        s.set("General", "Probe", "true").unwrap();
        assert!(s.get_bool("General", "Probe"));
    }

    #[test]
    fn get_int_and_get_float_return_none_when_the_value_does_not_parse() {
        let s = fresh("get-numbers");
        assert_eq!(s.get_int("General", "MaxRecentGames"), Some(8));
        assert_eq!(s.get_int("General", "ClientVolume"), None, "0.5 is not an int");
        assert_eq!(s.get_int("General", "Language"), None);
        assert_eq!(s.get_int("General", "NoSuchKey"), None);

        assert_eq!(s.get_float("General", "ClientVolume"), Some(0.5));
        assert_eq!(s.get_float("General", "MaxRecentGames"), Some(8.0));
        assert_eq!(s.get_float("General", "Language"), None);

        s.set("General", "Probe", "-12").unwrap();
        assert_eq!(s.get_int("General", "Probe"), Some(-12));
        // `set` stores the value verbatim, but the INI parser trims it on the
        // way back in, so a padded value changes meaning after a restart.
        s.set("General", "Probe", " 12 ").unwrap();
        assert_eq!(s.get("General", "Probe").unwrap().as_deref(), Some(" 12 "));
        assert_eq!(s.get_int("General", "Probe"), None);
        let reopened = SettingsStore::new(s.file_path.clone());
        assert_eq!(reopened.get("General", "Probe").unwrap().as_deref(), Some("12"));
        assert_eq!(reopened.get_int("General", "Probe"), Some(12));
    }

    #[test]
    fn get_string_falls_back_to_an_empty_string() {
        let s = fresh("get-string");
        assert_eq!(s.get_string("General", "Language"), "en");
        assert_eq!(s.get_string("General", "NoSuchKey"), "");
        assert_eq!(s.get_string("NoSuchSection", "NoSuchKey"), "");
    }

    // ---- set --------------------------------------------------------------------

    #[test]
    fn set_creates_sections_updates_values_and_persists_every_time() {
        let s = fresh("set");
        s.set("BrandNew", "Key", "value").unwrap();
        assert_eq!(s.get("BrandNew", "Key").unwrap().as_deref(), Some("value"));

        s.set("BrandNew", "Key", "changed").unwrap();
        assert_eq!(s.get("BrandNew", "Key").unwrap().as_deref(), Some("changed"));
        assert_eq!(
            s.get_all().unwrap()["BrandNew"].len(),
            1,
            "updating must not duplicate the key"
        );

        let reopened = SettingsStore::new(s.file_path.clone());
        assert_eq!(reopened.get_string("BrandNew", "Key"), "changed");
    }

    #[test]
    fn set_with_a_blank_value_removes_the_key_instead_of_storing_it() {
        let s = fresh("set-blank");
        s.set("General", "SavedPlaceId", "606849621").unwrap();
        assert_eq!(s.get_string("General", "SavedPlaceId"), "606849621");

        s.set("General", "SavedPlaceId", "").unwrap();
        assert_eq!(s.get("General", "SavedPlaceId").unwrap(), None);

        s.set("General", "SavedPlaceId", "1").unwrap();
        s.set("General", "SavedPlaceId", "   ").unwrap();
        assert_eq!(s.get("General", "SavedPlaceId").unwrap(), None);

        let reopened = SettingsStore::new(s.file_path.clone());
        assert_eq!(reopened.get("General", "SavedPlaceId").unwrap(), None);
    }

    #[test]
    fn set_does_not_validate_anything_it_is_given() {
        // Documented trap: update_setting writes any section/key/value pair.
        let s = fresh("set-unvalidated");
        s.set("Script.my-script", "Weird Key", "value with spaces = and equals")
            .unwrap();
        s.set("WebServer", "Password", "plain-text-secret").unwrap();

        let reopened = SettingsStore::new(s.file_path.clone());
        assert_eq!(
            reopened.get_string("Script.my-script", "Weird Key"),
            "value with spaces = and equals"
        );
        assert_eq!(
            reopened.get_string("WebServer", "Password"),
            "plain-text-secret",
            "secrets are stored in clear text"
        );
    }

    /// A ordem manual dos grupos e gravada em JSON porque nome de grupo e texto
    /// livre: um grupo chamado "Alts, velhas" quebraria uma lista por virgula.
    /// O risco real fica no INI, que corta a linha no primeiro `=` — este teste
    /// prova que o JSON volta inteiro do arquivo.
    #[test]
    fn group_order_json_survives_the_ini_round_trip_even_with_commas() {
        let s = fresh("group-order-round-trip");
        let value = r#"["Alts, velhas","5 Mains","Zeta"]"#;
        s.set("General", "GroupOrder", value).unwrap();

        let reopened = SettingsStore::new(s.file_path.clone());
        assert_eq!(reopened.get_string("General", "GroupOrder"), value);
    }

    /// Armadilha documentada do INI: valor vazio **apaga** a chave (`IniSection::set`).
    /// Como `GroupOrder` tem default, apagar nao deixa a chave ausente: o default
    /// `[]` volta no proximo boot, e `[]` significa "sem ordem manual". De um jeito
    /// ou de outro, quem le nunca recebe erro.
    #[test]
    fn an_empty_group_order_falls_back_to_the_default_instead_of_erroring() {
        let s = fresh("group-order-empty");
        s.set("General", "GroupOrder", r#"["Zeta"]"#).unwrap();
        s.set("General", "GroupOrder", "").unwrap();
        assert_eq!(s.get_string("General", "GroupOrder"), "", "a chave sai do arquivo");

        let reopened = SettingsStore::new(s.file_path.clone());
        assert_eq!(reopened.get_string("General", "GroupOrder"), "[]");
    }

    #[test]
    fn get_all_exposes_every_section_as_a_flat_string_map() {
        let s = fresh("get-all");
        let all = s.get_all().unwrap();
        for section in [
            "General",
            "Developer",
            "WebServer",
            "AccountControl",
            "Watcher",
            "Optimization",
            "Linux",
            "Generator",
            "BloxGen",
            "Versions",
            "Isolation",
            "Login",
            "Afk",
        ] {
            assert!(all.contains_key(section), "missing section {section}");
            assert!(!all[section].is_empty(), "empty section {section}");
        }
        assert_eq!(all["General"]["Language"], "en");
    }

    // ---- save --------------------------------------------------------------------

    #[test]
    fn save_reports_an_error_when_the_file_cannot_be_written() {
        let s = fresh("save-error");
        let dir_path = s.file_path.with_extension("dir");
        fs::create_dir_all(&dir_path).unwrap();

        let blocked = SettingsStore {
            ini: Mutex::new(IniFile::new()),
            file_path: dir_path.clone(),
            load_failed: std::sync::atomic::AtomicBool::new(false),
        };
        {
            let mut ini = blocked.ini.lock().unwrap();
            ini.section("General").set("Language", "en", None);
        }
        let err = blocked.save().expect_err("writing over a directory must fail");
        assert!(err.starts_with("Failed to save INI file:"), "{err}");

        let _ = fs::remove_dir_all(&dir_path);
    }

    #[test]
    fn an_unreadable_settings_file_is_never_overwritten() {
        // `IniFile::load` engole erros de I/O, então um RAMSettings.ini ilegível
        // virava um INI vazio e `apply_defaults` gravava só os defaults por cima
        // das settings do usuário. Agora a falha fica latcheada e `save` recusa.
        let path = temp_path("unreadable");
        // Bytes que não são UTF-8 fazem `read_to_string` falhar.
        let broken = b"[General]\nLanguage=\xff\xfe\xfd\n";
        fs::write(&path, broken).unwrap();

        let store = SettingsStore::new(path.clone());
        let err = store
            .save()
            .expect_err("gravar sobre um INI ilegível deve falhar");
        assert!(err.contains("refusing to overwrite"), "{err}");
        assert_eq!(
            fs::read(&path).unwrap(),
            broken.to_vec(),
            "nem o construtor nem o save tocam no arquivo do usuário"
        );

        let _ = fs::remove_file(&path);
    }
}
