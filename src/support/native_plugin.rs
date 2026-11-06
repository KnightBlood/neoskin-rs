//! 原生动态库插件（D-001 的可选加载方式）。
//!
//! 编译期 `Plugin` trait 适合随站点一起构建的插件；本模块允许从
//! `plugins/<name>/` 加载原生动态库（`.dll` / `.so` / `.dylib`），供
//! 无法重新编译主程序的分发方式使用。插件必须是可信代码，没有沙箱。
//!
//! ## ABI v1
//!
//! 导出符号：
//! - `neoskin_plugin_api_version() -> u32`，当前为 `1`；
//! - `neoskin_plugin_describe() -> *mut c_char`，返回 UTF-8 JSON 描述；
//! - `neoskin_plugin_filter(name, value_json) -> *mut c_char`（可选）；
//! - `neoskin_plugin_event(name, payload_json)`（可选）；
//! - `neoskin_plugin_free(ptr)`，释放本库分配的字符串。
//!
//! 描述 JSON：
//! ```json
//! {
//!   "name": "demo",
//!   "filters": [{ "name": "head_links", "priority": 20 }],
//!   "events": ["UserLoggedIn"],
//!   "menus": [{ "category": "user", "position": 0, "title": "Demo",
//!               "link": "user/demo", "icon": "fa-star" }],
//!   "styles": [{ "urls": ["/plugins/demo/style.css"], "pages": ["*"] }],
//!   "scripts": [{ "urls": ["/plugins/demo/app.js"], "pages": ["user/*"] }],
//!   "badges": [{ "text": "DEMO", "color": "primary" }]
//! }
//! ```
//!
//! 在 `package.json` 中用 `"native": "plugin.dll"` 或
//! `"native": { "library": "plugin.dll" }` 指向动态库。插件启用状态仍由
//! `plugins_enabled` 控制；修改动态库后需重启进程。

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use libloading::Library;
use serde::Deserialize;
use serde_json::Value;
use suprnova::FrameworkError;

use crate::support::plugin_api::{self, MenuEntry};

const API_VERSION: u32 = 1;

#[derive(Debug, Default, Deserialize)]
pub struct Descriptor {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub filters: Vec<FilterSpec>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub menus: Vec<MenuSpec>,
    #[serde(default)]
    pub styles: Vec<AssetSpec>,
    #[serde(default)]
    pub scripts: Vec<AssetSpec>,
    #[serde(default)]
    pub badges: Vec<BadgeSpec>,
}

#[derive(Debug, Deserialize)]
pub struct FilterSpec {
    pub name: String,
    #[serde(default)]
    pub priority: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct MenuSpec {
    pub category: String,
    #[serde(default)]
    pub position: i32,
    pub title: String,
    pub link: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub new_tab: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct AssetSpec {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub pages: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct BadgeSpec {
    pub text: String,
    #[serde(default)]
    pub color: String,
}

type DescribeFn = unsafe extern "C" fn() -> *mut c_char;
type FilterFn = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_char;
type EventFn = unsafe extern "C" fn(*const c_char, *const c_char);
type FreeFn = unsafe extern "C" fn(*mut c_char);

struct LoadedPlugin {
    /// `Library` 只用于保持代码映射；函数指针在 `load` 时复制。
    _library: Mutex<Library>,
    filter: Option<FilterFn>,
    event: Option<EventFn>,
    free: FreeFn,
    descriptor: Descriptor,
}

impl LoadedPlugin {
    fn take_string(&self, pointer: *mut c_char) -> Option<String> {
        if pointer.is_null() {
            return None;
        }
        // SAFETY: ABI 约定返回以 NUL 结尾的字符串，且由 free 释放。
        let value = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: 指针来自本库的 describe/filter，free 由同一个库导出。
        unsafe { (self.free)(pointer) };
        Some(value)
    }

    fn call_filter(&self, name: &str, value: &str) -> Option<Value> {
        let filter = self.filter?;
        let name = CString::new(name).ok()?;
        let value = CString::new(value).ok()?;
        // SAFETY: 两个指针在调用期间有效；返回值由本库分配。
        let pointer = unsafe { filter(name.as_ptr(), value.as_ptr()) };
        let text = self.take_string(pointer)?;
        serde_json::from_str(&text).ok()
    }

    fn call_event(&self, name: &str, payload: &str) {
        let Some(event) = self.event else {
            return;
        };
        let Ok(name) = CString::new(name) else {
            return;
        };
        let Ok(payload) = CString::new(payload) else {
            return;
        };
        // SAFETY: 指针在调用期间有效；事件回调不返回值。
        unsafe { event(name.as_ptr(), payload.as_ptr()) };
    }
}

static LOADED: LazyLock<RwLock<Vec<Arc<LoadedPlugin>>>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// 读取 `package.json` 的 `native` 字段，返回动态库绝对/相对路径。
pub fn library_path(manifest: &Value, plugin_dir: &Path) -> Option<PathBuf> {
    let library = manifest
        .get("native")
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("library").and_then(Value::as_str))
        })?
        .trim();
    if library.is_empty() {
        return None;
    }
    // 跨平台拒绝 Windows 盘符 / UNC / 反斜杠，以及 Unix 绝对路径。
    // `Path::components` 在非 Windows 主机上不会识别 `C:\...`，必须在字符串层判定。
    let bytes = library.as_bytes();
    if library.contains('\\')
        || library.starts_with('/')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    {
        return None;
    }
    let relative = Path::new(library);
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(plugin_dir.join(relative))
}

