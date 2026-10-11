# Gravações (sequências tocadas na janela de cada conta)

## Objetivo

Pedido do dono (10/10/2026, item 1.8 do [plano](../plano-ideias.md)): uma
**sequência de passos** — tecla, clique num ponto da janela, espera — que o app
toca nas janelas do Roblox, como o TinyTask. O cliente do Roblox só aceita
entrada na janela **em primeiro plano**, então a gravação toca **uma janela por
vez**: traz a janela da conta para frente, toca a sequência inteira, passa para a
próxima e só no fim devolve o foco.

**O que entrou (fase 1, nas duas edições):** biblioteca, editor de passos,
reprodução, qual gravação vale para cada conta e os dois gatilhos (Modo AFK e
depois da reconexão). Os passos são escritos à mão no editor.

**O que não entrou:** a gravação ao vivo (capturar o que o usuário faz numa
janela e virar passos). Não foi implementada nesta branch — ver "Fora de escopo"
abaixo.

## Onde fica o código

| Arquivo | Papel |
|---|---|
| [data/recordings.rs](../../src-tauri/src/data/recordings.rs) | Modelo (`Recording`, `RecordingStep`, `RecordingsFile`), lista fechada de teclas (`RECORDING_KEYS`), validação (`normalize_recording`), estimativa de duração, qual gravação vale para a conta (`recording_for_account`) e o store `RAMRecordings.json` |
| [commands/recordings.rs](../../src-tauri/src/commands/recordings.rs) | Comandos da tela, a reprodução (`play_recording_with`, `play_recording_in_window`), o "Tocar agora" e o gatilho depois da reconexão (`AfterReconnectBook`) |
| [commands/afk.rs](../../src-tauri/src/commands/afk.rs) | O ciclo que toca (`run_afk_cycle_blocking`, ação `AfkCycleAction::Recording`), o modo `recording` do Modo AFK e os códigos `noRecording`/`focusLost`/`stopped` |
| [platform/windows/input.rs](../../src-tauri/src/platform/windows/input.rs) | As portas novas: `press_recording_key` (tecla da lista das gravações) e `click_recording_point` (a receita do clique do AFK, com o clique de foco opcional) |
| [commands/reconnect.rs](../../src-tauri/src/commands/reconnect.rs) | Arma o gatilho no `Relaunched` e chama a passada dele a cada 2 s |
| [RecordingsTab.tsx](../../src/components/afk-mode/RecordingsTab.tsx), [recordings/useRecordings.ts](../../src/components/afk-mode/recordings/useRecordings.ts) | A aba **Recordings** do Modo AFK |
| [recordings.ts](../../src/recordings.ts) | Tipos e regras puras da tela (espelho dos limites do backend) |
| [ClicksTab.tsx](../../src/components/afk-mode/ClicksTab.tsx), [useClicksController.ts](../../src/components/afk-mode/clicks/useClicksController.ts) | O modo "Play the recording" dos cliques AFK |

## A tela

Aba **Recordings** do Modo AFK, entre **AFK clicks** e **Auto Rejoin** (regra do
dono: configuração de muitas contas não vai para o painel de uma conta — ver
[ui-layout.md](ui-layout.md)).

- **Library:** lista das gravações (nome, passos, duração estimada) com
  renomear (prompt), duplicar ("Nome (copy)", traduzido) e apagar (com
  confirmação). **New recording** na barra de cima.
- **Editor:** nome, passos em ordem (tipo, tecla, tempo, ponto; subir, descer,
  remover), botões para acrescentar cada tipo de passo, duração estimada "por
  janela", **Save recording** / **Discard changes**. Trocar de gravação com
  mudança pede confirmação. O motivo de não poder salvar aparece embaixo (sem
  nome, nome longo, tecla fora da lista, passos demais, mais de 10 minutos).
- **Mark** num passo de clique: os mesmos 3 s do Marcar do Modo AFK
  (`afk_capture_point`, que só lê a **posição** do cursor uma vez); o ponto é
  porcentagem da área interna da janela.
- **Try it now:** marca contas com cliente aberto pelo app e toca a gravação
  **salva** nelas (`play_recording_now`). Com mudança sem salvar, não toca.
  Erros por conta aparecem embaixo, com a frase de cada código.
