//! 验证码：本地 SVG 验证码（session 短语）或 reCAPTCHA。
//!
//! PHP 使用 Gregwar 图片验证码；Rust 端使用同语义的 SVG 验证码（随机短语 +
//! 干扰线，session 保存短语），并支持配置 `recaptcha_secretkey` 时走
//! recaptcha.net 校验。

use rand::RngExt;
use serde::Deserialize;
use suprnova::session::{session, session_mut};

use crate::support::options;

pub const CAPTCHA_SESSION_KEY: &str = "neoskin_captcha";
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

fn random_phrase() -> String {
    let mut rng = rand::rng();
    (0..5)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// 生成新短语，写入 session，返回短语（供 SVG 渲染）。
pub fn issue() -> String {
    let phrase = random_phrase();
    let stored = phrase.clone();
    session_mut(|data| data.put(CAPTCHA_SESSION_KEY, stored));
    phrase
}

/// 消费 session 中的短语并与输入比较（不区分大小写）。
pub fn verify_local(input: &str) -> bool {
    let expected: Option<String> = session().and_then(|data| data.get(CAPTCHA_SESSION_KEY));
    session_mut(|data| data.forget(CAPTCHA_SESSION_KEY));
    match expected {
        Some(expected) => expected.eq_ignore_ascii_case(input.trim()),
        None => false,
    }
}

#[derive(Deserialize)]
struct RecaptchaResponse {
    success: bool,
}

/// 校验用户提交的验证码；未配置 reCAPTCHA 时使用本地验证码。
pub async fn verify(input: &str) -> bool {
    let secret = options::get("recaptcha_secretkey")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if secret.trim().is_empty() {
        return verify_local(input);
    }
    if input.trim().is_empty() {
        return false;
    }

    let client = reqwest::Client::new();
    let response = client
        .post("https://www.recaptcha.net/recaptcha/api/siteverify")
        .form(&[("secret", secret.as_str()), ("response", input.trim())])
        .send()
        .await;
    match response {
        Ok(response) => match response.json::<RecaptchaResponse>().await {
            Ok(payload) => payload.success,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

pub fn is_recaptcha_enabled(secret: Option<&str>) -> bool {
    secret
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// 渲染一个简单的 SVG 验证码（浏览器 `<img>` 可直接显示）。
pub fn svg(phrase: &str) -> String {
    let mut rng = rand::rng();
    let mut noise = String::new();
    for _ in 0..6 {
        let x1 = rng.random_range(0..120);
        let y1 = rng.random_range(0..40);
        let x2 = rng.random_range(0..120);
        let y2 = rng.random_range(0..40);
        noise.push_str(&format!(
            r##"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#adb5bd" stroke-width="1"/>"##
        ));
    }
    let chars: Vec<char> = phrase.chars().collect();
    let mut text = String::new();
    for (index, character) in chars.iter().enumerate() {
        let x = 12 + index * 20;
        let y = rng.random_range(26..34);
        let rotate = rng.random_range(-12..12);
        text.push_str(&format!(
            r##"<text x="{x}" y="{y}" font-family="monospace" font-size="22" font-weight="bold" fill="#212529" transform="rotate({rotate} {x} {y})">{character}</text>"##
        ));
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40" viewBox="0 0 120 40"><rect width="120" height="40" fill="#f8f9fa"/>{noise}{text}</svg>"##
    )
}

#[cfg(test)]
mod tests {
    use super::svg;

    #[test]
    fn svg_contains_document_and_phrase() {
        let markup = svg("AB12C");
        assert!(markup.starts_with("<svg"));
        assert!(markup.ends_with("</svg>"));
        assert!(markup.contains(">A</text>"));
        assert!(markup.contains(">C</text>"));
    }
}
