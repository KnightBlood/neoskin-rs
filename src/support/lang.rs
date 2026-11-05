//! 运行时语言包：把 `resources/lang/<locale>/front-end.yml` 与数据库
//! `language_lines`（group = front-end）合并，生成为
//! `public/lang/<locale>.js`（`blessing.i18n = {...}`）。
//!
//! PHP 由 `Translations\JavaScript` 在请求时生成并缓存；这里在启动时生成
//! 一次，翻译覆盖更新后重新生成，由静态文件路由提供。

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use suprnova::Model;

use crate::models::language_line::LanguageLine;

pub const DEFAULT_LOCALE: &str = "zh_CN";

fn set_path(root: &mut Value, key: &str, text: &str) {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = root;
    for part in &parts[..parts.len().saturating_sub(1)] {
        if !current.is_object() {
            *current = Value::Object(Default::default());
        }
        let object = current.as_object_mut().expect("object");
        current = object
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    if let Some(last) = parts.last()
        && let Some(object) = current.as_object_mut()
    {
        object.insert((*last).to_string(), Value::String(text.to_string()));
    }
}

async fn overrides() -> HashMap<String, Vec<(String, String)>> {
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    // 注意：`group` 在部分数据库中是保留字，不用它做 WHERE 条件。
    let Ok(lines) = LanguageLine::all().await else {
        return map;
    };
    for line in lines
        .into_vec()
        .into_iter()
        .filter(|line| line.group == "front-end")
    {
        let Ok(translations) = serde_json::from_str::<Value>(&line.text) else {
            continue;
        };
        let Some(object) = translations.as_object() else {
            continue;
        };
        for (locale, value) in object {
            if let Some(text) = value.as_str() {
                map.entry(locale.clone())
                    .or_default()
                    .push((line.key.clone(), text.to_string()));
            }
        }
    }
    map
}

fn write_locale(
    locale: &str,
    yaml_path: &Path,
    out_dir: &Path,
    overrides: &[(String, String)],
) -> Result<(), String> {
    let yaml = std::fs::read_to_string(yaml_path)
        .map_err(|error| format!("读取 {} 失败: {error}", yaml_path.display()))?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml)
        .map_err(|error| format!("解析 {locale} 语言包失败: {error}"))?;
    let mut value: Value = serde_json::to_value(parsed)
        .map_err(|error| format!("转换 {locale} 语言包失败: {error}"))?;
    for (key, text) in overrides {
        set_path(&mut value, key, text);
    }
    let json = serde_json::to_string(&value)
        .map_err(|error| format!("序列化 {locale} 语言包失败: {error}"))?;
    let target = out_dir.join(format!("{locale}.js"));
    std::fs::write(&target, format!("blessing.i18n = {json}"))
        .map_err(|error| format!("写入 {} 失败: {error}", target.display()))?;
    Ok(())
}

/// 生成所有存在的语言文件；忽略单个语言的失败并继续。
pub async fn generate() -> Result<usize, String> {
    let lang_dir = Path::new("resources/lang");
    let out_dir = Path::new("public/lang");
    std::fs::create_dir_all(out_dir).map_err(|error| format!("创建 public/lang 失败: {error}"))?;

    let overrides = overrides().await;
    let mut count = 0;
    let entries = match std::fs::read_dir(lang_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(0),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(locale) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(yaml) = ["front-end.yml", "front-end.yaml"]
            .iter()
            .map(|name| path.join(name))
            .find(|candidate| candidate.is_file())
        else {
            continue;
        };
        let locale_overrides = overrides.get(locale).map(Vec::as_slice).unwrap_or_default();
        if write_locale(locale, &yaml, out_dir, locale_overrides).is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::set_path;

    #[test]
    fn set_path_creates_nested_objects() {
        let mut value = serde_json::json!({"a": {"b": "old"}});
        set_path(&mut value, "a.c", "new");
        assert_eq!(value["a"]["c"], "new");
        set_path(&mut value, "x.y.z", "deep");
        assert_eq!(value["x"]["y"]["z"], "deep");
    }

    #[test]
    fn set_path_overwrites_existing_leaf() {
        let mut value = serde_json::json!({"a": "old"});
        set_path(&mut value, "a", "new");
        assert_eq!(value["a"], "new");
    }
}
