//! 衣柜：`/api/closet` 系列（PHP `ClosetController`）。
//!
//! `user_closet` 无主键，SeaORM 侧以 `user_uid` 作为逻辑主键；所有查询都按
//! `(user_uid, texture_tid)` 组合过滤，插入/删除按 `delete_many` 处理。

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, sea_query::Expr};
use serde::Deserialize;
use serde_json::{Value, json};
use suprnova::{DB, FrameworkError, HttpResponse, Model, Request, Response, attrs, handler};

use crate::models::closet_item::{
    ActiveModel as ClosetActiveModel, Column as ClosetColumn, Entity as ClosetItem,
};
use crate::models::texture::{Column as TextureColumn, Entity as Texture, Model as TextureModel};
use crate::models::user::User;
use crate::support::api::{failure, require_user, success, validation_error};
use crate::support::options;

fn texture_json(texture: &TextureModel, user_uid: i32, item_name: Option<&str>) -> Value {
    json!({
        "tid": texture.tid,
        "name": texture.name,
        "type": texture.texture_type,
        "hash": texture.hash,
        "size": texture.size,
        "uploader": texture.uploader,
        "public": texture.is_public(),
        "upload_at": texture.upload_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        "likes": texture.likes,
        "model": if texture.texture_type == "alex" { "slim" } else { "default" },
        "pivot": {
            "user_uid": user_uid,
            "texture_tid": texture.tid,
            "item_name": item_name,
        },
    })
}

/// 分页 URL：与 Laravel `LengthAwarePaginator` 一致，查询串只带 `page`。
fn page_url(base: &str, page: i64) -> String {
    format!("{base}?page={page}")
}

fn pagination_links(base: &str, page: i64, last_page: i64) -> Value {
    let mut links = Vec::new();
    links.push(json!({
        "url": if page > 1 { Some(page_url(base, page - 1)) } else { None },
        "label": "&laquo; Previous",
        "active": false,
    }));
    for index in 1..=last_page {
        links.push(json!({
            "url": page_url(base, index),
            "label": index.to_string(),
            "active": index == page,
        }));
    }
    links.push(json!({
        "url": if page < last_page { Some(page_url(base, page + 1)) } else { None },
        "label": "Next &raquo;",
        "active": false,
    }));
    Value::Array(links)
}

fn request_base(request: &Request) -> String {
    let app_url = std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost".to_string());
    format!("{}{}", app_url.trim_end_matches('/'), request.path())
}

/// GET /api/closet —— 衣柜分页列表。
#[handler]
pub async fn list(request: Request) -> Response {
    if let Err(response) = crate::support::api::require_scopes(
        &request,
        &["Closet.Read", "Closet.ReadWrite", "Closet.ReadWrtie"],
    )
    .await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let category = request
        .query_param("category")
        .unwrap_or_else(|| "skin".to_string());
    let search = request.query_param("q").unwrap_or_default();
    let page: i64 = request
        .query_param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
        .max(1);
    let per_page: i64 = request
        .query_param("perPage")
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(6);

    let connection = DB::connection()?;
    let closet = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(user.uid as i32))
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    let mut rows: Vec<(TextureModel, Option<String>)> = Vec::new();
    if !closet.is_empty() {
        let tids: Vec<u32> = closet.iter().map(|item| item.texture_tid as u32).collect();
        let textures = Texture::find()
            .filter(TextureColumn::Tid.is_in(tids))
            .all(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?;
        let mut by_id: std::collections::HashMap<u32, TextureModel> = textures
            .into_iter()
            .map(|texture| (texture.tid, texture))
            .collect();
        for item in closet {
            if let Some(texture) = by_id.remove(&(item.texture_tid as u32)) {
                rows.push((texture, item.item_name));
            }
        }
    }

    rows.retain(|(texture, item_name)| {
        let in_category = if category == "cape" {
            texture.texture_type == "cape"
        } else {
            texture.texture_type == "steve" || texture.texture_type == "alex"
        };
        let matches_search = search.is_empty()
            || item_name
                .as_deref()
                .map(|name| name.contains(&search))
                .unwrap_or(false);
        in_category && matches_search
    });
    rows.sort_by(|left, right| right.0.tid.cmp(&left.0.tid));

    let total = rows.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;
    let window: Vec<Value> = rows
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .map(|(texture, item_name)| texture_json(texture, user.uid as i32, item_name.as_deref()))
        .collect();
    let from = if window.is_empty() {
        None
    } else {
        Some(offset as i64 + 1)
    };
    let to = if window.is_empty() {
        None
    } else {
        Some(offset as i64 + window.len() as i64)
    };
    let base = request_base(&request);

    Ok(HttpResponse::json(json!({
        "current_page": page,
        "data": window,
        "first_page_url": page_url(&base, 1),
        "from": from,
        "last_page": last_page,
        "last_page_url": page_url(&base, last_page),
        "links": pagination_links(&base, page, last_page),
        "next_page_url": if page < last_page { Some(page_url(&base, page + 1)) } else { None },
        "path": base,
        "per_page": per_page,
        "prev_page_url": if page > 1 { Some(page_url(&base, page - 1)) } else { None },
        "to": to,
        "total": total,
    })))
}

/// GET /api/closet/ids —— 衣柜内所有材质 tid。
#[handler]
pub async fn ids(request: Request) -> Response {
    if let Err(response) = crate::support::api::require_scopes(
        &request,
        &["Closet.Read", "Closet.ReadWrite", "Closet.ReadWrtie"],
    )
    .await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let connection = DB::connection()?;
    let items = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(user.uid as i32))
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let ids: Vec<i32> = items.into_iter().map(|item| item.texture_tid).collect();
    Ok(HttpResponse::json(json!(ids)))
}

