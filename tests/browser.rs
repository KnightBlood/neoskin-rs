//! 浏览器页面 E2E：启动真实 Rust 服务 + 临时 SQLite，运行 Playwright flows。
//!
//! 默认跳过；设置 `NEOSKIN_BROWSER_E2E=1` 后执行：
//!
//! ```powershell
//! npm install --prefix tests/browser
//! cargo build
//! $env:NEOSKIN_BROWSER_E2E='1'
//! cargo test --test browser -- --nocapture
//! ```

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use sea_orm::ConnectionTrait;
use tokio::process::Command;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("free port")
        .port()
}

fn has_frontend(root: &Path) -> bool {
    std::fs::read_dir(root.join("public/app"))
        .map(|entries| {
            entries.flatten().any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".js") && !name.ends_with(".map")
            })
        })
        .unwrap_or(false)
}

async fn wait_healthy(base: &str) {
    let client = reqwest::Client::new();
    for _ in 0..120 {
        if let Ok(response) = client.get(format!("{base}/__neoskin/health")).send().await
            && response.status().is_success()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("server did not become healthy: {base}");
}

#[tokio::test]
async fn browser_flows() {
    if std::env::var("NEOSKIN_BROWSER_E2E").ok().as_deref() != Some("1") {
        eprintln!("skipped: set NEOSKIN_BROWSER_E2E=1 to run browser E2E");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !has_frontend(&root) {
        eprintln!("skipped: public/app assets missing (copy from PHP public/app)");
        return;
    }
    if Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| !status.success())
        .unwrap_or(true)
    {
        eprintln!("skipped: node is not installed");
        return;
    }
    if !root
        .join("tests/browser/node_modules/playwright-core")
        .exists()
    {
        eprintln!("skipped: run npm install --prefix tests/browser first");
        return;
    }

    let work = std::env::temp_dir().join(format!(
        "neoskin-rs-browser-{}",
        neoskin_rs::support::id::uuid_v4()
    ));
    std::fs::create_dir_all(&work).expect("work dir");
    let database = work.join("e2e.sqlite");
    let storage = work.join("storage");
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let database_url = format!("sqlite://{}", database.to_string_lossy().replace('\\', "/"));
    let envs = [
        ("APP_ENV", "local".to_string()),
        ("APP_DEBUG", "true".to_string()),
        (
            "APP_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
        ),
        ("APP_URL", base.clone()),
        ("SERVER_HOST", "127.0.0.1".to_string()),
        ("SERVER_PORT", port.to_string()),
        ("SESSION_SECURE", "false".to_string()),
        ("DATABASE_URL", database_url.clone()),
        (
            "NEOSKIN_STORAGE_ROOT",
            storage.to_string_lossy().into_owned(),
        ),
    ];
    let binary = env!("CARGO_BIN_EXE_neoskin_rs");

    let migrate = Command::new(binary)
        .arg("migrate")
        .current_dir(&root)
        .envs(envs.iter().map(|(key, value)| (*key, value.as_str())))
        .output()
        .await
        .expect("run migrate");
    assert!(
        migrate.status.success(),
        "migrate failed: {}",
        String::from_utf8_lossy(&migrate.stderr)
    );

    let connection = sea_orm::Database::connect(&database_url)
        .await
        .expect("connect seed db");
    let hash = neoskin_rs::support::hash::hash_password("secret123", None).expect("hash");
    let now = chrono::Utc::now()
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    connection
        .execute_unprepared(&format!(
            "INSERT INTO users (email, nickname, score, avatar, password, ip, is_dark_mode, \
             permission, last_sign_at, register_at, verified, verification_token) \
             VALUES ('admin@example.com', 'Admin', 1000, 0, '{hash}', '127.0.0.1', 0, 2, \
             '{now}', '{now}', 1, '')"
        ))
        .await
        .expect("seed admin");
    connection.close().await.expect("close seed db");

    let mut server = Command::new(binary)
        .arg("serve")
        .current_dir(&root)
        .envs(envs.iter().map(|(key, value)| (*key, value.as_str())))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start server");
    wait_healthy(&base).await;

    let chrome = std::env::var("NEOSKIN_CHROME_PATH")
        .unwrap_or_else(|_| r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string());
    let output = Command::new("node")
        .arg(root.join("tests/browser/flows.mjs"))
        .current_dir(&root)
        .env("NEOSKIN_BASE_URL", &base)
        .env("NEOSKIN_CHROME_PATH", chrome)
        .output()
        .await
        .expect("run playwright flows");

    let _ = server.kill().await;
    let _ = server.wait().await;
    let _ = std::fs::remove_dir_all(&work);

    assert!(
        output.status.success(),
        "playwright flows failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
