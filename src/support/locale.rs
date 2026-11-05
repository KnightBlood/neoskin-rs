//! 语言选择（对应 `config/locales.php` 与 `DetectLocale` 中间件）。

use suprnova::Request;

/// (id, 名称, short_name)
pub const LOCALES: &[(&str, &str, &str)] = &[
    ("zh_CN", "中文 (简体)", "zh-CN"),
    ("zh_TW", "中文 (正體)", "zh-TW"),
    ("en", "English", "en"),
    ("es_ES", "Español", "es"),
    ("ru_RU", "Русский язык", "ru"),
];

pub fn is_supported(locale: &str) -> bool {
    LOCALES.iter().any(|(id, _, _)| *id == locale)
}

fn from_accept_language(value: &str) -> Option<String> {
    let tags: Vec<&str> = value
        .split(',')
        .filter_map(|part| {
            let tag = part.split(';').next()?.trim();
            (!tag.is_empty()).then_some(tag)
        })
        .collect();
    // 先匹配完整标签（如 `zh-TW` → `zh_TW`），再回退到语言前缀。
    for tag in &tags {
        let normalized = tag.replace('-', "_");
        if let Some((id, _, _)) = LOCALES.iter().find(|(id, _, _)| *id == normalized) {
            return Some((*id).to_string());
        }
    }
    for tag in &tags {
        let language = tag.split('-').next().unwrap_or("");
        if let Some((id, _, _)) = LOCALES
            .iter()
            .find(|(id, _, _)| id.split('_').next() == Some(language))
        {
            return Some((*id).to_string());
        }
    }
    None
}

/// 解析请求语言：`?lang=` → session → Accept-Language → `zh_CN`。
pub fn resolve(request: &Request) -> String {
    if let Some(lang) = request.query_param("lang")
        && is_supported(&lang)
    {
        suprnova::session_mut(|data| data.put("locale", lang.clone()));
        return lang;
    }
    if let Some(locale) = suprnova::session().and_then(|data| data.get::<String>("locale"))
        && is_supported(&locale)
    {
        return locale;
    }
    if let Some(header) = request.header("accept-language")
        && let Some(locale) = from_accept_language(header)
    {
        return locale;
    }
    "zh_CN".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_accept_language_variants() {
        assert_eq!(
            from_accept_language("en-US,en;q=0.9"),
            Some("en".to_string())
        );
        assert_eq!(
            from_accept_language("zh-TW;q=0.8"),
            Some("zh_TW".to_string())
        );
        assert_eq!(from_accept_language("xx-YY"), None);
    }

    #[test]
    fn supported_list_matches_config() {
        assert!(is_supported("zh_CN"));
        assert!(is_supported("ru_RU"));
        assert!(!is_supported("ru"));
    }

    #[test]
    fn matches_language_prefix_when_region_is_unknown() {
        assert_eq!(
            from_accept_language("en-GB,en;q=0.9"),
            Some("en".to_string())
        );
        assert_eq!(
            from_accept_language("ru-RU,ru;q=0.8"),
            Some("ru_RU".to_string())
        );
        assert_eq!(
            from_accept_language("es-MX,es;q=0.8"),
            Some("es_ES".to_string())
        );
    }

    #[test]
    fn ignores_unknown_languages() {
        assert_eq!(from_accept_language("fr-FR,de;q=0.9"), None);
        assert_eq!(from_accept_language(""), None);
    }
}
