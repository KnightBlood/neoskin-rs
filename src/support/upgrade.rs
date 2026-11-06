//! 升级编排（PHP `App\Console\Commands\UpdateCommand`）。
//!
//! PHP 的 `RedirectToSetup` 中间件在
//! `app.version > option('version')` 时请求期调用 `Artisan::call('update')`；
//! `update` 命令也复用同一套逻辑。本模块把它抽成可重入的幂等操作：
//!
//! 1. 按 `option('version')` 依次执行历史 procedure；
//! 2. 把 `version` 写为当前版本；
//! 3. `migrate --force`；
//! 4. 清理视图缓存（`view:clear`）；
//! 5. 写 `storage/install.lock`；
//! 6. 清空 Cache。
//!
//! 进程内用异步互斥锁串行化，中间件触发的并发请求不会重复执行迁移。

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use sea_orm_migration::MigratorTrait;
use serde_json::json;
use suprnova::{Cache, DB, FrameworkError};

use crate::controllers::home::VERSION;
use crate::migrations::Migrator;
use crate::support::{install, options, plugin_api, storage, view};

/// 历史升级 procedure；版本低于下表阈值时执行一次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Procedure {
    /// `0.0.1`：仅用于测试的事件 `__0.0.1`。
    Event001,
    /// `5.0.0`：默认主页背景由 JPG 换为 WebP。
    HomePicWebp,
}

impl Procedure {
    /// 引入该 procedure 的版本号（PHP `procedures()` 数组的键）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Event001 => "0.0.1",
            Self::HomePicWebp => "5.0.0",
        }
    }
}

/// 把 Composer 风格版本归一化为 `(major, minor, patch)`。
///
/// `6.0.11_patch` 与 `6.0.11` 都解析为 `(6, 0, 11)`：Composer 把 `_patch`
/// 视作稳定性标记，与稳定版比较时为相等（`Comparator::greaterThan` 为 false）。
/// 无法解析时返回 `None`。
pub fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let trimmed = version.trim().trim_start_matches(['v', 'V']);
    let numeric: String = trimmed
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    if numeric.is_empty() {
        return None;
    }
    let mut parts = numeric.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    let patch = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    Some((major, minor, patch))
}

/// `Comparator::greaterThan(candidate, base)`。
pub fn is_newer(candidate: &str, base: &str) -> bool {
    match (parse_version(candidate), parse_version(base)) {
        (Some(candidate), Some(base)) => candidate > base,
        _ => candidate.trim() != base.trim(),
    }
}

/// `option('version')` 低于阈值时需要执行的 procedure（按版本升序）。
pub fn procedures_for(stored: &str) -> Vec<Procedure> {
    let mut procedures = Vec::new();
    if is_newer(Procedure::Event001.name(), stored) {
        procedures.push(Procedure::Event001);
    }
    if is_newer(Procedure::HomePicWebp.name(), stored) {
        procedures.push(Procedure::HomePicWebp);
    }
    procedures
}

/// `5.0.0` procedure 的目标值；不需要迁移或已是 WebP 时返回 `None`。
pub fn home_pic_after(stored: &str, current: &str) -> Option<&'static str> {
    if procedures_for(stored).contains(&Procedure::HomePicWebp) && current == "./app/bg.jpg" {
        Some("./app/bg.webp")
    } else {
        None
    }
}

#[derive(Clone, Debug, Default)]
pub struct UpdateReport {
    pub from: String,
    pub to: String,
    pub procedures: Vec<&'static str>,
    pub migrated: bool,
}

static UPDATE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
static CHECKED: AtomicBool = AtomicBool::new(false);

fn update_lock() -> &'static tokio::sync::Mutex<()> {
    UPDATE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 请求期检查：仅在版本落后时升级；每个进程最多检查一次（成功或失败后
/// 不再重试，避免每个请求都打数据库）。并发请求由互斥锁串行化。
pub async fn run_if_needed() -> Result<bool, FrameworkError> {
    if CHECKED.load(Ordering::Acquire) {
        return Ok(false);
    }
    let _guard = update_lock().lock().await;
    if CHECKED.load(Ordering::Acquire) {
        return Ok(false);
    }
    let result = run_unlocked(true).await;
    // 仅当检查成功时置位；数据库临时故障时下个请求可重试。
    if result.is_ok() {
        CHECKED.store(true, Ordering::Release);
    }
    result.map(|report| !report.procedures.is_empty() || report.from != report.to)
}

