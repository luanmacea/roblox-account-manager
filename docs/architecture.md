# Arquitetura

## Visão geral

```mermaid
flowchart LR
    subgraph Frontend["Frontend (WebView — React/TS)"]
        UI[Componentes<br/>src/components]
        Store[StoreProvider<br/>src/store.tsx]
        Worker[Web Worker de scripts<br/>src/scripting]
        UI --> Store
        Worker -. host-request .-> UI
    end

    subgraph Backend["Backend (Rust — Tauri 2)"]
        Cmd[Comandos Tauri<br/>commands/*, data/*, chromium/*]
        Stores[(State gerenciado:<br/>AccountStore, SettingsStore,<br/>ThemeStore, ScriptStore,<br/>VersionsCatalogStore, ImageCache...)]
        Api[api/auth.rs + api/roblox/*]
        Plat[platform/windows · platform/macos]
        WS[[api/server — axum<br/>feature webserver]]
        NX[[nexus — WebSocket<br/>feature nexus]]
        Cmd --> Stores
        Cmd --> Api
        Cmd --> Plat
        WS --> Stores
        NX --> Cmd
    end

    Store -- "invoke(cmd, args)" --> Cmd
    Cmd -- "app.emit(evento, payload)" --> Store
    Api -- HTTPS --> Roblox[(APIs Roblox)]
    Stores -- leitura/escrita --> Files[(AccountData.json<br/>RAMSettings.ini<br/>RAMTheme.ini<br/>RAMScripts.json<br/>RAMAvatars.json<br/>RAMGameLists.json<br/>RAMVersions.json)]
    Plat --> RbxProc[Processos RobloxPlayerBeta]
```

## Divisão frontend / backend

- **Frontend** ([src/](../src)): React 19 + Vite. Todo o estado global fica em um único Context em [store.tsx](../src/store.tsx) (`StoreProvider`/`useStore`), que chama o backend com `invoke(...)` de `@tauri-apps/api/core` e escuta eventos com `listen(...)`.
- **Backend** ([src-tauri/src/](../src-tauri/src)): ponto de entrada em [main.rs](../src-tauri/src/main.rs) → `run()` em [lib.rs](../src-tauri/src/lib.rs).
- **IPC**: exclusivamente via comandos Tauri registrados em `tauri::generate_handler![...]` em [lib.rs](../src-tauri/src/lib.rs). Todos retornam `Result<T, String>`; o erro chega ao frontend como string (normalmente exibida via `setError`/toast).
- Os nomes de argumentos em Rust são `snake_case` e no `invoke` do frontend são `camelCase` (ex.: `user_id` ↔ `{ userId }` em [store.tsx](../src/store.tsx)).

### Exceções à regra "frontend não acessa rede"

A regra do projeto é que o frontend fala só com o backend, mas o código tem exceções reais:

