# Contas

## Objetivo

Armazenar e gerenciar as contas Roblox (alts) do usuário: sessão (cookie), metadados (alias, descrição, grupo, campos livres), ordem na lista e proteção em disco do arquivo `AccountData.json`. Também expõe as ações por conta que falam com a API do Roblox (amizade, bloqueio, avatar, privacidade, senha/e-mail etc.).

## Onde fica o código

| Parte | Arquivo |
|---|---|
| Modelo `Account` (serde, compatível com RAM v3) | [data/accounts/model.rs](../../src-tauri/src/data/accounts/model.rs) |
| `AccountStore` (load/save/criptografia/import) | [data/accounts/store.rs](../../src-tauri/src/data/accounts/store.rs) |
| Comandos de CRUD e senha | [data/accounts/commands.rs](../../src-tauri/src/data/accounts/commands.rs) |
| Criptografia (RustCrypto, DPAPI, hash do aparelho) | [data/crypto.rs](../../src-tauri/src/data/crypto.rs) |
| Chave mestra do vault (`AccountData.key`) | [data/vault_key.rs](../../src-tauri/src/data/vault_key.rs) |
| Comandos de API por conta | [commands/account_api.rs](../../src-tauri/src/commands/account_api.rs), [commands/account_helpers.rs](../../src-tauri/src/commands/account_helpers.rs) |
| Grupo `moderadas` | [commands/launch_shared.rs](../../src-tauri/src/commands/launch_shared.rs) (`MODERATED_GROUP`, `is_moderated_error`, `mark_account_moderated`) |
| Login por navegador / user:pass | [chromium/commands.rs](../../src-tauri/src/chromium/commands.rs) |
| Estado e ações no frontend | [store.tsx](../../src/store.tsx) (`loadAccounts`, `addAccountByCookie`, `removeAccounts`, `updateAccount`, `moveToGroup`, `reorderAccounts`, `unlock`, `applyEncryptionMethod`) |
| Tipo TS | [types.ts](../../src/types.ts) (`Account`, `parseGroupName`, `getFreshnessColor`) |
| Telas de senha/criptografia | [PasswordScreen.tsx](../../src/components/layout/PasswordScreen.tsx), [EncryptionSetupScreen.tsx](../../src/components/layout/EncryptionSetupScreen.tsx), [MiscellaneousTab.tsx](../../src/components/settings/MiscellaneousTab.tsx) |
| Adicionar/importar | [Toolbar.tsx](../../src/components/layout/Toolbar.tsx), [ImportDialog.tsx](../../src/components/dialogs/ImportDialog.tsx), [AccountList.tsx](../../src/components/accounts/AccountList.tsx) (drag & drop) |
| Campos | [AccountFieldsDialog.tsx](../../src/components/dialogs/AccountFieldsDialog.tsx) |
| Utilitários por conta | [AccountUtilsDialog.tsx](../../src/components/dialogs/AccountUtilsDialog.tsx), [ContextMenu.tsx](../../src/components/menus/ContextMenu.tsx) |

## Modelo de dados

