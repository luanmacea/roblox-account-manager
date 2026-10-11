# Configurações (Settings)

## Objetivo

Guardar todas as preferências do app em `RAMSettings.ini` (formato herdado do RAM antigo), com defaults aplicados pelo backend, e oferecer uma tela de configurações dividida em abas.

## Onde fica o código

| Parte | Arquivo |
|---|---|
| Store + **defaults** | [data/settings/store.rs](../../src-tauri/src/data/settings/store.rs) (`apply_defaults`) |
| Parser/gravador INI | [data/settings/ini.rs](../../src-tauri/src/data/settings/ini.rs) |
| Caminhos | [data/settings/paths.rs](../../src-tauri/src/data/settings/paths.rs) |
| Comandos (`get_all_settings`, `get_setting`, `update_setting`, tema, presets, fontes) | [data/settings/commands.rs](../../src-tauri/src/data/settings/commands.rs) |
| Tema (`RAMTheme.ini`) e presets | [data/settings/theme.rs](../../src-tauri/src/data/settings/theme.rs), [data/settings/presets.rs](../../src-tauri/src/data/settings/presets.rs) |
| Página e seções | [SettingsPage.tsx](../../src/components/pages/SettingsPage.tsx) (seções numa lista vertical), [tabs.tsx](../../src/components/settings/tabs.tsx) (ordem e ícones), [TabContent.tsx](../../src/components/settings/TabContent.tsx) |
| Abas | [GeneralTab](../../src/components/settings/GeneralTab.tsx), [BackupsTab](../../src/components/settings/BackupsTab.tsx), [DeveloperTab](../../src/components/settings/DeveloperTab.tsx), [WebServerTab](../../src/components/settings/WebServerTab.tsx), [WatcherTab](../../src/components/settings/WatcherTab.tsx), [GeneratorTab](../../src/components/settings/GeneratorTab.tsx), [IsolationTab](../../src/components/settings/IsolationTab.tsx), [VersionsTab](../../src/components/settings/VersionsTab.tsx), [OptimizationTab](../../src/components/settings/OptimizationTab.tsx), [MiscellaneousTab](../../src/components/settings/MiscellaneousTab.tsx) |
| Hook de leitura/escrita | [hooks/useSettings.ts](../../src/hooks/useSettings.ts) |
| Cópia global na store | [store.tsx](../../src/store.tsx) (`settings`, `reloadSettings`) |

## Fluxo

1. `SettingsStore::new` lê `RAMSettings.ini` (se existir), aplica os defaults **só para chaves ausentes** e salva o arquivo imediatamente.
2. O frontend carrega tudo com `get_all_settings` → `Record<section, Record<key, string>>` guardado em `store.settings`.
3. Ao abrir o diálogo, `useSettings().load()` busca de novo. Cada `set(section, key, value)` atualiza o estado local e agenda gravação (debounce **160 ms**, agrupando por `section::key`); a gravação chama `update_setting` para cada chave e dispara o status "Settings saved".
4. Ao fechar o diálogo, `onSettingsChanged` → `store.reloadSettings()` (recarrega o mapa e o idioma).
5. No backend, `SettingsStore::set` altera a chave e regrava o arquivo inteiro.

### Abas

| Aba | Visível quando | Seções/chaves |
|---|---|---|
| General | sempre | `General.*` (updates, idioma, launch, privacidade de nomes, multi-Roblox, presença, tray...), `Login.*` |
| Backups | sempre | nenhuma chave: criar, listar, restaurar e apagar backups inline ([BackupsTab](../../src/components/settings/BackupsTab.tsx), ver [backups.md](backups.md)). Era um diálogo aberto por "Manage" em Misc > Data até 03/10/2026 |
| Developer | sempre | `Developer.DevMode`, `Developer.EnableWebServer` (toggle só se `ENABLE_WEBSERVER`) |
| WebServer | `ENABLE_WEBSERVER` (build) — a aba aparece sempre; o que destrava os ajustes é `DevMode` ou `EnableWebServer`, dentro da própria aba ([SettingsPage.tsx](../../src/components/pages/SettingsPage.tsx) só a esconde sem `ENABLE_WEBSERVER`) | `WebServer.*` — ver [webserver.md](webserver.md) |
| Watcher | sempre | `Watcher.*` — ver [watcher.md](watcher.md) |
| Account Generator | `ENABLE_ACCOUNT_GENERATOR` (build, **desligado** por padrão desde 03/10/2026 — ver [account-creation.md](account-creation.md)) | `Generator.*`, `BloxGen.*` |
| Isolation | sempre | `Isolation.*` — ver [isolation.md](isolation.md) |
| Versions | sempre | `Versions.*` — ver [roblox-versions.md](roblox-versions.md) |
| Optimization | sempre | `Optimization.*`, `General.BottingUseSharedClientProfile` |
| Misc | sempre | `General.*` diversos + botão "Change Encryption Method" |

