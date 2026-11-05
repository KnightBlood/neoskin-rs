//! Passport 兼容的 OAuth2：默认 scope、RSA 密钥与 JWT 签发。

use std::path::PathBuf;
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openssl::rsa::Rsa;
use serde_json::json;
use suprnova::attrs;
use suprnova::{FrameworkError, HttpResponse, Model};

use crate::models::oauth::{OAuthAccessToken, OAuthClient, OAuthRefreshToken};
use crate::models::user::User;
use crate::support::hash::hash_password;
use crate::support::remember::random_token;

pub const TOKEN_TTL_SECONDS: i64 = 31_536_000;
pub const CODE_TTL_SECONDS: i64 = 600;

/// 与 PHP `config/oauth.php` / Go `oauth.DefaultScopes` 一致（含历史拼写
/// `Closet.ReadWrtie`，不可擅自修正）。
pub const DEFAULT_SCOPES: &[(&str, &str)] = &[
    ("User.Read", "auth.oauth.scope.user.read"),
    ("Notification.Read", "auth.oauth.scope.notification.read"),
    (
        "Notification.ReadWrite",
        "auth.oauth.scope.notification.readwrite",
    ),
    ("Player.Read", "auth.oauth.scope.player.read"),
    ("Player.ReadWrite", "auth.oauth.scope.player.readwrite"),
    ("Closet.Read", "auth.oauth.scope.closet.read"),
    ("Closet.ReadWrtie", "auth.oauth.scope.closet.readwrite"),
    (
        "UsersManagement.Read",
        "auth.oauth.scope.users-management.read",
    ),
    (
        "UsersManagement.ReadWrite",
        "auth.oauth.scope.users-management.readwrite",
    ),
    (
        "PlayersManagement.Read",
        "auth.oauth.scope.players-management.read",
    ),
    (
        "PlayersManagement.ReadWrite",
        "auth.oauth.scope.players-management.readwrite",
    ),
    (
        "ClosetManagement.Read",
        "auth.oauth.scope.closet-management.read",
    ),
    (
        "ClosetManagement.ReadWrite",
        "auth.oauth.scope.closet-management.readwrite",
    ),
    (
        "ReportsManagement.Read",
        "auth.oauth.scope.reports-management.read",
    ),
    (
        "ReportsManagement.ReadWrite",
        "auth.oauth.scope.reports-management.readwrite",
    ),
];

pub fn is_default_scope(name: &str) -> bool {
    DEFAULT_SCOPES.iter().any(|(scope, _)| *scope == name)
}

fn storage_root() -> PathBuf {
    std::env::var("NEOSKIN_STORAGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("storage"))
}

struct Keys {
    private_pem: Vec<u8>,
    public_pem: Vec<u8>,
}

static KEYS: OnceLock<Result<Keys, String>> = OnceLock::new();

fn load_keys() -> Result<&'static Keys, FrameworkError> {
    KEYS.get_or_init(|| {
        let root = storage_root();
        let private_path = root.join("oauth-private.key");
        let public_path = root.join("oauth-public.key");
        if let (Ok(private_pem), Ok(public_pem)) =
            (std::fs::read(&private_path), std::fs::read(&public_path))
        {
            return Ok(Keys {
                private_pem,
                public_pem,
            });
        }

        let rsa = Rsa::generate(2048).map_err(|error| error.to_string())?;
        let private_pem = rsa
            .private_key_to_pem()
            .map_err(|error| error.to_string())?;
        let public_pem = rsa.public_key_to_pem().map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        std::fs::write(&private_path, &private_pem).map_err(|error| error.to_string())?;
        std::fs::write(&public_path, &public_pem).map_err(|error| error.to_string())?;
        Ok(Keys {
            private_pem,
            public_pem,
        })
    })
    .as_ref()
    .map_err(|message| FrameworkError::internal(message.clone()))
}

