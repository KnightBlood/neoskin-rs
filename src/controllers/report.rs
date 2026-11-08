//! 举报（PHP `ReportController`）。

use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::{Value, json};
use suprnova::{DB, FrameworkError, HttpResponse, Model, Request, Response, attrs, handler};

use crate::models::report::{Report, STATUS_PENDING, STATUS_REJECTED, STATUS_RESOLVED};
use crate::models::texture::{Column as TextureColumn, Entity as Texture};
use crate::models::user::{PERMISSION_BANNED, User};
use crate::support::api::{failure, require_user, validation_error};
use crate::support::options;
use crate::support::storage;

#[derive(Deserialize, Default)]
pub struct SubmitRequest {
    #[serde(default)]
    pub tid: Option<i64>,
    #[serde(default)]
    pub reason: String,
}

/// POST /skinlib/report
#[handler]
pub async fn submit(request: Request) -> Response {
    let user = match require_user(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    if let Some(response) = crate::support::api::require_verified(&user, &request).await {
        return Ok(response);
    }
    let body: SubmitRequest = request.input().await?;
    let Some(tid) = body.tid.filter(|value| *value > 0) else {
        return Err(validation_error("tid", "tid 不能为空。").into());
    };
    let reason = body.reason.trim().to_string();
    if reason.is_empty() {
        return Err(validation_error("reason", "举报理由 不能为空。").into());
    }

    let connection = DB::connection()?;
    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(tid as i32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let Some(texture) = texture else {
        return Err(validation_error("tid", "tid 不存在。").into());
    };

    let duplicate = Report::filter("reporter", user.uid)
        .filter("tid", tid as i32)
        .first()
        .await?;
    if duplicate.is_some() {
        return Ok(failure(
            "您已经举报过该材质了，请耐心等待管理员处理。您可以在用户中心查看举报的处理进度。",
            1,
        ));
    }

    let modification = options::int_value("reporter_score_modification").await?;
    if modification < 0 && (user.score as i64) < -modification {
        return Ok(failure("积分不足", 1));
    }
    let next_score = user.score + modification as i32;
    let reporter = user.uid;
    user.update(attrs! { score: next_score }).await?;

    Report::create(attrs! {
        tid: tid as i32,
        uploader: texture.uploader,
        reporter: reporter,
        reason: reason,
        status: STATUS_PENDING,
        report_at: Utc::now(),
    })
    .await?;

    Ok(failure("举报已提交，请等待管理员处理", 0))
}

/// GET /api/user/reports —— 我的举报分页（新增契约端点）。
#[handler]
pub async fn track(request: Request) -> Response {
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

    let mut reports: Vec<Report> = Report::filter("reporter", user.uid).get().await?.into_vec();
    reports.sort_by(|left, right| right.report_at.cmp(&left.report_at));

    let total = reports.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;

    let connection = DB::connection()?;
    let mut items = Vec::new();
    for report in reports.iter().skip(offset).take(per_page as usize) {
        let texture = Texture::find()
            .filter(TextureColumn::Tid.eq(report.tid as u32))
            .one(connection.inner())
            .await
            .map_err(|error| FrameworkError::database(error.to_string()))?;
        items.push(json!({
            "id": report.id,
            "tid": report.tid,
            "texture_name": texture.map(|texture| texture.name).unwrap_or_default(),
            "reason": report.reason,
            "status": report.status,
            "report_at": report.report_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        }));
    }

    Ok(HttpResponse::json(json!({
        "data": items,
        "current_page": page,
        "last_page": last_page,
        "per_page": per_page,
        "total": total,
    })))
}

async fn admin(request: &Request) -> Result<User, Response> {
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

async fn report_json(report: &Report) -> Result<Value, FrameworkError> {
    let connection = DB::connection()?;
    let texture = Texture::find()
        .filter(TextureColumn::Tid.eq(report.tid as u32))
        .one(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    let uploader = User::find(report.uploader as u32).await?;
    let informer = User::find(report.reporter as u32).await?;

    Ok(json!({
        "id": report.id,
        "tid": report.tid,
        "uploader": report.uploader,
        "reporter": report.reporter,
        "reason": report.reason,
        "status": report.status,
        "report_at": report.report_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        "texture": texture.map(|texture| json!({
            "tid": texture.tid,
            "name": texture.name,
            "type": texture.texture_type,
            "hash": texture.hash,
            "size": texture.size,
            "uploader": texture.uploader,
            "public": texture.is_public(),
            "upload_at": texture.upload_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            "likes": texture.likes,
        })),
        "texture_uploader": uploader.map(|user| json!({
            "uid": user.uid,
            "nickname": user.nickname,
            "email": user.email,
            "permission": user.permission,
        })),
        "informer": informer.map(|user| json!({
            "uid": user.uid,
            "nickname": user.nickname,
            "email": user.email,
            "permission": user.permission,
        })),
    }))
}

/// GET /admin/reports/list
#[handler]
pub async fn manage(request: Request) -> Response {
    if let Err(response) = crate::support::api::require_scopes(
        &request,
        &["ReportsManagement.Read", "ReportsManagement.ReadWrite"],
    )
    .await
    {
        return Ok(response);
    }
    if let Err(response) = admin(&request).await {
        return response;
    }
    let query = request.query_param("q").unwrap_or_default();
    let page: i64 = request
        .query_param("page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
        .max(1);
    let per_page = 9i64;

    let mut reports: Vec<Report> = Report::all().await?.into_vec();
    reports.sort_by(|a, b| b.report_at.cmp(&a.report_at));
    if !query.is_empty() {
        let needle = query.to_ascii_lowercase();
        reports.retain(|report| {
            report.reason.to_ascii_lowercase().contains(&needle)
                || report.id.to_string() == query
                || report.tid.to_string() == query
                || report.uploader.to_string() == query
                || report.reporter.to_string() == query
                || report.status.to_string() == query
        });
    }

    let total = reports.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(last_page);
    let offset = ((page - 1) * per_page) as usize;
    let mut data = Vec::new();
    for report in reports.iter().skip(offset).take(per_page as usize) {
        data.push(report_json(report).await?);
    }

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

#[derive(Deserialize, Default)]
pub struct ReviewRequest {
    #[serde(default)]
    pub action: String,
}

async fn adjust_informer(report: &Report, delta: i64) -> Result<(), FrameworkError> {
    if delta == 0 {
        return Ok(());
    }
    if let Some(informer) = User::find(report.reporter as u32).await? {
        let next = informer.score + delta as i32;
        informer.update(attrs! { score: next }).await?;
    }
    Ok(())
}

/// PUT /admin/reports/{report}
#[handler]
pub async fn review(request: Request) -> Response {
    if let Err(response) =
        crate::support::api::require_scopes(&request, &["ReportsManagement.ReadWrite"]).await
    {
        return Ok(response);
    }
    let current = match admin(&request).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let Some(id) = request
        .param("report")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some(report) = Report::find(id).await? else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let body: ReviewRequest = request.input().await?;
    if !["delete", "ban", "reject"].contains(&body.action.as_str()) {
        return Err(validation_error("action", "action 不在允许的范围内。").into());
    }

    let modification = options::int_value("reporter_score_modification").await?;
    let reward = options::int_value("reporter_reward_score").await?;

    if body.action == "reject" {
        if report.status == STATUS_PENDING && modification > 0 {
            adjust_informer(&report, -modification).await?;
        }
        report.update(attrs! { status: STATUS_REJECTED }).await?;
        return Ok(HttpResponse::json(json!({
            "code": 0,
            "message": "操作成功",
            "data": { "status": STATUS_REJECTED },
        })));
    }

    match body.action.as_str() {
        "delete" => {
            let connection = DB::connection()?;
            let texture = Texture::find()
                .filter(TextureColumn::Tid.eq(report.tid))
                .one(connection.inner())
                .await
                .map_err(|error| FrameworkError::database(error.to_string()))?;
            match texture {
                Some(texture) => {
                    let same_hash = Texture::find()
                        .filter(TextureColumn::Hash.eq(texture.hash.clone()))
                        .all(connection.inner())
                        .await
                        .map_err(|error| FrameworkError::database(error.to_string()))?;
                    if same_hash.len() <= 1 {
                        let _ = storage::delete(&texture.hash);
                    }
                    Texture::delete_by_id(texture.tid)
                        .exec(connection.inner())
                        .await
                        .map_err(|error| FrameworkError::database(error.to_string()))?;
                }
                None => {
                    if report.status == STATUS_PENDING && modification < 0 {
                        adjust_informer(&report, -modification).await?;
                    }
                    report.update(attrs! { status: STATUS_RESOLVED }).await?;
                    return Ok(HttpResponse::json(json!({
                        "code": 0,
                        "message": "请求的材质已被删除",
                        "data": { "status": STATUS_RESOLVED },
                    })));
                }
            }
        }
        "ban" => {
            let Some(uploader) = User::find(report.uploader as u32).await? else {
                return Ok(failure("用户不存在", 1));
            };
            if current.permission <= uploader.permission {
                return Ok(failure("你无权操作此用户", 1));
            }
            uploader
                .update(attrs! { permission: PERMISSION_BANNED })
                .await?;
        }
        _ => {}
    }

    if report.status == STATUS_PENDING && modification < 0 {
        adjust_informer(&report, -modification).await?;
    }
    if report.status == STATUS_PENDING {
        adjust_informer(&report, reward).await?;
    }
    report.update(attrs! { status: STATUS_RESOLVED }).await?;

    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": "操作成功",
        "data": { "status": STATUS_RESOLVED },
    })))
}
