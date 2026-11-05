//! 系统更新：检查、运行包暂存与安装。
//!
//! PHP 的 patch 包不能直接覆盖 Rust 文件；本端口定义独立的运行包格式：
//!
//! ```text
//! manifest.json          {"runtime":"neoskin-rs","version":"6.0.12","platform":"any"}
//! payload/...            与站点根目录同构的文件
//! ```
//!
//! `download`/`upload` 只负责校验并暂存；`install` 才把 `payload/` 写入站点
//! 根目录。正在运行的二进制无法在 Windows 上覆盖，会写成 `*.new` 并提示重启。
//! 包是可信发布物，不做签名校验；有 `sha256` 时校验整包。

use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use suprnova::FrameworkError;

use crate::support::options;

pub const SPEC: i64 = 2;
pub const DEFAULT_SOURCE: &str = "https://skin.blessing.dreamcraft.eu.org/update.json";
const RUNTIME_PHP_VERSION: &str = "8.3.0";
const MAX_PACKAGE_BYTES: usize = 256 * 1024 * 1024;

/// 更新检测结果。
pub struct Check {
    pub info: Value,
    pub can_update: bool,
    pub error: String,
}

/// 运行包 manifest。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub runtime: String,
    pub version: String,
    #[serde(default)]
    pub platform: String,
}

/// 已暂存、等待安装的包。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pending {
    pub id: String,
    pub version: String,
    pub platform: String,
    pub source: String,
    pub sha256: String,
    pub created_at: String,
}

/// 安装结果。
#[derive(Clone, Debug)]
pub struct InstallReport {
    pub version: String,
    pub files: usize,
    pub binary_updated: bool,
}

impl InstallReport {
    pub fn to_json(&self) -> Value {
        json!({
            "version": self.version,
            "files": self.files,
            "restart_required": self.binary_updated,
        })
    }
}

/// 当前平台标记，例如 `windows-x86_64`。
pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// 站点根目录；测试可通过 `NEOSKIN_BASE_PATH` 覆盖。
pub fn base_dir() -> PathBuf {
    std::env::var("NEOSKIN_BASE_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn update_dir() -> PathBuf {
    crate::support::storage::storage_root().join("framework/update")
}

fn pending_path() -> PathBuf {
    update_dir().join("pending.json")
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

fn platform_matches(manifest: &Manifest) -> bool {
    let platform = manifest.platform.trim();
    platform.is_empty() || platform.eq_ignore_ascii_case("any") || platform == current_platform()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
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
    let runtime = body
        .get("runtime")
        .and_then(Value::as_str)
        .unwrap_or("php")
        .to_string();
    let platform = body
        .get("platform")
        .and_then(Value::as_str)
        .unwrap_or("any")
        .to_string();

    let mut error = String::new();
    let can_update = if runtime != "neoskin-rs" {
        error = "更新源提供的是 PHP 运行包；Rust 端口请配置 NeoSkinRS 更新源。".to_string();
        false
    } else if !platform.is_empty()
        && !platform.eq_ignore_ascii_case("any")
        && platform != current_platform()
    {
        error = format!("运行包平台为 {platform}，当前为 {}。", current_platform());
        false
    } else if !php.is_empty() && is_newer(&php, RUNTIME_PHP_VERSION) {
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
            "runtime": runtime,
            "platform": platform,
            "url": body.get("url").cloned().unwrap_or(Value::Null),
            "package": body.get("package").cloned().unwrap_or(Value::Null),
            "sha256": body.get("sha256").cloned().unwrap_or(Value::Null),
        }),
        can_update,
        error,
    })
}

fn read_manifest(bytes: &[u8]) -> Result<Manifest, FrameworkError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| FrameworkError::internal(format!("无效的运行包: {error}")))?;
    let mut manifest = None;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| FrameworkError::internal(format!("读取运行包失败: {error}")))?;
        if entry.name() == "manifest.json" {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|error| {
                FrameworkError::internal(format!("读取 manifest 失败: {error}"))
            })?;
            manifest = Some(serde_json::from_str::<Manifest>(&text).map_err(|error| {
                FrameworkError::internal(format!("manifest 格式错误: {error}"))
            })?);
            break;
        }
    }
    let manifest = manifest.ok_or_else(|| FrameworkError::internal("运行包缺少 manifest.json"))?;
    if manifest.runtime != "neoskin-rs" {
        return Err(FrameworkError::internal(format!(
            "运行包 runtime 为 {}，不是 neoskin-rs",
            manifest.runtime
        )));
    }
    if manifest.version.trim().is_empty() {
        return Err(FrameworkError::internal("运行包 manifest 缺少 version"));
    }
    if !platform_matches(&manifest) {
        return Err(FrameworkError::internal(format!(
            "运行包平台为 {}，当前为 {}",
            manifest.platform,
            current_platform()
        )));
    }
    // 暂存阶段就校验所有路径，避免安装时才失败。
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| FrameworkError::internal(format!("无效的运行包: {error}")))?;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| FrameworkError::internal(format!("读取运行包失败: {error}")))?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            return Err(FrameworkError::internal("运行包包含非法路径"));
        };
        if path != Path::new("manifest.json") {
            validate_payload_path(&path)?;
        }
    }
    Ok(manifest)
}

