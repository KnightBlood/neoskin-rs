//! 编译期注册的原生 Rust 插件（D-001）。
//!
//! 站点作者在这里把实现了 `support::plugin_api::Plugin` 的插件注册进进程：
//!
//! ```ignore
//! use std::sync::Arc;
//!
//! pub async fn register_all() -> Result<(), suprnova::FrameworkError> {
//!     let enabled = crate::support::plugin::enabled_state().await;
//!     if enabled.iter().any(|(name, _)| name == "my-plugin") {
//!         crate::support::plugin_api::register_plugin(Arc::new(MyPlugin));
//!     }
//!     Ok(())
//! }
//! ```
//!
//! 插件仍需在 `plugins/<name>/package.json` 里提供清单，才能被后台的
//! 启停/市场管理；插件代码本身编译进二进制，不执行 PHP。

use suprnova::FrameworkError;

pub async fn register_all() -> Result<(), FrameworkError> {
    let enabled = crate::support::plugin::enabled_state().await;
    if enabled.is_empty() {
        return Ok(());
    }
    let plugins = crate::support::plugin::all().await?;
    for plugin in plugins {
        if !enabled.iter().any(|(name, _)| name == &plugin.name) {
            continue;
        }
        let Some(library) =
            crate::support::native_plugin::library_path(&plugin.manifest, &plugin.path)
        else {
            continue;
        };
        if let Err(error) = crate::support::native_plugin::load(&library) {
            crate::support::plugin_api::fire(
                "PluginBootFailed",
                serde_json::json!({
                    "name": plugin.name,
                    "error": error.to_string(),
                }),
            );
        }
    }
    Ok(())
}
