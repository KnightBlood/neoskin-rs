//! 通信内容渲染（对应 PHP `App\Services\CommunicationRenderer`）。
//!
//! 使用 `pulldown-cmark` 渲染 GFM Markdown，再用 `ammonia` 做与 PHP
//! `sanitize()` 对齐的标签/属性/URL 白名单清洗。

use std::collections::{HashMap, HashSet};

use ammonia::Builder;
use pulldown_cmark::{Options, Parser, html};
use serde_json::{Value, json};

use crate::models::communication::Communication;

/// PHP `sanitize()` 允许的标签集合。
const ALLOWED_TAGS: &[&str] = &[
    "div",
    "span",
    "p",
    "br",
    "hr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "strong",
    "b",
    "em",
    "i",
    "u",
    "s",
    "del",
    "small",
    "blockquote",
    "pre",
    "code",
    "a",
    "button",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "details",
    "summary",
    "img",
];

/// PHP 对所有元素保留的属性（`style` 只保留 font-weight/padding，这里直接丢弃）。
const GENERIC_ATTRIBUTES: &[&str] = &[
    "id",
    "class",
    "title",
    "alt",
    "data-toggle",
    "data-target",
    "aria-controls",
    "aria-expanded",
    "role",
    "open",
];

/// 清洗 HTML（PHP `CommunicationRenderer::sanitize` 的等价实现）。
///
/// 与 PHP 的差异：不做 `id` 前缀重写、`class`/`style` 值过滤；
/// `style` 整体丢弃（PHP 只保留 font-weight/padding）。差异见 `PORT-NOTES.md`。
pub fn sanitize(html: &str) -> String {
    let mut tags = HashSet::new();
    tags.extend(ALLOWED_TAGS.iter().copied());

    let mut generic = HashSet::new();
    generic.extend(GENERIC_ATTRIBUTES.iter().copied());

    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    tag_attributes.insert("a", HashSet::from(["href"]));
    tag_attributes.insert("img", HashSet::from(["src"]));
    tag_attributes.insert("button", HashSet::from(["type"]));

    let mut schemes = HashSet::new();
    schemes.insert("http");
    schemes.insert("https");
    schemes.insert("mailto");

    Builder::default()
        .tags(tags)
        .generic_attributes(generic)
        .tag_attributes(tag_attributes)
        .url_schemes(schemes)
        .link_rel(Some("noopener noreferrer"))
        .clean(html)
        .to_string()
}

pub fn markdown(source: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(source, options);
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn render_blocks(blocks: &Value) -> String {
    let Some(items) = blocks.as_array() else {
        return String::new();
    };
    let mut output = String::new();
    for (index, block) in items.iter().enumerate() {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        let title = block.get("title").and_then(Value::as_str).unwrap_or("");
        let tone = block.get("tone").and_then(Value::as_str).unwrap_or("info");
        let tone = if [
            "info",
            "success",
            "warning",
            "danger",
            "primary",
            "secondary",
        ]
        .contains(&tone)
        {
            tone
        } else {
            "info"
        };
        let content = markdown(block.get("content").and_then(Value::as_str).unwrap_or(""));
        match block_type {
            "heading" => output.push_str(&format!("<h3>{}</h3>", escape_html(title))),
            "alert" => output.push_str(&format!(
                "<div class=\"alert alert-{tone}\">{content}</div>"
            )),
            "collapse" => {
                let expanded = block
                    .get("expanded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                output.push_str(&format!(
                    "<div class=\"card card-outline card-{tone}\"><div class=\"card-header\">\
                     <button type=\"button\" class=\"btn btn-link text-left p-0\" \
                     data-toggle=\"collapse\" data-target=\"#block-{index}\" \
                     aria-expanded=\"{expanded}\">{}</button></div>\
                     <div id=\"block-{index}\" class=\"card-body collapse{}\">{content}</div></div>",
                    escape_html(title),
                    if expanded { " show" } else { "" }
                ));
            }
            "divider" => output.push_str("<hr>"),
            _ => output.push_str(&content),
        }
    }
    sanitize(&output)
}

/// 渲染一条通信（`source` 或 `blocks` 编辑器）。
pub fn render_communication(communication: &Communication) -> String {
    if communication.editor == "blocks"
        && let Some(raw) = communication.blocks.as_deref()
    {
        let blocks: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
        let html = render_blocks(&blocks);
        if !html.is_empty() {
            return html;
        }
    }
    sanitize(&markdown(communication.source.as_deref().unwrap_or("")))
}

/// 把编辑器数据（title/editor/source/blocks）渲染为 HTML。
pub fn render_data(data: &Value) -> String {
    if data.get("editor").and_then(Value::as_str) == Some("blocks")
        && let Some(blocks) = data.get("blocks")
    {
        let html = render_blocks(blocks);
        if !html.is_empty() {
            return html;
        }
    }
    sanitize(&markdown(
        data.get("source").and_then(Value::as_str).unwrap_or(""),
    ))
}