/// `update` 命令与请求期升级共用的入口；总是执行迁移/清缓存/补锁。
pub async fn run() -> Result<UpdateReport, FrameworkError> {
    let _guard = update_lock().lock().await;
    run_unlocked(false).await
}

async fn run_unlocked(only_if_needed: bool) -> Result<UpdateReport, FrameworkError> {
    let current = VERSION.to_string();
    let stored = options::get("version")
        .await?
        .unwrap_or_else(|| current.clone());
    let procedures = procedures_for(&stored);

    if only_if_needed && procedures.is_empty() && !is_newer(&current, stored.as_str()) {
        return Ok(UpdateReport {
            from: stored,
            to: current,
            procedures: Vec::new(),
            migrated: false,
        });
    }

    for procedure in &procedures {
        match procedure {
            Procedure::Event001 => {
                plugin_api::fire("__0.0.1", json!({}));
            }
            Procedure::HomePicWebp => {
                let home_pic = options::value("home_pic_url").await?;
                if let Some(target) = home_pic_after(&stored, &home_pic) {
                    options::set("home_pic_url", target).await?;
                }
            }
        }
    }

    options::set("version", &current).await?;
    migrate().await?;
    view::clear_cache();
    install::lock()
        .map_err(|error| FrameworkError::internal(format!("无法写入 install.lock: {error}")))?;
    // PHP `Cache::flush()`；缓存驱动未配置时不应阻断升级。
    let _ = Cache::flush().await;

    Ok(UpdateReport {
        from: stored,
        to: current,
        procedures: procedures
            .iter()
            .map(|procedure| procedure.name())
            .collect(),
        migrated: true,
    })
}

async fn migrate() -> Result<(), FrameworkError> {
    let connection = DB::connection()?;
    Migrator::up(connection.inner(), None)
        .await
        .map_err(|error| FrameworkError::database(format!("升级迁移失败: {error}")))
}

/// `storage/options.php` 的路径（`options:cache` 使用）。
pub fn options_cache_path() -> std::path::PathBuf {
    storage::storage_root().join("options.php")
}

#[cfg(test)]
mod tests {
    use super::{Procedure, home_pic_after, is_newer, parse_version, procedures_for, update_lock};

    #[test]
    fn version_comparison_normalizes_patch_suffixes() {
        assert_eq!(parse_version("6.0.11_patch"), Some((6, 0, 11)));
        assert_eq!(parse_version("v5.0.0"), Some((5, 0, 0)));
        assert_eq!(parse_version("not-a-version"), None);
        assert!(is_newer("6.0.12", "6.0.11_patch"));
        assert!(!is_newer("6.0.11_patch", "6.0.11"));
        assert!(!is_newer("6.0.10", "6.0.11_patch"));
        assert!(is_newer("5.0.0", "4.9.9"));
    }

    #[test]
    fn procedures_match_php_less_than_semantics() {
        // 空版本（未安装/未写入）→ 所有 procedure 都执行。
        assert_eq!(
            procedures_for(""),
            vec![Procedure::Event001, Procedure::HomePicWebp]
        );
        // 源：Comparator::lessThan(option('version'), $version)
        assert_eq!(procedures_for("0.0.1"), vec![Procedure::HomePicWebp]);
        assert_eq!(procedures_for("4.9.9"), vec![Procedure::HomePicWebp]);
        // 刚好等于阈值时不执行。
        assert!(procedures_for("5.0.0").is_empty());
        assert!(procedures_for("5.0.1").is_empty());
        assert!(procedures_for("6.0.11_patch").is_empty());
    }

    #[test]
    fn home_pic_procedure_runs_at_most_once() {
        assert_eq!(
            home_pic_after("4.9.9", "./app/bg.jpg"),
            Some("./app/bg.webp")
        );
        // 已经是 WebP，或版本已升级，不再修改。
        assert_eq!(home_pic_after("4.9.9", "./app/bg.webp"), None);
        assert_eq!(home_pic_after("6.0.11_patch", "./app/bg.jpg"), None);
    }

    #[test]
    fn update_lock_is_shared() {
        let first = update_lock() as *const _;
        let second = update_lock() as *const _;
        assert_eq!(first, second);
    }
}
