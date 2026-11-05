//! 通信管理（PHP `CommunicationsController` + `App\Services\Communications`）。
//!
//! 页面端点（index/inbox/message）待 Twig 视图；此处实现编辑、预览、
//! 导入、撤回与投递/同步逻辑。

use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::{Value, json};
use suprnova::{DB, FrameworkError, HttpResponse, Model, Request, Response, attrs, handler};

use crate::models::communication::{
    Communication, EDITOR_BLOCKS, EDITOR_SOURCE, KIND_ANNOUNCEMENT, KIND_BOTH, KIND_MESSAGE,
    STATUS_DRAFT, STATUS_PUBLISHED, STATUS_WITHDRAWN,
};
use crate::models::notification::{
    ActiveModel as NotificationActiveModel, Column as NotificationColumn, Entity as Notification,
    Model as NotificationModel, NOTIFIABLE_TYPE,
};
use crate::models::user::{PERMISSION_NORMAL, User};
use crate::support::api::validation_error;
use crate::support::communication::{import_blocks, render_communication, render_data};
use crate::support::remember::random_token;

async fn require_admin(request: &Request) -> Result<User, Response> {
    let user = match crate::support::api::require_user(request).await {
        Ok(user) => user,
        Err(response) => return Err(Ok(response)),
    };
    if !user.is_admin() {
        return Err(Ok(HttpResponse::json(json!({
            "code": 1,
            "message": "只有管理员才能访问此页面",
        }))
        .status(403)));
    }
    Ok(user)
}

