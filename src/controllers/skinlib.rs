//! 皮肤库（PHP `SkinlibController`）。
//!
//! 已实现：列表、详情、上传、重命名、改类型、改隐私、删除。
//! 视图端点（`/skinlib`、`/skinlib/show/{texture}`、`/skinlib/upload`）待
//! Twig 契约接入；`GET /skinlib/list` 本身返回 JSON 分页，已实现。
//!
//! 上传会重新编码 PNG（与 Go 端口一致），PHP 用 Imagick 的压缩参数不同，
//! 因此新上传文件的 SHA-256 与 PHP 可能不同；存量 hash 读取不受影响。

use image::{ImageFormat, ImageReader};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::{Value, json};
use suprnova::{
    DB, FrameworkError, HttpResponse, Model, Request, Response, UploadedFile, attrs, handler,
};

use crate::models::texture::{
    ActiveModel as TextureActiveModel, Column as TextureColumn, Entity as Texture,
    Model as TextureModel,
};
use crate::models::user::User;
use crate::support::api::{failure, require_user, validation_error};
use crate::support::hash::sha256_hex;
use crate::support::options;
use crate::support::storage;

fn texture_json(texture: &TextureModel) -> Value {
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
    })
}

fn texture_name(kind: &str) -> &'static str {
    if kind == "cape" { "披风" } else { "皮肤" }
}

async fn find_texture(tid: u32) -> Result<Option<TextureModel>, FrameworkError> {
    let connection = DB::connection()?;
    Texture::find()
        .filter(TextureColumn::Tid.eq(tid))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))
}

async fn all_textures() -> Result<Vec<TextureModel>, FrameworkError> {
    let connection = DB::connection()?;
    Texture::find()
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))
}

fn parse_tid(request: &Request, name: &str) -> Option<u32> {
    request
        .param(name)
        .ok()
        .and_then(|value| value.parse().ok())
}

/// 私有材质的访问控制；返回错误响应。
async fn ensure_viewable(
    request: &Request,
    texture: &TextureModel,
) -> Result<Option<Response>, FrameworkError> {
    if texture.is_public() {
        return Ok(None);
    }
    let user = crate::support::auth::current_user(request).await?;
    if let Some(user) = &user
        && (user.uid as i32 == texture.uploader || user.is_admin())
    {
        return Ok(None);
    }
    let status = options::int_or("status_code_for_private", 403).await?;
    let message = if status == 404 {
        "请求的材质文件已经被删除"
    } else {
        "请求的材质已经设为私密，仅上传者和管理员可查看"
    };
    Ok(Some(Ok(HttpResponse::text(message).status(status as u16))))
}

/// 管理操作的权限检查（上传者或管理员）。
async fn ensure_manage(
    request: &Request,
    texture: &TextureModel,
) -> Result<Option<Response>, FrameworkError> {
    let user = match require_user(request).await {
        Ok(user) => user,
        Err(response) => return Ok(Some(Ok(response))),
    };
    if let Some(response) = crate::support::api::require_verified(&user, request).await {
        return Ok(Some(Ok(response)));
    }
    if user.uid as i32 == texture.uploader || user.is_admin() {
        return Ok(None);
    }
    Ok(Some(Ok(HttpResponse::json(
        json!({ "code": 1, "message": "你没有权限修改此材质" }),
    )
    .status(403))))
}

