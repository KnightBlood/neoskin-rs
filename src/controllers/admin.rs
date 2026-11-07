//! 后台管理（PHP `UsersManagementController` / `PlayersManagementController`
//! / `ClosetManagementController` / `NotificationsController@send`）。

use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::{Value, json};
use suprnova::{DB, FrameworkError, HttpResponse, Model, Request, Response, attrs, handler};

use crate::controllers::player::validate_name;
use crate::models::closet_item::{
    ActiveModel as ClosetActiveModel, Column as ClosetColumn, Entity as ClosetItem,
};
use crate::models::communication::{Communication, EDITOR_SOURCE, KIND_MESSAGE, STATUS_PUBLISHED};
use crate::models::notification::{ActiveModel as NotificationActiveModel, Entity as Notification};
use crate::models::player::Player;
use crate::models::texture::{Column as TextureColumn, Entity as Texture};
use crate::models::user::{
    PERMISSION_ADMIN, PERMISSION_BANNED, PERMISSION_NORMAL, PERMISSION_SUPER, User,
};
use crate::support::api::{require_user, validation_error};
use crate::support::hash::hash_password;
use crate::support::options;
use crate::support::remember::random_token;

async fn require_admin(request: &Request) -> Result<User, Response> {
    let user = match require_user(request).await {
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

/// GET /api/admin/dashboard —— 管理端统计（共享前端 P4）。
#[handler]
pub async fn dashboard(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }

    let user_count = User::all().await?.into_vec().len() as i64;
    let player_count = Player::all().await?.into_vec().len() as i64;
    let connection = DB::connection()?;
    let textures = Texture::find()
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let texture_count = textures.len() as i64;
    let storage: i64 = textures.iter().map(|texture| texture.size as i64).sum();
    let storage_human = if storage > 1024 {
        format!("{:.1}MB", storage as f64 / 1024.0)
    } else {
        format!("{storage}KB")
    };

    Ok(HttpResponse::json(json!({
        "users": user_count,
        "players": player_count,
        "textures": texture_count,
        "storage": storage,
        "storageHuman": storage_human,
    })))
}

/// 供页面控制器复用（`admin.users` 的 `extra.currentUser`）。
pub(crate) fn user_json_public(user: &User) -> Value {
    user_json(user)
}

fn user_json(user: &User) -> Value {
    json!({
        "uid": user.uid,
        "email": user.email,
        "nickname": user.nickname,
        "locale": user.locale,
        "score": user.score,
        "avatar": user.avatar,
        "ip": user.ip,
        "permission": user.permission,
        "last_sign_at": user.last_sign_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        "register_at": user.register_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        "verified": user.verified,
        "is_dark_mode": user.is_dark_mode,
        "verification_token": user.verification_token,
    })
}

fn player_json(player: &Player) -> Value {
    json!({
        "pid": player.pid,
        "uid": player.uid,
        "name": player.name,
        "tid_skin": player.tid_skin,
        "tid_cape": player.tid_cape,
        "last_modified": player.last_modified.format("%Y-%m-%d %H:%M:%S").to_string(),
    })
}

fn app_base(request: &Request) -> String {
    let app_url = std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost".to_string());
    format!("{}{}", app_url.trim_end_matches('/'), request.path())
}

fn paginator(request: &Request, data: Vec<Value>, total: i64, page: i64, per_page: i64) -> Value {
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let base = app_base(request);
    let page_url = |page: i64| format!("{base}?page={page}");
    let from = if data.is_empty() {
        None
    } else {
        Some((page - 1) * per_page + 1)
    };
    let to = if data.is_empty() {
        None
    } else {
        Some((page - 1) * per_page + data.len() as i64)
    };
    json!({
        "current_page": page,
        "data": data,
        "first_page_url": page_url(1),
        "from": from,
        "last_page": last_page,
        "last_page_url": page_url(last_page),
        "next_page_url": if page < last_page { Some(page_url(page + 1)) } else { None },
        "path": base,
        "per_page": per_page,
        "prev_page_url": if page > 1 { Some(page_url(page - 1)) } else { None },
        "to": to,
        "total": total,
    })
}

fn requested_page(request: &Request) -> i64 {
    request
        .query_param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
        .max(1)
}

// ---------------------------------------------------------------------------
// 用户管理
// ---------------------------------------------------------------------------

/// GET /admin/users/list 与 GET /api/admin/users
#[handler]
pub async fn users_list(request: Request) -> Response {
    if let Err(response) = crate::support::api::require_scopes(
        &request,
        &["UsersManagement.Read", "UsersManagement.ReadWrite"],
    )
    .await
    {
        return Ok(response);
    }
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let query = request.query_param("q").unwrap_or_default();
    let page = requested_page(&request);
    let per_page = 10i64;

    let mut users: Vec<User> = User::all().await?.into_vec();
    users.sort_by(|a, b| a.uid.cmp(&b.uid));
    if !query.is_empty() {
        let needle = query.to_ascii_lowercase();
        users.retain(|user| {
            user.email.to_ascii_lowercase().contains(&needle)
                || user.nickname.to_ascii_lowercase().contains(&needle)
                || user.uid.to_string() == query
                || user.ip.contains(&query)
        });
    }
    let total = users.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;
    let data: Vec<Value> = users
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .map(user_json)
        .collect();
    Ok(HttpResponse::json(paginator(
        &request, data, total, page, per_page,
    )))
}

/// 目标用户与权限检查（PHP `UsersManagementController` 构造器）。
async fn target_user(request: &Request, admin: &User) -> Result<Option<User>, FrameworkError> {
    let Some(uid) = request
        .param("user")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(None);
    };
    let Some(target) = User::find(uid).await? else {
        return Ok(None);
    };
    if target.uid != admin.uid && target.permission >= admin.permission {
        return Ok(None);
    }
    Ok(Some(target))
}

