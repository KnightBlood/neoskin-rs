//! OAuth2 管理（Passport 兼容）：scopes/clients/tokens/personal tokens。
//!
//! token 交换（authorization_code / refresh_token）与授权页在后续补齐；
//! 本阶段先让个人令牌与客户端管理可用，Bearer 校验随后接入。

use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use suprnova::session::invalidate_session;
use suprnova::{
    Cookie, FrameworkError, HttpResponse, Model, Request, Response, SameSite, SessionConfig, attrs,
    get_csrf_token, handler, session, session_mut,
};

use crate::models::oauth::{OAuthAccessToken, OAuthAuthCode, OAuthClient, OAuthRefreshToken};
use crate::models::scope::Scope;
use crate::support::api::{require_user, validation_error};
use crate::support::hash::verify_password;
use crate::support::oauth as oauth_support;
use crate::support::remember::random_token;

fn not_found() -> Response {
    Ok(HttpResponse::text("Not Found").status(404))
}

fn no_content() -> Response {
    Ok(HttpResponse::text("").status(204))
}

#[derive(Deserialize)]
pub struct ClientRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub redirect: String,
}

#[derive(Deserialize)]
pub struct PersonalTokenRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scopes: Option<serde_json::Value>,
}

fn parse_scopes(value: Option<serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::trim).map(ToString::to_string))
            .filter(|item| !item.is_empty())
            .collect(),
        Some(serde_json::Value::String(raw)) => raw
            .replace(',', " ")
            .split_whitespace()
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

async fn known_scopes() -> Result<Vec<String>, FrameworkError> {
    let mut names: Vec<String> = oauth_support::DEFAULT_SCOPES
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    for scope in Scope::all().await? {
        if !names.contains(&scope.name) {
            names.push(scope.name);
        }
    }
    Ok(names)
}

#[handler]
pub async fn scopes() -> Response {
    let mut data: Vec<serde_json::Value> = oauth_support::DEFAULT_SCOPES
        .iter()
        .map(|(name, description)| json!({ "id": name, "description": description }))
        .collect();
    if let Ok(custom) = Scope::all().await {
        for scope in custom {
            if !oauth_support::is_default_scope(&scope.name) {
                data.push(json!({ "id": scope.name, "description": scope.description }));
            }
        }
    }
    Ok(HttpResponse::json(json!(data)))
}

#[handler]
pub async fn clients(request: Request) -> Response {
    let user = require_user(&request).await?;
    let all = OAuthClient::all().await?;
    let data: Vec<serde_json::Value> = all
        .iter()
        .filter(|client| client.user_id == Some(user.uid as u64))
        .map(oauth_support::client_json)
        .collect();
    Ok(HttpResponse::json(json!(data)))
}

#[handler]
pub async fn store_client(request: Request) -> Response {
    let user = require_user(&request).await?;
    let body: ClientRequest = request.input().await?;
    let name = body.name.trim();
    let redirect = body.redirect.trim();
    if name.is_empty() {
        return Err(validation_error("name", "名称不能为空。").into());
    }
    if redirect.is_empty() {
        return Err(validation_error("redirect", "回调地址不能为空。").into());
    }

    let secret = random_token();
    let hashed = oauth_support::hash_client_secret(&secret).await?;
    let now = Utc::now();
    let client = OAuthClient::create(attrs! {
        user_id: Some(user.uid as u64),
        name: name.to_string(),
        secret: Some(hashed),
        provider: None::<String>,
        redirect: redirect.to_string(),
        personal_access_client: false,
        password_client: false,
        revoked: false,
        created_at: Some(now),
        updated_at: Some(now),
    })
    .await?;

    let mut value = oauth_support::client_json(&client);
    if let Some(object) = value.as_object_mut() {
        object.insert("plain_secret".to_string(), json!(secret));
    }
    Ok(HttpResponse::json(value))
}

