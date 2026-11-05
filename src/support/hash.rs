//! PHP/Laravel 兼容的密码与哈希工具。

use std::borrow::Cow;

use sha2::{Digest, Sha256};

/// Laravel 默认 bcrypt cost。
pub const DEFAULT_BCRYPT_COST: u32 = 10;

/// `bcrypt` crate 不识别 PHP 的 `$2y$` 前缀；算法与 `$2a$` 相同，验证前归一化。
pub fn normalize_bcrypt_hash(hash: &str) -> Cow<'_, str> {
    if let Some(rest) = hash.strip_prefix("$2y$") {
        return Cow::Owned(format!("$2a${rest}"));
    }
    Cow::Borrowed(hash)
}

/// 验证 PHP `password_hash()` 生成的密码（支持 `$2y$`/`$2a$`/`$2b$`）。
pub fn verify_password(hash: &str, password: &str) -> bool {
    bcrypt::verify(password, normalize_bcrypt_hash(hash).as_ref()).unwrap_or(false)
}

/// 生成与 PHP 兼容的 bcrypt 哈希（输出 `$2y$` 前缀）。
pub fn hash_password(password: &str, cost: Option<u32>) -> Result<String, bcrypt::BcryptError> {
    let hash = bcrypt::hash(password, cost.unwrap_or(DEFAULT_BCRYPT_COST))?;
    // `bcrypt` 输出 `$2b$`/`$2a$`；统一成 PHP/Laravel 常见的 `$2y$`。
    let body = hash
        .strip_prefix("$2b$")
        .or_else(|| hash.strip_prefix("$2a$"))
        .unwrap_or(&hash);
    Ok(format!("$2y${body}"))
}

/// 纹理 hash：SHA-256 小写十六进制。
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_php_2y_hash() {
        let hash = hash_password("secret123", Some(4)).expect("hash");
        assert!(hash.starts_with("$2y$"));
        assert!(verify_password(&hash, "secret123"));
        assert!(!verify_password(&hash, "wrong"));
    }

    #[test]
    fn accepts_2a_and_2b_prefixes() {
        let hash = hash_password("secret123", Some(4)).expect("hash");
        let as_2a = hash.replacen("$2y$", "$2a$", 1);
        let as_2b = hash.replacen("$2y$", "$2b$", 1);
        assert!(verify_password(&as_2a, "secret123"));
        assert!(verify_password(&as_2b, "secret123"));
    }

    #[test]
    fn sha256_matches_php() {
        assert_eq!(sha256_hex(b"").len(), 64);
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn normalizes_2y_prefix_to_2a() {
        let hash = hash_password("secret123", Some(4)).expect("hash");
        let normalized = normalize_bcrypt_hash(&hash);
        assert!(normalized.starts_with("$2a$"));
        assert!(verify_password(&normalized, "secret123"));
    }

    #[test]
    fn salts_are_random() {
        let first = hash_password("same", Some(4)).expect("hash");
        let second = hash_password("same", Some(4)).expect("hash");
        assert_ne!(first, second);
        assert!(verify_password(&first, "same"));
        assert!(verify_password(&second, "same"));
    }
}
