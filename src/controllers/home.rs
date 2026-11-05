//! 站点入口（API 元信息与 `.well-known` 重定向）。

use serde_json::json;
use suprnova::{HttpResponse, Request, Response, handler};

use crate::support::options;

pub const VERSION: &str = "6.0.11_patch";

pub const COPYRIGHT: &[&str] = &[
    "Powered with ❤ by Blessing Skin Server.",
    "Powered by Blessing Skin Server.",
    "Proudly powered by Blessing Skin Server.",
    "由 Blessing Skin Server 强力驱动。",
    "采用 Blessing Skin Server 搭建。",
    "使用 Blessing Skin Server 稳定运行。",
    "自豪地采用 Blessing Skin Server。",
];

/// GET /api —— 皮肤站 API 元信息（PHP `HomeController@apiRoot`）。
#[handler]
pub async fn api_root(_request: Request) -> Response {
    let preferred = options::int_value("copyright_prefer").await.unwrap_or(0);
    let copyright = COPYRIGHT
        .get(preferred.max(0) as usize)
        .copied()
        .unwrap_or(COPYRIGHT[0]);
    let site_name = options::value("site_name")
        .await
        .unwrap_or_else(|_| "Blessing Skin".to_string());
    Ok(HttpResponse::json(json!({
        "blessing_skin": VERSION,
        "spec": 0,
        "copyright": copyright,
        "site_name": site_name,
    })))
}

/// GET /.well-known/change-password —— 重定向到资料页。
#[handler]
pub async fn change_password(_request: Request) -> Response {
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/user/profile"))
}

/// `GET /{player}` —— 根级单段路径（PHP 的静态 catch-all）。
///
/// 优先级与源一致：`public/` 中的真实文件 → `<name>.json` 的
/// CustomSkinAPI 响应 → 404 页面。
#[handler]
pub async fn root_path(request: Request) -> Response {
    let raw = request.param("player").unwrap_or_default().to_string();
    if let Some(name) = raw.strip_suffix(".json")
        && !name.is_empty()
    {
        return crate::controllers::texture::player_json_by_name(name).await;
    }
    match root_file(&raw).await {
        Some(response) => response,
        None => crate::controllers::page::not_found(&request).await,
    }
}

/// 读取 `public/<name>`（单段路径）。
async fn root_file(name: &str) -> Option<Response> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.starts_with('.')
    {
        return None;
    }
    let base = std::env::var_os("APP_BASE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join("public");
    let canonical_base = base.canonicalize().ok()?;
    let canonical = base.join(name).canonicalize().ok()?;
    if !canonical.starts_with(&canonical_base) || !canonical.is_file() {
        return None;
    }
    let data = tokio::fs::read(&canonical).await.ok()?;
    let length = data.len();
    Some(Ok(HttpResponse::bytes_body(
        data,
        crate::support::static_middleware::mime_for(name),
    )
    .header("Content-Length", length.to_string())
    .header(
        "Cache-Control",
        crate::support::static_middleware::cache_for(name),
    )))
}