#[handler]
pub async fn update_client(request: Request) -> Response {
    let user = require_user(&request).await?;
    let id: u64 = request
        .param("client_id")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let Some(client) = OAuthClient::find(id).await? else {
        return not_found();
    };
    if client.user_id != Some(user.uid as u64) {
        return not_found();
    }
    let body: ClientRequest = request.input().await?;
    let name = if body.name.trim().is_empty() {
        client.name.clone()
    } else {
        body.name.trim().to_string()
    };
    let redirect = if body.redirect.trim().is_empty() {
        client.redirect.clone()
    } else {
        body.redirect.trim().to_string()
    };
    client
        .update(attrs! {
            name: name,
            redirect: redirect,
            updated_at: Some(Utc::now()),
        })
        .await?;
    match OAuthClient::find(id).await? {
        Some(updated) => Ok(HttpResponse::json(oauth_support::client_json(&updated))),
        None => not_found(),
    }
}

#[handler]
pub async fn delete_client(request: Request) -> Response {
    let user = require_user(&request).await?;
    let id: u64 = request
        .param("client_id")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let Some(client) = OAuthClient::find(id).await? else {
        return not_found();
    };
    if client.user_id != Some(user.uid as u64) {
        return not_found();
    }
    client.delete().await?;
    no_content()
}

async fn token_entries(
    user_id: u64,
    personal_only: bool,
) -> Result<Vec<serde_json::Value>, FrameworkError> {
    let tokens = OAuthAccessToken::all().await?;
    let clients = OAuthClient::all().await?;
    let now = Utc::now();
    Ok(tokens
        .iter()
        .filter(|token| token.user_id == Some(user_id))
        .filter(|token| !token.revoked)
        .filter(|token| token.expires_at.map(|exp| exp > now).unwrap_or(true))
        .filter(|token| {
            if !personal_only {
                return true;
            }
            clients
                .iter()
                .find(|client| client.id == token.client_id)
                .map(|client| client.personal_access_client)
                .unwrap_or(false)
        })
        .map(|token| {
            let client = clients.iter().find(|client| client.id == token.client_id);
            oauth_support::token_json(token, client)
        })
        .collect())
}

#[handler]
pub async fn tokens(request: Request) -> Response {
    let user = require_user(&request).await?;
    let data = token_entries(user.uid as u64, false).await?;
    Ok(HttpResponse::json(json!(data)))
}

#[handler]
pub async fn delete_token(request: Request) -> Response {
    let user = require_user(&request).await?;
    let id = request.param("token_id").unwrap_or_default().to_string();
    revoke_token(user.uid as u64, &id).await?
}

#[handler]
pub async fn personal_tokens(request: Request) -> Response {
    let user = require_user(&request).await?;
    let data = token_entries(user.uid as u64, true).await?;
    Ok(HttpResponse::json(json!(data)))
}

#[handler]
pub async fn store_personal_token(request: Request) -> Response {
    let user = require_user(&request).await?;
    let body: PersonalTokenRequest = request.input().await?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(validation_error("name", "名称不能为空。").into());
    }
    if name.chars().count() > 255 {
        return Err(validation_error("name", "名称不能超过 255 个字符。").into());
    }
    let scopes = parse_scopes(body.scopes);
    let known = known_scopes().await?;
    if let Some(scope) = scopes.iter().find(|scope| !known.contains(scope)) {
        return Err(
            validation_error("scopes", &format!("已选的属性 scopes 非法（{scope}）。")).into(),
        );
    }

    let client = oauth_support::personal_client().await?;
    let issued =
        oauth_support::issue(Some(user.uid as u64), client.id, Some(name), &scopes, false).await?;
    Ok(HttpResponse::json(json!({
        "accessTokenId": issued.token_id,
        "tokenType": "Bearer",
        "expiresIn": issued.expires_in,
        "accessToken": issued.access_token,
    })))
}

#[handler]
pub async fn delete_personal_token(request: Request) -> Response {
    let user = require_user(&request).await?;
    let id = request.param("token_id").unwrap_or_default().to_string();
    revoke_token(user.uid as u64, &id).await?
}

