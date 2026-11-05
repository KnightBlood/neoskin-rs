//! 插件清单与开关（PHP `App\Services\PluginManager` / `Plugin`）。
//!
//! 本端口不执行 PHP 插件代码；这里实现插件发现、`package.json` 清单解析、
//! 启用状态持久化、依赖/冲突检查与只读 README。插件静态资源仍由
//! `/plugins/*` 中间件从 `public/plugins` 提供。
//!
//! 目录优先级：`PLUGINS_DIR` / `NEOSKIN_PLUGINS_DIR`（逗号分隔）→
//! `plugins/`。

use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};
use serde_json::Value;
use suprnova::FrameworkError;

use crate::support::options;

const DEFAULT_ICON: &[(&str, &str)] = &[("fa", "plug"), ("faType", "fas"), ("bg", "navy")];

#[derive(Clone, Debug)]
pub struct Plugin {
    pub name: String,
    pub path: PathBuf,
    pub manifest: Value,
    pub enabled: bool,
}

impl Plugin {
    fn attr(&self, key: &str) -> Option<&Value> {
        let mut current = &self.manifest;
        for part in key.split('.') {
            current = current.get(part)?;
        }
        Some(current)
    }

    fn string_attr(&self, key: &str) -> Option<String> {
        self.attr(key).and_then(Value::as_str).map(str::to_string)
    }

    pub fn title(&self) -> String {
        self.string_attr("title")
            .or_else(|| self.string_attr("name"))
            .unwrap_or_else(|| self.name.clone())
    }

    pub fn version(&self) -> String {
        self.string_attr("version").unwrap_or_default()
    }

    pub fn description(&self) -> String {
        self.string_attr("description").unwrap_or_default()
    }

    pub fn readme_path(&self) -> Option<PathBuf> {
        for filename in ["README.md", "readme.md", "README.MD"] {
            let candidate = self.path.join(filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    pub fn has_readme(&self) -> bool {
        self.readme_path().is_some()
    }

    pub fn has_config(&self) -> bool {
        self.attr("enchants.config").is_some()
            || self
                .attr("config")
                .and_then(Value::as_str)
                .map(|filename| self.path.join(filename).is_file())
                .unwrap_or(false)
    }

    pub fn icon(&self) -> Value {
        let mut icon = serde_json::Map::new();
        for (key, value) in DEFAULT_ICON {
            icon.insert((*key).to_string(), Value::String((*value).to_string()));
        }
        if let Some(Value::Object(custom)) = self.attr("enchants.icon") {
            for (key, value) in custom {
                icon.insert(key.clone(), value.clone());
            }
        }
        Value::Object(icon)
    }

    pub fn data(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "title": self.title(),
            "description": self.description(),
            "version": self.version(),
            "enabled": self.enabled,
            "readme": self.has_readme(),
            "config": self.has_config(),
            "icon": self.icon(),
        })
    }
}

/// 插件根目录列表。
pub fn plugins_dirs() -> Vec<PathBuf> {
    let configured = std::env::var("PLUGINS_DIR")
        .or_else(|_| std::env::var("NEOSKIN_PLUGINS_DIR"))
        .ok()
        .filter(|value| !value.trim().is_empty());
    match configured {
        Some(value) => value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect(),
        None => vec![PathBuf::from("plugins")],
    }
}

/// 读取 `plugins_enabled`：JSON 数组 `[{name, version}]`。
pub async fn enabled_state() -> Vec<(String, String)> {
    let raw = options::value("plugins_enabled").await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&raw).unwrap_or(Value::Array(Vec::new()));
    let Some(items) = parsed.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let version = item
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some((name, version))
        })
        .collect()
}

async fn save_enabled(state: &[(String, String)]) -> Result<(), FrameworkError> {
    let items: Vec<Value> = state
        .iter()
        .map(|(name, version)| serde_json::json!({ "name": name, "version": version }))
        .collect();
    options::set("plugins_enabled", &Value::Array(items).to_string()).await
}

/// 扫描所有插件目录，返回按名称排序的插件列表。
pub async fn all() -> Result<Vec<Plugin>, FrameworkError> {
    let enabled = enabled_state().await;
    let mut plugins: Vec<Plugin> = Vec::new();
    for dir in plugins_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("package.json");
            let Ok(contents) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<Value>(&contents) else {
                continue;
            };
            let Some(name) = manifest.get("name").and_then(Value::as_str) else {
                continue;
            };
            if plugins.iter().any(|plugin| plugin.name == name) {
                return Err(FrameworkError::internal(format!(
                    "插件 {name} 在多个目录中重复安装"
                )));
            }
            let version = manifest
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let enabled = enabled.iter().any(|(enabled_name, _)| enabled_name == name);
            plugins.push(Plugin {
                name: name.to_string(),
                path,
                manifest,
                enabled,
            });
            let _ = version;
        }
    }
    plugins.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(plugins)
}

pub fn find<'a>(plugins: &'a [Plugin], name: &str) -> Option<&'a Plugin> {
    plugins.iter().find(|plugin| plugin.name == name)
}