#[derive(Deserialize, Default)]
pub struct AdminEmailRequest {
    #[serde(default)]
    pub email: String,
}

/// PUT /admin/users/{user}/email
#[handler]
pub async fn user_email(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["UsersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(target) = target_user(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminEmailRequest = request.input().await?;
    let email = body.email.trim().to_string();
    if email.is_empty() {
        return Err(validation_error("email", "邮箱 不能为空。").into());
    }
    if !email.contains('@') {
        return Err(validation_error("email", "邮箱 格式不正确。").into());
    }
    if email != target.email
        && User::filter("email", email.clone())
            .first()
            .await?
            .is_some()
    {
        return Err(validation_error("email", "邮箱 已经被占用。").into());
    }
    target.update(attrs! { email: email }).await?;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "邮箱修改成功" }),
    ))
}

/// PUT /admin/users/{user}/verification
#[handler]
pub async fn user_verification(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["UsersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(target) = target_user(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let next = !target.verified;
    target.update(attrs! { verified: next }).await?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": "用户的邮箱验证状态已修改",
    })))
}

#[derive(Deserialize, Default)]
pub struct AdminNicknameRequest {
    #[serde(default)]
    pub nickname: String,
}

/// PUT /admin/users/{user}/nickname
#[handler]
pub async fn user_nickname(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["UsersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(target) = target_user(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminNicknameRequest = request.input().await?;
    let nickname = body.nickname.trim().to_string();
    if nickname.is_empty() {
        return Err(validation_error("nickname", "昵称 不能为空。").into());
    }
    target.update(attrs! { nickname: nickname.clone() }).await?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("昵称已成功设置为 {nickname}"),
    })))
}

#[derive(Deserialize, Default)]
pub struct AdminPasswordRequest {
    #[serde(default)]
    pub password: String,
}