/// 校验 Passport RS256 JWT（返回 claims）；供 Bearer 中间件后续使用。
pub fn verify_jwt(token: &str) -> Option<serde_json::Value> {
    let keys = load_keys().ok()?;
    let decoding = jsonwebtoken::DecodingKey::from_rsa_pem(&keys.public_pem).ok()?;
    let mut validation = jsonwebtoken::Validation::new(Algorithm::RS256);
    validation.validate_exp = true;
    // Passport 的 `aud` 是客户端 id；这里只校验签名与过期，不限定 audience。
    validation.validate_aud = false;
    jsonwebtoken::decode::<serde_json::Value>(token, &decoding, &validation)
        .ok()
        .map(|data| data.claims)
}

pub struct Issued {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_id: String,
    pub expires_in: i64,
}

pub fn format_datetime(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Passport 允许 `redirect` 字段用逗号登记多个回调地址。
pub fn redirect_allowed(registered: &str, target: &str) -> bool {
    !target.is_empty()
        && registered
            .split(',')
            .any(|candidate| candidate.trim() == target)
}

pub fn is_expired(expires_at: Option<DateTime<Utc>>) -> bool {
    expires_at.map(|value| value <= Utc::now()).unwrap_or(true)
}

/// 按空格/逗号拆分 scope（Passport 用空格，部分客户端用逗号）。
pub fn parse_scopes(raw: &str) -> Vec<String> {
    raw.replace(',', " ")
        .split_whitespace()
        .map(ToString::to_string)
        .filter(|value| !value.is_empty())
        .collect()
}

/// Passport/league 的错误响应体，保持字段名一致。
pub fn error_response(status: u16, error: &str, description: &str) -> HttpResponse {
    HttpResponse::json(json!({
        "error": error,
        "error_description": description,
        "hint": description,
        "message": description,
    }))
    .status(status)
}

/// 授权码 / 刷新令牌的 token 响应。
pub fn token_response(issued: &Issued) -> serde_json::Value {
    let mut body = json!({
        "token_type": "Bearer",
        "expires_in": issued.expires_in,
        "access_token": issued.access_token,
    });
    if let Some(refresh) = &issued.refresh_token
        && let Some(object) = body.as_object_mut()
    {
        object.insert("refresh_token".to_string(), json!(refresh));
    }
    body
}

/// 找一个 personal access client；没有则创建（Passport `--personal` 的等价物）。
pub async fn personal_client() -> Result<OAuthClient, FrameworkError> {
    let clients = OAuthClient::all().await?;
    if let Some(client) = clients.iter().find(|client| client.personal_access_client) {
        return Ok(client.clone());
    }
    let now = Utc::now();
    let created = OAuthClient::create(attrs! {
        user_id: None::<u64>,
        name: "NeoSkin Personal Access Client",
        secret: None::<String>,
        provider: None::<String>,
        redirect: "",
        personal_access_client: true,
        password_client: false,
        revoked: false,
        created_at: Some(now),
        updated_at: Some(now),
    })
    .await?;
    Ok(created)
}

/// 签发 access token（可选 refresh token），返回 JWT 与 token id。
pub async fn issue(
    user_id: Option<u64>,
    client_id: u64,
    name: Option<&str>,
    scopes: &[String],
    with_refresh: bool,
) -> Result<Issued, FrameworkError> {
    let keys = load_keys()?;
    let now = Utc::now();
    let exp = now + chrono::Duration::seconds(TOKEN_TTL_SECONDS);
    let token_id = random_token();
    let scope_value = scopes.join(" ");

    OAuthAccessToken::create(attrs! {
        id: token_id.clone(),
        user_id: user_id,
        client_id: client_id,
        name: name.map(ToString::to_string),
        scopes: Some(scope_value),
        revoked: false,
        created_at: Some(now),
        updated_at: Some(now),
        expires_at: Some(exp),
    })
    .await?;

    let sub = user_id
        .map(|uid| uid.to_string())
        .unwrap_or_else(|| client_id.to_string());
    let claims = json!({
        "aud": [client_id.to_string()],
        "jti": token_id,
        "iat": now.timestamp(),
        "nbf": now.timestamp(),
        "exp": exp.timestamp(),
        "sub": sub,
        "scopes": scopes,
    });
    let access_token = encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(&keys.private_pem)
            .map_err(|error| FrameworkError::internal(error.to_string()))?,
    )
    .map_err(|error| FrameworkError::internal(error.to_string()))?;

    let refresh_token = if with_refresh {
        let refresh = random_token();
        OAuthRefreshToken::create(attrs! {
            id: refresh.clone(),
            access_token_id: token_id.clone(),
            revoked: false,
            expires_at: Some(exp),
        })
        .await?;
        Some(refresh)
    } else {
        None
    };

    Ok(Issued {
        access_token,
        refresh_token,
        token_id,
        expires_in: TOKEN_TTL_SECONDS,
    })
}

