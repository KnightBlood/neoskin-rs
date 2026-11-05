//! 安装状态（PHP `CheckInstallation` / `RedirectToSetup` 中间件）。

use suprnova::{HttpResponse, Middleware, Next, Request, Response, async_trait};

use crate::support::storage;

/// `storage/install.lock` 是否存在。
pub fn is_locked() -> bool {
    storage::install_lock_path().is_file()
}

/// 写入安装锁；重复调用无副作用。
pub fn lock() -> Result<(), std::io::Error> {
    let path = storage::install_lock_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, b"")
}

/// 已有用户数据但缺少锁文件时自动补锁（兼容从 PHP 迁移的库）。
pub fn ensure_lock_for_existing_install(user_count: usize) {
    if user_count > 0 && !is_locked() {
        let _ = lock();
    }
}

fn is_exempt(path: &str) -> bool {
    path == "/setup"
        || path.starts_with("/setup/")
        || path.starts_with("/app/")
        || path.starts_with("/lang/")
        || path.starts_with("/plugins/")
        || path.starts_with("/__neoskin/")
}

/// 未安装时把所有页面请求重定向到 `/setup`。
pub struct InstallMiddleware;

#[async_trait]
impl Middleware for InstallMiddleware {
    async fn handle(&self, request: Request, next: Next) -> Response {
        let path = request.path().to_string();
        if is_locked() || is_exempt(&path) {
            return next(request).await;
        }
        Ok(HttpResponse::text("")
            .status(302)
            .header("Location", "/setup"))
    }
}

#[cfg(test)]
mod tests {
    use super::is_exempt;

    #[test]
    fn setup_and_assets_are_exempt() {
        assert!(is_exempt("/setup"));
        assert!(is_exempt("/setup/database"));
        assert!(is_exempt("/app/style.css"));
        assert!(is_exempt("/lang/zh_CN.js"));
        assert!(is_exempt("/__neoskin/health"));
        assert!(!is_exempt("/"));
        assert!(!is_exempt("/auth/login"));
    }
}