fn as_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).map(|value| match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

/// 校验并规范化编辑器数据（PHP `validateContent`）。
async fn parse_content(body: &Value) -> Result<Value, FrameworkError> {
    let mut data = body.clone();

    if let Some(raw) = body.get("blocks_json").and_then(Value::as_str) {
        let blocks: Value = serde_json::from_str(raw)
            .map_err(|_| validation_error("blocks_json", "blocks_json 必须是有效的 JSON。"))?;
        if !blocks.is_array() {
            return Err(validation_error("blocks_json", "blocks_json 必须是数组。"));
        }
        if let Some(object) = data.as_object_mut() {
            object.insert("blocks".to_string(), blocks);
        }
    }

    let title = as_string(&data, "title").unwrap_or_default();
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(validation_error("title", "标题 不能为空。"));
    }
    if title.chars().count() > 100 {
        return Err(validation_error("title", "标题 不能大于 100 个字符。"));
    }
    let kind = as_string(&data, "kind").unwrap_or_default();
    if ![KIND_ANNOUNCEMENT, KIND_MESSAGE, KIND_BOTH].contains(&kind.as_str()) {
        return Err(validation_error("kind", "kind 不在允许的范围内。"));
    }
    let editor = as_string(&data, "editor").unwrap_or_default();
    if ![EDITOR_SOURCE, EDITOR_BLOCKS].contains(&editor.as_str()) {
        return Err(validation_error("editor", "editor 不在允许的范围内。"));
    }
    let source = as_string(&data, "source").unwrap_or_default();
    if source.chars().count() > 100_000 {
        return Err(validation_error(
            "source",
            "source 不能大于 100000 个字符。",
        ));
    }
    let locale = as_string(&data, "locale").unwrap_or_default();
    if locale.chars().count() > 20 {
        return Err(validation_error("locale", "locale 不能大于 20 个字符。"));
    }
    let operation = as_string(&data, "operation").unwrap_or_default();
    if !["draft", "publish"].contains(&operation.as_str()) {
        return Err(validation_error(
            "operation",
            "operation 不在允许的范围内。",
        ));
    }
    let receiver = as_string(&data, "receiver").unwrap_or_default();
    if !["all", "normal", "uid", "email"].contains(&receiver.as_str()) {
        return Err(validation_error("receiver", "receiver 不在允许的范围内。"));
    }

    let uid = data.get("uid").and_then(Value::as_u64);
    let email = data
        .get("email")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|value| !value.is_empty());
    if receiver == "uid" {
        let Some(uid) = uid else {
            return Err(validation_error(
                "uid",
                "当 receiver 为 uid 时 uid 不能为空。",
            ));
        };
        let Ok(uid) = u32::try_from(uid) else {
            return Err(validation_error("uid", "uid 不存在。"));
        };
        if User::find(uid).await?.is_none() {
            return Err(validation_error("uid", "uid 不存在。"));
        }
    }
    if receiver == "email" {
        let Some(email) = email.as_deref() else {
            return Err(validation_error(
                "email",
                "当 receiver 为 email 时 email 不能为空。",
            ));
        };
        if User::filter("email", email).first().await?.is_none() {
            return Err(validation_error("email", "email 不存在。"));
        }
    }

    let blocks = data.get("blocks").cloned().unwrap_or(Value::Null);
    if let Some(items) = blocks.as_array() {
        if items.len() > 80 {
            return Err(validation_error("blocks", "blocks 最多只有 80 个单元。"));
        }
        for item in items {
            let block_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            if !["text", "heading", "alert", "collapse", "divider"].contains(&block_type) {
                return Err(validation_error("blocks", "blocks.type 不在允许的范围内。"));
            }
            if let Some(content) = item.get("content").and_then(Value::as_str)
                && content.chars().count() > 10_000
            {
                return Err(validation_error(
                    "blocks",
                    "blocks.content 不能大于 10000 个字符。",
                ));
            }
            if let Some(title) = item.get("title").and_then(Value::as_str)
                && title.chars().count() > 200
            {
                return Err(validation_error(
                    "blocks",
                    "blocks.title 不能大于 200 个字符。",
                ));
            }
            if let Some(tone) = item.get("tone").and_then(Value::as_str)
                && ![
                    "info",
                    "success",
                    "warning",
                    "danger",
                    "primary",
                    "secondary",
                ]
                .contains(&tone)
            {
                return Err(validation_error("blocks", "blocks.tone 不在允许的范围内。"));
            }
        }
    }

    let audience = json!({
        "receiver": receiver,
        "uid": uid,
        "email": email,
    });
    if let Some(object) = data.as_object_mut() {
        object.insert("title".to_string(), Value::String(title));
        object.insert("kind".to_string(), Value::String(kind));
        object.insert("editor".to_string(), Value::String(editor));
        object.insert("source".to_string(), Value::String(source));
        object.insert("locale".to_string(), Value::String(locale));
        object.insert("operation".to_string(), Value::String(operation));
        object.insert("audience".to_string(), audience);
    }
    Ok(data)
}

fn blocks_text(data: &Value) -> Option<String> {
    data.get("blocks")
        .filter(|value| value.is_array())
        .map(|value| value.to_string())
}

/// 新建/更新公告（PHP `saveAnnouncement`）。
async fn save_announcement(
    data: &Value,
    author: i32,
    origin: Option<u64>,
) -> Result<Communication, FrameworkError> {
    let locale = as_string(data, "locale").unwrap_or_default();
    let mut latest: Option<Communication> = None;
    for communication in Communication::filter("kind", KIND_ANNOUNCEMENT)
        .filter("locale", locale.clone())
        .get()
        .await?
        .into_vec()
    {
        if latest
            .as_ref()
            .is_none_or(|current| communication.id > current.id)
        {
            latest = Some(communication);
        }
    }
    let published = as_string(data, "operation").as_deref() == Some("publish");
    let audience = origin.map(|id| json!({ "communication_id": id }).to_string());
    let existing_published_at = latest
        .as_ref()
        .and_then(|communication| communication.published_at);
    let title = as_string(data, "title").unwrap_or_default();
    let editor = as_string(data, "editor").unwrap_or_default();
    let source = as_string(data, "source").unwrap_or_default();
    let status = if published {
        STATUS_PUBLISHED
    } else {
        STATUS_DRAFT
    };
    let blocks = blocks_text(data);
    let now = Utc::now();
    let saved = match latest {
        Some(communication) => {
            communication
                .update(attrs! {
                    title: title,
                    editor: editor,
                    source: Some(source),
                    blocks: blocks,
                    author_id: Some(author),
                    audience: audience,
                    status: status,
                    published_at: if published { Some(now) } else { existing_published_at },
                    updated_at: Some(now),
                })
                .await?
        }
        None => {
            Communication::create(attrs! {
                author_id: Some(author),
                title: title,
                kind: KIND_ANNOUNCEMENT,
                editor: editor,
                source: Some(source),
                blocks: blocks,
                locale: locale,
                audience: audience,
                status: status,
                published_at: if published { Some(now) } else { None },
                created_at: Some(now),
                updated_at: Some(now),
            })
            .await?
        }
    };
    Ok(saved)
}

