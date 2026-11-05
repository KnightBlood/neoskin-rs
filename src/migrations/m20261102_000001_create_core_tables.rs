//! 16 张业务表（与 PHP `database/migrations` 最终结构一致）。
//!
//! 字段名、默认值与索引名逐字对齐；不使用外键，避免存量数据/删除顺序差异。

use sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        create_users(manager, backend).await?;
        create_players(manager, backend).await?;
        create_textures(manager, backend).await?;
        create_options(manager, backend).await?;
        create_user_closet(manager, backend).await?;
        create_reports(manager, backend).await?;
        create_notifications(manager, backend).await?;
        create_jobs(manager, backend).await?;
        create_language_lines(manager, backend).await?;
        create_scopes(manager, backend).await?;
        create_oauth_auth_codes(manager, backend).await?;
        create_oauth_access_tokens(manager, backend).await?;
        create_oauth_refresh_tokens(manager, backend).await?;
        create_oauth_clients(manager, backend).await?;
        create_oauth_personal_access_clients(manager, backend).await?;
        create_communications(manager, backend).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "communications",
            "oauth_personal_access_clients",
            "oauth_clients",
            "oauth_refresh_tokens",
            "oauth_access_tokens",
            "oauth_auth_codes",
            "scopes",
            "language_lines",
            "jobs",
            "notifications",
            "reports",
            "user_closet",
            "options",
            "textures",
            "players",
            "users",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(table)).to_owned())
                .await?;
        }
        Ok(())
    }
}

fn col(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name)).to_owned()
}

/// MySQL 的 `increments()` / `bigIncrements()` 是无符号整数；
/// PostgreSQL 与 SQLite 只有有符号整数，保持 `integer` / `bigint`
/// （后者在 `postgres-use-serial-pk` 下生成 `SERIAL` / `BIGSERIAL`）。
fn int_id(name: &str, backend: DbBackend) -> ColumnDef {
    let mut def = col(name);
    def.integer().not_null().auto_increment().primary_key();
    if backend == DbBackend::MySql {
        def.unsigned();
    }
    def
}

fn big_id(name: &str, backend: DbBackend) -> ColumnDef {
    let mut def = col(name);
    def.big_integer().not_null().auto_increment().primary_key();
    if backend == DbBackend::MySql {
        def.big_unsigned();
    }
    def
}

fn uint(name: &str, backend: DbBackend) -> ColumnDef {
    let mut def = col(name);
    def.integer();
    if backend == DbBackend::MySql {
        def.unsigned();
    }
    def
}

fn ubig(name: &str, backend: DbBackend) -> ColumnDef {
    let mut def = col(name);
    def.big_integer();
    if backend == DbBackend::MySql {
        def.big_unsigned();
    }
    def
}

fn tiny_uint(name: &str, backend: DbBackend) -> ColumnDef {
    let mut def = col(name);
    def.tiny_integer();
    if backend == DbBackend::MySql {
        def.tiny_unsigned();
    }
    def
}

async fn create_users(manager: &SchemaManager<'_>, backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("users"))
                .if_not_exists()
                .col(int_id("uid", backend))
                .col(col("email").string_len(100).not_null())
                .col(col("nickname").string_len(50).not_null().default(""))
                .col(col("locale").string_len(255).null())
                .col(col("score").integer().not_null())
                .col(col("avatar").integer().not_null().default(0))
                .col(col("password").string_len(255).not_null())
                .col(col("ip").string_len(45).not_null())
                .col(col("is_dark_mode").boolean().not_null().default(false))
                .col(col("permission").integer().not_null().default(0))
                .col(col("last_sign_at").date_time().not_null())
                .col(col("register_at").date_time().not_null())
                .col(col("verified").boolean().not_null().default(false))
                .col(
                    col("verification_token")
                        .string_len(255)
                        .not_null()
                        .default(""),
                )
                .col(col("remember_token").string_len(100).null())
                .to_owned(),
        )
        .await
}

async fn create_players(manager: &SchemaManager<'_>, backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("players"))
                .if_not_exists()
                .col(int_id("pid", backend))
                .col(col("uid").integer().not_null())
                .col(col("name").string_len(50).not_null())
                .col(col("tid_cape").integer().not_null().default(0))
                .col(col("last_modified").date_time().not_null())
                .col(col("tid_skin").integer().not_null().default(-1))
                .to_owned(),
        )
        .await
}

async fn create_textures(manager: &SchemaManager<'_>, backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("textures"))
                .if_not_exists()
                .col(int_id("tid", backend))
                .col(col("name").string_len(50).not_null())
                .col(col("type").string_len(10).not_null())
                .col(col("hash").string_len(64).not_null())
                .col(col("size").integer().not_null())
                .col(col("uploader").integer().not_null())
                .col(col("public").tiny_integer().not_null())
                .col(col("upload_at").date_time().not_null())
                .col(uint("likes", backend).not_null().default(0))
                .to_owned(),
        )
        .await
}

