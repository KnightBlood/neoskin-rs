//! PHP `App\Services\OptionForm` 的 Rust 等价渲染器。
//!
//! 只负责生成与源项目 `resources/views/forms/*.twig` 一致的 HTML；
//! 值的读取与保存由调用方（`controllers::admin_page`）完成。

use serde_json::Value;

use crate::support::communication::escape_html;
use crate::support::i18n;

fn lookup(locale: &str, key: &str) -> Option<Value> {
    match i18n::translate_value(locale, key, None) {
        Value::String(text) if text == key => None,
        Value::Null => None,
        other => Some(other),
    }
}

fn part(locale: &str, key: &str, name: &str) -> Option<String> {
    match lookup(locale, key)? {
        Value::Object(map) => map.get(name).and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// `trans("options.<form>.<id>")`：对象取 `title`，字符串原样返回。
fn label_of(locale: &str, key: &str) -> String {
    match lookup(locale, key) {
        Some(Value::Object(map)) => map
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Some(Value::String(text)) => text,
        _ => String::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Checkbox,
    Textarea,
    Select,
}

/// 单个表单控件。
pub struct Field {
    kind: Kind,
    pub id: String,
    name: String,
    value: String,
    label: String,
    hint: String,
    description: String,
    placeholder: String,
    options: Vec<(String, String)>,
    rows: u32,
}

impl Field {
    fn new(kind: Kind, locale: &str, form: &str, id: &str) -> Self {
        Self {
            kind,
            id: id.to_string(),
            name: label_of(locale, &format!("options.{form}.{id}")),
            value: String::new(),
            label: String::new(),
            hint: String::new(),
            description: String::new(),
            placeholder: String::new(),
            options: Vec::new(),
            rows: 3,
        }
    }

    pub fn text(locale: &str, form: &str, id: &str) -> Self {
        Self::new(Kind::Text, locale, form, id)
    }

    pub fn checkbox(locale: &str, form: &str, id: &str) -> Self {
        Self::new(Kind::Checkbox, locale, form, id)
    }

    pub fn textarea(locale: &str, form: &str, id: &str) -> Self {
        Self::new(Kind::Textarea, locale, form, id)
    }

    pub fn select(locale: &str, form: &str, id: &str) -> Self {
        Self::new(Kind::Select, locale, form, id)
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 自动读取 `options.<form>.<id>.hint`。
    pub fn hint(mut self, locale: &str, form: &str) -> Self {
        self.hint = part(locale, &format!("options.{form}.{}", self.id), "hint")
            .or_else(|| part(locale, &format!("options.{form}.hint"), "hint"))
            .unwrap_or_default();
        self
    }

    pub fn hint_html(mut self, html: impl Into<String>) -> Self {
        self.hint = html.into();
        self
    }

    /// 自动读取 `options.<form>.<id>.description`。
    pub fn description(mut self, locale: &str, form: &str) -> Self {
        self.description = part(
            locale,
            &format!("options.{form}.{}", self.id),
            "description",
        )
        .unwrap_or_default();
        self
    }

    /// 自动读取 `options.<form>.<id>.label`。
    pub fn label(mut self, locale: &str, form: &str) -> Self {
        self.label =
            part(locale, &format!("options.{form}.{}", self.id), "label").unwrap_or_default();
        self
    }

    /// 自动读取 `options.<form>.<id>.placeholder`（不存在时留空）。
    pub fn placeholder(mut self, locale: &str, form: &str) -> Self {
        self.placeholder = part(
            locale,
            &format!("options.{form}.{}", self.id),
            "placeholder",
        )
        .unwrap_or_default();
        self
    }

    pub fn option(mut self, value: impl Into<String>, name: impl Into<String>) -> Self {
        self.options.push((value.into(), name.into()));
        self
    }

    pub fn rows(mut self, rows: u32) -> Self {
        self.rows = rows;
        self
    }

    pub fn checked(self, value: bool) -> Self {
        self.value(if value { "true" } else { "" })
    }

    fn render(&self) -> String {
        let id = escape_html(&self.id);
        let value = escape_html(&self.value);
        match self.kind {
            Kind::Text => format!(
                "<input type=\"text\" class=\"form-control\" name=\"{id}\" value=\"{value}\" placeholder=\"{}\">",
                escape_html(&self.placeholder)
            ),
            Kind::Checkbox => format!(
                "<label for=\"{id}\"><input type=\"checkbox\" {} id=\"{id}\" name=\"{id}\" value=\"true\"> {}</label>",
                if self.value.is_empty() {
                    ""
                } else {
                    "checked=\"true\""
                },
                escape_html(&self.label)
            ),
            Kind::Textarea => format!(
                "<textarea class=\"form-control\" rows=\"{}\" name=\"{id}\">{value}</textarea>",
                self.rows
            ),
            Kind::Select => {
                let mut options = String::new();
                for (option_value, option_name) in &self.options {
                    let selected = if *option_value == self.value {
                        "selected=selected"
                    } else {
                        ""
                    };
                    options.push_str(&format!(
                        "<option value=\"{}\" {selected}>{}</option>",
                        escape_html(option_value),
                        escape_html(option_name)
                    ));
                }
                format!("<select class=\"form-control\" name=\"{id}\">{options}</select>")
            }
        }
    }
}

/// 组合控件（`OptionFormGroup`）：一组输入框 + 后缀文本。
pub enum GroupItem {
    Text(Field),
    Addon(String),
}

pub struct Group {
    pub id: String,
    name: String,
    hint: String,
    description: String,
    pub items: Vec<GroupItem>,
}

impl Group {
    pub fn new(locale: &str, form: &str, id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: label_of(locale, &format!("options.{form}.{id}")),
            hint: String::new(),
            description: String::new(),
            items: Vec::new(),
        }
    }

    pub fn text(mut self, field: Field) -> Self {
        self.items.push(GroupItem::Text(field));
        self
    }

    pub fn addon_text(mut self, text: impl Into<String>) -> Self {
        self.items.push(GroupItem::Addon(text.into()));
        self
    }

    /// 自动读取 `options.<form>.<groupId>.addon`。
    pub fn addon(mut self, locale: &str, form: &str) -> Self {
        if let Some(text) = part(locale, &format!("options.{form}.{}", self.id), "addon") {
            self.items.push(GroupItem::Addon(text));
        }
        self
    }

    pub fn hint(mut self, locale: &str, form: &str) -> Self {
        self.hint =
            part(locale, &format!("options.{form}.{}", self.id), "hint").unwrap_or_default();
        self
    }

    pub fn hint_html(mut self, html: impl Into<String>) -> Self {
        self.hint = html.into();
        self
    }

    pub fn description(mut self, locale: &str, form: &str) -> Self {
        self.description = part(
            locale,
            &format!("options.{form}.{}", self.id),
            "description",
        )
        .unwrap_or_default();
        self
    }

    fn render(&self) -> String {
        let mut inner = String::new();
        for item in &self.items {
            match item {
                GroupItem::Text(field) => inner.push_str(&field.render()),
                GroupItem::Addon(text) => inner.push_str(&format!(
                    "<div class=\"input-group-append\"><span class=\"input-group-text\">{}</span></div>",
                    escape_html(text)
                )),
            }
        }
        format!("<div class=\"input-group\">{inner}</div>")
    }
}

pub enum Item {
    Field(Field),
    Group(Group),
}

pub struct Button {
    pub href: Option<String>,
    pub text: String,
    pub class: String,
    pub kind: String,
    pub name: String,
}

impl Button {
    pub fn link(href: impl Into<String>, text: impl Into<String>, class: &str) -> Self {
        Self {
            href: Some(href.into()),
            text: text.into(),
            class: format!("btn btn-{class}"),
            kind: "button".to_string(),
            name: String::new(),
        }
    }
}

/// 一个 `OptionForm`。
pub struct Form {
    pub id: String,
    pub title: String,
    box_type: String,
    hint: String,
    messages: Vec<(String, String)>,
    alerts: Vec<(String, String)>,
    items: Vec<Item>,
    buttons: Vec<Button>,
}

impl Form {
    pub fn new(locale: &str, id: &str) -> Self {
        Self {
            id: id.to_string(),
            title: i18n::translate(locale, &format!("options.{id}.title"), None),
            box_type: "primary".to_string(),
            hint: String::new(),
            messages: Vec::new(),
            alerts: Vec::new(),
            items: Vec::new(),
            buttons: Vec::new(),
        }
    }

    pub fn box_type(mut self, kind: impl Into<String>) -> Self {
        self.box_type = kind.into();
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn hint(mut self, locale: &str) -> Self {
        self.hint = format!(
            "<i class=\"fas fa-question-circle\" title=\"{}\" data-toggle=\"tooltip\" data-placement=\"top\"></i>",
            escape_html(&i18n::translate(
                locale,
                &format!("options.{}.hint", self.id),
                None
            ))
        );
        self
    }

    pub fn message(mut self, content: impl Into<String>, style: &str) -> Self {
        self.messages.push((content.into(), style.to_string()));
        self
    }

    /// 自动读取 `options.<form>.message`。
    pub fn auto_message(mut self, locale: &str, style: &str) -> Self {
        let text = i18n::translate(locale, &format!("options.{}.message", self.id), None);
        self.messages.push((text, style.to_string()));
        self
    }

    pub fn alert(mut self, content: impl Into<String>, style: &str) -> Self {
        self.alerts.push((content.into(), style.to_string()));
        self
    }

    pub fn field(mut self, field: Field) -> Self {
        self.items.push(Item::Field(field));
        self
    }

    pub fn group(mut self, group: Group) -> Self {
        self.items.push(Item::Group(group));
        self
    }

    pub fn button(mut self, button: Button) -> Self {
        self.buttons.push(button);
        self
    }

    fn render_item(&self, index: usize, total: usize) -> String {
        let item = &self.items[index];
        let (name, hint, description, rendered) = match item {
            Item::Field(field) => (
                field.name.clone(),
                field.hint.clone(),
                field.description.clone(),
                field.render(),
            ),
            Item::Group(group) => (
                group.name.clone(),
                group.hint.clone(),
                group.description.clone(),
                group.render(),
            ),
        };
        let row_class = if index == 0 {
            "mb-3"
        } else if index + 1 == total {
            "mt-3"
        } else {
            "my-3"
        };
        let mut html = format!(
            "<div class=\"row {row_class}\"><div class=\"col-sm-4\">{} {hint}</div><div class=\"col-sm-8\">{rendered}",
            escape_html(&name)
        );
        if !description.is_empty() {
            html.push_str(&format!("<p class=\"description\">{description}</p>"));
        }
        html.push_str("</div></div>");
        html
    }

    /// 渲染；`csrf` 为 CSRF token（见 `suprnova::get_csrf_token`）。
    pub fn render(&self, locale: &str, csrf: &str) -> String {
        let mut html = format!(
            "<div class=\"card card-{}\"><div class=\"card-header\"><h3 class=\"card-title\">{} {}</h3></div><form method=\"post\"><input type=\"hidden\" name=\"_token\" value=\"{}\"><input type=\"hidden\" name=\"option\" value=\"{}\"><div class=\"card-body\">",
            escape_html(&self.box_type),
            escape_html(&self.title),
            self.hint,
            escape_html(csrf),
            escape_html(&self.id)
        );
        for (content, style) in &self.messages {
            html.push_str(&format!(
                "<div class=\"callout callout-{}\">{content}</div>",
                escape_html(style)
            ));
        }
        for (content, style) in &self.alerts {
            html.push_str(&format!(
                "<div class=\"alert alert-{}\"><span>{content}</span></div>",
                escape_html(style)
            ));
        }
        html.push_str("<div class=\"container-fluid\">");
        let total = self.items.len();
        for index in 0..total {
            html.push_str(&self.render_item(index, total));
        }
        html.push_str("</div></div><div class=\"card-footer\">");
        for button in &self.buttons {
            match &button.href {
                Some(href) if !href.is_empty() => {
                    html.push_str(&format!(
                        "<a href=\"{}\" class=\"{}\">{}</a>",
                        escape_html(href),
                        escape_html(&button.class),
                        button.text
                    ));
                }
                _ => {
                    html.push_str(&format!(
                        "<button type=\"{}\" name=\"{}\" class=\"{}\">{}</button>",
                        escape_html(&button.kind),
                        escape_html(&button.name),
                        escape_html(&button.class),
                        button.text
                    ));
                }
            }
        }
        html.push_str(&format!(
            "<button type=\"submit\" name=\"submit_{}\" class=\"btn btn-primary\">{}</button>",
            escape_html(&self.id),
            escape_html(&i18n::translate(locale, "general.submit", None))
        ));
        html.push_str("</div></form></div>");
        html
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_field_escapes_value_and_placeholder() {
        let mut field = Field::text("zh_CN", "general", "site_name")
            .name("名称")
            .value("<script>");
        field.placeholder = "<x>".to_string();
        let html = field.render();
        assert!(html.contains("name=\"site_name\""));
        assert!(html.contains("value=\"&lt;script&gt;\""));
        assert!(html.contains("placeholder=\"&lt;x&gt;\""));
    }

    #[test]
    fn checkbox_reflects_checked_state() {
        let checked = Field::checkbox("zh_CN", "general", "force_ssl")
            .checked(true)
            .render();
        assert!(checked.contains("checked=\"true\""));
        let unchecked = Field::checkbox("zh_CN", "general", "force_ssl")
            .checked(false)
            .render();
        assert!(!unchecked.contains("checked="));
    }

    #[test]
    fn textarea_uses_rows_and_escapes_body() {
        let field = Field::textarea("zh_CN", "homepage", "copyright_text")
            .rows(6)
            .value("<b>x</b>");
        let html = field.render();
        assert!(html.contains("rows=\"6\""));
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
    }

    #[test]
    fn select_marks_the_current_option() {
        let field = Field::select("zh_CN", "general", "player_name_rule")
            .value("cjk")
            .option("official", "Official")
            .option("cjk", "CJK");
        let html = field.render();
        assert!(html.contains("<option value=\"cjk\" selected=selected>"));
        assert!(html.contains("<option value=\"official\" >"));
    }

    #[test]
    fn group_renders_addons_and_nested_fields() {
        let group = Group::new("zh_CN", "general", "max_upload_file_size")
            .text(Field::text("zh_CN", "general", "max_upload_file_size").value("1024"))
            .addon_text("KB");
        let html = group.render();
        assert!(html.contains("input-group"));
        assert!(html.contains("input-group-append"));
        assert!(html.contains(">KB</span>"));
    }

    #[test]
    fn form_renders_csrf_option_and_submit() {
        let form = Form::new("zh_CN", "general")
            .field(
                Field::text("zh_CN", "general", "site_name")
                    .name("站点名称")
                    .value("Blessing Skin"),
            )
            .button(Button::link("?clear-cache", "清除缓存", "warning"));
        let html = form.render("zh_CN", "csrf-token");
        assert!(html.contains("name=\"_token\" value=\"csrf-token\""));
        assert!(html.contains("name=\"option\" value=\"general\""));
        assert!(html.contains("name=\"site_name\""));
        assert!(html.contains("?clear-cache"));
        assert!(html.contains("name=\"submit_general\""));
    }
}
