//! 翻译覆盖（`language_lines`）。

use chrono::{DateTime, Utc};
use suprnova::model;

#[model(table = "language_lines", primary_key = "id", key_type = "u32")]
pub struct LanguageLine {
    pub id: u32,
    pub group: String,
    pub key: String,
    pub text: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}
