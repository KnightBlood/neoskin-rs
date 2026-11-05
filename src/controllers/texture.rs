//! 纹理服务端点（PHP `TextureController`）。
//!
//! 已实现：CustomSkinAPI 的 `/{player}.json`、`/csl/{player}.json`，
//! 原始文件 `/textures/{hash}`、`/csl/textures/{hash}`、`/raw/{tid}`，
//! 以及 2D 头像 `avatar/*` 与预览 `preview/*`。
//!
//! 皮肤渲染见 `support::skin`：2D 视图由皮肤贴图拼接；`?3d` 暂时回退为
//! 2D 头像，输出固定 PNG（PHP 默认 WebP）。

use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use suprnova::{FrameworkError, HttpResponse, Model, Request, Response, handler};

use crate::models::player::Player;
use crate::models::texture::{Column as TextureColumn, Entity as Texture, Model as TextureModel};
use crate::models::user::User;
use crate::support::options;
use crate::support::storage;

/// 去掉 URL 里的 `.json` 后缀；不是 `.json` 时返回 404。
fn player_name(raw: &str) -> Option<&str> {
    raw.strip_suffix(".json")
}

fn not_found() -> Response {
    Ok(HttpResponse::text("Not Found").status(404))
}

fn http_date(value: DateTime<Utc>) -> String {
    value.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// GET `/{player}.json` 与 `GET /csl/{player}.json`。
#[handler]
pub async fn player_json(request: Request) -> Response {
    let raw = request.param("player").unwrap_or_default().to_string();
    let Some(name) = player_name(&raw) else {
        return not_found();
    };
    player_json_by_name(name).await
}

/// 按角色名生成 CustomSkinAPI 响应（供根级静态/JSON 路由复用）。
pub async fn player_json_by_name(name: &str) -> Response {
    if name.is_empty() {
        return not_found();
    }
    let name = name.to_string();

    let Some(player) = Player::filter("name", name).first().await? else {
        return not_found();
    };
    if let Some(user) = User::find(player.uid as u32).await?
        && user.is_banned()
    {
        return Ok(HttpResponse::text("该角色拥有者已被本站封禁").status(403));
    }

    let skin = if player.tid_skin > 0 {
        Texture::find()
            .filter(TextureColumn::Tid.eq(player.tid_skin))
            .one(suprnova::DB::connection()?.inner())
            .await
            .map_err(|error| suprnova::FrameworkError::database(error.to_string()))?
    } else {
        None
    };
    let cape = if player.tid_cape > 0 {
        Texture::find()
            .filter(TextureColumn::Tid.eq(player.tid_cape))
            .one(suprnova::DB::connection()?.inner())
            .await
            .map_err(|error| suprnova::FrameworkError::database(error.to_string()))?
    } else {
        None
    };

    let model = skin
        .as_ref()
        .map(|texture| {
            if texture.texture_type == "alex" {
                "slim"
            } else {
                "default"
            }
        })
        .unwrap_or("default");
    let skin_hash = skin.as_ref().map(|texture| texture.hash.clone());
    let cape_hash = cape.as_ref().map(|texture| texture.hash.clone());

    let body = json!({
        "username": player.name,
        "skins": { model: skin_hash },
        "cape": cape_hash,
    });

    Ok(HttpResponse::json(body).header("Last-Modified", http_date(player.last_modified)))
}

/// 读取并返回纹理原始字节（`Content-Type: image/png`）。
fn texture_file(hash: &str) -> Response {
    if !storage::exists(hash) {
        return not_found();
    }
    let data = storage::read(hash)?;
    let size = data.len();
    let mut response =
        HttpResponse::bytes_body(data, "image/png").header("Content-Length", size.to_string());
    if let Some(modified) = storage::modified(hash) {
        response = response.header("Last-Modified", storage::last_modified(modified));
    }
    Ok(response)
}

/// GET `/textures/{hash}` 与 `GET /csl/textures/{hash}`。
#[handler]
pub async fn texture(request: Request) -> Response {
    let hash = request.param("hash").unwrap_or_default().to_string();
    texture_file(&hash)
}

/// GET `/raw/{tid}`。
#[handler]
pub async fn raw(request: Request) -> Response {
    if !options::bool_or("allow_downloading_texture", true).await? {
        return Ok(HttpResponse::text("Forbidden").status(403));
    }
    let tid = request
        .param("tid")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let Some(tid) = tid else {
        return not_found();
    };
    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(tid))
        .one(suprnova::DB::connection()?.inner())
        .await
        .map_err(|error| suprnova::FrameworkError::database(error.to_string()))?;
    let Some(texture) = texture else {
        return not_found();
    };
    texture_file(&texture.hash)
}

#[cfg(test)]
mod tests {
    use super::player_name;

