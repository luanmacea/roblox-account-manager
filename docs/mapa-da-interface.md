# Mapa da interface

Onde fica cada coisa e para que serve. Este arquivo existe porque o app nasceu de
um fork e acumulou funcionalidade mais rápido do que explicação: muita coisa útil
está a três cliques de distância sem nada na tela dizendo que existe.

A interface está **em inglês** (ou alemão); aqui os nomes aparecem como na tela,
com a explicação em português.

> Avaliação de usabilidade deste mesmo inventário: [ux-checkup.md](ux-checkup.md).

---

## Como o app se organiza

```
Barra de título ─ GitHub · minimizar · maximizar · fechar
Toolbar ──────── busca · selecionar tudo · Names · painel │ Add ▾ │ Session · Theme · Nexus · AFK · Avatars · Scripts · Settings
Lista de contas ─ agrupada, arrastável, com bolinhas de estado
Barra inferior ── aparece ao selecionar: Clear · Account · Actions ▾ · Choose Game
StatusBar ─────── contadores e legenda das cores
```

Três telas cobrem quase tudo: a **lista de contas** (principal), a **Choose Game**
(entrar em jogo, servidores, amigos) e o **Settings**. O resto são diálogos.

---

## 1. Toolbar

| Na tela | O que faz |
|---|---|
| `Filter accounts...` | Filtra a lista. Atenção: contas escondidas pelo filtro **continuam selecionadas** e sujeitas às ações em lote. |
| ícone de checkbox | `Select all` / `Deselect all`. |
| `Names shown` / `Names hidden` | Mascara os nomes das contas em todas as telas, inclusive toasts e confirmações (para gravar tela). Quantas letras ficam visíveis: Settings › General › `Preview Letters`. |
| ícone de painel | Mostra/esconde o painel de detalhes. Fica desabilitado quando não há exatamente uma conta selecionada. |
| `Presets` | Presets de launch: "estas contas → este jogo/servidor" salvos com nome, abertos com `Launch`; opcionalmente abrem e fecham sozinhos num horário (só com o app aberto, e fechar mexe só no que o preset abriu). Ver [presets.md](features/presets.md). |
| `Add ▾` | Todas as formas de trazer conta para dentro (abaixo). |
| ícone de gamepad | `Session`: fila de lançamento, clientes abertos e reconexão automática (padrão no resumo, chave por conta em `In game`). |
| ícone de paleta | `Theme`: editor de cores e fontes. |
| ícone de camadas | `Nexus`: controle de clientes por script Lua (exige executor externo). |
| ícone de teclado | `AFK Mode`: manda uma tecla de tempo em tempo para a janela de cada conta escolhida, para não perder o estado no jogo. **Cada envio traz a janela do Roblox para frente por um instante.** Fica aceso enquanto o modo está mandando. |
| ícone de camiseta | `Avatars`: monta avatares só com itens **oficiais gratuitos** do Roblox (aba Build, com sorteio) e os distribui entre as contas escolhidas (aba Distribute), pegando de graça as peças que faltam. Nunca gasta Robux; conta que o Roblox pede verificação é pulada. |
| ícone de terminal | `Scripts`: automação em JavaScript **do próprio gerenciador**. |
| ícone de engrenagem | `Settings`. |
| `?` | Reabre o tour de primeira execução. |

### Menu `Add`

| Item | O que faz |
|---|---|
| `Quick Add` | Pede cookie **ou** nome de usuário. Com nome de usuário a conta entra **sem sessão** (a tela avisa e a linha fica marcada) — serve só de marcador até você colar o cookie. |
| `Browser Login` | Abre um navegador embutido para você logar normalmente. É o caminho mais seguro. |
| `User:Pass Login` | Cola `usuario:senha`, uma por linha; abre o navegador para concluir cada login. Linha `usuario:senha:cookie` entra direto pelo cookie, sem navegador nem CAPTCHA. |
| `Quick Login` | Mostra um código de 6 caracteres; você o aprova em `roblox.com/crossdevicelogin` num celular ou PC **já logado** na conta e ela entra sozinha — sem colar cookie nem digitar senha no app. Se o Roblox pedir CAPTCHA nesse passo, o app avisa e manda usar o `Browser Login`. Também no diálogo Add Account. |
| `Import Cookie` | Cola um `.ROBLOSECURITY` por linha — ou `usuario:senha:cookie`, que guarda a senha junto. O cookie sai do navegador em que você já está logado: DevTools › Application › Cookies › roblox.com. |
| `Import Old Account Data` | Traz o `AccountData.json` da versão antiga do RAM. |
| `Create Accounts` | Cria contas **de graça** no navegador embutido: o app preenche nome, senha, data e gênero; **você resolve o CAPTCHA**. O campo `Name prefix` padroniza os nomes do lote: `arvore` gera `arvore_k3p9z` (prefixo + 5 caracteres sorteados). |
| `Account Generator` | Compra contas prontas de um serviço **pago de terceiro** (BloxGen), com chave de API. **Escondido** desde 03/10/2026 (`ENABLE_ACCOUNT_GENERATOR` desligado — ver [account-creation.md](features/account-creation.md)). |
| `Roblox Versions` | Gerencia versões instaladas do cliente Roblox (mora aqui por acidente histórico). |

