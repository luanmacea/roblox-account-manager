// `send_noting` no lugar de `send`: o `.ROBLOSECURITY` novo que o Roblox
// devolver em qualquer resposta fica registrado para a conta (ver
// `api::cookie_rotation`).
use crate::api::cookie_rotation::SendNoting;
use crate::api::endpoints;
use crate::api::http_client;
use reqwest::header::{COOKIE, REFERER};
use serde::{Deserialize, Serialize};

/// Referer Roblox expects on the auth-ticket endpoints. A function rather than
/// a const because the host comes from `endpoints`.
fn referer_url() -> String {
    format!("{}/games/2753915549/Blox-Fruits", endpoints::host("www"))
}

/// O cliente dos endpoints de autenticação. O teto de tempo vem de
/// [`http_client::builder`]: sem ele um `auth.roblox.com` que aceita e não
/// responde prendia o pedido de auth ticket — a primeira chamada de rede de todo
/// launch — e com ele a reserva da sequência de launch.
fn build_client() -> reqwest::Client {
    http_client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Safari/537.36")
        .build()
        .unwrap()
}

fn cookie_header(security_token: &str) -> String {
    format!(".ROBLOSECURITY={}", security_token)
}

fn normalize_quick_login_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_digit()).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    #[serde(alias = "UserId")]
    pub user_id: i64,
    #[serde(alias = "Name")]
    pub name: String,
    #[serde(alias = "DisplayName")]
    pub display_name: String,
    #[serde(alias = "UserEmail", default)]
    pub user_email: Option<String>,
    #[serde(alias = "IsEmailVerified", default)]
    pub is_email_verified: bool,
    #[serde(alias = "AgeBracket", default)]
    pub age_bracket: i32,
    #[serde(alias = "UserAbove13", default)]
    pub user_above_13: bool,
}

pub async fn validate_cookie(security_token: &str) -> Result<AccountInfo, String> {
    let client = build_client();

    let response = client
        .get(format!("{}/my/account/json", endpoints::host("www")))
        .header(COOKIE, cookie_header(security_token))
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))?;

    // Conta moderada (banida, advertida, em análise): o site redireciona a
    // página para `/not-approved` em vez de responder. O cookie é bom; quem
    // diz de quem ele é passa a ser a API de usuários, que não redireciona.
    if response.status().is_redirection() {
        let to_moderation = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|loc| loc.contains("not-approved"));
        return validate_cookie_via_users_api(security_token, to_moderation).await;
    }

    if !response.status().is_success() {
        return Err(format!(
            "Invalid cookie (status {})",
            response.status().as_u16()
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    serde_json::from_str::<AccountInfo>(&body).map_err(|e| {
        format!(
            "Failed to parse account info: {} (body: {})",
            e,
            body.chars().take(200).collect::<String>()
        )
    })
}

/// Reserva do [`validate_cookie`] quando o site redireciona: a API de
/// usuários diz id e nome sem passar pela página que a moderação bloqueia.
/// `to_moderation` = o redirecionamento ia para `/not-approved`.
async fn validate_cookie_via_users_api(
    security_token: &str,
    to_moderation: bool,
) -> Result<AccountInfo, String> {
    #[derive(Deserialize)]
    struct AuthenticatedUser {
        id: i64,
        name: String,
        #[serde(rename = "displayName", default)]
        display_name: String,
    }

    let response = build_client()
        .get(format!("{}/v1/users/authenticated", endpoints::host("users")))
        .header(COOKIE, cookie_header(security_token))
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))?;
    let status = response.status();

    if status.is_success() {
        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;
        let user = serde_json::from_str::<AuthenticatedUser>(&body)
            .map_err(|e| format!("Failed to parse account info: {}", e))?;
        return Ok(AccountInfo {
            user_id: user.id,
            display_name: if user.display_name.is_empty() { user.name.clone() } else { user.display_name },
            name: user.name,
            user_email: None,
            is_email_verified: false,
            age_bracket: 0,
            user_above_13: false,
        });
    }
    if status.as_u16() == 401 || !to_moderation {
        return Err(format!("Invalid cookie (status {})", status.as_u16()));
    }
    Err("Roblox is restricting this account (moderated: banned, warned or under review).          Check it at roblox.com/not-approved, then add it again."
        .to_string())
}

pub async fn get_csrf_token(security_token: &str) -> Result<String, String> {
    let client = build_client();

    let response = client
        .post(format!("{}/v1/authentication-ticket/", endpoints::host("auth")))
        .header(COOKIE, cookie_header(security_token))
        .header(REFERER, referer_url())
        .header("RBXAuthenticationNegotiation", "1")
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))?;

    if let Some(token) = response
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
    {
        return Ok(token);
    }

    // Historically Roblox returns 403 and includes the x-csrf-token header.
    // If the status code or behavior changes, surface the response to help debug.
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(format!(
        "[{} {}] {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or(""),
        body
    ))
}

/// Sends a request that needs a CSRF token, retrying once when the service
/// answers `403` with a token of its own.
///
/// Roblox's XSRF tokens are **per service**: the token `get_csrf_token` reads
/// from `auth.roblox.com` is rejected by `apis.roblox.com` with
/// `{"code":0,"message":"XSRF token invalid"}`, and the 403 carries, in the
/// `x-csrf-token` header, the token that service does accept. Retrying with it
/// is what the web client does — without this, share links, blocking and the
/// other `apis.roblox.com` calls fail every time.
///
/// The builder passed in must **not** carry `X-CSRF-TOKEN` already; this adds
/// it (reqwest appends headers, so a second one would be sent as well).
pub async fn send_with_csrf_retry(
    builder: reqwest::RequestBuilder,
    csrf: &str,
) -> Result<reqwest::Response, String> {
    let retry = builder.try_clone();
    let response = builder
        .header("X-CSRF-TOKEN", csrf)
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))?;

    if response.status() != reqwest::StatusCode::FORBIDDEN {
        return Ok(response);
    }

    let fresh = response
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != csrf);

    match (fresh, retry) {
        (Some(fresh), Some(retry)) => retry
            .header("X-CSRF-TOKEN", fresh)
            .send_noting()
            .await
            .map_err(|e| http_client::describe_error(&e)),
        // No token to retry with (or a streaming body): keep the original 403
        // so the caller reports Roblox's own message.
        _ => Ok(response),
    }
}

pub async fn get_auth_ticket(security_token: &str) -> Result<String, String> {
    let csrf = get_csrf_token(security_token).await?;

    let client = build_client();

    let response = client
        .post(format!("{}/v1/authentication-ticket/", endpoints::host("auth")))
        .header(COOKIE, cookie_header(security_token))
        .header("x-csrf-token", &csrf)
        .header(REFERER, referer_url())
        .header("RBXAuthenticationNegotiation", "1")
        .header("Content-Type", "application/json")
        .body("")
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))?;

    if let Some(ticket) = response
        .headers()
        .get("rbx-authentication-ticket")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
    {
        return Ok(ticket);
    }

    if let Some(message) = challenge_message(response.headers()) {
        return Err(message);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(format!(
        "Failed to get authentication ticket (status {}): {}",
        status.as_u16(),
        body
    ))
}

