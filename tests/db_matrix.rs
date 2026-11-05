//! 数据库矩阵集成测试（对齐 Go `TestMatrix` / .NET `DatabaseMatrixTest` /
//! Spring `MatrixSpringBootTest`）。
//!
//! 仅当 `DB_CONNECTION` 为 `mysql|mariadb|pgsql` 时执行；默认 `cargo test`
//! 会直接跳过。`DATABASE_URL` 优先，其次按 `DB_*` 拼 URL。
//!
//! 覆盖：幂等迁移 → MySQL/MariaDB 无符号 DDL 校验 → 用户插入/查询 →
//! `register_at` 的 PHP 时间格式 → 纹理插入/计数 → 清理。不会 DropAll，
//! 避免破坏共享库。
//!
//! 本机示例：
//! ```powershell
//! $env:DB_CONNECTION='mysql'; $env:DB_PORT='3307'
//! $env:DB_USERNAME='blessing'; $env:DB_PASSWORD='123456'
//! $env:DB_DATABASE='blessing_skin'
//! cargo test --test db_matrix -- --nocapture
//! ```

use std::error::Error;

use chrono::{Duration, NaiveDateTime, Utc};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement, Value};
use sea_orm_migration::MigratorTrait;

use neoskin_rs::migrations::Migrator;
use neoskin_rs::support::hash::hash_password;
use neoskin_rs::support::remember::random_token;

struct MatrixConfig {
    connection: String,
    backend: DbBackend,
    url: String,
}

fn env_or(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn matrix_config() -> Option<MatrixConfig> {
    let connection = std::env::var("DB_CONNECTION").ok()?;
    let connection = connection.trim().to_ascii_lowercase();
    let (default_port, default_user, scheme) = match connection.as_str() {
        "mysql" => ("3306", "blessing", "mysql"),
        "mariadb" => ("3306", "blessing", "mysql"),
        "pgsql" | "postgres" | "postgresql" => ("5432", "postgres", "postgres"),
        _ => return None,
    };
    let backend = if scheme == "postgres" {
        DbBackend::Postgres
    } else {
        DbBackend::MySql
    };
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{scheme}://{}:{}@{}:{}/{}",
                env_or("DB_USERNAME", default_user),
                env_or("DB_PASSWORD", ""),
                env_or("DB_HOST", "127.0.0.1"),
                env_or("DB_PORT", default_port),
                env_or("DB_DATABASE", "blessing_skin"),
            )
        });
    Some(MatrixConfig {
        connection,
        backend,
        url,
    })
}

fn ph(backend: DbBackend, index: usize) -> String {
    if backend == DbBackend::Postgres {
        format!("${index}")
    } else {
        "?".to_string()
    }
}

async fn insert_user(
    connection: &DatabaseConnection,
    config: &MatrixConfig,
    email: &str,
    password_hash: &str,
    now: NaiveDateTime,
) -> Result<i32, Box<dyn Error>> {
    let sql = format!(
        "INSERT INTO users \
         (email, nickname, score, avatar, password, ip, permission, last_sign_at, register_at, verified) \
         VALUES ({}, {}, 0, 0, {}, '127.0.0.1', 0, {}, {}, false)",
        ph(config.backend, 1),
        ph(config.backend, 2),
        ph(config.backend, 3),
        ph(config.backend, 4),
        ph(config.backend, 5),
    );
    connection
        .execute_raw(Statement::from_sql_and_values(
            config.backend,
            sql,
            [
                Value::from(email),
                Value::from("matrix"),
                Value::from(password_hash),
                Value::from(now - Duration::days(1)),
                Value::from(now),
            ],
        ))
        .await?;

    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            config.backend,
            format!(
                "SELECT uid, register_at FROM users WHERE email = {}",
                ph(config.backend, 1)
            ),
            [Value::from(email)],
        ))
        .await?
        .ok_or("inserted user not found")?;
    let uid: i32 = if config.backend == DbBackend::MySql {
        // PHP `increments()` 生成 `INT UNSIGNED`，应用模型层以 `u32` 读取；
        // 矩阵测试同样用 `u32` 解码后再转成 `i32` 供 `textures.uploader` 使用。
        row.try_get::<u32>("", "uid")? as i32
    } else {
        row.try_get("", "uid")?
    };
    let register_at: NaiveDateTime = row.try_get("", "register_at")?;
    let formatted = register_at.format("%Y-%m-%d %H:%M:%S").to_string();
    let pattern = regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$")?;
    if !pattern.is_match(&formatted) {
        return Err(format!("register_at={formatted}, expected Y-m-d H:i:s").into());
    }
    Ok(uid)
}

