//! 系统更新检测（PHP `UpdateController@getUpdateInfo/canUpdate`）。
//!
//! 只实现「检查更新」：下载/安装运行包仍由 `admin/update/*` 返回占位响应，
//! 因为 Rust 端口没有与 PHP patch 包兼容的运行包格式。

use std::time::Duration;

use semver::Version;
use serde_json::{Value, json};
use suprnova::FrameworkError;

use crate::support::options;

pub const SPEC: i64 = 2;
pub const DEFAULT_SOURCE: &str = "https://skin.blessing.dreamcraft.eu.org/update.json";
const RUNTIME_PHP_VERSION: &str = "8.3.0";

/// 更新检测结果。
pub struct Check {
    pub info: Value,
    pub can_update: bool,
    pub error: String,
}

/// 更新源：`update_source` 选项 → `UPDATE_SOURCE` 环境变量 → 默认地址。
pub async fn source_url() -> String {
    let configured = options::value("update_source").await.unwrap_or_default();
    if !configured.trim().is_empty() {
        return configured;
    }
    std::env::var("UPDATE_SOURCE").unwrap_or_else(|_| DEFAULT_SOURCE.to_string())
}

fn parse_version(value: &str) -> Option<Version> {
    if let Ok(version) = Version::parse(value) {
        return Some(version);
    }
    // 兼容 `6.0.11_patch` 这类带补丁后缀的版本。
    let numeric: String = value
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    Version::parse(&numeric).ok()
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => latest != current,
    }
}

/// 拉取更新源并判断是否可更新。
pub async fn check() -> Result<Check, FrameworkError> {
    let current = crate::controllers::home::VERSION;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| FrameworkError::internal(format!("无法初始化 HTTP 客户端: {error}")))?;
    let url = source_url().await;
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(_) => {
            return Ok(Check {
                info: json!({ "latest": current, "current": current }),
                can_update: false,
                error: "更新源连接失败".to_string(),
            });
        }
    };
    if !response.status().is_success() {
        return Ok(Check {
            info: json!({ "latest": current, "current": current }),
            can_update: false,
            error: format!("HTTP status code: {}", response.status()),
        });
    }
    let body: Value = match response.json().await {
        Ok(body) => body,
        Err(_) => {
            return Ok(Check {
                info: json!({ "latest": current, "current": current }),
                can_update: false,
                error: "更新源连接失败".to_string(),
            });
        }
    };
    if body.get("spec").and_then(Value::as_i64) != Some(SPEC) {
        return Ok(Check {
            info: json!({ "latest": current, "current": current }),
            can_update: false,
            error: "不支持当前的更新源。".to_string(),
        });
    }
    let latest = body
        .get("latest")
        .and_then(Value::as_str)
        .unwrap_or(current)
        .to_string();
    let php = body
        .get("php")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut error = String::new();
    let can_update = if !php.is_empty() && is_newer(&php, RUNTIME_PHP_VERSION) {
        error = format!("PHP 版本过低，至少需要 {php}。");
        false
    } else {
        is_newer(&latest, current)
    };

    Ok(Check {
        info: json!({
            "latest": latest,
            "current": current,
            "php": php,
            "url": body.get("url").cloned().unwrap_or(Value::Null),
            "package": body.get("package").cloned().unwrap_or(Value::Null),
            "sha256": body.get("sha256").cloned().unwrap_or(Value::Null),
        }),
        can_update,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert!(is_newer("6.0.12", "6.0.11_patch"));
        assert!(!is_newer("6.0.10", "6.0.11_patch"));
        assert!(!is_newer("6.0.11", "6.0.11_patch"));
    }

    #[test]
    fn parses_patch_suffixes() {
        let parsed = super::parse_version("6.0.11_patch").expect("version");
        assert_eq!(parsed.to_string(), "6.0.11");
        assert!(super::parse_version("not-a-version").is_none());
    }
}