/// PUT /admin/users/{user}/password
#[handler]
pub async fn user_password(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["UsersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(target) = target_user(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminPasswordRequest = request.input().await?;
    let length = body.password.chars().count();
    if !(8..=16).contains(&length) {
        return Err(validation_error("password", "密码 必须介于 8 - 16 个字符之间。").into());
    }
    let hash = hash_password(&body.password, None)
        .map_err(|error| FrameworkError::internal(format!("密码哈希失败: {error}")))?;
    target.update(attrs! { password: hash }).await?;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "密码修改成功" }),
    ))
}

#[derive(Deserialize, Default)]
pub struct AdminScoreRequest {
    #[serde(default)]
    pub score: Option<i64>,
}

/// PUT /admin/users/{user}/score
#[handler]
pub async fn user_score(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["UsersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(target) = target_user(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminScoreRequest = request.input().await?;
    let Some(score) = body.score else {
        return Err(validation_error("score", "score 不能为空。").into());
    };
    target.update(attrs! { score: score as i32 }).await?;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "积分修改成功" }),
    ))
}

#[derive(Deserialize, Default)]
pub struct AdminPermissionRequest {
    #[serde(default)]
    pub permission: Option<i64>,
}

/// PUT /admin/users/{user}/permission
#[handler]
pub async fn user_permission(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["UsersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(target) = target_user(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminPermissionRequest = request.input().await?;
    let Some(permission) = body.permission else {
        return Err(validation_error("permission", "permission 不能为空。").into());
    };
    if ![PERMISSION_BANNED, PERMISSION_NORMAL, PERMISSION_ADMIN].contains(&(permission as i32)) {
        return Err(validation_error("permission", "permission 不在允许的范围内。").into());
    }
    if permission as i32 == PERMISSION_ADMIN && admin.permission < PERMISSION_SUPER {
        return Ok(HttpResponse::text("Forbidden").status(403));
    }
    if target.uid == admin.uid {
        return Ok(HttpResponse::text("Forbidden").status(403));
    }
    target
        .update(attrs! { permission: permission as i32 })
        .await?;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "权限已更改" }),
    ))
}

/// DELETE /admin/users/{user}
#[handler]
pub async fn user_delete(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["UsersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(target) = target_user(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let uid = target.uid;
    // PHP `User::delete()` 先删除该用户的全部角色。
    for player in Player::filter("uid", uid).get().await?.into_vec() {
        player.delete().await?;
    }
    target.delete().await?;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "账号已被成功删除" }),
    ))
}

// ---------------------------------------------------------------------------
// 角色管理
// ---------------------------------------------------------------------------

/// GET /admin/players/list 与 GET /api/admin/players
#[handler]
pub async fn players_list(request: Request) -> Response {
    if let Err(response) = crate::support::api::require_scopes(
        &request,
        &["PlayersManagement.Read", "PlayersManagement.ReadWrite"],
    )
    .await
    {
        return Ok(response);
    }
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let query = request.query_param("q").unwrap_or_default();
    let page = requested_page(&request);
    let per_page = 10i64;

    let mut players: Vec<Player> = Player::all().await?.into_vec();
    players.sort_by(|a, b| a.pid.cmp(&b.pid));
    if !query.is_empty() {
        let needle = query.to_ascii_lowercase();
        players.retain(|player| {
            player.name.to_ascii_lowercase().contains(&needle)
                || player.pid.to_string() == query
                || player.uid.to_string() == query
        });
    }
    let total = players.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;
    let data: Vec<Value> = players
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .map(player_json)
        .collect();
    Ok(HttpResponse::json(paginator(
        &request, data, total, page, per_page,
    )))
}

async fn target_player(request: &Request, admin: &User) -> Result<Option<Player>, FrameworkError> {
    let Some(pid) = request
        .param("player")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(None);
    };
    let Some(player) = Player::find(pid).await? else {
        return Ok(None);
    };
    let owner = User::find(player.uid as u32).await?;
    if let Some(owner) = owner
        && owner.uid != admin.uid
        && owner.permission >= admin.permission
    {
        return Ok(None);
    }
    Ok(Some(player))
}