/// GET /skinlib/list —— 皮肤库分页（Laravel `paginate(20)`）。
#[handler]
pub async fn library(request: Request) -> Response {
    let kind = request
        .query_param("filter")
        .unwrap_or_else(|| "skin".to_string());
    let uploader = request
        .query_param("uploader")
        .and_then(|value| value.parse::<u32>().ok());
    let keyword = request.query_param("keyword").unwrap_or_default();
    let sort = request
        .query_param("sort")
        .unwrap_or_else(|| "time".to_string());
    let page: i64 = request
        .query_param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
        .max(1);
    let per_page = 20i64;

    let current = crate::support::auth::current_user(&request).await?;
    let mut rows: Vec<(TextureModel, Option<String>)> = Vec::new();
    for texture in all_textures().await? {
        let matches_type = if kind == "skin" {
            texture.texture_type == "steve" || texture.texture_type == "alex"
        } else {
            texture.texture_type == kind
        };
        if !matches_type {
            continue;
        }
        if !keyword.is_empty() && !texture.name.contains(&keyword) {
            continue;
        }
        if let Some(uploader) = uploader
            && texture.uploader as u32 != uploader
        {
            continue;
        }
        match &current {
            Some(user) if user.is_admin() => {}
            Some(user) => {
                if !texture.is_public() && texture.uploader != user.uid as i32 {
                    continue;
                }
            }
            None => {
                if !texture.is_public() {
                    continue;
                }
            }
        }
        let nickname = User::find(texture.uploader as u32)
            .await?
            .map(|user| user.nickname);
        rows.push((texture, nickname));
    }

    match sort.as_str() {
        "likes" => rows.sort_by(|a, b| b.0.likes.cmp(&a.0.likes)),
        "tid" => rows.sort_by(|a, b| b.0.tid.cmp(&a.0.tid)),
        "name" => rows.sort_by(|a, b| b.0.name.cmp(&a.0.name)),
        _ => rows.sort_by(|a, b| b.0.upload_at.cmp(&a.0.upload_at)),
    }

    let total = rows.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;
    let data: Vec<Value> = rows
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .map(|(texture, nickname)| {
            json!({
                "tid": texture.tid,
                "name": texture.name,
                "type": texture.texture_type,
                "uploader": texture.uploader,
                "public": texture.is_public(),
                "likes": texture.likes,
                "nickname": nickname,
            })
        })
        .collect();

    let app_url = std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost".to_string());
    let base = format!("{}{}", app_url.trim_end_matches('/'), request.path());
    let page_url = |page: i64| format!("{base}?page={page}");
    let from = if data.is_empty() {
        None
    } else {
        Some(offset as i64 + 1)
    };
    let to = if data.is_empty() {
        None
    } else {
        Some(offset as i64 + data.len() as i64)
    };

    Ok(HttpResponse::json(json!({
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
    })))
}

/// GET /texture/{texture} 与 GET /skinlib/info/{texture}
#[handler]
pub async fn info(request: Request) -> Response {
    let Some(tid) = parse_tid(&request, "texture") else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(texture) = find_texture(tid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if let Some(response) = ensure_viewable(&request, &texture).await? {
        return response;
    }
    Ok(HttpResponse::json(texture_json(&texture)))
}

#[derive(Deserialize, Default)]
pub struct UploadRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub file: Option<UploadedFile>,
    #[serde(rename = "type", default)]
    pub texture_type: String,
    #[serde(default)]
    pub public: Option<Value>,
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_i64().map(|value| value != 0).unwrap_or(false),
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        _ => false,
    }
}

