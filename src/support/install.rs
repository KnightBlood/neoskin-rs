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

/// `$request->is('setup*')`：按路径段匹配，`setupx` 不算。
pub fn is_setup_path(path: &str) -> bool {
    let path = path.trim_start_matches('/');
    path == "setup" || path.starts_with("setup/")
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
        let locked = is_locked();

        // PHP `RedirectToSetup`：已安装且 app.version 高于 option('version') 时
        // 在请求期执行 `Artisan::call('update')`。升级失败只记日志，不阻断请求。
        if locked
            && !is_setup_path(&path)
            && let Err(error) = crate::support::upgrade::run_if_needed().await
        {
            eprintln!("auto update skipped: {error}");
        }

        if locked || is_exempt(&path) {
            return next(request).await;
        }
        Ok(HttpResponse::text("")
            .status(302)
            .header("Location", "/setup"))
    }
}

#[cfg(test)]
mod tests {
    use super::{is_exempt, is_setup_path};

    #[test]
    fn setup_paths_are_segment_matched() {
        assert!(is_setup_path("/setup"));
        assert!(is_setup_path("/setup/database"));
        assert!(is_setup_path("setup/info"));
        assert!(!is_setup_path("/setupx"));
        assert!(!is_setup_path("/"));
    }

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