- **Which recording plays:** a de **todas as contas** e, por conta, a **própria**
  (que vence) ou "Same as all accounts". A lista mostra as contas com cliente
  aberto pelo app e as que já têm gravação própria.
- **When it plays:** "Play after an automatic reconnect" e o tempo no jogo antes
  de tocar; o Modo AFK é escolhido na aba **AFK clicks** ("What to send" →
  "Play the recording").
- Barra de estado: "Playing a recording" e **Stop playing** enquanto uma
  reprodução avulsa roda (evento `recording-playback`).

## Passos

| `type` | Campos | O que faz |
|---|---|---|
| `key` | `key`, `holdMs` (10–10 000, padrão 40) | aperta, segura, solta |
| `keyDown` | `key` | aperta e deixa apertada (combinar teclas: segurar W e pular) |
| `keyUp` | `key` | solta |
| `click` | `xPct`, `yPct` (0–100) | clique esquerdo no ponto relativo da área interna |
| `wait` | `ms` (0–600 000) | espera |

**Lista fechada de teclas** (`RECORDING_KEYS`): as 14 do Modo AFK, na mesma
ordem, mais Shift (esquerdo), as setas (teclas estendidas), as outras letras e os
outros números. Fora, pelos mesmos motivos do AFK: Enter (chat), Tab, Escape
(menu; Esc + L sai do jogo), F-keys (Alt + F4 fecha o cliente, F9 abre o
console), Alt, Ctrl, Windows, Backspace/Delete, `/` e `` ` ``. O backend recusa
gravação com tecla fora da lista, e a porta do envio (`press_recording_key`)
recusa de novo.

Limites: 100 gravações, 500 passos, nome de até 60 caracteres, **10 minutos**
estimados por gravação (esperas + teclas seguradas + 0,8 s por clique).

## Arquivo

`RAMRecordings.json` na pasta de dados (em `DATA_FILES`: vai no backup, na
restauração e na migração de pasta):

```json
{
  "version": 1,
  "recordings": [
    { "id": "rec-1760000000000", "name": "Walk to the farm", "createdAt": 0, "updatedAt": 0,
      "steps": [
        { "type": "keyDown", "key": "W" },
        { "type": "wait", "ms": 1500 },
        { "type": "key", "key": "Space", "holdMs": 40 },
        { "type": "keyUp", "key": "W" },
        { "type": "click", "xPct": 37.5, "yPct": 62.5 }
      ] }
  ],
  "defaultId": "rec-1760000000000",
  "accountIds": { "123456": "rec-1760000000000" }
}
```

- Gravação atômica (temporário + troca), versão anterior em `.bak`; arquivo
  ilegível **trava** a gravação em vez de ser sobrescrito; o store não guarda
  nada em memória (restaurar um backup vale na hora).
- O tempo é guardado como **espera explícita entre passos** (e não como instante
  de cada evento): sobrevive a cortar, reordenar e editar à mão.
- Apagar uma gravação a tira da escolha de todas as contas e das contas que a
  usavam. Escolha que aponta para gravação inexistente (arquivo editado à mão)
  cai na de todas.

## Como toca

É o ciclo do Modo AFK ([afk-mode.md](afk-mode.md#fluxo)), com a ação
`AfkCycleAction::Recording` — mesmo `AFK_CYCLE_LOCK`/`AFK_CYCLE_SEQ` (nunca duas
reproduções nem uma reprodução e um ciclo do AFK ao mesmo tempo), mesmo
`bring_forward_for_cycle`, mesma conferência de que a janela chegou à frente
(`focusDenied`), re-minimiza quem estava minimizado e devolve o foco no fim com
`give_focus_back_after_cycle`.

Por conta, dentro do ciclo (`play_recording_with`):

1. Conta sem gravação (nem a própria, nem a de todas, ou a dela está vazia): erro
   `noRecording`, e a janela **nem vem para frente**.
2. Antes de **cada** tecla apertada e de cada clique, confere que a janela da
   conta continua na frente. Se outra janela veio (o usuário clicou nela), para
   com `focusLost` — o resto cairia na janela do usuário.
3. Esperas em fatias de 25 ms, olhando o "parar" a cada fatia, com prazo próprio
   (o atraso das fatias não acumula). Parar interrompe até uma espera de 10 min.
4. O **primeiro** clique da reprodução leva o clique de foco da receita do AFK
   (a janela acabou de vir para frente); os seguintes não, para não apertar um
   botão do jogo duas vezes (`afk_click_plan_with`).
5. **No fim, sempre**, solta toda tecla que a reprodução apertou e não soltou —
   parada, erro, foco perdido ou `keyDown` sem `keyUp` na gravação. Só solta o
   que ela mesma apertou. Soltar não confere o foco (é o que evita tecla presa).

O que o usuário digita enquanto a janela de uma conta está na frente vai para o
Roblox — a tela diz isso, como no Modo AFK. O ciclo leva a soma das gravações das
contas visitadas.

## Quando toca

### Tocar agora

`play_recording_now { userIds, recordingId }`: a gravação escolhida em todas as
contas marcadas (ou, sem `recordingId`, a de cada conta). Só alcança cliente que
o app abriu (é o tracker que liga conta a janela). **Stop playing**
(`stop_recording_playback`) para na hora; um "parar" de antes não vale para a
reprodução seguinte (o flag é zerado quando ela ganha a vez no lock).

### Modo AFK

Em **AFK clicks**, "What to send" → **Play the recording** (`Afk.Mode =
recording`). Cada conta no modo toca a gravação dela a cada intervalo (minutos +
segundos, contado do fim do ciclo, como os outros modos). A gravação é lida a cada
ciclo: editar ou trocar a gravação vale no ciclo seguinte. O Start recusa se
nenhuma conta marcada tem gravação; a linha de cada conta mostra o nome da
gravação dela ou "no recording". "Play now" (o "enviar agora") toca na hora nas
contas do modo. Parar o Modo AFK interrompe a gravação no meio.

### Depois da reconexão

`Recordings.AfterReconnect` (padrão desligado) e
`Recordings.AfterReconnectDelaySeconds` (padrão 30, 5–3600). Quando a
[reconexão automática](watcher.md#reconexão-automática) relança uma conta
(`ReconnectNotice::Relaunched`), ela é armada (`arm_recording_after_reconnect`).
A cada 2 s (`recording_after_reconnect_tick`, chamado no começo da passada da
reconexão, antes das saídas cedo):

- conta no jogo (log do cliente, `client_health_of`) desde X; quando passa o
  tempo configurado, a gravação **dela** toca, **uma vez**, só nela;
- saiu do jogo antes: a contagem recomeça; cliente novo (outra tentativa da
  reconexão): conta de novo;
- cliente do site (adotado) nunca recebe; quem não volta ao jogo em 15 min é
  esquecido; desligar a opção esquece todo mundo;
- sem gravação para a conta: só a linha no Console.

Console (`step: "recording"`): "Tocando a gravação …", "Gravação … tocada" ou
"… não tocada: motivo".

## Regras de negócio

- **Nada aqui lê teclado nem botão do mouse.** A reprodução é crate-root, a mesma
  árvore do Modo AFK, e o `afk_input_safety_tests` continua varrendo tudo: as
  portas novas moram no `input.rs`, o único que chama `SendInput`.
- **Nada aqui fecha, mata ou minimiza cliente.** Parar só para de mandar.
- Só cliente aberto pelo app (tracker) recebe. PID reaproveitado não recebe (a
  mesma conferência do ciclo do AFK).
- A gravação toca inteira numa janela antes da próxima: o foco fica fora da
  janela do usuário pela soma das gravações.

## Pesquisa (10/10/2026)

**O AutoClicker do dono** (`Desktop/AutoClicker`, C#/WPF; só leitura, nada foi
rodado). As notas dele dizem que as fases de gravação nunca foram validadas à mão
e não citam o Roblox. O que serviu:

- Formato com `schema_version` e campos novos acrescentados com padrão seguro (em
  vez de subir a versão). Aqui: `version: 1` e `#[serde(default)]`.