fn validate_payload_path(path: &Path) -> Result<PathBuf, FrameworkError> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(part)) if part == "payload" => {}
        _ => return Err(FrameworkError::internal("运行包文件必须位于 payload/ 下")),
    }
    let relative: PathBuf = components.collect();
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(FrameworkError::internal("运行包包含非法路径"));
    }
    let first = relative
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();
    if relative == Path::new(".env")
        || first == "storage"
        || first == "plugins"
        || first == "database.sqlite"
        || first.starts_with("database.sqlite")
    {
        return Err(FrameworkError::internal(format!(
            "运行包不允许覆盖 {}",
            relative.display()
        )));
    }
    Ok(relative)
}

/// 暂存 ZIP 到默认目录。
pub fn stage_bytes(bytes: &[u8], source: &str) -> Result<Pending, FrameworkError> {
    stage_bytes_at(&update_dir(), bytes, source)
}

fn stage_bytes_at(directory: &Path, bytes: &[u8], source: &str) -> Result<Pending, FrameworkError> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(FrameworkError::internal("运行包超过 256 MB 限制"));
    }
    let manifest = read_manifest(bytes)?;
    let pending = Pending {
        id: crate::support::id::uuid_v4(),
        version: manifest.version,
        platform: manifest.platform,
        source: source.to_string(),
        sha256: sha256_hex(bytes),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    std::fs::create_dir_all(directory)
        .map_err(|error| FrameworkError::internal(format!("无法创建更新目录: {error}")))?;
    std::fs::write(directory.join("package.zip"), bytes)
        .map_err(|error| FrameworkError::internal(format!("无法写入运行包: {error}")))?;
    std::fs::write(
        directory.join("pending.json"),
        serde_json::to_vec_pretty(&pending)
            .map_err(|error| FrameworkError::internal(format!("无法写入更新状态: {error}")))?,
    )
    .map_err(|error| FrameworkError::internal(format!("无法写入更新状态: {error}")))?;
    Ok(pending)
}