async fn revoke_token(user_id: u64, id: &str) -> Result<Response, FrameworkError> {
    let Some(token) = OAuthAccessToken::find(id.to_string()).await? else {
        return Ok(not_found());
    };
    if token.user_id != Some(user_id) {
        return Ok(not_found());
    }
    token
        .update(attrs! {
            revoked: true,
            updated_at: Some(Utc::now()),
        })
        .await?;

    for refresh in OAuthRefreshToken::all().await? {
        if refresh.access_token_id == id && !refresh.revoked {
            refresh
                .update(attrs! {
                    revoked: true,
                })
                .await?;
        }
    }
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// 授权码流程（Passport / league-oauth2 兼容）
// ---------------------------------------------------------------------------

const SESSION_AUTH_TOKEN: &str = "oauth_auth_token";
const SESSION_CLIENT_ID: &str = "oauth_client_id";
const SESSION_REDIRECT_URI: &str = "oauth_redirect_uri";
const SESSION_STATE: &str = "oauth_state";
const SESSION_SCOPES: &str = "oauth_scopes";

const AUTHORIZE_CSS: &str = r#"
:root { color-scheme: light dark; }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; display: grid; place-items: center;
  background: #f4f5f7; color: #1f2329; font-family: -apple-system,
  BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; }
.card { width: min(460px, calc(100vw - 32px)); background: #fff; border-radius: 14px;
  padding: 32px; box-shadow: 0 12px 32px rgba(15, 23, 42, .08); }