/// Nome curto do tipo de verificação que o Roblox pediu (`rblx-challenge-type`).
/// Tipo desconhecido vira "a security check": a mensagem continua útil mesmo
/// quando o Roblox inventa um desafio novo.
fn challenge_kind_label(kind: &str) -> &'static str {
    match kind.trim().to_ascii_lowercase().as_str() {
        "twostepverification" | "forcetwostepverification" => "2-step verification",
        "captcha" => "a CAPTCHA",
        "reauthentication" => "your password again",
        _ => "a security check",
    }
}

/// Mensagem para quando o Roblox responde com um desafio (`rblx-challenge-id` /
/// `rblx-challenge-type`) em vez do ticket. O app **nunca** tenta resolver o
/// desafio: só diz onde terminá-lo. O texto evita de propósito as palavras que
/// `is_auth_session_error` (renovaria a sessão) e `is_moderated_error` (moveria
/// a conta para "moderadas") reconhecem.
fn challenge_message(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let id = read("rblx-challenge-id");
    let kind = read("rblx-challenge-type");
    if id.is_none() && kind.is_none() {
        return None;
    }
    Some(format!(
        "Roblox wants to verify this account ({}). Open it in the browser (account panel › Tools › Browser), finish the check there, then try again.",
        challenge_kind_label(kind.as_deref().unwrap_or(""))
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinInfo {
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "unlockedUntil", default)]
    pub unlocked_until: Option<serde_json::Value>,
}

pub async fn check_pin(security_token: &str) -> Result<bool, String> {
    let _csrf = get_csrf_token(security_token).await?;

    let client = build_client();

    let response = client
        .get(format!("{}/v1/account/pin/", endpoints::host("auth")))
        .header(COOKIE, cookie_header(security_token))
        .header(REFERER, format!("{}/", endpoints::host("www")))
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to check pin (status {})",
            response.status().as_u16()
        ));
    }

    let info: PinInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse pin info: {}", e))?;

    if !info.is_enabled {
        return Ok(true);
    }

    match &info.unlocked_until {
        Some(serde_json::Value::Number(n)) if n.as_i64().unwrap_or(0) > 0 => Ok(true),
        _ => Ok(false),
    }
}

pub async fn unlock_pin(security_token: &str, pin: &str) -> Result<bool, String> {
    if pin.len() != 4 {
        return Err("Pin must be 4 digits".to_string());
    }

    let csrf = get_csrf_token(security_token).await?;

    let client = build_client();

    let request = client
        .post(format!("{}/v1/account/pin/unlock", endpoints::host("auth")))
        .header(COOKIE, cookie_header(security_token))
        .header(REFERER, format!("{}/", endpoints::host("www")))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("pin={}", pin));
    let response = send_with_csrf_retry(request, &csrf).await?;

    if !response.status().is_success() {
        return Ok(false);
    }

    let info: PinInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse pin response: {}", e))?;

    Ok(info.is_enabled
        && matches!(&info.unlocked_until, Some(serde_json::Value::Number(n)) if n.as_i64().unwrap_or(0) > 0))
}

pub struct RefreshResult {
    pub success: bool,
    pub new_cookie: Option<String>,
}

pub async fn log_out_other_sessions(security_token: &str) -> Result<RefreshResult, String> {
    let csrf = get_csrf_token(security_token).await?;

    let client = build_client();

    let request = client
        .post(format!("{}/authentication/signoutfromallsessionsandreauthenticate", endpoints::host("www")))
        .header(COOKIE, cookie_header(security_token))
        .header(REFERER, format!("{}/", endpoints::host("www")))
        .header("Content-Type", "application/x-www-form-urlencoded");
    let response = send_with_csrf_retry(request, &csrf).await?;

    // Roblox may return redirects for this endpoint while still setting cookies.
    if !(response.status().is_success() || response.status().is_redirection()) {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Failed to sign out other sessions (status {}): {}",
            status.as_u16(),
            body
        ));
    }

    let new_cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find_map(|v| {
            let s = v.to_str().ok()?;
            if s.starts_with(".ROBLOSECURITY=") {
                let value = s.strip_prefix(".ROBLOSECURITY=")?;
                let value = value.split(';').next()?;
                Some(value.to_string())
            } else {
                None
            }
        });

    Ok(RefreshResult {
        success: true,
        new_cookie,
    })
}

pub async fn change_password(
    security_token: &str,
    current_password: &str,
    new_password: &str,
) -> Result<Option<String>, String> {
    let csrf = get_csrf_token(security_token).await?;

    let client = build_client();

    let request = client
        .post(format!("{}/v2/user/passwords/change", endpoints::host("auth")))
        .header(COOKIE, cookie_header(security_token))
        .header(REFERER, format!("{}/", endpoints::host("www")))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "currentPassword={}&newPassword={}",
            urlencoding::encode(current_password),
            urlencoding::encode(new_password)
        ));
    let response = send_with_csrf_retry(request, &csrf).await?;

    if !response.status().is_success() {
        return Err("Failed to change password".to_string());
    }

    let new_cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find_map(|v| {
            let s = v.to_str().ok()?;
            if s.starts_with(".ROBLOSECURITY=") {
                let value = s.strip_prefix(".ROBLOSECURITY=")?;
                let value = value.split(';').next()?;
                Some(value.to_string())
            } else {
                None
            }
        });

    Ok(new_cookie)
}

pub async fn change_email(
    security_token: &str,
    password: &str,
    new_email: &str,
) -> Result<(), String> {
    let csrf = get_csrf_token(security_token).await?;

    let client = build_client();

    let request = client
        .post(format!("{}/v1/email", endpoints::host("accountsettings")))
        .header(COOKIE, cookie_header(security_token))
        .header(REFERER, format!("{}/", endpoints::host("www")))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "password={}&emailAddress={}",
            urlencoding::encode(password),
            urlencoding::encode(new_email)
        ));
    let response = send_with_csrf_retry(request, &csrf).await?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err("Failed to change email".to_string())
    }
}

pub async fn quick_login_enter_code(
    security_token: &str,
    code: &str,
) -> Result<serde_json::Value, String> {
    let normalized_code = normalize_quick_login_code(code);
    if normalized_code.len() != 6 {
        return Err("Code must be 6 digits".to_string());
    }

    let csrf = get_csrf_token(security_token).await?;
    let client = build_client();

    let request = client
        .post(format!("{}/auth-token-service/v1/login/enterCode", endpoints::host("apis")))
        .header(COOKIE, cookie_header(security_token))
        .json(&serde_json::json!({ "code": normalized_code }));
    let response = send_with_csrf_retry(request, &csrf).await?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Failed to enter code: {}", body));
    }

    response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

pub async fn quick_login_validate_code(security_token: &str, code: &str) -> Result<(), String> {
    let normalized_code = normalize_quick_login_code(code);
    if normalized_code.len() != 6 {
        return Err("Code must be 6 digits".to_string());
    }

    let csrf = get_csrf_token(security_token).await?;
    let client = build_client();

    let request = client
        .post(format!("{}/auth-token-service/v1/login/validateCode", endpoints::host("apis")))
        .header(COOKIE, cookie_header(security_token))
        .json(&serde_json::json!({ "code": normalized_code }));
    let response = send_with_csrf_retry(request, &csrf).await?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err("Failed to validate code".to_string())
    }
}