/// 当前已暂存包的信息。
pub fn pending() -> Option<Pending> {
    let text = std::fs::read_to_string(pending_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// 下载更新源中的运行包并暂存。
pub async fn download_and_stage() -> Result<Pending, FrameworkError> {
    let check = check().await?;
    if !check.can_update {
        return Err(FrameworkError::internal(if check.error.is_empty() {
            "当前已是最新版本".to_string()
        } else {
            check.error
        }));
    }
    let url = check
        .info
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| FrameworkError::internal("更新源缺少运行包 URL"))?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(FrameworkError::internal("运行包 URL 必须是 HTTP(S)"));
    }
    let expected = check
        .info
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| FrameworkError::internal(format!("无法初始化 HTTP 客户端: {error}")))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| FrameworkError::internal(format!("下载运行包失败: {error}")))?;
    if !response.status().is_success() {
        return Err(FrameworkError::internal(format!(
            "下载运行包失败，HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| FrameworkError::internal(format!("读取运行包失败: {error}")))?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(FrameworkError::internal("运行包超过 256 MB 限制"));
    }
    if !expected.is_empty() && expected != sha256_hex(&bytes) {
        return Err(FrameworkError::internal("运行包 SHA-256 校验失败"));
    }
    stage_bytes(&bytes, "download")
}

/// 上传入口：先暂存，等待管理员确认安装。
pub fn upload_and_stage(bytes: &[u8]) -> Result<Pending, FrameworkError> {
    stage_bytes(bytes, "upload")
}

/// 安装已暂存的运行包到 `root`；默认站点根目录。
pub fn install() -> Result<InstallReport, FrameworkError> {
    install_at(&base_dir(), &update_dir())
}

fn install_at(root: &Path, directory: &Path) -> Result<InstallReport, FrameworkError> {
    let pending = {
        let text = std::fs::read_to_string(directory.join("pending.json"))
            .map_err(|_| FrameworkError::internal("没有待安装的运行包"))?;
        serde_json::from_str::<Pending>(&text)
            .map_err(|error| FrameworkError::internal(format!("待安装状态损坏: {error}")))?
    };
    let bytes = std::fs::read(directory.join("package.zip"))
        .map_err(|_| FrameworkError::internal("没有待安装的运行包"))?;
    if sha256_hex(&bytes) != pending.sha256 {
        return Err(FrameworkError::internal("待安装运行包 SHA-256 不匹配"));
    }
    let manifest = read_manifest(&bytes)?;
    if manifest.version != pending.version {
        return Err(FrameworkError::internal("待安装运行包版本与状态不一致"));
    }

    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes))
        .map_err(|error| FrameworkError::internal(format!("无效的运行包: {error}")))?;
    let mut files = 0usize;
    let mut binary_updated = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| FrameworkError::internal(format!("读取运行包失败: {error}")))?;
        let Some(path) = entry.enclosed_name() else {
            return Err(FrameworkError::internal("运行包包含非法路径"));
        };
        if entry.is_dir() {
            continue;
        }
        if path == Path::new("manifest.json") {
            continue;
        }
        let relative = validate_payload_path(&path)?;
        let target = root.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| FrameworkError::internal(format!("创建目录失败: {error}")))?;
        }
        let existing = target.canonicalize().ok();
        let is_running = current_exe.is_some() && existing == current_exe;
        let output = if is_running {
            binary_updated = true;
            let mut name = target
                .file_name()
                .map(|value| value.to_os_string())
                .unwrap_or_default();
            name.push(".new");
            target.with_file_name(name)
        } else {
            target.clone()
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| FrameworkError::internal(format!("读取运行包文件失败: {error}")))?;
        std::fs::write(&output, bytes).map_err(|error| {
            FrameworkError::internal(format!("写入 {} 失败: {error}", output.display()))
        })?;
        files += 1;
    }

    std::fs::remove_file(directory.join("package.zip")).ok();
    std::fs::remove_file(directory.join("pending.json")).ok();
    Ok(InstallReport {
        version: pending.version,
        files,
        binary_updated,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn package(manifest: Value, files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("manifest.json", options).unwrap();
            writer
                .write_all(serde_json::to_string(&manifest).unwrap().as_bytes())
                .unwrap();
            for (path, contents) in files {
                writer.start_file(path, options).unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn temp(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "neoskin-update-{name}-{}",
            crate::support::id::uuid_v4()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

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

    #[test]
    fn stages_and_installs_runtime_package() {
        let storage = temp("storage");
        let root = temp("root");
        let bytes = package(
            json!({ "runtime": "neoskin-rs", "version": "6.0.12", "platform": "any" }),
            &[("payload/resources/app.txt", b"hello")],
        );
        let pending = stage_bytes_at(&storage, &bytes, "test").expect("stage");
        assert_eq!(pending.version, "6.0.12");
        let report = install_at(&root, &storage).expect("install");
        assert_eq!(report.files, 1);
        assert_eq!(
            std::fs::read_to_string(root.join("resources/app.txt")).unwrap(),
            "hello"
        );
        assert!(!storage.join("pending.json").exists());
        let _ = std::fs::remove_dir_all(&storage);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_traversal_and_unsafe_paths() {
        let storage = temp("storage-traversal");
        let traversal = package(
            json!({ "runtime": "neoskin-rs", "version": "6.0.12", "platform": "any" }),
            &[("payload/../evil.txt", b"bad")],
        );
        assert!(stage_bytes_at(&storage, &traversal, "test").is_err());
        let env = package(
            json!({ "runtime": "neoskin-rs", "version": "6.0.12", "platform": "any" }),
            &[("payload/.env", b"bad")],
        );
        assert!(stage_bytes_at(&storage, &env, "test").is_err());
        let _ = std::fs::remove_dir_all(&storage);
    }

    #[test]
    fn rejects_wrong_runtime_and_platform() {
        let storage = temp("storage-runtime");
        let wrong_runtime = package(
            json!({ "runtime": "php", "version": "6.0.12", "platform": "any" }),
            &[("payload/a.txt", b"x")],
        );
        assert!(stage_bytes_at(&storage, &wrong_runtime, "test").is_err());
        let wrong_platform = package(
            json!({ "runtime": "neoskin-rs", "version": "6.0.12", "platform": "plan9-x86_64" }),
            &[("payload/a.txt", b"x")],
        );
        assert!(stage_bytes_at(&storage, &wrong_platform, "test").is_err());
        let _ = std::fs::remove_dir_all(&storage);
    }

    #[test]
    fn rejects_tampered_pending_package() {
        let storage = temp("storage-tampered");
        let root = temp("root-tampered");
        let bytes = package(
            json!({ "runtime": "neoskin-rs", "version": "6.0.12", "platform": "any" }),
            &[("payload/a.txt", b"x")],
        );
        stage_bytes_at(&storage, &bytes, "test").expect("stage");
        std::fs::write(storage.join("package.zip"), b"tampered").unwrap();
        assert!(install_at(&root, &storage).is_err());
        let _ = std::fs::remove_dir_all(&storage);
        let _ = std::fs::remove_dir_all(&root);
    }
}