#[derive(Deserialize, Default)]
pub struct AdminPlayerNameRequest {
    #[serde(default)]
    pub player_name: String,
}

/// PUT /admin/players/{player}/name
#[handler]
pub async fn player_name(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["PlayersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(player) = target_player(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminPlayerNameRequest = request.input().await?;
    let name = body.player_name.trim().to_string();
    if let Some(message) = validate_name(&name).await? {
        return Err(validation_error("player_name", &message).into());
    }
    let length = name.chars().count() as i64;
    let min = options::int_or("player_name_length_min", 3).await?;
    let max = options::int_or("player_name_length_max", 16).await?;
    if length < min || length > max {
        return Err(validation_error(
            "player_name",
            &format!("角色名最少要求 {min} 个字符，最多不超过 {max} 个字符。"),
        )
        .into());
    }
    if let Some(existing) = Player::filter("name", name.clone()).first().await?
        && existing.pid != player.pid
    {
        return Err(validation_error("player_name", "该角色名已被占用").into());
    }
    player
        .update(attrs! {
            name: name.clone(),
            last_modified: Utc::now(),
        })
        .await?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("角色名成功更改为 {name}"),
    })))
}

#[derive(Deserialize, Default)]
pub struct AdminPlayerOwnerRequest {
    #[serde(default)]
    pub uid: Option<i64>,
}

/// PUT /admin/players/{player}/owner
#[handler]
pub async fn player_owner(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["PlayersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(player) = target_player(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminPlayerOwnerRequest = request.input().await?;
    let Some(uid) = body.uid else {
        return Err(validation_error("uid", "uid 不能为空。").into());
    };
    let Ok(uid) = u32::try_from(uid) else {
        return Ok(HttpResponse::json(
            json!({ "code": 1, "message": "用户不存在" }),
        ));
    };
    let Some(owner) = User::find(uid).await? else {
        return Ok(HttpResponse::json(
            json!({ "code": 1, "message": "用户不存在" }),
        ));
    };
    let name = player.name.clone();
    player.update(attrs! { uid: uid as i32 }).await?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("角色 {name} 已被转给用户 {} 。", owner.nickname),
    })))
}

#[derive(Deserialize, Default)]
pub struct AdminPlayerTextureRequest {
    #[serde(default)]
    pub tid: Option<i64>,
    #[serde(rename = "type", default)]
    pub texture_type: String,
}

/// PUT /admin/players/{player}/textures
#[handler]
pub async fn player_texture(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["PlayersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(player) = target_player(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    let body: AdminPlayerTextureRequest = request.input().await?;
    let Some(tid) = body.tid else {
        return Err(validation_error("tid", "tid 不能为空。").into());
    };
    if !["skin", "cape"].contains(&body.texture_type.as_str()) {
        return Err(validation_error("type", "type 不在允许的范围内。").into());
    }
    if tid != 0 {
        let connection = DB::connection()?;
        let exists = Texture::find()
            .filter(TextureColumn::Tid.eq(tid as i32))
            .one(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?
            .is_some();
        if !exists {
            return Ok(HttpResponse::json(json!({
                "code": 1,
                "message": format!("材质 tid.{tid} 不存在"),
            })));
        }
    }
    let name = player.name.clone();
    let mut tid_skin = player.tid_skin;
    let mut tid_cape = player.tid_cape;
    if body.texture_type == "skin" {
        tid_skin = tid as i32;
    } else {
        tid_cape = tid as i32;
    }
    player
        .update(attrs! {
            tid_skin: tid_skin,
            tid_cape: tid_cape,
            last_modified: Utc::now(),
        })
        .await?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("角色 {name} 的材质修改成功"),
    })))
}

/// DELETE /admin/players/{player}
#[handler]
pub async fn player_delete(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["PlayersManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let Some(player) = target_player(&request, &admin).await? else {
        return Ok(HttpResponse::text("Forbidden").status(403));
    };
    player.delete().await?;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "角色已被成功删除" }),
    ))
}