fn strip_tags(html: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output.trim().to_string()
}

/// `CommunicationRenderer::importBlocks` 的简化实现：
/// 识别标题、分隔线、alert 与其余文本，返回 blocks 数组。
pub fn import_blocks(source: &str) -> Vec<Value> {
    let html = markdown(source);
    let mut blocks = Vec::new();
    let mut rest = html.as_str();
    while !rest.is_empty() && blocks.len() < 80 {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            break;
        }
        rest = trimmed;
        if let Some(after) = rest.strip_prefix("<hr")
            && let Some(end) = after.find('>')
        {
            blocks.push(json!({ "type": "divider" }));
            rest = &after[end + 1..];
            continue;
        }
        let mut handled = false;
        for level in 1..=6 {
            let open = format!("<h{level}>");
            if let Some(after) = rest.strip_prefix(open.as_str())
                && let Some(end) = after.find(&format!("</h{level}>"))
            {
                blocks.push(json!({
                    "type": "heading",
                    "title": strip_tags(&after[..end]),
                }));
                rest = &after[end + level + 4..];
                handled = true;
                break;
            }
        }
        if handled {
            continue;
        }
        if let Some(after) = rest.strip_prefix("<div class=\"alert alert-")
            && let Some(quote) = after.find('"')
            && let Some(body) = after[quote + 1..].strip_prefix('>')
            && let Some(end) = body.find("</div>")
        {
            let tone = &after[..quote];
            blocks.push(json!({
                "type": "alert",
                "content": body[..end].to_string(),
                "tone": tone,
            }));
            rest = &body[end + 6..];
            continue;
        }
        // 其余内容归入一个文本块，直到遇到下一个顶层标签。
        let end = next_block_start(rest);
        if end == 0 {
            break;
        }
        blocks.push(json!({ "type": "text", "content": rest[..end].to_string() }));
        rest = &rest[end..];
    }
    blocks
}

fn next_block_start(rest: &str) -> usize {
    let tail = &rest[1..];
    let mut next = rest.len();
    for marker in ["<h", "<hr", "<div class=\"alert"] {
        if let Some(index) = tail.find(marker) {
            next = next.min(index + 1);
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_renders_strong() {
        assert!(markdown("**bold**").contains("<strong>bold</strong>"));
    }

    #[test]
    fn sanitize_strips_scripts_and_event_handlers() {
        let html = sanitize(
            "<div class=\"alert alert-info\" onclick=\"evil()\">ok<script>alert(1)</script>\
             <img src=\"https://example.com/a.png\" onerror=\"evil()\"></div>",
        );
        assert!(html.contains("<div"));
        assert!(html.contains("alert-info"));
        assert!(html.contains("https://example.com/a.png"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn sanitize_drops_javascript_urls_but_keeps_relative_links() {
        let html =
            sanitize("<a href=\"javascript:alert(1)\">bad</a><a href=\"/user/profile\">ok</a>");
        assert!(!html.contains("javascript:"));
        assert!(html.contains("/user/profile"));
        assert!(html.contains("rel=\"noopener noreferrer\""));
    }

    #[test]
    fn render_data_sanitizes_source_html() {
        let html = render_data(&serde_json::json!({
            "editor": "source",
            "source": "<script>alert(1)</script>\n\nhello"
        }));
        assert!(!html.contains("<script"));
        assert!(html.contains("hello"));
    }

    #[test]
    fn blocks_render_alerts_and_headings() {
        let blocks = serde_json::json!([
            { "type": "heading", "title": "Hi" },
            { "type": "alert", "content": "careful", "tone": "danger" },
            { "type": "divider" }
        ]);
        let html = render_blocks(&blocks);
        assert!(html.contains("<h3>Hi</h3>"));
        assert!(html.contains("alert-danger"));
        assert!(html.contains("<hr>"));
    }

    #[test]
    fn import_blocks_detects_heading_and_divider() {
        let blocks = import_blocks("# Title\n\n---\n\nhello");
        assert!(blocks.iter().any(|b| b["type"] == "heading"));
        assert!(blocks.iter().any(|b| b["type"] == "divider"));
    }

    #[test]
    fn escape_html_covers_markup_characters() {
        assert_eq!(escape_html("&<>\"'"), "&amp;&lt;&gt;&quot;&#39;");
    }

    #[test]
    fn render_data_falls_back_to_source_markdown() {
        let html = render_data(&serde_json::json!({
            "editor": "source",
            "source": "**hello**"
        }));
        assert!(html.contains("<strong>hello</strong>"));
    }

    #[test]
    fn render_blocks_falls_back_to_content_for_unknown_types() {
        let blocks = serde_json::json!([{ "type": "unknown", "content": "**x**" }]);
        assert!(render_blocks(&blocks).contains("<strong>x</strong>"));
    }
}
