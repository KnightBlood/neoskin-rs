//! 认证控制器（Phase 2：登录/登出/当前用户/记住我/验证码）。
//!
//! 凭证规则与 PHP `AuthController` 对齐：`identification` 可为邮箱或角色名，
//! 密码 bcrypt（支持 `$2y$`）；同一 IP 失败超过 3 次后要求验证码。

use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use suprnova::session::{invalidate_session, regenerate_session_id, session_mut};
use suprnova::{
    Cache, FrameworkError, HttpResponse, Model, Request, Response, ResponseExt, SessionConfig,
    ValidationErrors, attrs, handler, json_response,
};

use crate::controllers::player::validate_name;
use crate::models::player::Player;
use crate::models::user::{PERMISSION_NORMAL, User};
use crate::support::auth::{SESSION_UID_KEY, api_user};
use crate::support::hash::{hash_password, verify_password};
use crate::support::remember;
use crate::support::signed_url;
use crate::support::{captcha, i18n, locale, login_fails, mail, options};

#[derive(Deserialize, Debug)]
pub struct LoginRequest {
    #[serde(default)]
    pub identification: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub captcha: Option<String>,
    #[serde(default, alias = "remember")]
    pub keep: bool,
}

fn validation_error(field: &str, message: &str) -> FrameworkError {
    let mut errors = ValidationErrors::new();
    errors.add(field, message.to_string());
    FrameworkError::Validation(errors)
}

/// 确保用户有 remember_token（没有则生成并落库），返回 token。
async fn ensure_remember_token(user: &User) -> Result<String, FrameworkError> {
    if let Some(token) = user
        .remember_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return Ok(token.to_string());
    }
    let token = remember::random_token();
    remember::store_token(user.uid, &token).await?;
    Ok(token)
}

fn client_ip(request: &Request) -> String {
    let ip = request.ip().unwrap_or_else(|| "unknown".to_string());
    match crate::support::plugin_api::apply_filter("client_ip", Value::String(ip.clone())) {
        Value::String(filtered) => filtered,
        _ => ip,
    }
}

#[handler]
pub async fn login(request: Request) -> Response {
    let ip = client_ip(&request);
    let body: LoginRequest = request.input().await?;
    let identification = body.identification.trim();
    if identification.is_empty() {
        return Err(validation_error("identification", "邮箱或角色名不能为空。").into());
    }
    let password_len = body.password.chars().count();
    if password_len == 0 {
        return Err(validation_error("password", "密码不能为空。").into());
    }
    if !(6..=32).contains(&password_len) {
        return Err(validation_error("password", "密码 必须介于 6 - 32 个字符之间。").into());
    }

    let can = crate::support::plugin_api::apply_filter("can_login", Value::Null);
    if !can.is_null() {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": can.as_str().unwrap_or("登录被插件拒绝"),
        })));
    }

    // PHP：同一 IP 失败超过 3 次后要求验证码。
    if login_fails::count(&ip).await > 3 {
        let input = body.captcha.clone().unwrap_or_default();
        if !captcha::verify(&input).await {
            let secret = options::get("recaptcha_secretkey").await.ok().flatten();
            let message = if captcha::is_recaptcha_enabled(secret.as_deref()) {
                "未能通过 reCAPTCHA 的验证。"
            } else {
                "验证码不正确。"
            };
            return Err(validation_error("captcha", message).into());
        }
    }

    let user = if identification.contains('@') {
        User::filter("email", identification).first().await?
    } else {
        let player = Player::filter("name", identification).first().await?;
        match player {
            Some(player) => User::find(player.uid as u32).await?,
            None => None,
        }
    };

    let Some(user) = user else {
        return json_response!({
            "code": 2,
            "message": "用户不存在。",
            "data": { "reason": "user" }
        });
    };
    if user.is_banned() {
        return json_response!({
            "code": 3,
            "message": "该账户已被封禁。",
        });
    }
    crate::support::plugin_api::fire(
        "UserTryToLogin",
        serde_json::json!({ "identification": identification, "uid": user.uid }),
    );
    let passed = verify_password(&user.password, &body.password);
    let passed = crate::support::plugin_api::apply_filter("verify_password", Value::Bool(passed))
        .as_bool()
        .unwrap_or(passed);
    if !passed {
        login_fails::record(&ip).await;
        return json_response!({
            "code": 1,
            "message": "密码错误。",
            "data": { "reason": "password" }
        });
    }
    login_fails::clear(&ip).await;

    // 防 session fixation：登录成功后轮换 session id。
    regenerate_session_id();
    session_mut(|data| {
        data.put(SESSION_UID_KEY, user.uid);
    });

    let redirect_to = session_mut(|data| data.pull::<String>("last_requested_path"))
        .flatten()
        .filter(|path| path.starts_with('/'))
        .unwrap_or_else(|| "/user".to_string());

    let mut response = json_response!({
        "code": 0,
        "message": "登录成功，欢迎回来",
        "data": {
            "redirectTo": redirect_to,
            "uid": user.uid,
        }
    });
    crate::support::plugin_api::fire(
        "UserLoggedIn",
        serde_json::json!({ "uid": user.uid, "ip": ip }),
    );

    if body.keep {
        let token = ensure_remember_token(&user).await?;
        if let Ok(value) = remember::cookie_value(&user, &token) {
            let secure = SessionConfig::from_env().cookie_secure;
            response = response.cookie(remember::cookie(value, secure));
        }
    }

    response
}

