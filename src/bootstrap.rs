//! Application bootstrap.
//!
//! Phase 0/2 只注册数据库、Session 与 CSRF；业务中间件随阶段推进补齐。

use suprnova::{CsrfMiddleware, DB, Model, SessionConfig, SessionMiddleware, global_middleware};

pub async fn register() {
    DB::init().await.expect("Failed to connect to database");
    // 首次启动时写入 `config/options.php` 的默认选项（PHP import_options）。
    let _ = crate::support::options::seed_defaults().await;
    // 从 PHP 迁移过来的库通常没有 install.lock；有用户数据时自动补锁。
    let users = crate::models::user::User::all()
        .await
        .map(|rows| rows.len())
        .unwrap_or(0);
    crate::support::install::ensure_lock_for_existing_install(users);

    // 注册编译期原生插件（D-001）；插件在此注册 Filter / Hook / 事件。
    let _ = crate::support::plugin::boot().await;

    let session_config = SessionConfig::from_env();
    // 静态资源先于会话处理（逻辑前端产物不依赖会话）。
    global_middleware!(crate::support::static_middleware::StaticMiddleware);
    // 未安装时把页面请求引导到安装向导（PHP `RedirectToSetup`）。
    global_middleware!(crate::support::install::InstallMiddleware);
    // 启动时生成 `public/lang/<locale>.js`（front-end 翻译）。
    let _ = crate::support::lang::generate().await;
    // 本地 HTTP 开发时 SESSION_SECURE=false；XSRF cookie 跟随同一开关。
    let secure = session_config.cookie_secure;
    global_middleware!(SessionMiddleware::new(session_config));
    // Passport 的 token 端点不在 `web` 中间件组内，因此没有 CSRF 保护。
    global_middleware!(
        CsrfMiddleware::new()
            .except(vec!["/api", "/api/*"])
            .except_method("POST", "/oauth/token")
            .xsrf_cookie_secure(secure)
    );
    // 插件通过 `Hook::pushMiddleware()` 追加的全局中间件。
    global_middleware!(crate::support::plugin_api::PluginMiddleware);
}
