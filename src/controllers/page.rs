//! 页面渲染（Phase 6）：首页、认证页与错误页。
//!
//! 模板为 MiniJinja 语法，位于 `resources/views`；DOM 结构沿用源 Twig。

use serde_json::{Value, json};
use suprnova::{HttpResponse, Model, Request, Response, handler};

use crate::models::user::User;
use crate::support::auth::current_user;
use crate::support::{login_fails, options, view};

pub(crate) fn frontend_mode() -> String {
    match std::env::var("FRONTEND_MODE") {
        Ok(value) if value.eq_ignore_ascii_case("next") => "next".to_string(),
        _ => "legacy".to_string(),
    }
}

fn csrf_token() -> String {
    suprnova::get_csrf_token().unwrap_or_default()
}

async fn localized(name: &str, locale: &str) -> String {
    if let Ok(Some(value)) = options::get(&format!("{name}_{locale}")).await
        && !value.is_empty()
    {
        return value;
    }
    options::value(name).await.unwrap_or_default()
}

fn locale_name(locale: &str) -> String {
    crate::support::locale::LOCALES
        .iter()
        .find(|(id, _, _)| *id == locale)
        .map(|(_, name, _)| (*name).to_string())
        .unwrap_or_else(|| locale.to_string())
}

pub(crate) fn page_languages(locale: &str, path: &str) -> (String, Value) {
    let langs: Vec<Value> = crate::support::locale::LOCALES
        .iter()
        .map(|(id, name, _)| {
            json!({
                "name": name,
                "url": format!("{path}?lang={id}"),
            })
        })
        .collect();
    (locale_name(locale), Value::Array(langs))
}

pub(crate) async fn page_user_extras(user: &User) -> Value {
    let base = std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost".to_string());
    let base = base.trim_end_matches('/').to_string();
    let avatar = format!("{base}/avatar/{}?size=36", user.avatar);
    let avatar_png = format!("{base}/avatar/{}?size=36&png=true", user.avatar);
    let mut menu = vec![
        json!({ "label": crate::support::i18n::translate("zh_CN", "general.user-center", None), "link": "/user" }),
        json!({ "label": crate::support::i18n::translate("zh_CN", "general.profile", None), "link": "/user/profile" }),
    ];
    if user.is_admin() {
        menu.push(json!({ "label": "", "link": "#divider" }));
        menu.push(json!({ "label": "管理面板", "link": "/admin" }));
        menu.push(json!({ "label": "用户管理", "link": "/admin/users" }));
        menu.push(json!({ "label": "举报管理", "link": "/admin/reports" }));
    }
    json!({
        "avatar": avatar,
        "avatar_png": avatar_png,
        "menu": menu,
    })
}

pub(crate) async fn shell(request: &Request) -> (String, Value) {
    let locale = crate::support::locale::resolve(request);
    let user = current_user(request).await.ok().flatten();
    let token = csrf_token();
    let mut context = view::base_context(&locale, request.path(), &token, user.as_ref()).await;
    let (current, langs) = page_languages(&locale, request.path());

    let mut user_json = Value::Null;
    let mut avatar = Value::Null;
    let mut avatar_png = Value::Null;
    let mut menu = Value::Array(Vec::new());
    if let Some(user) = &user {
        let extras = page_user_extras(user).await;
        user_json = json!({
            "uid": user.uid,
            "email": user.email,
            "nickname": user.nickname,
            "avatar": user.avatar,
            "isAdmin": user.is_admin(),
            "isDarkMode": user.is_dark_mode,
        });
        avatar = extras["avatar"].clone();
        avatar_png = extras["avatar_png"].clone();
        menu = extras["menu"].clone();
    }

    if let Some(object) = context.as_object_mut() {
        object.insert("current".to_string(), Value::String(current));
        object.insert("langs".to_string(), langs);
        object.insert("transparent_navbar".to_string(), Value::Bool(false));
        object.insert("fixed_bg".to_string(), Value::Bool(false));
        object.insert("hide_intro".to_string(), Value::Bool(false));
        object.insert(
            "home_pic_url".to_string(),
            Value::String("./app/bg.webp".to_string()),
        );
        object.insert("avatar".to_string(), avatar);
        object.insert("avatar_png".to_string(), avatar_png);
        object.insert("menu".to_string(), menu);
        if !user_json.is_null() {
            object.insert("user".to_string(), user_json);
        }
    }
    (locale, context)
}

