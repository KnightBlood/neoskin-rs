//! 纹理（`textures`）。
//!
//! `type` 是 Rust 关键字，`public` 与部分数据库保留字冲突，因此使用手写
//! SeaORM 实体并显式映射列名。

use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "textures")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub tid: u32,
    pub name: String,
    #[sea_orm(column_name = "type")]
    pub texture_type: String,
    pub hash: String,
    pub size: i32,
    pub uploader: i32,
    #[sea_orm(column_name = "public")]
    pub public_flag: i32,
    pub upload_at: NaiveDateTime,
    pub likes: u32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn is_public(&self) -> bool {
        self.public_flag != 0
    }
}