async fn create_options(manager: &SchemaManager<'_>, backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("options"))
                .if_not_exists()
                .col(int_id("id", backend))
                .col(col("option_name").string_len(50).not_null())
                .col(col("option_value").text().not_null())
                .to_owned(),
        )
        .await
}

async fn create_user_closet(manager: &SchemaManager<'_>, _backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("user_closet"))
                .if_not_exists()
                .col(col("user_uid").integer().not_null())
                .col(col("texture_tid").integer().not_null())
                .col(col("item_name").text().null())
                .to_owned(),
        )
        .await
}

async fn create_reports(manager: &SchemaManager<'_>, backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("reports"))
                .if_not_exists()
                .col(int_id("id", backend))
                .col(col("tid").integer().not_null())
                .col(col("uploader").integer().not_null())
                .col(col("reporter").integer().not_null())
                .col(col("reason").text().not_null())
                .col(col("status").integer().not_null())
                .col(col("report_at").date_time().not_null())
                .to_owned(),
        )
        .await
}

async fn create_notifications(
    manager: &SchemaManager<'_>,
    backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("notifications"))
                .if_not_exists()
                .col(col("id").string_len(36).not_null().primary_key())
                .col(col("type").string_len(255).not_null())
                .col(col("notifiable_type").string_len(255).not_null())
                .col(ubig("notifiable_id", backend).not_null())
                .col(col("data").text().not_null())
                .col(col("read_at").timestamp().null())
                .col(col("created_at").timestamp().null())
                .col(col("updated_at").timestamp().null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "notifications",
        "notifications_notifiable_type_notifiable_id_index",
        Index::create()
            .if_not_exists()
            .name("notifications_notifiable_type_notifiable_id_index")
            .table(Alias::new("notifications"))
            .col(Alias::new("notifiable_type"))
            .col(Alias::new("notifiable_id"))
            .to_owned(),
    )
    .await
}

async fn create_jobs(manager: &SchemaManager<'_>, backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("jobs"))
                .if_not_exists()
                .col(big_id("id", backend))
                .col(col("queue").string_len(255).not_null())
                .col(col("payload").text().not_null())
                .col(tiny_uint("attempts", backend).not_null())
                .col(uint("reserved_at", backend).null())
                .col(uint("available_at", backend).not_null())
                .col(uint("created_at", backend).not_null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "jobs",
        "jobs_queue_index",
        Index::create()
            .if_not_exists()
            .name("jobs_queue_index")
            .table(Alias::new("jobs"))
            .col(Alias::new("queue"))
            .to_owned(),
    )
    .await
}

async fn create_language_lines(
    manager: &SchemaManager<'_>,
    backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("language_lines"))
                .if_not_exists()
                .col(int_id("id", backend))
                .col(col("group").string_len(255).not_null())
                .col(col("key").string_len(255).not_null())
                .col(col("text").text().not_null())
                .col(col("created_at").timestamp().null())
                .col(col("updated_at").timestamp().null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "language_lines",
        "language_lines_group_index",
        Index::create()
            .if_not_exists()
            .name("language_lines_group_index")
            .table(Alias::new("language_lines"))
            .col(Alias::new("group"))
            .to_owned(),
    )
    .await
}

async fn create_scopes(manager: &SchemaManager<'_>, backend: DbBackend) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("scopes"))
                .if_not_exists()
                .col(int_id("id", backend))
                .col(col("name").string_len(255).not_null())
                .col(col("description").string_len(255).not_null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "scopes",
        "scopes_name_unique",
        Index::create()
            .if_not_exists()
            .name("scopes_name_unique")
            .table(Alias::new("scopes"))
            .col(Alias::new("name"))
            .unique()
            .to_owned(),
    )
    .await
}

async fn create_oauth_auth_codes(
    manager: &SchemaManager<'_>,
    backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("oauth_auth_codes"))
                .if_not_exists()
                .col(col("id").string_len(100).not_null().primary_key())
                .col(ubig("user_id", backend).not_null())
                .col(ubig("client_id", backend).not_null())
                .col(col("scopes").text().null())
                .col(col("revoked").boolean().not_null())
                .col(col("expires_at").date_time().null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "oauth_auth_codes",
        "oauth_auth_codes_user_id_index",
        Index::create()
            .if_not_exists()
            .name("oauth_auth_codes_user_id_index")
            .table(Alias::new("oauth_auth_codes"))
            .col(Alias::new("user_id"))
            .to_owned(),
    )
    .await
}

