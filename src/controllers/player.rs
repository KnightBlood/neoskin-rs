//! 玩家角色：`/api/players` 与 `/user/player` 的 JSON 端点。
//!
//! 视图端点（GET `/user/player`）等 Phase 6 接 Twig 后再补，这里先落地
//! 前端实际调用的 API 契约。

use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::{Value, json};
use suprnova::{DB, FrameworkError, HttpResponse, Model, Request, Response, attrs, handler};

use crate::models::closet_item::{Column as ClosetColumn, Entity as ClosetItem};
use crate::models::player::Player;
use crate::models::texture::{Column as TextureColumn, Entity as Texture};
use crate::models::user::User;
use crate::support::api::{failure, require_user, success, validation_error};
use crate::support::options;

fn player_json(player: &Player) -> Value {
    json!({
        "pid": player.pid,
        "uid": player.uid,
        "name": player.name,
        "tid_cape": player.tid_cape,
        "last_modified": player.last_modified.format("%Y-%m-%d %H:%M:%S").to_string(),
        "tid_skin": player.tid_skin,
    })
}

fn forbidden() -> Response {
    Ok(HttpResponse::json(json!({ "code": 1, "message": "你无权操作此角色" })).status(403))
}

/// `/user/player/*` 需要邮箱验证；`/api/players` 不检查（PHP 的 API 路由不挂 `verified`）。
async fn verified_guard(request: &Request, user: &User) -> Option<HttpResponse> {
    if request.path().starts_with("/api") {
        return None;
    }
    crate::support::api::require_verified(user, request).await
}

/// 对应 PHP `Rules\PlayerName` + `min/max`；返回首条错误消息。
pub(crate) async fn validate_name(name: &str) -> Result<Option<String>, FrameworkError> {
    if name.is_empty() {
        return Ok(Some("角色名 不能为空。".to_string()));
    }
    let rule = options::value("player_name_rule").await?;
    let rule = if rule.is_empty() {
        "official".to_string()
    } else {
        rule
    };
    let allowed = match rule.as_str() {
        "official" => name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_'),
        "cjk" => name.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character == '_'
                || character == '§'
                || ('\u{4e00}'..='\u{9fff}').contains(&character)
        }),
        "utf8" => !name.chars().any(char::is_whitespace),
        "custom" => {
            let pattern = options::value("custom_player_name_regexp").await?;
            custom_pattern_matches(&pattern, name)
        }
        _ => true,
    };
    if !allowed {
        let message = match rule.as_str() {
            "cjk" => "角色名可使用拉丁字母、数字、下划线以及汉字（中日韩统一表意文字）。",
            "utf8" => "角色名必须是 UTF-8 字符串。",
            "custom" => "本站使用了自定义的角色名规则，详情请咨询站点管理员。",
            _ => "角色名只能包含拉丁字母、数字以及下划线。",
        };
        return Ok(Some(message.to_string()));
    }

    let length = name.chars().count() as i64;
    let min = options::int_or("player_name_length_min", 3).await?;
    let max = options::int_or("player_name_length_max", 16).await?;
    if length < min {
        return Ok(Some(format!("角色名 至少为 {min} 个字符。")));
    }
    if length > max {
        return Ok(Some(format!("角色名 不能大于 {max} 个字符。")));
    }
    Ok(None)
}

/// 把 PHP 的 `/pattern/flags` 转为 Rust `regex`。
fn custom_pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return true;
    }
    let (body, flags) = if pattern.starts_with('/')
        && let Some(end) = pattern.rfind('/')
        && end > 0
    {
        (&pattern[1..end], &pattern[end + 1..])
    } else {
        (pattern, "")
    };
    let mut builder = regex::RegexBuilder::new(body);
    builder
        .case_insensitive(flags.contains('i'))
        .dot_matches_new_line(flags.contains('s'))
        .multi_line(flags.contains('m'));
    builder
        .build()
        .map(|regex| regex.is_match(name))
        .unwrap_or(false)
}

async fn name_taken(name: &str, ignore_pid: Option<u32>) -> Result<bool, FrameworkError> {
    let existing = Player::filter("name", name).first().await?;
    Ok(existing
        .map(|player| Some(player.pid) != ignore_pid)
        .unwrap_or(false))
}