// ---------------------------------------------------------------------------
// 衣柜管理
// ---------------------------------------------------------------------------

async fn admin_closet_rows(uid: i32) -> Result<Vec<Value>, FrameworkError> {
    let connection = DB::connection()?;
    let items = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(uid))
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let mut rows = Vec::new();
    for item in items {
        let texture = Texture::find()
            .filter(TextureColumn::Tid.eq(item.texture_tid))
            .one(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?;
        if let Some(texture) = texture {
            rows.push(json!({
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
                    "user_uid": item.user_uid,
                    "texture_tid": item.texture_tid,
                    "item_name": item.item_name,
                },
            }));
        }
    }
    Ok(rows)
}

fn target_uid(request: &Request) -> Option<i32> {
    request
        .param("user")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
}

/// GET /api/admin/closet/{user}
#[handler]
pub async fn closet_list(request: Request) -> Response {
    if let Err(response) = crate::support::api::require_scopes(
        &request,
        &["ClosetManagement.Read", "ClosetManagement.ReadWrite"],
    )
    .await
    {
        return Ok(response);
    }
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let Some(uid) = target_uid(&request) else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if User::find(uid as u32).await?.is_none() {
        return Ok(HttpResponse::text("Not Found").status(404));
    }
    Ok(HttpResponse::json(json!(admin_closet_rows(uid).await?)))
}

#[derive(Deserialize, Default)]
pub struct AdminClosetRequest {
    #[serde(default)]
    pub tid: Option<i64>,
}

/// POST /admin/closet/{user}
#[handler]
pub async fn closet_add(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["ClosetManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let Some(uid) = target_uid(&request) else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let body: AdminClosetRequest = request.input().await?;
    let Some(tid) = body.tid else {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": "该材质不存在",
        })));
    };
    let connection = DB::connection()?;
    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(tid as i32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let Some(texture) = texture else {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": "该材质不存在",
        })));
    };
    let existing = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(uid))
        .filter(ClosetColumn::TextureTid.eq(tid as i32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    if existing.is_some() {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": "你已经收藏过这个材质啦",
        })));
    }
    ClosetItem::insert(ClosetActiveModel {
        user_uid: sea_orm::Set(uid),
        texture_tid: sea_orm::Set(tid as i32),
        item_name: sea_orm::Set(Some(texture.name.clone())),
    })
    .exec(connection.inner())
    .await
    .map_err(|error| FrameworkError::database(error.to_string()))?;
    let user = User::find(uid as u32).await?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": "",
        "data": {
            "user": user.as_ref().map(user_json),
            "texture": json!({
                "tid": texture.tid,
                "name": texture.name,
                "type": texture.texture_type,
                "hash": texture.hash,
                "size": texture.size,
                "uploader": texture.uploader,
                "public": texture.is_public(),
            }),
        },
    })))
}

/// DELETE /admin/closet/{user}
#[handler]
pub async fn closet_remove(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["ClosetManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let Some(uid) = target_uid(&request) else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let body: AdminClosetRequest = request.input().await?;
    let Some(tid) = body.tid else {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": "衣柜中不存在此材质",
        })));
    };
    let connection = DB::connection()?;
    let item = ClosetItem::find()
        .filter(ClosetColumn::UserUid.eq(uid))
        .filter(ClosetColumn::TextureTid.eq(tid as i32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    if item.is_none() {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": "衣柜中不存在此材质",
        })));
    }
    ClosetItem::delete_many()
        .filter(ClosetColumn::UserUid.eq(uid))
        .filter(ClosetColumn::TextureTid.eq(tid as i32))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(tid as i32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let user = User::find(uid as u32).await?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": "",
        "data": {
            "user": user.as_ref().map(user_json),
            "texture": texture.map(|texture| json!({
                "tid": texture.tid,
                "name": texture.name,
                "type": texture.texture_type,
                "hash": texture.hash,
                "size": texture.size,
                "uploader": texture.uploader,
                "public": texture.is_public(),
            })),
        },
    })))
}

