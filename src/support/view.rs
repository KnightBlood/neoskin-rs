//! MiniJinja 视图引擎（Twig 兼容层的子集）。
//!
//! 模板位于 `resources/views/**/*.html`，采用 Jinja2 语法；Twig 的
//! `{{ include('x') }}`、`|raw`、`|json_encode` 已转换为 Jinja 等价写法。
//! 提供的全局函数/变量：
//! - `trans(key, params?)`
//! - `url(path)`（基于上下文 `base_url`）
//! - `csrf_field()`（安全 HTML）
//! - `csrf_token`、`locale`、`user`、`site_name` 等由调用方传入。

use std::sync::OnceLock;

use minijinja::{AutoEscape, Environment, State, Value, path_loader};
use serde_json::json;
use suprnova::HttpResponse;

/// Twig 的 HTML 转义规则：`& < > " '`，与 PHP `htmlspecialchars` 近似，
/// 但**不转义 `/`**（MiniJinja 默认会把 `/` 变成 `&#x2f;`，会破坏 URL 输出）。
fn twig_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            other => out.push(other),
        }
    }
    out
}

fn environment() -> &'static Environment<'static> {
    static ENV: OnceLock<Environment<'static>> = OnceLock::new();
    ENV.get_or_init(|| {
        let mut env = Environment::new();
        env.set_loader(path_loader("resources/views"));
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        env.set_formatter(|out, state, value| {
            if matches!(state.auto_escape(), AutoEscape::Html) && !value.is_safe() {
                out.write_str(&twig_escape(&value.to_string()))
                    .map_err(minijinja::Error::from)
            } else {
                minijinja::escape_formatter(out, state, value)
            }
        });
        env.add_function(
            "trans",
            |state: &State, key: String, params: Option<Value>| {
                let locale = state
                    .lookup("locale_raw")
                    .and_then(|value| value.as_str().map(ToString::to_string))
                    .unwrap_or_else(|| "zh_CN".to_string());
                let params = params.and_then(|value| match serde_json::to_value(value) {
                    Ok(serde_json::Value::Object(map)) => Some(map),
                    _ => None,
                });
                let translated =
                    crate::support::i18n::translate_value(&locale, &key, params.as_ref());
                Value::from_serialize(&translated)
            },
        );
        env.add_function("url", |state: &State, path: String| -> String {
            let base = state
                .lookup("base_url")
                .and_then(|value| value.as_str().map(ToString::to_string))
                .unwrap_or_default();
            let base = base.trim_end_matches('/');
            if path.is_empty() || path == "/" {
                format!("{base}/")
            } else {
                format!("{base}/{}", path.trim_start_matches('/'))
            }
        });
        env.add_function("csrf_field", |state: &State| -> Value {
            let token = state
                .lookup("csrf_token")
                .and_then(|value| value.as_str().map(ToString::to_string))
                .unwrap_or_default();
            Value::from_safe_string(format!(
                "<input type=\"hidden\" name=\"_token\" value=\"{}\">",
                crate::support::communication::escape_html(&token)
            ))
        });
        env.add_filter("json_encode", |value: Value| -> String {
            serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
        });
        env.add_filter("tojson", |value: Value| -> Value {
            Value::from_safe_string(
                serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string()),
            )
        });
        // Twig `nl2br`：在换行前插入 `<br />`；调用方仍需 `|safe`。
        env.add_filter("nl2br", |value: String| -> String {
            value.replace('\n', "<br />\n")
        });
        env
    })
}

/// 渲染模板，失败时返回 500 文本（包含模板错误，便于本地开发）。
pub fn render(name: &str, context: serde_json::Value) -> HttpResponse {
    let env = environment();
    match env
        .get_template(name)
        .and_then(|template| template.render(context))
    {
        Ok(html) => HttpResponse::html(html),
        Err(error) => {
            let mut message = format!("{error}");
            let mut source = std::error::Error::source(&error);
            while let Some(cause) = source {
                message.push_str(&format!(" | {cause}"));
                source = cause.source();
            }
            HttpResponse::text(format!("Template error ({name}): {message}")).status(500)
        }
    }
}

pub fn render_status(name: &str, context: serde_json::Value, status: u16) -> HttpResponse {
    render(name, context).status(status)
}

/// 语言包 URL（`/lang/<locale>.js`）。
pub fn lang_url(locale: &str) -> String {
    format!("/lang/{locale}.js")
}

