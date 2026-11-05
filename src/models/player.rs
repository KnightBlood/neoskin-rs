//! 玩家角色模型（`players`）。

use chrono::{DateTime, Utc};
use suprnova::model;

#[model(table = "players", primary_key = "pid", key_type = "u32")]
pub struct Player {
    pub pid: u32,
    pub uid: i32,
    pub name: String,
    pub tid_cape: i32,
    pub last_modified: DateTime<Utc>,
    pub tid_skin: i32,
}