/// POST /texture —— 上传材质（multipart）。
#[handler]
pub async fn handle_upload(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = crate::support::api::require_verified(&user, &request).await {
        return Ok(response);
    }
    let body: UploadRequest = request.input().await?;

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(validation_error("name", "名称 不能为空。").into());
    }
    if !["steve", "alex", "cape"].contains(&body.texture_type.as_str()) {
        return Err(validation_error("type", "type 不在允许的范围内。").into());
    }
    let Some(file) = body.file else {
        return Err(validation_error("file", "file 不能为空。").into());
    };
    let is_public = body.public.as_ref().map(truthy).unwrap_or(false);

    let data = file.bytes().await?;
    let max_size = options::int_or("max_upload_file_size", 1024).await?;
    if max_size > 0 && data.len() as i64 > max_size * 1024 {
        return Ok(failure(
            format!("File is too large (max {max_size} KB)."),
            1,
        ));
    }

    let decoded = ImageReader::new(std::io::Cursor::new(&data))
        .with_guessed_format()
        .map_err(|_| validation_error("file", "file 必须是 png 文件。"))?
        .decode()
        .map_err(|_| validation_error("file", "file 必须是 png 文件。"))?;
    let width = decoded.width() as i64;
    let height = decoded.height() as i64;

    let max_width = options::int_or("max_texture_width", 8192).await?;
    if max_width > 0 && width > max_width {
        return Ok(failure(
            format!("材质过宽（{width}px），本站允许的最大宽度为 {max_width}px"),
            1,
        ));
    }
    if width % 64 != 0 || height % 32 != 0 {
        return Ok(failure(
            format!(
                "不是有效的 {} 文件（宽 {width}，高 {height}）",
                texture_name(&body.texture_type)
            ),
            1,
        ));
    }
    let ratio = width as f64 / height as f64;
    let valid_ratio = if body.texture_type == "cape" {
        ratio == 2.0
    } else {
        (ratio == 2.0 || ratio == 1.0) && !(body.texture_type == "alex" && ratio == 2.0)
    };
    if !valid_ratio {
        return Ok(failure(
            format!(
                "不是有效的 {} 文件（宽 {width}，高 {height}）",
                texture_name(&body.texture_type)
            ),
            1,
        ));
    }

    let mut sanitized = Vec::new();
    image::DynamicImage::ImageRgba8(decoded.to_rgba8())
        .write_to(&mut std::io::Cursor::new(&mut sanitized), ImageFormat::Png)
        .map_err(|error| FrameworkError::internal(format!("PNG 编码失败: {error}")))?;
    let hash = sha256_hex(&sanitized);

    let connection = DB::connection()?;
    let duplicated = Texture::find()
        .filter(TextureColumn::Hash.eq(hash.clone()))
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?
        .into_iter()
        .find(|texture| texture.is_public() || texture.uploader as u32 == user.uid);
    if let Some(duplicated) = duplicated {
        return Ok(HttpResponse::json(json!({
            "code": 2,
            "message": "已经有人上传过这个材质了，直接添加到衣柜使用吧~",
            "data": { "tid": duplicated.tid },
        })));
    }

    let file_size = sanitized.len().div_ceil(1024) as i32;
    let public_cost = options::int_value("score_per_storage").await?;
    let private_cost = options::int_value("private_score_per_storage").await?;
    let mut cost = file_size as i64 * if is_public { public_cost } else { private_cost };
    cost += options::int_value("score_per_closet_item").await?;
    cost -= options::int_value("score_award_per_texture").await?;
    if (user.score as i64) < cost {
        return Ok(failure("积分不足", 1));
    }

    let inserted = Texture::insert(TextureActiveModel {
        name: sea_orm::Set(name.clone()),
        texture_type: sea_orm::Set(body.texture_type.clone()),
        hash: sea_orm::Set(hash.clone()),
        size: sea_orm::Set(file_size),
        uploader: sea_orm::Set(user.uid as i32),
        public_flag: sea_orm::Set(if is_public { 1 } else { 0 }),
        upload_at: sea_orm::Set(chrono::Utc::now().naive_utc()),
        likes: sea_orm::Set(1),
        ..Default::default()
    })
    .exec(connection.inner())
    .await
    .map_err(|error| FrameworkError::database(error.to_string()))?;
    let tid = inserted.last_insert_id;

    if !storage::exists(&hash) {
        storage::write(&hash, &sanitized)?;
    }

    crate::models::closet_item::Entity::insert(crate::models::closet_item::ActiveModel {
        user_uid: sea_orm::Set(user.uid as i32),
        texture_tid: sea_orm::Set(tid as i32),
        item_name: sea_orm::Set(Some(name.clone())),
    })
    .exec(connection.inner())
    .await
    .map_err(|error| FrameworkError::database(error.to_string()))?;

    let next_score = user.score - cost as i32;
    user.update(attrs! { score: next_score }).await?;

    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("材质 {name} 上传成功"),
        "data": { "tid": tid },
    })))
}

