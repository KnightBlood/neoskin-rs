//! OAuth2 / Passport 兼容的 5 张表模型。

use chrono::{DateTime, Utc};
use suprnova::model;

#[model(
    table = "oauth_auth_codes",
    primary_key = "id",
    key_type = "String",
    auto_increment = false,
    fillable = ["id", "user_id", "client_id", "scopes", "revoked", "expires_at"]
)]
pub struct OAuthAuthCode {
    pub id: String,
    pub user_id: u64,
    pub client_id: u64,
    pub scopes: Option<String>,
    pub revoked: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

#[model(
    table = "oauth_access_tokens",
    primary_key = "id",
    key_type = "String",
    auto_increment = false,
    fillable = ["id", "user_id", "client_id", "name", "scopes", "revoked", "created_at", "updated_at", "expires_at"]
)]
pub struct OAuthAccessToken {
    pub id: String,
    pub user_id: Option<u64>,
    pub client_id: u64,
    pub name: Option<String>,
    pub scopes: Option<String>,
    pub revoked: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[model(
    table = "oauth_refresh_tokens",
    primary_key = "id",
    key_type = "String",
    auto_increment = false,
    fillable = ["id", "access_token_id", "revoked", "expires_at"]
)]
pub struct OAuthRefreshToken {
    pub id: String,
    pub access_token_id: String,
    pub revoked: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

#[model(table = "oauth_clients", primary_key = "id", key_type = "u64")]
pub struct OAuthClient {
    pub id: u64,
    pub user_id: Option<u64>,
    pub name: String,
    pub secret: Option<String>,
    pub provider: Option<String>,
    pub redirect: String,
    pub personal_access_client: bool,
    pub password_client: bool,
    pub revoked: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[model(
    table = "oauth_personal_access_clients",
    primary_key = "id",
    key_type = "u64"
)]
pub struct OAuthPersonalAccessClient {
    pub id: u64,
    pub client_id: u64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}