/// 站点基础上下文（供所有页面复用）。
pub async fn base_context(
    locale: &str,
    request_path: &str,
    csrf_token: &str,
    user: Option<&crate::models::user::User>,
) -> serde_json::Value {
    let site_name = crate::support::options::value("site_name")
        .await
        .unwrap_or_else(|_| "Blessing Skin".to_string());
    let navbar_color = crate::support::options::value("navbar_color")
        .await
        .unwrap_or_else(|_| "cyan".to_string());
    let color_mode = if ["light", "warning", "white", "orange"].contains(&navbar_color.as_str()) {
        "light"
    } else {
        "dark"
    };
    let site_url = crate::support::options::value("site_url")
        .await
        .unwrap_or_default();
    let base_url = if site_url.is_empty() {
        std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost".to_string())
    } else {
        site_url.clone()
    };
    let favicon = crate::support::options::value("favicon_url")
        .await
        .unwrap_or_else(|_| "app/favicon.ico".to_string());
    let favicon = if favicon.starts_with("http") {
        favicon
    } else {
        format!("{}/{favicon}", base_url.trim_end_matches('/'))
    };
    let theme_color = match navbar_color.as_str() {
        "primary" => "#007bff",
        "secondary" => "#6c757d",
        "success" => "#28a745",
        "warning" => "#ffc107",
        "danger" => "#dc3545",
        "navy" => "#001f3f",
        "olive" => "#3d9970",
        "lime" => "#01ff70",
        "fuchsia" => "#f012be",
        "maroon" => "#d81b60",
        "indigo" => "#6610f2",
        "purple" => "#6f42c1",
        "pink" => "#e83e8c",
        "orange" => "#fd7e14",
        "teal" => "#20c997",
        "cyan" => "#17a2b8",
        "gray" => "#6c757d",
        _ => "#17a2b8",
    };
    let meta_keywords = crate::support::options::value("meta_keywords")
        .await
        .unwrap_or_default();
    let meta_description = crate::support::options::value("meta_description")
        .await
        .unwrap_or_default();
    let meta_extras = crate::support::options::value("meta_extras")
        .await
        .unwrap_or_default();
    let custom_css = crate::support::options::value("custom_css")
        .await
        .unwrap_or_default();
    let custom_js = crate::support::options::value("custom_js")
        .await
        .unwrap_or_default();
    let cdn = crate::support::options::value("cdn_address")
        .await
        .unwrap_or_default();
    let copyright = crate::support::options::int_value("copyright_prefer")
        .await
        .unwrap_or(0);
    let custom_copyright = crate::support::options::value("copyright_text")
        .await
        .unwrap_or_default();
    let locale_dash = locale.replace('_', "-");
    let links = crate::support::plugin_api::head_links(request_path);
    let scripts = crate::support::plugin_api::scripts(request_path);

    json!({
        "locale": locale_dash,
        "locale_raw": locale,
        "site_name": site_name,
        "site_url": site_url,
        "base_url": base_url,
        "navbar_color": navbar_color,
        "color_mode": color_mode,
        "dark_mode": user.map(|user| user.is_dark_mode).unwrap_or(false),
        "csrf_token": csrf_token,
        "favicon": favicon,
        "theme_color": theme_color,
        "cdn_base": cdn,
        "custom_cdn_host": cdn,
        "inline_css": custom_css,
        "inline_js": custom_js,
        "seo": {
            "keywords": meta_keywords,
            "description": meta_description,
            "extra": meta_extras,
        },
        "blessing": {
            "version": "6.0.11_patch",
            "locale": locale,
            "base_url": base_url.clone(),
            "site_name": site_name,
            "route": request_path,
        },
        "i18n": lang_url(locale),
        "links": links,
        "scripts": scripts,
        "extra_head": [],
        "extra_foot": [],
        "copyright": copyright,
        "custom_copyright": custom_copyright,
        "locales": crate::support::locale::LOCALES
            .iter()
            .map(|(id, name, _)| json!({ "id": id, "name": name }))
            .collect::<Vec<_>>(),
        "user": user.map(|user| json!({
            "uid": user.uid,
            "email": user.email,
            "nickname": user.nickname,
            "avatar": user.avatar,
            "isAdmin": user.is_admin(),
            "isDarkMode": user.is_dark_mode,
        })),
    })
}
