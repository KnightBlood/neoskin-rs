//! 原生插件 ABI v1 的集成测试。
//!
//! 只有设置 `NEOSKIN_NATIVE_PLUGIN`（动态库路径）时才执行；默认跳过，
//! 这样 `cargo test` 在未构建示例插件时仍然全绿。
//!
//! ```powershell
//! cargo build --release --manifest-path samples/native-plugin/Cargo.toml
//! $env:NEOSKIN_NATIVE_PLUGIN='samples/native-plugin/target/release/neoskin_native_plugin_example.dll'
//! cargo test --test native_plugin -- --nocapture
//! ```

use std::path::PathBuf;

use serde_json::json;

#[test]
fn loads_native_plugin_and_applies_filter() {
    let Ok(path) = std::env::var("NEOSKIN_NATIVE_PLUGIN") else {
        eprintln!("skipped: NEOSKIN_NATIVE_PLUGIN is not set");
        return;
    };
    let path = PathBuf::from(path);
    assert!(path.is_file(), "missing plugin library: {}", path.display());

    let name = neoskin_rs::support::native_plugin::load(&path)
        .expect("load native plugin")
        .expect("plugin name");
    assert_eq!(name, "native-example");
    assert!(
        neoskin_rs::support::native_plugin::loaded_names()
            .iter()
            .any(|loaded| loaded == "native-example")
    );

    let value = neoskin_rs::support::plugin_api::apply_filter("native:example", json!("hello"));
    assert_eq!(value, json!("hello!native"));
}
