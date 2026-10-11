# Autenticação

## Objetivo

Encapsular a comunicação autenticada com o Roblox a partir do cookie `.ROBLOSECURITY` de cada conta: validar cookie, obter token CSRF, gerar auth ticket para o launch, renovar a sessão quando o Roblox a invalida e operações sensíveis (senha, e-mail, PIN, quick login, display name).

## Onde fica o código

| Parte | Arquivo |
|---|---|
| Chamadas HTTP de auth | [api/auth.rs](../../src-tauri/src/api/auth.rs) |
| Retry de sessão, refresh, comandos | [commands/account_api.rs](../../src-tauri/src/commands/account_api.rs) |
| Leitura do cookie da store | [commands/account_helpers.rs](../../src-tauri/src/commands/account_helpers.rs) (`get_cookie`) |
| Retry de rede genérico (429/erros) | [api/roblox/http.rs](../../src-tauri/src/api/roblox/http.rs) (`send_with_retry`) |
| Auto-refresh no frontend | [store.tsx](../../src/store.tsx) (efeito com `AutoCookieRefresh`) |
| Refresh manual em lote | [BottomActionBar.tsx](../../src/components/layout/BottomActionBar.tsx) (`handleRefreshAll`) |
| Uso no launch | [commands/launch.rs](../../src-tauri/src/commands/launch.rs) |

## Fluxo

### Cliente HTTP de auth

`build_client()` em [auth.rs](../../src-tauri/src/api/auth.rs): **sem seguir redirects** (`Policy::none()`) e user-agent de Chrome 120 no Windows. O cookie vai no header `Cookie: .ROBLOSECURITY=<token>`.

### Validar cookie — `validate_cookie`

1. `GET https://www.roblox.com/my/account/json`.
2. Status não-2xx → `Err("Invalid cookie (status N)")`.
3. Corpo parseado em `AccountInfo` (`UserId`, `Name`, `DisplayName`, e-mail, idade...). Usado ao adicionar contas por cookie.

### CSRF — `get_csrf_token`

1. `POST https://auth.roblox.com/v1/authentication-ticket/` sem token, com `Referer` fixo e `RBXAuthenticationNegotiation: 1`.
2. Lê o header `x-csrf-token` da resposta (o Roblox responde 403 com o header).
3. Se o header não vier: `Err("[status reason] corpo")`.

Todas as operações mutáveis pedem um CSRF novo antes (não há cache global; o único reuso é por conta dentro de `make_selected_friends`).

### O token é **por serviço** — `send_with_csrf_retry`

O XSRF do Roblox não vale em todos os domínios. O token lido de `auth.roblox.com` é recusado por `apis.roblox.com` com:

```json
{"errors":[{"code":0,"message":"XSRF token invalid"}]}
```

…e esse mesmo 403 já devolve, no header `x-csrf-token`, o token que aquele serviço aceita (confirmado contra a API: repetir a chamada com ele passa do XSRF e cai em `401`, que é só falta de cookie).

Por isso **toda** chamada mutável passa por `send_with_csrf_retry(request, &csrf)` ([auth.rs](../../src-tauri/src/api/auth.rs)):

1. envia com o `X-CSRF-TOKEN` que o chamador tem;
2. se voltar `403` **com** um `x-csrf-token` diferente, refaz a requisição (mesmo corpo) com o token novo — uma vez só;
3. qualquer outro caso devolve a resposta original, para o chamador reportar a mensagem do próprio Roblox.

Regras: o `RequestBuilder` passado **não** pode já ter o header (o reqwest acumula headers, e dois `X-CSRF-TOKEN` são recusados); nunca repetir o mesmo token (seria recusado de novo); sem header novo, não há retry — não existe laço. Coberto por `csrf_retry_tests` e, ponta a ponta, por `share_link_csrf_tests` (link de convite colado na UI).

### "Lembrar de mim" na tela de senha

A senha que destranca o `AccountData.json` pode ser guardada por um tempo (padrão 24 h, teto 7 dias), para não redigitá-la a cada abertura. Código: [data/accounts/remember.rs](../../src-tauri/src/data/accounts/remember.rs).

Como o arquivo vale o mínimo possível para quem não for o dono da máquina:

- é cifrado pelo **DPAPI do Windows no escopo do usuário atual**, com entropia própria do app — copiar `RAMUnlock.bin` para outra máquina, ou abri-lo com outra conta do Windows, não devolve nada;
- o **prazo mora dentro do blob cifrado**: editar o arquivo não estende a validade;
- é apagado quando expira, quando não abre mais, quando a senha muda ou sai (`set_encryption_password`), e pelo botão **Forget** em Settings → Misc → seção Security (não existe aba Security);
- é **opt-in**: só existe se o usuário marcar a caixa.

Fora do Windows não há DPAPI e a caixa nem aparece (`remembered_unlock_state.supported == false`) — guardar a senha em texto puro seria pior que digitá-la.

Comandos: `unlock_accounts(password, rememberHours)`, `try_remembered_unlock()` (chamado na inicialização antes de mostrar a tela), `remembered_unlock_state()`, `forget_remembered_unlock()`.

### Adicionar conta por Quick Login (ideia 12)

O lado de **quem entra** do Quick Login oficial do Roblox (o lado de **aprovar** um código já existia: `quick_login_enter_code`/`quick_login_validate_code`, item "Quick Login" do menu de contexto). Código: [api/auth.rs](../../src-tauri/src/api/auth.rs) (`quick_login_create`, `quick_login_status`, `quick_login_redeem`) e [commands/quick_login.rs](../../src-tauri/src/commands/quick_login.rs); tela [QuickLoginDialog.tsx](../../src/components/dialogs/QuickLoginDialog.tsx), aberta pelo menu Add e pelo diálogo Add Account.

1. `add_by_quick_login_start` → `POST apis/auth-token-service/v1/login/create` (`{}`; sem conta, com o aperto de mão do XSRF: 403 com `x-csrf-token`, repete uma vez). O Roblox devolve `code`, `privateKey` e `expirationTime`. A **chave privada fica no backend** (`PENDING_QUICK_LOGIN`); a tela recebe só o código.
2. A tela mostra o código e manda a pessoa abrir `roblox.com/crossdevicelogin` (ou Configurações › Quick Log In no app) num aparelho **já logado na conta**, digitar e confirmar.
3. A cada 3 s, `add_by_quick_login_poll` → `POST apis/auth-token-service/v1/login/status` (`code` + `privateKey`): `Created` = esperando, `UserLinked` = digitado (com o nome da conta), `Validated` = confirmado, `Cancelled`, e 400/`CodeInvalid` = vencido.
4. Confirmado: `POST auth/v2/login` com `{ctype: "AuthToken", cvalue: code, password: privateKey}` devolve o `.ROBLOSECURITY` no `Set-Cookie`. Daí é o mesmo caminho do Quick Add: `validate_cookie` → `AccountStore::add` (conta já salva = cookie atualizado). O login acaba ali, dando certo ou não: o código é de uso único.
5. Fechar a tela chama `add_by_quick_login_cancel`.

**Se o Roblox pedir verificação** (`rblx-challenge-id`/`rblx-challenge-type`, como CAPTCHA) no passo 4, o app **para** com `QUICK_LOGIN_CHALLENGE_MESSAGE` ("…MultiAlt doesn't solve those. Add the account with Browser Login instead") — não tenta resolver nem contornar. O mesmo vale para qualquer resposta sem cookie. O fluxo foi conferido em dois projetos de terceiros (só leitura, nada executado) e testado com HTTP mockado; **ainda falta um teste do dono com uma conta real** — pode ser que o Roblox exija verificação nesse passo para algumas contas, e aí a mensagem acima é o resultado esperado.

URLs sempre por `endpoints::host` (`quick_login_urls_come_from_endpoints` falha com literal `https://*.roblox.com`).

### Auth ticket — `get_auth_ticket`

1. Obtém CSRF.
2. `POST .../authentication-ticket/` com `x-csrf-token`, `Referer`, `RBXAuthenticationNegotiation: 1` e corpo vazio.
3. Retorna o header `rbx-authentication-ticket`; ausência → erro com status e corpo.
   - **Exceção — o Roblox pediu verificação.** Se a resposta traz `rblx-challenge-id` ou `rblx-challenge-type` (403 de "Challenge is required"), o erro vira uma frase que diz o que fazer: "Roblox wants to verify this account (2-step verification). Open it in the browser (account panel › Tools › Browser), finish the check there, then try again." O tipo (`twostepverification`, `captcha`, `reauthentication`; outro qualquer vira "a security check") só muda o trecho entre parênteses. **O app nunca tenta resolver o desafio.** O texto evita as palavras de `is_auth_session_error` (senão o launch dispararia o refresh que desloga a conta) e de `is_moderated_error` (senão a conta iria para `moderadas`) — travado por `auth_challenge_tests`.
