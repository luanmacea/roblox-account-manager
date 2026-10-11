# Documentação — Roblox Account Manager 4

Roblox Account Manager 4 (RAM4) é um gerenciador desktop de múltiplas contas Roblox escrito em **Tauri 2** (backend Rust + frontend React/TypeScript). Ele permite:

- guardar várias contas (cookie `.ROBLOSECURITY`, senha opcional, alias, grupo, campos livres) em um arquivo local sempre criptografado — com senha, se o usuário quiser; sem senha, pela chave do aparelho (`AccountData.key`);
- lançar um ou vários clientes Roblox ao mesmo tempo (multi-Roblox), em jogos públicos, Job IDs específicos ou servidores VIP/privados;
- navegar por jogos/servidores, manter favoritos (com vários links VIP por jogo) e jogos recentes;
- automatizar re-join de contas (Auto Rejoin), mandar tecla de tempo em tempo para não perder o estado (AFK mode), vigiar processos (Watcher), isolar sessões antes do launch e fixar versões específicas do cliente Roblox;
- expor uma API HTTP local (feature `webserver`) e um servidor WebSocket para scripts Lua (feature `nexus`);
- rodar scripts JavaScript do usuário em um sandbox (Web Worker) dentro do app.

Plataforma principal: **Windows** (APIs Win32 para processos, registro, janelas, DPAPI). macOS é parcial.

## Como navegar

