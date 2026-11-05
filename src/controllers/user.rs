//! 用户资料与积分（PHP `UserController` 的 JSON 端点）。

use chrono::{Datelike, Duration, Local, TimeZone, Utc};
use rand::RngExt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::json;
use suprnova::session::invalidate_session;
use suprnova::{DB, FrameworkError, HttpResponse, Model, Request, Response, attrs, handler};

use crate::models::texture::{Column as TextureColumn, Entity as Texture};
use crate::models::user::User;
use crate::support::api::{failure, require_user, validation_error};
use crate::support::hash::hash_password;
use crate::support::hash::verify_password;
use crate::support::options;

/// PHP 的 `(int) $string`：取开头数字，`'true'` 之类的字符串得到 0。
fn php_int(value: &str) -> i64 {
    let trimmed = value.trim();
    let mut end = 0;
    let mut chars = trimmed.char_indices().peekable();
    if let Some((_, sign)) = chars.peek()
        && (*sign == '-' || *sign == '+')
    {
        end = 1;
        chars.next();
    }
    for (index, character) in chars {
        if character.is_ascii_digit() {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    trimmed[..end].parse().unwrap_or(0)
}

fn sign_range(raw: &str) -> (i64, i64) {
    let mut parts = raw.split(',');
    let min = parts.next().map(php_int).unwrap_or(0);
    let max = parts.next().map(php_int).unwrap_or(min);
    (min, max.max(min))
}

fn http_date(value: chrono::DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// GET /user/score-info
#[handler]
pub async fn score_info(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let connection = DB::connection()?;
    let players = crate::models::player::Player::filter("uid", user.uid as i32)
        .get()
        .await?
        .len();
    let textures = Texture::find()
        .filter(TextureColumn::Uploader.eq(user.uid as i32))
        .all(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let storage: i64 = textures.iter().map(|texture| texture.size as i64).sum();

    Ok(HttpResponse::json(json!({
        "user": {
            "score": user.score,
            "lastSignAt": http_date(user.last_sign_at),
        },
        "rate": {
            "storage": options::int_value("score_per_storage").await?,
            "players": options::int_value("score_per_player").await?,
        },
        "usage": {
            "players": players,
            "storage": storage,
        },
        "signAfterZero": options::bool_or("sign_after_zero", false).await?,
        "signGapTime": options::int_or("sign_gap_time", 24).await?,
    })))
}

/// 签到剩余秒数（对应 PHP `diffInSeconds($target, false)`）。
fn seconds_until(target: chrono::DateTime<Utc>) -> i64 {
    (target - Utc::now()).num_seconds()
}

/// POST /user/sign
#[handler]
pub async fn sign(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let sign_after_zero = options::bool_or("sign_after_zero", false).await?;
    let gap_hours = options::int_or("sign_gap_time", 24).await?;

    let target = if sign_after_zero {
        let now = Local::now();
        let start_of_today = Local
            .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
            .single()
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        if user.last_sign_at <= start_of_today {
            user.last_sign_at
        } else {
            start_of_today + Duration::days(1)
        }
    } else {
        user.last_sign_at + Duration::hours(gap_hours)
    };

    if seconds_until(target) > 0 {
        return Ok(failure("", 1));
    }

    let (min, max) = sign_range(&options::value("sign_score").await?);
    let acquired = if max > min {
        let mut rng = rand::rng();
        rng.random_range(min..=max)
    } else {
        min
    };
    let next_score = user.score + acquired as i32;
    user.update(attrs! {
        score: next_score,
        last_sign_at: Utc::now(),
    })
    .await?;

    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": format!("签到成功，获得了 {acquired} 积分"),
        "data": { "score": next_score },
    })))
}

#[derive(Deserialize, Default)]
pub struct ProfileRequest {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub new_nickname: Option<String>,
    #[serde(default)]
    pub current_password: Option<String>,
    #[serde(default)]
    pub new_password: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

fn check_password_len(
    field: &str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), FrameworkError> {
    let length = value.chars().count();
    if length == 0 {
        return Err(validation_error(
            field,
            &format!("{} 不能为空。", label(field)),
        ));
    }
    if !(min..=max).contains(&length) {
        return Err(validation_error(
            field,
            &format!("{} 必须介于 {min} - {max} 个字符之间。", label(field)),
        ));
    }
    Ok(())
}

fn label(field: &str) -> &'static str {
    match field {
        "current_password" => "当前密码",
        "new_password" => "新密码",
        "password" => "密码",
        "email" => "邮箱",
        "new_nickname" => "昵称",
        _ => "字段",
    }
}

