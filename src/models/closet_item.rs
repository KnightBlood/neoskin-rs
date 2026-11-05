//! 衣柜条目（`user_closet`）。
//!
//! 该表没有主键；SeaORM 要求实体声明主键，这里以 `user_uid` 作为逻辑主键，
//! 仅用于查询/插入/删除，不依赖单行更新语义。

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "user_closet")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_uid: i32,
    pub texture_tid: i32,
    pub item_name: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
