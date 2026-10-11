# Desempenho enquanto você joga

## Objetivo

Deixar rápido o cliente que o usuário está jogando e aliviar os outros, sem
fechar nada. Vem das ideias 18, 20 e 22 de
[ideias-de-outros-gerenciadores.md](../ideias-de-outros-gerenciadores.md)
(pacote **Desempenho**, ver [plano-ideias.md](../plano-ideias.md)). Tudo
opcional e desligado por padrão.

## Onde fica o código

| Parte | Arquivo |
|---|---|
| Decisão (quem é o cliente em uso, carência, o que mudar) e o laço | [platform/windows/focus_follow.rs](../../src-tauri/src/platform/windows/focus_follow.rs) |
| Troca de prioridade/EcoQoS/memória ao vivo, teto do Job | [platform/windows/optimization.rs](../../src-tauri/src/platform/windows/optimization.rs) (`apply_process_policy_live`, `set_job_cpu_cap`) |
| Grade menor e sem moldura | [platform/windows/windowing.rs](../../src-tauri/src/platform/windows/windowing.rs) (`GridWindowStyle`, `grid_swp_flags`, `borderless_style`), [commands/launch_shared.rs](../../src-tauri/src/commands/launch_shared.rs) (`grid_window_style`) |
| Volume ao vivo (decisão sempre; COM só com `live-audio`) | [platform/windows/live_audio.rs](../../src-tauri/src/platform/windows/live_audio.rs) |
| Timer de 1 s e devolução ao fechar o app | [commands/focus_follow.rs](../../src-tauri/src/commands/focus_follow.rs), [lib.rs](../../src-tauri/src/lib.rs) |
| Launch avisa o perfil do cliente | [commands/launch_shared.rs](../../src-tauri/src/commands/launch_shared.rs) (`apply_windows_post_launch_profile`) |
| Tela | Settings > Optimization, cartão **While you play** ([OptimizationTab.tsx](../../src/components/settings/OptimizationTab.tsx)) |

## Otimização que segue o foco (`Optimization.FollowFocus`)

Interruptor **Follow the window in use**: o cliente que você está jogando roda
a toda velocidade, os outros desaceleram.

### Fluxo

1. Uma vez por segundo o app pergunta ao Windows qual janela está em primeiro
   plano (`GetForegroundWindow` + `GetWindowThreadProcessId`).
2. Se ela é de um cliente **que o app abriu**, ele vira o "cliente em uso".
   Janela de fora (Discord, o próprio MultiAlt, um cliente aberto pelo site)
   **não** troca o cliente em uso: o último continua a toda velocidade.
3. Cliente em uso e cliente na carência → **toda velocidade**: prioridade
   normal, power throttling devolvido ao Windows (`ControlMask = 0`), memória
   normal e, se o perfil tem teto de CPU, o teto do Job **desligado**.
4. Os outros → **fundo**: a política do perfil com que a conta abriu se o
   usuário ligou `EnableProcessPolicy`; senão uma leve (abaixo do normal, EcoQoS,
   timer ignorado, memória baixa). O teto de CPU do Job volta, se o perfil tem.
5. Só há chamada ao processo quando a velocidade desejada **muda**; a olhada
   de cada segundo não abre processo nenhum.

### Regras de negócio

- **Só clientes que o app abriu** (rastreados e não adotados). O cliente
  aberto pelo site só é exibido: nunca recebe prioridade, teto nem nada.
- **Carência de 35 s** para cliente novo (carregar o jogo é a parte pesada).
  Os que já estavam abertos quando a opção ligou não têm carência.
- Antes de mexer num PID o app confere que ele ainda é um Roblox (o Windows
  reaproveita PIDs).
- Com a opção ligada, o launch **não aplica** a prioridade do perfil (o laço
  cuida dela); o Job (teto de CPU e de memória) continua sendo criado no
  launch, e o laço reaplica a velocidade logo depois (`focus_follow_forget_applied`).
- **Desligar a opção ou fechar o app** devolve cada cliente ao que o launch
  deu: a política do perfil se ligada, senão o normal; o teto do Job volta se o
  perfil o tiver. Fechar o app não fecha cliente nenhum.
