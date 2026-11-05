//! 站内信（`notifications`）。
//!
//! `type` 是 Rust 关键字，使用手写 SeaORM 实体并映射列名。

use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use suprnova::eloquent::StoredU64;

pub const SITE_MESSAGE_TYPE: &str = "App\\Notifications\\SiteMessage";
pub const NOTIFIABLE_TYPE: &str = "App\\Models\\User";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "notifications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(column_name = "type")]
    pub notification_type: String,
    pub notifiable_type: String,
    /// PHP `morphs()` 生成 `BIGINT UNSIGNED`；`StoredU64` 在 MySQL 读无符号列，
    /// 在 PostgreSQL/SQLite 读有符号 `BIGINT`，是唯一可移植的存储形态。
    pub notifiable_id: StoredU64,
    pub data: String,
    pub read_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
