//! 站点选项（`options`）。

use suprnova::model;

#[model(table = "options", primary_key = "id", key_type = "u32")]
pub struct OptionItem {
    pub id: u32,
    pub option_name: String,
    pub option_value: String,
}
