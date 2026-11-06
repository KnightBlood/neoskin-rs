//! SPA 壳与契约 v1 bootstrap（见 NeoSkinFrontend/docs/BACKEND-CONTRACT.md）。

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suprnova::{HttpResponse, Request, Response, handler};

use crate::controllers::home::VERSION;
use crate::support::{auth, locale, options as option_store, static_middleware};

const CONTRACT_VERSION: i64 = 1;
const APP_NEXT_ROOT: &str = "public/app-next";

const LOCALES: &[&str] = &[
    "zh_CN", "zh_TW", "en", "de_DE", "es_ES", "fr_FR", "it_IT", "ja_JP", "ko_KR", "nl_NL", "pt_PT",
    "ru_RU", "el_GR",
];

async fn localized(name: &str, locale: &str) -> String {
    if let Ok(Some(value)) = option_store::get(&format!("{name}_{locale}")).await
        && !value.is_empty()
    {
        return value;
    }
    option_store::value(name).await.unwrap_or_default()
}

async fn option_bool(name: &str, fallback: bool) -> bool {
    match option_store::value(name).await {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "on" | "yes" => true,
            "false" | "0" | "off" | "no" => false,
            _ => fallback,
        },
        Err(_) => fallback,
    }
}

async fn option_int(name: &str, fallback: i64) -> i64 {
    option_store::int_value(name).await.unwrap_or(fallback)
}

async fn bootstrap_payload(request: &Request, mode: &str) -> Value {
    let current_locale = locale::resolve(request);
    let site_name = localized("site_name", &current_locale).await;
    if site_name.is_empty() {
        // 站点名缺失时保持与其它端口一致的默认值。
    }
    let site_name = if site_name.is_empty() {
        "Blessing Skin".to_string()
    } else {
        site_name
    };

    let user = match auth::current_user(request).await {
        Ok(Some(user)) => json!({
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
        }),
        _ => Value::Null,
    };

    json!({
        "contractVersion": CONTRACT_VERSION,
        "apiBase": "",
        "locale": current_locale,
        "localeFallback": "en",
        "siteName": site_name,
        "version": VERSION,
        "basePath": format!("/app-next/{mode}/"),
        "mode": mode,
        "csrfToken": suprnova::get_csrf_token().unwrap_or_default(),
        "locales": LOCALES,
        "user": user,
        "options": {
            "site_name": localized("site_name", &current_locale).await,
            "site_description": localized("site_description", &current_locale).await,
            "user_can_register": true,
            "register_with_player_name": option_bool("register_with_player_name", true).await,
            "captcha": false,
            "recaptcha": !option_store::value("recaptcha_sitekey").await.unwrap_or_default().is_empty(),
            "recaptcha_site_key": option_store::value("recaptcha_sitekey").await.unwrap_or_default(),
            "max_upload_file_size": option_int("max_upload_file_size", 1024).await,
            "max_texture_width": option_int("max_texture_width", 8192).await,
            "score_per_storage": option_int("score_per_storage", 1).await,
            "private_score_per_storage": option_int("private_score_per_storage", 10).await,
            "score_award_per_texture": option_int("score_award_per_texture", 0).await,
            "score_per_player": option_int("score_per_player", 100).await,
            "score_per_closet_item": option_int("score_per_closet_item", 0).await,
            "sign_score": option_store::value("sign_score").await.unwrap_or_default(),
            "take_back_scores_after_deletion": option_bool("take_back_scores_after_deletion", true).await,
            "player_name_rule": option_store::value("player_name_rule").await.unwrap_or_else(|_| "official".to_string()),
            "custom_player_name_regexp": option_store::value("custom_player_name_regexp").await.unwrap_or_default(),
            "navbar_color": option_store::value("navbar_color").await.unwrap_or_else(|_| "cyan".to_string()),
            "sidebar_color": option_store::value("sidebar_color").await.unwrap_or_else(|_| "dark-maroon".to_string()),
            "copyright_prefer": option_store::value("copyright_prefer").await.unwrap_or_else(|_| "0".to_string()),
            "copyright_text": option_store::value("copyright_text").await.unwrap_or_default(),
            "update_source": option_store::value("update_source").await.unwrap_or_default(),
            "plugin_market_source": option_store::value("plugin_market_source").await.unwrap_or_default(),
        },
        "features": {
            "registration": true,
            "emailVerification": option_bool("require_verification", false).await,
            "passwordReset": false,
            "oauth": true,
            "reports": true,
            "plugins": true,
        },
        "plugins": {
            "menus": [],
            "badges": [],
            "styles": plugin_styles(),
            "scripts": plugin_scripts(),
        },
    })
}

