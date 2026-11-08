//! 安装向导（PHP `SetupController` + `setup.wizard.*` 视图）。

use serde::Deserialize;
use serde_json::{Value, json};
use suprnova::{FrameworkError, HttpResponse, Model, Request, Response, attrs, handler};

use crate::models::user::{PERMISSION_SUPER, User};
use crate::support::hash::hash_password;
use crate::support::{install, locale, options, view};

fn insert(context: &mut Value, key: &str, value: Value) {
    if let Some(object) = context.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

async fn context(request: &Request) -> Value {
    let locale_code = locale::resolve(request);
    let token = suprnova::get_csrf_token().unwrap_or_default();
    let mut context = view::base_context(&locale_code, request.path(), &token, None).await;
    let (current, langs) = super::page::page_languages(&locale_code, request.path());
    if let Some(object) = context.as_object_mut() {
        object.insert("lang_name".to_string(), Value::String(current));
        object.insert("langs".to_string(), langs);
        object.insert("styles".to_string(), json!([]));
        object.insert("scripts".to_string(), json!([]));
    }
    context
}

fn locked_page() -> Response {
    Ok(view::render(
        "setup/locked.html",
        json!({
            "locale": "zh-CN",
            "site_name": "NeoSkin",
            "csrf_token": suprnova::get_csrf_token().unwrap_or_default(),
            "langs": [],
            "lang_name": "",
            "styles": [],
            "scripts": [],
        }),
    ))
}

/// `GET /setup` —— 欢迎页。
#[handler]
pub async fn welcome(request: Request) -> Response {
    if install::is_locked() {
        return locked_page();
    }
    let mut context = context(&request).await;
    insert(
        &mut context,
        "version",
        Value::String(crate::controllers::home::VERSION.to_string()),
    );
    Ok(view::render("setup/wizard/welcome.html", context))
}

/// `GET|POST /setup/database`
#[handler]
pub async fn database(request: Request) -> Response {
    if install::is_locked() {
        return locked_page();
    }
    if request.method().as_str() == "GET" {
        // 数据库已配置（应用能启动即说明可连接）→ 进入下一步。
        if suprnova::DB::connection().is_ok() {
            return Ok(HttpResponse::text("")
                .status(302)
                .header("Location", "/setup/info"));
        }
        let mut context = context(&request).await;
        insert(&mut context, "database", database_env());
        return Ok(view::render("setup/wizard/database.html", context));
    }

    let mut context = context(&request).await;
    let body: DatabaseRequest = request.input().await?;
    let mut env = std::fs::read_to_string(".env").unwrap_or_default();
    for (key, value) in [
        ("DB_CONNECTION", body.database_type.as_str()),
        ("DB_HOST", body.host.as_str()),
        ("DB_PORT", body.port.as_str()),
        ("DB_DATABASE", body.db.as_str()),
        ("DB_USERNAME", body.username.as_str()),
        ("DB_PASSWORD", body.password.as_str()),
        ("DB_PREFIX", body.prefix.as_str()),
    ] {
        env = set_env_line(&env, key, value);
    }
    if let Err(error) = std::fs::write(".env", env) {
        insert(&mut context, "database", database_env());
        insert(
            &mut context,
            "errors",
            json!([format!("无法写入 .env：{error}")]),
        );
        return Ok(view::render("setup/wizard/database.html", context));
    }
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/setup/info"))
}

fn set_env_line(content: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key}=");
    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                found = true;
                format!("{prefix}{value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(format!("{prefix}{value}"));
    }
    let mut result = lines.join("\n");
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn database_env() -> Value {
    json!({
        "type": std::env::var("DB_CONNECTION").unwrap_or_else(|_| "sqlite".to_string()),
        "host": std::env::var("DB_HOST").unwrap_or_default(),
        "port": std::env::var("DB_PORT").unwrap_or_default(),
        "username": std::env::var("DB_USERNAME").unwrap_or_default(),
        "password": std::env::var("DB_PASSWORD").unwrap_or_default(),
        "database": std::env::var("DB_DATABASE").unwrap_or_default(),
        "prefix": std::env::var("DB_PREFIX").unwrap_or_default(),
    })
}

#[derive(Deserialize, Default)]
struct DatabaseRequest {
    #[serde(rename = "type", default)]
    database_type: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    db: String,
    #[serde(default)]
    prefix: String,
}

#[derive(Deserialize, Default)]
struct FinishRequest {
    #[serde(default)]
    email: String,
    #[serde(default)]
    nickname: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    password_confirmation: String,
    #[serde(default)]
    site_name: String,
}

/// `GET /setup/info` —— 管理员与站点信息。
#[handler]
pub async fn info(request: Request) -> Response {
    if install::is_locked() {
        return locked_page();
    }
    let mut context = context(&request).await;
    insert(
        &mut context,
        "form",
        json!({
            "email": "",
            "nickname": "",
            "site_name": options::value("site_name").await.unwrap_or_else(|_| "NeoSkin".to_string()),
        }),
    );
    insert(&mut context, "errors", json!([]));
    Ok(view::render("setup/wizard/info.html", context))
}

/// `POST /setup/finish` —— 创建超级管理员并写入安装锁。
#[handler]
pub async fn finish(request: Request) -> Response {
    if install::is_locked() {
        return locked_page();
    }
    let mut context = context(&request).await;
    let body: FinishRequest = request.input().await?;
    let email = body.email.trim().to_string();
    let nickname = body.nickname.trim().to_string();
    let site_name = body.site_name.trim().to_string();

    let mut errors = Vec::new();
    if email.is_empty() || !email.contains('@') {
        errors.push("请输入有效的邮箱地址。".to_string());
    }
    if nickname.is_empty() {
        errors.push("昵称不能为空。".to_string());
    }
    if body.password.chars().count() < 8 || body.password.chars().count() > 32 {
        errors.push("密码长度必须为 8–32 位。".to_string());
    } else if body.password != body.password_confirmation {
        errors.push("两次输入的密码不一致。".to_string());
    }
    if site_name.is_empty() {
        errors.push("站点名称不能为空。".to_string());
    }
    if !errors.is_empty() {
        insert(
            &mut context,
            "form",
            json!({ "email": email, "nickname": nickname, "site_name": site_name }),
        );
        insert(&mut context, "errors", json!(errors));
        return Ok(view::render("setup/wizard/info.html", context));
    }

    if User::filter("email", email.clone())
        .first()
        .await?
        .is_some()
    {
        insert(
            &mut context,
            "form",
            json!({ "email": email, "nickname": nickname, "site_name": site_name }),
        );
        insert(&mut context, "errors", json!(["该邮箱已被注册。"]));
        return Ok(view::render("setup/wizard/info.html", context));
    }

    let password_hash = hash_password(&body.password, None)
        .map_err(|error| FrameworkError::internal(format!("密码哈希失败: {error}")))?;
    let now = chrono::Utc::now();
    let ip = std::env::var("REMOTE_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    User::create(attrs! {
        email: email,
        nickname: nickname,
        score: options::int_or("user_initial_score", 1000).await? as i32,
        avatar: 0,
        password: password_hash,
        ip: ip,
        permission: PERMISSION_SUPER,
        last_sign_at: now - chrono::Duration::days(1),
        register_at: now,
        verified: true,
        verification_token: "",
    })
    .await?;

    options::set("site_name", &site_name).await?;
    let site_url = std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost".to_string());
    options::set("site_url", &site_url).await?;

    install::lock()
        .map_err(|error| FrameworkError::internal(format!("无法写入 install.lock: {error}")))?;

    Ok(view::render("setup/wizard/finish.html", context))
}