#[handler]
pub async fn captcha_image(_request: Request) -> Response {
    let phrase = captcha::issue();
    Ok(
        HttpResponse::bytes_body(captcha::svg(&phrase).into_bytes(), "image/svg+xml")
            .header("Cache-Control", "no-store, no-cache, must-revalidate")
            .header("Pragma", "no-cache"),
    )
}

#[handler]
pub async fn logout(request: Request) -> Response {
    // 有 remember cookie 时轮换 token，使所有已记住设备失效。
    if let Some(raw) = request.cookie(remember::REMEMBER_COOKIE)
        && let Some((uid, token, mac)) = remember::parse_cookie(&raw)
        && let Ok(Some(user)) = User::find(uid).await
        && remember::verify(&user, token, mac)
    {
        let _ = remember::store_token(uid, &remember::random_token()).await;
    }

    invalidate_session();
    json_response!({
        "code": 0,
        "message": "已登出。",
    })
    .cookie(remember::forget_cookie())
}

#[handler]
pub async fn me(request: Request) -> Response {
    if let Err(response) = crate::support::api::require_scopes(&request, &["User.Read"]).await {
        return Ok(response);
    }
    match api_user(&request).await? {
        Some(user) => json_response!({
            "code": 0,
            "message": "",
            "data": {
                "uid": user.uid,
                "email": user.email,
                "nickname": user.nickname,
                "avatar": format!("/avatar/{}", user.avatar),
                "score": user.score,
                "permission": user.permission,
                "verified": user.verified,
                "isAdmin": user.is_admin(),
                "isBanned": user.is_banned(),
                "darkMode": user.is_dark_mode,
            }
        }),
        None => json_response!({
            "code": 1,
            "message": "Unauthenticated.",
        })
        .status(401),
    }
}

#[derive(Deserialize, Default)]
pub struct RegisterRequest {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub captcha: String,
    #[serde(default)]
    pub player_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
}

fn is_email(value: &str) -> bool {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN
        .get_or_init(|| regex::Regex::new(r"^[^@\s]+@[^@\s]+\.[^@\s]+$").expect("email regex"))
        .is_match(value)
}

