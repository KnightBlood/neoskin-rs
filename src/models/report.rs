//! 举报（`reports`）。

use chrono::{DateTime, Utc};
use suprnova::model;

pub const STATUS_PENDING: i32 = 0;
pub const STATUS_RESOLVED: i32 = 1;
pub const STATUS_REJECTED: i32 = 2;

#[model(table = "reports", primary_key = "id", key_type = "u32")]
pub struct Report {
    pub id: u32,
    pub tid: i32,
    pub uploader: i32,
    pub reporter: i32,
    pub reason: String,
    pub status: i32,
    pub report_at: DateTime<Utc>,
}