async fn recipients(data: &Value) -> Result<Vec<User>, FrameworkError> {
    let audience = data.get("audience").cloned().unwrap_or(Value::Null);
    let receiver = audience
        .get("receiver")
        .and_then(Value::as_str)
        .unwrap_or("all");
    Ok(match receiver {
        "normal" => User::filter("permission", PERMISSION_NORMAL)
            .get()
            .await?
            .into_vec(),
        "uid" => match audience.get("uid").and_then(Value::as_u64) {
            Some(uid) => match u32::try_from(uid) {
                Ok(uid) => User::filter("uid", uid).get().await?.into_vec(),
                Err(_) => Vec::new(),
            },
            None => Vec::new(),
        },
        "email" => match audience.get("email").and_then(Value::as_str) {
            Some(email) => User::filter("email", email).get().await?.into_vec(),
            None => Vec::new(),
        },
        _ => {
            let mut users = Vec::new();
            for user in User::all().await?.into_vec() {
                if user.permission >= PERMISSION_NORMAL {
                    users.push(user);
                }
            }
            users
        }
    })
}

fn payload(data: &Value, communication: &Communication) -> Value {
    json!({
        "title": communication.title,
        "content": render_data(data),
        "format": "html",
        "communication_id": communication.id,
        "delivery_token": communication.delivery_token,
    })
}

/// 首次发布：为受众写通知（PHP `deliver`）。
async fn deliver(data: &Value, communication: &Communication) -> Result<(), FrameworkError> {
    let connection = DB::connection()?;
    let body = payload(data, communication).to_string();
    let now = Utc::now().naive_utc();
    for user in recipients(data).await? {
        Notification::insert(NotificationActiveModel {
            id: sea_orm::Set(crate::support::id::uuid_v4()),
            notification_type: sea_orm::Set("App\\Notifications\\SiteMessage".to_string()),
            notifiable_type: sea_orm::Set("App\\Models\\User".to_string()),
            notifiable_id: sea_orm::Set(suprnova::eloquent::StoredU64::from(user.uid)),
            data: sea_orm::Set(body.clone()),
            read_at: sea_orm::Set(None),
            created_at: sea_orm::Set(Some(now)),
            updated_at: sea_orm::Set(Some(now)),
        })
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    }
    Ok(())
}

/// 已发布内容更新：同步现有通知（PHP `synchronize`）。
async fn synchronize(data: &Value, communication: &Communication) -> Result<(), FrameworkError> {
    use crate::models::notification::Column as NotificationColumn;
    let connection = DB::connection()?;
    let body = payload(data, communication).to_string();
    let now = Utc::now().naive_utc();
    let rows = Notification::find()
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    for row in rows {
        let parsed: Value = serde_json::from_str(&row.data).unwrap_or(Value::Null);
        let same_communication =
            parsed.get("communication_id").and_then(Value::as_u64) == Some(communication.id);
        let same_token = parsed.get("delivery_token").and_then(Value::as_str)
            == communication.delivery_token.as_deref();
        if same_communication && same_token {
            Notification::update_many()
                .col_expr(
                    NotificationColumn::Data,
                    sea_orm::sea_query::Expr::value(body.clone()),
                )
                .col_expr(
                    NotificationColumn::UpdatedAt,
                    sea_orm::sea_query::Expr::value(Some(now)),
                )
                .filter(NotificationColumn::Id.eq(row.id))
                .exec(connection.inner())
                .await
                .map_err(|error| FrameworkError::database(error.to_string()))?;
        }
    }
    Ok(())
}