// ---------------------------------------------------------------------------
// 发送通知
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct SendNotificationRequest {
    #[serde(default)]
    pub receiver: String,
    #[serde(default)]
    pub uid: Option<i64>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
}

/// POST /admin/notifications/send 与 POST /api/admin/notifications
#[handler]
pub async fn send_notification(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Notification.ReadWrite"]).await
    {
        return Ok(response);
    }
    let admin = match require_admin(&request).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let body: SendNotificationRequest = request.input().await?;
    if !["all", "normal", "uid", "email"].contains(&body.receiver.as_str()) {
        return Err(validation_error("receiver", "receiver 不在允许的范围内。").into());
    }
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return Err(validation_error("title", "标题 不能为空。").into());
    }
    if title.chars().count() > 20 {
        return Err(validation_error("title", "标题 不能大于 20 个字符。").into());
    }
    let content = body.content.clone().unwrap_or_default();

    let created_at = Utc::now().naive_utc();
    let delivery_token = random_token();
    let audience = json!({
        "receiver": body.receiver,
        "uid": body.uid,
        "email": body.email,
    });
    let communication = Communication::create(attrs! {
        author_id: Some(admin.uid),
        delivery_token: Some(delivery_token.clone()),
        title: title.clone(),
        kind: KIND_MESSAGE,
        status: STATUS_PUBLISHED,
        locale: "",
        editor: EDITOR_SOURCE,
        source: Some(content.clone()),
        audience: Some(audience.to_string()),
        published_at: Some(Utc::now()),
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
    })
    .await?;

    let recipients: Vec<User> = match body.receiver.as_str() {
        "normal" => User::filter("permission", PERMISSION_NORMAL)
            .get()
            .await?
            .into_vec(),
        "uid" => match body.uid {
            Some(uid) => User::filter("uid", uid as i32).get().await?.into_vec(),
            None => Vec::new(),
        },
        "email" => match body.email.as_deref() {
            Some(email) => User::filter("email", email).get().await?.into_vec(),
            None => Vec::new(),
        },
        _ => User::all().await?.into_vec(),
    };

    let connection = DB::connection()?;
    for user in recipients {
        let data = json!({
            "title": title.clone(),
            "content": content.clone(),
            "format": "html",
            "communication_id": communication.id,
            "delivery_token": delivery_token.clone(),
        });
        Notification::insert(NotificationActiveModel {
            id: sea_orm::Set(crate::support::id::uuid_v4()),
            notification_type: sea_orm::Set("App\\Notifications\\SiteMessage".to_string()),
            notifiable_type: sea_orm::Set("App\\Models\\User".to_string()),
            notifiable_id: sea_orm::Set(suprnova::eloquent::StoredU64::from(user.uid as u64)),
            data: sea_orm::Set(data.to_string()),
            read_at: sea_orm::Set(None),
            created_at: sea_orm::Set(Some(created_at)),
            updated_at: sea_orm::Set(Some(created_at)),
        })
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    }

    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin"))
}

// ---------------------------------------------------------------------------
// 翻译覆盖（PHP `TranslationsController`）
// ---------------------------------------------------------------------------

use crate::models::language_line::LanguageLine;

/// 当前语言：Accept-Language 首选，规范化成 `zh_CN` 形式；默认 `zh_CN`。
fn current_locale(request: &Request) -> String {
    let raw = request
        .header("accept-language")
        .and_then(|value| value.split(',').next())
        .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
        .unwrap_or_default();
    if raw.is_empty() {
        return "zh_CN".to_string();
    }
    raw.replace('-', "_")
}