---

## 2. Lista de contas

- **Seleção**: clique, `Ctrl+clique` (alterna), `Shift+clique` (intervalo), arrastar no vazio (retângulo), `Ctrl+A`, `Esc`.
- **Arrastar a alça `⋮⋮`** reordena dentro do grupo ou move para outro grupo.
- **Setas ▲▼ à direita** de cada conta e de cada cabeçalho de grupo fazem o mesmo, uma posição por clique, e são o único caminho que funciona pelo teclado. Nas contas o movimento é limitado ao grupo: subir a primeira a jogaria para outro grupo, o que é mudar de grupo e não de ordem.
- **Largar texto com cookie** na lista adiciona a conta.
- **Grupos**: o cabeçalho colapsa, aceita drop, tem checkbox e um punho de arrastar. Um **número no começo do nome ordena o grupo** e some da exibição (`1 Main` mostra `Main`) — convenção herdada do RAM antigo. Arrastar o grupo (ou usar as setas) grava uma **ordem manual** que passa a mandar, e ela sobrevive a fechar o app.
- **Bolinhas** (da esquerda para a direita): vermelha = sessão inválida ou conta sem cookie; laranja = `idle 20d+` (nenhum uso registrado há 20 dias ou mais); âmbar = cliente aberto pelo RAM; azul/verde/violeta = online / em jogo / no Studio.

### Clique direito numa conta

`Set Alias` · `Set Description` · `Copy ▸` (Cookie, Username, Password, User:Pass, User ID, Profile Link) · `Focus client` · `Restart client` · `Move to Group ▸` · `Copy Group` · `Sort Alphabetically` · `Toggle Group Visibility` · `Show Details` · `Quick Login` (confirma o código de 6 dígitos que o Roblox mostra em outro dispositivo) · `Remove Account`.

Com **Developer Mode** ligado aparecem também `Copy ▸ rbx-player Link`, `Copy ▸ App Link`, `Get Auth Ticket` e `View/Edit Fields`.
Com **Auto Rejoin** ativo aparece `Add N account(s) to Auto Rejoin`.

### Painel da conta (uma selecionada)

Nesta ordem: as ferramentas `Server List`, `Utilities`, `Browser`, `Join Group`; `Launch Exceptions`; `Roblox Version (all accounts)` — é a versão **global**; a versão por conta existe no backend (campo `RobloxVersion`, honrado pelo launch e pelo Auto Rejoin), mas **não tem tela**: só dá para definir em `View/Edit Fields` (Developer Mode), por script ou pelo web server —; Alias; descrição; e por último `History`, recolhível pelo título (onde a conta jogou, por quanto tempo e como cada sessão terminou; `Join again` volta ao servidor de uma sessão recente; `Export CSV` — ver [history.md](features/history.md)). A reconexão automática **não** fica aqui: está na página `Session` (ver abaixo).

### Barra inferior (qualquer seleção)

`Clear` · `Account` (abre o painel) · `Actions ▾` · **`Choose Game`**.

`Actions` reúne: `Refresh Cookies`, `Copy All Cookies`, `Make Friends` (modo *mesh* = todos com todos, *star* = todos com uma, com o **intervalo entre pedidos** em segundos no próprio submenu), `Move to Group`, `Restart Launched`, `Open Auto Rejoin`, `Add to Auto Rejoin`, `Close All Roblox`, `Remove`.

O andamento do Make Friends aparece no **Painel de Sessão** (botão `Session` na barra de cima, ou aba `Console` da Choose Game): uma linha por conta com aguardando / processando / amizade feita / erro, e o contador "X / Y contas processadas".

---

## 3. Choose Game

