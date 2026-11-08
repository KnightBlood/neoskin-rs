//! 读取 `options` 表（PHP `option()` 的最小实现）。
//!
//! 数据库尚未写入 `options` 初始数据时，回退到 PHP `config/options.php`
//! 的默认值，保证安装前的行为一致。

use suprnova::{FrameworkError, Model};

use crate::models::option::OptionItem;

/// `config/options.php` 的默认值（顺序与源文件一致）。
pub const DEFAULTS: &[(&str, &str)] = &[
    ("site_url", ""),
    ("site_name", "NeoSkin"),
    ("site_description", "NeoSkin Minecraft Skin Hosting Service"),
    ("register_with_player_name", "true"),
    ("require_verification", "false"),
    ("regs_per_ip", "3"),
    ("announcement", "Welcome to NeoSkin {version}!"),
    ("home_pic_url", "./app/bg.webp"),
    ("custom_css", ""),
    ("custom_js", ""),
    ("player_name_rule", "official"),
    ("custom_player_name_regexp", ""),
    ("player_name_length_min", "3"),
    ("player_name_length_max", "16"),
    ("user_initial_score", "1000"),
    ("sign_gap_time", "24"),
    ("sign_score", "10,100"),
    ("score_per_storage", "true"),
    ("private_score_per_storage", "10"),
    ("return_score", "true"),
    ("score_per_player", "100"),
    ("sign_after_zero", "false"),
    ("version", ""),
    (
        "copyright_text",
        "<b>Copyright &copy; {year} <a href=\"{site_url}\">{site_name}</a>.</b> All rights reserved.",
    ),
    ("auto_del_invalid_texture", "false"),
    ("allow_downloading_texture", "true"),
    ("texture_name_regexp", ""),
    ("cache_expire_time", "31536000"),
    ("max_upload_file_size", "1024"),
    ("force_ssl", "false"),
    ("auto_detect_asset_url", "true"),
    ("plugins_enabled", ""),
    ("copyright_prefer", "0"),
    ("score_per_closet_item", "0"),
    ("favicon_url", "app/favicon.ico"),
    ("score_award_per_texture", "0"),
    ("take_back_scores_after_deletion", "true"),
    ("score_award_per_like", "0"),
    ("meta_keywords", ""),
    ("meta_description", ""),
    ("meta_extras", ""),
    ("cdn_address", ""),
    ("update_source", ""),
    ("plugins_registry", ""),
    ("recaptcha_sitekey", ""),
    ("recaptcha_secretkey", ""),
    ("recaptcha_invisible", "false"),
    ("reporter_score_modification", "0"),
    ("reporter_reward_score", "0"),
    ("content_policy", ""),
    ("transparent_navbar", "false"),
    ("status_code_for_private", "403"),
    ("navbar_color", "cyan"),
    ("sidebar_color", "dark-maroon"),
    ("max_texture_width", "8192"),
];

pub fn default_value(name: &str) -> Option<&'static str> {
    DEFAULTS
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

/// 读取数据库中的值；不存在时返回 `None`。
pub async fn get(name: &str) -> Result<Option<String>, FrameworkError> {
    let row = OptionItem::filter("option_name", name).first().await?;
    Ok(row.map(|item| item.option_value))
}

/// 读取值，数据库缺失时回退到 `config/options.php` 默认值。
pub async fn value(name: &str) -> Result<String, FrameworkError> {
    if let Some(value) = get(name).await? {
        return Ok(value);
    }
    Ok(default_value(name).unwrap_or("").to_string())
}

pub async fn string_or(name: &str, fallback: &str) -> Result<String, FrameworkError> {
    Ok(get(name).await?.unwrap_or_else(|| fallback.to_string()))
}

/// PHP `option_localized($id)`：优先 `<id>_<locale>`，为空时回退 `<id>`。
pub async fn localized(locale: &str, name: &str) -> Result<String, FrameworkError> {
    let key = format!("{name}_{locale}");
    if let Some(value) = get(&key).await?
        && !value.is_empty()
    {
        return Ok(value);
    }
    value(name).await
}

pub async fn int_or(name: &str, fallback: i64) -> Result<i64, FrameworkError> {
    Ok(value(name).await?.trim().parse::<i64>().unwrap_or(fallback))
}

/// PHP `Option::get()` 的布尔语义 + `(int)` 转换：
/// `true`/`false`/`null` 先转成布尔/空，再取整。
pub async fn int_value(name: &str) -> Result<i64, FrameworkError> {
    let raw = value(name).await?;
    Ok(match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "(true)" => 1,
        "false" | "(false)" | "null" | "(null)" => 0,
        other => parse_leading_int(other),
    })
}

fn parse_leading_int(value: &str) -> i64 {
    let trimmed = value.trim();
    let mut end = 0;
    let mut chars = trimmed.char_indices().peekable();
    if let Some((_, sign)) = chars.peek()
        && (*sign == '-' || *sign == '+')
    {
        end = 1;
        chars.next();
    }
    for (index, character) in chars {
        if character.is_ascii_digit() {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    trimmed[..end].parse().unwrap_or(0)
}

/// 写入/更新一个选项（PHP `Option::set`）。
pub async fn set(name: &str, value: &str) -> Result<(), FrameworkError> {
    if let Some(row) = OptionItem::filter("option_name", name).first().await? {
        let value = value.to_string();
        row.update(suprnova::attrs! { option_value: value }).await?;
    } else {
        let name = name.to_string();
        let value = value.to_string();
        OptionItem::create(suprnova::attrs! {
            option_name: name,
            option_value: value,
        })
        .await?;
    }
    Ok(())
}

pub async fn bool_or(name: &str, fallback: bool) -> Result<bool, FrameworkError> {
    let raw = value(name).await?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Ok(true),
        "0" | "false" | "off" | "no" => Ok(false),
        "" => Ok(fallback),
        _ => Ok(fallback),
    }
}

/// 对应 PHP `2021_..._import_options`：首次启动时写入缺失的默认选项。
///
/// 与 PHP 迁移一致，写入时替换 `{version}` / `{year}`；已存在的键不覆盖。
pub async fn seed_defaults() -> Result<(), FrameworkError> {
    let version = crate::controllers::home::VERSION;
    let year = chrono::Utc::now().format("%Y").to_string();
    for (name, raw) in DEFAULTS {
        if get(name).await?.is_some() {
            continue;
        }
        let value = match *name {
            "version" => version.to_string(),
            "announcement" => raw.replace("{version}", version),
            "copyright_text" => raw.replace("{year}", &year),
            _ => (*raw).to_string(),
        };
        if value.is_empty() {
            continue;
        }
        set(name, &value).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DEFAULTS;
    use std::collections::HashSet;

    #[test]
    fn defaults_have_unique_names() {
        let mut seen = HashSet::new();
        for (name, _) in DEFAULTS {
            assert!(seen.insert(*name), "duplicate option {name}");
        }
    }

    #[test]
    fn defaults_include_core_and_late_migration_keys() {
        let names: HashSet<&str> = DEFAULTS.iter().map(|(name, _)| *name).collect();
        for key in [
            "site_name",
            "site_url",
            "version",
            "copyright_text",
            "navbar_color",
            "max_texture_width",
        ] {
            assert!(names.contains(key), "missing option {key}");
        }
    }

    #[test]
    fn text_values_are_serialized_as_strings() {
        assert_eq!(
            DEFAULTS
                .iter()
                .find(|(name, _)| *name == "regs_per_ip")
                .map(|(_, value)| *value),
            Some("3")
        );
    }
}