async fn owned_player(request: &Request, user: &User) -> Result<Option<Player>, FrameworkError> {
    let pid = request
        .param("player")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let Some(pid) = pid else {
        return Ok(None);
    };
    let player = Player::find(pid).await?;
    match player {
        Some(player) if player.uid == user.uid as i32 => Ok(Some(player)),
        _ => Ok(None),
    }
}

#[derive(Deserialize, Default)]
pub struct AddPlayerRequest {
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize, Default)]
pub struct SetTextureRequest {
    #[serde(default)]
    pub skin: Option<i64>,
    #[serde(default)]
    pub cape: Option<i64>,
}

#[derive(Deserialize, Default)]
pub struct ClearTextureRequest {
    #[serde(default, rename = "type")]
    pub types: Option<Value>,
    #[serde(default)]
    pub skin: Option<Value>,
    #[serde(default)]
    pub cape: Option<Value>,
}

/// GET /api/players
#[handler]
pub async fn list(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Player.Read", "Player.ReadWrite"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = verified_guard(&request, &user).await {
        return Ok(response);
    }
    let players = Player::filter("uid", user.uid).get().await?;
    let data: Vec<Value> = players.iter().map(player_json).collect();
    Ok(HttpResponse::json(json!(data)))
}

/// POST /api/players
#[handler]
pub async fn add(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Player.ReadWrite"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = verified_guard(&request, &user).await {
        return Ok(response);
    }
    let body: AddPlayerRequest = request.input().await?;
    let name = body.name.trim().to_string();
    if let Some(message) = validate_name(&name).await? {
        return Err(validation_error("name", &message).into());
    }
    if name_taken(&name, None).await? {
        return Err(validation_error("name", "该角色名已被占用").into());
    }

    let cost = options::int_or("score_per_player", 100).await?;
    if (user.score as i64) < cost {
        return Ok(failure("添加角色失败，积分不足", 7));
    }

    crate::support::plugin_api::fire(
        "PlayerWillBeAdded",
        json!({ "uid": user.uid, "name": name.clone() }),
    );
    let player = Player::create(attrs! {
        uid: user.uid,
        name: name.clone(),
        tid_skin: 0,
        tid_cape: 0,
        last_modified: Utc::now(),
    })
    .await?;
    crate::support::plugin_api::fire(
        "PlayerWasAdded",
        json!({ "pid": player.pid, "uid": player.uid, "name": player.name.clone() }),
    );
    let next_score = user.score - cost as i32;
    user.update(attrs! {
        score: next_score,
    })
    .await?;

    Ok(success(
        format!("成功添加了角色 {name}"),
        Some(player_json(&player)),
    ))
}

/// DELETE /api/players/{player}
#[handler]
pub async fn delete(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Player.ReadWrite"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = verified_guard(&request, &user).await {
        return Ok(response);
    }
    let Some(player) = owned_player(&request, &user).await? else {
        return forbidden();
    };
    let name = player.name.clone();
    let pid = player.pid;
    crate::support::plugin_api::fire(
        "PlayerWillBeDeleted",
        json!({ "pid": player.pid, "uid": player.uid, "name": player.name.clone() }),
    );
    player.delete().await?;
    crate::support::plugin_api::fire(
        "PlayerWasDeleted",
        json!({ "pid": pid, "uid": user.uid, "name": name.clone() }),
    );

    if options::bool_or("return_score", true).await? {
        let cost = options::int_or("score_per_player", 100).await?;
        let next_score = user.score + cost as i32;
        user.update(attrs! {
            score: next_score,
        })
        .await?;
    }

    Ok(success(format!("角色 {name} 已被删除"), None))
}

/// PUT /api/players/{player}/name
#[handler]
pub async fn rename(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Player.ReadWrite"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = verified_guard(&request, &user).await {
        return Ok(response);
    }
    let Some(player) = owned_player(&request, &user).await? else {
        return forbidden();
    };
    let body: AddPlayerRequest = request.input().await?;
    let name = body.name.trim().to_string();
    if let Some(message) = validate_name(&name).await? {
        return Err(validation_error("name", &message).into());
    }
    if name_taken(&name, Some(player.pid)).await? {
        return Err(validation_error("name", "该角色名已被占用").into());
    }

    let old = player.name.clone();
    let pid = player.pid;
    player
        .update(attrs! {
            name: name.clone(),
            last_modified: Utc::now(),
        })
        .await?;
    let updated = Player::find(pid).await?;
    crate::support::plugin_api::fire(
        "PlayerProfileUpdated",
        json!({ "pid": pid, "uid": user.uid, "field": "name", "name": name.clone() }),
    );

    Ok(success(
        format!("角色 {old} 已更名为 {name}"),
        updated.as_ref().map(player_json),
    ))
}

