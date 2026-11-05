//! 最小原生动态库插件示例（ABI v1）。
//!
//! 构建：
//! ```sh
//! cargo build --release --manifest-path samples/native-plugin/Cargo.toml
//! ```
//!
//! 将产物（`target/release/neoskin_native_plugin_example.dll` 或 `.so`）
//! 放到插件目录，并在 `package.json` 中声明：
//! ```json
//! { "name": "native-example", "version": "1.0.0",
//!   "native": "neoskin_native_plugin_example.dll" }
//! ```

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[unsafe(no_mangle)]
pub extern "C" fn neoskin_plugin_api_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn neoskin_plugin_describe() -> *mut c_char {
    let json = r#"{
        "name": "native-example",
        "filters": [{ "name": "native:example", "priority": 20 }],
        "events": ["UserLoggedIn"],
        "badges": [{ "text": "NATIVE", "color": "success" }]
    }"#;
    CString::new(json).expect("no NUL").into_raw()
}

#[unsafe(no_mangle)]
pub extern "C" fn neoskin_plugin_filter(_name: *const c_char, value: *const c_char) -> *mut c_char {
    let input = if value.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned()
    };
    let output = format!("\"{}\"", input.trim_matches('"').to_string() + "!native");
    CString::new(output).expect("no NUL").into_raw()
}

#[unsafe(no_mangle)]
pub extern "C" fn neoskin_plugin_event(_name: *const c_char, _payload: *const c_char) {
    // 示例：真实插件可在这里写日志、更新选项或派发其他事件。
}

#[unsafe(no_mangle)]
pub extern "C" fn neoskin_plugin_free(pointer: *mut c_char) {
    if pointer.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(pointer));
    }
}