async fn insert_texture(
    connection: &DatabaseConnection,
    config: &MatrixConfig,
    uid: i32,
    hash: &str,
    now: NaiveDateTime,
) -> Result<(), Box<dyn Error>> {
    let sql = format!(
        "INSERT INTO textures (name, type, hash, size, uploader, public, upload_at, likes) \
         VALUES ({}, 'steve', {}, 1, {}, 1, {}, 1)",
        ph(config.backend, 1),
        ph(config.backend, 2),
        ph(config.backend, 3),
        ph(config.backend, 4),
    );
    connection
        .execute_raw(Statement::from_sql_and_values(
            config.backend,
            sql,
            [
                Value::from("matrix"),
                Value::from(hash),
                Value::from(uid),
                Value::from(now),
            ],
        ))
        .await?;

    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            config.backend,
            format!(
                "SELECT COUNT(*) AS total FROM textures WHERE hash = {}",
                ph(config.backend, 1)
            ),
            [Value::from(hash)],
        ))
        .await?
        .ok_or("texture count query returned no row")?;
    let total: i64 = row.try_get("", "total")?;
    if total != 1 {
        return Err(format!("texture count={total}, expected 1").into());
    }
    Ok(())
}

async fn cleanup(connection: &DatabaseConnection, config: &MatrixConfig, email: &str, hash: &str) {
    for (sql, value) in [
        (
            format!(
                "DELETE FROM textures WHERE hash = {}",
                ph(config.backend, 1)
            ),
            hash,
        ),
        (
            format!("DELETE FROM users WHERE email = {}", ph(config.backend, 1)),
            email,
        ),
    ] {
        let _ = connection
            .execute_raw(Statement::from_sql_and_values(
                config.backend,
                sql,
                [Value::from(value)],
            ))
            .await;
    }
}

/// MySQL/MariaDB DDL 必须与 PHP `database/migrations` 的列类型逐字对齐：
/// `increments()` → `INT UNSIGNED`、`bigIncrements()` / `unsignedBigInteger()`
/// → `BIGINT UNSIGNED`，而 `integer()` 仍是有符号 `INT`。
async fn assert_mysql_unsigned_schema(
    connection: &DatabaseConnection,
) -> Result<(), Box<dyn Error>> {
    let rows = connection
        .query_all_raw(Statement::from_string(
            DbBackend::MySql,
            "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE()"
                .to_string(),
        ))
        .await?;
    let mut types = std::collections::HashMap::new();
    // MariaDB 会附带显示宽度（`int(10) unsigned`），MySQL 不会；比较前统一去掉。
    let display_width = regex::Regex::new(r"\(\d+\)")?;
    for row in rows {
        let table: String = row.try_get("", "TABLE_NAME")?;
        let column: String = row.try_get("", "COLUMN_NAME")?;
        let column_type: String = row.try_get("", "COLUMN_TYPE")?;
        let column_type = display_width.replace_all(&column_type, "").to_string();
        types.insert(
            format!("{}.{}", table.to_ascii_lowercase(), column),
            column_type,
        );
    }

    let expected: &[(&str, &str)] = &[
        ("users.uid", "int unsigned"),
        ("players.pid", "int unsigned"),
        ("players.uid", "int"),
        ("textures.tid", "int unsigned"),
        ("textures.uploader", "int"),
        ("textures.likes", "int unsigned"),
        ("reports.id", "int unsigned"),
        ("reports.tid", "int"),
        ("user_closet.user_uid", "int"),
        ("user_closet.texture_tid", "int"),
        ("notifications.notifiable_id", "bigint unsigned"),
        ("jobs.id", "bigint unsigned"),
        ("jobs.attempts", "tinyint unsigned"),
        ("jobs.available_at", "int unsigned"),
        ("communications.id", "bigint unsigned"),
        ("communications.author_id", "int unsigned"),
        ("oauth_clients.id", "bigint unsigned"),
        ("oauth_access_tokens.user_id", "bigint unsigned"),
        ("options.id", "int unsigned"),
        ("language_lines.id", "int unsigned"),
        ("scopes.id", "int unsigned"),
    ];
    for (key, want) in expected {
        let got = types
            .get(*key)
            .ok_or_else(|| format!("missing column {key}"))?;
        if got != want {
            return Err(format!("{key} is `{got}`, expected `{want}`").into());
        }
    }
    Ok(())
}

#[tokio::test]
async fn matrix_round_trip() -> Result<(), Box<dyn Error>> {
    let Some(config) = matrix_config() else {
        eprintln!("skipped: DB_CONNECTION is not mysql/mariadb/pgsql");
        return Ok(());
    };

    let connection = Database::connect(&config.url).await?;
    Migrator::up(&connection, None).await?;
    if config.backend == DbBackend::MySql {
        assert_mysql_unsigned_schema(&connection).await?;
    }

    let unique = random_token();
    let email = format!("matrix-{unique}@{}", config.connection);
    let hash = format!("{:0>64}", unique); // 64 位十六进制风格，实际只需唯一
    let password_hash =
        hash_password("secret123", Some(4)).map_err(|error| format!("bcrypt: {error}"))?;
    let now = Utc::now().naive_utc();

    let result = async {
        let uid = insert_user(&connection, &config, &email, &password_hash, now).await?;
        insert_texture(&connection, &config, uid, &hash, now).await?;
        Ok::<(), Box<dyn Error>>(())
    }
    .await;

    cleanup(&connection, &config, &email, &hash).await;
    result
}