#[derive(Deserialize, Default)]
pub struct AddRequest {
    #[serde(default)]
    pub tid: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
}

/// POST /api/closet —— 收藏材质到衣柜。
#[handler]
pub async fn add(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Closet.ReadWrite", "Closet.ReadWrtie"])
            .await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let body: AddRequest = request.input().await?;
    let raw_tid = body.tid.unwrap_or(0);
    let name = body.name.unwrap_or_default();
    if raw_tid <= 0 {
        return Err(validation_error("tid", "tid 不能为空。").into());
    }
    let Some(tid) = u32::try_from(raw_tid).ok() else {
        return Err(validation_error("tid", "tid 不能为空。").into());
    };
    if name.is_empty() {
        return Err(validation_error("name", "名称 不能为空。").into());
    }

    let connection = DB::connection()?;
    let cost = options::int_or("score_per_closet_item", 0).await?;
    if (user.score as i64) < cost {
        return Ok(failure("收藏失败，积分不足", 1));
    }

    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(tid))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let Some(texture) = texture else {
        return Ok(failure("该材质不存在", 1));
    };
    if !texture.is_public() && texture.uploader != user.uid as i32 && !user.is_admin() {
        return Ok(failure("请求的材质已经设为私密，仅上传者和管理员可查看", 1));
    }

    let existing = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(user.uid as i32))
        .filter(ClosetColumn::TextureTid.eq(tid as i32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    if existing.is_some() {
        return Ok(failure("你已经收藏过这个材质啦", 1));
    }

    ClosetItem::insert(ClosetActiveModel {
        user_uid: sea_orm::Set(user.uid as i32),
        texture_tid: sea_orm::Set(tid as i32),
        item_name: sea_orm::Set(Some(name.clone())),
    })
    .exec(connection.inner())
    .await
    .map_err(|error| FrameworkError::database(error.to_string()))?;

    let next_score = user.score - cost as i32;
    let uid = user.uid;
    user.update(attrs! { score: next_score }).await?;

    Texture::update_many()
        .col_expr(
            TextureColumn::Likes,
            sea_orm::sea_query::ExprTrait::add(Expr::col(TextureColumn::Likes), 1),
        )
        .filter(TextureColumn::Tid.eq(tid))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    if texture.uploader != uid as i32
        && let Ok(Some(uploader)) = User::find(texture.uploader as u32).await
    {
        let award = options::int_or("score_award_per_like", 0).await?;
        if award != 0 {
            let next_score = uploader.score + award as i32;
            uploader.update(attrs! { score: next_score }).await?;
        }
    }

    Ok(success(format!("材质 {name} 收藏成功"), None))
}

#[derive(Deserialize, Default)]
pub struct RenameRequest {
    #[serde(default)]
    pub name: Option<String>,
}

/// PUT /api/closet/{tid} —— 重命名衣柜条目。
#[handler]
pub async fn rename(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Closet.ReadWrite", "Closet.ReadWrtie"])
            .await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let tid = request
        .param("tid")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);
    let body: RenameRequest = request.input().await?;
    let name = body.name.unwrap_or_default();
    if name.is_empty() {
        return Err(validation_error("name", "名称 不能为空。").into());
    }

    let connection = DB::connection()?;
    let item = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(user.uid as i32))
        .filter(ClosetColumn::TextureTid.eq(tid))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    if item.is_none() {
        return Ok(failure("衣柜中不存在此材质", 1));
    }

    ClosetItem::update_many()
        .col_expr(ClosetColumn::ItemName, Expr::value(Some(name.clone())))
        .filter(ClosetColumn::UserUid.eq(user.uid as i32))
        .filter(ClosetColumn::TextureTid.eq(tid))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    Ok(success(format!("衣柜物品成功重命名至 {name}"), None))
}

/// DELETE /api/closet/{tid} —— 从衣柜移除并返还积分。
#[handler]
pub async fn remove(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Closet.ReadWrite", "Closet.ReadWrtie"])
            .await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let tid = request
        .param("tid")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);

    let connection = DB::connection()?;
    let item = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(user.uid as i32))
        .filter(ClosetColumn::TextureTid.eq(tid))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    if item.is_none() {
        return Ok(failure("衣柜中不存在此材质", 1));
    }

    ClosetItem::delete_many()
        .filter(ClosetColumn::UserUid.eq(user.uid as i32))
        .filter(ClosetColumn::TextureTid.eq(tid))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    let uid = user.uid;
    if options::bool_or("return_score", true).await? {
        let cost = options::int_or("score_per_closet_item", 0).await?;
        let next_score = user.score + cost as i32;
        user.update(attrs! { score: next_score }).await?;
    }

    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(tid as u32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    Texture::update_many()
        .col_expr(
            TextureColumn::Likes,
            sea_orm::sea_query::ExprTrait::sub(Expr::col(TextureColumn::Likes), 1),
        )
        .filter(TextureColumn::Tid.eq(tid as u32))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    if let Some(texture) = texture
        && texture.uploader != uid as i32
        && let Ok(Some(uploader)) = User::find(texture.uploader as u32).await
    {
        let award = options::int_or("score_award_per_like", 0).await?;
        if award != 0 {
            let next_score = uploader.score - award as i32;
            uploader.update(attrs! { score: next_score }).await?;
        }
    }

    Ok(success("材质已从衣柜中移除", None))
}