// ── Quick Login do lado de quem **entra** (ideia 12) ────────────────────────
//
// O fluxo oficial do Roblox para entrar num aparelho novo sem digitar senha:
// 1. `auth-token-service/v1/login/create` devolve um código curto e uma chave
//    privada (a chave nunca sai do backend);
// 2. a pessoa digita o código num aparelho já logado (roblox.com/crossdevicelogin
//    ou Configurações › Quick Log In) e confirma;
// 3. `login/status` passa de `Created` para `UserLinked` e depois `Validated`;
// 4. `auth.roblox.com/v2/login` com `ctype: AuthToken` troca código + chave pela
//    sessão (`.ROBLOSECURITY` no `Set-Cookie`).
//
// Se o Roblox pedir verificação (captcha ou outro desafio) no passo 4, o app
// **para** com uma mensagem — não tenta resolver nem contornar o desafio.

/// Um Quick Login em andamento. A chave privada fica só aqui.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickLoginSession {
    pub code: String,
    pub private_key: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QuickLoginStatus {
    /// Esperando o código ser digitado no outro aparelho.
    Pending,
    /// Código digitado; esperando a confirmação lá.
    #[serde(rename_all = "camelCase")]
    Linked { account_name: Option<String> },
    /// Confirmado: já dá para trocar pela sessão.
    Validated,
    Cancelled,
    Expired,
}

/// Mensagem de quando o Roblox pede verificação para concluir o Quick Login.
pub const QUICK_LOGIN_CHALLENGE_MESSAGE: &str =
    "Roblox asked for an extra check (like a CAPTCHA) to finish this sign-in, and MultiAlt doesn't solve those. Add the account with Browser Login instead.";

/// POST sem conta que precisa de XSRF: a primeira resposta é 403 com o token
/// no `x-csrf-token`, e o pedido é repetido uma vez com ele.
async fn post_with_csrf_handshake(
    url: String,
    body: &serde_json::Value,
) -> Result<reqwest::Response, String> {
    let client = build_client();
    let first = client
        .post(&url)
        .json(body)
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))?;
    if first.status() != reqwest::StatusCode::FORBIDDEN {
        return Ok(first);
    }
    let Some(token) = first
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return Ok(first);
    };
    client
        .post(&url)
        .header("X-CSRF-TOKEN", token)
        .json(body)
        .send_noting()
        .await
        .map_err(|e| http_client::describe_error(&e))
}

pub async fn quick_login_create() -> Result<QuickLoginSession, String> {
    let response = post_with_csrf_handshake(
        format!("{}/auth-token-service/v1/login/create", endpoints::host("apis")),
        &serde_json::json!({}),
    )
    .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Roblox couldn't start Quick Login (status {}).",
            status.as_u16()
        ));
    }
    parse_quick_login_session(&body)
}

fn parse_quick_login_session(body: &str) -> Result<QuickLoginSession, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "Unexpected Quick Login answer from Roblox.".to_string())?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match (text("code"), text("privateKey")) {
        (Some(code), Some(private_key)) => Ok(QuickLoginSession {
            code,
            private_key,
            expires_at: text("expirationTime"),
        }),
        _ => Err("Unexpected Quick Login answer from Roblox.".to_string()),
    }
}

/// O que a resposta do `login/status` quer dizer. 400 é código inválido ou
/// vencido (o Roblox responde `CodeInvalid`); estado desconhecido conta como
/// "ainda esperando".
fn parse_quick_login_status(status: u16, body: &str) -> Result<QuickLoginStatus, String> {
    if status == 400 {
        return Ok(QuickLoginStatus::Expired);
    }
    if !(200..300).contains(&status) {
        return Err(format!("Roblox returned status {status} while checking Quick Login."));
    }
    let value: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let field = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
    Ok(match field("status").as_deref() {
        Some("Validated") => QuickLoginStatus::Validated,
        Some("UserLinked") => QuickLoginStatus::Linked {
            account_name: field("accountName").filter(|s| !s.trim().is_empty()),
        },
        Some("Cancelled") => QuickLoginStatus::Cancelled,
        Some("CodeInvalid") | Some("Expired") => QuickLoginStatus::Expired,
        _ => QuickLoginStatus::Pending,
    })
}

pub async fn quick_login_status(session: &QuickLoginSession) -> Result<QuickLoginStatus, String> {
    let response = post_with_csrf_handshake(
        format!("{}/auth-token-service/v1/login/status", endpoints::host("apis")),
        &serde_json::json!({ "code": session.code, "privateKey": session.private_key }),
    )
    .await?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    parse_quick_login_status(status, &body)
}

/// O `.ROBLOSECURITY` de um `Set-Cookie`, se houver.
fn roblosecurity_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|cookie| {
            cookie
                .strip_prefix(".ROBLOSECURITY=")
                .map(|rest| rest.split(';').next().unwrap_or_default().to_string())
        })
        .filter(|token| !token.is_empty())
}

/// Troca um Quick Login confirmado pela sessão da conta.
pub async fn quick_login_redeem(session: &QuickLoginSession) -> Result<String, String> {
    let response = post_with_csrf_handshake(
        format!("{}/v2/login", endpoints::host("auth")),
        &serde_json::json!({
            "ctype": "AuthToken",
            "cvalue": session.code,
            "password": session.private_key,
        }),
    )
    .await?;
    if let Some(token) = roblosecurity_from_headers(response.headers()) {
        return Ok(token);
    }
    if challenge_message(response.headers()).is_some() {
        return Err(QUICK_LOGIN_CHALLENGE_MESSAGE.to_string());
    }
    Err(format!(
        "Roblox didn't finish the sign-in (status {}). Try again with a new code, or use Browser Login.",
        response.status().as_u16()
    ))
}