4. O ticket é usado para montar o launch do cliente (ver [launch.md](launch.md)) e nos links `roblox-player://` do menu de contexto (modo desenvolvedor).

### Retry de sessão — `run_with_session_retry`

```
cookie = get_cookie(user_id)
r = operação(cookie)
se ok → retorna
se erro NÃO é de sessão → retorna erro
novo_cookie = refresh_account_session(user_id)   // falha → erro
r2 = operação(novo_cookie)
se r2 falha por sessão → "Roblox invalidated this session. Re-login required."
```

`is_auth_session_error` considera erro de sessão se a mensagem (minúscula) contém: `status 401`, `[401`, `unauthorized`, `not authenticated`, `user is not authenticated`, `invalid cookie`, `authorization has been denied`, `"code":9002` ou `code 9002`. **`token validation failed` não conta** como sessão morta: é CSRF vencido (403), e tratá-lo como sessão disparava um refresh que deslogava a conta em todo lugar.

### Refresh de sessão — `refresh_account_session` / comando `refresh_cookie`

1. `mark_refresh_attempt`: grava `LastAttemptedRefresh = agora` na conta (persistido).
2. `log_out_other_sessions`: CSRF + `POST https://www.roblox.com/authentication/signoutfromallsessionsandreauthenticate`. Aceita 2xx **ou** 3xx.
3. Extrai o novo `.ROBLOSECURITY` dos headers `set-cookie`.
4. Sem cookie novo (ou vazio) → `"Roblox invalidated this session. Re-login required."`.
5. `persist_cookie_update`: grava o novo `SecurityToken` e `Valid = true`.

### Cookie novo devolvido numa resposta qualquer — `api::cookie_rotation`

Às vezes o Roblox troca o cookie da conta no meio de uma chamada comum (`Set-Cookie: .ROBLOSECURITY=…`). Antes só o refresh e a troca de senha liam esse header; em qualquer outra hora a conta salva ficava com o cookie velho e aparecia "inválida" sem motivo.

1. Todo envio da camada de API passa por `cookie_rotation::send` (`.send_noting()` no lugar de `.send()`): em `auth.rs`, em `send_with_csrf_retry`, em `send_with_retry` e nos módulos `api/roblox/*` e `api/batch.rs`. Ele lê o cookie que **foi enviado** no pedido e, se a resposta traz um `.ROBLOSECURITY` novo, guarda em memória: cookie velho → cookie novo.
2. `read_without_refresh` e `run_with_session_retry` ([account_api.rs](../../src-tauri/src/commands/account_api.rs)), depois da chamada (com sucesso **ou** erro), chamam `adopt_rotated_cookie`, que tira o novo da memória e grava com `AccountStore::replace_token_if` — **só se a conta ainda estiver com o cookie enviado** (um login novo no meio-tempo não é sobrescrito), sob o lock do store e pelo caminho criptografado normal. Marca `Valid = true`.
3. Ignorado: `Set-Cookie` de outro nome, valor vazio, curto (< 50 caracteres) ou com caractere inválido, e cookie de **apagar** (`Max-Age=0` / `Expires` em 1970 — é como o Roblox desloga). Igual ao enviado também não conta.
4. **O valor nunca vai para log nem para mensagem de erro.** Leituras que pegam o cookie direto com `get_cookie` (amigos, presença, lote de avatares) não recolhem a rotação; o mapa em memória tem teto (256) para não crescer.

Travado por `cookie_rotation_tests` (parser), `account_token_swap_tests` (store) e `cookie_rotation_command_tests` (wiremock: resposta com cookie novo → conta atualizada; sem → igual; malformado → ignorado; pelos dois caminhos de comando).

### Auto-refresh (frontend)

A cada **5 minutos**, se `General.AutoCookieRefresh != "false"` e o app está desbloqueado, para cada conta:
- pula se `Fields.NoCookieRefresh == "true"`;
- pula se `LastUse` tem menos de **20 dias** (e `LastUse` anda a cada launch bem-sucedido, então conta em uso nunca entra aqui — o que é o desejado: o refresh desloga todas as sessões da conta);
- pula se `LastAttemptedRefresh` tem menos de **7 dias**;
- senão chama `refresh_cookie` e espera 5 s antes da próxima.