/// DELETE /texture/{texture}
#[handler]
pub async fn delete(request: Request) -> Response {
    let Some(tid) = parse_tid(&request, "texture") else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(texture) = find_texture(tid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if let Some(response) = ensure_manage(&request, &texture).await? {
        return response;
    }

    let connection = DB::connection()?;
    crate::support::plugin_api::fire(
        "TextureDeleting",
        json!({
            "tid": tid,
            "uploader": texture.uploader,
            "hash": texture.hash.clone(),
        }),
    );
    let same_hash = Texture::find()
        .filter(TextureColumn::Hash.eq(texture.hash.clone()))
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    if same_hash.len() <= 1 {
        storage::delete(&texture.hash)?;
    }

    use sea_orm::EntityTrait;
    Texture::delete_by_id(tid)
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    Ok(HttpResponse::json(
        json!({ "code": 0, "message": "材质已被成功删除" }),
    ))
}

/// PUT /texture/{texture}/name
#[handler]
pub async fn rename(request: Request) -> Response {
    let Some(tid) = parse_tid(&request, "texture") else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(texture) = find_texture(tid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if let Some(response) = ensure_manage(&request, &texture).await? {
        return response;
    }
    let body: RenameRequest = request.input().await?;
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(validation_error("name", "名称 不能为空。").into());
    }
    let connection = DB::connection()?;
    Texture::update_many()
        .col_expr(
            TextureColumn::Name,
            sea_orm::sea_query::Expr::value(name.clone()),
        )
        .filter(TextureColumn::Tid.eq(tid))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("材质名称已被成功设置为 {name}"),
    })))
}

#[derive(Deserialize, Default)]
pub struct RenameRequest {
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize, Default)]
pub struct TypeRequest {
    #[serde(rename = "type", default)]
    pub texture_type: String,
}

/// PUT /texture/{texture}/type
#[handler]
pub async fn set_texture_type(request: Request) -> Response {
    let Some(tid) = parse_tid(&request, "texture") else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(texture) = find_texture(tid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if let Some(response) = ensure_manage(&request, &texture).await? {
        return response;
    }
    let body: TypeRequest = request.input().await?;
    if !["steve", "alex", "cape"].contains(&body.texture_type.as_str()) {
        return Err(validation_error("type", "type 不在允许的范围内。").into());
    }
    let connection = DB::connection()?;
    Texture::update_many()
        .col_expr(
            TextureColumn::TextureType,
            sea_orm::sea_query::Expr::value(body.texture_type.clone()),
        )
        .filter(TextureColumn::Tid.eq(tid))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("材质的适用模型已被修改为 {}", body.texture_type),
    })))
}

/// PUT /texture/{texture}/privacy
#[handler]
pub async fn privacy(request: Request) -> Response {
    let Some(tid) = parse_tid(&request, "texture") else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(texture) = find_texture(tid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if let Some(response) = ensure_manage(&request, &texture).await? {
        return response;
    }
    let Some(uploader) = User::find(texture.uploader as u32).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };

    let public_cost = options::int_value("score_per_storage").await?;
    let private_cost = options::int_value("private_score_per_storage").await?;
    let award = options::int_value("score_award_per_texture").await?;
    let mut diff = texture.size as i64
        * (private_cost - public_cost)
        * if texture.is_public() { -1 } else { 1 };
    if texture.is_public() && options::bool_or("take_back_scores_after_deletion", true).await? {
        diff -= award;
    }
    if (uploader.score as i64) + diff < 0 {
        return Ok(failure("积分不足", 1));
    }

    if !texture.is_public() {
        let connection = DB::connection()?;
        let duplicated = Texture::find()
            .filter(TextureColumn::Hash.eq(texture.hash.clone()))
            .filter(TextureColumn::PublicFlag.eq(1))
            .one(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?;
        if let Some(duplicated) = duplicated {
            return Ok(HttpResponse::json(json!({
                "code": 2,
                "message": "已经有人上传过这个材质了，直接添加到衣柜使用吧~",
                "data": { "tid": duplicated.tid },
            })));
        }
    }

    let next_score = uploader.score + diff as i32;
    uploader.update(attrs! { score: next_score }).await?;

    let connection = DB::connection()?;
    Texture::update_many()
        .col_expr(
            TextureColumn::PublicFlag,
            sea_orm::sea_query::Expr::value(if texture.is_public() { 0 } else { 1 }),
        )
        .filter(TextureColumn::Tid.eq(tid))
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;

    let label = if texture.is_public() {
        "私密"
    } else {
        "公开"
    };
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("材质已被设为 {label}"),
    })))
}

