//! 纹理文件存储（对应 PHP `Storage::disk('textures')`）。
//!
//! 根目录优先级：`TEXTURES_DIR` → `NEOSKIN_STORAGE_ROOT/textures` →
//! `storage/textures`。文件名即 SHA-256 十六进制 hash。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use suprnova::FrameworkError;

/// 纹理目录。
pub fn textures_dir() -> PathBuf {
    storage_root().join("textures")
}

/// `NEOSKIN_STORAGE_ROOT` 指定的应用存储根目录（默认 `storage/`）。
pub fn storage_root() -> PathBuf {
    if let Ok(dir) = std::env::var("TEXTURES_DIR")
        && !dir.trim().is_empty()
    {
        // `TEXTURES_DIR` 直接指定纹理目录时，其父目录视为存储根。
        return PathBuf::from(dir)
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("storage"));
    }
    let root = std::env::var("NEOSKIN_STORAGE_ROOT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "storage".to_string());
    PathBuf::from(root)
}

/// `storage/install.lock`：与 PHP `CheckInstallation` 中间件一致。
pub fn install_lock_path() -> PathBuf {
    storage_root().join("install.lock")
}

/// 只接受 SHA-256 十六进制，避免路径穿越与子目录。
pub fn is_valid_hash(hash: &str) -> bool {
    !hash.is_empty()
        && hash.len() <= 128
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-' || byte == b'_')
}

fn path_for(hash: &str) -> Result<PathBuf, FrameworkError> {
    if !is_valid_hash(hash) {
        return Err(FrameworkError::not_found("Not Found"));
    }
    Ok(textures_dir().join(hash))
}

pub fn path(hash: &str) -> Result<PathBuf, FrameworkError> {
    path_for(hash)
}

pub fn exists(hash: &str) -> bool {
    path_for(hash).map(|path| path.is_file()).unwrap_or(false)
}

pub fn read(hash: &str) -> Result<Vec<u8>, FrameworkError> {
    let path = path_for(hash)?;
    std::fs::read(&path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => FrameworkError::not_found("Not Found"),
        _ => FrameworkError::internal(format!("无法读取纹理 {hash}: {error}")),
    })
}

pub fn write(hash: &str, data: &[u8]) -> Result<(), FrameworkError> {
    let path = path_for(hash)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| FrameworkError::internal(format!("无法创建纹理目录: {error}")))?;
    }
    std::fs::write(&path, data)
        .map_err(|error| FrameworkError::internal(format!("无法写入纹理 {hash}: {error}")))
}

/// 删除纹理文件；不存在时视为成功。
pub fn delete(hash: &str) -> Result<(), FrameworkError> {
    let Ok(path) = path_for(hash) else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FrameworkError::internal(format!(
            "无法删除纹理 {hash}: {error}"
        ))),
    }
}

pub fn modified(hash: &str) -> Option<SystemTime> {
    let path = path_for(hash).ok()?;
    std::fs::metadata(path).ok()?.modified().ok()
}

/// HTTP 的 `Last-Modified`（IMF-fixdate，GMT）。
pub fn last_modified(value: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Utc> = value.into();
    datetime.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

pub fn size(hash: &str) -> Option<u64> {
    let path = path_for(hash).ok()?;
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// 测试/调试用：确保目录存在。
pub fn ensure_dir(dir: &Path) -> Result<(), FrameworkError> {
    std::fs::create_dir_all(dir)
        .map_err(|error| FrameworkError::internal(format!("无法创建目录: {error}")))
}

#[cfg(test)]
mod tests {
    use super::is_valid_hash;

    #[test]
    fn accepts_sha256_hex_and_refuses_traversal() {
        assert!(is_valid_hash(
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        ));
        assert!(!is_valid_hash(""));
        assert!(!is_valid_hash("../etc/passwd"));
        assert!(!is_valid_hash("a/b"));
        assert!(!is_valid_hash("a b"));
    }

    #[test]
    fn install_lock_path_ends_with_lock_name() {
        assert!(
            super::install_lock_path()
                .to_string_lossy()
                .ends_with("install.lock")
        );
    }
}
