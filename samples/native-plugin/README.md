# Native plugin example

Minimal ABI v1 plugin used by the `native_plugin` integration test.

```powershell
cargo build --release --manifest-path samples/native-plugin/Cargo.toml
$env:NEOSKIN_NATIVE_PLUGIN='samples/native-plugin/target/release/neoskin_native_plugin_example.dll'
cargo test --test native_plugin -- --nocapture
```

On Linux/macOS the library is `libneoskin_native_plugin_example.so` / `.dylib`.
