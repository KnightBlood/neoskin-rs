//! 站内信（PHP `NotificationsController@all/read`）。
//!
//! Laravel 的通知存放在 `notifications` 表：`data` 是 JSON，包含
//! `title` / `content`，新版还带 `communication_id` / `delivery_token`。
//! 可见性规则与 `App\Services\Communications::inboxQuery/visible` 对齐：
//! 没有 `communication_id` 的通知始终可见；否则对应 `communications`
//! 必须处于 `published` + `message|both` 且 `delivery_token` 一致。

use chrono::Utc;
use serde_json::{Value, json};
use suprnova::{DB, FrameworkError, HttpResponse, Model, Request, Response, handler};

use crate::models::communication::Communication;
use crate::models::notification::Entity as Notification;
use crate::support::api::require_user;
use crate::support::communication::{markdown, render_communication};

fn parse_data(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

/// 判断通知是否可见（`Communications::visible`）。
async fn visible(data: &Value) -> Result<bool, FrameworkError> {
    let Some(id) = data.get("communication_id").and_then(Value::as_u64) else {
        return Ok(true);
    };
    let Some(communication) = Communication::find(id).await? else {
        return Ok(false);
    };
    let token = data.get("delivery_token").and_then(Value::as_str);
    Ok(communication.status == "published"
        && (communication.kind == "message" || communication.kind == "both")
        && communication.delivery_token.as_deref() == token)
}

async fn title_of(data: &Value) -> Result<String, FrameworkError> {
    if let Some(id) = data.get("communication_id").and_then(Value::as_u64)
        && let Some(communication) = Communication::find(id).await?
    {
        return Ok(communication.title);
    }
    Ok(data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("站内信")
        .to_string())
}

/// GET /api/user/notifications —— 未读通知列表。
#[handler]
pub async fn all(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Notification.Read"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };

    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let connection = suprnova::DB::connection()?;
    let rows = Notification::find()
        .filter(crate::models::notification::Column::NotifiableType.eq("App\\Models\\User"))
        .filter(
            crate::models::notification::Column::NotifiableId
                .eq(suprnova::eloquent::StoredU64::from(user.uid)),
        )
        .filter(crate::models::notification::Column::ReadAt.is_null())
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    let mut items = Vec::new();
    for row in rows {
        let data = parse_data(&row.data);
        if !visible(&data).await? {
            continue;
        }
        items.push(json!({
            "id": row.id,
            "title": title_of(&data).await?,
        }));
    }
    Ok(HttpResponse::json(json!(items)))
}

/// GET /api/user/inbox —— 站内信分页（新增契约端点）。
#[handler]
pub async fn inbox(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let page: i64 = request
        .query_param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
        .max(1);
    let per_page: i64 = request
        .query_param("per_page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(10)
        .clamp(1, 100);

    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let connection = DB::connection()?;
    let rows = Notification::find()
        .filter(crate::models::notification::Column::NotifiableType.eq("App\\Models\\User"))
        .filter(
            crate::models::notification::Column::NotifiableId
                .eq(suprnova::eloquent::StoredU64::from(user.uid)),
        )
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    let mut visible_rows = Vec::new();
    for row in rows {
        let data = parse_data(&row.data);
        if visible(&data).await? {
            visible_rows.push((row, data));
        }
    }
    visible_rows.sort_by(|left, right| right.0.created_at.cmp(&left.0.created_at));

    let total = visible_rows.len() as i64;
    let unread = visible_rows
        .iter()
        .filter(|(row, _)| row.read_at.is_none())
        .count() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;

    let mut items = Vec::new();
    for (row, data) in visible_rows.iter().skip(offset).take(per_page as usize) {
        let content = content_of(data).await?;
        items.push(json!({
            "id": row.id,
            "title": title_of(data).await?,
            "preview": preview(&content, 120),
            "format": data.get("format").and_then(Value::as_str).unwrap_or("html"),
            "read_at": row.read_at.map(format_time),
            "created_at": row.created_at.map(format_time),
            "communication_id": data.get("communication_id").cloned().unwrap_or(Value::Null),
        }));
    }

    Ok(HttpResponse::json(json!({
        "data": items,
        "current_page": page,
        "last_page": last_page,
        "per_page": per_page,
        "total": total,
        "unread": unread,
    })))
}

/// GET /api/user/inbox/{id} —— 站内信详情（自动已读，新增契约端点）。
#[handler]
pub async fn message(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let id = request.param("id").unwrap_or_default().to_string();

    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let connection = DB::connection()?;
    let row = Notification::find()
        .filter(crate::models::notification::Column::Id.eq(id.clone()))
        .filter(crate::models::notification::Column::NotifiableType.eq("App\\Models\\User"))
        .filter(
            crate::models::notification::Column::NotifiableId
                .eq(suprnova::eloquent::StoredU64::from(user.uid)),
        )
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let Some(row) = row else {
        return Ok(crate::support::api::not_found());
    };
    let data = parse_data(&row.data);
    if !visible(&data).await? {
        return Ok(crate::support::api::not_found());
    }

    let read_at = match row.read_at {
        Some(value) => value,
        None => {
            let now = Utc::now().naive_utc();
            Notification::update_many()
                .col_expr(
                    crate::models::notification::Column::ReadAt,
                    sea_orm::sea_query::Expr::value(Some(now)),
                )
                .filter(crate::models::notification::Column::Id.eq(id))
                .exec(connection.inner())
                .await
                .map_err(|error| FrameworkError::database(error.to_string()))?;
            now
        }
    };

    Ok(crate::support::api::success(
        "",
        Some(json!({
            "id": row.id,
            "title": title_of(&data).await?,
            "content": content_of(&data).await?,
            "format": data.get("format").and_then(Value::as_str).unwrap_or("html"),
            "read_at": format_time(read_at),
            "created_at": row.created_at.map(format_time),
            "communication_id": data.get("communication_id").cloned().unwrap_or(Value::Null),
        })),
    ))
}

/// 与 `Communications::content($notification)` 对齐。
async fn content_of(data: &Value) -> Result<String, FrameworkError> {
    if let Some(communication_id) = data.get("communication_id").and_then(Value::as_u64) {
        return Ok(match Communication::find(communication_id).await? {
            Some(communication) => render_communication(&communication),
            None => String::new(),
        });
    }
    Ok(markdown(
        data.get("content").and_then(Value::as_str).unwrap_or(""),
    ))
}

fn format_time(value: chrono::NaiveDateTime) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 纯文本预览（最多 `limit` 个字符）。
fn preview(content: &str, limit: usize) -> String {
    let mut plain = String::with_capacity(content.len());
    let mut in_tag = false;
    for ch in content.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                plain.push(' ');
            }
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    let plain = plain
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    plain.chars().take(limit).collect()
}

#[handler]
pub async fn read(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Notification.Read"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let id = request.param("id").unwrap_or_default().to_string();

    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let connection = suprnova::DB::connection()?;
    let row = Notification::find()
        .filter(crate::models::notification::Column::Id.eq(id.clone()))
        .filter(crate::models::notification::Column::NotifiableType.eq("App\\Models\\User"))
        .filter(
            crate::models::notification::Column::NotifiableId
                .eq(suprnova::eloquent::StoredU64::from(user.uid)),
        )
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let Some(row) = row else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let data = parse_data(&row.data);
    if !visible(&data).await? {
        return Ok(HttpResponse::text("Not Found").status(404));
    }

    let title = title_of(&data).await?;
    let content =
        if let Some(communication_id) = data.get("communication_id").and_then(Value::as_u64) {
            match Communication::find(communication_id).await? {
                Some(communication) => render_communication(&communication),
                None => String::new(),
            }
        } else {
            markdown(data.get("content").and_then(Value::as_str).unwrap_or(""))
        };
    let time = (row.created_at.unwrap_or_else(|| Utc::now().naive_utc()))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    Notification::update_many()
        .col_expr(
            crate::models::notification::Column::ReadAt,
            sea_orm::sea_query::Expr::value(Some(Utc::now().naive_utc())),
        )
        .filter(crate::models::notification::Column::Id.eq(id))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    Ok(HttpResponse::json(json!({
        "title": title,
        "content": content,
        "time": time,
    })))
}
