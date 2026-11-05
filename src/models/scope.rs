//! OAuth scope（`scopes`）。

use suprnova::model;

#[model(table = "scopes", primary_key = "id", key_type = "u32")]
pub struct Scope {
    pub id: u32,
    pub name: String,
    pub description: String,
}