- `BackgroundMode` do perfil ("Idle até no cliente em foco") vale só para os
  clientes de fundo enquanto a opção está ligada.

### Por que pergunta em vez de evento

A ideia original reage à troca de foco por `SetWinEventHook`. Aqui não:

- `SetWinEventHook` seria uma API nova no binário (risco de antivírus), e o
  `afk_input_safety_tests` reprova o AFK mode citar um módulo que leia
  entrada — pôr o evento em `platform/windows/` faria do módulo `windows`
  inteiro um "leitor".
- `GetForegroundWindow` e `GetWindowThreadProcessId` já estavam no binário. A
  pergunta custa microssegundos; a troca de janela aparece em até 1 s.

### Fora daqui (de propósito)

- **Teto de memória que pagina em vez de matar** (RobloxKeeper:
  `EmptyWorkingSet`/`SetProcessWorkingSetSizeEx`): ficou de fora porque nenhuma
  das duas APIs está no binário hoje.
- Nada de afinidade de CPU, turbo, plano de energia, tarefa agendada ou admin.

## Volume ao vivo por cliente (`Optimization.MuteBackgroundClients`)

Interruptor **Mute the Roblox windows you're not using**: só a janela que você
está jogando faz som. É o mixer de volume do Windows (a sessão de áudio de cada
processo), com o jogo aberto — o arquivo de configurações do Roblox não é
tocado. O volume configurado no launch (`OverrideClientVolume`) continua igual.

### Atrás da feature `live-audio` (nas duas edições)

A sessão de áudio só se alcança por **COM** (`IMMDeviceEnumerator` →
`IAudioSessionManager2` → `IAudioSessionControl2` → `ISimpleAudioVolume`),
código nativo novo no binário. Por isso:

- tudo que fala COM fica em `mod live_audio_com`, compilado só com
  `--features live-audio`. Sem a feature o arquivo compila apenas a decisão
  (pura, testada), `supportsLiveAudio` vem `false` e a opção **some da tela**;
- nenhum crate novo: o `windows-sys` não traz interfaces COM, então as tabelas
  de métodos usadas estão escritas à mão, só até o último método chamado. A
  feature liga apenas `Win32_System_Com` no `windows-sys` (`CoInitializeEx`,
  `CoCreateInstance`, `CoUninitialize`, do `ole32.dll` — que o WebView2 já
  carrega);
