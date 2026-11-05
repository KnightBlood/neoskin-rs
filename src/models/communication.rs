//! 通信/公告（`communications`）。

use chrono::{DateTime, Utc};
use suprnova::model;

pub const KIND_ANNOUNCEMENT: &str = "announcement";
pub const KIND_MESSAGE: &str = "message";
pub const KIND_BOTH: &str = "both";

pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_PUBLISHED: &str = "published";
pub const STATUS_WITHDRAWN: &str = "withdrawn";

pub const EDITOR_SOURCE: &str = "source";
pub const EDITOR_BLOCKS: &str = "blocks";

#[model(table = "communications", primary_key = "id", key_type = "u64")]
pub struct Communication {
    pub id: u64,
    pub author_id: Option<u32>,
    pub legacy_key: Option<String>,
    pub delivery_token: Option<String>,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub locale: String,
    pub editor: String,
    pub source: Option<String>,
    pub blocks: Option<String>,
    pub audience: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}