#[cfg(test)]
mod quick_login_add_tests {
    use super::*;
    use crate::api::endpoints::test_support::{mock_path, mock_server};
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, Request, ResponseTemplate};

    #[test]
    fn a_created_session_needs_both_code_and_private_key() {
        let ok = parse_quick_login_session(
            r#"{"code":"ABC123","status":"Created","privateKey":"pk-1","expirationTime":"2026-10-11T10:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(ok.code, "ABC123");
        assert_eq!(ok.private_key, "pk-1");
        assert_eq!(ok.expires_at.as_deref(), Some("2026-10-11T10:00:00Z"));
        assert!(parse_quick_login_session(r#"{"code":"ABC123"}"#).is_err());
        assert!(parse_quick_login_session(r#"{"privateKey":"x"}"#).is_err());
        assert!(parse_quick_login_session("not json").is_err());
    }

    #[test]
    fn the_status_maps_every_roblox_state() {
        assert_eq!(parse_quick_login_status(200, r#"{"status":"Created"}"#), Ok(QuickLoginStatus::Pending));
        assert_eq!(
            parse_quick_login_status(200, r#"{"status":"UserLinked","accountName":"Someone"}"#),
            Ok(QuickLoginStatus::Linked { account_name: Some("Someone".into()) })
        );
        assert_eq!(
            parse_quick_login_status(200, r#"{"status":"UserLinked","accountName":""}"#),
            Ok(QuickLoginStatus::Linked { account_name: None })
        );
        assert_eq!(parse_quick_login_status(200, r#"{"status":"Validated"}"#), Ok(QuickLoginStatus::Validated));
        assert_eq!(parse_quick_login_status(200, r#"{"status":"Cancelled"}"#), Ok(QuickLoginStatus::Cancelled));
        assert_eq!(parse_quick_login_status(400, r#""CodeInvalid""#), Ok(QuickLoginStatus::Expired));
        assert_eq!(parse_quick_login_status(200, "{}"), Ok(QuickLoginStatus::Pending));
        assert!(parse_quick_login_status(500, "").is_err());
    }

    #[test]
    fn the_status_serializes_with_a_kind_tag_for_the_ui() {
        let json = serde_json::to_value(QuickLoginStatus::Linked { account_name: Some("A".into()) }).unwrap();
        assert_eq!(json["kind"], "linked");
        assert_eq!(json["accountName"], "A");
        assert_eq!(serde_json::to_value(QuickLoginStatus::Expired).unwrap()["kind"], "expired");
    }

    #[tokio::test]
    async fn create_does_the_csrf_handshake_and_returns_the_code() {
        let server = mock_server().await;
        Mock::given(method("POST"))
            .and(path(mock_path("apis", "/auth-token-service/v1/login/create")))
            .and(|req: &Request| !req.headers.contains_key("x-csrf-token"))
            .respond_with(ResponseTemplate::new(403).insert_header("x-csrf-token", "ql-create-csrf"))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path(mock_path("apis", "/auth-token-service/v1/login/create")))
            .and(header("x-csrf-token", "ql-create-csrf"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":"QWE789","status":"Created","privateKey":"pk-create"}"#,
            ))
            .mount(server)
            .await;

        let session = quick_login_create().await.expect("session");
        assert_eq!(session.code, "QWE789");
        assert_eq!(session.private_key, "pk-create");
    }

    #[tokio::test]
    async fn status_sends_code_and_private_key() {
        let server = mock_server().await;
        Mock::given(method("POST"))
            .and(path(mock_path("apis", "/auth-token-service/v1/login/status")))
            .and(body_partial_json(serde_json::json!({ "code": "ST0001", "privateKey": "pk-status" })))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status":"Validated"}"#))
            .mount(server)
            .await;
        let session = QuickLoginSession {
            code: "ST0001".into(),
            private_key: "pk-status".into(),
            expires_at: None,
        };
        assert_eq!(quick_login_status(&session).await, Ok(QuickLoginStatus::Validated));
    }

    #[tokio::test]
    async fn redeem_returns_the_session_cookie() {
        let server = mock_server().await;
        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v2/login")))
            .and(body_partial_json(serde_json::json!({
                "ctype": "AuthToken",
                "cvalue": "RD0001",
                "password": "pk-redeem"
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", ".ROBLOSECURITY=_|WARNING|_ql-cookie; domain=.roblox.com; path=/"),
            )
            .mount(server)
            .await;
        let session = QuickLoginSession {
            code: "RD0001".into(),
            private_key: "pk-redeem".into(),
            expires_at: None,
        };
        assert_eq!(quick_login_redeem(&session).await, Ok("_|WARNING|_ql-cookie".into()));
    }

    #[tokio::test]
    async fn a_challenge_stops_with_a_clear_message_instead_of_being_solved() {
        let server = mock_server().await;
        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v2/login")))
            .and(body_partial_json(serde_json::json!({ "cvalue": "CH0001" })))
            .respond_with(
                ResponseTemplate::new(403)
                    .insert_header("rblx-challenge-id", "abc")
                    .insert_header("rblx-challenge-type", "captcha"),
            )
            .mount(server)
            .await;
        let session = QuickLoginSession {
            code: "CH0001".into(),
            private_key: "pk-challenge".into(),
            expires_at: None,
        };
        assert_eq!(
            quick_login_redeem(&session).await,
            Err(QUICK_LOGIN_CHALLENGE_MESSAGE.to_string())
        );
    }

    #[test]
    fn quick_login_urls_come_from_endpoints() {
        // Literal `https://*.roblox.com` aqui quebraria os testes mockados.
        let source = include_str!("auth.rs");
        let section = source
            .split("// ── Quick Login do lado de quem **entra**")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("section");
        assert!(!section.contains("https://apis.roblox.com"));
        assert!(!section.contains("https://auth.roblox.com"));
    }
}

pub async fn set_display_name(
    security_token: &str,
    user_id: i64,
    display_name: &str,
) -> Result<(), String> {
    let csrf = get_csrf_token(security_token).await?;

    let client = build_client();

    let request = client
        .patch(&format!(
            "{}/v1/users/{}/display-names",
            endpoints::host("users"),
            user_id
        ))
        .header(COOKIE, cookie_header(security_token))
        .json(&serde_json::json!({ "newDisplayName": display_name }));
    let response = send_with_csrf_retry(request, &csrf).await?;

    if response.status().is_success() {
        Ok(())
    } else {
        let body = response.text().await.unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(msg) = json["errors"][0]["message"].as_str() {
                return Err(msg.to_string());
            }
        }
        Err(format!("Failed to set display name: {}", body))
    }
}

#[cfg(test)]
mod auth_http_tests {
    use super::*;
    use crate::api::endpoints::test_support::{cookie_of, mock_path, mock_server, mount_csrf};
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, ResponseTemplate};

    #[tokio::test]
    async fn validate_cookie_parses_account_info() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("valid-cookie")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "UserId": 1234,
                "Name": "alt_one",
                "DisplayName": "Alt One",
                "UserEmail": "a***@example.com",
                "IsEmailVerified": true,
                "AgeBracket": 0,
                "UserAbove13": true
            })))
            .mount(server)
            .await;

        let info = validate_cookie("valid-cookie").await.expect("account info");
        assert_eq!(info.user_id, 1234);
        assert_eq!(info.name, "alt_one");
        assert_eq!(info.display_name, "Alt One");
        assert!(info.is_email_verified);
        assert!(info.user_above_13);
    }

    #[tokio::test]
    async fn validate_cookie_rejects_unauthorized() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("expired-cookie")))
            .respond_with(ResponseTemplate::new(401))
            .mount(server)
            .await;

        let err = validate_cookie("expired-cookie").await.unwrap_err();
        assert_eq!(err, "Invalid cookie (status 401)");
    }

    /// Conta moderada: a página `/my/account/json` redireciona (302) para
    /// `/not-approved`. O cookie é bom — antes isso virava "Invalid cookie
    /// (status 302)" e a conta não entrava (teste do dono, 10/10/2026).
    #[tokio::test]
    async fn validate_cookie_accepts_a_moderated_account_through_the_users_api() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("moderated-cookie")))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "https://www.roblox.com/not-approved"))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(mock_path("users", "/v1/users/authenticated")))
            .and(header("cookie", cookie_of("moderated-cookie")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 5678, "name": "banned_alt", "displayName": "Banned Alt"
            })))
            .mount(server)
            .await;

        let info = validate_cookie("moderated-cookie").await.expect("account info");
        assert_eq!(info.user_id, 5678);
        assert_eq!(info.name, "banned_alt");
        assert_eq!(info.display_name, "Banned Alt");
    }

    #[tokio::test]
    async fn validate_cookie_explains_a_moderated_account_the_users_api_refuses() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("locked-cookie")))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "https://www.roblox.com/not-approved"))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(mock_path("users", "/v1/users/authenticated")))
            .and(header("cookie", cookie_of("locked-cookie")))
            .respond_with(ResponseTemplate::new(403))
            .mount(server)
            .await;

        let err = validate_cookie("locked-cookie").await.unwrap_err();
        assert!(err.contains("moderat"), "{err}");
        assert!(!err.contains("Invalid cookie"), "{err}");
    }

    #[tokio::test]
    async fn validate_cookie_still_rejects_a_dead_cookie_after_a_redirect() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("dead-redirect")))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "https://www.roblox.com/login"))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(mock_path("users", "/v1/users/authenticated")))
            .and(header("cookie", cookie_of("dead-redirect")))
            .respond_with(ResponseTemplate::new(401))
            .mount(server)
            .await;

        let err = validate_cookie("dead-redirect").await.unwrap_err();
        assert_eq!(err, "Invalid cookie (status 401)");
    }

    #[tokio::test]
    async fn validate_cookie_reports_non_json_body() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("html-cookie")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("<!DOCTYPE html><html>login</html>", "text/html"),
            )
            .mount(server)
            .await;

        let err = validate_cookie("html-cookie").await.unwrap_err();
        assert!(
            err.starts_with("Failed to parse account info: "),
            "unexpected error: {}",
            err
        );
        // The body is echoed (truncated) so the user can tell a login page from
        // a real API error.
        assert!(err.contains("<!DOCTYPE html>"), "unexpected error: {}", err);
    }

    #[tokio::test]
    async fn get_csrf_token_reads_the_header_off_a_403() {
        mount_csrf("csrf-account", "csrf-token-abc").await;

        let token = get_csrf_token("csrf-account").await.expect("csrf token");
        assert_eq!(token, "csrf-token-abc");
    }

    #[tokio::test]
    async fn get_csrf_token_errors_when_header_is_missing() {
        let server = mock_server().await;
        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v1/authentication-ticket/")))
            .and(header("cookie", cookie_of("no-csrf-account")))
            .respond_with(ResponseTemplate::new(403).set_body_string("Token Validation Failed"))
            .mount(server)
            .await;

        let err = get_csrf_token("no-csrf-account").await.unwrap_err();
        assert!(err.starts_with("[403 Forbidden]"), "unexpected error: {}", err);
        assert!(err.contains("Token Validation Failed"), "unexpected error: {}", err);
    }

    #[tokio::test]
    async fn get_auth_ticket_returns_the_ticket_header() {
        let server = mock_server().await;
        mount_csrf("ticket-account", "csrf-for-ticket").await;

        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v1/authentication-ticket/")))
            .and(header("cookie", cookie_of("ticket-account")))
            .and(header_exists("x-csrf-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("rbx-authentication-ticket", "ticket-xyz"),
            )
            .mount(server)
            .await;

        let ticket = get_auth_ticket("ticket-account").await.expect("ticket");
        assert_eq!(ticket, "ticket-xyz");
    }

    #[tokio::test]
    async fn get_auth_ticket_surfaces_a_moderated_account() {
        let server = mock_server().await;
        mount_csrf("moderated-account", "csrf-for-moderated").await;

        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v1/authentication-ticket/")))
            .and(header("cookie", cookie_of("moderated-account")))
            .and(header_exists("x-csrf-token"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "errors": [{ "code": 0, "message": "User is moderated" }]
            })))
            .mount(server)
            .await;

        let err = get_auth_ticket("moderated-account").await.unwrap_err();
        assert!(
            err.starts_with("Failed to get authentication ticket (status 403): "),
            "unexpected error: {}",
            err
        );
        // The launch flow buckets the account into the "moderadas" group based
        // on this exact string; keep the two in sync.
        assert!(
            crate::is_moderated_error(&err),
            "launch_shared::is_moderated_error should classify: {}",
            err
        );
    }
}

