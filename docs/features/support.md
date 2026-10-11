# Suporte: checagem do launch e reportar problema

## Objetivo

Ajudar quem relata "clico e nada acontece" sem precisar de conversa longa:

- **Checagem "o launch não faz nada"** (ideia 16 de [ideias-de-outros-gerenciadores.md](../ideias-de-outros-gerenciadores.md)): uma lista do que costuma travar um launch, cada linha com OK/aviso/problema e uma frase do que fazer.

## Onde fica o código

| Peça | Arquivo |
|---|---|
| Checagens (backend) | `src-tauri/src/commands/diagnostics.rs` — comando `run_launch_diagnostics`, funções puras `install_check`, `folder_check`, `probe_folder_writable`, `internet_check`, `count_stuck_processes`, `multi_roblox_check` |
| Frases (frontend) | `src/utils/diagnostics.ts` — `diagnosticText`, `overallStatus`, `diagnosticsSummaryLines` |
| Tela | `src/components/dialogs/DiagnosticsDialog.tsx` (estado `diagnosticsOpen` na store) |
| Entradas | Settings › General › "Launch does nothing?" e o botão "Check what's wrong" da faixa de erro (`App.tsx`), que aparece quando há log de launch |

## Fluxo

1. A tela abre e chama `run_launch_diagnostics` (de novo em "Check again").
2. O backend devolve uma lista `{ id, status, reason, count? }`, na ordem:

| `id` | O que confere | Estados |
|---|---|---|
| `robloxInstall` | Há build do Roblox que o app consiga achar (`get_roblox_path`). Só Windows. | `found` (ok), `missing` (aviso: o launch baixa a build sozinho) |
| `dataFolder` | A pasta de dados do app aceita escrita. | `writable` (ok), `notWritable` (problema), `unknown` |
| `versionsFolder` | A pasta das builds baixadas pelo app (`RobloxVersions`) aceita escrita; se ainda não existe, a pasta acima dela. | idem |
| `internet` | Dois hosts do Roblox (`users`, `auth` via `endpoints::host`) respondem. Qualquer resposta HTTP conta, até 404. | `reachable`, `partial` (aviso), `unreachable` (problema) |
| `stuckProcesses` | `RobloxPlayerBeta.exe` **sem janela há 150 s ou mais**. Só Windows. | `none`, `stuck` (aviso, com `count`) |
| `multiRoblox` | Multi Roblox ligado e quem segura a trava (`mutex_holder_label`). Só Windows. | `off`, `offWithClients` (aviso), `held`, `free`, `clientOpen`, `legacyRam` (problema) |

3. O frontend troca cada `id.reason` pela frase traduzida e mostra no topo o pior estado.

## Regras de negócio

- **Só lê.** Nenhuma checagem fecha cliente, mexe em registro, baixa build ou solta a trava do Multi Roblox. O teste `the_diagnostics_never_close_or_kill_anything` lê o próprio arquivo e falha se a seção chamar `kill_process`, `kill_all_roblox`, `TerminateProcess` ou `close_roblox_singleton_handles`. Para processo preso, a frase manda a pessoa finalizar no Gerenciador de Tarefas — o app não faz por ela (o RobloxKeeper tem um "Repair" que fecha tudo; não foi trazido).
- A única escrita é o arquivo de prova da checagem de pasta (`.multialt-write-check-<pid>.tmp`), apagado na hora. Pasta que não existe **não** é criada pela checagem.
- O backend não manda caminho, nome de conta nem PID: só `id`, `reason` e um número. Por isso o resultado pode ir no resumo do "Reportar problema" sem anonimizar.
- Processo sem idade conhecida não conta como preso (melhor não acusar um cliente que acabou de abrir).
- Build ausente é aviso, não problema: o `launch_url` baixa a build de produção sozinho ([launch.md](launch.md)).

## Configurações relacionadas

Nenhuma. A checagem lê `General.EnableMultiRbx`.

## Armadilhas / cuidados

- Ao criar um `reason` novo no backend, crie a frase em `diagnosticText`: o teste `has a sentence for every result the backend can send` lê o `.rs` e falha se faltar.
- As frases são texto público: não citam programas de segurança pelo nome.