## Chaves do `RAMSettings.ini`

Todos os valores são strings; booleanos são `"true"`/`"false"` (qualquer outra coisa conta como falso em `get_bool`). "—" = sem default no backend (a chave só aparece quando alguém grava; o código usa fallback).

### `[General]`

| Chave | Default | Significado |
|---|---|---|
| `CheckForUpdates` | `true` | Checar update ao iniciar. |
| `UpdaterReleaseChannel` | `beta` | Canal de release do updater. |
| `UpdaterFeatureChannel` | a edição que está rodando (`standard` na padrão, `nexus-ws` na completa) | Canal de features do updater. Gravado no INI, nunca é reposto por uma versão nova; o botão "Get the complete edition" da página Avatars grava `nexus-ws` ([avatars.md](avatars.md#trocar-para-a-edição-completa-pelo-app)). |
| `AccountJoinDelay` | `8` | Segundos entre contas no multi-launch. |
| `AsyncJoin` | `false` | "Launch one account at a time": espera o sinal `next_account` (teto de 120 s) depois de cada conta. Nada na tela manda o sinal, então na prática são 2 minutos entre contas — é o que a descrição diz. Ligado, `AccountJoinDelay` e `WaitForGameJoin` ficam desabilitados com a dica de que voltam a valer ao desligá-lo. |
| `KeepPcAwake` | `true` | Não deixa o Windows dormir (a tela pode apagar) enquanto o Modo AFK, o Auto Rejoin ou a reconexão automática roda; solta ao parar tudo e ao fechar o app. Só Windows. Ver [afk-mode.md](afk-mode.md#pc-acordado). |
| `AutoReconnect` | `false` | Padrão de todas as contas para a [reconexão automática](watcher.md#reconexão-automática): reabre no mesmo jogo o cliente que o app abriu quando ele cai. O campo `AutoReconnect` da conta (chave na lista "In game" da página Session) vence. Também muda no cartão "Keep accounts in game" da página Session. Só Windows. |
| `WaitForGameJoin` | `true` | "Start the next account once the previous one is in the game". Multi-launch no Windows: passa para a próxima conta quando o log diz que a anterior entrou no jogo (nunca antes de 8 s, no máximo 20 s ou o delay, se maior); sem log achado, vale o `AccountJoinDelay`. Ver [multi-launch.md](multi-launch.md). |
| `DisableAgingAlert` | `false` | Esconde indicador de conta sem uso há 20+ dias. |
| `HideUsernames` | `false` | Mascara nomes na lista. |
| `HiddenNameLetters` | — (0) | Letras visíveis quando nomes estão ocultos. |
| `WrapLongNames` | `false` | Ligado, o alias/username quebra em mais de uma linha na linha da conta em vez de truncar (o alias vai até 240 caracteres). |
| `ShowAvatarsWhenHidden` / `HideRobuxWhenHidden` | — (false) | Comportamento com nomes ocultos. |
| `DisableImages` | — (false) | Não carregar thumbnails de avatar. |
| `ServerRegionFormat` | `<city>, <countryCode>` | Formato do rótulo de região: `<city>`, `<region>`, `<country>`, `<countryCode>`, `<ip>` — ver [server-choice.md](server-choice.md). |
| `ServerPreference` | `bestfit` | Preferência de servidor do lote: `bestfit` \| `fullest` \| `emptiest` \| `random` \| `none`. |
| `ServerRegionFilter` | — (vazio) | País exigido ao escolher servidor (`BR`); vazio = sem filtro. |
| `ServerScanPages` | `30` | Páginas de 100 servidores varridas na aba Servers (teto 500). |
| `MaxRecentGames` | `8` | Tamanho da lista de jogos recentes. |
| `MaxRecentJobs` | `12` | Tamanho da lista de servidores recentes (Job IDs). |
| `GroupOrder` | `[]` | Ordem manual dos grupos na lista, em JSON (`["Zeta","Alts, velhas"]`). Vazio/`[]` = ordem automática por prefixo numérico e depois alfabética. Ver [accounts.md](accounts.md). |
| `Language` | `en` | `en`, `pt` (português do Brasil) ou `de`. |
| `AutoCookieRefresh` | `true` | Refresh automático de cookies (ver [authentication.md](authentication.md)). |
| `AutoCloseLastProcess` | `false` | Fecha a instância anterior da mesma conta ao relançar. |
| `AutoCloseRobloxForMultiRbx` | `false` | Fecha Roblox abertos se não conseguir ativar multi-Roblox. |
| `EnableMultiRbx` | — (false) | Multi-Roblox; também controla a limpeza ao sair do app. |
| `ShowPresence` | `true` | Mostra presença na lista. |
| `ShowAccountNameOnWindow` | `true` | Só Windows: título "conta — Roblox" (nome primeiro) em cada janela que o app acompanha (mascarado com `HideUsernames`). Ver [watcher.md](watcher.md#nome-da-conta-na-janela). |
| `PresenceUpdateRate` | `5` | Minutos entre atualizações de presença (mínimo efetivo 30 s). |
| `WarnOnOnlineJoin` | `true` | Confirma antes de entrar com conta online. |
| `WarnOnCopyCredential` | `true` | Confirma antes de copiar cookie/senha para a área de transferência (opt-out no próprio aviso). |
| `ShuffleJobId` | — (false) | Sorteia instância de servidor. |
| `UnlockFPS` / `MaxFPSValue` | `false` / `120` | Desbloqueio de FPS (perfil Normal). |
| `CustomClientSettings` | `""` | Caminho de ClientSettings customizado (perfil Normal). |
| `OverrideClientVolume` / `ClientVolume` | `false` / `0.5` | Volume do cliente. |
| `OverrideClientGraphics` / `ClientGraphicsLevel` | `false` / `10` | Qualidade gráfica. |
| `OverrideClientWindowSize` / `ClientWindowWidth` / `ClientWindowHeight` | `false` / `1280` / `720` | Tamanho da janela. |
| `StartRobloxMinimized` | `false` | Abrir cliente minimizado. |
| `BottingPlayer*` / `BottingBot*` (mesmas chaves acima com prefixo) | iguais ao Normal | Perfis de cliente para Auto Rejoin (ver [botting.md](botting.md)). |
| `StartOnPCStartup` | `false` | Autostart (plugin `autostart`). |
| `MinimizeToTray` | `false` | Botão fechar esconde na bandeja. |
| `ThemeWindowsNavbar` | `true` | Barra de título do Windows segue o tema. |
| `ThemeWindowsNavbarAutoEnabledV1` | `true` | Marcador de migração (força `ThemeWindowsNavbar=true` uma vez). |
| `RestrictedBackgroundStyle` | `warp` | Fundo animado da tela de senha: `bubbles`, `warp`, `warpLegacy`, `waves`. |
| `InterfaceScale` | — (`auto`) | Tamanho da interface (zoom nativo do WebView): `auto` encolhe em janela pequena (80–100%), ou fixo `110`/`100`/`90`/`80`. Vale na hora, sem sair da página. Ver [ui-layout.md](ui-layout.md#tamanho-da-interface). |
| `BottingEnabled` | `false` | Habilita ferramentas de Auto Rejoin. |
| `BottingUseSharedClientProfile` | `true` | Os perfis Main/Alt (`BottingPlayer*`/`BottingBot*`) herdam o Normal. |
| `BottingAutoShareLaunchFields` | `true` | **Ignorada desde 03/10/2026**: o Auto Rejoin não tem mais os campos Place/Job/Data para sincronizar com a sidebar (o servidor é onde as contas estão ou um jogo dos Favoritos). Saiu da aba Miscellaneous; a chave fica no INI sem efeito. |
| `BottingDualPanelDialog` | `true` | **Ignorada desde 03/10/2026**: a visão Classic saiu, a aba Auto Rejoin tem uma visão só. Saiu da aba Miscellaneous; a chave fica no INI sem efeito. |
| `BottingDefaultIntervalMinutes` | `19` | Intervalo de ciclo. |
| `BottingLaunchDelaySeconds` | `20` | Espaço entre launches. |
| `BottingRetryMax` / `BottingRetryBaseSeconds` | `6` / `8` | Backoff de retry. |
| `BottingPlayerGraceMinutes` | `15` | Carência para contas main (papel `player` no INI). Sem campo na tela desde 03/10/2026 (a tela não tem mais conta main); vai no Start com o valor do INI. |
| `BottingDraft*` (`PlaceId`, `JobId`, `LaunchData`, `PlayerAccountId(s)`, `SelectedUserIds`) | `""` | Rascunho do Auto Rejoin. Desde 03/10/2026 só `PlaceId`/`JobId` são gravados e lidos (o último jogo escolhido, gravado no Start com "um jogo que eu escolher"); `LaunchData`, `PlayerAccountId(s)` e `SelectedUserIds` ficam sem efeito. |
| `EncryptionMethod` | `default` | `default` ou `password` (ver [accounts.md](accounts.md)). |
| `EncryptionOnboardingState` | `pending` (novo) / `completed` (INI existente) | Onboarding de criptografia. |
| `FirstRunWalkthroughState` | `pending` (novo) / `completed` (INI existente) | Walkthrough; vira `skipped`/`completed`. |
| `SavedPlaceId` / `SavedJobId` / `SavedLaunchData` | — | Últimos valores de launch. |
| `GridGap` / `GridMonitors` | — (20 / vazio) | Arranjo de janelas em grade (Choose Game). |
| `GridAllowSmallWindows` | `false` | Célula da grade menor que o mínimo do Roblox (~800x600), só nos clientes que o app abriu. Ver [performance.md](performance.md). |
| `GridBorderless` | `false` | Janelas da grade sem barra de título e borda, só nos clientes que o app abriu; desligar devolve as molduras. Ver [performance.md](performance.md). |
| `AutoArrangeGrid` | `true` | Grade automática no launch: cada janela nova do Roblox (launch, fila, Auto Rejoin) vai para a primeira célula livre da grade de `GridMonitors`/`GridGap`. Contas com tamanho de janela próprio ficam de fora. Só `"false"` desliga (chave ausente = ligada). Interruptor na aba Windows da Choose Game e em Settings > Optimization. Ver [ui-layout.md](ui-layout.md#grade-de-janelas). |

### `[Developer]`

| Chave | Default | Significado |
|---|---|---|
| `DevMode` | `false` | Opções avançadas (auth ticket, links brutos, edição de campos). |
| `EnableWebServer` | `false` | Inicia a API HTTP no startup (feature `webserver`). |
| `IsTeleport` | `false` | Flag de teleport no join. |
| `UseOldJoin` | `false` | Usa o método antigo de join. |

### `[WebServer]` — detalhes em [webserver.md](webserver.md)

`WebServerPort=7963`, `AllowGetCookie=false`, `AllowGetAccounts=false`, `AllowLaunchAccount=false`, `AllowAccountEditing=false`, `EveryRequestRequiresPassword=false`, `AllowExternalConnections=false`, `Password` (—).

### `[AccountControl]` — Nexus, detalhes em [nexus.md](nexus.md)

`AllowExternalConnections=false`, `StartOnLaunch=false`, `RelaunchDelay=60`, `LauncherDelay=9`, `NexusPort=5242`, `AutoMinimizeEnabled=false`, `AutoCloseEnabled=false`, `InternetCheck=false`, `UsePresence=false`, `AutoMinimizeInterval=15`, `AutoCloseInterval=5`, `MaxInstances=3`, `AutoCloseType=0`. Editadas no [NexusDialog](../../src/components/pages/NexusPage.tsx), não na tela de Settings.

### `[Watcher]` — detalhes em [watcher.md](watcher.md)

| Chave | Default | Unidade/Significado |
|---|---|---|
| `Enabled` | `false` | Liga o watcher. |
| `ScanInterval` | `6` | s |
| `ReadInterval` | `250` | ms |
| `ExitIfNoConnection` / `NoConnectionTimeout` | `false` / `60` | fecha sem conexão após N s |
| `ExitOnBeta` | `false` | fecha se detectar Roblox beta |
| `CloseIfNotResponding` | `false` | fecha o cliente do app que fica "Não respondendo" por 30 s |
| `CloseRbxMemory` / `MemoryLowValue` | `false` / `200` | fecha abaixo de N MB |
| `CloseRbxWindowTitle` / `ExpectedWindowTitle` | `false` / `Roblox` | fecha se o título divergir |
| `SaveWindowPositions` | `false` | salva posição da janela nos `Fields` da conta |

### `[Optimization]`

Três perfis com as mesmas 13 chaves, prefixadas por `Normal`, `BottingPlayer` e `BottingBot`:

| Sufixo | Normal | BottingPlayer | BottingBot |
|---|---|---|---|
| `EnableProcessPolicy` | false | false | false |
| `ProcessPolicyDelayMs` | 1500 | 1500 | 1500 |
| `PriorityClass` | normal | normal | below_normal |
| `BackgroundMode` | false | false | true |
| `EcoQos` | false | false | true |
| `IgnoreTimerResolution` | false | false | true |
| `MemoryPriority` | normal | normal | low |
| `EnableFastFlags` / `FastFlagsJson` | false / "" | false / "" | false / "" |
| `EnableJobCpuLimit` / `JobCpuLimitPercent` | false / 25 | false / 25 | false / 20 |
| `EnableJobMemoryLimit` / `JobMemoryLimitMb` | false / 2048 | false / 2048 | false / 1536 |

Fora dos perfis, uma chave para todos os clientes (cartão **While you play** da aba, ver [performance.md](performance.md)):

| Chave | Default | Significado |
|---|---|---|
| `FollowFocus` | `false` | Otimização que segue o foco: o cliente em uso (que o app abriu) a toda velocidade, os outros com a política de fundo; 35 s de carência para cliente novo. |
| `MuteBackgroundClients` | `false` | Fundo mudo: só o cliente em uso faz som (mixer do Windows). Só vale no binário com a feature `live-audio` (nas duas edições, via `standard`); sem ela a opção nem aparece. |

Com `General.BottingUseSharedClientProfile=true`, Main e Alt usam o perfil Normal (`effective_launch_profile` em [launch_shared.rs](../../src-tauri/src/commands/launch_shared.rs)).

### `[Versions]` — detalhes em [roblox-versions.md](roblox-versions.md)

`DefaultVersion=""` (vazio = instalação oficial), `MaxParallelDownloads=4`, `CatalogCacheMinutes=10`, `PreferOldJoinForVersioned=true` (launch direto do exe para versões gerenciadas), `ShowPreReleaseVersions=false`, `AllowLaunchOnOpenVersion=false` (ligado, a guarda de versão aceita abrir numa versão que já tem cliente aberto — ver [launch.md](launch.md)).

### `[Isolation]` — detalhes em [isolation.md](isolation.md)

`Mode=Off` (`Off` ou `Full`), `SpoofMachineGuid=false`, `SpoofMacAddress=false`, `TargetAdapter=""` (vazio = adaptador ativo principal), `IncludeStudio=false`, `PreserveFastFlags=true`, `PreserveBasicSettings=true`, `BackupMachineGuid=""`, `BackupNetworkAddress=""`, `BackupAdapterId=""` (backups para restauração).

### `[Login]`

| Chave | Default | Significado |
|---|---|---|
| `PersistentProfile` | `true` | Reutiliza o perfil do Chromium de login (menos captchas). Se `false`, o perfil é apagado antes/depois. |
| `StealthMode` | `true` | Esconde sinais de automação no navegador de login. |

### `[Afk]` — detalhes em [afk-mode.md](afk-mode.md)

| Chave | Default | Significado |
|---|---|---|
| `IntervalMinutes` | `10` | Parte em minutos do intervalo entre dois envios da **mesma** conta (0–120). |
| `IntervalSeconds` | `0` | Parte em segundos do mesmo intervalo (0–59). O total (mínimo 5 s, máximo 120 min) conta do **fim** de cada ciclo. |
| `Key` | `""` | Tecla escolhida pelo usuário, de dentro da lista fechada do AFK mode. Vazio = o modo não liga. |
| `BeepOnCycle` | `false` | Bipe curto (sintetizado, sem arquivo de áudio) quando um ciclo de envio termina. |
| `Mode` | `key` | `key`, `click` ou `recording` (toca a gravação de cada conta — [recordings.md](recordings.md)). |
| `ClickX`, `ClickY` | `50`, `50` | Ponto padrão do modo clique, em % da área interna da janela. |

### `[Recordings]` — detalhes em [recordings.md](recordings.md)

| Chave | Default | Significado |
|---|---|---|
| `AfterReconnect` | `false` | Toca a gravação da conta que a reconexão automática devolveu ao jogo (uma vez, só nela). |
| `AfterReconnectDelaySeconds` | `30` | Quanto tempo a conta fica no jogo antes de a gravação tocar (5–3600 s). |

A biblioteca de gravações e qual vale para cada conta ficam em `RAMRecordings.json`, não no INI.

### `[Generator]` / `[BloxGen]`

| Chave | Default | Significado |
|---|---|---|
| `Generator.Provider` | `bloxgen` | Único provedor disponível. |
| `Generator.ExtraDelaySeconds` | `1` | Espera extra após cooldown. |
| `Generator.TargetGroup` | `BloxGen` | Grupo onde contas geradas são colocadas. |
| `Generator.MaxAccounts` | `0` | Parar após N contas (0 = sem limite). |
| `Generator.MaxConsecutiveFailures` | `3` | Para após N falhas seguidas. |
| `Generator.SignupUsernamePrefix` | `""` | Prefixo do nome das contas criadas pelo **formulário do Roblox**: `arvore` gera `arvore_k3p9z`. Vazio = nome de palavras. Não vale para o BloxGen, cujo nome vem do provedor. Ver [account-creation.md](account-creation.md). |
| `BloxGen.Endpoint` | `https://core.bloxgen.net` | URL da API. |
| `BloxGen.ApiKey` | `""` | Chave (em texto no INI). |
| `BloxGen.AccountType` | `alt` | `alt`, `+30 days old`, `+1 year old`, `5+ years old`, `dump`. |

### `[Linux]`

Defaults criados mas sem aba na UI: `PreferredRunner=sober`, `CustomLaunchCommand=""`, `CustomProcessMatch=sober,flatpak,roblox,robloxplayerbeta`, `CustomLogDir=""`, `EnableExperimentalMultiRbx=false`, `WindowControlBackend=auto`.

### Outras seções

| Seção | Origem |
|---|---|
| `[Prompts]` | Criada vazia por `apply_defaults`. |
| `[Friends]` `RequestDelayMs` | Lida por `make_selected_friends` (fallback 2500 ms). |
| `[Script.<id>]` | Configurações de cada script do usuário (ver [scripts.md](scripts.md)). |

## Regras de negócio

- Defaults nunca sobrescrevem valores existentes (`if !exists`).
- `EncryptionOnboardingState` e `FirstRunWalkthroughState` usam o fato de o INI **já existir** para decidir entre `completed` (upgrade) e `pending` (instalação nova).
- `get_bool` só é verdadeiro para exatamente `"true"`; `get_int`/`get_float` retornam `None` se não parsear e o chamador aplica fallback.
- Seções sem propriedades não são gravadas no arquivo ([ini.rs](../../src-tauri/src/data/settings/ini.rs) `save`).
- `ServerRegionFormat` é gravado com um comentário acima da chave.

## Configurações relacionadas

- Idioma: `General.Language` também é aplicado imediatamente via `i18n.changeLanguage` ao mudar na aba General.
- `StartOnPCStartup` chama `enable()`/`disable()` do `@tauri-apps/plugin-autostart` além de gravar a chave.

## Armadilhas / cuidados

- Qualquer comando pode gravar qualquer seção/chave (`update_setting` não valida nada). Scripts com permissão `allowSettings` ficam restritos à própria seção `Script.<id>` pela camada do frontend, não pelo backend.
- Valores sensíveis (`WebServer.Password`, `BloxGen.ApiKey`) ficam em **texto puro** no INI.
- Mudanças em `Developer.EnableWebServer` e `AccountControl.StartOnLaunch` só têm efeito automático no próximo startup (lidas em `setup` do [lib.rs](../../src-tauri/src/lib.rs)).
- O arquivo é regravado por completo a cada `set`; comentários/ordem de um INI editado à mão podem ser normalizados.
- **Default `""` não é gravado:** `IniSection::set` trata valor em branco como remoção, então as ~20 chaves documentadas com default vazio (`General.CustomClientSettings`, `BottingDraft*`, `Optimization.*FastFlagsJson`, `Versions.DefaultVersion`, `Isolation.TargetAdapter`/`Backup*`, `BloxGen.ApiKey`, `Linux.*`) simplesmente **não aparecem** no `RAMSettings.ini` — leia-as sempre com fallback.
- `set` guarda o valor **sem** trim, mas o parser faz trim na leitura: `set("General","X"," 12 ")` volta como `" 12 "` até reiniciar o app, e como `"12"` depois. Normalize antes de gravar.
- Para adicionar uma chave com default, inclua-a em `apply_defaults` na seção certa; o frontend deve sempre ter fallback, pois instalações antigas só recebem a chave no próximo start.