/// 页面级登录检查：未登录时 302 到登录页。
async fn require_page_user(request: &Request) -> Result<User, HttpResponse> {
    match crate::support::auth::current_user(request).await? {
        Some(user) => Ok(user),
        None => Err(HttpResponse::text("")
            .status(302)
            .header("Location", "/auth/login")),
    }
}

/// 皮肤库页面（顶栏布局）的公共上下文。
async fn page_context(request: &Request, user: Option<&User>) -> Result<Value, FrameworkError> {
    let locale_code = crate::support::locale::resolve(request);
    let token = suprnova::get_csrf_token().unwrap_or_default();
    let mut context =
        crate::support::view::base_context(&locale_code, request.path(), &token, user).await;
    let (current, langs) = super::page::page_languages(&locale_code, request.path());
    let mut avatar = Value::Null;
    let mut avatar_png = Value::Null;
    let mut menu = Value::Array(Vec::new());
    if let Some(user) = user {
        let extras = super::page::page_user_extras(user).await;
        avatar = extras["avatar"].clone();
        avatar_png = extras["avatar_png"].clone();
        menu = extras["menu"].clone();
    }
    if let Some(object) = context.as_object_mut() {
        object.insert("current".to_string(), Value::String(current));
        object.insert("langs".to_string(), langs);
        object.insert("avatar".to_string(), avatar);
        object.insert("avatar_png".to_string(), avatar_png);
        object.insert("menu".to_string(), menu);
        object.insert("notifications".to_string(), Value::Array(Vec::new()));
    }
    Ok(context)
}

/// GET /skinlib —— 皮肤库首页。
#[handler]
pub async fn home(request: Request) -> Response {
    let user = crate::support::auth::current_user(&request).await?;
    let mut context = page_context(&request, user.as_ref()).await?;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "extra".to_string(),
            json!({ "currentUid": user.as_ref().map(|user| user.uid).unwrap_or(0) }),
        );
    }
    Ok(crate::support::view::render("skinlib/index.html", context))
}

/// GET /skinlib/show/{texture} —— 材质详情页。
#[handler]
pub async fn show_page(request: Request) -> Response {
    let Some(tid) = parse_tid(&request, "texture") else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(texture) = find_texture(tid).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    if let Some(response) = ensure_viewable(&request, &texture).await? {
        return response;
    }
    if !storage::exists(&texture.hash) {
        if options::bool_or("auto_del_invalid_texture", false).await? {
            let connection = DB::connection()?;
            use sea_orm::EntityTrait;
            Texture::delete_by_id(tid)
                .exec(connection.inner())
                .await
                .map_err(|error| FrameworkError::database(error.to_string()))?;
        }
        return Ok(HttpResponse::text("请求的材质文件已经被删除").status(404));
    }

    let user = crate::support::auth::current_user(&request).await?;
    let uploader = User::find(texture.uploader as u32).await?;
    let mut badges = Vec::new();
    if let Some(uploader) = &uploader
        && uploader.is_admin()
    {
        badges.push(json!({ "text": "STAFF", "color": "primary" }));
    }
    let badges = crate::support::plugin_api::badges(badges);
    let in_closet = if let Some(user) = &user {
        crate::models::closet_item::Entity::find()
            .filter(crate::models::closet_item::Column::UserUid.eq(user.uid))
            .filter(crate::models::closet_item::Column::TextureTid.eq(texture.tid))
            .one(DB::connection()?.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?
            .is_some()
    } else {
        false
    };

    let mut context = page_context(&request, user.as_ref()).await?;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "texture".to_string(),
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
            }),
        );
        object.insert(
            "grid".to_string(),
            json!({
                "layout": [["md-8", "md-4"]],
                "widgets": [[
                    ["shared/previewer.html"],
                    ["skinlib/widgets/show/side.html"],
                ]],
            }),
        );
        object.insert(
            "extra".to_string(),
            json!({
                "download": options::bool_or("allow_downloading_texture", true).await?,
                "currentUid": user.as_ref().map(|user| user.uid).unwrap_or(0),
                "admin": user.as_ref().map(|user| user.is_admin()).unwrap_or(false),
                "inCloset": in_closet,
                "uploaderExists": uploader.is_some(),
                "nickname": uploader
                    .as_ref()
                    .map(|user| user.nickname.clone())
                    .unwrap_or_else(|| "不存在的用户".to_string()),
                "report": options::int_or("reporter_score_modification", 0).await?,
                "badges": badges,
            }),
        );
    }
    Ok(crate::support::view::render("skinlib/show.html", context))
}