`Account` é serializado em **PascalCase** (mesmo formato do RAM antigo em C#):

| Campo JSON | Rust | Tipo | Default / observação |
|---|---|---|---|
| `Valid` | `valid` | bool | `false`; `Account::new` cria com `true`. |
| `SecurityToken` | `security_token` | string | cookie `.ROBLOSECURITY`. `null` → `""`. |
| `Username` | `username` | string | |
| `LastUse` | `last_use` | datetime | formato `%Y-%m-%dT%H:%M:%S%.f`; vazio/`null` → agora. |
| `Alias` | `alias` | string | nome exibido na lista quando preenchido; até 240 caracteres (`MAX_ALIAS_LENGTH` em [types.ts](../../src/types.ts)) — a UI corta no limite ao digitar/colar em qualquer um dos três lugares que gravam alias (sidebar, menu de contexto). Os chips de "Targets" do Auto Rejoin ([RejoinConfig.tsx](../../src/components/afk-mode/rejoin/RejoinConfig.tsx)) truncam a exibição em `max-w-[160px]` e mostram o nome inteiro no `title` — só a linha da conta oferece a opção de quebrar em vez de truncar (`WrapLongNames`, abaixo). |
| `Description` | `description` | string | notas. |
| `Password` | `password` | string | senha em texto (opcional). |
| `Group` | `group` | string | `"Default"`; **omitido no JSON quando for `"Default"`**. |
| `UserID` | `user_id` | i64 | **chave única** da conta. |
| `Fields` | `fields` | map string→string | valores `null` viram `""`. |
| `LastAttemptedRefresh` | `last_attempted_refresh` | datetime | atualizado a cada tentativa de refresh de sessão. |
| `BrowserTrackerID` | `browser_tracker_id` | string | aceita alias `BrowserTrackerId`; gerado sob demanda no launch. |

### Campos (`Fields`) com significado no código

| Chave | Usado por | Efeito |
|---|---|---|
| `RobloxVersion` | [commands/versions.rs](../../src-tauri/src/commands/versions.rs), [launch.rs](../../src-tauri/src/commands/launch.rs) | Override de versão do Roblox por conta, honrado pelo launch e pelo Auto Rejoin. **Nenhuma tela o define** (o painel da conta grava a `DefaultVersion` global): só View/Edit Fields com Developer Mode, script (`update_account`) ou web server (`SetField`). Ver [roblox-versions.md](roblox-versions.md). |
| `NoCookieRefresh` | [store.tsx](../../src/store.tsx) | `"true"` exclui a conta do auto-refresh de cookie. |
| `Window_Position_X`, `Window_Position_Y`, `Window_Width`, ... | [watcher.rs](../../src-tauri/src/commands/watcher.rs) | Posição de janela salva pelo Watcher (`SaveWindowPositions`). |
| `ClientOverridesEnabled`, `ClientOverrideMaxFPS`, `ClientOverrideVolume`, `ClientOverrideGraphics`, `ClientOverrideFullscreen`, `ClientOverrideStartMinimized`, `ClientOverrideWindowWidth`, `ClientOverrideWindowHeight` | [launch_shared.rs](../../src-tauri/src/commands/launch_shared.rs) | Exceções de launch por conta — ver [launch.md](launch.md#exceções-de-launch-por-conta). |

| `MemoryLimit` | [memory_ceiling.rs](../../src-tauri/src/commands/memory_ceiling.rs) | Teto de memória desta conta em MB (`0` = sem limite); seletor na linha da conta em "In game", página Session, e lote com as linhas marcadas. Sem o campo vale `Optimization.MemoryLimit`. Só com a feature `memory-trim`. Ver [watcher.md](watcher.md#teto-de-memória). |
| `AutoReconnect` | [reconnect.rs](../../src-tauri/src/commands/reconnect.rs) | `true`/`false`: reconexão automática desta conta (chave na linha da conta em "In game", página Session; lote com as linhas marcadas). Sem o campo vale `General.AutoReconnect`. Ver [watcher.md](watcher.md#reconexão-automática). |

Qualquer outra chave é livre (editável em "View/Edit Fields").

## Fluxo

### Carregamento / desbloqueio

1. No startup ([lib.rs](../../src-tauri/src/lib.rs)) o backend chama **`load()`**, que é a única porta de entrada. Ele decide sozinho, na ordem:
   - arquivo **ausente ou de 0 byte** → cria a chave do aparelho (`.key`) e para aí. Isso existe para o **primeiro `add` já gravar cifrado**; antes, um vault novo nascia em texto puro e a migração nunca acontecia;
   - arquivo **cifrado** → `load_encrypted`: usa o segredo que já estiver em memória; se não houver, só tenta a chave do aparelho **quando o `.key` existe**. Sem `.key`, é vault de senha e ele para com erro (não vale gastar um Argon2 por candidato de aparelho a cada boot para descobrir isso);
   - arquivo em **texto puro** (ou DPAPI legado do RAM v3) → lê e **migra** (ver abaixo).
2. Depois disso, `needs_password()` é só "está cifrado e nada em memória abre". O frontend chama o comando `needs_password`; se `true`, [App.tsx](../../src/App.tsx) mostra `PasswordScreen`.
3. O usuário digita a senha → `unlock_accounts(password)` → `load_with_password`: calcula `sha512(senha.trim())`, descriptografa, parseia e **guarda em memória o hash e a chave já derivada** (`SessionKey`), reutilizados por todos os saves da sessão.
4. Falha de criptografia **nunca** impede o app de abrir: `lib.rs` só registra o aviso. É na tela do programa que o usuário lê o que aconteceu.

### Trancar por inatividade (ideia 27)

Opção em Settings › Misc › Security, **desligada por padrão** e só disponível com **senha do app** (com a chave do aparelho a caixa fica desabilitada, dizendo por quê).

1. Ligada, [useInactivityLock](../../src/hooks/useInactivityLock.ts) marca a hora de cada clique, tecla, roda ou movimento **na janela do MultiAlt** e confere a cada 10 s. Janela minimizada ou atrás de outra conta como inatividade.
2. Passados `General.LockAfterMinutes` minutos (1 a 240, padrão 10), `store.lockApp()` liga `appLocked`.
3. [App.tsx](../../src/App.tsx) põe a tela de senha (`LockOverlay` → `PasswordScreen mode="lock"`) **por cima** de tudo e deixa o resto `inert`. **Nada é desmontado**: Scripts, o lote de avatares e os ouvintes de eventos continuam vivos, e no backend o Modo AFK, a reconexão, o Auto Rejoin e a fila de launch nem ficam sabendo. Teclas digitadas na tela trancada não chegam aos atalhos da janela.
4. A senha vai para `verify_app_password` → `AccountStore::verify_password`, que **só confere** se ela abre o arquivo do disco: não relê contas, não troca a sessão, não grava. (O `unlock_accounts` do boot relê tudo do disco — usá-lo aqui trocaria a memória por baixo do que está rodando.)
5. Senha errada: as três primeiras são livres; depois a próxima tentativa espera 5 s, dobrando até 60 s (`app_lock_retry_delay_secs`), e nem a senha certa passa durante a espera. Acertar zera a contagem.
6. Sem a caixa "manter conectado" nessa tela: ela é do boot.

### Migração de `AccountData.json` em texto puro

Acontece na primeira abertura depois da mudança, em `migrate_plain_vault`, **nesta ordem** — ela é o único momento em que o usuário pode perder contas:

1. **`AccountData.json.bak`**, cópia byte a byte do arquivo de antes. Falha aqui **aborta a migração**: sem rede de segurança não se troca o formato do arquivo de contas.
2. **`AccountData.key`**, a chave, gravada antes do arquivo que ela cifra (na ordem contrária uma falha deixaria um vault que ninguém abre).
3. Regravação cifrada, atômica (`.json.tmp` + `atomic_replace`).

Morrer entre 1 e 2, ou entre 2 e 3, deixa o vault **em texto puro e inteiro**; a abertura seguinte recomeça daqui e reencontra a mesma chave (`ensure_device_session` é idempotente). Dentro de 3 não existe estado intermediário: o arquivo é o de antes ou o de depois.

⚠️ O `.json.bak` é o arquivo **em texto puro**, com os cookies legíveis. É de propósito — perder o arquivo é pior que ficar sem criptografia — mas quem já confirmou que as contas abrem deve apagá-lo.

### Onboarding / troca de método de criptografia

1. Primeira execução (settings recém-criadas → `EncryptionOnboardingState = pending`) e zero contas → `EncryptionSetupScreen` em modo `firstRun`.
2. Também acessível por Settings → Misc → "Change Encryption Method".
3. Opções:
   - **Pass Lock**: senha com pelo menos 8 caracteres (validado na UI e no backend) → `set_encryption_password(password)`.
   - **No Password (Device Key)**: `set_encryption_password(null)` — o arquivo **continua cifrado**, pela chave do aparelho. O rótulo já foi "Default Encryption" (escondia que era texto puro) e depois "No Password (Not Encrypted)" (verdade na época, mentira agora).
4. `set_password` **não faz cópia**, e isso é decisão. A cópia que existia (`.json.rekey.bak`) era cifrada pela chave do aparelho e o `.key` era apagado em seguida: um arquivo que nunca mais abria, que ninguém limpava e que nada avisava. O que protege esta operação não é uma cópia, é (a) a gravação atômica, que deixa o arquivo antigo intacto em qualquer falha, e (b) a ordem "chave antes do arquivo que ela cifra". Sobra de versão anterior do app é **removida**, para ninguém confiar nela. A cópia em texto puro da migração (`.json.bak`) continua sendo a rede de verdade e não é tocada aqui. Depois troca o segredo em memória e re-grava o arquivo no novo formato.
   - O latch de gravação é checado na **entrada** de `set_password`: com a gravação trancada (restauração de backup) ele não pode nem começar, senão "somente leitura" não seria literal — ele mexeria em arquivo antes de descobrir que não podia gravar. Se o store está **bloqueado** (nada em memória) e o arquivo é criptografado → erro "Accounts are locked; unlock them before changing the password." (re-cifrar um arquivo nunca decifrado gravaria uma lista vazia por cima). As duas ordens importam:
   - **definindo senha**: grava o vault com a senha e **só depois** apaga o `.key`. Na ordem contrária, uma falha na gravação deixaria um vault cifrado pela chave do aparelho sem a chave para abri-lo. E a sessão da senha só **passa a valer** se essa gravação der certo: a troca, a gravação e a volta à sessão anterior no erro acontecem com o lock de contas seguro. Antes a sessão trocava primeiro, então uma gravação que falhava (a UI dizendo "não aplicou", o `.key` ainda no disco) valia mesmo assim na próxima gravação de fundo — e o boot seguinte mandava restaurar uma chave em vez de pedir a senha;
   - **tirando a senha**: monta a chave do aparelho (gravando o `.key`) **antes** de trocar a sessão. Erro aí deixa arquivo e sessão exatamente como estavam, ainda cifrados pela senha que o usuário tem — e **sem aviso na faixa**: nada depende do `.key` que não saiu, e um `writeFailed` mandaria pôr senha em quem já tem. O erro diz "Removing the password failed … the password is still in use", não o "left unencrypted" do primeiro boot. E, como no ramo da senha, a chave do aparelho só **passa a valer** se a gravação do vault der certo; se falhar, a sessão da senha volta e o `.key` que esta tentativa criou é apagado. Antes a sessão trocava primeiro, e a próxima gravação de fundo tirava a senha que a UI disse que ficou (`removing_the_password_*`, em `vault_migration_tests`).
5. O frontend grava `General.EncryptionOnboardingState = completed` e `General.EncryptionMethod = password|default`.

### Adicionar contas

| Origem | Caminho |
|---|---|
| Cookie (Quick Add / Import Cookie / drag & drop de texto) | `validate_cookie(cookie)` → `add_account(securityToken, username, userId)` |
| `username:password:cookie` em lote (mesma caixa do Import Cookie, e também aceito na aba User:Pass e no Quick Add) | `parseImportLine` ([utils/cookies.ts](../../src/utils/cookies.ts)) → `validate_cookie(cookie)` → `add_account(..., password)` — sem navegador, porque a sessão já veio na linha |
| Quick Login (menu Add / Add Account) | código aprovado num aparelho já logado → `add_by_quick_login_poll` troca pela sessão → `validate_cookie` → `AccountStore::add`, no backend (o cookie nem passa pelo frontend). Ver [authentication.md](authentication.md#adicionar-conta-por-quick-login-ideia-12). |
| Username (Quick Add sem cookie) | `lookup_user(username)` → `add_account` com `securityToken: ""` (conta sem sessão) |
| Login no navegador | `open_login_browser` abre Chromium via CDP; ao detectar o cookie emite `browser-login-detected`; a store chama `extract_browser_cookie` (até 8 tentativas, 350 ms) → `addAccountByCookie` → `close_login_browser`. |
| user:pass em lote | uma linha `usuario:senha` por vez → `import_userpass`: abre o login, preenche `#login-username`, espera até ~240 s (480 × 500 ms) pelo cookie, valida e salva com `Password` preenchida. |
| Arquivo antigo (`AccountData.json` de RAM v3/v4) | `import_old_account_data(fileData, password?)`. Sem senha informada, tenta os **segredos que este app tem** (a sessão atual e a chave mestra do `.key`) — é o caso "exportei e reimportei nesta máquina", que não tem senha nenhuma para pedir. Só depois disso devolve `IMPORT_PASSWORD_REQUIRED` e a UI pede a senha. Vault de **outra** instalação não abre, e o erro é o mesmo (nunca um import silencioso de zero contas). |

### Remover, editar, reordenar, agrupar

- `remove_account(userId)` — a UI (barra inferior) exige digitar `REMOVE` para confirmar remoção em lote.
- `update_account(account)` substitui a conta com o mesmo `UserID` (retorna `false` se não existir), **exceto credenciais**: `SecurityToken` e `Password` são sempre copiados do registro salvo no store. O snapshot do frontend fica velho quando o backend rotaciona o cookie (refresh de sessão, `SetField` do web server…), e editar alias/grupo a partir dele não pode regravar o cookie invalidado.
- `reorder_accounts(userIds)` — usado por drag & drop e "Sort Alphabetically" (ordena por `Alias || Username` dentro do grupo).
- `moveToGroup` no frontend = `update_account` para cada conta com o novo `Group`.

## Regras de negócio

- **`UserID` é a identidade**: `add` com um `UserID` existente **atualiza** `SecurityToken`, `Username`, `Valid`, `LastUse` (e `Password` só se a nova não for vazia); alias, grupo, campos e descrição são preservados.
- `reorder`: IDs listados vão na ordem pedida; contas não listadas são anexadas no final na ordem atual.
- Toda mutação (`add`, `remove` efetivo, `update` efetivo, `reorder`, `set_password`, import com mudanças) regrava o arquivo inteiro.
- **Save atômico e sob o mesmo lock:** as mutações chamam `save_locked(&accounts, …)` **ainda segurando o guard do `Mutex` de contas**, então o que vai para o disco é exatamente o snapshot que a mutação acabou de produzir — não existe janela entre mutar e gravar em que outra escrita possa intercalar. A gravação em si vai para `AccountData.json.tmp` e troca pelo arquivo final com `atomic_replace` (`MoveFileExW` com `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` no Windows, `rename` nos demais). Crash ou disco cheio nunca deixam o arquivo truncado.
- **Ordem de lock:** `accounts` → `session`, sempre. `set_password(Some)` deriva a chave da senha **antes** de pegar `accounts` (Argon2 não roda com o lock) e só então troca a sessão, segurando `accounts` → `session` nessa ordem.
- **Argon2 só no unlock:** a chave de gravação (`SessionKey`) é derivada uma única vez por unlock/`set_password` e reutilizada. O salt de 16 bytes passa a ser sorteado por sessão em vez de por gravação; o **nonce** continua sorteado a cada gravação, e o layout do arquivo é o mesmo de antes. Antes, cada `save()` rodava um Argon2i MODERATE (256 MiB) segurando o lock na thread principal — mover N contas de grupo congelava a UI N vezes.
- **Save recusado após load com falha:** se o arquivo existe mas não pôde ser decodificado, o store marca `load_failed` e todo `save()` retorna erro ("Account file could not be loaded; refusing to overwrite it…") até um load bem-sucedido — uma lista vazia em memória nunca sobrescreve as contas do usuário.
- **Sem texto puro por cima de arquivo criptografado:** com o store bloqueado (sem segredo em memória) e o arquivo criptografado, `save()` recusa ("Accounts are locked; unlock them before making changes."). Não há exceção — `set_password(None)` também passa a cifrar, com a chave do aparelho.
- **Gravação em texto puro só existe num caminho degradado:** store sem segredo **e** arquivo que não está cifrado. É onde cai quem não conseguiu criar o `.key` (disco cheio, antivírus). É uma proteção a menos, de propósito: travar a gravação deixaria o usuário sem poder cadastrar conta, e isso é pior que gravar como antes desta mudança.
- **Linha de import (`parseImportLine`, [utils/cookies.ts](../../src/utils/cookies.ts))** — um só parser para as duas abas de import e para o drag & drop de texto:
  - o `.ROBLOSECURITY` **tem `:` dentro dele** (`_|WARNING:-DO-NOT-SHARE...`), então o corte **nunca** é `split(":")`: acha-se onde o cookie começa (`COOKIE_PATTERN`, ou o marcador `_|WARNING` quando o aviso vem torto), o que está antes é o prefixo, e dele tira-se **só** o delimitador (`\s*:\s*$`). Um `[\s:;,]+$` comeria a pontuação final de uma senha legítima;
  - a senha pode conter `:` — só o **primeiro** `:` do prefixo separa usuário de senha;
  - `user:cookie` (sem senha) vale como cookie sozinho. O `Username` gravado é sempre o que o `validate_cookie` devolve, não o da linha;
  - **linha incompleta é pulada, não importada pela metade**: `user:pass` sem cookie na aba de cookie vira "Skipped …: no cookie in this line" e a importação segue para a linha seguinte (uma conta gravada só com senha não abre nada);
  - a mesma linha `user:pass:cookie` colada na aba **User:Pass** é importada direto pelo cookie, sem abrir navegador nem pedir CAPTCHA;
  - a senha só é enviada ao `add_account` quando a linha a traz (o parâmetro é `Option<String>` no backend). Conta que **já existe** não passa pelo `add_account`, então a senha daquela linha não é guardada — e a mensagem do resultado diz isso ("…the password in this line was not stored"), em vez de só "already exists". O texto evita a palavra "saved" de propósito: ela é marcador de sucesso em `toneFromMessage` ([utils/toastTone.ts](../../src/utils/toastTone.ts)) e pintaria de verde uma mensagem que não é;
  - o Quick Add (Toolbar e Add Account, numa função só: [utils/quickAdd.ts](../../src/utils/quickAdd.ts)) lê a linha com o mesmo `parseImportLine`. Antes as duas telas decidiam com `includes(COOKIE_MARKER)` e mandavam `usuario:senha:cookie` **inteiro** como cookie — a senha viajava no cabeçalho de cookie e voltava "Invalid cookie" — e `usuario:senha` sem cookie ia para o `lookup_user` como se fosse nome. Agora só o cookie vai como cookie (a senha segue separada, como no import) e `usuario:senha` sem cookie é recusado com a indicação do User:Pass Login. O `COOKIE_MARKER` (`_|WARNING`) continua sendo a rede de segurança do parser para cookie com o aviso torto.
- **Aviso do diálogo de import:** o cookie entrega a sessão, e a senha entrega a **conta** (troca de e-mail e de senha, e sair de todas as sessões não a revoga) — e ela fica no `AccountData.json`, que é cifrado **sempre**: com a senha do app, ou, sem senha, com a chave do aparelho (`AccountData.key`, ao lado), que qualquer programa rodando como o usuário consegue abrir (ver [A chave do aparelho](#a-chave-do-aparelho-accountdatakey)). Não é "cifrado só com senha" — isso era verdade antes da criptografia por padrão. Os dois riscos estão na tela, em dois parágrafos, cobertos por teste ([ImportDialog.test.tsx](../../src/components/dialogs/ImportDialog.test.tsx)).
- **Import de arquivo antigo** (`import_old_account_data`):
  - contas com `UserID <= 0` são ignoradas (`skipped`);
  - `UserID` duplicado dentro do arquivo importado conta como `skipped` e o **último** vence;
  - `UserID` já existente é **substituído integralmente** (`replaced`); novos são anexados (`added`);
  - duplicatas na lista final são removidas; só salva se `added > 0 || replaced > 0`;
  - a senha de import é `trim()`-ada, igual à de unlock — a mesma senha colada com espaço no fim serve para os dois.
- **Grupos**:
  - string livre; vazio é tratado como `"Default"`;
  - prefixo numérico de 1–3 dígitos define a ordem (`"01 Main"` → sortKey 1, exibido como `Main`); sem prefixo → sortKey 999999, depois ordem alfabética ([types.ts](../../src/types.ts) `parseGroupName`);
  - se o único grupo existente é `Default`, a lista é mostrada sem cabeçalho;
  - **ordem manual**: arrastar um grupo pelo punho do cabeçalho reordena e grava `General.GroupOrder`. A partir daí manda a ordem manual, e o prefixo numérico/alfabética só ordena o que ficou de fora. Detalhes que importam:
    - grava a lista **inteira** dos grupos existentes, não só os visíveis — com busca ativa a tela mostra um subconjunto, e gravar "o que está na tela" apagaria da ordem os grupos escondidos pelo filtro;
    - o valor é **JSON** (`["Zeta","Alts, velhas"]`), não lista por vírgula como as outras chaves de lista do INI: nome de grupo é texto livre e pode conter vírgula;
    - valor estragado (texto que não é JSON, JSON que não é lista) vira "sem ordem manual" — a lista de contas não pode deixar de abrir por causa de uma linha torta no INI;
    - grupo que não existe mais sai da ordem; grupo novo entra no fim, não no meio;
    - o arrasto começa num **punho** no cabeçalho, não no cabeçalho inteiro, porque o cabeçalho também colapsa o grupo e recebe drop de conta. O estado do arrasto de grupo (`groupDragState`) é separado do de conta (`dragState`) pelo mesmo motivo: misturados, soltar um grupo moveria contas. Soltar um grupo sobre uma **linha de conta** não faz nada — o alvo é o cabeçalho;
    - **setas ▲▼ à direita** fazem o mesmo, uma posição por clique, e são o caminho que não depende de arrastar (e o único que funciona pelo teclado). Nas pontas a seta fica desligada. Contas têm o mesmo par de setas, limitado ao **grupo da conta**: subir a primeira do grupo a jogaria para dentro de outro grupo, que é mudança de grupo e não de ordem.

  ⚠️ **Armadilha que já mordeu (duas vezes):**

  1. O `mousedown` da lista chama `preventDefault` para desenhar a caixa de seleção, e `preventDefault` no mousedown **cancela o arrasto nativo** do navegador. A linha de conta já era poupada; o cabeçalho de grupo não, então puxar um grupo pelo punho desenhava uma caixinha de seleção em vez de arrastar. `AccountList.handleMouseDown` agora sai cedo em qualquer `[draggable='true']`, com teste.
  2. O **Tauri intercepta o drag-and-drop do sistema** na janela do WebView quando `dragDropEnabled` é `true` (o default): o WebView2 engole o gesto e os eventos HTML5 nunca chegam à página — cursor de "bloqueado" e nada se move. No navegador o mesmo código funciona, porque lá não existe a interceptação, então o harness **não pega esse caso**. Desligado em [tauri.conf.json](../../src-tauri/tauri.conf.json) (`"dragDropEnabled": false`), travado por [tauriWindowConfig.test.ts](../../src/tauriWindowConfig.test.ts). O app não usa a API de drag-drop do Tauri em lugar nenhum; depende dos eventos HTML5 para reordenar, mover conta de grupo, soltar cookie em texto e soltar arquivo no diálogo de importação — este último, aliás, também não funcionava por causa disso.
- **Grupo `moderadas`**: quando obter o auth ticket falha no launch e o erro contém `moderated`, `is banned` ou `account has been` (case-insensitive), `mark_account_moderated` move a conta para o grupo `moderadas`, persiste e emite `account-moderated`. Não faz nada se já estiver nesse grupo. O frontend recarrega a lista e mostra o toast "`<nome>` is moderated — moved to 'moderadas'". Chamado em [launch.rs](../../src-tauri/src/commands/launch.rs) nos fluxos de launch único e múltiplo.
- **Moderação (banida / advertida / encerrada)** — [commands/moderation.rs](../../src-tauri/src/commands/moderation.rs) e [api/roblox/moderation.rs](../../src-tauri/src/api/roblox/moderation.rs):
  - **Endpoint:** `GET https://usermoderation.roblox.com/v1/not-approved` (host por `endpoints::host("usermoderation")`) com o cookie da conta, pelo caminho **sem refresh** (`read_without_refresh`). Conta limpa responde `{}`. Campos lidos, e só eles: `punishmentTypeDescription` ("Warn", "Ban 1 Day"…, "Delete"), `endDate` (fim do ban, ISO; sem fuso = UTC) e `messageToUser` (nota do moderador).
  - **Classificação:** "Delete" → encerrada; tipo que começa com "Ban" **ou** tem `endDate` → banida; o resto (inclusive tipo desconhecido sem data) → advertida. Bloquear launch por um rótulo que não entendemos seria pior que deixar o Roblox recusar.
  - **Falhas:** 401 = cookie morto (não é ban); **429 / rede / 5xx = "tente depois"**, nunca "banida", e nada vai para o cache.
  - **Cache:** por conta, 10 min, só em memória (`MODERATION_TTL`). "Check ban status" no painel ignora o cache.
  - **Onde aparece:** selo na linha (`banned` com ícone de proibido para banida/encerrada, `warned` com triângulo para advertida ou ban que já acabou), com a nota do moderador no tooltip; linha no painel da conta ("Banned until dd/mm", "Warned", "Terminated", "No moderation" ou "Ban status not checked yet") com o botão **Check ban status**; a legenda da barra de status ganha esses dois selos só quando alguma conta os tem. O frontend guarda em `moderationByUserId` (evento `account-moderation`).
  - **Antes do launch** (`moderation_launch_block`, nos quatro fluxos de launch — conta única e lote, Windows e macOS): se a conta está **banida com ban valendo** ou **encerrada**, o launch dela é **pulado** com a frase "Skipped: this account is banned until dd/mm/aaaa hh:mm." (ou "…terminated…") e a nota do moderador; no lote a fila segue para a próxima. Advertência e ban que já acabou **não** bloqueiam. Qualquer falha da consulta (429, rede, cookie) **não** bloqueia: o launch segue o caminho de sempre. Teto de 8 s para a consulta não segurar a fila. Desligável em `General.CheckModerationBeforeLaunch` (Settings › General › "Check Bans Before Launch", ligado por padrão).
  - **Grupo `moderadas`:** banida (valendo) ou encerrada também passa por `mark_account_moderated`, a mesma regra do erro de auth ticket.
- **Conferir contas ("Check Accounts")** — [commands/account_check.rs](../../src-tauri/src/commands/account_check.rs), no menu **Actions › Batch Actions** da barra inferior, logo abaixo de "Refresh Cookies" (é o oposto dele: só lê). Age sobre a **seleção**: para conferir todas, "Select all" na barra de cima (ou Ctrl+A) e depois Actions.
  - Para cada conta: primeiro a moderação (acima; conta banida pode ter a sessão recusada sem o cookie estar morto), depois a sessão em `GET https://users.roblox.com/v1/users/authenticated` (campo lido: `id`) — as duas pelo caminho **sem refresh**.
  - **401** na sessão → `Valid = false` (o ponto vermelho de sempre, gravado por `AccountStore::set_valid`, que só regrava o arquivo quando o valor muda). **200** → `Valid = true` de novo. Conta sem cookie conta como inválida. **429 / rede / 5xx = "couldn't check"**: nada muda na conta.
  - **Ritmo:** no máximo 2 contas ao mesmo tempo, 600 ms de respiro antes de cada conta depois das duas primeiras, e 3 s de freio depois de qualquer "couldn't check". Um check por vez (`ACCOUNT_CHECK_RUNNING`).
  - **Tela:** o botão Actions vira "Checking 3/20…" (evento `account-check-progress`) e, no fim, um toast "18 ok, 3 invalid, 1 banned, 1 couldn't check" (+ ", N warned" quando há). A lista recarrega para mostrar os pontos vermelhos novos; banida/advertida ganham o selo de moderação.
- **Indicadores na linha** ([AccountRow.tsx](../../src/components/accounts/AccountRow.tsx)): ponto vermelho = `Valid == false`; cor de "idade" quando `LastUse` > 20 dias (ou seja, 20 dias **sem jogar**, não desde o cadastro) (amarelo → vermelho em 30 dias; desligável com `DisableAgingAlert`); âmbar = lançado pelo app; presença (Online/In Game/In Studio) se `ShowPresence`.
- **Nome longo na linha:** um alias de até 240 caracteres não cabe na linha da conta. Por padrão o nome (e o `@username` embaixo dele) é cortado com reticências; `General.WrapLongNames` troca para quebra de linha em vez de corte. `maskAccountName` ([utils/accountName.ts](../../src/utils/accountName.ts), o mascaramento de "esconder usernames") não depende do tamanho do nome — continua correto nos dois casos.
- **Busca** filtra por `Username`, `Alias`, `Description` e `Group` (case-insensitive).
- **Amizade entre contas** (`make_selected_friends`): mínimo 2 contas; modo `mesh` (todos os pares) ou `star` (todos com uma conta principal, que deve estar na seleção). Pula pares já amigos, envia pedido nos dois sentidos, espera `delayMs` → `Friends.RequestDelayMs` → 2500 ms (limitado a **500–60000**) entre pedidos, e verifica no final. O intervalo é editável no próprio submenu Make Friends da [BottomActionBar.tsx](../../src/components/layout/BottomActionBar.tsx), em segundos: o campo grava `Friends.RequestDelayMs` e manda o valor no `delayMs`, limitado à **mesma faixa do backend** — um campo mostrando 0,1 s enquanto o backend usa 0,5 s estaria mentindo. Acima de 30 pedidos a UI pede confirmação. Guarda no máximo 20 mensagens de erro.
  - **Acompanhamento por conta:** o backend guarda o estado da execução e emite `friend-link-state` com o retrato completo a cada mudança (mesmo desenho da fila de launch); `get_friend_link_state` devolve o retrato para a tela que abre no meio. Uma conta é "processada" quando todos os **pares dela** terminaram — o progresso antigo (`{phase, done, total}`) contava pares na fase de envio e não dizia quais contas já tinham acabado. Erro de envio fica na conta que **enviou** o pedido (é o cookie/CSRF dela que falhou); par que não virou amizade na verificação marca as duas.
  - **Uma execução por vez:** um `AtomicBool` estático bloqueia chamadas concorrentes → erro "Já existe uma vinculação de amizades em andamento." (a UI pode remontar e disparar um segundo lote, dobrando pedidos e o risco de rate limit/captcha).
  - **Nunca renova sessão:** a leitura de amigos (`fetch_friend_set`) não usa `run_with_session_retry` (falha vira lista vazia) e `send_directed_friend` apenas reporta erro de sessão (401) como "(sessão inválida — refaça o login da conta)". Motivo: o refresh chama `signoutfromallsessionsandreauthenticate`, que desloga a conta de todas as sessões e derruba clientes abertos (ver [authentication.md](authentication.md)).

### Criptografia

| Aspecto | Implementação ([crypto.rs](../../src-tauri/src/data/crypto.rs)) |
|---|---|
| Hash da senha | `sha512(senha.trim())` |
| KDF | Argon2i v0x13, t_cost=6 / m_cost=131072 KiB (128 MiB) / paralelismo 1, salt de 16 bytes (RustCrypto `argon2`) |
| Cifra | XSalsa20-Poly1305 (RustCrypto `crypto_secretbox`), nonce de 24 bytes, MAC de 16 bytes na frente |
| Layout | `RAM_HEADER` + salt(16) + nonce(24) + ciphertext |
| Quando o salt e a chave são sorteados/derivados | Uma vez por unlock ou `set_password` (`SessionKey` em [accounts/store.rs](../../src-tauri/src/data/accounts/store.rs)); o **nonce** continua novo a cada gravação |
| Headers aceitos | `RAM_HEADER` (ic3w0lf22) e `TRANSITION_RAM_HEADER` (niccdevs); gravação sempre com `RAM_HEADER` |
| Legado | Windows: `CryptUnprotectData` (DPAPI) com entropia fixa, só para **leitura** |
| DPAPI | Um ponto só (`crypto::dpapi_protect` / `dpapi_unprotect`, escopo do usuário, `CRYPTPROTECT_UI_FORBIDDEN`), com a entropia vinda do chamador. Usado pela chave do vault e pelo "lembrar de mim" — dois blocos `unsafe` iguais eram duas chances de errar um ponteiro |

### A chave do aparelho (`AccountData.key`)

O vault é **sempre cifrado**, mesmo sem senha. Sem senha, quem cifra é uma **chave mestra aleatória de 32 bytes** guardada em `AccountData.key`, ao lado do vault ([data/vault_key.rs](../../src-tauri/src/data/vault_key.rs)). Ela fica embrulhada **duas vezes no mesmo arquivo**, e **qualquer um dos dois abre**:

| Embrulho | O que é | Onde vale |
|---|---|---|
| `dpapi` | `CryptProtectData`, escopo do usuário do Windows, entropia `RAM4 vault master key v1` | Windows. É a proteção de verdade |
| `device` | `crypto::encrypt` com um hash derivado do aparelho (`<identificador>\|<usuário>\|ram-device-v1`) | Todos os sistemas. É o seguro contra "o DPAPI parou de abrir" |

Ter só um seria o app trancar o usuário fora das contas na primeira vez que aquele um falhasse.

**Todo open bem-sucedido regrava o `.key`** (`recover_and_refresh_master_key`), incondicionalmente. Não é otimização, é a correção de um jeito de perder as contas em silêncio: se o usuário renomeia o PC, o blob `device` fica preso ao nome antigo e **morre sem ninguém notar**, porque o DPAPI continua abrindo. Meses depois o perfil é recriado — exatamente o caso para o qual o segundo embrulho existe — e não sobra caminho nenhum. Conferir sem regravar custaria o mesmo Argon2 que regravar, então não há motivo para só conferir. Falha na regravação não é fatal: a chave em memória continua boa e o `.key` de antes continua no disco.

Pela mesma razão a `SessionKey` **guarda a chave mestra**, não só o hash derivado: se o `.key` desaparecer **ou ficar ilegível** com o app rodando (antivírus, limpeza de disco, queda de energia no meio da regravação), `save_locked` o recria. Sem isso o app seguiria gravando o dia todo um vault que ninguém mais abre, e cada backup automático criado depois levaria o vault **sem** a chave dele — até a poda apagar os backups bons.

A condição é **"o `.key` guarda a chave desta sessão"** (`key_file_holds_master`). Foram três níveis, e cada um existiu por um defeito real: "não existe" deixava passar o arquivo truncado; "não abre" deixava passar o `.key` que abre para **outra** chave — pasta de dados em OneDrive/Dropbox, em que o `.key` volta a uma versão anterior e o vault fica novo, e o boot seguinte pede uma senha que nunca existiu; só "abre e é a minha" fecha os dois. É a mesma comparação que `resolve_dpapi_blob` faz.

**O tom do aviso foi medido, não deduzido.** A primeira versão classificava transitório-vs-permanente por `ErrorKind` e saía **invertida nos dois casos que importavam** (Windows, `rustc 1.96`): antivírus com handle exclusivo chega como `raw_os_error 32` / `Uncategorized` e levava o alarme vermelho, enquanto diretório no lugar do arquivo e ACL negada chegam como `5` / `PermissionDenied` e levavam "tento de novo na próxima alteração" **para sempre**. Os códigos 32 (`ERROR_SHARING_VIOLATION`) e 33 (`ERROR_LOCK_VIOLATION`) não têm `ErrorKind` estável, então só o `raw_os_error` os identifica; e `PermissionDenied` aparece nos dois lados, ou seja, é ambíguo. **Ambíguo conta como permanente**: sub-avisar é o que custa contas, e um vermelho falso se limpa na gravação seguinte. Um `.key` truncado existe e passava batido — e essa janela deixou de ser "uma vez na vida do arquivo" para ser "a cada boot" justamente por causa da regravação acima. No caminho saudável a checagem custa uma chamada de DPAPI (sem Argon2); só o caminho quebrado paga caro, e paga uma vez. As gravações do vault e do `.key` são **write → fsync → rename**: sem o fsync o rename pode publicar um arquivo cujo conteúdo ainda está em cache, e uma queda de energia entre os dois deixaria um arquivo que existe e está vazio.

**Falha com o `.key` vira faixa na tela, não `eprintln!`.** Numa build GUI o stderr não vai a lugar nenhum, e este é o defeito que passa o dia inteiro invisível (a chave está em memória, tudo funciona) para virar lockout no boot seguinte — quando não há senha para digitar, porque nunca houve senha.

| Parte | Onde |
|---|---|
| Estado no backend | `AccountStore::vault_key_warning` → `VaultKeyWarning { code, path, detail }` |
| Comando | `vault_key_warning` ([accounts/commands.rs](../../src-tauri/src/data/accounts/commands.rs)) |
| Evento | `vault-key-warning-changed` (payload: o aviso, ou `null`), publicado a **cada mudança** do slot por `forward_vault_key_warning`, ligado no `setup` do [lib.rs](../../src-tauri/src/lib.rs) |
| Estado no frontend | `store.vaultKeyWarning`, lido **no boot** e a cada `loadAccounts`, e atualizado **na hora** pelo evento |
| Tela | [VaultKeyBanner.tsx](../../src/components/layout/VaultKeyBanner.tsx), faixa fixa ao lado da do update |

Quatro detalhes que são correção de bug, não estilo:

- **Gravação de fundo avisa na hora, por evento.** O aviso nasce dentro do `save_locked`, e quem grava sem a UI pedir — Auto Rejoin a cada ciclo, Watcher, servidor HTTP — não passa por nenhuma leitura da tela. Com o Auto Rejoin a noite inteira, o `.key` que ficava ruim de madrugada virava aviso só no backend, e o dono descobria no boot seguinte: o lockout que a faixa existe para evitar. Agora `set_key_warning` e os dois `clear_*` — os únicos pontos que mudam o slot — publicam num canal (`watch_key_warning`), **com o slot travado**, para a ordem dos eventos ser a ordem das mudanças. O store só enfileira (não bloqueia, e o aviso muda segurando o lock de contas); quem chama o `emit` do Tauri é uma thread própria, fora de qualquer lock do store, porque um comando síncrono na thread principal (restaurar backup) pode estar esperando esse lock. Na UI, uma leitura que já estava a caminho quando um evento chegou é descartada: ela é mais velha que ele. Escolhido no lugar de consulta periódica porque cobre todo gravador presente e futuro sem cada um lembrar de nada, mostra **e tira** a faixa no mesmo instante, e não deixa um timer rodando para sempre por um evento raro (que o WebView2 ainda estrangularia com a janela minimizada).
- **A consulta acontece no boot.** O efeito de inicialização do `store.tsx` não passa por `loadAccounts` (chama `get_accounts` direto), e o backend descobre o problema no `load()` do startup. Sem a chamada explícita ali, o aviso só apareceria depois de uma mutação — e quem usa a chave do aparelho, o único afetado, pode passar a sessão inteira sem fazer nenhuma e sem nunca destrancar por senha.
- **`null` limpa.** A primeira versão usava `setActionStatusMessage`, que é substituível (qualquer "Launching…" apagava o aviso) e nunca era limpa quando o problema sumia — errava nos dois sentidos.
- **O texto vem do `code`, não do backend.** A frase mora no catálogo de i18n e passa por `t()` (Global Constraint 8); o backend manda código + caminho. `writeFailedTransient` tem tom **brando** de propósito: é quase sempre antivírus segurando o arquivo por um instante, e alarme falso treina o usuário a ignorar alarme — o que desarmaria justamente esta rede.

Os cinco códigos que a faixa desenha:

| `code` | Tom | Quando |
|---|---|---|
| `writeFailed` | vermelho | o `.key` não pôde ser gravado, e a falha **não** parece transitória. O texto manda **pôr senha antes de fechar o app** (Change Encryption Method > Pass Lock): `set_password(Some)` recifra a partir da memória e o vault deixa de depender do `.key`. E diz que **backup sozinho não resolve**: o aviso existe justamente porque o `.key` em disco não guarda a chave desta sessão, e o backup copia do disco — o zip sairia com o vault e sem chave que o abra. O texto antigo ("faça um backup agora") mandava exatamente isso |
| `writeFailedTransient` | âmbar | mesma falha, mas com cara de antivírus segurando o handle: o app tenta de novo na gravação seguinte |
| `weakWrapper` | âmbar | gravou, mas sem o embrulho do DPAPI |
| `syncUnconfirmed` | âmbar | gravou, mas o disco não confirmou o `fsync` |
| `migrationFailed` | vermelho | **o `AccountData.json` continua em texto puro** (ver abaixo) |

**`migrationFailed` é o mais consequente**, porque o usuário acha que está criptografado e não está. Se a cópia de segurança ou a regravação da migração falha, o arquivo fica **inteiro e legível**, com o cookie de todas as contas. Antes disto nada aparecia: o `load()` devolvia `Err`, o `lib.rs` fazia `eprintln!` (invisível em build GUI), o `mark_memory_fresh` já tinha limpado o `load_failed` e `needs_password()` era `false` porque o arquivo é texto puro — então o app subia normal e a tela de criptografia dizia "Device Key".

A criação do arquivo no **primeiro boot** e em `set_password(None)` passa pelo mesmo caminho: antes ela descartava o health e, quando falhava, morria num `eprintln!` do `lib.rs`. E `set_password(Some(...))` **limpa** o aviso: dali em diante o `.key` foi apagado e falar dele é falso alarme.

**Sem chave e sem sessão, `migrationFailed` vence** (`warn_left_in_plain_text`). Quando o `.key` não pode ser criado na migração, no primeiro boot ou na nova tentativa pelo Change Encryption Method, o `refresh_key_file` já deixou no slot um aviso sobre o `.key` — e os dois textos possíveis mentiam: o transitório prometia "tento de novo na próxima alteração" (sem sessão o `save_locked` nem toca no `.key`), o `writeFailed` dizia "pode não abrir depois de fechar" (o arquivo está legível e abre). Antes o `migrationFailed` só entrava com o slot vazio. Agora ele substitui o aviso do `.key` sempre que não há sessão, que é exatamente quando o arquivo está (ou vai nascer) em texto puro; com sessão, nada muda.

**O slot é único, então tem escopo e gravidade.** Dois avisos falam do `.key` e outros do `AccountData.json`, e sem isso a rede se desligava sozinha: o braço "o `.key` está saudável" limpava o slot inteiro — inclusive o `migrationFailed`, que diz que o vault continua em texto puro — e limpava **antes** de a gravação acontecer. E essa "gravação seguinte" pode ser um ciclo de Auto Rejoin, sem o dono clicar em nada. Agora:

- `clear_key_warning_for(path)` limpa **só** o aviso daquele arquivo, e **nenhum caminho limpa aviso de escopo alheio** — vale para o braço do `save_locked` e para o `refresh_key_file`, que também limpava sem escopo e antes da gravação. A exceção legítima é `set_password(Some(..))`, que limpa o slot inteiro **depois** do `save_locked` cifrado com a senha: ali o `.key` e o vault são resolvidos de uma vez;
- o aviso do vault só é considerado resolvido **depois** de uma gravação que deu certo **e** foi cifrada (no caminho degradado o arquivo continua legível, e limpar seria mentir);
- a exceção legítima é `set_password(Some(..))`, que limpa o slot inteiro **depois** do `save_locked` cifrado com a senha: ali `.key` e vault são resolvidos de uma vez;
- `set_key_warning` **nunca rebaixa** gravidade (`writeFailed`/`migrationFailed` valem mais que os âmbares) e não repete um aviso idêntico — senão o `syncUnconfirmed`, que é do mesmo tipo de volume, encobria o vermelho do `writeFailed` no mesmo `save_locked`.

**A faixa acompanha as telas de senha e de criptografia** ([App.tsx](../../src/App.tsx)), não só a tela principal. São exatamente as telas do momento de pânico — e o backend manda o usuário olhar para lá ("See the warning on screen") quando a chave não pôde ser criada. Os `return` antecipados deixavam o aviso atrás delas.

E as duas viraram `flex h-screen flex-col` com a tela em `min-h-0 flex-1 overflow-auto`: como irmãs soltas num fragmento, a faixa somava altura em cima de uma tela de viewport inteiro num `body` com `overflow: hidden`, e o rodapé saía da janela **sem rolagem** — na tela de criptografia o rodapé são os botões Continue/Cancel. Medido no harness a 900x460: antes o Continue ficava 159 px abaixo da janela e inalcançável; depois a página não estoura e o conteúdo rola. A faixa também reserva o canto direito (`pr-28`), onde mora a pílula fixa de minimizar/fechar — sem TitleBar, ela cobria o fim do texto.

Quando o embrulho novo do DPAPI não pode ser produzido, o **anterior é preservado** (`resolve_dpapi_blob`), mas só se ele abrir para a **mesma** chave mestra: preservar o de outra chave faria `load_master_key` devolver a chave errada, o que é pior que ficar sem DPAPI. Sem essa preservação, a regravação incondicional poderia trocar um DPAPI saudável por nada num instante de falha.

**O identificador do aparelho é o nome da máquina, e o `MachineGuid` é só fallback de leitura.** Isso é uma diferença deliberada em relação ao upstream, e o motivo é este projeto: o isolamento pré-launch (`Isolation.SpoofMachineGuid`) **reescreve** `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`. Amarrar a chave a esse valor seria o app trancar o usuário fora das próprias contas na primeira vez que ele usasse um recurso central — foi exatamente o defeito que o upstream precisou consertar duas vezes. A ordem dos candidatos é `COMPUTERNAME` → `MachineGuid` → a constante `ram-device`; **grava-se sempre com o primeiro**, e os outros só são tentados na leitura.

O `.key` acompanha o **vault**, não um caminho fixo (`key_file_path_for` = `with_extension("key")`), então o modo portátil leva os dois juntos. Ele entra em `DATA_FILES`, ou seja, viaja na migração de pasta e **no zip de backup** — um vault cifrado sem a chave dele é um vault perdido. Restaurar o `.key` **exige reiniciar o app** (`restored_vault_key_requires_restart` em [commands/backups.rs](../../src-tauri/src/commands/backups.rs)): o segredo da sessão é o de antes da restauração, e gravar com ele por cima de um `.key` diferente deixaria o vault sem abrir no boot seguinte.

⚠️ **Modo portátil em outra máquina: levar o pen drive não basta.** Os dois embrulhos são presos ao aparelho de origem — o DPAPI ao usuário do Windows, o outro ao nome da máquina + nome do usuário. Então o vault **não abre** na máquina B, mesmo com o `.key` do lado; o app abre normalmente, mostra a tela de senha e não toca em arquivo nenhum. Quem realmente usa o app em mais de um PC deve escolher **Pass Lock**: uma senha viaja na cabeça e abre em qualquer máquina. Isso é o preço direto de "proteger contra arquivo copiado" — a mesma propriedade que barra o ladrão barra a cópia legítima.

#### O que essa proteção vale, e o que não vale

Protege contra: `AccountData.json` **copiado sem o `.key` do lado** para outra máquina (é a condição que o comentário de [vault_key.rs](../../src-tauri/src/data/vault_key.rs) põe), e **outro usuário do Windows** no mesmo PC.

Cópia **da pasta inteira** — vault e `.key` juntos, como no pen drive do modo portátil ou numa pasta de dados sincronizada em nuvem — é o mesmo caso do backup vazado abaixo: o DPAPI não abre fora do perfil de origem, e o que sobra protegendo é o embrulho `device`.

**Não** protege **backup vazado**, e isso tem que estar dito com essas palavras: o zip de backup **tem** que levar o `.key` (senão o backup não restaura — ver [backups.md](backups.md)), então quem tem o zip tem a chave, e o que sobra protegendo é o embrulho `device` — `sha512("COMPUTERNAME|USERNAME|ram-device-v1")`, duas strings que quem tem o zip normalmente já sabe (o `USERNAME` aparece em caminhos dentro do próprio `RAMSettings.ini`). Quem guarda backup em nuvem precisa de **senha**. A tela de criptografia diz isso no lugar onde o usuário decide, e o diálogo de backups diz de novo **onde o zip é criado** (ao lado do botão de criar, enquanto não houver senha): a tela de criptografia só abre sozinha para quem ainda não tem contas, então quem já tinha contas migrava em silêncio e nunca lia a frase.

**Não** protege contra **malware rodando como o próprio usuário** — esse programa lê o `.key` e chama `CryptUnprotectData` igual ao app. Nada guardado no perfil do usuário resiste a isso. Quem quer proteção contra alguém com acesso ao perfil precisa de **senha**: aí a chave vem da cabeça do usuário e não existe em disco. Isso está escrito no módulo, na tela de criptografia (com essas palavras) e aqui — não vender proteção que não existe.

### Comandos de API por conta ([account_api.rs](../../src-tauri/src/commands/account_api.rs))

Nem todo comando com cookie renova a sessão — e a diferença é de propósito (ver [authentication.md](authentication.md#regras-de-negócio)):

- **Leituras** usam `read_without_refresh` (pegam o cookie e chamam a API direto; cookie vencido vira erro na tela): `get_robux`, `check_pin`, `get_blocked_users`, `get_private_server_invite_privacy`, `get_csrf_token`, `get_auth_ticket` e `resolve_join_link`, travadas por `read_only_retry_tests`. `get_account_game_location` e `get_presence` também não renovam.
- **Ações** pedidas na conta (pedido de amizade avulso, bloqueios, privacidade, avatar, grupo, compra, troca de senha/e-mail/display name, PIN, quick login) passam por `run_with_session_retry`.
- **Cookie novo do Roblox é gravado nos dois caminhos.** Se a resposta de uma chamada traz `Set-Cookie: .ROBLOSECURITY=…` novo, `read_without_refresh` e `run_with_session_retry` gravam-no na conta (só se ela ainda tiver o cookie enviado) — ver [authentication.md](authentication.md#cookie-novo-devolvido-numa-resposta-qualquer--apicookie_rotation).
- `make_selected_friends` nunca renova a sessão.
- `avatar_apply_batch` ([commands/avatars.rs](../../src-tauri/src/commands/avatars.rs)) também nunca renova: lê o cookie uma vez com `get_cookie`, como o `make_selected_friends` — cookie vencido só falha aquela conta (ver [avatars.md](avatars.md)).

| Comando | O que faz |
|---|---|
| `get_robux` | Saldo de Robux. |
| `get_user_info`, `lookup_user` | Info pública / busca por username (sem cookie). |
| `send_friend_request`, `make_selected_friends` | Amizade (individual / em lote). |
| `block_user`, `unblock_user`, `get_blocked_users`, `unblock_all_users` | Bloqueios. |
| `set_follow_privacy`, `get/set_private_server_invite_privacy` | Privacidade. |
| `set_avatar`, `get_outfits`, `get_outfit_details` | Avatar/outfits (copiar avatar de outro usuário). |
| `avatar_free_catalog`, `avatar_list_saved`, `avatar_save`, `avatar_delete`, `avatar_apply_batch`, `avatar_cancel_batch`, `get_avatar_batch_state`, `invalidate_avatar_headshots` | Avatares grátis: catálogo oficial gratuito, avatares salvos e o lote que os distribui entre contas — ver [avatars.md](avatars.md). |
| `join_group` | Entrar em grupo Roblox. |
| `purchase_product` | Compra com preço e vendedor esperados. |
| `change_password` | Troca senha; se a resposta trouxer novo cookie, persiste. |
| `change_email`, `set_display_name` | Conta. |
| `quick_login_enter_code`, `quick_login_validate_code` | Aprovar login por código de 6 dígitos. |
| `check_pin`, `unlock_pin` | PIN da conta (4 dígitos). |
| `get_place_details`, `get_servers`, `get_universe_places`, `get_asset_*` | Usam cookie **se** `userId` for passado; sem retry. |

## Configurações relacionadas

| Seção.Chave | Default | Uso |
|---|---|---|
| `General.EncryptionMethod` | `default` | Método escolhido (`default`/`password`), informativo. |
| `General.EncryptionOnboardingState` | `pending` em instalação nova, `completed` se o INI já existia | Abre o onboarding. |
| `General.LockOnInactivity` | — (`false`) | Tranca a tela depois de um tempo sem usar a janela; só vale com senha do app. Ver [Trancar por inatividade](#trancar-por-inatividade-ideia-27). |
| `General.LockAfterMinutes` | — (`10`) | Minutos sem interação até trancar (1 a 240). |
| `General.AutoCookieRefresh` | `true` | Auto-refresh (ver [authentication.md](authentication.md)). |
| `General.DisableAgingAlert` | `false` | Esconde indicador de idade. |
| `General.CheckModerationBeforeLaunch` | `true` | Consulta a moderação logo antes de abrir cada conta e pula a banida/encerrada. |
| `General.GroupOrder` | `[]` | Ordem manual dos grupos, em JSON. Vazio/`[]` = ordem automática (prefixo numérico, depois alfabética). |
| `General.HideUsernames`, `HiddenNameLetters`, `ShowAvatarsWhenHidden`, `HideRobuxWhenHidden` | `false` / `0` | Mascaramento de nomes. |
| `General.ShowPresence`, `PresenceUpdateRate` | `true`, `5` | Presença na lista. |
| `Friends.RequestDelayMs` | (sem default no INI → 2500) | Intervalo entre pedidos de amizade, em ms. Editável no submenu Make Friends (em segundos). |
| `Login.PersistentProfile`, `Login.StealthMode` | `true`, `true` | Comportamento do navegador de login. |

## Armadilhas / cuidados

- **"Sem senha" não é mais "sem criptografia".** O `AccountData.json` é sempre cifrado; sem senha, pela chave do aparelho. A opção se chama **"No Password (Device Key)"** e a tela diz o limite real dela: barra um `AccountData.json` copiado e outro usuário no PC — **não** barra programa rodando como você, e **não** barra backup vazado (o zip leva a chave junto, por obrigação). Essa última parte é fácil de errar na doc: se você encontrar "backup vazado" listado como protegido em qualquer lugar, é bug de texto — ver a seção da chave do aparelho acima.
- **`is_accounts_encrypted` responde "tem senha", não "os bytes estão cifrados".** O comando devolve `has_user_password()`, porque é isso que a tela de criptografia sempre quis dizer e agora os bytes estão cifrados nos dois casos. O sinal é a presença do `.key` (existe com chave do aparelho, é removido quando há senha). `AccountStore::is_encrypted()` continua sendo o fato bruto do arquivo e é usado só pelas guardas de gravação.
- **Fail-safe nas guardas, não fail-open.** `vault_may_hold_data()` trata "não consegui saber se existe vault cifrado" como **"existe"**. Essa guarda decide se uma chave mestra nova pode ser sorteada por cima da antiga, e `is_encrypted().unwrap_or(false)` fazia o contrário. As duas leituras falham juntas mais do que parece — varredura de antivírus e rehidratação do OneDrive pegam a **pasta inteira** —, então `.key` ilegível e `AccountData.json` ilegível são eventos correlacionados.
- **Chave irrecuperável não apaga nada.** Se o vault está cifrado e o `.key` não abre (Windows reinstalado, perfil novo, arquivo trocado por antivírus), `load()` devolve erro e **não toca no arquivo**; o latch de `load_failed` recusa toda gravação depois disso. Perder o arquivo é pior que ficar sem criptografia. A mensagem do unlock também muda quando o `.key` existe — pedir a senha de novo não resolveria esse caso. A mensagem só cita o `.json.bak` **quando ele existe**: numa instalação que nasceu cifrada esse arquivo nunca existiu, e mandar restaurá-lo no momento de pânico é mandar caçar um arquivo inexistente. E ela dá o caminho **à mão, em ordem**, porque é lida na tela de senha — que só tem senha e Continue; Settings não abre com as contas trancadas, e a versão anterior mandava "restaurar pelo Settings": 1) fechar o app; 2) tirar `AccountData.json` e `AccountData.key` da pasta de dados, guardando os dois; 3) pôr de volta os dois de um zip da pasta `backups` (ou o `.json.bak` renomeado, quando existe — texto puro de antes da cifra, sem as contas adicionadas depois); 4) abrir o app. O Settings só entra **depois** do passo 2: com os dois arquivos fora, o app abre vazio, restaura o backup pela tela e pede reinício.
- **`.key` que abre mas não é a chave deste vault.** Dois caminhos chegam aí e a mensagem serve aos dois: o `.key` órfão de um `set_password` interrompido (o vault é de senha, basta digitá-la — e o órfão é **removido** quando a senha abre o arquivo), e o vault cifrado por **outra** chave de aparelho (o usuário copiou um `AccountData.json` antigo por cima, ou restaurou um vault sem a chave dele). A mensagem começa com "Password required" mas continua dizendo que, se nunca houve senha, o caminho é restaurar o `AccountData.key` correspondente — pedir senha e parar seria um beco sem saída para quem nunca teve uma. E, como a do item acima, dá os passos **à mão, em ordem** (fechar o app; tirar `AccountData.json` e `AccountData.key` da pasta de dados, guardando; pôr de volta os dois **do mesmo zip** da pasta `backups`; abrir o app), sem mandar para o Settings, que não abre com as contas trancadas.
- **Senha errada sem `.key`.** Pelos arquivos, "senha errada" e "vault sem senha cujo `.key` sumiu" (usuário, antivírus, limpeza de disco) são o mesmo estado. A mensagem do unlock começa pelo caso comum ("Failed to decrypt: wrong password.") e depois cita o arquivo de chave que falta e os mesmos passos à mão — tirar o `AccountData.json` do lugar guardando, pôr de volta `AccountData.json` e `AccountData.key` de um zip de backup — em vez do antigo "restaure de um backup", que não dizia como (`a_failed_unlock_without_a_key_file_says_how_to_restore_by_hand`).
- **Duas travas de gravação, por motivos diferentes.** `load_failed` = o **arquivo** em disco não pôde ser lido. `write_block` (`lock_writes_until_restart`) = o arquivo está bom e é a **memória** que está velha; é o caso da restauração de backup. Sem a segunda, restaurar um backup e lançar uma conta (`mark_used` → `save`) sobrescrevia o vault restaurado com o segredo da sessão anterior — lockout no boot seguinte. Só reiniciar o app limpa as duas, e `allow_writes_after_reload` é o **único** ponto que solta a segunda (ver [backups.md](backups.md)). Ligar a segunda **espera a gravação em andamento terminar** (toma o mutex de contas antes): senão uma gravação que já tinha passado pela checagem publicava o vault velho depois da restauração.
- Esquecer a senha = perda do arquivo; não há recuperação no código. Perder o `.key` **junto com** o vault cifrado por ele, idem.
- Nenhum log, mensagem de erro ou payload carrega cookie, senha ou chave: o `.key` guarda só os dois embrulhos, e `own_secret_hashes` circula hashes derivados, nunca a chave mestra.
- A senha é `trim()`-ada em todos os caminhos que a consomem: `load_with_password`, `set_password` e `decode_accounts_for_import`.
- Não altere `RAM_HEADER`, parâmetros do Argon2 ou o layout sem migração: quebra a leitura de todos os arquivos existentes. **Os parâmetros do Argon2 são os que o libsodium usava** (o formato foi mantido ao trocar a lib): o `decrypts_a_libsodium_fixture` em [crypto.rs](../../src-tauri/src/data/crypto.rs) trava isso decifrando um arquivo gravado pela versão antiga.
- **Por que RustCrypto e não sodiumoxide:** o libsodium (biblioteca C embutida no `.exe`) fazia o binário ser marcado como `Trojan:Win32/Wacatac.B!ml` pelo Windows Defender — falso positivo de modelo de ML, achado por bissecção com `bun run vt` em 28/09/2026. A criptografia em Rust puro (`argon2` + `crypto_secretbox` + `sha2`) faz o mesmo, no mesmo formato, e o binário sai 0/75 no VirusTotal e limpo no Defender.
- `update_account` substitui o objeto inteiro (menos `SecurityToken`/`Password`, que vêm do store) — sempre envie a conta completa (o frontend faz `{ ...account, Campo: valor }`). Para trocar o cookie use `add_account` (mesmo `UserID`) ou os fluxos de refresh; `update_account` ignora cookie/senha enviados.
- **Latch de arquivo ilegível (vale para todas as stores de dados).** Quando um arquivo de persistência existe mas não pôde ser lido ou parseado, a store carrega vazia/no default, marca `load_failed` e **recusa toda gravação** até o arquivo ser corrigido/restaurado e o app reiniciado — assim a primeira gravação não apaga os dados do usuário. Mudanças feitas nessa sessão retornam erro. Hoje aplicam o latch: `AccountStore` (`AccountData.json`), `ScriptStore` (`RAMScripts.json`), `AvatarStore` (`RAMAvatars.json`), `VersionsCatalogStore` (`RAMVersions.json`), `ThemePresetStore`, `ThemeStore` e `SettingsStore` (`RAMSettings.ini`). Um arquivo de 0 byte **não** conta como corrupção (é um estado vazio legítimo e continua gravável).
- **`LastUse` mede uso, não cadastro.** É escrito ao criar/atualizar via `add` e, desde então, também a cada launch que dá certo: `AccountStore::mark_used` é chamado em `launch_queue_mark` quando a fila marca `Done` (cobre conta única e lote, nas duas plataformas), no ciclo do Auto Rejoin (`launch_account_for_cycle`) e nos dois launches do web server (que não passam pela fila). Estado que não seja `Done` não conta — tentativa não é uso.
- O indicador de idade e o auto-refresh de cookie dependem dele, e é por isso que a diferença importa: **antes**, uma conta jogada todo dia mas cadastrada há 30 dias entrava no auto-refresh, e o refresh desloga a conta de todas as sessões (podendo derrubar o cliente aberto). Agora o auto-refresh mira quem está de fato parado, que é o que a regra de 20 dias sempre quis dizer.
- **Copiar credencial (cookie, senha, user:pass) passa pelo backend** — `copy_account_secret(userIds, kind)` em [commands/clipboard.rs](../../src-tauri/src/commands/clipboard.rs), chamado por [utils/copySecret.ts](../../src/utils/copySecret.ts) a partir do menu de contexto (Copy › Cookie/Password/User:Pass) e do "Copy All Cookies" da barra inferior. O aviso de antes (`useCopyCredentialWarning`, desligável em `General.WarnOnCopyCredential`) continua.
  - O texto sai do **store do backend**, não do snapshot da tela (que fica velho quando o Roblox troca o cookie). Cookie/senha vazios ficam de fora.
  - **Windows** ([platform/windows/clipboard.rs](../../src-tauri/src/platform/windows/clipboard.rs)): grava o texto (`CF_UNICODETEXT`) junto com três formatos registrados — `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0 e `CanUploadToCloudClipboard` = 0 —, o que deixa a cópia **fora do histórico do Win+V e da nuvem**. Guarda o `GetClipboardSequenceNumber` de depois da cópia e, **30 s** depois, apaga a área de transferência **só se o número ainda for o mesmo** (a pessoa não copiou outra coisa). O aviso e o toast dizem "Cleared from the clipboard in 30 s".
  - **Só escrita.** O módulo nunca lê o conteúdo da área de transferência (`GetClipboardData` e afins): é o padrão de programa que rouba senha, e os antivírus o marcam. Quem sabe se o conteúdo ainda é nosso é o número de sequência. Travado por `clipboard_write_only_tests`, que varre o arquivo.
  - **Fora do Windows** o backend responde `CLIPBOARD_UNSUPPORTED` e a tela copia pelo `navigator.clipboard`, sem limpeza (e o aviso mantém o texto antigo). Qualquer **outro** erro no Windows (área de transferência ocupada) vira "Failed to copy" — não cai no caminho sem proteção.
  - APIs Win32 novas no binário (feature `Win32_System_DataExchange` do `windows-sys`): `OpenClipboard`, `EmptyClipboard`, `SetClipboardData`, `CloseClipboard`, `RegisterClipboardFormatW`, `GetClipboardSequenceNumber`; mais `GlobalAlloc`/`GlobalLock`/`GlobalUnlock`/`GlobalFree`, de features que já estavam lá.