fn version_satisfies(version: &str, constraint: &str) -> bool {
    let (Ok(version), Ok(requirement)) = (Version::parse(version), VersionReq::parse(constraint))
    else {
        // 兼容 `6.0.11_patch` 这类非严格 semver：无法解析时视为满足。
        return true;
    };
    requirement.matches(&version)
}

/// `/admin/plugins/data` 所需的插件数组。
pub async fn data() -> Result<Value, FrameworkError> {
    let plugins = all().await?;
    Ok(Value::Array(
        plugins.iter().map(Plugin::data).collect::<Vec<_>>(),
    ))
}

/// `GET /admin/plugins/readme/{name}` 的内容。
pub async fn readme(name: &str) -> Result<Option<(String, String)>, FrameworkError> {
    let plugins = all().await?;
    let Some(plugin) = find(&plugins, name) else {
        return Ok(None);
    };
    let Some(path) = plugin.readme_path() else {
        return Ok(None);
    };
    let source = std::fs::read_to_string(path)
        .map_err(|error| FrameworkError::internal(format!("无法读取 README: {error}")))?;
    Ok(Some((
        plugin.title(),
        crate::support::communication::markdown(&source),
    )))
}

/// 未满足依赖的映射（PHP `getUnsatisfied` 的 JSON 形状）。
pub async fn unsatisfied_map(manifest: &Value) -> Value {
    let enabled = enabled_state().await;
    let mut map = serde_json::Map::new();
    let Some(requirements) = manifest.get("require").and_then(Value::as_object) else {
        return Value::Object(map);
    };
    for (name, constraint) in requirements {
        let Some(constraint) = constraint.as_str() else {
            continue;
        };
        let version = if name == "blessing-skin-server" {
            Some(crate::controllers::home::VERSION.to_string())
        } else if name == "php" {
            Some("8.3.0".to_string())
        } else {
            enabled
                .iter()
                .find(|(enabled_name, _)| enabled_name == name)
                .map(|(_, version)| version.clone())
        };
        let satisfied = version
            .as_deref()
            .map(|version| version_satisfies(version, constraint))
            .unwrap_or(false);
        if !satisfied {
            map.insert(
                name.clone(),
                serde_json::json!({
                    "version": version,
                    "constraint": constraint,
                }),
            );
        }
    }
    Value::Object(map)
}

/// 未满足的依赖：`(name, version, constraint)`。
async fn unsatisfied(plugin: &Plugin) -> Vec<(String, Option<String>, String)> {
    let enabled = enabled_state().await;
    let mut result = Vec::new();
    let Some(requirements) = plugin.attr("require").and_then(Value::as_object) else {
        return result;
    };
    for (name, constraint) in requirements {
        let Some(constraint) = constraint.as_str() else {
            continue;
        };
        let version = if name == "blessing-skin-server" {
            Some(crate::controllers::home::VERSION.to_string())
        } else if name == "php" {
            Some("8.3.0".to_string())
        } else {
            enabled
                .iter()
                .find(|(enabled_name, _)| enabled_name == name)
                .map(|(_, version)| version.clone())
        };
        let satisfied = match &version {
            Some(version) => version_satisfies(version, constraint),
            None => false,
        };
        if !satisfied {
            result.push((name.clone(), version, constraint.to_string()));
        }
    }
    result
}

/// 冲突：`(name, version, constraint)`。
async fn conflicts(plugin: &Plugin) -> Vec<(String, String, String)> {
    let enabled = enabled_state().await;
    let mut result = Vec::new();
    let Some(conflict) = plugin.attr("enchants.conflicts").and_then(Value::as_object) else {
        return result;
    };
    for (name, constraint) in conflict {
        let Some(constraint) = constraint.as_str() else {
            continue;
        };
        if let Some((_, version)) = enabled
            .iter()
            .find(|(enabled_name, _)| enabled_name == name)
            && version_satisfies(version, constraint)
        {
            result.push((name.clone(), version.clone(), constraint.to_string()));
        }
    }
    result
}

/// 依赖/冲突的本地化文本（PHP `formatUnresolved`）。
pub async fn unresolved_messages(
    plugin: &Plugin,
    locale: &str,
) -> Result<Vec<String>, FrameworkError> {
    let plugins = all().await?;
    let mut messages = Vec::new();
    for (name, version, constraint) in unsatisfied(plugin).await {
        let title = if name == "blessing-skin-server" {
            "Blessing Skin Server".to_string()
        } else if name == "php" {
            "PHP".to_string()
        } else {
            find(&plugins, &name)
                .map(Plugin::title)
                .unwrap_or_else(|| name.clone())
        };
        let mut params = serde_json::Map::new();
        params.insert("title".to_string(), Value::String(title.clone()));
        params.insert("constraint".to_string(), Value::String(constraint.clone()));
        params.insert("name".to_string(), Value::String(title));
        let key = if version.is_some() {
            "admin.plugins.operations.unsatisfied.version"
        } else {
            "admin.plugins.operations.unsatisfied.disabled"
        };
        messages.push(crate::support::i18n::translate(locale, key, Some(&params)));
    }
    for (name, _, _) in conflicts(plugin).await {
        let title = find(&plugins, &name)
            .map(Plugin::title)
            .unwrap_or_else(|| name.clone());
        let mut params = serde_json::Map::new();
        params.insert("title".to_string(), Value::String(title));
        messages.push(crate::support::i18n::translate(
            locale,
            "admin.plugins.operations.unsatisfied.conflict",
            Some(&params),
        ));
    }
    Ok(messages)
}