/// 新建/更新通信（PHP `Communications::save`）。
async fn save(
    data: &Value,
    existing: Option<Communication>,
    author: i32,
) -> Result<Communication, FrameworkError> {
    let kind = as_string(data, "kind").unwrap_or_default();
    if kind == KIND_ANNOUNCEMENT {
        return save_announcement(data, author, None).await;
    }

    let published = as_string(data, "operation").as_deref() == Some("publish");
    let inbox = kind == KIND_MESSAGE || kind == KIND_BOTH;
    let was_published = existing
        .as_ref()
        .map(|communication| communication.status == STATUS_PUBLISHED)
        .unwrap_or(false);
    let was_inbox = was_published
        && existing
            .as_ref()
            .map(|communication| {
                communication.kind == KIND_MESSAGE || communication.kind == KIND_BOTH
            })
            .unwrap_or(false);
    let start_delivery = published && inbox && !was_inbox;
    let delivery_token = if start_delivery {
        Some(random_token())
    } else {
        existing
            .as_ref()
            .and_then(|communication| communication.delivery_token.clone())
    };
    let now = Utc::now();
    let title = as_string(data, "title").unwrap_or_default();
    let editor = as_string(data, "editor").unwrap_or_default();
    let source = as_string(data, "source").unwrap_or_default();
    let locale = as_string(data, "locale").unwrap_or_default();
    let audience = data.get("audience").map(|value| value.to_string());
    let status = if published {
        STATUS_PUBLISHED
    } else {
        STATUS_DRAFT
    };
    let blocks = blocks_text(data);
    let published_at = if published && !was_published {
        Some(now)
    } else {
        existing
            .as_ref()
            .and_then(|communication| communication.published_at)
    };

    let saved = match existing {
        Some(communication) => {
            communication
                .update(attrs! {
                    title: title,
                    kind: kind,
                    editor: editor,
                    source: Some(source),
                    blocks: blocks,
                    locale: locale,
                    audience: audience,
                    status: status,
                    delivery_token: delivery_token,
                    published_at: published_at,
                    updated_at: Some(now),
                })
                .await?
        }
        None => {
            Communication::create(attrs! {
                author_id: Some(author),
                title: title,
                kind: kind,
                editor: editor,
                source: Some(source),
                blocks: blocks,
                locale: locale,
                audience: audience,
                status: status,
                delivery_token: delivery_token,
                published_at: published_at,
                created_at: Some(now),
                updated_at: Some(now),
            })
            .await?
        }
    };

    if published && inbox {
        if start_delivery {
            deliver(data, &saved).await?;
        } else {
            synchronize(data, &saved).await?;
        }
    }
    if kind == KIND_BOTH && published {
        save_announcement(data, author, Some(saved.id)).await?;
    }
    Ok(saved)
}

/// POST /admin/communications/preview
#[handler]
pub async fn preview(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let body: Value = request.input().await?;
    let mut body = body;
    let title = as_string(&body, "title")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "预览".to_string());
    if let Some(object) = body.as_object_mut() {
        object.insert("title".to_string(), Value::String(title));
        object.insert(
            "kind".to_string(),
            Value::String(KIND_ANNOUNCEMENT.to_string()),
        );
        object.insert("receiver".to_string(), Value::String("all".to_string()));
        object.insert("operation".to_string(), Value::String("draft".to_string()));
    }
    let data = match parse_content(&body).await {
        Ok(data) => data,
        Err(error) => return Err(error.into()),
    };
    Ok(HttpResponse::json(json!({ "html": render_data(&data) }))
        .header("Cache-Control", "no-store"))
}