async fn create_oauth_access_tokens(
    manager: &SchemaManager<'_>,
    backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("oauth_access_tokens"))
                .if_not_exists()
                .col(col("id").string_len(100).not_null().primary_key())
                .col(ubig("user_id", backend).null())
                .col(ubig("client_id", backend).not_null())
                .col(col("name").string_len(255).null())
                .col(col("scopes").text().null())
                .col(col("revoked").boolean().not_null())
                .col(col("created_at").timestamp().null())
                .col(col("updated_at").timestamp().null())
                .col(col("expires_at").date_time().null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "oauth_access_tokens",
        "oauth_access_tokens_user_id_index",
        Index::create()
            .if_not_exists()
            .name("oauth_access_tokens_user_id_index")
            .table(Alias::new("oauth_access_tokens"))
            .col(Alias::new("user_id"))
            .to_owned(),
    )
    .await
}

async fn create_oauth_refresh_tokens(
    manager: &SchemaManager<'_>,
    _backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("oauth_refresh_tokens"))
                .if_not_exists()
                .col(col("id").string_len(100).not_null().primary_key())
                .col(col("access_token_id").string_len(100).not_null())
                .col(col("revoked").boolean().not_null())
                .col(col("expires_at").date_time().null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "oauth_refresh_tokens",
        "oauth_refresh_tokens_access_token_id_index",
        Index::create()
            .if_not_exists()
            .name("oauth_refresh_tokens_access_token_id_index")
            .table(Alias::new("oauth_refresh_tokens"))
            .col(Alias::new("access_token_id"))
            .to_owned(),
    )
    .await
}

async fn create_oauth_clients(
    manager: &SchemaManager<'_>,
    backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("oauth_clients"))
                .if_not_exists()
                .col(big_id("id", backend))
                .col(ubig("user_id", backend).null())
                .col(col("name").string_len(255).not_null())
                .col(col("secret").string_len(100).null())
                .col(col("provider").string_len(255).null())
                .col(col("redirect").text().not_null())
                .col(col("personal_access_client").boolean().not_null())
                .col(col("password_client").boolean().not_null())
                .col(col("revoked").boolean().not_null())
                .col(col("created_at").timestamp().null())
                .col(col("updated_at").timestamp().null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "oauth_clients",
        "oauth_clients_user_id_index",
        Index::create()
            .if_not_exists()
            .name("oauth_clients_user_id_index")
            .table(Alias::new("oauth_clients"))
            .col(Alias::new("user_id"))
            .to_owned(),
    )
    .await
}

async fn create_oauth_personal_access_clients(
    manager: &SchemaManager<'_>,
    backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("oauth_personal_access_clients"))
                .if_not_exists()
                .col(big_id("id", backend))
                .col(ubig("client_id", backend).not_null())
                .col(col("created_at").timestamp().null())
                .col(col("updated_at").timestamp().null())
                .to_owned(),
        )
        .await
}

async fn create_communications(
    manager: &SchemaManager<'_>,
    backend: DbBackend,
) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Alias::new("communications"))
                .if_not_exists()
                .col(big_id("id", backend))
                .col(uint("author_id", backend).null())
                .col(col("legacy_key").string_len(100).null())
                .col(col("delivery_token").string_len(32).null())
                .col(col("title").string_len(100).not_null())
                .col(
                    col("kind")
                        .string_len(16)
                        .not_null()
                        .default("announcement"),
                )
                .col(col("status").string_len(16).not_null().default("draft"))
                .col(col("locale").string_len(20).not_null().default(""))
                .col(col("editor").string_len(16).not_null().default("blocks"))
                .col(col("source").text().null())
                .col(col("blocks").text().null())
                .col(col("audience").text().null())
                .col(col("published_at").timestamp().null())
                .col(col("created_at").timestamp().null())
                .col(col("updated_at").timestamp().null())
                .to_owned(),
        )
        .await?;
    create_index_if_missing(
        manager,
        "communications",
        "communications_legacy_key_unique",
        Index::create()
            .if_not_exists()
            .name("communications_legacy_key_unique")
            .table(Alias::new("communications"))
            .col(Alias::new("legacy_key"))
            .unique()
            .to_owned(),
    )
    .await?;
    create_index_if_missing(
        manager,
        "communications",
        "communications_status_index",
        Index::create()
            .if_not_exists()
            .name("communications_status_index")
            .table(Alias::new("communications"))
            .col(Alias::new("status"))
            .to_owned(),
    )
    .await
}

/// MySQL 不支持 `CREATE INDEX IF NOT EXISTS`，统一先查再建，保证矩阵可重复执行。
async fn create_index_if_missing(
    manager: &SchemaManager<'_>,
    table: &str,
    index: &str,
    stmt: IndexCreateStatement,
) -> Result<(), DbErr> {
    if manager.has_index(table, index).await? {
        return Ok(());
    }
    manager.create_index(stmt).await
}