/// POST /auth/register
#[handler]
pub async fn register(request: Request) -> Response {
    let ip = client_ip(&request);
    let body: RegisterRequest = request.input().await?;
    let can = crate::support::plugin_api::apply_filter("can_register", Value::Null);
    if !can.is_null() {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": can.as_str().unwrap_or("注册被插件拒绝"),
        })));
    }
    let register_with_player = options::bool_or("register_with_player_name", true).await?;
    let min = options::int_or("player_name_length_min", 3).await?;
    let max = options::int_or("player_name_length_max", 16).await?;

    let email = body.email.trim().to_string();
    if email.is_empty() {
        return Err(validation_error("email", "邮箱 不能为空。").into());
    }
    if !is_email(&email) {
        return Err(validation_error("email", "邮箱 格式不正确。").into());
    }
    if User::filter("email", email.clone())
        .first()
        .await?
        .is_some()
    {
        return Err(validation_error("email", "邮箱 已经被占用。").into());
    }

    let password_len = body.password.chars().count();
    if password_len == 0 {
        return Err(validation_error("password", "密码 不能为空。").into());
    }
    if !(8..=32).contains(&password_len) {
        return Err(validation_error("password", "密码 必须介于 8 - 32 个字符之间。").into());
    }

    if body.captcha.trim().is_empty() {
        return Err(validation_error("captcha", "验证码 不能为空。").into());
    }
    if !captcha::verify(&body.captcha).await {
        let secret = options::get("recaptcha_secretkey").await.ok().flatten();
        let message = if captcha::is_recaptcha_enabled(secret.as_deref()) {
            "未能通过 reCAPTCHA 的验证。"
        } else {
            "验证码不正确。"
        };
        return Err(validation_error("captcha", message).into());
    }

    let display_name = if register_with_player {
        body.player_name
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    } else {
        body.nickname
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };

    if register_with_player {
        if let Some(message) = validate_name(&display_name).await? {
            return Err(validation_error("player_name", &message).into());
        }
        let length = display_name.chars().count() as i64;
        if length < min {
            return Err(
                validation_error("player_name", &format!("角色名 至少为 {min} 个字符。")).into(),
            );
        }
        if length > max {
            return Err(validation_error(
                "player_name",
                &format!("角色名 不能大于 {max} 个字符。"),
            )
            .into());
        }
        if Player::filter("name", display_name.clone())
            .first()
            .await?
            .is_some()
        {
            return json_response!({
                "code": 1,
                "message": "该角色名已被占用",
            });
        }
    } else {
        if display_name.is_empty() {
            return Err(validation_error("nickname", "昵称 不能为空。").into());
        }
        if display_name.chars().count() > 255 {
            return Err(validation_error("nickname", "昵称 不能大于 255 个字符。").into());
        }
    }

    let regs_per_ip = options::int_or("regs_per_ip", 3).await?;
    let registered = User::filter("ip", ip.clone()).get().await?.len() as i64;
    if regs_per_ip > 0 && registered >= regs_per_ip {
        return json_response!({
            "code": 1,
            "message": format!("你在本站注册的账号已达到上限 {regs_per_ip} 个，无法继续注册"),
        });
    }

    let initial_score = options::int_or("user_initial_score", 1000).await?;
    let password_hash = hash_password(&body.password, None)
        .map_err(|error| FrameworkError::internal(format!("密码哈希失败: {error}")))?;
    let password_hash = match crate::support::plugin_api::apply_filter(
        "user_password",
        Value::String(password_hash),
    ) {
        Value::String(filtered) => filtered,
        _ => {
            return Err(FrameworkError::internal("插件返回了非法的密码哈希").into());
        }
    };
    let now = Utc::now();
    let user = User::create(attrs! {
        email: email.clone(),
        nickname: display_name.clone(),
        score: initial_score as i32,
        avatar: 0,
        password: password_hash,
        ip: ip.clone(),
        permission: PERMISSION_NORMAL,
        last_sign_at: now - Duration::days(1),
        register_at: now,
        verified: false,
        verification_token: "",
    })
    .await?;

    if register_with_player {
        Player::create(attrs! {
            uid: user.uid,
            name: display_name.clone(),
            tid_skin: 0,
            tid_cape: 0,
            last_modified: Utc::now(),
        })
        .await?;
    }
    crate::support::plugin_api::fire(
        "UserRegistered",
        serde_json::json!({ "uid": user.uid, "email": user.email.clone(), "nickname": user.nickname.clone() }),
    );

    regenerate_session_id();
    session_mut(|data| {
        data.put(SESSION_UID_KEY, user.uid);
    });

    json_response!({
        "code": 0,
        "message": "注册成功，正在跳转...",
    })
}