/// GET /admin/i18n/list
#[handler]
pub async fn translations_list(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let page = requested_page(&request);
    let per_page = 10i64;
    let mut lines: Vec<LanguageLine> = LanguageLine::all().await?.into_vec();
    lines.sort_by(|a, b| a.id.cmp(&b.id));
    let total = lines.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;
    let data: Vec<Value> = lines
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .map(|line| {
            json!({
                "id": line.id,
                "group": line.group,
                "key": line.key,
                "text": line.text,
                "created_at": line.created_at.map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string()),
                "updated_at": line.updated_at.map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string()),
            })
        })
        .collect();
    Ok(HttpResponse::json(paginator(
        &request, data, total, page, per_page,
    )))
}

#[derive(Deserialize, Default)]
pub struct TranslationCreateRequest {
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub text: String,
}

/// POST /admin/i18n
#[handler]
pub async fn translation_create(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let locale = current_locale(&request);
    let body: TranslationCreateRequest = request.input().await?;
    if body.group.trim().is_empty() {
        return Err(validation_error("group", "分组 不能为空。").into());
    }
    if body.key.trim().is_empty() {
        return Err(validation_error("key", "键 不能为空。").into());
    }
    if body.text.is_empty() {
        return Err(validation_error("text", "文本 不能为空。").into());
    }
    let text = json!({ locale: body.text }).to_string();
    let now = Utc::now();
    LanguageLine::create(attrs! {
        group: body.group,
        key: body.key,
        text: text,
        created_at: Some(now),
        updated_at: Some(now),
    })
    .await?;
    let _ = crate::support::lang::generate().await;
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/i18n"))
}

#[derive(Deserialize, Default)]
pub struct TranslationUpdateRequest {
    #[serde(default)]
    pub text: String,
}

async fn target_line(request: &Request) -> Result<Option<LanguageLine>, FrameworkError> {
    let Some(id) = request
        .param("line")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(None);
    };
    LanguageLine::find(id).await
}

/// PUT /admin/i18n/{line}
#[handler]
pub async fn translation_update(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let locale = current_locale(&request);
    let Some(line) = target_line(&request).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let body: TranslationUpdateRequest = request.input().await?;
    if body.text.is_empty() {
        return Err(validation_error("text", "文本 不能为空。").into());
    }
    let mut translations: Value = serde_json::from_str(&line.text).unwrap_or_else(|_| json!({}));
    if let Some(object) = translations.as_object_mut() {
        object.insert(locale, Value::String(body.text));
    }
    let text = translations.to_string();
    line.update(attrs! {
        text: text,
        updated_at: Some(Utc::now()),
    })
    .await?;
    let _ = crate::support::lang::generate().await;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "条目更新成功" }),
    ))
}

/// DELETE /admin/i18n/{line}
#[handler]
pub async fn translation_delete(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let Some(line) = target_line(&request).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    line.delete().await?;
    let _ = crate::support::lang::generate().await;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "条目已删除" }),
    ))
}

// ---------------------------------------------------------------------------
// 站点选项（PHP `OptionsController` 的 POST 处理）
// ---------------------------------------------------------------------------