/// POST /user/profile
#[handler]
pub async fn handle_profile(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let body: ProfileRequest = request.input().await?;

    match body.action.as_str() {
        "nickname" => {
            let nickname = body.new_nickname.unwrap_or_default().trim().to_string();
            if nickname.is_empty() {
                return Err(validation_error("new_nickname", "昵称 不能为空。").into());
            }
            user.update(attrs! { nickname: nickname.clone() }).await?;
            Ok(HttpResponse::json(json!({
                "code": 0,
                "message": format!("昵称已成功设置为 {nickname}"),
            })))
        }
        "password" => {
            let current = body.current_password.unwrap_or_default();
            let next = body.new_password.unwrap_or_default();
            check_password_len("current_password", &current, 6, 32)?;
            check_password_len("new_password", &next, 8, 32)?;
            if !verify_password(&user.password, &current) {
                return Ok(failure("原密码错误", 1));
            }
            let hash = hash_password(&next, None)
                .map_err(|error| FrameworkError::internal(format!("密码哈希失败: {error}")))?;
            user.update(attrs! { password: hash }).await?;
            invalidate_session();
            Ok(failure("密码修改成功，请重新登录", 0))
        }
        "email" => {
            let email = body.email.unwrap_or_default().trim().to_string();
            let password = body.password.unwrap_or_default();
            if email.is_empty() {
                return Err(validation_error("email", "邮箱 不能为空。").into());
            }
            check_password_len("password", &password, 6, 32)?;
            if !email.contains('@') {
                return Err(validation_error("email", "邮箱 格式不正确。").into());
            }
            if User::filter("email", email.clone())
                .first()
                .await?
                .is_some()
            {
                return Ok(failure("此邮箱已被占用", 1));
            }
            if !verify_password(&user.password, &password) {
                return Ok(failure("密码错误", 1));
            }
            user.update(attrs! {
                email: email,
                verified: false,
            })
            .await?;
            invalidate_session();
            Ok(failure("邮箱修改成功，请重新登录", 0))
        }
        "delete" => {
            let password = body.password.unwrap_or_default();
            check_password_len("password", &password, 6, 32)?;
            if user.is_admin() {
                return Ok(failure("拥有管理员权限的账号不能被删除", 1));
            }
            if !verify_password(&user.password, &password) {
                return Ok(failure("密码错误", 1));
            }
            invalidate_session();
            user.delete().await?;
            Ok(failure("账号已被成功删除", 0))
        }
        _ => Ok(failure("非法参数", 1)),
    }
}

#[derive(Deserialize, Default)]
pub struct AvatarRequest {
    #[serde(default)]
    pub tid: Option<u64>,
}

/// POST /user/profile/avatar
#[handler]
pub async fn set_avatar(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let body: AvatarRequest = request.input().await?;
    let Some(tid) = body.tid else {
        return Err(validation_error("tid", "tid 不能为空。").into());
    };

    if tid == 0 {
        user.update(attrs! { avatar: 0 }).await?;
        return Ok(failure("设置成功", 0));
    }
    let Ok(tid) = u32::try_from(tid) else {
        return Err(validation_error("tid", "tid 不能为空。").into());
    };

    let connection = DB::connection()?;
    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(tid))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let Some(texture) = texture else {
        return Ok(failure("材质不存在", 1));
    };
    if texture.texture_type == "cape" {
        return Ok(failure("披风不能被设置为头像", 1));
    }
    if !texture.is_public() && texture.uploader != user.uid as i32 && !user.is_admin() {
        return Ok(failure("请求的材质已经设为私密，仅上传者和管理员可查看", 1));
    }
    user.update(attrs! { avatar: tid as i32 }).await?;
    Ok(failure("设置成功", 0))
}

/// PUT /user/dark-mode
#[handler]
pub async fn toggle_dark_mode(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let next = !user.is_dark_mode;
    user.update(attrs! { is_dark_mode: next }).await?;
    Ok(HttpResponse::text("").status(204))
}

#[cfg(test)]
mod tests {
    use super::{php_int, seconds_until, sign_range};

    #[test]
    fn php_int_matches_php_casts() {
        assert_eq!(php_int("true"), 0);
        assert_eq!(php_int("100"), 100);
        assert_eq!(php_int("-5abc"), -5);
        assert_eq!(php_int(""), 0);
    }

    #[test]
    fn sign_range_parses_option() {
        assert_eq!(sign_range("10,100"), (10, 100));
        assert_eq!(sign_range("50"), (50, 50));
    }

    #[test]
    fn seconds_until_past_is_negative() {
        assert!(seconds_until(chrono::Utc::now() - chrono::Duration::seconds(5)) < 0);
    }
}