/// GET / —— 首页。
#[handler]
pub async fn home(request: Request) -> Response {
    if frontend_mode() == "next" {
        return Ok(HttpResponse::text("")
            .status(302)
            .header("Location", "/app-next/site/"));
    }
    let (locale, mut context) = shell(&request).await;
    let description = localized("site_description", &locale).await;
    let home_pic_url = options::value("home_pic_url")
        .await
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "./app/bg.webp".to_string());
    if let Some(object) = context.as_object_mut() {
        object.insert("site_description".to_string(), Value::String(description));
        object.insert(
            "transparent_navbar".to_string(),
            Value::Bool(
                options::bool_or("transparent_navbar", false)
                    .await
                    .unwrap_or(false),
            ),
        );
        object.insert(
            "fixed_bg".to_string(),
            Value::Bool(options::bool_or("fixed_bg", false).await.unwrap_or(false)),
        );
        object.insert(
            "hide_intro".to_string(),
            Value::Bool(options::bool_or("hide_intro", false).await.unwrap_or(false)),
        );
        object.insert("home_pic_url".to_string(), Value::String(home_pic_url));
    }
    Ok(view::render("home.html", context))
}

async fn auth_context(request: &Request, extra: Value) -> Value {
    let (_locale, mut context) = shell(request).await;
    let flash = suprnova::session_mut(|data| data.pull::<String>("msg"))
        .flatten()
        .map(Value::String);
    if let Some(object) = context.as_object_mut() {
        object.insert("extra".to_string(), extra);
        object.insert("flash".to_string(), flash.unwrap_or(Value::Null));
        object.insert("errors".to_string(), Value::Null);
    }
    context
}

/// GET /auth/login
#[handler]
pub async fn login_page(request: Request) -> Response {
    if let Some(redirect) = request.query_param("redirect")
        && redirect.starts_with('/')
        && !redirect.starts_with("//")
    {
        suprnova::session_mut(|data| data.put("last_requested_path", redirect));
    }
    let ip = request.ip().unwrap_or_else(|| "unknown".to_string());
    let too_many_fails = login_fails::count(&ip).await > 3;
    let recaptcha = options::value("recaptcha_sitekey")
        .await
        .unwrap_or_default();
    let invisible = options::bool_or("recaptcha_invisible", false)
        .await
        .unwrap_or(false);
    let extra = json!({
        "tooManyFails": too_many_fails,
        "recaptcha": recaptcha,
        "invisible": invisible,
    });
    let context = auth_context(&request, extra).await;
    Ok(view::render("auth/login.html", context))
}

/// GET /auth/register
#[handler]
pub async fn register_page(request: Request) -> Response {
    let recaptcha = options::value("recaptcha_sitekey")
        .await
        .unwrap_or_default();
    let invisible = options::bool_or("recaptcha_invisible", false)
        .await
        .unwrap_or(false);
    let extra = json!({
        "player": options::bool_or("register_with_player_name", true).await.unwrap_or(true),
        "recaptcha": recaptcha,
        "invisible": invisible,
    });
    let context = auth_context(&request, extra).await;
    Ok(view::render("auth/register.html", context))
}

/// GET /auth/forgot
#[handler]
pub async fn forgot_page(request: Request) -> Response {
    if !crate::support::mail::enabled() {
        let locale_code = crate::support::locale::resolve(&request);
        return Ok(error_page(
            &request,
            403,
            crate::support::i18n::translate(&locale_code, "auth.forgot.disabled", None),
        )
        .await);
    }
    let recaptcha = options::value("recaptcha_sitekey")
        .await
        .unwrap_or_default();
    let invisible = options::bool_or("recaptcha_invisible", false)
        .await
        .unwrap_or(false);
    let extra = json!({
        "recaptcha": recaptcha,
        "invisible": invisible,
    });
    let context = auth_context(&request, extra).await;
    Ok(view::render("auth/forgot.html", context))
}