fn form_text(body: &Value, key: &str) -> Option<String> {
    body.get(key).map(|value| match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn form_flag(body: &Value, key: &str) -> bool {
    match body.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_i64().map(|value| value != 0).unwrap_or(false),
        Some(Value::String(value)) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        _ => false,
    }
}

async fn save_text(body: &Value, key: &str) -> Result<(), FrameworkError> {
    if let Some(value) = form_text(body, key) {
        options::set(key, &value).await?;
    }
    Ok(())
}

async fn save_texts(body: &Value, keys: &[&str]) -> Result<(), FrameworkError> {
    for key in keys {
        save_text(body, key).await?;
    }
    Ok(())
}

async fn save_flags(body: &Value, keys: &[&str]) -> Result<(), FrameworkError> {
    for key in keys {
        let value = if form_flag(body, key) {
            "true"
        } else {
            "false"
        };
        options::set(key, value).await?;
    }
    Ok(())
}

/// POST /admin/customize
#[handler]
pub async fn save_customize(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let locale = current_locale(&request);
    let body: Value = request.input().await?;

    if form_text(&body, "action").as_deref() == Some("color") {
        for key in ["navbar", "sidebar"] {
            if let Some(value) = form_text(&body, key)
                && !value.is_empty()
            {
                options::set(&format!("{key}_color"), &value).await?;
            }
        }
    }

    save_texts(
        &body,
        &["home_pic_url", "favicon_url", "custom_css", "custom_js"],
    )
    .await?;
    save_flags(&body, &["transparent_navbar", "hide_intro", "fixed_bg"]).await?;
    save_text(&body, &format!("copyright_prefer_{locale}")).await?;
    save_text(&body, &format!("copyright_text_{locale}")).await?;

    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/customize"))
}

/// POST /admin/score
#[handler]
pub async fn save_score(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let body: Value = request.input().await?;

    save_texts(
        &body,
        &[
            "score_per_storage",
            "private_score_per_storage",
            "score_per_closet_item",
            "score_per_player",
            "user_initial_score",
            "reporter_score_modification",
            "reporter_reward_score",
            "sign_gap_time",
            "score_award_per_texture",
            "score_award_per_like",
        ],
    )
    .await?;
    save_flags(
        &body,
        &[
            "return_score",
            "sign_after_zero",
            "take_back_scores_after_deletion",
        ],
    )
    .await?;

    let from = form_text(&body, "sign_score_from");
    let to = form_text(&body, "sign_score_to");
    if let (Some(from), Some(to)) = (from, to)
        && !from.is_empty()
        && !to.is_empty()
    {
        options::set("sign_score", &format!("{from},{to}")).await?;
    }

    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/score"))
}

fn normalize_site_url(url: &str) -> String {
    let mut value = url.trim().to_string();
    while value.ends_with('/') {
        value.pop();
    }
    if value.ends_with("/index.php") {
        value.truncate(value.len() - 10);
    }
    value
}

fn normalize_cdn(url: &str) -> String {
    let mut value = url.trim().to_string();
    while value.ends_with('/') {
        value.pop();
    }
    value
}

/// POST /admin/options
#[handler]
pub async fn save_options(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let locale = current_locale(&request);
    let body: Value = request.input().await?;

    if let Some(site_url) = form_text(&body, "site_url") {
        options::set("site_url", &normalize_site_url(&site_url)).await?;
    }
    save_texts(
        &body,
        &[
            "regs_per_ip",
            "max_upload_file_size",
            "max_texture_width",
            "player_name_rule",
            "custom_player_name_regexp",
            "player_name_length_min",
            "player_name_length_max",
            "status_code_for_private",
            "texture_name_regexp",
            "meta_keywords",
            "meta_description",
            "meta_extras",
            "recaptcha_sitekey",
            "recaptcha_secretkey",
            "update_source",
            "plugins_registry",
        ],
    )
    .await?;
    save_flags(
        &body,
        &[
            "register_with_player_name",
            "require_verification",
            "auto_del_invalid_texture",
            "allow_downloading_texture",
            "recaptcha_invisible",
        ],
    )
    .await?;

    for key in ["site_name", "site_description", "content_policy"] {
        if let Some(value) = form_text(&body, key) {
            options::set(key, &value).await?;
            options::set(&format!("{key}_{locale}"), &value).await?;
        }
    }

    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/options"))
}

/// POST /admin/resource
#[handler]
pub async fn save_resource(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return response;
    }
    let body: Value = request.input().await?;

    if let Some(cdn) = form_text(&body, "cdn_address") {
        options::set("cdn_address", &normalize_cdn(&cdn)).await?;
    }
    save_texts(&body, &["cache_expire_time"]).await?;
    save_flags(
        &body,
        &[
            "force_ssl",
            "auto_detect_asset_url",
            "enable_avatar_cache",
            "enable_preview_cache",
        ],
    )
    .await?;

    // `?clear-cache`：Suprnova 的缓存驱动未在本项目启用，暂不执行清空。

    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/resource"))
}