/// POST /admin/communications
#[handler]
pub async fn store(request: Request) -> Response {
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let body: Value = request.input().await?;
    let data = parse_content(&body).await?;
    let saved = save(&data, None, admin.uid as i32).await?;
    let location = if saved.kind == KIND_ANNOUNCEMENT {
        "/admin/communications?announcement=1".to_string()
    } else {
        format!("/admin/communications?edit={}", saved.id)
    };
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", location))
}

/// PUT /admin/communications/{communication}
#[handler]
pub async fn update(request: Request) -> Response {
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(id) = request
        .param("communication")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(existing) = Communication::find(id).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let body: Value = request.input().await?;
    let mut data = parse_content(&body).await?;
    // 已发布内容的受众保持原样（PHP `CommunicationsController@update`）。
    if existing.status == STATUS_PUBLISHED
        && let Some(audience) = existing
            .audience
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        && let Some(object) = data.as_object_mut()
    {
        object.insert("audience".to_string(), audience);
    }
    let saved = save(&data, Some(existing), admin.uid as i32).await?;
    let location = if saved.kind == KIND_ANNOUNCEMENT {
        "/admin/communications?announcement=1".to_string()
    } else {
        format!("/admin/communications?edit={}", saved.id)
    };
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", location))
}

/// POST /admin/communications/{communication}/withdraw
#[handler]
pub async fn withdraw(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let Some(id) = request
        .param("communication")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(communication) = Communication::find(id).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let kind = communication.kind.clone();
    communication
        .update(attrs! { status: STATUS_WITHDRAWN, updated_at: Some(Utc::now()) })
        .await?;

    if kind == KIND_BOTH {
        for announcement in Communication::filter("kind", KIND_ANNOUNCEMENT)
            .get()
            .await?
            .into_vec()
        {
            let linked = announcement
                .audience
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .and_then(|value| value.get("communication_id").and_then(Value::as_u64))
                == Some(id);
            if linked {
                announcement
                    .update(attrs! {
                        status: STATUS_WITHDRAWN,
                        updated_at: Some(Utc::now()),
                    })
                    .await?;
            }
        }
    }

    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/communications"))
}

/// POST /admin/communications/import
#[handler]
pub async fn import(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let body: Value = request.input().await?;
    let source = as_string(&body, "source").unwrap_or_default();
    if source.chars().count() > 100_000 {
        return Err(validation_error("source", "source 不能大于 100000 个字符。").into());
    }
    Ok(HttpResponse::json(
        json!({ "blocks": import_blocks(&source) }),
    ))
}

/// 页面级登录检查：未登录时 302 到登录页（与 PHP `auth` 中间件一致）。
async fn require_page_user(request: &Request) -> Result<User, HttpResponse> {
    match crate::support::auth::current_user(request).await? {
        Some(user) => Ok(user),
        None => Err(HttpResponse::text("")
            .status(302)
            .header("Location", "/auth/login")),
    }
}

fn notification_data(notification: &NotificationModel) -> Value {
    serde_json::from_str::<Value>(&notification.data).unwrap_or(Value::Null)
}

/// PHP `Communications::visible`：已撤回/被替换投递令牌的通知不可见。
async fn notification_visible(notification: &NotificationModel) -> Result<bool, FrameworkError> {
    let data = notification_data(notification);
    let Some(id) = data.get("communication_id").and_then(Value::as_u64) else {
        return Ok(true);
    };
    let Some(communication) = Communication::find(id).await? else {
        return Ok(false);
    };
    let token = data.get("delivery_token").and_then(Value::as_str);
    Ok(communication.status == STATUS_PUBLISHED
        && (communication.kind == KIND_MESSAGE || communication.kind == KIND_BOTH)
        && communication.delivery_token.as_deref() == token)
}

/// PHP `Communications::title`。
async fn notification_title(notification: &NotificationModel) -> Result<String, FrameworkError> {
    let data = notification_data(notification);
    if let Some(id) = data.get("communication_id").and_then(Value::as_u64) {
        return Ok(Communication::find(id)
            .await?
            .map(|communication| communication.title)
            .unwrap_or_default());
    }
    Ok(data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("站内信")
        .to_string())
}