/// Everything in `auth.rs` that `auth_http_tests` does not already reach: the
/// pure code-normalisation guards and the pin / session / account-edit calls.
///
/// Every test uses its own `.ROBLOSECURITY` token so its mocks can never match
/// another test's request on the shared mock server.
/// The per-service XSRF retry behind `send_with_csrf_retry`.
///
/// Roblox issues XSRF tokens per service: the one `get_csrf_token` reads from
/// auth.roblox.com is refused by apis.roblox.com with
/// `{"code":0,"message":"XSRF token invalid"}`, and that 403 carries the token
/// the service does accept. Losing this retry breaks every share link.
#[cfg(test)]
mod csrf_retry_tests {
    use super::*;
    use crate::api::endpoints::test_support::{mock_path, mock_server};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, Request, ResponseTemplate};

    /// Counts how many times the mock server was hit on one path.
    async fn hits(route: &str) -> usize {
        mock_server()
            .await
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == route)
            .count()
    }

    /// Answers 200 for `good`, and 403 + `x-csrf-token: good` for anything
    /// else — the two mocks are mutually exclusive, so match order is
    /// irrelevant.
    async fn mount_service_token(route: &str, good: &'static str) {
        let server = mock_server().await;

        Mock::given(method("POST"))
            .and(path(route.to_string()))
            .and(header("x-csrf-token", good))
            .respond_with(ResponseTemplate::new(200).set_body_string("accepted"))
            .mount(server)
            .await;

        Mock::given(method("POST"))
            .and(path(route.to_string()))
            .and(move |req: &Request| {
                req.headers
                    .get("x-csrf-token")
                    .map(|v| v.as_bytes() != good.as_bytes())
                    .unwrap_or(true)
            })
            .respond_with(
                ResponseTemplate::new(403)
                    .insert_header("x-csrf-token", good)
                    .set_body_string(r#"{"errors":[{"code":0,"message":"XSRF token invalid"}]}"#),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_refused_token_is_replaced_by_the_one_the_service_hands_back() {
        let route = mock_path("apis", "/csrf-retry/replaced");
        mount_service_token(&route, "token-from-apis").await;

        let request = build_client().post(format!(
            "{}/csrf-retry/replaced",
            endpoints::host("apis")
        ));
        let response = send_with_csrf_retry(request, "token-from-auth")
            .await
            .expect("response");

        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().await.unwrap(), "accepted");
        assert_eq!(hits(&route).await, 2, "should have retried exactly once");
    }

    /// The body has to survive the retry — a cloned builder that dropped it
    /// would resolve the wrong link.
    #[tokio::test]
    async fn the_body_is_resent_on_the_retry() {
        let route = mock_path("apis", "/csrf-retry/body");
        mount_service_token(&route, "token-body").await;

        let request = build_client()
            .post(format!("{}/csrf-retry/body", endpoints::host("apis")))
            .json(&serde_json::json!({ "linkId": "abc", "linkType": "ExperienceInvite" }));
        let response = send_with_csrf_retry(request, "stale").await.expect("response");
        assert_eq!(response.status().as_u16(), 200);

        let sent = mock_server()
            .await
            .received_requests()
            .await
            .unwrap_or_default();
        let bodies: Vec<String> = sent
            .iter()
            .filter(|r| r.url.path() == route)
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
            .collect();
        assert_eq!(bodies.len(), 2);
        assert!(
            bodies.iter().all(|b| b.contains("\"linkId\":\"abc\"")),
            "body lost on retry: {:?}",
            bodies
        );
    }

    /// A token the service already accepts must not cost a second request.
    #[tokio::test]
    async fn an_accepted_token_is_sent_once() {
        let route = mock_path("apis", "/csrf-retry/accepted");
        mount_service_token(&route, "token-accepted").await;

        let request = build_client().post(format!(
            "{}/csrf-retry/accepted",
            endpoints::host("apis")
        ));
        let response = send_with_csrf_retry(request, "token-accepted")
            .await
            .expect("response");

        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(hits(&route).await, 1);
    }

    /// A 403 that carries no token of its own is handed back untouched, so the
    /// caller reports Roblox's own message instead of retrying forever.
    #[tokio::test]
    async fn a_403_without_a_token_is_returned_as_is() {
        let route = mock_path("apis", "/csrf-retry/denied");
        Mock::given(method("POST"))
            .and(path(route.clone()))
            .respond_with(ResponseTemplate::new(403).set_body_string("Challenge is required"))
            .mount(mock_server().await)
            .await;

        let request = build_client().post(format!(
            "{}/csrf-retry/denied",
            endpoints::host("apis")
        ));
        let response = send_with_csrf_retry(request, "whatever")
            .await
            .expect("response");

        assert_eq!(response.status().as_u16(), 403);
        assert_eq!(response.text().await.unwrap(), "Challenge is required");
        assert_eq!(hits(&route).await, 1);
    }

    /// Repeating the same token would just be refused again.
    #[tokio::test]
    async fn the_same_token_is_not_retried() {
        let route = mock_path("apis", "/csrf-retry/same-token");
        Mock::given(method("POST"))
            .and(path(route.clone()))
            .respond_with(
                ResponseTemplate::new(403)
                    .insert_header("x-csrf-token", "loop-token")
                    .set_body_string("XSRF token invalid"),
            )
            .mount(mock_server().await)
            .await;

        let request = build_client().post(format!(
            "{}/csrf-retry/same-token",
            endpoints::host("apis")
        ));
        let response = send_with_csrf_retry(request, "loop-token")
            .await
            .expect("response");

        assert_eq!(response.status().as_u16(), 403);
        assert_eq!(hits(&route).await, 1);
    }
}

