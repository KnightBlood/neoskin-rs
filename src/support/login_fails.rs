//! 登录失败计数（PHP：`sha1('login_fails_' + ip)`，Cache 3600 秒）。

use std::time::Duration;

use suprnova::Cache;

const TTL: Duration = Duration::from_secs(3600);

fn key(ip: &str) -> String {
    format!("login_fails:{ip}")
}

pub async fn count(ip: &str) -> i32 {
    Cache::get::<i32>(&key(ip))
        .await
        .ok()
        .flatten()
        .unwrap_or(0)
}

pub async fn record(ip: &str) -> i32 {
    let next = count(ip).await + 1;
    let _ = Cache::put(&key(ip), &next, Some(TTL)).await;
    next
}

pub async fn clear(ip: &str) {
    let _ = Cache::forget(&key(ip)).await;
}