- Lá, só parte dos arquivos era gravada de forma atômica; aqui, todos.
- Reprodução sempre por `SendInput` com scan code; `PostMessage` "falha em jogos
  e apps com raw input" (palavras dele) — o mesmo achado do Modo AFK.
- Esperas com prazo absoluto e fatias curtas, checando o "parar" a cada fatia;
  soltar tudo que ficou apertado em qualquer saída. Os dois entraram aqui.
- Ponto relativo à janela: lá era deslocamento em pixel da borda de fora,
  achando a janela pelo título — quebra ao redimensionar. Aqui é a porcentagem da
  área interna que o Modo AFK já usa.
- Não havia foco: a reprodução de lá não traz a janela para frente. Aqui é o
  ciclo do AFK, que traz e confere.

**Gravadores de macro de código aberto** (só as ideias; nenhum código copiado):
PyMacroRecord (GPL-3.0, JSON com tempo relativo ao evento anterior), Jitbit Macro
Recorder (fechado; texto com passos "Delay" explícitos e mouse relativo à
janela), xmacro (GPL; texto com `Delay`), rdev e enigo (MIT, Rust), monio
(Apache-2.0; JSON e velocidade). Conclusões aplicadas:

- **Lista de comandos com espera explícita** (estilo Jitbit/xmacro) em vez de log
  cru de eventos: é o que se edita à mão. Milissegundos inteiros.
