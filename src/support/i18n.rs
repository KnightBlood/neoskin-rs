//! 服务端翻译（PHP `trans()`）：读取 `resources/lang/<locale>/<domain>.yml`。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

fn cache() -> &'static Mutex<HashMap<(String, String), Value>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), Value>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 语言别名（与 `config/locales.php` 对齐）。
pub fn normalize(locale: &str) -> &str {
    match locale {
        "zh_HANS_CN" => "zh_CN",
        "en_US" => "en",
        "ru" => "ru_RU",
        other => other,
    }
}

fn load(locale: &str, domain: &str) -> Option<Value> {
    let locale = normalize(locale).to_string();
    let key = (locale.clone(), domain.to_string());
    if let Ok(cache) = cache().lock()
        && let Some(value) = cache.get(&key)
    {
        return Some(value.clone());
    }
    let path = format!("resources/lang/{locale}/{domain}.yml");
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let value = serde_json::to_value(parsed).ok()?;
    if let Ok(mut cache) = cache().lock() {
        cache.insert(key, value.clone());
    }
    Some(value)
}

fn lookup<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    Some(current)
}

fn interpolate(text: &str, params: Option<&serde_json::Map<String, Value>>) -> String {
    let Some(params) = params else {
        return text.to_string();
    };
    let mut output = text.to_string();
    for (key, value) in params {
        let replacement = match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        output = output.replace(&format!(":{key}"), &replacement);
    }
    output
}

/// 解析 `domain.key.path`，返回原始 JSON 值（数组/对象也可）。
pub fn translate_value(
    locale: &str,
    key: &str,
    params: Option<&serde_json::Map<String, Value>>,
) -> Value {
    let (domain, rest) = key.split_once('.').unwrap_or((key, ""));
    let path: Vec<&str> = rest.split('.').filter(|part| !part.is_empty()).collect();
    let locales = [normalize(locale), "en", "zh_CN"];
    for candidate in locales {
        if let Some(domain_value) = load(candidate, domain)
            && let Some(value) = lookup(&domain_value, &path)
        {
            return match value {
                Value::String(text) => Value::String(interpolate(text, params)),
                other => other.clone(),
            };
        }
    }
    Value::String(key.to_string())
}

/// 字符串形式（非字符串返回 JSON 文本）。
pub fn translate(
    locale: &str,
    key: &str,
    params: Option<&serde_json::Map<String, Value>>,
) -> String {
    match translate_value(locale, key, params) {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_known_key() {
        assert_eq!(translate("zh_CN", "auth.login.title", None), "登录");
    }

    #[test]
    fn interpolates_parameters() {
        let mut params = serde_json::Map::new();
        params.insert("sitename".to_string(), Value::String("Test".to_string()));
        assert!(translate("zh_CN", "auth.register.message", Some(&params)).contains("Test"));
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(normalize("zh_HANS_CN"), "zh_CN");
        assert_eq!(normalize("en_US"), "en");
    }

    #[test]
    fn unknown_key_returns_key() {
        assert_eq!(translate("zh_CN", "no.such.key", None), "no.such.key");
    }

    #[test]
    fn missing_parameter_stays_literal() {
        assert_eq!(
            translate("zh_CN", "auth.register.message", None),
            "欢迎使用 :sitename！"
        );
    }
}