fn plugin_styles() -> Vec<String> {
    crate::support::plugin_api::HOOK.styles_for("*")
}

fn plugin_scripts() -> Vec<String> {
    crate::support::plugin_api::HOOK.scripts_for("*")
}

/// GET /api/bootstrap
#[handler]
pub async fn bootstrap(request: Request) -> Response {
    let mode = request
        .query_param("mode")
        .filter(|value| value == "site" || value == "admin")
        .unwrap_or_else(|| "site".to_string());
    let payload = bootstrap_payload(&request, &mode).await;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": "ok",
        "data": payload,
    })))
}

/// GET /app-next/{mode} 与 GET /app-next/{mode}/{*path}
#[handler]
pub async fn shell(request: Request) -> Response {
    let mode = request.param("mode").unwrap_or_default();
    if mode != "site" && mode != "admin" {
        return Ok(HttpResponse::text("Not Found").status(404));
    }
    let requested = request.param("path").unwrap_or_default();
    let relative = if requested.is_empty() {
        "index.html".to_string()
    } else {
        requested.to_string()
    };

    let Some(base) = app_next_dir(mode) else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let target = base.join(relative.trim_start_matches('/'));
    let canonical_base = match base.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(HttpResponse::text("Not Found").status(404)),
    };
    let canonical = target.canonicalize();
    if let Ok(path) = canonical
        && path.starts_with(&canonical_base)
        && path.is_file()
    {
        let name = relative.rsplit('/').next().unwrap_or_default();
        if name.ends_with(".html") {
            return render_shell(&request, mode, &path).await;
        }
        let data = match tokio::fs::read(&path).await {
            Ok(data) => data,
            Err(_) => return Ok(HttpResponse::text("Not Found").status(404)),
        };
        return Ok(
            HttpResponse::bytes_body(data, static_middleware::mime_for(name))
                .header("Cache-Control", "public, max-age=31536000, immutable"),
        );
    }

    if Path::new(&relative)
        .extension()
        .map(|ext| !ext.eq_ignore_ascii_case("html"))
        .unwrap_or(false)
    {
        return Ok(HttpResponse::text("Not Found").status(404));
    }

    let index = base.join("index.html");
    if !index.is_file() {
        return Ok(HttpResponse::text(
            "app-next 前端产物缺失：请先运行 pnpm -r build 并执行 scripts/publish.ps1",
        )
        .status(404));
    }
    render_shell(&request, mode, &index).await
}

fn app_next_dir(mode: &str) -> Option<PathBuf> {
    let root = std::env::var_os("APP_BASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    Some(root.join(APP_NEXT_ROOT).join(mode))
}

async fn render_shell(request: &Request, mode: &str, index: &Path) -> Response {
    let raw = match tokio::fs::read_to_string(index).await {
        Ok(raw) => raw,
        Err(_) => return Ok(HttpResponse::text("Not Found").status(404)),
    };
    let payload = bootstrap_payload(request, mode).await;
    let encoded = serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string());
    let csrf = suprnova::get_csrf_token().unwrap_or_default();
    let injection = format!(
        "<meta name=\"csrf-token\" content=\"{}\">\n\
         <meta name=\"neoskin-mode\" content=\"{}\">\n\
         <script id=\"neoskin-bootstrap\" type=\"application/json\">{}</script>\n",
        escape_html(&csrf),
        mode,
        escape_script_json(&encoded)
    );
    let html = raw.replacen("</head>", &format!("{injection}</head>"), 1);
    Ok(HttpResponse::html(html).header("Cache-Control", "no-cache"))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_script_json(value: &str) -> String {
    value
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