/// PUT /api/players/{player}/textures
#[handler]
pub async fn set_textures(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Player.ReadWrite"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = verified_guard(&request, &user).await {
        return Ok(response);
    }
    let Some(player) = owned_player(&request, &user).await? else {
        return forbidden();
    };
    let body: SetTextureRequest = request.input().await?;
    let connection = DB::connection()?;

    let mut tid_skin = player.tid_skin;
    let mut tid_cape = player.tid_cape;
    for (kind, tid) in [("skin", body.skin), ("cape", body.cape)] {
        let Some(tid) = tid.filter(|value| *value > 0) else {
            continue;
        };
        let texture_exists = Texture::find()
            .filter(TextureColumn::Tid.eq(tid as i32))
            .one(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?
            .is_some();
        if !texture_exists {
            return Ok(failure("材质不存在", 1));
        }
        let in_closet = ClosetItem::find()
            .filter(ClosetColumn::UserUid.eq(user.uid))
            .filter(ClosetColumn::TextureTid.eq(tid as i32))
            .one(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?
            .is_some();
        if !in_closet {
            return Ok(failure("衣柜中不存在此材质", 1));
        }
        if kind == "skin" {
            tid_skin = tid as i32;
        } else {
            tid_cape = tid as i32;
        }
    }

    let pid = player.pid;
    let name = player.name.clone();
    player
        .update(attrs! {
            tid_skin: tid_skin,
            tid_cape: tid_cape,
            last_modified: Utc::now(),
        })
        .await?;
    let updated = Player::find(pid).await?;
    crate::support::plugin_api::fire(
        "PlayerProfileUpdated",
        json!({ "pid": pid, "uid": user.uid, "field": "textures" }),
    );

    Ok(success(
        format!("材质已成功应用至角色 {name}"),
        updated.as_ref().map(player_json),
    ))
}

/// DELETE /api/players/{player}/textures
#[handler]
pub async fn clear_textures(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["Player.ReadWrite"]).await
    {
        return Ok(response);
    }
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = verified_guard(&request, &user).await {
        return Ok(response);
    }
    let Some(player) = owned_player(&request, &user).await? else {
        return forbidden();
    };
    let body: ClearTextureRequest = request.input().await?;

    let listed: Vec<String> = body
        .types
        .as_ref()
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut tid_skin = player.tid_skin;
    let mut tid_cape = player.tid_cape;
    if body.skin.is_some() || listed.iter().any(|value| value == "skin") {
        tid_skin = 0;
    }
    if body.cape.is_some() || listed.iter().any(|value| value == "cape") {
        tid_cape = 0;
    }

    let pid = player.pid;
    let name = player.name.clone();
    player
        .update(attrs! {
            tid_skin: tid_skin,
            tid_cape: tid_cape,
            last_modified: Utc::now(),
        })
        .await?;
    let updated = Player::find(pid).await?;
    crate::support::plugin_api::fire(
        "PlayerProfileUpdated",
        json!({ "pid": pid, "uid": user.uid, "field": "textures" }),
    );

    Ok(success(
        format!("角色 {name} 的材质已被成功重置"),
        updated.as_ref().map(player_json),
    ))
}

#[cfg(test)]
mod tests {
    use super::custom_pattern_matches;

    #[test]
    fn custom_patterns_accept_php_delimiters_and_flags() {
        assert!(custom_pattern_matches("/^[a-z]+$/i", "Steve"));
        assert!(!custom_pattern_matches("/^[a-z]+$/", "Steve"));
        assert!(custom_pattern_matches("", "任何名字"));
        assert!(custom_pattern_matches("/^[A-Za-z0-9_]+$/", "Alex_1"));
    }
}