- vai **nas duas edições** (decisão do dono, 10/10/2026): `live-audio` está na
  lista da feature `standard` do [Cargo.toml](../../src-tauri/Cargo.toml), que o
  [release-v4.yml](../../.github/workflows/release-v4.yml) compila na padrão
  com `--no-default-features --features standard`; a completa
  (`--features full`) a pega via `standard`. Tirá-la de uma edição é tirá-la da
  lista. Travado em `live_audio_ships_in_both_editions` (e no
  `release-workflow.test.mjs`, que exige toda feature opcional no `full`). Ver
  [As duas edições](../development.md#as-duas-edições).

### Regras de negócio

- Usa o mesmo "cliente em uso" da otimização que segue o foco (as duas opções
  são independentes; a olhada de 1 s serve às duas).
- **Só clientes que o app abriu.** Cliente aberto pelo site nunca é mutado.
- Sem cliente em uso (nenhum cliente nosso esteve em primeiro plano ainda),
  ninguém é mutado.
- O app só desmuta o que ele mesmo mutou: um mudo posto pelo usuário no mixer
  fica.
- O Roblox só abre a sessão de áudio quando o jogo começa a tocar: cliente sem
  sessão é tentado de novo a cada 5 s. O COM só é chamado quando o cliente em
  uso ou a lista de clientes muda (ou nessa nova tentativa).
- Antes de mutar, o PID é conferido como Roblox (PID reaproveitado não é
  mutado).
- **Desligar a opção ou fechar o app desmuta** tudo o que o app mutou.

## Grade menor que o mínimo e sem moldura (`General.GridAllowSmallWindows`, `General.GridBorderless`)

Dois interruptores em Settings > Optimization, logo abaixo de **Arrange in grid
on launch**. Valem na grade automática do launch e no botão **Arrange in grid**
(aba Windows da Choose Game) — ver [ui-layout.md](ui-layout.md#grade-de-janelas).

- **Allow smaller windows in the grid**: a célula fica do tamanho pedido, mesmo
  abaixo do mínimo do Roblox (~800x600 de área útil). A janela é posta com
  `SWP_NOSENDCHANGING`: o `WM_WINDOWPOSCHANGING` não chega ao Roblox, que é onde
  ele impõe o mínimo. Piso de 200x200 (a grade não enxerga janela menor). A
  conferência que segura a célula logo depois do launch usa o mesmo flag — sem
  isso ela devolveria o mínimo.
- **Remove window borders in the grid**: tira a barra de título e a borda
  (`WS_CAPTION | WS_THICKFRAME`) antes de medir a célula, então a janela
  inteira vira área do jogo e as janelas encostam. O resto do estilo fica (o
  botão da barra de tarefas continua minimizando).

### Regras de negócio

- **Só clientes que o app abriu.** No botão manual, as janelas de clientes
  abertos pelo site são arrumadas como sempre: com moldura, e a célula volta ao
  mínimo do Roblox se uma delas está na grade (ela cresceria e sobreporia as
  vizinhas — `grid_small_allowed`).
- A moldura original de cada janela fica guardada (`hwnd → pid, estilo`) e
  **volta**: quando a opção desliga (olhada de 1 s), quando o botão manual
  arruma sem a opção, e ao fechar o app. Só volta o que a janela tinha.
- Janela sem moldura do tamanho de uma célula não é confundida com tela cheia
  (a tela cheia exige cobrir o monitor inteiro).
- Só constantes novas (`SWP_NOSENDCHANGING`, `SWP_NOMOVE`, `SWP_NOSIZE`,
  `WS_THICKFRAME`); as APIs (`SetWindowPos`, `SetWindowLongW`,
  `GetWindowLongW`) já eram usadas pela grade e pela saída da tela cheia.
- **Confirmar com cliente real** (está assim no catálogo de ideias): o Roblox
  pode desenhar mal abaixo do mínimo, ou voltar a moldura sozinho ao sair da
  tela cheia — nesse caso a moldura fica, e o app não briga.



| Seção | Chave | Default | Efeito |
|---|---|---|---|
| Optimization | `FollowFocus` | `false` | Liga a otimização que segue o foco. |
| Optimization | `MuteBackgroundClients` | `false` | Fundo mudo (só com a feature `live-audio`, que vai nas duas edições). |
| General | `GridAllowSmallWindows` | `false` | Célula da grade menor que o mínimo do Roblox. |
| General | `GridBorderless` | `false` | Janelas da grade sem moldura. |
| General | `RestoreRobloxSettingsOnExit` | `false` | Devolve ao fechar o app o que o launch mudou nos arquivos do Roblox (ver acima). |
| Optimization | `{Normal,BottingPlayer,BottingBot}EnableProcessPolicy` e demais | ver [settings.md](settings.md#optimization) | Política de fundo (se ligada) e o estado devolvido ao desligar. |

## Devolver as configurações do Roblox ao fechar (`General.RestoreRobloxSettingsOnExit`)

Ideia 21 (pacote **Conforto**). O launch grava FPS, volume, qualidade, janela
e FastFlags nos arquivos **do Roblox** — o `GlobalBasicSettings_13.xml` (em
`%LOCALAPPDATA%\Roblox`) e o `ClientAppSettings.json` da pasta da versão —, e
o jogo aberto pelo site lê os mesmos. Com a opção "Restore Roblox settings when
MultiAlt closes" (Settings › Optimization, no cartão dos perfis; **desligada**
por padrão, só Windows), o app devolve os valores do usuário ao fechar.

- **Onde fica:** [platform/windows/settings_restore.rs](../../src-tauri/src/platform/windows/settings_restore.rs)
  (anotação, cópia de segurança e devolução), chamado por
  `patch_client_settings_for_launch` ([launch_shared.rs](../../src-tauri/src/commands/launch_shared.rs))
  e `cmd_apply_fps_unlock` ([launch.rs](../../src-tauri/src/commands/launch.rs)),
  e na saída por `restore_roblox_settings_on_exit` ([lib.rs](../../src-tauri/src/lib.rs)).
- **Anota em volta da escrita:** com a opção ligada, o launch lê os dois
  arquivos antes (`snapshot_roblox_settings`) e, depois de gravar, anota **por
  propriedade** o que mudou (`RestoreJournal::record`): o valor de antes da
  **primeira** mudança e o que o app escreveu por último. No XML contam só as
  propriedades que o launch escreve (FPS, volume, qualidade, janela); no JSON,
  as chaves do topo (o `DFIntTaskSchedulerTargetFps`, as FastFlags e o arquivo
  do "Custom ClientSettings"). A anotação mora em `RobloxSettingsRestore.json`,
  na pasta de dados, e sobrevive a reabrir o app.
- **Cópia de segurança:** antes da primeira mudança de cada arquivo, a cópia
  inteira dele vai para `RobloxSettingsBackup/` na pasta de dados
  (`0-GlobalBasicSettings_13.xml`, `1-ClientAppSettings.json`…). Fica lá depois
  da devolução, para recuperar à mão; um ciclo novo começa limpando as cópias
  do anterior.
- **Ao fechar** (`ExitRequested`/`Exit`, decidido uma vez só —
  `settings_on_exit_plan`): sem cliente **que o app abriu** rodando, cada
  propriedade que ainda tem o valor do app volta ao do usuário (ou some, se não
  existia); a que mudou depois — o jogador mexeu no jogo — fica. Arquivo que o
  app criou e ficou vazio é apagado. Com cliente do app rodando, **nada é
  fechado**: a anotação fica para o próximo fechar (o cliente aberto relê e
  regrava esses arquivos). Cliente aberto pelo site não segura a devolução.
- **Desligar a opção** esquece a anotação no próximo fechar, sem mexer nos
  arquivos. Ligada depois, só conta o que mudar dali em diante.
- **Convive com o registro das exceções** (`ClientOverrideLedger.json`, o que
  impede a exceção de uma conta de vazar para a próxima): depois da devolução,
  os valores anotados lá não batem mais com o arquivo e ele os descarta sozinho.
- **Nunca** deixa o arquivo como somente-leitura (o que outro gerenciador faz).
- Testes: `win_settings_restore_tests` (devolve o valor do app, mantém o que o
  jogador mudou, duas aberturas seguidas, propriedade criada pelo app, FastFlags
  do usuário ficam, arquivo criado pelo app apagado, cópia antes da primeira
  mudança — tudo em pastas temporárias) e `settings_on_exit_tests` (devolve só
  sem cliente do app; desligada, esquece). Interruptor em `settingsTabs.test.tsx`.

## Testes

`focus_follow_tests` (cliente em uso, carência, plano de mudanças, políticas,
devolução, padrão desligado), `win_optimization_tests` (máscaras do power
throttling e do teto do Job ao vivo), `live_audio_tests` (quem é mutado, só o
que o app mutou é desmutado, feature só na edição completa, COM só neste
arquivo), `live_audio_com_tests` (com a feature: percorre o mixer de verdade
**sem** mutar nada, tamanho das tabelas), `platform_info_tests`
(`supportsLiveAudio`), `win_grid_style_tests` (célula pequena, flags do
`SetWindowPos`, moldura tirada e devolvida, cliente do site na grade),
`win_grid_slot_tests`, `client_window_plan_tests` (padrão desligado),
`settingsTabs.test.tsx` (interruptores).
Suíte: `bun run t performance`.

## Armadilhas / cuidados

- O efeito só se vê com cliente real: Gerenciador de Tarefas > Detalhes >
  coluna Prioridade (Normal no jogo em uso, Abaixo do normal nos outros) e a
  coluna "Modo de eficiência".
- O rastreamento é o do app: cliente que o app abriu e que o Auto Rejoin
  relançou entra de novo com carência nova.