Abre com contas selecionadas e mostra `N accounts will be launched together` com chips removíveis. Sai com `Back` ou `Esc`.

`Save as preset` (no cabeçalho) guarda essas contas como preset de launch, para abrir depois com um clique pela Toolbar › `Presets`.

| Aba | Para que serve |
|---|---|
| `Favorites` | Seus jogos salvos, com servidores VIP/privados guardados por jogo. Tem `Browse servers` como as outras listas. |
| `Games` | Busca na Roblox. Cada card traz `Browse servers`, `Favorite` e `Join Game`. |
| `Recent` | Jogos abertos recentemente, com as mesmas ações das outras listas (tamanho em Settings › General › `Max Recent Games`). |
| `Servers` | Varredura de servidores do place, ordenada por **quanto o lote cabe**. Filtros: `Sort by`, `Region`, `Pages to scan`. |
| `Friends` | Amigos online de cada conta; `Join` manda o lote inteiro para o servidor do amigo. |
| `Follow` | **`Join link`** (cola qualquer link de convite/servidor privado) e **`Follow a Player`** (por nome de usuário). Também atalhos para Server List, Utilities, Auto Rejoin e Scripts. |
| `Console` | Painel de sessão e **histórico ao vivo das ações** — launch, Auto Rejoin e Watcher, cada linha com a origem entre colchetes (`[rejoin]`, `[watcher]`). É aqui que aparece o motivo de uma falha, inclusive por que o Watcher fechou um cliente. |
| `Windows` | Organiza as janelas do Roblox em grade nos monitores escolhidos (`Arrange in grid`). |

### Clique direito num jogo (Games, Favoritos, Recentes)

`Join Game` · `Browse servers` · `Favorite` (ou `Rename`/`Remove`, nos favoritos) · **`Auto Rejoin`** · **`Scripts`** · `Copy Place ID`.

As duas em negrito abrem a tela já **com aquele jogo preenchido** — antes era preciso copiar o Place ID e colar na mão.

### Aba Servers, em detalhe

- `Sort by`: **Best fit** (padrão) procura o servidor mais cheio em que o lote ainda caiba deixando **uma vaga de folga**; depois `Fullest`, `Emptiest`, `Random` e `Let Roblox choose`.
- `Pages to scan`: cada página são 100 servidores. Jogo grande precisa de mais páginas.
- O campo `Place ID` aceita a **URL do jogo** colada. Link de convite e `share?code=` não carregam place: esses vão na aba Follow, em `Join link`.
- Assim que o place é reconhecido, **o nome e o ícone do jogo aparecem ao lado** — um número de 10 dígitos não diz qual jogo é, e esta aba manda todas as contas selecionadas de uma vez. O mesmo vale na barra de launch, no Auto Rejoin e no Nexus.
- `Region` só filtra depois que as regiões forem resolvidas (`Check servers`, que também marca os servidores **sem permissão** para a conta da consulta), porque a região não vem da API do Roblox — sai do IP do servidor.
- A linha diz `N free` ou `N free · needs M`; quando nada cabe, o resumo explica em vez de fingir.

---

## 4. Settings

Seções numa lista vertical à esquerda. As mais úteis no dia a dia:

| Aba | O que mora ali |
|---|---|
| `General` | Idioma, updates, **Multi Roblox**, **Auto Rejoin**, `Launch one account at a time` e o atraso entre lançamentos (piso de 8 s), presença, nomes ocultos, navegador de login. |
| `Backups` | Criar, listar, restaurar e apagar backups (contas, settings, scripts e temas), com a pasta de dados e o aviso de chave no zip. |
| `Developer` | `Enable Developer Mode` (destrava itens do menu de contexto), web server, diagnóstico de mutex. |
| `WebServer` | API HTTP local para ferramentas externas. A aba é sempre visível; os ajustes destravam com Developer Mode ou com o servidor ligado. A senha precisa de 6+ caracteres ou o servidor responde 401 a tudo. |
| `Watcher` | Vigia o cliente do Roblox: fecha se cair a conexão, se a memória baixar, se o título mudar. |
| `Account Generator` | Provedor pago de contas: endpoint, chave de API, tipo de conta, grupo de destino. **Escondida** com `ENABLE_ACCOUNT_GENERATOR` desligado (o padrão). |
| `Isolation` | Limpeza de rastros antes de cada launch (cache, registro, MachineGuid, MAC). Windows. |
| `Versions` | Versão padrão do Roblox e o baixador de versões. |
| `Optimization` | FPS, gráficos, tamanho de janela e política de processo do Windows — um perfil por papel: `Normal`, `Auto Rejoin Main` e `Auto Rejoin Alt` (os dois últimos só aparecem com o Auto Rejoin ligado e perfis separados). |
| `Misc` | Sincronia dos campos de launch, shuffle de Job ID, criptografia, **trancar por inatividade** (só com senha do app; a tela tranca e tudo continua rodando) e "lembrar senha". |

