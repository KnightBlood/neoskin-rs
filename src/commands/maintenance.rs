//! 维护命令（对应 PHP `app/Console/Commands`）。
//!
//! 命令通过 Suprnova 的 `inventory` 注册，`src/bin/console.rs` 无需逐个接线：
//!
//! ```text
//! cargo run --bin console -- update
//! cargo run --bin console -- bs:install admin@example.com secret123 Admin
//! cargo run --bin console -- options:cache
//! cargo run --bin console -- plugin:enable <name>
//! cargo run --bin console -- plugin:disable <name>
//! cargo run --bin console -- salt:random [--show]
//! ```
//!
//! 说明：本工程不执行 PHP 插件（D-001），`plugin:enable/disable` 只操作清单层
//! 的启用状态；`salt:random` 为 PHP 兼容保留，Rust 运行时本身不读取 `SALT`。

use std::fmt::Write as _;
use std::path::PathBuf;

use async_trait::async_trait;
use clap::Parser;
use rand::RngExt;
use suprnova::{Command, FrameworkError, Model, TypedCommand, attrs};

use crate::models::option::OptionItem;
use crate::models::user::{PERMISSION_SUPER, User};
use crate::support::{hash, install, options, plugin, storage, upgrade};

fn line(message: impl AsRef<str>) {
    suprnova::console::line(message.as_ref());
}

// ------------------------------------------------------------------ update

/// `php artisan update` / 请求期 `RedirectToSetup` 的自动升级。
#[derive(Parser, Command, Debug)]
#[console(
    name = "update",
    description = "Run database migrations and version procedures"
)]
pub struct Update;

#[async_trait]
impl TypedCommand for Update {
    async fn run(self) -> Result<(), FrameworkError> {
        let report = upgrade::run().await?;
        line(format!(
            "Updated {} -> {} (procedures: {})",
            report.from,
            report.to,
            if report.procedures.is_empty() {
                "none".to_string()
            } else {
                report.procedures.join(", ")
            }
        ));
        Ok(())
    }
}

// --------------------------------------------------------------- bs:install

/// `php artisan bs:install {email} {password} {nickname}`。
#[derive(Parser, Command, Debug)]
#[console(
    name = "bs:install",
    description = "Create the database schema and a super administrator"
)]
pub struct BsInstall {
    /// 管理员邮箱。
    pub email: String,
    /// 管理员密码（8–32 位）。
    pub password: String,
    /// 管理员昵称。
    pub nickname: String,
}

#[async_trait]
impl TypedCommand for BsInstall {
    async fn run(self) -> Result<(), FrameworkError> {
        if install::is_locked() {
            line("You have installed Blessing Skin Server. Nothing to do.");
            return Ok(());
        }

        upgrade::run().await?;

        let score = options::int_or("user_initial_score", 1000).await? as i32;
        let password_hash = hash::hash_password(&self.password, None)
            .map_err(|error| FrameworkError::internal(format!("密码哈希失败: {error}")))?;
        let now = chrono::Utc::now();

        let created = User::create(attrs! {
            email: self.email.clone(),
            nickname: self.nickname.clone(),
            score: score,
            avatar: 0,
            password: password_hash,
            ip: "127.0.0.1".to_string(),
            permission: PERMISSION_SUPER,
            last_sign_at: now - chrono::Duration::days(1),
            register_at: now,
            verified: true,
            verification_token: "",
        })
        .await;

        if let Err(error) = created {
            // `upgrade::run()` 已经写了锁；管理员创建失败时撤掉，避免留下
            // 「已安装但没有超管」的半成品站点。
            let _ = std::fs::remove_file(storage::install_lock_path());
            return Err(error);
        }

        let site_url = std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost".to_string());
        options::set("site_url", &site_url).await?;

        install::lock()
            .map_err(|error| FrameworkError::internal(format!("无法写入 install.lock: {error}")))?;

        line("Installation completed!");
        line("We recommend to modify your \"Site URL\" option if incorrect.");
        Ok(())
    }
}

// ------------------------------------------------------------ options:cache

/// `php artisan options:cache`：把 options 表导出成 `storage/options.php`。
#[derive(Parser, Command, Debug)]
#[console(name = "options:cache", description = "Cache Blessing Skin options")]
pub struct OptionsCache;

#[async_trait]
impl TypedCommand for OptionsCache {
    async fn run(self) -> Result<(), FrameworkError> {
        let rows: Vec<(String, String)> = OptionItem::all()
            .await?
            .into_vec()
            .into_iter()
            .map(|row| (row.option_name, row.option_value))
            .collect();
        let path = upgrade::options_cache_path();
        write_options_cache(&path, &rows)?;
        line("Options cached successfully.");
        Ok(())
    }
}

