//! 认证辅助：Session 当前用户 + 记住我回退。

use suprnova::session::{session, session_mut};
use suprnova::{FrameworkError, Model, Request};

use crate::models::user::User;
use crate::support::oauth;
use crate::support::remember;

pub const SESSION_UID_KEY: &str = "neoskin_uid";

/// 认证身份：Session/记住我 或 Bearer JWT。
pub struct Identity {
    pub user: User,
    /// `None` 表示 Session 登录（不受 OAuth scope 限制）。
    pub scopes: Option<Vec<String>>,
}

/// 解析当前身份：Session/记住我优先，其次 Bearer。
pub async fn identity(request: &Request) -> Result<Option<Identity>, FrameworkError> {
    if let Some(user) = current_user(request).await? {
        return Ok(Some(Identity { user, scopes: None }));
    }
    let Some(header) = request.header("authorization") else {
        return Ok(None);
    };
    let Some(token) = header.strip_prefix("Bearer ") else {
        return Ok(None);
    };
    if token.trim().is_empty() {
        return Ok(None);
    }
    Ok(oauth::bearer_identity(token.trim())
        .await?
        .map(|(user, scopes)| Identity {
            user,
            scopes: Some(scopes),
        }))
}

/// 当前登录用户；未登录返回 `None`。
///
/// 先读 Session；Session 不存在时尝试 remember cookie（校验 token + HMAC），
/// 命中后重建 Session 登录态。
pub async fn current_user(request: &Request) -> Result<Option<User>, FrameworkError> {
    if let Some(uid) = session().and_then(|data| data.get::<u32>(SESSION_UID_KEY))
        && let Some(user) = User::find(uid).await?
    {
        return Ok(Some(user));
    }

    let Some(raw) = request.cookie(remember::REMEMBER_COOKIE) else {
        return Ok(None);
    };
    let Some((uid, token, mac)) = remember::parse_cookie(&raw) else {
        return Ok(None);
    };
    let Some(user) = User::find(uid).await? else {
        return Ok(None);
    };
    if !remember::verify(&user, token, mac) {
        return Ok(None);
    }
    let user_uid = user.uid;
    session_mut(|data| data.put(SESSION_UID_KEY, user_uid));
    Ok(Some(user))
}

/// API 用户：Session/记住我优先，其次 `Authorization: Bearer <JWT>`。
pub async fn api_user(request: &Request) -> Result<Option<User>, FrameworkError> {
    if let Some(user) = current_user(request).await? {
        return Ok(Some(user));
    }
    if let Some(header) = request.header("authorization")
        && let Some(token) = header.strip_prefix("Bearer ")
        && !token.trim().is_empty()
    {
        return oauth::user_from_bearer(token.trim()).await;
    }
    Ok(None)
}
