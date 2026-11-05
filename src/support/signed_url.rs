//! 带签名的临时链接（对应 Laravel `URL::signedRoute` / `temporarySignedRoute`）。
//!
//! 格式：`<path>?expires=<unix>[&signature=<hex hmac-sha256>]` 或
//! `<path>?signature=...`。签名体为 `path`（无查询）或 `path?sorted-query`
//! （不含 `signature`），密钥取 `NEOSKIN_APP_KEY` / `APP_KEY`。
//!
//! 与 Laravel 的差异：本端口自行生成和校验，签名算法相同，但不保证与
//! PHP 使用不同 `APP_KEY` 时互通。

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn app_key() -> Option<String> {
    std::env::var("NEOSKIN_APP_KEY")
        .or_else(|_| std::env::var("APP_KEY"))
        .ok()
        .filter(|key| !key.trim().is_empty())
}

fn signature_with(key: &str, payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn canonical_pairs(pairs: &[(String, String)]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = pairs.to_vec();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    pairs
}

fn encode_query(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn sign_with(key: &str, path: &str, expires: Option<i64>) -> String {
    let mut pairs = Vec::new();
    if let Some(expires) = expires {
        pairs.push(("expires".to_string(), expires.to_string()));
    }
    let pairs = canonical_pairs(&pairs);
    let query = encode_query(&pairs);
    let payload = if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    };
    let signature = signature_with(key, &payload);
    let separator = if query.is_empty() { "?" } else { "&" };
    format!("{payload}{separator}signature={signature}")
}

/// 为 `path` 生成带签名（可选过期时间）的 URL 路径。
pub fn sign_path(path: &str, expires: Option<i64>) -> Option<String> {
    app_key().map(|key| sign_with(&key, path, expires))
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((key, value)) => (key.to_string(), value.to_string()),
            None => (part.to_string(), String::new()),
        })
        .collect()
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// 校验当前请求的 `expires` / `signature`。缺少任意一项返回 `false`。
pub fn verify(path: &str, query: Option<&str>) -> bool {
    match app_key() {
        Some(key) => verify_with(&key, path, query),
        None => false,
    }
}

fn verify_with(key: &str, path: &str, query: Option<&str>) -> bool {
    let Some(query) = query else {
        return false;
    };
    let pairs = parse_query(query);
    let Some((_, signature_value)) = pairs.iter().find(|(key, _)| key == "signature") else {
        return false;
    };
    let expires = pairs
        .iter()
        .find(|(key, _)| key == "expires")
        .and_then(|(_, value)| value.parse::<i64>().ok());
    if let Some(expires) = expires
        && expires <= chrono::Utc::now().timestamp()
    {
        return false;
    }
    let rest: Vec<(String, String)> = pairs
        .iter()
        .filter(|(key, _)| key != "signature")
        .cloned()
        .collect();
    let rest = canonical_pairs(&rest);
    let rest_query = encode_query(&rest);
    let payload = if rest_query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{rest_query}")
    };
    constant_time_eq(&signature_with(key, &payload), signature_value)
}

#[cfg(test)]
mod tests {
    use super::{parse_query, sign_with, verify_with};

    const KEY: &str = "test-app-key";

    #[test]
    fn round_trips_with_expiry() {
        let signed = sign_with(KEY, "/auth/reset/7", Some(4_102_444_800));
        let (path, query) = signed.split_once('?').unwrap();
        assert!(verify_with(KEY, path, Some(query)));
    }

    #[test]
    fn rejects_expired_and_tampered() {
        let signed = sign_with(KEY, "/auth/reset/7", Some(1));
        let (path, query) = signed.split_once('?').unwrap();
        assert!(!verify_with(KEY, path, Some(query)));

        let signed = sign_with(KEY, "/auth/reset/7", None);
        let (path, query) = signed.split_once('?').unwrap();
        assert!(verify_with(KEY, path, Some(query)));
        assert!(!verify_with(KEY, "/auth/reset/8", Some(query)));
        assert!(!verify_with("another-key", path, Some(query)));
    }

    #[test]
    fn parses_pair_list() {
        assert_eq!(
            parse_query("expires=1&signature=abc"),
            vec![
                ("expires".to_string(), "1".to_string()),
                ("signature".to_string(), "abc".to_string())
            ]
        );
    }

    #[test]
    fn verifies_links_without_expiry() {
        let signed = sign_with(KEY, "/auth/verify/2", None);
        let (path, query) = signed.split_once('?').unwrap();
        assert!(verify_with(KEY, path, Some(query)));
        assert!(!verify_with(KEY, path, None));
        assert!(!verify_with(KEY, path, Some("expires=9999999999")));
    }
}
