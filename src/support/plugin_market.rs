//! 插件市场：注册中心抓取、zip 下载/解压、上传安装。
//!
//! 与 PHP `MarketController` / `PluginController@upload/wget` 对齐：
//! 注册中心返回 `{"packages": [...]}`，插件包为 zip，解压到第一个
//! 插件目录。

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use suprnova::FrameworkError;

use crate::support::options;
use crate::support::plugin;

pub const DEFAULT_REGISTRY: &str = "https://plugin.blessing.dreamcraft.eu.org/registry_{lang}.json";
const MAX_DOWNLOAD_BYTES: usize = 128 * 1024 * 1024;

async fn registry_urls(locale: &str) -> Vec<String> {
    let configured = options::value("plugins_registry").await.unwrap_or_default();
    let raw = if configured.trim().is_empty() {
        std::env::var("PLUGINS_REGISTRY").unwrap_or_else(|_| DEFAULT_REGISTRY.to_string())
    } else {
        configured
    };
    let lang = if matches!(locale, "en" | "zh_CN") {
        locale
    } else {
        "en"
    };
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.replace("{lang}", lang))
        .collect()
}

fn client() -> Result<reqwest::Client, FrameworkError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| FrameworkError::internal(format!("无法初始化 HTTP 客户端: {error}")))
}

/// 拉取所有注册中心的 `packages`，与 PHP `MarketController::fetch` 一致。
pub async fn fetch(locale: &str) -> Result<Vec<Value>, FrameworkError> {
    let client = client()?;
    let mut packages = Vec::new();
    for url in registry_urls(locale).await {
        let response = client.get(&url).send().await.map_err(|error| {
            FrameworkError::internal(format!("无法连接插件市场源 {url}: {error}"))
        })?;
        if !response.status().is_success() {
            return Err(FrameworkError::internal(format!(
                "插件市场源返回 HTTP {}",
                response.status()
            )));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| FrameworkError::internal(format!("解析插件市场响应失败: {error}")))?;
        if let Some(items) = body.get("packages").and_then(Value::as_array) {
            packages.extend(items.iter().cloned());
        }
    }
    Ok(packages)
}

fn is_newer(candidate: &str, installed: &str) -> bool {
    use semver::Version;
    match (Version::parse(candidate), Version::parse(installed)) {
        (Ok(candidate), Ok(installed)) => candidate > installed,
        _ => candidate != installed,
    }
}

/// 为市场条目补充 `installed` / `can_update` / `dependencies`。
pub async fn enrich(packages: Vec<Value>) -> Result<Vec<Value>, FrameworkError> {
    let installed = plugin::all().await?;
    let mut result = Vec::with_capacity(packages.len());
    for mut item in packages {
        let Some(name) = item.get("name").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        let version = item
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let current = installed
            .iter()
            .find(|plugin| plugin.name == name)
            .map(|plugin| plugin.version());
        let dependencies = item
            .get("require")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let unsatisfied = plugin::unsatisfied_map(&item).await;
        if let Some(object) = item.as_object_mut() {
            object.insert(
                "installed".to_string(),
                current
                    .as_ref()
                    .map(|version| Value::String(version.clone()))
                    .unwrap_or(Value::Bool(false)),
            );
            object.insert(
                "can_update".to_string(),
                Value::Bool(
                    current
                        .as_deref()
                        .map(|installed| is_newer(&version, installed))
                        .unwrap_or(false),
                ),
            );
            object.insert(
                "dependencies".to_string(),
                serde_json::json!({
                    "all": dependencies,
                    "unsatisfied": unsatisfied,
                }),
            );
            object.remove("require");
        }
        result.push(item);
    }
    Ok(result)
}

/// 第一个插件目录（不存在时返回默认 `plugins/`）。
pub fn install_dir() -> PathBuf {
    plugin::plugins_dirs()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("plugins"))
}

/// 下载 zip 并解压到插件目录。
pub async fn download_and_install(url: &str) -> Result<(), FrameworkError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(FrameworkError::internal("仅支持 HTTP(S) 下载地址"));
    }
    let client = client()?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| FrameworkError::internal(format!("下载失败: {error}")))?;
    if !response.status().is_success() {
        return Err(FrameworkError::internal(format!(
            "下载失败，HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| FrameworkError::internal(format!("读取下载内容失败: {error}")))?;
    if bytes.len() > MAX_DOWNLOAD_BYTES {
        return Err(FrameworkError::internal("插件包超过大小限制"));
    }
    install_zip(&bytes)
}

/// 解压 zip 到插件目录。
pub fn install_zip(bytes: &[u8]) -> Result<(), FrameworkError> {
    let target = install_dir();
    extract_zip(bytes, &target)
}

fn extract_zip(bytes: &[u8], target: &Path) -> Result<(), FrameworkError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| FrameworkError::internal(format!("无效的 zip 文件: {error}")))?;
    std::fs::create_dir_all(target)
        .map_err(|error| FrameworkError::internal(format!("无法创建插件目录: {error}")))?;
    let canonical_target = target
        .canonicalize()
        .map_err(|error| FrameworkError::internal(format!("无法解析插件目录: {error}")))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| FrameworkError::internal(format!("读取 zip 条目失败: {error}")))?;
        // `enclosed_name` 拒绝绝对路径与 `..`，防止 zip slip。
        let Some(relative) = entry.enclosed_name() else {
            return Err(FrameworkError::internal("插件包包含非法路径"));
        };
        let output = canonical_target.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)
                .map_err(|error| FrameworkError::internal(format!("创建目录失败: {error}")))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| FrameworkError::internal(format!("创建目录失败: {error}")))?;
        }
        let mut buffer = Vec::new();
        entry
            .read_to_end(&mut buffer)
            .map_err(|error| FrameworkError::internal(format!("读取 zip 内容失败: {error}")))?;
        std::fs::write(&output, buffer)
            .map_err(|error| FrameworkError::internal(format!("写入插件文件失败: {error}")))?;
    }
    Ok(())
}

/// 从市场条目中查找 `name`。
pub fn find_package<'a>(packages: &'a [Value], name: &str) -> Option<&'a Value> {
    packages
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
}

/// 市场条目下载地址（`dist.url`）。
pub fn package_url(package: &Value) -> Option<&str> {
    package
        .get("dist")
        .and_then(|dist| dist.get("url"))
        .and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert!(is_newer("1.2.0", "1.1.9"));
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert!(is_newer("2.0.0-beta", "1.9.0"));
        assert!(is_newer("6.0.11_patch", "6.0.10"));
    }

    #[test]
    fn extracts_zip_without_traversal() {
        use std::io::Write;
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("demo/package.json", options).unwrap();
            writer.write_all(br#"{"name":"demo"}"#).unwrap();
            writer.finish().unwrap();
        }
        let target = std::env::temp_dir().join("neoskin-plugin-test-zip");
        let _ = std::fs::remove_dir_all(&target);
        super::extract_zip(buffer.get_ref(), &target).unwrap();
        assert!(target.join("demo/package.json").is_file());
        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn finds_package_metadata() {
        let packages = vec![serde_json::json!({
            "name": "demo",
            "dist": { "url": "https://example.test/demo.zip" }
        })];
        let package = super::find_package(&packages, "demo").expect("package");
        assert_eq!(
            super::package_url(package),
            Some("https://example.test/demo.zip")
        );
        assert!(super::find_package(&packages, "missing").is_none());
    }
}