Em General também fica, **desligado**, `Experimental: keep clients open across teleports` (só com Multi Roblox): reserva o nome que o Roblox usa para permitir uma janela só, para um teleporte não fechar outra conta. Ainda falta teste com teleporte de verdade.

**Dois interruptores em General mudam o app inteiro:** `Multi Roblox` (várias instâncias ao mesmo tempo) e `Auto Rejoin` (destrava todo o ciclo de rejoin automático). Sem eles ligados, várias funcionalidades simplesmente não aparecem.

---

## 5. Diálogos de ferramenta

| Diálogo | Onde | Para que serve |
|---|---|---|
| `Scripts` | toolbar | **JavaScript rodando dentro do RAM**, num Web Worker isolado, com a API `ram.*` para automatizar o gerenciador (lançar contas, ler settings, HTTP, UI própria). **Não** é executor de Roblox e **não** precisa de injector: nada disso entra no cliente do jogo. As 8 permissões por script controlam o que ele pode tocar. |
| `Nexus` (título na tela: `Account Control`) | toolbar | Servidor WebSocket que conversa com o `Nexus.lua` **executado dentro do Roblox por um executor de terceiros**. Só então dá para mandar comandos e scripts Lua para os clientes. |
| `Auto Rejoin` | Actions ▾ ou Choose Game › Follow | Mantém um grupo de contas alt dentro de um servidor: a cada N minutos **fecha e relança** cada alt. As contas marcadas como **main** abrem uma vez e **nunca** são reiniciadas pelo timer; a carência (`player_grace_minutes`) é da main **rebaixada** a alt com o cliente aberto. Exige Multi Roblox. |
| `Account Utilities` | painel da conta › Tools | Operações na conta Roblox: display name, privacidade, **trocar senha**, **trocar e-mail**, PIN, encerrar outras sessões, bloqueios, outfits, avatar por JSON. |
| `Roblox Versions` | Add ▾ ou Settings › Versions | Instala, rotula e remove versões do cliente; `Browse` lista o catálogo remoto. |
| `Backups` | Settings › Backups | Cópia de contas, settings, scripts e temas; restaura com backup de segurança automático. |
| `Theme Editor` | toolbar | Cores, estilo de botão e fontes; presets exportáveis. |
| `Launch check` | Settings › General › `Launch does nothing?` ou o botão `Check what's wrong` da faixa de erro do launch | Confere Roblox instalado, pastas graváveis, internet até o Roblox, processos do Roblox sem janela e o Multi Roblox; cada linha diz o que fazer. **Só olha**: nunca fecha nada. Ver [support.md](features/support.md). |
| `Session` | toolbar | Fila de lançamento (cancelar), clientes abertos (focar, fechar; cada conta com o jogo, servidor público/privado, tempo em jogo e estado — jogando, caiu, reconectando, não responde) e reconexão automática: padrão `Reconnect accounts that drop` no resumo, chave por conta em cada linha de `In game` e, com linhas marcadas, `Reconnect on` / `Reconnect off` / `Use default`. |

---

## 6. Rodapé

`N selected` · `N accounts` · `N online` · `N in game` · `N studio` · `N launched` · `auto rejoin next mm:ss` · `generating x/y`, e a legenda das cores.

Os contadores de presença são **mutuamente exclusivos**: quem está em jogo não é somado em `online`, e quem está no Studio tem contador próprio.

---

## 7. O que só existe no Windows

Isolation inteiro (registro, MachineGuid, MAC), **Auto Rejoin**, **AFK Mode**, política de processo em Optimization (prioridade, EcoQoS, limites de CPU/memória, fast flags), grade de janelas, `Focus client`, autostart, diagnóstico de mutex, Multi Roblox e o "lembrar senha" (DPAPI).

## 8. O que depende de flag de build

`Nexus` (`VITE_ENABLE_NEXUS`) e o `WebServer` (`VITE_ENABLE_WEBSERVER`). Ambos ligados por padrão.
