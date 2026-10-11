# Suporte: checagem do launch, reportar problema e afins

## Objetivo

Ajudar quem relata "clico e nada acontece" sem precisar de conversa longa:

- **Checagem "o launch não faz nada"** (ideia 16 de [ideias-de-outros-gerenciadores.md](../ideias-de-outros-gerenciadores.md)): uma lista do que costuma travar um launch, cada linha com OK/aviso/problema e uma frase do que fazer.
- **Reportar problema com resumo anonimizado** (ideia 28): o "Send feedback" › "Report a problem" pode levar versão, edição, sistema, a checagem e as últimas linhas do Console, sem nada que identifique a pessoa ou as contas.

## Onde fica o código

| Peça | Arquivo |
|---|---|
| Checagens (backend) | `src-tauri/src/commands/diagnostics.rs` — comando `run_launch_diagnostics`, funções puras `install_check`, `folder_check`, `probe_folder_writable`, `internet_check`, `count_stuck_processes`, `multi_roblox_check` |
| Frases (frontend) | `src/utils/diagnostics.ts` — `diagnosticText`, `overallStatus`, `diagnosticsSummaryLines` |
| Tela | `src/components/dialogs/DiagnosticsDialog.tsx` (estado `diagnosticsOpen` na store) |
| Entradas | Settings › General › "Launch does nothing?" e o botão "Check what's wrong" da faixa de erro (`App.tsx`), que aparece quando há log de launch |
| Resumo do relato | `src/utils/problemReport.ts` (`buildProblemReport`, `anonymizeContextFor`), `src/utils/anonymize.ts` (`anonymize`), `src/components/dialogs/FeedbackDialog.tsx` (passo "Report a problem"), comando `get_report_environment` em `src-tauri/src/commands/services.rs` e `os_version_label` em `platform/windows/core.rs` |

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

## Reportar problema com resumo anonimizado

### Fluxo

1. Barra lateral › **Send feedback** › **Report a problem** abre o segundo passo (antes abria o formulário direto).
2. A caixa **"Add a diagnostic summary"** começa **desmarcada**. Desmarcada, nada é montado e nenhum comando roda; "Open the form" abre o formulário de bug como antes.
3. Marcada, o app junta: `get_report_environment` (versão, edição `standard`/`complete`, versão do Windows lida do registro), `run_launch_diagnostics` (a mesma checagem acima) e as últimas 40 linhas do Console (`store.launchLogs`). Monta o texto (`buildProblemReport`) e passa tudo por `anonymize`.
4. A prévia mostra **o texto inteiro** que iria junto. "Copy summary" copia **exatamente** a prévia para a área de transferência; "Open the form" abre o formulário no navegador e a pessoa cola na descrição.
5. Nada vai na URL (ela é fixa e não pode ter `&`, ver [ui-layout.md](ui-layout.md)) e nada sai do PC sem dois cliques da pessoa — copiar e enviar o formulário no GitHub.

### O que a anonimização tira

| O quê | Como | Vira |
|---|---|---|
| Cookie e senha das contas salvas | pelo valor exato (`anonymizeContextFor`) | `[secret]` |
| Cookie do Roblox em qualquer forma | `_\|WARNING:…\|_…` e `.ROBLOSECURITY=…` | `[cookie]` |
| Campos de segredo | `cookie`, `authorization`, `x-csrf-token`, `ticket`, `token`, `privateKey`, `password`, `api_key`… seguidos de `:` ou `=` | `campo: [secret]` |
| Sequência opaca de 40+ caracteres | regex | `[token]` |
| Nome de usuário e alias das contas | sem diferenciar maiúsculas, palavra inteira, o mais longo primeiro | `[account]` |
| ID de usuário | os das contas salvas e qualquer número depois de `user`/`userId`/`uid` | `[user-id]` |
| Usuário do Windows no caminho | `C:\Users\<nome>`, `C:/Users/<nome>`, `/Users/<nome>`, `/home/<nome>` | `<user>` (o resto do caminho fica) |
| Job ID | UUID | `[job-id]` |
| Servidor privado | `vip:…`, `privateServerLinkCode=`, `linkCode=`, `accessCode=`, `launchData=`, `roblox.com/share?…` | `[private]` |
| E-mail | regex | `[email]` |
| IP | IPv4 (com porta) e IPv6 | `[ip]` |

No Console, cada conta aparece como `account N` (a posição na lista), só para ligar linhas da mesma conta. Ficam: Place ID (é público e ajuda a entender), versões, horários, mensagens de erro.

### Regras de negócio

- **Cookie nunca aparece.** Os testes (`anonymize.test.ts`, `problemReport.test.ts`, `FeedbackDialog.test.tsx`) passam um log cheio de cookie, senha, nome, ID, caminho, Job ID, IP e e-mail e conferem que nada disso sobra.
- O texto do resumo é em inglês (quem lê é o dono do projeto); a tela em volta é traduzida.
- A anonimização erra para o lado seguro: um nome de conta igual a uma palavra comum some do texto inteiro.
