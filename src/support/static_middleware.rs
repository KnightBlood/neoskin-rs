//! 静态资源中间件：`/app/*`、`/lang/*`、`/plugins/*` 直接读 `public/`，
//! 不经过路由匹配（避免 catch-all 与 `/` 路由的冲突）。

use suprnova::{HttpResponse, Middleware, Next, Request, Response, async_trait};

pub struct StaticMiddleware;

#[async_trait]
impl Middleware for StaticMiddleware {
    async fn handle(&self, request: Request, next: Next) -> Response {
        let method = request.method().as_str();
        if method != "GET" && method != "HEAD" {
            return next(request).await;
        }
        let path = request.path().to_string();
        for prefix in ["/app/", "/lang/", "/plugins/"] {
            if let Some(relative) = path.strip_prefix(prefix) {
                return serve(
                    &request,
                    prefix.trim_matches('/'),
                    relative,
                    method == "HEAD",
                )
                .await;
            }
        }
        next(request).await
    }
}

fn safe(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.contains('\\')
        && !relative.contains('\0')
        && !relative.split('/').any(|segment| {
            segment.is_empty() || segment == "." || segment == ".." || segment.starts_with('.')
        })
}

async fn serve(request: &Request, subdir: &str, relative: &str, head_only: bool) -> Response {
    if !safe(relative) {
        return Ok(HttpResponse::text("Not Found").status(404));
    }
    let base = std::env::var_os("APP_BASE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join("public")
        .join(subdir);
    let Ok(canonical_base) = base.canonicalize() else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Ok(canonical) = base.join(relative).canonicalize() else {
        return missing(request, relative).await;
    };
    if !canonical.starts_with(&canonical_base) || !canonical.is_file() {
        return missing(request, relative).await;
    }
    let length = std::fs::metadata(&canonical)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let content_type = mime_for(relative);
    if head_only {
        return Ok(HttpResponse::bytes_body(Vec::new(), content_type)
            .header("Content-Length", length.to_string())
            .header("Cache-Control", cache_for(relative)));
    }
    let data = match tokio::fs::read(&canonical).await {
        Ok(data) => data,
        Err(_) => return Ok(HttpResponse::text("Not Found").status(404)),
    };
    Ok(HttpResponse::bytes_body(data, content_type)
        .header("Content-Length", length.to_string())
        .header("Cache-Control", cache_for(relative)))
}

async fn missing(request: &Request, relative: &str) -> Response {
    if !relative
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .contains('.')
    {
        crate::controllers::page::not_found(request).await
    } else {
        Ok(HttpResponse::text("Not Found").status(404))
    }
}

pub(crate) fn cache_for(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or_default();
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() >= 3 && parts[parts.len() - 2].len() >= 6 {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    }
}

pub(crate) fn mime_for(path: &str) -> &'static str {
    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "map" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "eot" => "application/vnd.ms-fontobject",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}