h1 { margin: 0 0 16px; font-size: 22px; }
.lead { margin: 0 0 8px; line-height: 1.6; }
.hint { margin: 0 0 12px; color: #646a73; font-size: 14px; }
ul { margin: 0 0 24px; padding-left: 20px; color: #3b4250; line-height: 1.8; }
code { font-size: 13px; background: #f1f3f5; padding: 1px 5px; border-radius: 4px; }
.actions { display: flex; gap: 12px; }
button { flex: 1; border: 0; border-radius: 8px; padding: 11px 18px;
  font-size: 15px; cursor: pointer; }
.primary { background: #3370ff; color: #fff; }
.primary:hover { background: #245bdb; }
.secondary { background: #eff0f1; color: #1f2329; }
.secondary:hover { background: #e2e4e8; }
@media (prefers-color-scheme: dark) {
  body { background: #17181a; color: #e8eaed; }
  .card { background: #232529; box-shadow: none; }
  .hint { color: #9aa0a6; }
  ul { color: #c4c8ce; }
  code { background: #2f3238; }
  .secondary { background: #2f3238; color: #e8eaed; }
}
"#;

const AUTHORIZE_SCRIPT: &str = r#"
(() => {
  const button = document.getElementById('deny');
  if (!button) return;
  const csrf = document.querySelector('meta[name="csrf-token"]').content;
  button.addEventListener('click', async () => {
    const target = button.dataset.denyUrl;
    button.disabled = true;
    try {
      await fetch('/oauth/authorize', {
        method: 'DELETE',
        headers: { 'X-XSRF-TOKEN': csrf, 'X-CSRF-TOKEN': csrf },
        credentials: 'same-origin'
      });
    } catch (error) {
      // 忽略网络错误，仍按拒绝语义跳回客户端。
    }
    window.location.assign(target);
  });
})();
"#;

fn escape_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
    output
}

fn encode_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn append_query(base: &str, pairs: &[(&str, String)]) -> String {
    let mut url = base.to_string();
    let mut separator = if url.contains('?') { '&' } else { '?' };
    for (key, value) in pairs {
        if value.is_empty() {
            continue;
        }
        url.push(separator);
        url.push_str(key);
        url.push('=');
        url.push_str(&encode_component(value));
        separator = '&';
    }
    url
}

fn session_value<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    session().and_then(|data| data.get::<T>(key))
}

fn put_session<T: serde::Serialize>(key: &str, value: T) -> Result<(), FrameworkError> {
    session_mut(|data| data.put(key, value))
        .map(|_| ())
        .ok_or_else(|| FrameworkError::internal("session is not available"))
}

fn clear_authorize_session() {
    session_mut(|data| {
        for key in [
            SESSION_AUTH_TOKEN,
            SESSION_CLIENT_ID,
            SESSION_REDIRECT_URI,
            SESSION_STATE,
            SESSION_SCOPES,
        ] {
            data.forget(key);
        }
    });
}

async fn scope_description(name: &str) -> String {
    if let Some((_, description)) = oauth_support::DEFAULT_SCOPES
        .iter()
        .find(|(scope, _)| *scope == name)
    {
        return (*description).to_string();
    }
    if let Ok(scopes) = Scope::all().await
        && let Some(scope) = scopes.into_iter().find(|scope| scope.name == name)
    {
        return scope.description;
    }
    name.to_string()
}

/// `redirect_uri?error=...&state=...`。
fn redirect_error(redirect_uri: &str, error: &str, state: &str) -> Response {
    let target = append_query(
        redirect_uri,
        &[("error", error.to_string()), ("state", state.to_string())],
    );
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", target))
}

/// 下发授权码并回跳（批准与自动批准共用）。
async fn issue_code(
    user_id: u64,
    client_id: u64,
    scopes: &[String],
    redirect_uri: &str,
    state: &str,
) -> Response {
    let code = random_token();
    OAuthAuthCode::create(attrs! {
        id: code.clone(),
        user_id: user_id,
        client_id: client_id,
        scopes: Some(scopes.join(" ")),
        revoked: false,
        expires_at: Some(Utc::now() + Duration::seconds(oauth_support::CODE_TTL_SECONDS)),
    })
    .await?;
    let target = append_query(
        redirect_uri,
        &[("code", code), ("state", state.to_string())],
    );
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", target))
}

/// Passport `hasGrantedScopes`：用户是否已有覆盖所请求 scope 的有效令牌。
async fn has_granted_scopes(
    user_id: u64,
    client_id: u64,
    scopes: &[String],
) -> Result<bool, FrameworkError> {
    let now = Utc::now();
    let mut granted = std::collections::HashSet::new();
    let mut any_active = false;
    for token in OAuthAccessToken::all().await?.into_vec() {
        let active = token.user_id == Some(user_id)
            && token.client_id == client_id
            && !token.revoked
            && token
                .expires_at
                .map(|expires| expires > now)
                .unwrap_or(true);
        if !active {
            continue;
        }
        any_active = true;
        if let Some(raw) = token.scopes.as_deref() {
            for scope in oauth_support::parse_scopes(raw) {
                granted.insert(scope);
            }
        }
    }
    if scopes.is_empty() {
        return Ok(any_active);
    }
    Ok(scopes.iter().all(|scope| granted.contains(scope)))
}

/// GET /oauth/authorize —— Passport 授权页。
#[handler]
pub async fn authorize(request: Request) -> Response {
    let Some(client_id) = request
        .query_param("client_id")
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return Ok(oauth_support::error_response(
            400,
            "invalid_client",
            "Client authentication failed.",
        ));
    };
    let redirect_uri = request.query_param("redirect_uri").unwrap_or_default();
    let response_type = request
        .query_param("response_type")
        .unwrap_or_else(|| "code".to_string());
    let state = request.query_param("state").unwrap_or_default();
    let prompt: Vec<String> = request
        .query_param("prompt")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let wants_none = prompt.iter().any(|value| value == "none");
    let wants_login = prompt.iter().any(|value| value == "login");
    let wants_consent = prompt.iter().any(|value| value == "consent");
    let mut scopes = oauth_support::parse_scopes(&request.query_param("scope").unwrap_or_default());

    let Some(client) = OAuthClient::find(client_id).await? else {
        return Ok(oauth_support::error_response(
            400,
            "invalid_client",
            "Client authentication failed.",
        ));
    };
    if client.revoked {
        return Ok(oauth_support::error_response(
            400,
            "invalid_client",
            "Client is revoked.",
        ));
    }
    if !oauth_support::redirect_allowed(&client.redirect, &redirect_uri) {
        return Ok(oauth_support::error_response(
            400,
            "invalid_redirect_uri",
            "The requested redirect URI is not registered for this client.",
        ));
    }
    if response_type != "code" {
        return Ok(oauth_support::error_response(
            400,
            "unsupported_response_type",
            "Only the authorization code response type is supported.",
        ));
    }
    if scopes.is_empty() {
        scopes.push("User.Read".to_string());
    }
    let known = known_scopes().await?;
    if let Some(scope) = scopes.iter().find(|scope| !known.contains(scope)) {
        return Ok(oauth_support::error_response(
            400,
            "invalid_scope",
            &format!("The requested scope is invalid: {scope}."),
        ));
    }

    // `prompt=login`：强制登出一次，登录后回到本授权请求。
    if wants_login {
        let prompted = session()
            .and_then(|data| data.get::<bool>("prompted_for_login"))
            .unwrap_or(false);
        if !prompted {
            invalidate_session();
            let current = match request.query() {
                Some(query) => format!("{}?{}", request.path(), query),
                None => request.path().to_string(),
            };
            session_mut(|data| {
                data.put("prompted_for_login", true);
                data.put("last_requested_path", current.clone());
            });
            let target = format!("/auth/login?redirect={}", encode_component(&current));
            return Ok(HttpResponse::text("")
                .status(302)
                .header("Location", target));
        }
        session_mut(|data| {
            data.forget("prompted_for_login");
        });
    }

    let Some(user) = crate::support::auth::current_user(&request).await? else {
        if wants_none {
            return redirect_error(&redirect_uri, "login_required", &state);
        }
        let current = match request.query() {
            Some(query) => format!("{}?{}", request.path(), query),
            None => request.path().to_string(),
        };
        session_mut(|data| data.put("last_requested_path", current.clone()));
        let target = format!("/auth/login?redirect={}", encode_component(&current));
        return Ok(HttpResponse::text("")
            .status(302)
            .header("Location", target));
    };

    // 已授权过相同 scope 且未要求 consent 时直接放行（Passport 行为）。
    if !wants_consent && has_granted_scopes(user.uid as u64, client_id, &scopes).await? {
        return issue_code(user.uid as u64, client_id, &scopes, &redirect_uri, &state).await;
    }
    if wants_none {
        return redirect_error(&redirect_uri, "consent_required", &state);
    }

    let auth_token = random_token();
    put_session(SESSION_AUTH_TOKEN, auth_token.clone())?;
    put_session(SESSION_CLIENT_ID, client_id)?;
    put_session(SESSION_REDIRECT_URI, redirect_uri.clone())?;
    put_session(SESSION_STATE, state.clone())?;
    put_session(SESSION_SCOPES, scopes.clone())?;

    let mut scope_items = String::new();
    for scope in &scopes {
        let description = scope_description(scope).await;
        scope_items.push_str(&format!(
            "<li><code>{}</code> {}</li>",
            escape_html(scope),
            escape_html(&description)
        ));
    }

    let csrf = get_csrf_token().unwrap_or_default();
    let deny_pairs = [
        ("error", "access_denied".to_string()),
        ("state", state.clone()),
    ];
    let deny_url = append_query(&redirect_uri, &deny_pairs);

    let page = format!(
        "<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"csrf-token\" content=\"{csrf}\">\n\
         <title>授权应用 - NeoSkin</title>\n<style>{AUTHORIZE_CSS}</style>\n</head>\n<body>\n\
         <main class=\"card\">\n<h1>授权应用</h1>\n\
         <p class=\"lead\"><strong>{client}</strong> 请求访问你的账号 <strong>{nickname}</strong>。</p>\n\
         <p class=\"hint\">授权后，该应用将获得以下权限：</p>\n<ul>{scope_items}</ul>\n\
         <form method=\"post\" action=\"/oauth/authorize\">\n\
         <input type=\"hidden\" name=\"_token\" value=\"{csrf}\">\n\
         <input type=\"hidden\" name=\"auth_token\" value=\"{auth_token}\">\n\
         <div class=\"actions\">\n\
         <button type=\"submit\" class=\"primary\">授权</button>\n\
         <button type=\"button\" id=\"deny\" class=\"secondary\" data-deny-url=\"{deny_url}\">拒绝</button>\n\
         </div>\n</form>\n</main>\n<script>{AUTHORIZE_SCRIPT}</script>\n</body>\n</html>\n",
        csrf = escape_html(&csrf),
        client = escape_html(&client.name),
        nickname = escape_html(&user.nickname),
        scope_items = scope_items,
        auth_token = escape_html(&auth_token),
        deny_url = escape_html(&deny_url),
    );

    Ok(HttpResponse::html(page))
}

#[derive(Deserialize)]
pub struct ApproveRequest {
    #[serde(default)]
    pub auth_token: String,
}

/// POST /oauth/authorize —— 批准授权并下发 authorization code。
#[handler]
pub async fn approve(request: Request) -> Response {
    let user = require_user(&request).await?;
    let body: ApproveRequest = request.input().await?;

    let Some(stored) = session_value::<String>(SESSION_AUTH_TOKEN) else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    if body.auth_token != stored {
        return Ok(HttpResponse::text("Forbidden").status(403));
    }
    let Some(client_id) = session_value::<u64>(SESSION_CLIENT_ID) else {
        clear_authorize_session();
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let Some(redirect_uri) = session_value::<String>(SESSION_REDIRECT_URI) else {
        clear_authorize_session();
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let state = session_value::<String>(SESSION_STATE).unwrap_or_default();
    let scopes = session_value::<Vec<String>>(SESSION_SCOPES).unwrap_or_default();

    let response = issue_code(user.uid as u64, client_id, &scopes, &redirect_uri, &state).await;
    clear_authorize_session();
    response
}

/// DELETE /oauth/authorize —— 拒绝授权，带 `error=access_denied` 回跳。
#[handler]
pub async fn deny(request: Request) -> Response {
    let _ = require_user(&request).await?;
    let redirect_uri = session_value::<String>(SESSION_REDIRECT_URI);
    let state = session_value::<String>(SESSION_STATE).unwrap_or_default();
    clear_authorize_session();

    let Some(redirect_uri) = redirect_uri else {
        return Ok(HttpResponse::text("").status(204));
    };
    let target = append_query(
        &redirect_uri,
        &[("error", "access_denied".to_string()), ("state", state)],
    );
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", target))
}

#[derive(Deserialize, Default)]
pub struct TokenRequest {
    #[serde(default)]
    pub grant_type: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// 解析 `Authorization: Basic base64(client_id:client_secret)`。
fn basic_credentials(header: Option<&str>) -> Option<(String, String)> {
    use base64::Engine;
    let header = header?;
    let encoded = header
        .strip_prefix("Basic ")
        .or_else(|| header.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (id, secret) = text.split_once(':')?;
    Some((id.to_string(), secret.to_string()))
}

/// POST /oauth/token —— authorization_code / refresh_token 交换。
#[handler]
pub async fn token(request: Request) -> Response {
    let basic = basic_credentials(request.header("authorization"));
    let body: TokenRequest = request.input().await?;
    match body.grant_type.trim() {
        "authorization_code" => token_from_code(body, basic).await,
        "refresh_token" => token_from_refresh(body, basic).await,
        "client_credentials" => token_from_client_credentials(body, basic).await,
        grant => Ok(oauth_support::error_response(
            400,
            "unsupported_grant_type",
            &format!("The grant type `{grant}` is not supported."),
        )),
    }
}

async fn token_from_code(body: TokenRequest, basic: Option<(String, String)>) -> Response {
    let code_value = body.code.unwrap_or_default();
    if code_value.is_empty() {
        return Ok(oauth_support::error_response(
            400,
            "invalid_request",
            "The authorization code is missing.",
        ));
    }
    let client_id = basic
        .as_ref()
        .and_then(|(id, _)| id.parse::<u64>().ok())
        .or_else(|| {
            body.client_id
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
        });
    let Some(client_id) = client_id else {
        return Ok(oauth_support::error_response(
            401,
            "invalid_client",
            "Client authentication failed.",
        ));
    };

    let Some(client) = OAuthClient::find(client_id).await? else {
        return Ok(oauth_support::error_response(
            401,
            "invalid_client",
            "Client authentication failed.",
        ));
    };
    if client.revoked {
        return Ok(oauth_support::error_response(
            401,
            "invalid_client",
            "Client is revoked.",
        ));
    }
    if let Some(hash) = client.secret.as_deref() {
        let provided = basic
            .as_ref()
            .map(|(_, secret)| secret.as_str())
            .or(body.client_secret.as_deref())
            .unwrap_or("");
        if !verify_password(hash, provided) {
            return Ok(oauth_support::error_response(
                401,
                "invalid_client",
                "Client authentication failed.",
            ));
        }
    }

    let Some(code) = OAuthAuthCode::find(code_value).await? else {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The authorization code is invalid.",
        ));
    };
    if code.revoked || oauth_support::is_expired(code.expires_at) {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The authorization code is expired or revoked.",
        ));
    }
    if code.client_id != client_id {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The authorization code was issued to another client.",
        ));
    }
    if let Some(redirect) = body.redirect_uri.as_deref()
        && !redirect.is_empty()
        && !oauth_support::redirect_allowed(&client.redirect, redirect)
    {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The redirect URI does not match the authorization request.",
        ));
    }

    let user_id = code.user_id;
    let scopes = code
        .scopes
        .as_deref()
        .map(oauth_support::parse_scopes)
        .unwrap_or_default();
    code.update(attrs! { revoked: true }).await?;

    let issued = oauth_support::issue(Some(user_id), client_id, None, &scopes, true).await?;
    Ok(HttpResponse::json(oauth_support::token_response(&issued)))
}

async fn token_from_refresh(body: TokenRequest, basic: Option<(String, String)>) -> Response {
    let refresh_value = body.refresh_token.clone().unwrap_or_default();
    if refresh_value.is_empty() {
        return Ok(oauth_support::error_response(
            400,
            "invalid_request",
            "The refresh token is missing.",
        ));
    }
    let Some(refresh) = OAuthRefreshToken::find(refresh_value).await? else {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The refresh token is invalid.",
        ));
    };
    if refresh.revoked || oauth_support::is_expired(refresh.expires_at) {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The refresh token is expired or revoked.",
        ));
    }
    let Some(access) = OAuthAccessToken::find(refresh.access_token_id.clone()).await? else {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The access token is invalid.",
        ));
    };
    if access.revoked {
        return Ok(oauth_support::error_response(
            400,
            "invalid_grant",
            "The access token is revoked.",
        ));
    }

    // Passport 的刷新请求会带上 client_id/client_secret（或 HTTP Basic）；
    // 提供了就必须匹配。
    let client_id = basic
        .as_ref()
        .and_then(|(id, _)| id.parse::<u64>().ok())
        .or_else(|| {
            body.client_id
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
        });
    if let Some(client_id) = client_id {
        let Some(client) = OAuthClient::find(client_id).await? else {
            return Ok(oauth_support::error_response(
                401,
                "invalid_client",
                "Client authentication failed.",
            ));
        };
        if client.revoked || client.id != access.client_id {
            return Ok(oauth_support::error_response(
                401,
                "invalid_client",
                "Client authentication failed.",
            ));
        }
        if let Some(hash) = client.secret.as_deref() {
            let provided = basic
                .as_ref()
                .map(|(_, secret)| secret.as_str())
                .or(body.client_secret.as_deref())
                .unwrap_or("");
            if !verify_password(hash, provided) {
                return Ok(oauth_support::error_response(
                    401,
                    "invalid_client",
                    "Client authentication failed.",
                ));
            }
        }
    }

    let user_id = access.user_id;
    let client_id = access.client_id;
    let name = access.name.clone();
    let scopes = access
        .scopes
        .as_deref()
        .map(oauth_support::parse_scopes)
        .unwrap_or_default();
    refresh.update(attrs! { revoked: true }).await?;
    access
        .update(attrs! {
            revoked: true,
            updated_at: Some(Utc::now()),
        })
        .await?;

    let issued = oauth_support::issue(user_id, client_id, name.as_deref(), &scopes, true).await?;
    Ok(HttpResponse::json(oauth_support::token_response(&issued)))
}

