//! 用户模型（`users` 表，主键 `uid`）。

use chrono::{DateTime, Utc};
use suprnova::model;

/// 权限常量与 PHP `App\Models\User` 一致。
pub const PERMISSION_BANNED: i32 = -1;
pub const PERMISSION_NORMAL: i32 = 0;
pub const PERMISSION_ADMIN: i32 = 1;
pub const PERMISSION_SUPER: i32 = 2;

#[model(
    table = "users",
    primary_key = "uid",
    key_type = "u32",
    hidden = ["password", "remember_token", "verification_token"],
)]
pub struct User {
    pub uid: u32,
    pub email: String,
    pub nickname: String,
    pub locale: Option<String>,
    pub score: i32,
    pub avatar: i32,
    pub password: String,
    pub ip: String,
    pub is_dark_mode: bool,
    pub permission: i32,
    pub last_sign_at: DateTime<Utc>,
    pub register_at: DateTime<Utc>,
    pub verified: bool,
    pub verification_token: String,
    pub remember_token: Option<String>,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.permission >= PERMISSION_ADMIN
    }

    pub fn is_banned(&self) -> bool {
        self.permission < PERMISSION_NORMAL
    }
}