/// 启用插件；成功返回 `Ok(None)`，依赖/冲突返回消息列表。
pub async fn enable(name: &str, locale: &str) -> Result<Option<Vec<String>>, FrameworkError> {
    let plugins = all().await?;
    let Some(plugin) = find(&plugins, name).cloned() else {
        return Ok(Some(vec![crate::support::i18n::translate(
            locale,
            "admin.plugins.operations.not-found",
            None,
        )]));
    };
    if plugin.enabled {
        return Ok(None);
    }
    let messages = unresolved_messages(&plugin, locale).await?;
    if !messages.is_empty() {
        return Ok(Some(messages));
    }
    let mut state = enabled_state().await;
    state.retain(|(enabled_name, _)| enabled_name != name);
    state.push((plugin.name.clone(), plugin.version()));
    save_enabled(&state).await?;
    Ok(None)
}

pub async fn disable(name: &str) -> Result<(), FrameworkError> {
    let mut state = enabled_state().await;
    state.retain(|(enabled_name, _)| enabled_name != name);
    save_enabled(&state).await
}

pub async fn delete(name: &str) -> Result<bool, FrameworkError> {
    let plugins = all().await?;
    let Some(plugin) = find(&plugins, name) else {
        return Ok(false);
    };
    disable(name).await?;
    std::fs::remove_dir_all(&plugin.path)
        .map_err(|error| FrameworkError::internal(format!("无法删除插件目录: {error}")))?;
    Ok(true)
}

/// 插件目录中是否存在该插件。
pub async fn exists(name: &str) -> bool {
    all()
        .await
        .map(|plugins| find(&plugins, name).is_some())
        .unwrap_or(false)
}

/// 插件资源 URL（`Plugin::assets`）。
pub fn asset_url(name: &str, relative: &str, version: &str) -> String {
    let base = std::env::var("PLUGINS_URL").unwrap_or_else(|_| "/plugins".to_string());
    let base = base.trim_end_matches('/');
    format!("{base}/{}/{relative}?v={version}", urlencode(name))
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// 插件目录是否可写（`delete` 前的诊断用）。
pub fn writable(path: &Path) -> bool {
    path.metadata()
        .map(|meta| !meta.permissions().readonly())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{asset_url, version_satisfies};

    #[test]
    fn checks_semver_requirements() {
        assert!(version_satisfies("1.2.3", "^1.0"));
        assert!(!version_satisfies("2.0.0", "^1.0"));
        assert!(version_satisfies("1.2.3", ">=1.0, <2.0"));
    }

    #[test]
    fn non_semver_versions_do_not_block() {
        assert!(version_satisfies("6.0.11_patch", ">=6.0.0"));
    }

    #[test]
    fn builds_asset_urls() {
        let url = asset_url("demo plugin", "assets/app.js", "1.0.0");
        assert_eq!(url, "/plugins/demo%20plugin/assets/app.js?v=1.0.0");
    }

    #[test]
    fn plugin_data_exposes_manifest_fields() {
        let plugin = super::Plugin {
            name: "demo".to_string(),
            path: std::env::temp_dir().join("neoskin-plugin-missing"),
            manifest: serde_json::json!({
                "name": "demo",
                "title": "Demo",
                "description": "Desc",
                "version": "1.2.3",
                "enchants": { "icon": { "fa": "star", "bg": "green" } }
            }),
            enabled: true,
        };
        let data = plugin.data();
        assert_eq!(data["name"], "demo");
        assert_eq!(data["title"], "Demo");
        assert_eq!(data["description"], "Desc");
        assert_eq!(data["version"], "1.2.3");
        assert_eq!(data["enabled"], true);
        assert_eq!(data["readme"], false);
        assert_eq!(data["config"], false);
        assert_eq!(data["icon"]["fa"], "star");
        assert_eq!(data["icon"]["bg"], "green");
        assert_eq!(data["icon"]["faType"], "fas");
    }

    #[test]
    fn plugin_title_falls_back_to_name() {
        let plugin = super::Plugin {
            name: "fallback".to_string(),
            path: std::env::temp_dir(),
            manifest: serde_json::json!({ "version": "0.1.0" }),
            enabled: false,
        };
        assert_eq!(plugin.title(), "fallback");
        assert_eq!(plugin.version(), "0.1.0");
        assert_eq!(plugin.description(), "");
    }
}