/// 生成与 PHP `var_export()` 等价的数组文件。
fn write_options_cache(
    path: &std::path::Path,
    rows: &[(String, String)],
) -> Result<(), FrameworkError> {
    let mut content =
        String::from("<?php\n// This is auto-generated. DO NOT edit manually.\nreturn array (\n");
    for (name, value) in rows {
        let _ = writeln!(content, "  {} => {},", php_export(name), php_export(value));
    }
    content.push_str(");\n");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            FrameworkError::internal(format!("无法创建 options 缓存目录: {error}"))
        })?;
    }
    std::fs::write(path, content)
        .map_err(|error| FrameworkError::internal(format!("无法写入 options 缓存: {error}")))
}

/// PHP 单引号字符串：只转义 `\` 与 `'`。
fn php_export(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}

// ---------------------------------------------------------- plugin:enable/disable

/// `php artisan plugin:enable {name}`。
#[derive(Parser, Command, Debug)]
#[console(name = "plugin:enable", description = "Enable a plugin")]
pub struct PluginEnable {
    /// 插件目录名。
    pub name: String,
}

#[async_trait]
impl TypedCommand for PluginEnable {
    async fn run(self) -> Result<(), FrameworkError> {
        match plugin::enable(&self.name, "zh_CN").await? {
            None => line(format!("Plugin [{}] enabled.", self.name)),
            Some(messages) => {
                for message in messages {
                    line(message);
                }
            }
        }
        Ok(())
    }
}

/// `php artisan plugin:disable {name}`。
#[derive(Parser, Command, Debug)]
#[console(name = "plugin:disable", description = "Disable a plugin")]
pub struct PluginDisable {
    /// 插件目录名。
    pub name: String,
}

#[async_trait]
impl TypedCommand for PluginDisable {
    async fn run(self) -> Result<(), FrameworkError> {
        plugin::disable(&self.name).await?;
        line(format!("Plugin [{}] disabled.", self.name));
        Ok(())
    }
}

// ------------------------------------------------------------- salt:random

/// `php artisan salt:random {--show}`。
#[derive(Parser, Command, Debug)]
#[console(
    name = "salt:random",
    description = "Set or display the application salt"
)]
pub struct SaltRandom {
    /// 只显示盐值，不写入 `.env`。
    #[arg(long, default_value_t = false)]
    pub show: bool,
}

#[async_trait]
impl TypedCommand for SaltRandom {
    async fn run(self) -> Result<(), FrameworkError> {
        let mut bytes = [0u8; 16];
        rand::rng().fill(&mut bytes);
        let salt = hex::encode(bytes);

        if self.show {
            line(&salt);
            return Ok(());
        }

        let path = PathBuf::from(".env");
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        let mut replaced = false;
        let mut lines: Vec<String> = current
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("SALT=") {
                    replaced = true;
                    format!("SALT={salt}")
                } else {
                    line.to_string()
                }
            })
            .collect();
        if !replaced {
            lines.push(format!("SALT={salt}"));
        }
        let content = format!("{}\n", lines.join("\n"));
        std::fs::write(&path, content)
            .map_err(|error| FrameworkError::internal(format!("无法写入 .env: {error}")))?;

        line(format!("Application salt [{salt}] set successfully."));
        Ok(())
    }
}

/// 预留：`storage` 模块路径（供后续命令复用）。
#[allow(dead_code)]
fn storage_root() -> PathBuf {
    storage::storage_root()
}

#[cfg(test)]
mod tests {
    use super::{php_export, write_options_cache};

    #[test]
    fn php_string_escaping_matches_var_export() {
        assert_eq!(php_export("plain"), "'plain'");
        assert_eq!(php_export("a'b"), "'a\\'b'");
        assert_eq!(php_export("a\\b"), "'a\\\\b'");
    }

    #[test]
    fn options_cache_has_php_header_and_entries() {
        let dir =
            std::env::temp_dir().join(format!("neoskin-options-cache-{}", std::process::id()));
        let path = dir.join("options.php");
        let rows = vec![
            ("site_name".to_string(), "NeoSkin".to_string()),
            ("quote".to_string(), "it's".to_string()),
        ];
        write_options_cache(&path, &rows).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("<?php\n// This is auto-generated."));
        assert!(content.contains("'site_name' => 'NeoSkin',"));
        assert!(content.contains("'quote' => 'it\\'s',"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
