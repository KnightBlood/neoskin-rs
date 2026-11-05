//! 开发用：写入一个管理员账号（admin@example.com / secret123）。

use neoskin_rs::support::hash::hash_password;
use neoskin_rs::{bootstrap, config};
use sea_orm::ConnectionTrait;

#[suprnova::main]
async fn main() {
    config::register_all();
    bootstrap::register().await;

    let hash = hash_password("secret123", None).expect("hash");
    let now = chrono::Utc::now()
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let connection = suprnova::DB::connection().expect("db");
    let conn = connection.inner();

    conn.execute_unprepared("DELETE FROM users WHERE email = 'admin@example.com'")
        .await
        .expect("delete");

    let sql = format!(
        "INSERT INTO users \
         (email, nickname, score, avatar, password, ip, is_dark_mode, permission, \
          last_sign_at, register_at, verified, verification_token) \
         VALUES ('admin@example.com', 'Admin', 1000, 0, '{hash}', '127.0.0.1', 0, 2, \
                 '{now}', '{now}', 1, '')"
    );
    conn.execute_unprepared(&sql).await.expect("insert");
    println!("seeded admin@example.com / secret123");
}