/// `client_credentials` grant（Passport 默认启用，`password` grant 默认关闭）。
///
/// 与授权码流程不同：没有用户、没有 refresh token；`scope` 省略时退回
/// Passport 的默认 scope `User.Read`。
async fn token_from_client_credentials(
    body: TokenRequest,
    basic: Option<(String, String)>,
) -> Response {
    let client_id = basic
        .as_ref()
        .and_then(|(id, _)| id.parse::<u64>().ok())
        .or_else(|| {
            body.client_id
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
        });
    let Some(client_id) = client_id else {
        return Ok(oauth_support::error_response(
            401,
            "invalid_client",
            "Client authentication failed.",
        ));
    };

    let Some(client) = OAuthClient::find(client_id).await? else {
        return Ok(oauth_support::error_response(
            401,
            "invalid_client",
            "Client authentication failed.",
        ));
    };
    if client.revoked {
        return Ok(oauth_support::error_response(
            401,
            "invalid_client",
            "Client is revoked.",
        ));
    }
    if let Some(hash) = client.secret.as_deref() {
        let provided = basic
            .as_ref()
            .map(|(_, secret)| secret.as_str())
            .or(body.client_secret.as_deref())
            .unwrap_or("");
        if !verify_password(hash, provided) {
            return Ok(oauth_support::error_response(
                401,
                "invalid_client",
                "Client authentication failed.",
            ));
        }
    }

    let scopes = body
        .scope
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(oauth_support::parse_scopes)
        .unwrap_or_else(|| vec!["User.Read".to_string()]);

    let issued = oauth_support::issue(None, client.id, None, &scopes, false).await?;
    Ok(HttpResponse::json(oauth_support::token_response(&issued)))
}