| Se você quer... | Leia |
|---|---|
| Entender como as peças se encaixam (IPC, stores, arquivos, eventos, startup) | [architecture.md](architecture.md) |
| Rodar, buildar, adicionar um comando Tauri, extrair traduções | [development.md](development.md) |
| Entender uma funcionalidade específica | [features/](#funcionalidades) |
| Achar uma funcionalidade na tela (o que existe e onde fica) | [mapa-da-interface.md](mapa-da-interface.md) |
| Saber o que está confuso na interface e o que corrigir primeiro | [ux-checkup.md](ux-checkup.md) |
| Ver o que outros gerenciadores têm que vale trazer (ideias já triadas por segurança) | [ideias-de-outros-gerenciadores.md](ideias-de-outros-gerenciadores.md) |

Cada documento de funcionalidade segue a mesma estrutura: **Objetivo**, **Onde fica o código**, **Fluxo**, **Regras de negócio**, **Configurações relacionadas**, **Armadilhas / cuidados**.

## Documentos gerais

- [architecture.md](architecture.md) — arquitetura geral, persistência, feature flags, eventos backend→frontend, fluxo de inicialização.
- [development.md](development.md) — setup, comandos, features do Cargo, i18n, convenções, passo a passo para novo comando Tauri.
- [mapa-da-interface.md](mapa-da-interface.md) — onde fica cada funcionalidade na tela e para que serve, em português.
- [ux-checkup.md](ux-checkup.md) — revisão de usabilidade de setembro/2026: 131 achados triados por prioridade.
- [ideias-de-outros-gerenciadores.md](ideias-de-outros-gerenciadores.md) — funcionalidades de 48 gerenciadores open source que valem a pena trazer, com origem, licença e cuidados de segurança (skill `/competitor-scout`).
- [plano-ideias.md](plano-ideias.md) — em que ordem e em quais versões as ideias entram (pacotes por tema), e as respostas às dúvidas do dono sobre cada uma.
- [../site/README.md](../site/README.md) — site de divulgação (HTML estático no GitHub Pages): como roda, como publica e de onde vêm os links de download.

## Funcionalidades

### Contas e sessão
- [features/backups.md](features/backups.md) — backup/restauração dos dados pelo app e onde a pasta de dados fica (o `.exe` é portátil).
- [features/accounts.md](features/accounts.md) — modelo de conta, adicionar/remover/importar, criptografia do `AccountData.json`, tela de senha, grupos (incl. `moderadas`), campos, comandos de API por conta.
- [features/account-creation.md](features/account-creation.md) — criar contas em série: formulário preenchido pelo app com o CAPTCHA resolvido pelo usuário, e o gerador por provedor (BloxGen).
- [features/authentication.md](features/authentication.md) — cookie, CSRF, auth ticket, retry de sessão (`run_with_session_retry`), refresh de cookie.

### Launch e automação
- [features/launch.md](features/launch.md) — lançamento de uma conta.
- [features/multi-launch.md](features/multi-launch.md) — lançamento de várias contas / multi-Roblox.
- [features/presets.md](features/presets.md) — presets de launch (contas → jogo/servidor num clique) e horário de abrir/fechar, que fecha só o que o preset abriu.
- [features/external-clients.md](features/external-clients.md) — clientes abertos pelo site (ou antes de o app abrir) reconhecidos pelo log do Roblox; identificação manual.
- [features/botting.md](features/botting.md) — Auto Rejoin (auto-rejoin cíclico) e a tela do **Modo AFK**, que junta Auto Rejoin e cliques AFK em abas.
- [features/afk-mode.md](features/afk-mode.md) — AFK mode: envio periódico de uma tecla para a janela de cada conta, sem rejoin.
- [features/recordings.md](features/recordings.md) — Gravações: sequências de teclas, cliques e esperas tocadas na janela de cada conta, uma por vez (Modo AFK, depois da reconexão, ou na hora).
- [features/avatars.md](features/avatars.md) — avatares grátis: montar avatares só com itens oficiais gratuitos do Roblox e distribuí-los entre as contas (resgata o que falta de graça, nunca gasta Robux).
- [features/groups.md](features/groups.md) — página Groups: busca de grupos do Roblox e entrada das contas marcadas, uma por vez; captcha resolvido pela pessoa no navegador da conta.
- [features/isolation.md](features/isolation.md) — isolamento pré-launch (cache, registro, MachineGuid/MAC).
- [features/roblox-versions.md](features/roblox-versions.md) — instalação e seleção de versões do cliente Roblox.
- [features/watcher.md](features/watcher.md) — monitoramento de processos Roblox, quedas lidas do log e [reconexão automática](features/watcher.md#reconexão-automática).
- [features/performance.md](features/performance.md) — desempenho enquanto você joga: otimização que segue a janela em uso, fundo mudo (feature `live-audio`), grade menor que o mínimo e sem moldura.
- [features/history.md](features/history.md) — histórico de sessões por conta (entrou, saiu, caiu com motivo), tempo de jogo de 14 dias, "Join again" e export CSV.

### Integrações
- [features/webserver.md](features/webserver.md) — API HTTP local (feature `webserver`).
- [features/chromium.md](features/chromium.md) — navegador Chromium via CDP: login por janela/senha, captura do cookie e o que foi mitigado na porta de debug.
- [features/nexus.md](features/nexus.md) — servidor WebSocket para Nexus.lua (feature `nexus`).
- [features/scripts.md](features/scripts.md) — scripts do usuário e sandbox do frontend.

### Interface
- [features/server-choice.md](features/server-choice.md) — preferência de servidor (aleatório/mais vazio/mais cheio), filtro por região e a aba Servers da Choose Game.
- [features/server-list.md](features/server-list.md) — navegador de servidores/jogos, favoritos, recentes, VIP.
- [features/friends.md](features/friends.md) — aba "Friends": amigos online por conta e entrada de todas as contas no servidor do amigo.
- [features/join-links.md](features/join-links.md) — campo único de "Join link": convites de experiência, links VIP/privados, links de jogo e deep links.
- [features/settings.md](features/settings.md) — abas de configuração e chaves do `RAMSettings.ini`.
- [features/ui-layout.md](features/ui-layout.md) — shell da UI, temas/fontes, diálogos, lista de contas, barra de ações, tela "Choose Game".
- [features/webview-recovery.md](features/webview-recovery.md) — janela abrindo em branco/preta: safe mode de vídeo do WebView2, marcador preso à versão do runtime e tela de erro do React.
- [features/support.md](features/support.md) — pacote Suporte: checagem "o launch não faz nada" (só lê, nunca fecha nada).

## Registro de mudanças (2026-09-27)

**Contas e dados**

1. **`AccountData.json` cifrado sempre**, também sem senha: a chave do aparelho fica em `AccountData.key`, ao lado, embrulhada pelo DPAPI do usuário e por um hash do aparelho. A migração deixa `AccountData.json.bak` **em texto puro** (apague depois de conferir que as contas abrem), o `.key` viaja no zip de backup e restaurar a chave exige reiniciar. Problema com o `.key` vira faixa na tela (`VaultKeyBanner`) — [accounts.md](features/accounts.md#a-chave-do-aparelho-accountdatakey), [backups.md](features/backups.md).
2. **Import aceita `username:password:cookie`** nas abas Import Cookie e User:Pass, sem navegador — [accounts.md](features/accounts.md#adicionar-contas).
3. **Alias até 240 caracteres**, com a opção `WrapLongNames` de quebrar em vez de cortar — [accounts.md](features/accounts.md#regras-de-negócio).

**Launch e automação**

4. **Um launch por vez**: a sequência é reservada antes de abrir qualquer cliente e tem dono (geração); parar a fila não prende mais o app na espera entre contas — [multi-launch.md](features/multi-launch.md#uma-sequência-de-launch-por-vez).
5. **Teto de tempo** em toda chamada HTTP do launch — [launch.md](features/launch.md#teto-de-tempo-das-chamadas-http-do-launch).
6. **`ClientAppSettings.json` na pasta da versão que a conta vai abrir** (com a limitação sem versão do catálogo descrita lá) — [launch.md](features/launch.md#onde-o-clientappsettingsjson-é-gravado).
7. **Roblox instalado por Bloxstrap, Fishstrap ou Voidstrap** é encontrado quando não há instalação oficial — [roblox-versions.md](features/roblox-versions.md).
8. **Auto Rejoin**: é o novo nome do Botting na interface (por dentro continua `botting`); abre a conta na versão configurada dela (old join) e reporta essa versão à guarda de conflito; intervalo até 480 min — [botting.md](features/botting.md).
9. **AFK mode**: manda uma tecla de tempo em tempo para a janela de cada conta, e só depois de confirmar que a janela do Roblox está na frente — [afk-mode.md](features/afk-mode.md).
10. **Conflito de versão** diz quais versões estão abertas — [launch.md](features/launch.md#fluxo).

**Interface e base**

11. **Presença com cookie de "viewer"**, para a lista e o Follow terem o `gameId` — [friends.md](features/friends.md#armadilhas--cuidados).
12. **Servidores recentes** guardam os Job IDs, não só os jogos, e link privado duplo-codificado não aparece mais para todas as contas — [server-list.md](features/server-list.md#servidores-recentes-job-ids).
13. **Chromium**: caminho manual e, se o download falhar, o navegador do sistema (só nos fluxos de login) — [chromium.md](features/chromium.md#qual-binário-abre-manual-baixado-ou-navegador-do-sistema).
14. **Janela em branco do WebView2 se conserta sozinha** (safe mode de vídeo), com saída do modo pela própria faixa — [webview-recovery.md](features/webview-recovery.md).

## Registro de mudanças (2026-09-26)

1. **O updater aponta para este repositório**, com chave de assinatura própria (com senha); a release publica só o MSI (o setup `.exe` saiu em 03/10/2026; o portátil também, atrás do interruptor `PUBLISH_PORTABLE` desligado) — [development.md](development.md#atualizacoes-este-repositorio-com-chave-propria).
2. **O trabalho vive na `develop`**; a `main` só recebe release (cada push nela publica uma versão) — [CLAUDE.md](../CLAUDE.md).
3. **Exceções de launch por conta**: a conta principal pode abrir com FPS, volume, qualidade, tela e janela próprios — [launch.md](features/launch.md#exceções-de-launch-por-conta).
4. **Auto Rejoin adota conta que já está em jogo**, sem relançar — [botting.md](features/botting.md#regras-de-negócio).
5. **Criação de contas com prefixo** no nome (`arvore` → `arvore_k3p9z`) — [account-creation.md](features/account-creation.md).
6. **Painel de multi-seleção apagado** (ninguém conseguia abri-lo); ação em lote fica na barra inferior e na Choose Game — [ui-layout.md](features/ui-layout.md#armadilhas--cuidados).
7. **Letra mínima de 11px** (prosa em 12px), e arrastar grupo pelo punho voltou a funcionar — [development.md](development.md#piso-de-legibilidade-do-texto), [accounts.md](features/accounts.md#regras-de-negócio).

## Registro de mudanças (2026-09-25)

**Escolher onde o lote entra**

1. **Preferência de servidor por lote**: `Best fit` (padrão — o mais cheio que ainda caiba o lote com **uma vaga de folga**), `Fullest`, `Emptiest`, `Random` e `Let Roblox choose`. O servidor é resolvido uma vez para o lote inteiro — [server-choice.md](features/server-choice.md).
2. **Aba Servers** na Choose Game: lista com ocupação, região e ping, varredura assíncrona página a página (profundidade configurável), filtro por país e Join que manda todas as contas selecionadas.
3. **Região do servidor**: a API não devolve isso, então vem de `join-game-instance` → IP → geolocalização, sob demanda e com cache. Conserta de quebra o "Load Region" do navegador de servidores, que fazia `fetch` do frontend para um serviço que hoje exige desafio do Cloudflare.
4. **Servidor repetido entre páginas** quebrava a reordenação da lista (chave duplicada no React). A varredura passou a deduplicar por Job ID — [server-choice.md](features/server-choice.md#servidor-repetido-entre-páginas).

**Contas**

5. **Criação em série no navegador**: o app preenche o cadastro (usuário, senha, 18+, masculino), confere antes se o nome está livre, e o usuário só resolve o CAPTCHA. O laço de reparo preenche apenas campo vazio, para nunca brigar com o que está na tela — [account-creation.md](features/account-creation.md).
6. **Gerador BloxGen** deixou de tentar para sempre quando o erro nunca passa (estoque vazio, saldo zerado, chave vencida).
7. **Aba Friends**: amigos online de cada conta selecionada; clicar num amigo manda todas as contas para o servidor dele. A rota antiga do Roblox saiu do ar e respondia 404 — [friends.md](features/friends.md).

**Sessão e interface**

8. **Painel de Sessão**: fila de contas entrando (cancelar uma, parar a fila — sem fechar cliente nenhum) e lista de contas em jogo (focar/fechar) — [ui-layout.md](features/ui-layout.md#painel-de-sessão-sessionpanel), [multi-launch.md](features/multi-launch.md#fila-observável-e-cancelamento).
9. **"Lembrar de mim" na tela de senha** (padrão 24 h), com a senha protegida pelo DPAPI do usuário do Windows e prazo dentro do blob cifrado — [authentication.md](features/authentication.md#lembrar-de-mim-na-tela-de-senha).
10. **Atalhos pedidos**: "x" nos chips de conta da Choose Game, botão de servidores em cada jogo da aba Games, e entrada **Create Accounts** no menu Add.

**Base**

11. **XSRF por serviço**: o token do `auth.roblox.com` é recusado pelo `apis.roblox.com` — toda chamada mutável repete uma vez com o token que o próprio serviço devolve no 403 — [authentication.md](features/authentication.md#o-token-é-por-serviço--send_with_csrf_retry).
12. **Multi-instância sem fechar o jogo aberto**: o app fecha o `ROBLOX_singletonEvent` dos clientes já rodando — [launch.md](features/launch.md#regras-de-negócio).
13. **Harness de UI** (`bun run dev:ui`): roda o frontend no navegador com o lado Tauri dublado, com cenários por URL (inclusive um com dados reais da API). Foi com ele que a lista fora de ordem foi reproduzida e a correção conferida — [development.md](development.md#validando-a-ui-no-navegador).

## Registro de mudanças (2026-09-24)

1. **Dados saíram da pasta do executável** para `%LOCALAPPDATA%\Roblox Account Manager`, com migração que copia (sem apagar a origem nem sobrescrever o destino), modo portátil por `portable.txt` e override por `RAM_DATA_DIR` — [architecture.md](architecture.md#arquivos-de-persistência).
2. **Backups dentro do app**: criar, listar, restaurar e apagar, com backup automático de segurança antes de restaurar — [backups.md](features/backups.md).
3. **Suítes de teste por funcionalidade** (`bun run t <suite>`) com auditoria no `bun run check` — [development.md](development.md#suítes-por-funcionalidade-o-dia-a-dia).

## Registro de mudanças (2026-09-22)

0. **O app segue o canal do Roblox em vez de fixá-lo.** Forçar `production` fazia o launch pelo *site* divergir e chamar o instalador do Roblox (que fecha os clientes abertos). Agora `launch_url`/`default_player_dir` leem o canal do registro, garantem que a build daquele canal esteja instalada (baixando-a em silêncio) e abrem ela direto — app e site concordam, e cada build é baixada uma única vez — [launch.md](features/launch.md#canal-do-roblox-e-a-tela-de-atualização-causa-raiz-e-fix). *(Depois, em 24/09: o `launch_url` passou a abrir sempre a build de **produção**, porque o `channel:` vazio da URL vence o registro; só o old join segue o canal do registro. O app continua sem fixar canal — a única escrita é o reparo de canal morto. O estado atual está em launch.md.)*

1. **Atualização do Roblox não abre mais o instalador dele:** quando a build production não está instalada, o launcher baixa e instala essa build sozinho (`ensure_production_player_exe` + `install_build_to_dir`, progresso no evento `roblox-build-install`) — [launch.md](features/launch.md#canal-do-roblox-e-a-tela-de-atualização-causa-raiz-e-fix).
2. **Join links:** campo único na aba Follow aceita convite de experiência, link VIP/privado, `vip:<código>`, link de jogo, servidor específico, deep link e link curto — [join-links.md](features/join-links.md).
3. **Testes:** 160 testes Rust (incluindo API do Roblox com HTTP mockado via wiremock e a costura `api/endpoints.rs`) e 164 no frontend (vitest); portão único `bun run check` — [development.md](development.md#testes).

## Registro de correções (2026-09-21)

1. Canal fixado em `production` também no old join sem catálogo (`default_player_dir`), cache da build production de 60 s e ClientSettings gravados na pasta realmente lançada (`refresh_production_version` + `get_roblox_path`) — [launch.md](features/launch.md#canal-do-roblox-e-a-tela-de-atualização-causa-raiz-e-fix). *(**Superado em 22/09 — não reintroduzir:** fixar `production` no registro quebrava o launch pelo site e fazia o instalador do Roblox fechar todos os clientes. Hoje o app não fixa canal (a única escrita é o reparo de canal morto); ver a entrada 0 de 22/09.)*
2. Mutex `ROBLOX_singletonMutex` adquirido, segurado e liberado numa thread dedicada (`multi-roblox-mutex`) — [launch.md](features/launch.md#regras-de-negócio).
3. Isolamento pré-launch não fecha mais clientes abertos: com Roblox rodando ele é pulado — [isolation.md](features/isolation.md).
4. `launch_multiple` checa o cancelamento logo antes do spawn de cada conta (Close All para a conta em andamento) — [multi-launch.md](features/multi-launch.md).
5. `kill_for_user` / `kill_for_user_graceful` só matam se o PID ainda for Roblox (proteção contra reuso de PID) — [launch.md](features/launch.md#regras-de-negócio), [watcher.md](features/watcher.md).
6. `stop_botting_mode(closeBotAccounts)` fecha só os bots da sessão, não mais todo cliente não-player — [botting.md](features/botting.md).
7. `AccountData.json`: save atômico, bloqueio de save após load com falha e após lock, `update_account` preserva cookie/senha do store — [accounts.md](features/accounts.md).
8. `make_selected_friends`: uma execução por vez, delay 500–60000 ms e nunca renova sessão (refresh desloga todas as sessões) — [accounts.md](features/accounts.md#regras-de-negócio), [authentication.md](features/authentication.md).
9. Web server: bloqueio de requisições de páginas web (Origin/Sec-Fetch-Site), senha obrigatória para cookies e `check_password` em ImportCookie e rotas de edição/ação — [webserver.md](features/webserver.md).
10. Nexus: handshake com `Origin` recusado, desconexão não apaga a conexão nova de um rejoin, auto-execute com prazo de 60 s — [nexus.md](features/nexus.md).
11. Scripts: settings secretas removidas do `window:update` e mais APIs de rede/armazenamento bloqueadas no Worker — [scripts.md](features/scripts.md).
12. Frontend: listener `account-moderated` sem nome velho, cleanup de `listen()` com flag `disposed`, `onDragEnd` na alça de arrasto, recentes gravados pela store só em sucesso e Gap da grade aplicado no blur/Enter — [ui-layout.md](features/ui-layout.md), [server-list.md](features/server-list.md), [development.md](development.md).

## Glossário rápido

| Termo | Significado |
|---|---|
| Cookie / `SecurityToken` | Valor do cookie `.ROBLOSECURITY` da conta, usado em todas as chamadas autenticadas. |
| Place ID | ID do "lugar" (jogo) no Roblox. |
| Job ID | ID de uma instância de servidor específica. |
| VIP / servidor privado | Servidor privado; no app é representado por `vip:<código>`, link com `privateServerLinkCode`, share link ou access code. |
| Grupo | String livre em `Account.Group`; prefixo numérico (`"01 Main"`) define a ordem. |
| Multi-Roblox | Rodar vários clientes simultâneos (controlado por `General.EnableMultiRbx`). |