/// Roblox asking for a verification (2-step, CAPTCHA, password again…) on the
/// auth-ticket POST. It answers 403 with `rblx-challenge-*` headers; the raw
/// 403 used to reach the user as "Failed to get authentication ticket (status
/// 403)", which says nothing about what to do. The app never tries to solve the
/// challenge — it only says where to finish it.
#[cfg(test)]
mod auth_challenge_tests {
    use super::*;
    use crate::api::endpoints::test_support::{cookie_of, mock_path, mock_server, mount_csrf};
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, ResponseTemplate};

    async fn mount_challenge(token: &str, challenge_type: Option<&str>) {
        mount_csrf(token, &format!("csrf-{token}")).await;
        let mut response = ResponseTemplate::new(403)
            .insert_header("rblx-challenge-id", "11111111-2222-3333-4444-555555555555")
            .insert_header("rblx-challenge-metadata", "eyJ1c2VySWQiOiIxIn0=")
            .set_body_json(serde_json::json!({
                "errors": [{ "code": 0, "message": "Challenge is required to authorize the request" }]
            }));
        if let Some(kind) = challenge_type {
            response = response.insert_header("rblx-challenge-type", kind);
        }
        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v1/authentication-ticket/")))
            .and(header("cookie", cookie_of(token)))
            .and(header_exists("x-csrf-token"))
            .respond_with(response)
            .mount(mock_server().await)
            .await;
    }

    #[tokio::test]
    async fn a_two_step_challenge_becomes_a_clear_message() {
        mount_challenge("challenge-2sv", Some("twostepverification")).await;
        let err = get_auth_ticket("challenge-2sv").await.unwrap_err();
        assert!(err.starts_with("Roblox wants to verify this account"), "{err}");
        assert!(err.contains("2-step verification"), "{err}");
        assert!(err.contains("Browser"), "must say where to finish it: {err}");
        assert!(!err.contains("status 403"), "raw status leaked: {err}");
    }

    #[tokio::test]
    async fn a_captcha_challenge_names_the_captcha() {
        mount_challenge("challenge-captcha", Some("captcha")).await;
        let err = get_auth_ticket("challenge-captcha").await.unwrap_err();
        assert!(err.contains("CAPTCHA"), "{err}");
    }

    #[tokio::test]
    async fn an_unknown_challenge_type_still_gets_the_message() {
        mount_challenge("challenge-unknown", Some("somethingnew")).await;
        let err = get_auth_ticket("challenge-unknown").await.unwrap_err();
        assert!(err.starts_with("Roblox wants to verify this account"), "{err}");
        assert!(err.contains("a security check"), "{err}");
    }

    #[tokio::test]
    async fn a_challenge_id_without_a_type_is_still_a_challenge() {
        mount_challenge("challenge-no-type", None).await;
        let err = get_auth_ticket("challenge-no-type").await.unwrap_err();
        assert!(err.starts_with("Roblox wants to verify this account"), "{err}");
    }

    /// The message must not trip the classifiers that act on launch errors:
    /// a session error would trigger the sign-out refresh, and a moderated one
    /// would move the account into "moderadas".
    #[tokio::test]
    async fn the_message_is_neither_a_session_nor_a_moderation_error() {
        mount_challenge("challenge-classify", Some("reauthentication")).await;
        let err = get_auth_ticket("challenge-classify").await.unwrap_err();
        assert!(!crate::is_moderated_error(&err), "{err}");
        assert!(!crate::is_auth_session_error(&err), "{err}");
    }

    #[test]
    fn challenge_kinds_map_to_short_names() {
        assert_eq!(challenge_kind_label("twostepverification"), "2-step verification");
        assert_eq!(challenge_kind_label("TwoStepVerification"), "2-step verification");
        assert_eq!(challenge_kind_label("captcha"), "a CAPTCHA");
        assert_eq!(challenge_kind_label("reauthentication"), "your password again");
        assert_eq!(challenge_kind_label("proofofwork"), "a security check");
        assert_eq!(challenge_kind_label(""), "a security check");
    }
}