/// GET /skinlib/upload —— 上传页（需登录）。
#[handler]
pub async fn upload_page(request: Request) -> Response {
    let user = match require_page_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let mut context = page_context(&request, Some(&user)).await?;
    let regexp = options::value("texture_name_regexp")
        .await
        .unwrap_or_default();
    let locale_code = crate::support::locale::resolve(&request);
    let rule = if regexp.is_empty() {
        crate::support::i18n::translate(&locale_code, "skinlib.upload.name-rule", None)
    } else {
        let mut params = serde_json::Map::new();
        params.insert("regexp".to_string(), Value::String(regexp));
        crate::support::i18n::translate(
            &locale_code,
            "skinlib.upload.name-rule-regexp",
            Some(&params),
        )
    };
    let private_cost = options::int_or("private_score_per_storage", 10).await?;
    let mut params = serde_json::Map::new();
    params.insert("score".to_string(), Value::String(private_cost.to_string()));
    let privacy_notice = crate::support::i18n::translate(
        &locale_code,
        "skinlib.upload.private-score-notice",
        Some(&params),
    );
    let content_policy = crate::support::communication::markdown(
        &options::localized(&locale_code, "content_policy").await?,
    );
    let content_policy = match crate::support::plugin_api::apply_filter(
        "content_policy",
        Value::String(content_policy.clone()),
    ) {
        Value::String(filtered) => filtered,
        _ => content_policy,
    };
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "grid".to_string(),
            json!({
                "layout": [["md-6", "md-6"]],
                "widgets": [[
                    ["skinlib/widgets/upload/input.html"],
                    ["shared/previewer.html"],
                ]],
            }),
        );
        object.insert(
            "extra".to_string(),
            json!({
                "rule": rule,
                "privacyNotice": privacy_notice,
                "score": user.score,
                "scorePublic": options::int_or("score_per_storage", 1).await?,
                "scorePrivate": private_cost,
                "closetItemCost": options::int_or("score_per_closet_item", 0).await?,
                "award": options::int_or("score_award_per_texture", 0).await?,
                "contentPolicy": content_policy,
            }),
        );
    }
    Ok(crate::support::view::render("skinlib/upload.html", context))
}

#[cfg(test)]
mod tests {
    use super::{texture_name, truthy};
    use serde_json::json;

    #[test]
    fn truthy_matches_php_boolean_inputs() {
        assert!(truthy(&json!("1")));
        assert!(truthy(&json!("true")));
        assert!(truthy(&json!(true)));
        assert!(!truthy(&json!("0")));
        assert!(!truthy(&json!(false)));
        assert!(!truthy(&json!(null)));
    }

    #[test]
    fn texture_name_maps_types() {
        assert_eq!(texture_name("cape"), "披风");
        assert_eq!(texture_name("steve"), "皮肤");
        assert_eq!(texture_name("alex"), "皮肤");
    }
}