/// PHP `Communications::content`。
async fn notification_content(notification: &NotificationModel) -> Result<String, FrameworkError> {
    let data = notification_data(notification);
    if let Some(id) = data.get("communication_id").and_then(Value::as_u64) {
        return Ok(match Communication::find(id).await? {
            Some(communication) => render_communication(&communication),
            None => String::new(),
        });
    }
    Ok(render_data(&json!({
        "editor": "source",
        "source": data.get("content").and_then(Value::as_str).unwrap_or(""),
    })))
}

fn format_time(value: Option<chrono::NaiveDateTime>) -> String {
    value
        .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

/// GET /user/inbox —— 站内信列表。
#[handler]
pub async fn inbox(request: Request) -> Response {
    let user = match require_page_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = crate::support::locale::resolve(&request);
    let mut context = super::user_page::page_context(&request, &user, &locale_code).await;

    let connection = DB::connection()?;
    let mut rows = Notification::find()
        .filter(NotificationColumn::NotifiableId.eq(user.uid as i64))
        .filter(NotificationColumn::NotifiableType.eq(NOTIFIABLE_TYPE))
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let mut items = Vec::new();
    for row in rows.drain(..) {
        if !notification_visible(&row).await? {
            continue;
        }
        items.push(row);
    }
    items.sort_by(|left, right| right.created_at.cmp(&left.created_at));

    let total = items.len();
    let per_page = 20usize;
    let last_page = total.div_ceil(per_page).max(1);
    let page = request
        .query_param("page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, last_page);
    let start = (page - 1) * per_page;

    let mut messages = Vec::new();
    for item in items.iter().skip(start).take(per_page) {
        messages.push(json!({
            "id": item.id,
            "display_title": notification_title(item).await?,
            "read_at": item.read_at.is_some(),
            "created_at": format_time(item.created_at),
        }));
    }

    if let Some(object) = context.as_object_mut() {
        object.insert("messages".to_string(), Value::Array(messages));
        object.insert(
            "pagination".to_string(),
            json!({
                "total": total,
                "current_page": page,
                "last_page": last_page,
                "previous_url": if page > 1 { format!("/user/inbox?page={}", page - 1) } else { String::new() },
                "next_url": if page < last_page { format!("/user/inbox?page={}", page + 1) } else { String::new() },
            }),
        );
    }
    Ok(crate::support::view::render("user/inbox.html", context))
}

/// GET /user/inbox/{id} —— 查看站内信并标记已读。
#[handler]
pub async fn message(request: Request) -> Response {
    let user = match require_page_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let Some(id) = request.param("id").ok().map(str::to_string) else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let connection = DB::connection()?;
    let Some(notification) = Notification::find_by_id(id.clone())
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if notification.notifiable_id != suprnova::eloquent::StoredU64::from(user.uid)
        || notification.notifiable_type != NOTIFIABLE_TYPE
        || !notification_visible(&notification).await?
    {
        return Ok(HttpResponse::text("Not Found").status(404));
    }

    let title = notification_title(&notification).await?;
    let content = notification_content(&notification).await?;
    if notification.read_at.is_none() {
        Notification::update_many()
            .col_expr(
                NotificationColumn::ReadAt,
                sea_orm::sea_query::Expr::value(Utc::now().naive_utc()),
            )
            .filter(NotificationColumn::Id.eq(id))
            .exec(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?;
    }

    let locale_code = crate::support::locale::resolve(&request);
    let mut context = super::user_page::page_context(&request, &user, &locale_code).await;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "message".to_string(),
            json!({
                "id": notification.id,
                "created_at": format_time(notification.created_at),
                "read_at": notification.read_at.is_some(),
            }),
        );
        object.insert("message_title".to_string(), Value::String(title));
        object.insert("content".to_string(), Value::String(content));
    }
    Ok(crate::support::view::render("user/message.html", context))
}