/// Passport 客户端的响应结构。
pub fn client_json(client: &OAuthClient) -> serde_json::Value {
    json!({
        "id": client.id,
        "user_id": client.user_id,
        "name": client.name,
        "provider": client.provider,
        "redirect": client.redirect,
        "personal_access_client": client.personal_access_client,
        "password_client": client.password_client,
        "revoked": client.revoked,
        "created_at": client.created_at.map(format_datetime),
        "updated_at": client.updated_at.map(format_datetime),
    })
}

pub fn token_json(token: &OAuthAccessToken, client: Option<&OAuthClient>) -> serde_json::Value {
    let mut value = json!({
        "id": token.id,
        "user_id": token.user_id,
        "client_id": token.client_id,
        "name": token.name,
        "scopes": token.scopes.as_deref()
            .map(|raw| raw.split_whitespace().collect::<Vec<_>>())
            .unwrap_or_default(),
        "revoked": token.revoked,
        "created_at": token.created_at.map(format_datetime),
        "updated_at": token.updated_at.map(format_datetime),
        "expires_at": token.expires_at.map(format_datetime),
    });
    if let Some(client) = client
        && let Some(object) = value.as_object_mut()
    {
        object.insert("client".to_string(), client_json(client));
    }
    value
}

pub async fn hash_client_secret(secret: &str) -> Result<String, FrameworkError> {
    hash_password(secret, None).map_err(|error| FrameworkError::internal(error.to_string()))
}

/// 校验 Bearer JWT 并返回用户及其 scope。
pub async fn bearer_identity(token: &str) -> Result<Option<(User, Vec<String>)>, FrameworkError> {
    let Some(claims) = verify_jwt(token) else {
        return Ok(None);
    };
    let Some(jti) = claims.get("jti").and_then(|value| value.as_str()) else {
        return Ok(None);
    };
    let Some(row) = OAuthAccessToken::find(jti.to_string()).await? else {
        return Ok(None);
    };
    if row.revoked {
        return Ok(None);
    }
    if let Some(expires_at) = row.expires_at
        && expires_at <= Utc::now()
    {
        return Ok(None);
    }
    let Some(uid) = row.user_id else {
        return Ok(None);
    };
    let Some(user) = User::find(u32::try_from(uid).unwrap_or(0)).await? else {
        return Ok(None);
    };
    let scopes = claims
        .get("scopes")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .or_else(|| {
            row.scopes
                .as_deref()
                .map(|raw| raw.split_whitespace().map(str::to_string).collect())
        })
        .unwrap_or_default();
    Ok(Some((user, scopes)))
}

/// 用 Passport 兼容的 Bearer JWT 查找当前用户（校验签名、撤销与过期）。
pub async fn user_from_bearer(token: &str) -> Result<Option<User>, FrameworkError> {
    Ok(bearer_identity(token).await?.map(|(user, _)| user))
}