O "Refresh Cookies" manual da barra inferior faz o mesmo para as contas selecionadas, sem os filtros, com 2 s entre contas.

## Regras de negócio

- As **ações** que operam "como" uma conta (bloquear, avatar, privacidade, trocar senha/e-mail, PIN, quick login, compra, grupo, pedido de amizade avulso) e o auth ticket/private join do launch e do Auto Rejoin usam `run_with_session_retry` — no máximo **um** refresh e **uma** nova tentativa por chamada. **Leituras não** (regra logo abaixo), nem `make_selected_friends` (ver mais abaixo).
- **Regra: nunca use refresh de sessão (`run_with_session_retry` / `refresh_account_session`) em leituras ou ações não críticas.** Quem lê usa o helper `read_without_refresh` (pega o cookie e chama a API direto), e o teste estrutural `read_only_retry_tests` reprova se um comando da lista de leituras voltar a usar retry — `get_auth_ticket`, `get_csrf_token`, `check_pin`, `get_robux`, `get_blocked_users`, `get_private_server_invite_privacy` e `resolve_join_link` estão nessa lista. O refresh desloga a conta de **todas** as sessões e derruba clientes Roblox em execução. Para leituras tolerantes a falha, chame a API com `get_cookie` direto e trate o erro; para ações em lote, reporte a sessão inválida em vez de "consertá-la".
- O refresh **desloga todas as outras sessões** da conta (é o endpoint `signoutfromallsessionsandreauthenticate`): clientes Roblox abertos com o cookie antigo podem cair.
- `LastAttemptedRefresh` é atualizado mesmo se o refresh falhar.
- **`quick_login_validate_code` existe no backend ([api/auth.rs](../../src-tauri/src/api/auth.rs), com teste) e nada no frontend o chama**, de propósito registrado: o fluxo do Roblox que o app usa é `enterCode` + a confirmação que aparece no aparelho, e o frontend já barra código que não tenha 6 dígitos. A função fica disponível para quem quiser validar antes de enviar; se continuar sem uso, é candidata a remoção.
- `change_password` também pode devolver um novo cookie; ele é persistido.
- `unlock_pin` exige exatamente 4 caracteres; `quick_login_*` extrai só dígitos e exige 6.
- `check_pin` retorna `true` se o PIN estiver desativado ou desbloqueado (`unlockedUntil > 0`).
- `send_with_retry` (APIs de jogos/servidores) tenta até 3 vezes em erro de rede ou HTTP 429, com espera de 400 ms e 800 ms.
- Em `make_selected_friends`, o reuso de CSRF por conta tem regra própria: erro com `token validation` ou `status 403` → busca novo CSRF e tenta 1 vez; erro de sessão (401) → **não** renova, retorna o erro com "(sessão inválida — refaça o login da conta)". A leitura de listas de amigos (`fetch_friend_set`) também não passa por `run_with_session_retry`.
- No launch, falha ao obter ticket com mensagem de moderação move a conta para o grupo `moderadas` (ver [accounts.md](accounts.md)).

## Configurações relacionadas

| Seção.Chave | Default | Efeito |
|---|---|---|
| `General.AutoCookieRefresh` | `true` | Liga o loop de auto-refresh no frontend. |
| `Fields.NoCookieRefresh` (campo da conta) | — | `"true"` exclui a conta do auto-refresh. |
| `Developer.DevMode` | `false` | Mostra "Get Auth Ticket", "rbx-player Link" e "App Link" no menu de contexto. |

## Armadilhas / cuidados

- A detecção de erro de sessão é **por substring** da mensagem de erro. Se você criar uma função de API nova, mantenha mensagens como `"... (status 401)"` para que o retry funcione; mensagens genéricas ("Failed to X") não disparam refresh.
- `test_auth(cookie)` é um comando de diagnóstico que devolve texto com validação, CSRF e ticket truncados — não use para lógica.
- O `Referer` de CSRF/ticket é uma URL fixa de jogo (`REFERER_URL` em [auth.rs](../../src-tauri/src/api/auth.rs)); mudanças no Roblox podem exigir ajustá-la.
- Contas adicionadas só por username têm `SecurityToken` vazio: todas as operações autenticadas falham e o refresh não consegue recuperar.
- Como o refresh derruba outras sessões, evitar chamá-lo durante Auto Rejoin/multi-launch ativos. Note que o próprio launch usa `run_with_session_retry` para o auth ticket: uma conta com sessão inválida é renovada ali (e seus clientes antigos caem).