- **Ponto relativo à área interna** (não à tela): sobrevive a mover e
  redimensionar a janela.
- **Toque mínimo de ~40 ms** (alguns jogos perdem toque instantâneo); piso de
  10 ms no editor.
- `std::thread::sleep` do Rust já usa timer de alta resolução no Windows 10
  1803+; nada de `timeBeginPeriod` (muda o sistema inteiro).
- Esperar em fatias e checar o cancelamento a cada fatia (o PyMacroRecord só
  checa entre eventos, e uma espera longa atrasa o parar).
- Tecla presa no parar: rastrear o que foi apertado e soltar tudo na saída.
- O `SendInput` vai para a janela em foco, não para uma escolhida: confirmar a
  janela antes de cada passo e parar se o foco mudar.

## Fora de escopo

- **Gravação ao vivo** (capturar o que o usuário faz e virar passos): não
  implementada. O editor cobre o caso de uso com passos escritos à mão.
- Botão direito, rolagem, movimento de mouse, velocidade e repetição dentro da
  gravação (repetir é o intervalo do Modo AFK).
- Tocar em várias janelas ao mesmo tempo: o Roblox só aceita entrada na janela
  em foco.

## Testes

Suíte `recordings` (`bun run t recordings`):

- `recordings_store_tests` — id novo, JSON camelCase com `type`, JSON documentado
  acima, substituir mantendo a criação, duplicar, apagar limpando as escolhas,
  escolher gravação inexistente, `.bak`, arquivo ilegível nunca sobrescrito,
  leitura do disco a cada vez, teto da biblioteca.
- `recordings_validation_tests` — lista de teclas (começa com as do AFK, deixa
  fora as perigosas, todas as letras e números), setas estendidas, nome, tecla
  fora da lista, limites de tempo e de ponto, teto de passos e de 10 min,
  duração, a da conta vence a de todas.
- `recordings_playback_tests` (Windows de mentira) — tecla apertada/segurada/
  solta, só o primeiro clique com clique de foco, parar no meio de uma espera de
  10 min soltando a tecla, foco perdido antes da tecla e antes do clique, tecla
  deixada apertada solta no fim, soltar o que não foi apertado não manda nada,
  recusa de tecla/clique, códigos do status, qual gravação cada conta toca
  (vazia não toca).
- `recordings_after_reconnect_tests` — só depois do tempo no jogo, só a conta que
  reconectou, uma vez; sair do jogo recomeça; cliente novo recomeça; opção
  desligada esquece; cliente do site nunca; desiste em 15 min; limites do tempo.
- `recordings_afk_mode_tests` — modo `recording` no Modo AFK, liga sem tecla,
  clique sem o de foco é um clique só (e o AFK continua com dois).
- `win_input_tests` — códigos das teclas das gravações, tecla fora da lista nunca
  é enviada, janela nula nunca recebe clique.
- `RecordingsTab.test.tsx` — biblioteca, editor, salvar/descartar, motivo de não
  salvar, mover/remover, renomear/duplicar/apagar, releitura no evento, quem toca
  o quê, gatilho da reconexão no INI, tocar agora, erros por conta, parar; e o
  modo gravação dos cliques AFK. `recordings.test.ts` — regras puras.

Tecla, clique e janela de verdade ficam fora de teste: precisam de um cliente
Roblox aberto. **Falta teste do dono com cliente real.**