| Onde | O quê |
|---|---|
| [UpdateDialog.tsx](../src/components/dialogs/UpdateDialog.tsx), [releaseNotes.ts](../src/releaseNotes.ts) | `fetch` direto em `api.github.com` (`REPO_API_URL` de [repo.ts](../src/repo.ts)), só leitura e sem login: a janela de atualização lê as notas da release e o comparativo entre versões; a página "What's new" ([ChangelogPage](../src/components/pages/ChangelogPage.tsx)) lê a lista de releases, só quando é aberta e uma vez por sessão. |
| [ScriptsPage.tsx](../src/components/pages/ScriptsPage.tsx) | `fetch`/`WebSocket` em nome de scripts do usuário (`ram.http`, `ram.ws`), com permissão explícita. |
| [fontPresets.ts](../src/fontPresets.ts) | Carrega fontes de `fonts.googleapis.com`. |
| [server-list/gameListsSync.ts](../src/components/server-list/gameListsSync.ts) | Favoritos, jogos recentes e servidores recentes têm um **cache** em `localStorage` (`ram_favorite_games`, `ram_recent_games`, `ram_recent_jobs`) para as telas lerem e gravarem sem `await`. Desde 03/10/2026 **não** moram só ali: a cópia durável é o `RAMGameLists.json` do backend (`get_game_lists`/`save_game_lists`), que entra no backup e na migração de pasta. Cada gravação é espelhada; a abertura e a restauração de backup hidratam por união (ver [server-list.md](features/server-list.md#onde-as-listas-moram)). A versão que o usuário mandou pular no updater segue só no `localStorage` (`getUpdaterSkipVersionKey`, em [server-list/types.ts](../src/components/server-list/types.ts)). |
| [NavSidebar.tsx](../src/components/layout/NavSidebar.tsx) | Barra lateral recolhida ou não: `localStorage` (`ram_nav_collapsed`), preferência de quem está na máquina, com `try/catch` (armazenamento bloqueado só faz a escolha valer até fechar o app). |
| [tour/tourState.ts](../src/components/tour/tourState.ts) | Quais tutoriais de tela a pessoa já abriu: `localStorage` (`ram_tours_seen`), só para o pontinho de "novo" no botão Tutorial, com `try/catch`. |

A região de um servidor **não** é mais exceção: o frontend chama `get_server_regions` e o backend faz a geolocalização ([server-choice.md](features/server-choice.md)).

## Organização do backend

- Os arquivos em `src-tauri/src/commands/*.rs` **não são módulos**: são inseridos em `lib.rs` via `include!("commands/xxx.rs")`. Por isso as funções ficam na raiz do crate e compartilham imports/helpers (ex.: `get_cookie` de [account_helpers.rs](../src-tauri/src/commands/account_helpers.rs), `run_with_session_retry` de [account_api.rs](../src-tauri/src/commands/account_api.rs)).
- O mesmo padrão aparece em `data/accounts.rs` e `data/settings.rs` (incluem `model.rs`, `store.rs`, `commands.rs` etc.) e em `api/roblox.rs`.
- `data/*`: stores e persistência. `api/*`: clientes HTTP (Roblox) e servidor HTTP local. `chromium/*`: navegador de login via CDP. `platform/*`: código específico de SO. `nexus/*`: WebSocket.

## Stores (estado no backend)

Registradas com `.manage(...)` em [lib.rs](../src-tauri/src/lib.rs) e acessadas nos comandos via `tauri::State<'_, T>`.

| Store | Definição | Estado interno | Arquivo |
|---|---|---|---|
| `AccountStore` | [data/accounts/store.rs](../src-tauri/src/data/accounts/store.rs) | `Mutex<Vec<Account>>` + `Mutex<Option<SessionKey>>` (segredo da sessão: senha do usuário ou chave do aparelho) | `AccountData.json` + `AccountData.key` |
| `SettingsStore` | [data/settings/store.rs](../src-tauri/src/data/settings/store.rs) | `Mutex<IniFile>` | `RAMSettings.ini` |
| `ThemeStore` | [data/settings/theme.rs](../src-tauri/src/data/settings/theme.rs) | tema atual | `RAMTheme.ini` |
| `ThemePresetStore` | [data/settings/presets.rs](../src-tauri/src/data/settings/presets.rs) | `Mutex<Vec<ThemePresetData>>` | `RAMThemePresets.json` |
| `ScriptStore` | [data/scripts.rs](../src-tauri/src/data/scripts.rs) | `Mutex<Vec<ManagedScript>>` | `RAMScripts.json` |
| `AvatarStore` | [data/avatars.rs](../src-tauri/src/data/avatars.rs) | `Mutex<Vec<SavedAvatar>>` | `RAMAvatars.json` |
| `GameListsStore` | [data/game_lists.rs](../src-tauri/src/data/game_lists.rs) | só um `Mutex<()>` que serializa as gravações; relê o disco a cada leitura (por isso a restauração de backup não pede reinício) | `RAMGameLists.json` (+ `.bak`) |
| `LaunchPresetStore` | [data/launch_presets.rs](../src-tauri/src/data/launch_presets.rs) | só um `Mutex<()>`; relê o disco a cada leitura (restauração de backup vale sem reinício) | `RAMLaunchPresets.json` (+ `.bak`) |
| `SessionHistoryStore` | [data/session_history.rs](../src-tauri/src/data/session_history.rs) | só o contador de linhas desde a última compactação; lê o disco a cada consulta e grava por append | `RAMSessionHistory.jsonl` (+ `.bak`, `RAMSessionHistory.open.json`) |
| `VersionsCatalogStore` | [data/versions.rs](../src-tauri/src/data/versions.rs) | catálogo de versões instaladas | `RAMVersions.json` |
| `ImageCache` | [api/batch.rs](../src-tauri/src/api/batch.rs) | `Arc<Mutex<...>>` (filas + cache de URLs) | memória |
| `UpdaterRuntimeState` | [commands/updater.rs](../src-tauri/src/commands/updater.rs) | estado do updater | memória |
| `ChromiumManager` | [chromium/manager.rs](../src-tauri/src/chromium/manager.rs) | processos Chromium de login | memória |

Observação: as stores de dados usam `Mutex<_>` simples; o `Arc` fica por conta do `State` do Tauri. Apenas `ImageCache` usa `Arc<Mutex<_>>` explicitamente. Todas as mutações de `AccountStore`, `SettingsStore` e `ScriptStore` regravam o arquivo inteiro imediatamente. O `AccountData.json` é gravado de forma atômica (`.json.tmp` + `atomic_replace`/`MoveFileExW`) e o `AccountStore` recusa gravar se o load inicial falhou ou se estiver bloqueado com arquivo criptografado (ver [accounts.md](features/accounts.md#regras-de-negócio)).

## Arquivos de persistência

O diretório base é a **pasta de dados do usuário**, resolvida uma vez por processo em `get_runtime_data_dir()` ([data/settings/paths.rs](../src-tauri/src/data/settings/paths.rs)), nesta ordem:

1. variável de ambiente `RAM_DATA_DIR` (pasta própria; usada também nos testes);
2. **modo portátil** — arquivo `portable.txt` ao lado do executável → a pasta do executável (comportamento das versões antigas, útil para pendrive);
3. `%LOCALAPPDATA%\Roblox Account Manager` no Windows (`~/Library/Application Support/...` no macOS, `$XDG_DATA_HOME` no Linux);
4. pasta do executável, se o perfil do usuário não existir.

**Migração:** na primeira execução, os arquivos que estavam ao lado do executável são **copiados** para a pasta nova (`migrate_data_files`). A cópia nunca sobrescreve um arquivo já existente no destino e **nunca apaga a origem** — voltar para uma versão antiga do app continua funcionando.

| Arquivo | Onde | Formato | Código |
|---|---|---|---|
| `AccountData.json` | pasta de dados | Binário criptografado com header RAM (senha do usuário ou chave do aparelho); JSON puro em PascalCase é **lido** para migrar arquivos de RAM v3/v4. **Exceção:** se a chave do aparelho não pôde ser criada (disco cheio, antivírus), o store segue sem segredo e **grava JSON puro** — a faixa `VaultKeyBanner` avisa (ver [accounts.md](features/accounts.md#regras-de-negócio)) | [data/accounts/commands.rs](../src-tauri/src/data/accounts/commands.rs) `get_account_data_path` |
| `AccountData.key` | pasta de dados, ao lado do vault | JSON com a chave mestra de 32 bytes embrulhada duas vezes (DPAPI do usuário + hash do aparelho). Existe só quando **não** há senha de usuário | [data/vault_key.rs](../src-tauri/src/data/vault_key.rs) `key_file_path_for` |
| `RAMSettings.ini` | pasta de dados | INI | [paths.rs](../src-tauri/src/data/settings/paths.rs) `get_settings_path` |
| `RAMTheme.ini` | pasta de dados | INI (seção `Roblox Account Manager`, fallback `RBX Alt Manager`) | [paths.rs](../src-tauri/src/data/settings/paths.rs), [theme.rs](../src-tauri/src/data/settings/theme.rs) |
| `RAMThemePresets.json` | pasta de dados | JSON | [paths.rs](../src-tauri/src/data/settings/paths.rs) |
| `RAMThemeFonts/` | pasta de dados | fontes importadas, nomeadas por SHA-256 | [commands.rs](../src-tauri/src/data/settings/commands.rs) `import_theme_font_asset` |
| `RAMScripts.json` | pasta de dados | JSON (camelCase) | [data/scripts.rs](../src-tauri/src/data/scripts.rs) `get_scripts_path` |
| `RAMAvatars.json` | pasta de dados | JSON (camelCase), escrita atômica via `.json.tmp` | [data/avatars.rs](../src-tauri/src/data/avatars.rs) `get_avatars_path` |
| `RAMGameLists.json` | pasta de dados | JSON (camelCase) `{ favorites, recentGames, recentJobs }`, itens opacos ao Rust (o formato é do frontend). Escrita atômica via `.json.tmp`; a versão anterior fica em `RAMGameLists.json.bak` a cada gravação. Arquivo ilegível trava a gravação; gravação que zera todos os favoritos ou todos os VIPs só passa com `allowDestructive` (exclusão pedida pelo usuário) | [data/game_lists.rs](../src-tauri/src/data/game_lists.rs) `get_game_lists_path` |
| `RAMRecordings.json` | pasta de dados | JSON (camelCase) `{ version, recordings: [{ id, name, steps }], defaultId, accountIds }`; passos com `type` (`key`, `keyDown`, `keyUp`, `click`, `wait`). Escrita atômica, versão anterior em `.bak`; arquivo ilegível trava a gravação | [data/recordings.rs](../src-tauri/src/data/recordings.rs) — ver [recordings.md](features/recordings.md) |
| `RAMLaunchPresets.json` | pasta de dados | JSON (camelCase) `{ presets: [...] }`: contas, place, job/`vip:<código>`, grade e horário. Escrita atômica via `.json.tmp`, versão anterior em `.bak`; arquivo ilegível trava a gravação | [data/launch_presets.rs](../src-tauri/src/data/launch_presets.rs) — ver [presets.md](features/presets.md) |
| `RAMSessionHistory.jsonl` | pasta de dados | uma linha JSON (camelCase) por evento de sessão — `joined`, `teleported`, `left`, `dropped`, `closed`, `appClosed`, `moderated` —, gravada na hora; 90 dias / 50 000 eventos, compactação atômica com `.bak`. O checkpoint `RAMSessionHistory.open.json` (fora do backup) fecha sessões deixadas abertas por um fechamento à força | [data/session_history.rs](../src-tauri/src/data/session_history.rs) — ver [history.md](features/history.md) |
| `RAMVersions.json` | `%LOCALAPPDATA%\Roblox Account Manager\` (se `LOCALAPPDATA` não existir: pasta do exe) | JSON, escrita atômica via `.json.tmp` | [data/versions.rs](../src-tauri/src/data/versions.rs) `get_versions_catalog_path` |
| `RobloxVersions/` | `%LOCALAPPDATA%\Roblox Account Manager\` | versões do cliente instaladas | [data/versions.rs](../src-tauri/src/data/versions.rs) `ram_managed_versions_root` |
| `AccountControlData.json` | pasta de dados | JSON (lista de contas do Nexus) | [nexus/websocket/server_impl.rs](../src-tauri/src/nexus/websocket/server_impl.rs) `data_path` |
| `backups/*.zip` | pasta de dados | zip com os arquivos acima + manifesto | [commands/backups.rs](../src-tauri/src/commands/backups.rs) |
| `AccountData.json.bak` | pasta de dados | **texto puro, com os cookies legíveis**: a cópia que a migração para o formato cifrado deixa antes de regravar. Não entra no zip de backup; fica até alguém apagar | [data/accounts/store.rs](../src-tauri/src/data/accounts/store.rs) `migrate_plain_vault` — ver [accounts.md](features/accounts.md#migração-de-accountdatajson-em-texto-puro) |
| `RAMUnlock.bin` | pasta de dados | senha do "lembrar de mim", cifrada pelo DPAPI do usuário, com o prazo dentro do blob; só existe se o usuário marcar a caixa | [data/accounts/remember.rs](../src-tauri/src/data/accounts/remember.rs) |
| `webview.safemode` | pasta de dados, ao lado do `RAMSettings.ini` | marcador do safe mode de vídeo do WebView2 | [webview_recovery.rs](../src-tauri/src/webview_recovery.rs) — ver [webview-recovery.md](features/webview-recovery.md) |
| `ServerRegionCache.json` | pasta de dados | cache IP → região dos servidores | [api/roblox/server_regions.rs](../src-tauri/src/api/roblox/server_regions.rs) |
| `IsolationBackup/` | `%LOCALAPPDATA%/Roblox Account Manager/` (não segue `RAM_DATA_DIR`) | backups de fast flags e `GlobalBasicSettings_13.xml` do isolamento Full | [platform/windows/isolation.rs](../src-tauri/src/platform/windows/isolation.rs) |
| `chromium/`, `chromium-profiles/` | pasta de dados local do Tauri (`app_local_data_dir`) | Chromium baixado e perfis de login por conta | [chromium/download.rs](../src-tauri/src/chromium/download.rs), [chromium/manager.rs](../src-tauri/src/chromium/manager.rs) |

Regras:
- Na primeira execução com `%LOCALAPPDATA%` disponível, se existir um `RAMVersions.json` legado ao lado do exe, ele é **copiado** para o novo local.
- **`bun run tauri dev` usa a mesma pasta de dados do app instalado**, com o `AccountData.json`, o `AccountData.key` e as settings **de verdade** do dono. `decide_data_dir` não tem caso de debug: o binário de `src-tauri/target/debug/` só vira pasta de dados se tiver um `portable.txt` ao lado. Quem vai mexer em gravação, migração, criptografia ou restauração de backup roda o dev com `RAM_DATA_DIR` apontando para uma pasta descartável (ver [development.md](development.md#dados-em-desenvolvimento)) — senão testa contra as contas reais.
- Nem tudo segue `RAM_DATA_DIR`/`portable.txt`: `RAMVersions.json`, `RobloxVersions/` e `IsolationBackup/` ficam sempre em `%LOCALAPPDATA%/Roblox Account Manager`, e os perfis do Chromium de login na pasta de dados local do Tauri (`app_local_data_dir`). O lado do Roblox (registro, `%LOCALAPPDATA%/Roblox`, clientes abertos) também é o de verdade.

## Feature flags

### Backend (Cargo)

Definidas em [Cargo.toml](../src-tauri/Cargo.toml):

```toml
[features]
default = ["nexus", "webserver"]
nexus = []
webserver = ["dep:axum"]
```

- `nexus`: compila o módulo `nexus` (`#[cfg(feature = "nexus")] mod nexus;` em [lib.rs](../src-tauri/src/lib.rs)).
- `webserver`: compila o servidor axum em [api/server.rs](../src-tauri/src/api/server.rs).
- Os comandos `start_web_server`, `start_nexus_server`, `get_nexus_*` etc. em [services.rs](../src-tauri/src/commands/services.rs) têm **duas versões** (`#[cfg(feature = ...)]` e `#[cfg(not(feature = ...))]`), então o `generate_handler!` sempre compila; sem a feature, eles retornam erro/estado vazio.

### Frontend (Vite)

[featureFlags.ts](../src/featureFlags.ts) lê variáveis de ambiente de build:

| Constante | Variável | Default | Efeito |
|---|---|---|---|
| `ENABLE_NEXUS` | `VITE_ENABLE_NEXUS` | `true` | Mostra o item Nexus na [barra lateral](../src/components/layout/NavSidebar.tsx) e monta a `NexusPage` em [App.tsx](../src/App.tsx). |
| `ENABLE_WEBSERVER` | `VITE_ENABLE_WEBSERVER` | `true` | Inclui a aba WebServer em [SettingsPage.tsx](../src/components/pages/SettingsPage.tsx) e o toggle em [DeveloperTab.tsx](../src/components/settings/DeveloperTab.tsx). |
| `ENABLE_AVATAR_BATCH` | `VITE_ENABLE_AVATAR_BATCH` | `true` | Aba Distribute da [página Avatars](../src/components/pages/AvatarsPage.tsx): o lote, ou (desligada) o cartão que leva à edição completa. Par da feature Cargo `avatar-batch` ([features/avatars.md](features/avatars.md#as-duas-edições)). |

Valores aceitos: `1/true/yes/on` e `0/false/no/off` (qualquer outro → default). As flags do frontend e do Cargo são **independentes**: a CI ([ci.yml](../.github/workflows/ci.yml)) builda as duas combinações ("full" e "standard" com `--no-default-features --features standard`).

## Isolamento de plataforma

- [platform/mod.rs](../src-tauri/src/platform/mod.rs) compila `platform::windows` só em `target_os = "windows"` e `platform::macos` só em `target_os = "macos"`.
- Os comandos que dependem de SO usam `#[cfg(target_os = "windows")]` dentro do corpo ou versões alternativas `#[cfg(not(target_os = "windows"))]` (ex.: [diagnostics.rs](../src-tauri/src/commands/diagnostics.rs), [isolation.rs](../src-tauri/src/commands/isolation.rs), [botting.rs](../src-tauri/src/commands/botting.rs)).
- Dependências Win32 (`windows-sys`) só entram em `cfg(windows)` no [Cargo.toml](../src-tauri/Cargo.toml).
- Descriptografia DPAPI legada (`try_decrypt_legacy_dpapi` em [crypto.rs](../src-tauri/src/data/crypto.rs)) só existe no Windows; nos demais SOs retorna `None`.

## Eventos backend → frontend

Emitidos com `app.emit(nome, payload)` e escutados com `listen(nome, ...)`.

| Evento | Emitido em | Payload | Quem escuta |
|---|---|---|---|
| `launch-log` | [launch_shared.rs](../src-tauri/src/commands/launch_shared.rs) `emit_launch_log` (uma conta) e `emit_session_log` (`userId` nulo) | `{ userId, level, step, message }` | [store.tsx](../src/store.tsx) (console, buffer de 500) — **histórico geral**: launch, Auto Rejoin e Watcher. O `step` é a origem da linha e é desenhado no console |
| `launch-progress` | [launch.rs](../src-tauri/src/commands/launch.rs) | `{ userId, index, total }` | [store.tsx](../src/store.tsx) |
| `launch-complete` | [launch.rs](../src-tauri/src/commands/launch.rs) | `{}` | [store.tsx](../src/store.tsx) |
| `isolation-report` | [launch.rs](../src-tauri/src/commands/launch.rs) | relatório de isolamento | nenhum listener no frontend atualmente |
| `isolation-progress` | [platform/windows/isolation.rs](../src-tauri/src/platform/windows/isolation.rs) | progresso | [IsolationProgressOverlay.tsx](../src/components/IsolationProgressOverlay.tsx) |
| `account-moderated` | [launch_shared.rs](../src-tauri/src/commands/launch_shared.rs) `mark_account_moderated` | `{ userId, group: "moderadas" }` | [store.tsx](../src/store.tsx) (recarrega contas + toast com o nome lido de `accountsRef`, não de estado capturado) |
| `roblox-optimization-warning` | [launch_shared.rs](../src-tauri/src/commands/launch_shared.rs) | aviso de otimização | [store.tsx](../src/store.tsx) |
| `botting-status` | [launch_shared.rs](../src-tauri/src/commands/launch_shared.rs) `emit_botting_status` | `BottingStatusPayload` | [store.tsx](../src/store.tsx) |
| `botting-account-cycle` | [botting.rs](../src-tauri/src/commands/botting.rs) | `{ userId, ok, error }` | [store.tsx](../src/store.tsx) |
| `botting-stopped` | [botting.rs](../src-tauri/src/commands/botting.rs) | `{}` | [store.tsx](../src/store.tsx) |
| `generator-status` | [generators.rs](../src-tauri/src/commands/generators.rs) | `GeneratorStatus` | [store.tsx](../src/store.tsx) |
| `generator-account-added` | [generators.rs](../src-tauri/src/commands/generators.rs) | `{ userId, username }` | [store.tsx](../src/store.tsx), [GeneratorDialog.tsx](../src/components/dialogs/GeneratorDialog.tsx) |
| `generator-stopped` | [generators.rs](../src-tauri/src/commands/generators.rs) | `{}` | [store.tsx](../src/store.tsx) |
| `roblox-process-died` | [watcher.rs](../src-tauri/src/commands/watcher.rs) | `{ userId }` | [store.tsx](../src/store.tsx) |
| `roblox-low-memory` | [watcher.rs](../src-tauri/src/commands/watcher.rs) | `{ userId, memoryMb }` | [store.tsx](../src/store.tsx) |
| `roblox-title-mismatch` | [watcher.rs](../src-tauri/src/commands/watcher.rs) | `{ userId, title, expected }` | [store.tsx](../src/store.tsx) |
| `roblox-beta-detected` | [watcher.rs](../src-tauri/src/commands/watcher.rs) | `{ userId, title }` (macOS: `{ userId, logPath }`) | [store.tsx](../src/store.tsx) |
| `roblox-no-connection` | [watcher.rs](../src-tauri/src/commands/watcher.rs) | `{ userId, title, timeout }` (o `title` vai em minúsculas; macOS: `{ userId, timeout, logPath }`) | [store.tsx](../src/store.tsx) |
| `version-install-progress` | [platform/windows/versions.rs](../src-tauri/src/platform/windows/versions.rs) | `{ stage, ... }` | [VersionsDialog.tsx](../src/components/dialogs/VersionsDialog.tsx), [VersionsTab.tsx](../src/components/settings/VersionsTab.tsx), [SingleSelectSidebar.tsx](../src/components/accounts/SingleSelectSidebar.tsx) |
| `friend-link-state` | [account_api.rs](../src-tauri/src/commands/account_api.rs) `update_friend_link` | `FriendLinkSnapshot` completo: `{ active, phase, processed, total, accounts[{userId,state,error}], mode, mainUserId }` | [store.tsx](../src/store.tsx) → [SessionPanel.tsx](../src/components/session/SessionPanel.tsx), [BottomActionBar.tsx](../src/components/layout/BottomActionBar.tsx). Substituiu o `friend-link-progress`, que era `{phase, done, total}` e contava **pares** na fase de envio |
| `browser-login-detected` | [chromium/commands.rs](../src-tauri/src/chromium/commands.rs) | `()` | [store.tsx](../src/store.tsx) (extrai cookie e adiciona conta) |
| `chromium-download-progress` | [chromium/download.rs](../src-tauri/src/chromium/download.rs) | `{ stage, downloaded, total }` | [store.tsx](../src/store.tsx) |
| `nexus-log` | [nexus/websocket/server_impl.rs](../src-tauri/src/nexus/websocket/server_impl.rs) | `{ message }` | [NexusPage.tsx](../src/components/pages/NexusPage.tsx) |
| `nexus-element-created` / `nexus-element-newline` | [server_impl.rs](../src-tauri/src/nexus/websocket/server_impl.rs) | elemento / `{}` | [NexusPage.tsx](../src/components/pages/NexusPage.tsx) |
| `nexus-account-connected` / `nexus-account-disconnected` | [nexus/websocket/connection.rs](../src-tauri/src/nexus/websocket/connection.rs) | `{ username }` | [NexusPage.tsx](../src/components/pages/NexusPage.tsx) |
| `launch-preset` | [launch_presets.rs](../src-tauri/src/commands/launch_presets.rs) `emit_preset_outcome` | `{ presetId, name, action: open\|close, scheduled, ok, count, error }` | [store.tsx](../src/store.tsx): toast do que veio do horário e releitura da lista ([presets.md](features/presets.md)) |
| `session-history-changed` | [session_history.rs](../src-tauri/src/commands/session_history.rs) `record_session_history` / `record_moderated_history` | `{ userIds }` | [AccountHistory.tsx](../src/components/accounts/AccountHistory.tsx): relê o histórico da conta aberta |
| `launch-queue` | [launch.rs](../src-tauri/src/commands/launch.rs) `emit_launch_queue` | `LaunchQueuePayload`: `{ entries, active, placeId, jobId }` — retrato completo da fila | [store.tsx](../src/store.tsx) → Painel de Sessão ([multi-launch.md](features/multi-launch.md#fila-observável-e-cancelamento)) |
| `roblox-build-install` | [platform/windows/launch.rs](../src-tauri/src/platform/windows/launch.rs) `emit_build_install` | `{ version, stage, current, total, message }` — download silencioso da build do Roblox | [store.tsx](../src/store.tsx) (linha de `actionStatus`) |
| `recordings-changed` / `recording-playback` | [recordings.rs](../src-tauri/src/commands/recordings.rs) | `()` / `{ active }` | [useRecordings.ts](../src/components/afk-mode/recordings/useRecordings.ts) ([recordings.md](features/recordings.md)) |
| `afk-status` / `afk-cycle` / `afk-stopped` | [afk.rs](../src-tauri/src/commands/afk.rs) | `AfkStatusPayload` (`{ active, startedAtMs, intervalSeconds, key, mode, clickX, clickY, accounts }`) / `{ sent }` / `()` | [store.tsx](../src/store.tsx) ([afk-mode.md](features/afk-mode.md)) |
| `avatar-batch-state` | [avatars.rs](../src-tauri/src/commands/avatars.rs) `update_avatar_batch` | `AvatarBatchSnapshot` completo: `{ running, total, done, currentUserId, accounts[{userId,avatarId,status,reason,claimed,missing}] }` | [AvatarsDialog.tsx](../src/components/pages/AvatarsPage.tsx) ([avatars.md](features/avatars.md)) |
| `backup-restored` | [backups.rs](../src-tauri/src/commands/backups.rs) | `RestoreReport` (`backupId`, `safetyBackupId`, `restored`, `skipped`, `accountsReloaded`, `requiresRestart`, `restartReasons`) | [BackupsTab.tsx](../src/components/settings/BackupsTab.tsx) |
| `chromium-fallback` | [chromium/download.rs](../src-tauri/src/chromium/download.rs) | `{ browser, error }` — o download falhou e o login vai pelo navegador do sistema | [store.tsx](../src/store.tsx) |
| `signup-progress` | [chromium/signup_session.rs](../src-tauri/src/chromium/signup_session.rs) | `SignupStatus` (retrato da sessão de criação de contas) | [SignupPanel.tsx](../src/components/signup/SignupPanel.tsx) |
| `server-scan` | [account_api.rs](../src-tauri/src/commands/account_api.rs) `start_server_scan` | página a página da varredura de servidores | [servers/ServersTab.tsx](../src/components/servers/ServersTab.tsx) |
| `server-region-progress` | [account_api.rs](../src-tauri/src/commands/account_api.rs) `get_server_regions` | `{ done, total }` | [servers/ServersTab.tsx](../src/components/servers/ServersTab.tsx) |
| `friends-online-progress` | [account_api.rs](../src-tauri/src/commands/account_api.rs) `get_online_friends_for_accounts` | `{ done, total }` | [FriendsTab.tsx](../src/components/friends/FriendsTab.tsx) |
| `vault-key-warning-changed` | [accounts/commands.rs](../src-tauri/src/data/accounts/commands.rs) `forward_vault_key_warning` (thread própria, alimentada pelo canal `watch_key_warning` do store) | `VaultKeyWarning` ou `null` (sumiu) | [store.tsx](../src/store.tsx) → [VaultKeyBanner.tsx](../src/components/layout/VaultKeyBanner.tsx). Existe porque o aviso também nasce em gravação de fundo (Auto Rejoin, Watcher, servidor HTTP) — ver [accounts.md](features/accounts.md#a-chave-do-aparelho-accountdatakey) |

A maioria dos listeners de [store.tsx](../src/store.tsx) só é registrada depois que o app está inicializado e desbloqueado (`!needsPassword && initialized`). A exceção é `vault-key-warning-changed`, ligado sempre: a faixa também aparece nas telas de senha e de criptografia.

## Fluxo de inicialização

### Backend — `run()` em [lib.rs](../src-tauri/src/lib.rs)

0. **Windows:** `webview_recovery::prepare_environment()` — decide o safe mode de vídeo e monta `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` antes de o Tauri existir, porque o WebView2 lê essa variável na criação da janela ([webview-recovery.md](features/webview-recovery.md)).
1. `crypto::init()` (no-op; a criptografia em Rust puro nao tem init global).
2. Cria `AccountStore` com `AccountData.json` e chama **`load()`**, que é a única porta: ele abre pela chave do aparelho (`AccountData.key`) e migra um arquivo em texto puro, deixando `AccountData.json.bak` antes de qualquer escrita. Só depois consulta `needs_password()` — `true` quando o arquivo está cifrado e nada em memória abre, e aí a UI mostra a tela de senha. Erros viram apenas `eprintln!` (falha de criptografia não pode impedir o app de subir; é na tela dele que o usuário lê o que houve), mas um `load()` que falha marca `load_failed` e bloqueia qualquer `save()` posterior (o arquivo original fica intacto). Ver [accounts.md](features/accounts.md#carregamento--desbloqueio).
3. Cria `SettingsStore` (aplica defaults e já regrava o INI), `ThemeStore`, `ThemePresetStore`, `ScriptStore`, `VersionsCatalogStore`, `ImageCache`.
4. Registra plugins: `single-instance` (segunda instância só mostra/foca a janela `main`), `window-state`, `autostart` (LaunchAgent no macOS), `process`, `updater`.
5. `.manage(...)` de todas as stores + `UpdaterRuntimeState` + `ChromiumManager`.
6. `setup`: **Windows:** `webview_recovery::start_watchdog` (25 s para o frontend avisar que pintou, senão o app reabre em safe mode de vídeo — só em build de release); liga o aviso do `.key` à janela (`forward_vault_key_warning`, antes de qualquer gravação de fundo existir); cria o ícone de bandeja (menu Show/Quit; clique esquerdo mostra a janela).
7. Se compilado com `nexus` e `AccountControl.StartOnLaunch = true`: inicia o servidor Nexus na porta `AccountControl.NexusPort` (default 5242).
8. Se compilado com `webserver` e `Developer.EnableWebServer = true`: inicia o servidor HTTP (`api::server::start`).
9. Ao sair (`ExitRequested`/`Exit`): se `General.EnableMultiRbx` estiver ativo, mata todos os Roblox quando houver mais de um processo, limpa o tracker e desativa o multi-Roblox; em `Exit` também fecha a sessão de login do Chromium.

### Frontend — efeito inicial em [store.tsx](../src/store.tsx)

1. Aplica `DEFAULT_THEME` imediatamente.
2. `needs_password` → se `false`, `get_accounts` e carrega avatares.
3. `get_all_settings` → idioma, `HideUsernames`, `ShuffleJobId`, `SavedPlaceId`/`SavedJobId`/`SavedLaunchData`.
4. `is_accounts_encrypted`.
5. Se não há contas e `General.EncryptionOnboardingState = pending` → abre `EncryptionSetupScreen` (modo `firstRun`).
6. Se `FirstRunWalkthroughState = pending` e o onboarding de criptografia não está pendente → abre o walkthrough.
7. `get_theme` → aplica tema; `initialized = true`.
8. [App.tsx](../src/App.tsx) decide a tela: "Loading..." → `PasswordScreen` (se `needsPassword`) → `EncryptionSetupScreen` → app principal. Depois de inicializado e desbloqueado, roda uma checagem de update. Tudo isso dentro do `AppErrorBoundary`, para erro de render não virar tela branca ([webview-recovery.md](features/webview-recovery.md)).
9. Dois `requestAnimationFrame` depois do primeiro render, [main.tsx](../src/main.tsx) chama `frontend_painted` — é o sinal que desarma o watchdog do WebView2.

## Armadilhas / cuidados

- `get_platform_capabilities` ([platform_info.rs](../src-tauri/src/commands/platform_info.rs)) está incluído e registrado em `generate_handler!`; o frontend o chama no boot e quando uma chave de `[Linux]` muda. Se a chamada falhar, `platformCapabilities` fica `null` e `isWindowsPlatform` volta ao palpite pelo user agent — ver [launch.md](features/launch.md#capacidades-da-plataforma-get_platform_capabilities).
- O webserver é iniciado com um cast `unsafe` de `&AccountStore`/`&SettingsStore` para `'static` em [lib.rs](../src-tauri/src/lib.rs); qualquer mudança no ciclo de vida das stores precisa considerar isso.
- Como os comandos são `include!`-ados na raiz do crate, nomes de funções auxiliares precisam ser únicos entre todos os arquivos de `commands/` (ex.: `decode_url_component` existe tanto em `launch_shared.rs` quanto em `api/roblox/private_links.rs`, mas em escopos diferentes: raiz do crate vs. módulo `api::roblox`).
- O evento `isolation-report` é emitido mas ninguém escuta; se precisar mostrar o relatório na UI, é preciso criar o listener.
