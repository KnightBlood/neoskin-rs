//! 「记住我」cookie（与 Go 端口相同的自定义格式）。
//!
//! PHP/Laravel 的 recaller cookie 会被 `EncryptCookies` 整体加密，Rust 端不复
//! 刻该对称加密层；值格式为 `uid|token|hmac(sha256, password_hash, APP_KEY)`，
//! 语义与 Laravel 的三段校验等价（token 对应 `remember_token`，第三段使改密后
//! 的旧 cookie 立即失效）。
//!
//! cookie 名与 Laravel 默认一致：
//! `remember_web_59ba36addc2b2f9401580f014c7f58ea4e30989d`。

use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use rand::RngExt;
use rand::distr::Alphanumeric;
use sea_orm::ConnectionTrait;
use sha2::Sha256;
use suprnova::{Cookie, DB, FrameworkError, SameSite};

use crate::models::user::User;

pub const REMEMBER_COOKIE: &str = "remember_web_59ba36addc2b2f9401580f014c7f58ea4e30989d";
/// 5 年（与 Laravel `remember` cookie 一致）。
pub const REMEMBER_LIFETIME: Duration = Duration::from_secs(5 * 365 * 24 * 60 * 60);

type HmacSha256 = Hmac<Sha256>;

fn app_key() -> Result<String, FrameworkError> {
    std::env::var("NEOSKIN_APP_KEY")
        .or_else(|_| std::env::var("APP_KEY"))
        .map_err(|_| FrameworkError::internal("APP_KEY is not configured"))
}

fn password_mac(password_hash: &str) -> Result<String, FrameworkError> {
    let key = app_key()?;
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .map_err(|_| FrameworkError::internal("invalid APP_KEY"))?;
    mac.update(password_hash.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// 生成 60 位随机 token（等价 Laravel `Str::random(60)`）。
pub fn random_token() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(60)
        .map(char::from)
        .collect()
}

/// 写入新的 remember_token（仅字母数字，直接内联安全）。
pub async fn store_token(uid: u32, token: &str) -> Result<(), FrameworkError> {
    let connection = DB::connection()?;
    let sql = format!(
        "UPDATE users SET remember_token = '{}' WHERE uid = {}",
        token, uid
    );
    connection
        .inner()
        .execute_unprepared(&sql)
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    Ok(())
}

/// cookie 值：`uid|token|mac`。
pub fn cookie_value(user: &User, token: &str) -> Result<String, FrameworkError> {
    let mac = password_mac(&user.password)?;
    Ok(format!("{}|{}|{}", user.uid, token, mac))
}

pub fn parse_cookie(value: &str) -> Option<(u32, &str, &str)> {
    let mut parts = value.split('|');
    let uid = parts.next()?.parse::<u32>().ok()?;
    let token = parts.next()?;
    let mac = parts.next()?;
    if parts.next().is_some() || token.is_empty() || mac.is_empty() {
        return None;
    }
    Some((uid, token, mac))
}

/// 校验 token 与 HMAC（常量时间比较）。
pub fn verify(user: &User, token: &str, mac: &str) -> bool {
    if user.remember_token.as_deref() != Some(token) {
        return false;
    }
    let Ok(key) = app_key() else {
        return false;
    };
    let Ok(mut verifier) = HmacSha256::new_from_slice(key.as_bytes()) else {
        return false;
    };
    verifier.update(user.password.as_bytes());
    let Ok(provided) = hex::decode(mac) else {
        return false;
    };
    verifier.verify_slice(&provided).is_ok()
}

/// 构造下发的记住我 cookie。
pub fn cookie(value: String, secure: bool) -> Cookie {
    Cookie::new(REMEMBER_COOKIE, value)
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(REMEMBER_LIFETIME)
}

/// 删除记住我 cookie。
pub fn forget_cookie() -> Cookie {
    Cookie::forget(REMEMBER_COOKIE).path("/")
}

#[cfg(test)]
mod tests {
    use super::parse_cookie;

    #[test]
    fn parses_three_part_cookie() {
        assert_eq!(parse_cookie("7|token|mac"), Some((7, "token", "mac")));
    }

    #[test]
    fn rejects_malformed_cookie() {
        assert_eq!(parse_cookie("7|token"), None);
        assert_eq!(parse_cookie("7|token|mac|extra"), None);
        assert_eq!(parse_cookie("x|token|mac"), None);
        assert_eq!(parse_cookie("7||mac"), None);
    }
}