/// GET /auth/reset/{uid}
#[handler]
pub async fn reset_page(request: Request) -> Response {
    let locale_code = crate::support::locale::resolve(&request);
    if !crate::support::signed_url::verify(request.path(), request.query()) {
        return Ok(error_page(
            &request,
            403,
            crate::support::i18n::translate(&locale_code, "auth.reset.invalid", None),
        )
        .await);
    }
    let Some(uid) = request
        .param("uid")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(user) = User::find(uid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let (_locale, mut context) = shell(&request).await;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "user".to_string(),
            json!({ "nickname": user.nickname, "email": user.email }),
        );
    }
    Ok(view::render("auth/reset.html", context))
}

/// GET /auth/bind
#[handler]
pub async fn bind_page(request: Request) -> Response {
    let Some(_user) = current_user(&request).await? else {
        return Ok(HttpResponse::text("")
            .status(302)
            .header("Location", "/auth/login"));
    };
    let context = auth_context(&request, json!({})).await;
    Ok(view::render("auth/bind.html", context))
}

/// GET /auth/verify/{user}
#[handler]
pub async fn verify_page(request: Request) -> Response {
    let locale_code = crate::support::locale::resolve(&request);
    if !options::bool_or("require_verification", false)
        .await
        .unwrap_or(false)
    {
        return Ok(error_page(
            &request,
            403,
            crate::support::i18n::translate(&locale_code, "user.verification.disabled", None),
        )
        .await);
    }
    if !crate::support::signed_url::verify(request.path(), request.query()) {
        return Ok(error_page(
            &request,
            403,
            crate::support::i18n::translate(&locale_code, "auth.verify.invalid", None),
        )
        .await);
    }
    let Some(uid) = request
        .param("user")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if User::find(uid).await?.is_none() {
        return Ok(HttpResponse::text("Not Found").status(404));
    }
    let flash = suprnova::session_mut(|data| data.pull::<String>("errorMessage"))
        .flatten()
        .unwrap_or_default();
    let mut context = auth_context(&request, json!({})).await;
    if let Some(object) = context.as_object_mut() {
        object.insert("flash".to_string(), Value::String(flash));
    }
    Ok(view::render("auth/verify.html", context))
}

/// 渲染错误页（用于带消息的 403）。
async fn error_page(request: &Request, status: u16, message: String) -> HttpResponse {
    let (_locale, mut context) = shell(request).await;
    if let Some(object) = context.as_object_mut() {
        object.insert("message".to_string(), Value::String(message));
    }
    view::render_status("errors/403.html", context, status)
}

/// 渲染 404 页面（静态资源缺失时使用）。
pub async fn not_found(request: &Request) -> Response {
    let (_locale, mut context) = shell(request).await;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "message".to_string(),
            Value::String("请求的页面不存在。".to_string()),
        );
    }
    Ok(view::render_status("errors/404.html", context, 404))
}

/// 403 页面。
pub async fn forbidden(request: &Request) -> Response {
    let (_locale, mut context) = shell(request).await;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "message".to_string(),
            Value::String("403 Forbidden".to_string()),
        );
    }
    Ok(view::render_status("errors/403.html", context, 403))
}

/// POST /auth/bind —— 绑定邮箱（PHP `AuthController@fillEmail`）。
#[handler]
pub async fn fill_email(request: Request) -> Response {
    let Some(user) = current_user(&request).await? else {
        return Ok(HttpResponse::text("")
            .status(302)
            .header("Location", "/auth/login"));
    };
    #[derive(serde::Deserialize, Default)]
    struct BindRequest {
        #[serde(default)]
        email: String,
    }
    let mut context = auth_context(&request, json!({})).await;
    let body: BindRequest = request.input().await?;
    let email = body.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        if let Some(object) = context.as_object_mut() {
            object.insert("errors".to_string(), json!("请输入有效的邮箱地址。"));
        }
        return Ok(view::render("auth/bind.html", context));
    }
    if User::filter("email", email.clone())
        .first()
        .await?
        .is_some()
    {
        if let Some(object) = context.as_object_mut() {
            object.insert("errors".to_string(), json!("此邮箱已被占用"));
        }
        return Ok(view::render("auth/bind.html", context));
    }
    user.update(suprnova::attrs! { email: email }).await?;
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/user"))
}