/// POST /oauth/token/refresh —— 与 Passport 的 transient token 端点对齐。
///
/// PHP 版把 HS256 JWT 放进加密的 `laravel_token` cookie；Rust 端直接写
/// HS256 JWT（与 Go 端口一致），由后续 Cookie 鉴权中间件校验。
#[handler]
pub async fn refresh_transient_token(request: Request) -> Response {
    let user = require_user(&request).await?;
    let key = std::env::var("NEOSKIN_APP_KEY")
        .or_else(|_| std::env::var("APP_KEY"))
        .map_err(|_| FrameworkError::internal("APP_KEY is not configured"))?;
    let minutes: i64 = 120;
    let expires_at = Utc::now() + Duration::minutes(minutes);
    let claims = json!({
        "sub": user.uid.to_string(),
        "csrf": get_csrf_token().unwrap_or_default(),
        "exp": expires_at.timestamp(),
    });
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(key.as_bytes()),
    )
    .map_err(|error| FrameworkError::internal(error.to_string()))?;

    let secure = SessionConfig::from_env().cookie_secure;
    let cookie = Cookie::new("laravel_token", token)
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(std::time::Duration::from_secs(
            (minutes * 60).try_into().unwrap_or(7200),
        ));

    Ok(HttpResponse::text("Refreshed.").cookie(cookie))
}

#[cfg(test)]
mod tests {
    use super::{append_query, encode_component, escape_html};

    #[test]
    fn escape_html_covers_markup_characters() {
        assert_eq!(
            escape_html("<script>alert('x&y')</script>"),
            "&lt;script&gt;alert(&#39;x&amp;y&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn encode_component_keeps_unreserved_bytes() {
        assert_eq!(encode_component("aZ0-._~"), "aZ0-._~");
        assert_eq!(encode_component("a b&c"), "a%20b%26c");
    }

    #[test]
    fn append_query_handles_existing_query_and_empty_values() {
        assert_eq!(
            append_query("https://example.com/cb", &[("code", "abc".into())]),
            "https://example.com/cb?code=abc"
        );
        assert_eq!(
            append_query(
                "https://example.com/cb?x=1",
                &[("code", "a b".into()), ("state", String::new())]
            ),
            "https://example.com/cb?x=1&code=a%20b"
        );
    }
}
