//! Web/API 通用响应与鉴权辅助。
//!
//! PHP 端 `json($message, $code, $data)` 的 Rust 等价物，以及认证守卫。

use serde_json::{Value, json};
use suprnova::{FrameworkError, HttpResponse, Request};

use crate::models::user::User;

/// 当前登录用户；`/api/*` 支持 Bearer，其余路由仅 Session/记住我。
pub async fn require_user(request: &Request) -> Result<User, HttpResponse> {
    let identity = if request.path().starts_with("/api") {
        crate::support::auth::identity(request).await
    } else {
        crate::support::auth::current_user(request)
            .await
            .map(|user| user.map(|user| crate::support::auth::Identity { user, scopes: None }))
    };
    match identity {
        Ok(Some(identity)) => Ok(identity.user),
        Ok(None) => Err(HttpResponse::json(json!({ "message": "Unauthenticated." })).status(401)),
        Err(error) => Err(error.into()),
    }
}

/// `/api/*` 的 scope 校验；Session 登录不受限制。
pub async fn require_scopes(request: &Request, required: &[&str]) -> Result<(), HttpResponse> {
    if !request.path().starts_with("/api") {
        return Ok(());
    }
    let identity = crate::support::auth::identity(request)
        .await
        .map_err(HttpResponse::from)?;
    let Some(identity) = identity else {
        return Err(HttpResponse::json(json!({ "message": "Unauthenticated." })).status(401));
    };
    if let Some(scopes) = identity.scopes
        && !required
            .iter()
            .any(|needed| scopes.iter().any(|scope| scope == needed))
    {
        return Err(
            HttpResponse::json(json!({ "message": "Invalid scope(s) provided." })).status(403),
        );
    }
    Ok(())
}

/// PHP `verified` 中间件：`require_verification` 打开且用户尚未验证时返回 403。
///
/// <p>只用于 `/user/*`、`/texture*`、`/skinlib/*` 等 web 路由；Laravel 的 `/api/*`
/// 不挂该中间件，因此调用方需按路径判断。</p>
pub async fn require_verified(user: &User, request: &Request) -> Option<HttpResponse> {
    if user.verified {
        return None;
    }
    if !crate::support::options::bool_or("require_verification", false)
        .await
        .unwrap_or(false)
    {
        return None;
    }
    Some(
        HttpResponse::json(json!({
            "message": crate::support::i18n::translate(
                &crate::support::locale::resolve(request),
                "auth.check.verified",
                None,
            ),
        }))
        .status(403),
    )
}

/// `json($message, 0, $data)` / `json($message, 0)`。
pub fn success(message: impl Into<String>, data: Option<Value>) -> HttpResponse {
    let mut body = json!({ "code": 0, "message": message.into() });
    if let Some(data) = data
        && let Some(object) = body.as_object_mut()
    {
        object.insert("data".to_string(), data);
    }
    HttpResponse::json(body)
}

/// `json($message, $code)`。
pub fn failure(message: impl Into<String>, code: i64) -> HttpResponse {
    HttpResponse::json(json!({ "code": code, "message": message.into() }))
}

pub fn not_found() -> HttpResponse {
    HttpResponse::text("Not Found").status(404)
}

pub fn no_content() -> HttpResponse {
    HttpResponse::text("").status(204)
}

/// 把字段错误包装成 Laravel 风格的 422（`message` + `errors`）。
pub fn validation_error(field: &str, message: &str) -> FrameworkError {
    let mut errors = suprnova::ValidationErrors::new();
    errors.add(field, message.to_string());
    FrameworkError::Validation(errors)
}