fn base_url() -> String {
    let site_url = std::env::var("SITE_URL").unwrap_or_default();
    if !site_url.trim().is_empty() {
        return site_url.trim_end_matches('/').to_string();
    }
    std::env::var("APP_URL")
        .unwrap_or_else(|_| "http://localhost".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn mail_params(url: &str, site_name: &str) -> Map<String, Value> {
    let mut params = Map::new();
    params.insert("url".to_string(), Value::String(url.to_string()));
    params.insert("sitename".to_string(), Value::String(site_name.to_string()));
    params
}

async fn site_name(locale_code: &str) -> String {
    options::localized(locale_code, "site_name")
        .await
        .unwrap_or_else(|_| "NeoSkin".to_string())
}

/// 缓存中的「上次发信时间」距现在不足 `interval` 秒时返回剩余秒数。
async fn mail_remaining(key: &str, interval: i64) -> i64 {
    let now = Utc::now().timestamp();
    let last = Cache::get::<i64>(key).await.ok().flatten().unwrap_or(0);
    interval - (now - last)
}

async fn remember_mail_time(key: &str) {
    let now = Utc::now().timestamp();
    let _ = Cache::put(key, &now, Some(std::time::Duration::from_secs(3600))).await;
}

#[derive(Deserialize, Default)]
pub struct ForgotRequest {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub captcha: Option<String>,
}

/// POST /auth/forgot —— 发送重置密码邮件（PHP `handleForgot`）。
#[handler]
pub async fn forgot(request: Request) -> Response {
    let locale_code = locale::resolve(&request);
    let ip = client_ip(&request);
    let body: ForgotRequest = request.input().await?;
    if !mail::enabled() {
        return json_response!({
            "code": 1,
            "message": i18n::translate(&locale_code, "auth.forgot.disabled", None),
        });
    }
    let input = body.captcha.clone().unwrap_or_default();
    if !captcha::verify(&input).await {
        return json_response!({
            "code": 1,
            "message": "验证码错误",
        });
    }
    let email = body.email.trim().to_string();
    if email.is_empty() || !is_email(&email) {
        return Err(validation_error("email", "请输入有效的邮箱地址。").into());
    }

    let rate_key = format!("last_mail:{ip}");
    let remain = mail_remaining(&rate_key, 180).await;
    if remain > 0 {
        return json_response!({
            "code": 2,
            "message": i18n::translate(&locale_code, "auth.forgot.frequent-mail", None),
        });
    }

    let Some(user) = User::filter("email", email.clone()).first().await? else {
        return json_response!({
            "code": 1,
            "message": i18n::translate(&locale_code, "auth.forgot.unregistered", None),
        });
    };

    let expires = Utc::now().timestamp() + 3600;
    let Some(path) = signed_url::sign_path(&format!("/auth/reset/{}", user.uid), Some(expires))
    else {
        return json_response!({
            "code": 2,
            "message": i18n::translate(&locale_code, "auth.forgot.failed", Some(&mail_params("", "APP_KEY"))),
        });
    };
    let url = format!("{}{}", base_url(), path);
    let name = site_name(&locale_code).await;
    let params = mail_params(&url, &name);
    let subject = i18n::translate(&locale_code, "auth.forgot.mail.title", Some(&params));
    let html = format!(
        "<p>{}</p><p>{}</p><p>{}</p>",
        i18n::translate(&locale_code, "auth.forgot.mail.message", Some(&params)),
        i18n::translate(&locale_code, "auth.forgot.mail.reset", Some(&params)),
        i18n::translate(&locale_code, "auth.forgot.mail.ignore", Some(&params)),
    );
    if let Err(error) = mail::send_html(&email, &subject, html).await {
        let mut params = mail_params(&url, &name);
        params.insert("msg".to_string(), Value::String(error.to_string()));
        return json_response!({
            "code": 2,
            "message": i18n::translate(&locale_code, "auth.forgot.failed", Some(&params)),
        });
    }
    remember_mail_time(&rate_key).await;
    json_response!({
        "code": 0,
        "message": i18n::translate(&locale_code, "auth.forgot.success", None),
    })
}

#[derive(Deserialize, Default)]
pub struct ResetRequest {
    #[serde(default)]
    pub password: String,
}

/// POST /auth/reset/{uid} —— 校验签名并重置密码（PHP `handleReset`）。
#[handler]
pub async fn reset(request: Request) -> Response {
    let locale_code = locale::resolve(&request);
    if !signed_url::verify(request.path(), request.query()) {
        return Ok(
            HttpResponse::text(i18n::translate(&locale_code, "auth.reset.invalid", None))
                .status(403),
        );
    }
    let Some(uid) = request
        .param("uid")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let body: ResetRequest = request.input().await?;
    let length = body.password.chars().count();
    if !(8..=32).contains(&length) {
        return Err(validation_error("password", "密码长度必须为 8–32 位。").into());
    }
    let Some(user) = User::find(uid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let password_hash = hash_password(&body.password, None)
        .map_err(|error| FrameworkError::internal(format!("密码哈希失败: {error}")))?;
    user.update(attrs! { password: password_hash }).await?;
    json_response!({
        "code": 0,
        "message": i18n::translate(&locale_code, "auth.reset.success", None),
    })
}

#[derive(Deserialize, Default)]
pub struct VerifyRequest {
    #[serde(default)]
    pub email: String,
}

/// POST /auth/verify/{user} —— 校验签名并完成邮箱验证（PHP `handleVerify`）。
#[handler]
pub async fn verify(request: Request) -> Response {
    let locale_code = locale::resolve(&request);
    if !signed_url::verify(request.path(), request.query()) {
        return Ok(
            HttpResponse::text(i18n::translate(&locale_code, "auth.verify.invalid", None))
                .status(403),
        );
    }
    let Some(uid) = request
        .param("user")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let path = request.path().to_string();
    let query = request.query().map(str::to_string);
    let body: VerifyRequest = request.input().await?;
    let Some(user) = User::find(uid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if user.email != body.email.trim() {
        let message = i18n::translate(&locale_code, "auth.verify.not-matched", None);
        let _ = session_mut(|data| data.flash("errorMessage", message));
        let target = match query {
            Some(query) => format!("{path}?{query}"),
            None => path,
        };
        return Ok(HttpResponse::text("")
            .status(302)
            .header("Location", target));
    }
    user.update(attrs! { verified: true }).await?;
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/user"))
}

/// POST /user/email-verification —— 重发验证邮件（PHP `sendVerificationEmail`）。
#[handler]
pub async fn send_verification_email(request: Request) -> Response {
    let locale_code = locale::resolve(&request);
    if !options::bool_or("require_verification", false)
        .await
        .unwrap_or(false)
    {
        return json_response!({
            "code": 1,
            "message": i18n::translate(&locale_code, "user.verification.disabled", None),
        });
    }
    let Some(user) = crate::support::auth::current_user(&request).await? else {
        return Ok(HttpResponse::text("")
            .status(302)
            .header("Location", "/auth/login"));
    };
    let rate_key = format!("last_mail_time:{}", user.uid);
    let remain = mail_remaining(&rate_key, 60).await;
    if remain > 0 {
        return json_response!({
            "code": 1,
            "message": i18n::translate(&locale_code, "user.verification.frequent-mail", None),
        });
    }
    if user.verified {
        return json_response!({
            "code": 1,
            "message": i18n::translate(&locale_code, "user.verification.verified", None),
        });
    }
    let Some(path) = signed_url::sign_path(&format!("/auth/verify/{}", user.uid), None) else {
        return json_response!({
            "code": 2,
            "message": "APP_KEY 未配置，无法生成验证链接。",
        });
    };
    let url = format!("{}{}", base_url(), path);
    let name = site_name(&locale_code).await;
    let params = mail_params(&url, &name);
    let subject = i18n::translate(&locale_code, "user.verification.mail.title", Some(&params));
    let html = format!(
        "<p>{}</p><p>{}</p><p>{}</p>",
        i18n::translate(
            &locale_code,
            "user.verification.mail.message",
            Some(&params)
        ),
        i18n::translate(&locale_code, "user.verification.mail.reset", Some(&params)),
        i18n::translate(&locale_code, "user.verification.mail.ignore", Some(&params)),
    );
    if let Err(error) = mail::send_html(&user.email, &subject, html).await {
        let mut params = mail_params(&url, &name);
        params.insert("msg".to_string(), Value::String(error.to_string()));
        return json_response!({
            "code": 2,
            "message": i18n::translate(&locale_code, "user.verification.failed", Some(&params)),
        });
    }
    remember_mail_time(&rate_key).await;
    json_response!({
        "code": 0,
        "message": i18n::translate(&locale_code, "user.verification.success", None),
    })
}