    #[test]
    fn strips_only_json_suffix() {
        assert_eq!(player_name("Steve.json"), Some("Steve"));
        assert_eq!(player_name("Steve"), None);
        assert_eq!(player_name(".json"), Some(""));
        assert_eq!(player_name("Steve.JSON"), None);
        assert_eq!(player_name("Steve.json.json"), Some("Steve.json"));
    }
}

// ---------------------------------------------------------------------------
// 头像与预览
// ---------------------------------------------------------------------------

use crate::support::skin;

fn image_response(image: &image::RgbaImage) -> Response {
    let bytes = skin::encode_png(image)
        .map_err(|error| suprnova::FrameworkError::internal(format!("PNG 编码失败: {error}")))?;
    Ok(HttpResponse::bytes_body(bytes, "image/png"))
}

fn has_flag(request: &Request, flag: &str) -> bool {
    request
        .query()
        .map(|query| {
            query
                .split('&')
                .any(|pair| pair == flag || pair.starts_with(&format!("{flag}=")))
        })
        .unwrap_or(false)
}

fn query_size(request: &Request, key: &str, fallback: u32) -> u32 {
    request
        .query_param(key)
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
        .clamp(1, 2048)
}

async fn texture_by_tid(tid: u32) -> Result<Option<TextureModel>, FrameworkError> {
    let connection = suprnova::DB::connection()?;
    Texture::find()
        .filter(TextureColumn::Tid.eq(tid))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))
}

async fn texture_by_hash(hash: &str) -> Result<Option<TextureModel>, FrameworkError> {
    let connection = suprnova::DB::connection()?;
    Texture::find()
        .filter(TextureColumn::Hash.eq(hash))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))
}

fn avatar_for(request: &Request, texture: Option<&TextureModel>) -> Response {
    let three_d = has_flag(request, "3d");
    let size = query_size(request, "size", 100);
    if let Some(texture) = texture
        && texture.texture_type != "steve"
        && texture.texture_type != "alex"
    {
        return Ok(HttpResponse::text("").status(422));
    }
    let image = texture
        .filter(|texture| storage::exists(&texture.hash))
        .and_then(|texture| storage::read(&texture.hash).ok())
        .and_then(|data| skin::decode(&data))
        .map(|skin_image| skin::resize_square(&skin::head(&skin_image), size))
        .or_else(|| skin::fallback(three_d).map(|fallback| skin::resize_square(&fallback, size)));
    match image {
        Some(image) => image_response(&image),
        None => Ok(HttpResponse::text("Not Found").status(404)),
    }
}

/// GET /avatar/{texture}
#[handler]
pub async fn avatar_by_texture(request: Request) -> Response {
    let tid = request
        .param("texture")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let texture = match tid {
        Some(tid) => texture_by_tid(tid).await?,
        None => None,
    };
    avatar_for(&request, texture.as_ref())
}

/// GET /avatar/hash/{hash}
#[handler]
pub async fn avatar_by_hash(request: Request) -> Response {
    let hash = request.param("hash").unwrap_or_default().to_string();
    let texture = texture_by_hash(&hash).await?;
    avatar_for(&request, texture.as_ref())
}

/// GET /avatar/player/{name}
#[handler]
pub async fn avatar_by_player(request: Request) -> Response {
    let name = request.param("name").unwrap_or_default().to_string();
    let Some(player) = Player::filter("name", name).first().await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let texture = if player.tid_skin > 0 {
        texture_by_tid(player.tid_skin as u32).await?
    } else {
        None
    };
    avatar_for(&request, texture.as_ref())
}

/// GET /avatar/user/{uid}
#[handler]
pub async fn avatar_by_user(request: Request) -> Response {
    let uid = request
        .param("uid")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let texture = match uid {
        Some(uid) => match User::find(uid).await? {
            Some(user) if user.avatar > 0 => texture_by_tid(user.avatar as u32).await?,
            _ => None,
        },
        None => None,
    };
    avatar_for(&request, texture.as_ref())
}

fn preview_for(request: &Request, texture: Option<&TextureModel>) -> Response {
    let Some(texture) = texture else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if !storage::exists(&texture.hash) {
        return Ok(HttpResponse::text("Not Found").status(404));
    }
    let data = storage::read(&texture.hash)?;
    let Some(source) = skin::decode(&data) else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let image = if texture.texture_type == "cape" {
        let height = query_size(request, "height", 200);
        skin::cape_preview(&source, height)
    } else {
        skin::skin_preview(&source, 12, texture.texture_type == "alex")
    };
    image_response(&image)
}

/// GET /preview/{texture}
#[handler]
pub async fn preview(request: Request) -> Response {
    let tid = request
        .param("texture")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let texture = match tid {
        Some(tid) => texture_by_tid(tid).await?,
        None => None,
    };
    preview_for(&request, texture.as_ref())
}

/// GET /preview/hash/{hash}
#[handler]
pub async fn preview_by_hash(request: Request) -> Response {
    let hash = request.param("hash").unwrap_or_default().to_string();
    let texture = texture_by_hash(&hash).await?;
    preview_for(&request, texture.as_ref())
}