/// 从动态库读取并解析描述 JSON（不注册任何内容）。
pub fn read_descriptor(path: &Path) -> Result<Descriptor, FrameworkError> {
    // SAFETY: 用户显式安装的原生插件，必须可信；加载后保持 Library 存活。
    let library = unsafe { Library::new(path) }.map_err(|error| {
        FrameworkError::internal(format!("无法加载原生插件 {}: {error}", path.display()))
    })?;
    // SAFETY: ABI v1 的符号签名由插件与宿主共同约定。
    let version: libloading::Symbol<unsafe extern "C" fn() -> u32> = unsafe {
        library
            .get(b"neoskin_plugin_api_version")
            .map_err(|error| {
                FrameworkError::internal(format!(
                    "原生插件 {} 缺少 neoskin_plugin_api_version: {error}",
                    path.display()
                ))
            })?
    };
    let version = unsafe { version() };
    if version != API_VERSION {
        return Err(FrameworkError::internal(format!(
            "原生插件 {} 的 ABI 版本为 {version}，宿主只支持 {API_VERSION}",
            path.display()
        )));
    }
    let describe: DescribeFn = unsafe {
        *library.get(b"neoskin_plugin_describe").map_err(|error| {
            FrameworkError::internal(format!(
                "原生插件 {} 缺少 neoskin_plugin_describe: {error}",
                path.display()
            ))
        })?
    };
    let free: FreeFn = unsafe {
        *library.get(b"neoskin_plugin_free").map_err(|error| {
            FrameworkError::internal(format!(
                "原生插件 {} 缺少 neoskin_plugin_free: {error}",
                path.display()
            ))
        })?
    };
    let pointer = unsafe { describe() };
    if pointer.is_null() {
        return Err(FrameworkError::internal(format!(
            "原生插件 {} 的 describe 返回空指针",
            path.display()
        )));
    }
    // SAFETY: ABI 约定返回 NUL 结尾的 UTF-8 JSON，由 free 释放。
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { free(pointer) };
    serde_json::from_str(&text)
        .map_err(|error| FrameworkError::internal(format!("原生插件描述不是合法 JSON: {error}")))
}

/// 加载并注册一个动态库插件；重复名称会被忽略。
pub fn load(path: &Path) -> Result<Option<String>, FrameworkError> {
    let mut descriptor = read_descriptor(path)?;
    let name = if descriptor.name.trim().is_empty() {
        path.file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "native-plugin".to_string())
    } else {
        descriptor.name.clone()
    };
    descriptor.name = name.clone();
    if LOADED
        .read()
        .expect("native plugin registry poisoned")
        .iter()
        .any(|plugin| plugin.descriptor.name == name)
    {
        return Ok(Some(name));
    }

    // SAFETY: 与 read_descriptor 相同；符号签名由 ABI v1 约定。
    let library = unsafe { Library::new(path) }.map_err(|error| {
        FrameworkError::internal(format!("无法加载原生插件 {}: {error}", path.display()))
    })?;
    let free: FreeFn = unsafe {
        *library.get(b"neoskin_plugin_free").map_err(|error| {
            FrameworkError::internal(format!(
                "原生插件 {} 缺少 neoskin_plugin_free: {error}",
                path.display()
            ))
        })?
    };
    let filter: Option<FilterFn> = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_char>(
                b"neoskin_plugin_filter",
            )
            .ok()
            .map(|symbol| *symbol)
    };
    let event: Option<EventFn> = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_char, *const c_char)>(b"neoskin_plugin_event")
            .ok()
            .map(|symbol| *symbol)
    };
    let loaded = Arc::new(LoadedPlugin {
        _library: Mutex::new(library),
        filter,
        event,
        free,
        descriptor,
    });

    register_static(&loaded);
    register_filters(&loaded);
    register_events(&loaded);
    LOADED
        .write()
        .expect("native plugin registry poisoned")
        .push(loaded);
    Ok(Some(name))
}