#[cfg(test)]
mod auth_extra_tests {
    use super::*;
    use crate::api::endpoints::test_support::{cookie_of, mock_path, mock_server, mount_csrf};
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, ResponseTemplate};

    #[test]
    fn quick_login_codes_keep_only_digits() {
        assert_eq!(normalize_quick_login_code("123 456"), "123456");
        assert_eq!(normalize_quick_login_code("12-34-56"), "123456");
        assert_eq!(normalize_quick_login_code("AB12CD34EF56"), "123456");
        assert_eq!(normalize_quick_login_code(""), "");
    }

    #[test]
    fn the_referer_points_at_a_real_game_page() {
        assert!(referer_url().ends_with("/games/2753915549/Blox-Fruits"));
    }

    #[test]
    fn the_cookie_header_carries_the_security_token() {
        assert_eq!(cookie_header("abc"), ".ROBLOSECURITY=abc");
    }

    /// Both quick-login calls reject a malformed code before touching the
    /// network — no mock is mounted, so a request would 404 and change the
    /// error message.
    #[tokio::test]
    async fn quick_login_rejects_codes_that_are_not_six_digits() {
        assert_eq!(
            quick_login_enter_code("unused-token", "12345")
                .await
                .unwrap_err(),
            "Code must be 6 digits"
        );
        assert_eq!(
            quick_login_validate_code("unused-token", "1234567")
                .await
                .unwrap_err(),
            "Code must be 6 digits"
        );
        assert_eq!(
            quick_login_enter_code("unused-token", "abcdef")
                .await
                .unwrap_err(),
            "Code must be 6 digits"
        );
    }

    #[tokio::test]
    async fn quick_login_enter_code_posts_the_normalised_code() {
        let server = mock_server().await;
        mount_csrf("ql-enter", "csrf-ql-enter").await;

        Mock::given(method("POST"))
            .and(path(mock_path(
                "apis",
                "/auth-token-service/v1/login/enterCode",
            )))
            .and(header("cookie", cookie_of("ql-enter")))
            .and(body_string_contains("123456"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": "Validated" })),
            )
            .mount(server)
            .await;

        let body = quick_login_enter_code("ql-enter", "12-34-56")
            .await
            .expect("enter code");
        assert_eq!(body["status"], serde_json::json!("Validated"));
    }

    #[tokio::test]
    async fn quick_login_enter_code_surfaces_the_error_body() {
        let server = mock_server().await;
        mount_csrf("ql-enter-fail", "csrf-ql-enter-fail").await;

        Mock::given(method("POST"))
            .and(path(mock_path(
                "apis",
                "/auth-token-service/v1/login/enterCode",
            )))
            .and(header("cookie", cookie_of("ql-enter-fail")))
            .respond_with(ResponseTemplate::new(400).set_body_string("code expired"))
            .mount(server)
            .await;

        let err = quick_login_enter_code("ql-enter-fail", "654321")
            .await
            .unwrap_err();
        assert_eq!(err, "Failed to enter code: code expired");
    }

    #[tokio::test]
    async fn quick_login_validate_code_maps_the_status() {
        let server = mock_server().await;
        mount_csrf("ql-validate", "csrf-ql-validate").await;

        Mock::given(method("POST"))
            .and(path(mock_path(
                "apis",
                "/auth-token-service/v1/login/validateCode",
            )))
            .and(header("cookie", cookie_of("ql-validate")))
            .respond_with(ResponseTemplate::new(200))
            .mount(server)
            .await;

        assert!(quick_login_validate_code("ql-validate", "111111")
            .await
            .is_ok());

        mount_csrf("ql-validate-bad", "csrf-ql-validate-bad").await;
        Mock::given(method("POST"))
            .and(path(mock_path(
                "apis",
                "/auth-token-service/v1/login/validateCode",
            )))
            .and(header("cookie", cookie_of("ql-validate-bad")))
            .respond_with(ResponseTemplate::new(403))
            .mount(server)
            .await;

        assert_eq!(
            quick_login_validate_code("ql-validate-bad", "222222")
                .await
                .unwrap_err(),
            "Failed to validate code"
        );
    }

    /// A pin that is not enabled means the account is unlocked.
    #[tokio::test]
    async fn check_pin_is_true_when_the_pin_is_disabled() {
        let server = mock_server().await;
        mount_csrf("pin-off", "csrf-pin-off").await;

        Mock::given(method("GET"))
            .and(path(mock_path("auth", "/v1/account/pin/")))
            .and(header("cookie", cookie_of("pin-off")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "isEnabled": false,
                "unlockedUntil": serde_json::Value::Null
            })))
            .mount(server)
            .await;

        assert!(check_pin("pin-off").await.expect("pin"));
    }

    #[tokio::test]
    async fn check_pin_is_true_while_the_pin_is_temporarily_unlocked() {
        let server = mock_server().await;
        mount_csrf("pin-unlocked", "csrf-pin-unlocked").await;

        Mock::given(method("GET"))
            .and(path(mock_path("auth", "/v1/account/pin/")))
            .and(header("cookie", cookie_of("pin-unlocked")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "isEnabled": true,
                "unlockedUntil": 1_700_000_000
            })))
            .mount(server)
            .await;

        assert!(check_pin("pin-unlocked").await.expect("pin"));
    }

    #[tokio::test]
    async fn check_pin_is_false_for_an_enabled_locked_pin() {
        let server = mock_server().await;
        mount_csrf("pin-locked", "csrf-pin-locked").await;

        Mock::given(method("GET"))
            .and(path(mock_path("auth", "/v1/account/pin/")))
            .and(header("cookie", cookie_of("pin-locked")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "isEnabled": true,
                "unlockedUntil": serde_json::Value::Null
            })))
            .mount(server)
            .await;

        assert!(!check_pin("pin-locked").await.expect("pin"));
    }

    #[tokio::test]
    async fn check_pin_surfaces_the_status_on_failure() {
        let server = mock_server().await;
        mount_csrf("pin-error", "csrf-pin-error").await;

        Mock::given(method("GET"))
            .and(path(mock_path("auth", "/v1/account/pin/")))
            .and(header("cookie", cookie_of("pin-error")))
            .respond_with(ResponseTemplate::new(503))
            .mount(server)
            .await;

        assert_eq!(
            check_pin("pin-error").await.unwrap_err(),
            "Failed to check pin (status 503)"
        );
    }

    /// The length guard runs before the CSRF handshake.
    #[tokio::test]
    async fn unlock_pin_rejects_a_pin_that_is_not_four_digits() {
        assert_eq!(
            unlock_pin("unused-token", "123").await.unwrap_err(),
            "Pin must be 4 digits"
        );
    }

    #[tokio::test]
    async fn unlock_pin_reports_success_only_when_the_pin_is_unlocked() {
        let server = mock_server().await;
        mount_csrf("pin-unlock-ok", "csrf-pin-unlock-ok").await;

        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v1/account/pin/unlock")))
            .and(header("cookie", cookie_of("pin-unlock-ok")))
            .and(body_string_contains("pin=1234"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "isEnabled": true,
                "unlockedUntil": 1_700_000_000
            })))
            .mount(server)
            .await;

        assert!(unlock_pin("pin-unlock-ok", "1234").await.expect("unlock"));
    }

    #[tokio::test]
    async fn unlock_pin_is_false_on_a_rejected_pin() {
        let server = mock_server().await;
        mount_csrf("pin-unlock-bad", "csrf-pin-unlock-bad").await;

        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v1/account/pin/unlock")))
            .and(header("cookie", cookie_of("pin-unlock-bad")))
            .respond_with(ResponseTemplate::new(403).set_body_string("wrong pin"))
            .mount(server)
            .await;

        assert!(!unlock_pin("pin-unlock-bad", "0000").await.expect("unlock"));
    }

    /// The reauthenticate call hands back a brand new `.ROBLOSECURITY`; losing
    /// it would log the account out of the manager.
    #[tokio::test]
    async fn log_out_other_sessions_picks_up_the_refreshed_cookie() {
        let server = mock_server().await;
        mount_csrf("signout-ok", "csrf-signout-ok").await;

        Mock::given(method("POST"))
            .and(path(mock_path(
                "www",
                "/authentication/signoutfromallsessionsandreauthenticate",
            )))
            .and(header("cookie", cookie_of("signout-ok")))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("set-cookie", "RBXEventTrackerV2=abc; path=/")
                    .append_header(
                        "set-cookie",
                        ".ROBLOSECURITY=NEW-TOKEN; domain=.roblox.com; path=/",
                    ),
            )
            .mount(server)
            .await;

        let result = log_out_other_sessions("signout-ok").await.expect("refresh");
        assert!(result.success);
        assert_eq!(result.new_cookie.as_deref(), Some("NEW-TOKEN"));
    }

    #[tokio::test]
    async fn log_out_other_sessions_without_a_new_cookie_still_succeeds() {
        let server = mock_server().await;
        mount_csrf("signout-nocookie", "csrf-signout-nocookie").await;

        Mock::given(method("POST"))
            .and(path(mock_path(
                "www",
                "/authentication/signoutfromallsessionsandreauthenticate",
            )))
            .and(header("cookie", cookie_of("signout-nocookie")))
            .respond_with(ResponseTemplate::new(200))
            .mount(server)
            .await;

        let result = log_out_other_sessions("signout-nocookie")
            .await
            .expect("refresh");
        assert!(result.success);
        assert!(result.new_cookie.is_none());
    }

    #[tokio::test]
    async fn log_out_other_sessions_reports_a_failure_status() {
        let server = mock_server().await;
        mount_csrf("signout-fail", "csrf-signout-fail").await;

        Mock::given(method("POST"))
            .and(path(mock_path(
                "www",
                "/authentication/signoutfromallsessionsandreauthenticate",
            )))
            .and(header("cookie", cookie_of("signout-fail")))
            .respond_with(ResponseTemplate::new(400).set_body_string("nope"))
            .mount(server)
            .await;

        // `RefreshResult` is not `Debug`, so unwrap the error by hand.
        let err = match log_out_other_sessions("signout-fail").await {
            Ok(_) => panic!("a 400 must not be treated as a successful refresh"),
            Err(e) => e,
        };
        assert_eq!(err, "Failed to sign out other sessions (status 400): nope");
    }

    #[tokio::test]
    async fn change_password_returns_the_rotated_cookie() {
        let server = mock_server().await;
        mount_csrf("pw-ok", "csrf-pw-ok").await;

        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v2/user/passwords/change")))
            .and(header("cookie", cookie_of("pw-ok")))
            // Both values are form-encoded, so a `&` in a password cannot split
            // the body into extra fields.
            .and(body_string_contains("currentPassword=old%26pass"))
            .and(body_string_contains("newPassword=new%20pass"))
            .respond_with(ResponseTemplate::new(200).insert_header(
                "set-cookie",
                ".ROBLOSECURITY=ROTATED; domain=.roblox.com; path=/",
            ))
            .mount(server)
            .await;

        let cookie = change_password("pw-ok", "old&pass", "new pass")
            .await
            .expect("change password");
        assert_eq!(cookie.as_deref(), Some("ROTATED"));
    }

    #[tokio::test]
    async fn change_password_fails_on_a_rejected_request() {
        let server = mock_server().await;
        mount_csrf("pw-fail", "csrf-pw-fail").await;

        Mock::given(method("POST"))
            .and(path(mock_path("auth", "/v2/user/passwords/change")))
            .and(header("cookie", cookie_of("pw-fail")))
            .respond_with(ResponseTemplate::new(400))
            .mount(server)
            .await;

        assert_eq!(
            change_password("pw-fail", "old", "new").await.unwrap_err(),
            "Failed to change password"
        );
    }

    #[tokio::test]
    async fn change_email_maps_the_status() {
        let server = mock_server().await;
        mount_csrf("email-ok", "csrf-email-ok").await;

        Mock::given(method("POST"))
            .and(path(mock_path("accountsettings", "/v1/email")))
            .and(header("cookie", cookie_of("email-ok")))
            .and(body_string_contains("emailAddress=new%40example.com"))
            .respond_with(ResponseTemplate::new(200))
            .mount(server)
            .await;

        assert!(change_email("email-ok", "pw", "new@example.com")
            .await
            .is_ok());

        mount_csrf("email-fail", "csrf-email-fail").await;
        Mock::given(method("POST"))
            .and(path(mock_path("accountsettings", "/v1/email")))
            .and(header("cookie", cookie_of("email-fail")))
            .respond_with(ResponseTemplate::new(400))
            .mount(server)
            .await;

        assert_eq!(
            change_email("email-fail", "pw", "x@example.com")
                .await
                .unwrap_err(),
            "Failed to change email"
        );
    }

    #[tokio::test]
    async fn set_display_name_succeeds_on_200() {
        let server = mock_server().await;
        mount_csrf("dn-ok", "csrf-dn-ok").await;

        Mock::given(method("PATCH"))
            .and(path(mock_path("users", "/v1/users/321/display-names")))
            .and(header("cookie", cookie_of("dn-ok")))
            .and(body_string_contains("Nova"))
            .respond_with(ResponseTemplate::new(200))
            .mount(server)
            .await;

        assert!(set_display_name("dn-ok", 321, "Nova").await.is_ok());
    }

    /// Roblox explains *why* a display name was rejected inside `errors[0]`;
    /// that message is what the UI shows.
    #[tokio::test]
    async fn set_display_name_unwraps_the_roblox_error_message() {
        let server = mock_server().await;
        mount_csrf("dn-err", "csrf-dn-err").await;

        Mock::given(method("PATCH"))
            .and(path(mock_path("users", "/v1/users/322/display-names")))
            .and(header("cookie", cookie_of("dn-err")))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "errors": [{ "code": 6, "message": "Display name is not appropriate" }]
            })))
            .mount(server)
            .await;

        assert_eq!(
            set_display_name("dn-err", 322, "bad").await.unwrap_err(),
            "Display name is not appropriate"
        );
    }

    #[tokio::test]
    async fn set_display_name_falls_back_to_the_raw_body() {
        let server = mock_server().await;
        mount_csrf("dn-raw", "csrf-dn-raw").await;

        Mock::given(method("PATCH"))
            .and(path(mock_path("users", "/v1/users/323/display-names")))
            .and(header("cookie", cookie_of("dn-raw")))
            .respond_with(ResponseTemplate::new(503).set_body_string("upstream down"))
            .mount(server)
            .await;

        assert_eq!(
            set_display_name("dn-raw", 323, "x").await.unwrap_err(),
            "Failed to set display name: upstream down"
        );
    }

    /// `/my/account/json` omits the e-mail fields for some accounts; the
    /// defaults must keep the account usable instead of failing to parse.
    #[tokio::test]
    async fn validate_cookie_defaults_the_optional_fields() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("minimal-cookie")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "UserId": 9,
                "Name": "nine",
                "DisplayName": "Nine"
            })))
            .mount(server)
            .await;

        let info = validate_cookie("minimal-cookie")
            .await
            .expect("account info");
        assert_eq!(info.user_id, 9);
        assert!(info.user_email.is_none());
        assert!(!info.is_email_verified);
        assert_eq!(info.age_bracket, 0);
        assert!(!info.user_above_13);
    }

    /// The same payload is also accepted under the struct's own field names,
    /// which is what the PascalCase aliases exist for.
    #[tokio::test]
    async fn validate_cookie_accepts_the_snake_case_field_names() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path(mock_path("www", "/my/account/json")))
            .and(header("cookie", cookie_of("snake-cookie")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "user_id": 10,
                "name": "ten",
                "display_name": "Ten"
            })))
            .mount(server)
            .await;

        let info = validate_cookie("snake-cookie").await.expect("account info");
        assert_eq!(info.user_id, 10);
        assert_eq!(info.display_name, "Ten");
    }
}