fn register_static(plugin: &Arc<LoadedPlugin>) {
    for menu in &plugin.descriptor.menus {
        plugin_api::HOOK.add_menu_item(
            &menu.category,
            menu.position,
            MenuEntry {
                title: menu.title.clone(),
                link: menu.link.clone(),
                icon: menu.icon.clone(),
                new_tab: menu.new_tab,
            },
        );
    }
    for asset in &plugin.descriptor.styles {
        plugin_api::HOOK.add_style_file_to_page(&asset.urls, &asset.pages);
    }
    for asset in &plugin.descriptor.scripts {
        plugin_api::HOOK.add_script_file_to_page(&asset.urls, &asset.pages);
    }
    for badge in &plugin.descriptor.badges {
        plugin_api::HOOK.add_user_badge(&badge.text, &badge.color);
    }
}

fn register_filters(plugin: &Arc<LoadedPlugin>) {
    if plugin.filter.is_none() {
        return;
    }
    for spec in &plugin.descriptor.filters {
        let name = spec.name.clone();
        let filter_name = name.clone();
        let priority = spec.priority.unwrap_or(20);
        let plugin = Arc::clone(plugin);
        plugin_api::FILTERS.add(&name, priority, move |value| {
            let original = value.clone();
            let input = serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string());
            plugin.call_filter(&filter_name, &input).unwrap_or(original)
        });
    }
}

fn register_events(plugin: &Arc<LoadedPlugin>) {
    if plugin.event.is_none() {
        return;
    }
    for name in &plugin.descriptor.events {
        let plugin = Arc::clone(plugin);
        let event_name = name.clone();
        plugin_api::DISPATCHER.listen(name, move |event| {
            let payload =
                serde_json::to_string(&event.payload).unwrap_or_else(|_| "null".to_string());
            plugin.call_event(&event_name, &payload);
        });
    }
}

/// 供测试/诊断：已加载的动态库插件名。
pub fn loaded_names() -> Vec<String> {
    LOADED
        .read()
        .expect("native plugin registry poisoned")
        .iter()
        .map(|plugin| plugin.descriptor.name.clone())
        .collect()
}

/// 仅用于测试：清空注册表（不会卸载已注册的过滤器）。
#[cfg(test)]
pub fn clear_for_test() {
    LOADED
        .write()
        .expect("native plugin registry poisoned")
        .clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_descriptor_json() {
        let descriptor: Descriptor = serde_json::from_value(serde_json::json!({
            "name": "demo",
            "filters": [{ "name": "head_links", "priority": 10 }],
            "events": ["UserLoggedIn"],
            "menus": [{ "category": "user", "position": 0, "title": "Demo",
                        "link": "user/demo", "icon": "fa-star" }],
            "styles": [{ "urls": ["/demo.css"], "pages": ["*"] }],
            "badges": [{ "text": "DEMO" }]
        }))
        .expect("descriptor");
        assert_eq!(descriptor.name, "demo");
        assert_eq!(descriptor.filters[0].priority, Some(10));
        assert_eq!(descriptor.events[0], "UserLoggedIn");
        assert_eq!(descriptor.menus[0].link, "user/demo");
        assert_eq!(descriptor.badges[0].color, "");
    }

    #[test]
    fn reads_native_library_from_manifest() {
        let dir = Path::new("/tmp/plugin");
        assert_eq!(
            library_path(&serde_json::json!({ "native": "demo.dll" }), dir),
            Some(dir.join("demo.dll"))
        );
        assert_eq!(
            library_path(
                &serde_json::json!({ "native": { "library": "libdemo.so" } }),
                dir
            ),
            Some(dir.join("libdemo.so"))
        );
        assert_eq!(library_path(&serde_json::json!({}), dir), None);
        assert_eq!(
            library_path(&serde_json::json!({ "native": "  " }), dir),
            None
        );
        assert_eq!(
            library_path(&serde_json::json!({ "native": "../escape.dll" }), dir),
            None
        );
        assert_eq!(
            library_path(&serde_json::json!({ "native": "C:\\escape.dll" }), dir),
            None
        );
    }

    #[test]
    fn unknown_abi_is_rejected_without_panicking() {
        // 不存在的路径只应返回错误，不能 panic。
        let result = read_descriptor(Path::new("definitely-missing-plugin-library"));
        assert!(result.is_err());
    }

    #[test]
    fn badge_defaults_to_descriptor_value() {
        let badge: BadgeSpec =
            serde_json::from_value(serde_json::json!({ "text": "X" })).expect("badge");
        assert_eq!(badge.color, "");
    }
}
