//! 用户中心页面（Phase 6）：仪表盘、角色、衣柜、资料、举报。

use serde_json::{Value, json};
use suprnova::{HttpResponse, Model, Request, Response, handler};

use crate::models::communication::{Communication, KIND_ANNOUNCEMENT, KIND_BOTH, STATUS_PUBLISHED};
use crate::models::report::Report;
use crate::models::user::User;
use crate::support::communication::render_communication;
use crate::support::i18n::translate;
use crate::support::view;
use crate::support::{locale, options};

async fn require_login(request: &Request) -> Result<User, HttpResponse> {
    match crate::support::auth::current_user(request).await? {
        Some(user) => Ok(user),
        None => Err(HttpResponse::text("")
            .status(302)
            .header("Location", "/auth/login")),
    }
}

fn menu_item(title: &str, link: &str, icon: &str, path: &str) -> Value {
    let active = path == link || path.starts_with(&format!("{link}/"));
    json!({
        "title": title,
        "link": link,
        "icon": icon,
        "active": active,
        "classes": if active { json!(["active", "menu-open"]) } else { json!([]) },
        "children": [],
    })
}

fn menus(path: &str, user: &User) -> Value {
    let user_items = vec![
        menu_item("general.dashboard", "user", "fa-tachometer-alt", path),
        menu_item("站内信", "user/inbox", "fa-envelope", path),
        menu_item("general.my-closet", "user/closet", "fa-star", path),
        menu_item("general.player-manage", "user/player", "fa-users", path),
        menu_item("general.my-reports", "user/reports", "fa-flag", path),
        menu_item("general.profile", "user/profile", "fa-user", path),
        json!({
            "title": "general.developer",
            "link": "",
            "icon": "fa-code-branch",
            "active": path.starts_with("user/oauth"),
            "classes": [],
            "children": [menu_item("general.oauth-manage", "user/oauth/manage", "fa-feather-alt", path)],
        }),
    ];
    let explore = vec![menu_item("general.skinlib", "skinlib", "fa-archive", path)];
    let mut groups = vec![
        json!({ "header": "general.user-center", "items": user_items }),
        json!({ "header": "general.explore", "items": explore }),
    ];
    if user.is_admin() {
        groups.push(json!({
            "header": "general.manage",
            "items": [menu_item("general.admin-panel", "admin", "fa-cog", path)],
        }));
    }
    json!(groups)
}

pub(crate) async fn page_context(request: &Request, user: &User, locale_code: &str) -> Value {
    let token = suprnova::get_csrf_token().unwrap_or_default();
    let mut context = view::base_context(locale_code, request.path(), &token, Some(user)).await;
    let (current, langs) = super::page::page_languages(locale_code, request.path());
    let extras = super::page::page_user_extras(user).await;
    let mut menu_value = menus(request.path(), user);
    crate::support::plugin_api::merge_menu_hooks(&mut menu_value, "user", request.path());
    crate::support::plugin_api::merge_menu_hooks(&mut menu_value, "explore", request.path());
    if let Some(object) = context.as_object_mut() {
        object.insert("current".to_string(), Value::String(current));
        object.insert("langs".to_string(), langs);
        object.insert("menus".to_string(), menu_value);
        object.insert("sidebar_color".to_string(), json!("dark-maroon"));
        object.insert(
            "badges".to_string(),
            Value::Array(crate::support::plugin_api::badges(Vec::new())),
        );
        object.insert("avatar".to_string(), extras["avatar"].clone());
        object.insert("avatar_png".to_string(), extras["avatar_png"].clone());
        object.insert("menu".to_string(), extras["menu"].clone());
        object.insert(
            "user".to_string(),
            json!({
                "uid": user.uid,
                "email": user.email,
                "nickname": user.nickname,
                "avatar": user.avatar,
                "isAdmin": user.is_admin(),
                "isDarkMode": user.is_dark_mode,
            }),
        );
        object.insert("notifications".to_string(), json!([]));
    }
    context
}

fn grid(layout: Value, widgets: Value) -> Value {
    json!({ "layout": layout, "widgets": widgets })
}

/// GET /user
#[handler]
pub async fn index(request: Request) -> Response {
    let user = match require_login(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = page_context(&request, &user, &locale_code).await;

    let locale_prefix = format!("_{locale_code}");
    let mut announcement = None;
    for communication in Communication::all().await?.into_vec() {
        let is_announcement = communication.kind == KIND_ANNOUNCEMENT
            || (communication.kind == KIND_BOTH && communication.status == STATUS_PUBLISHED);
        if !is_announcement {
            continue;
        }
        if !communication.locale.is_empty() && communication.locale != locale_code {
            continue;
        }
        if announcement
            .as_ref()
            .is_none_or(|current: &Communication| communication.updated_at > current.updated_at)
        {
            announcement = Some(communication);
        }
    }
    let announcement_html = announcement
        .as_ref()
        .map(render_communication)
        .unwrap_or_default();

    let (min, max) = options::value("sign_score")
        .await
        .unwrap_or_else(|_| "10,100".to_string())
        .split_once(',')
        .map(|(min, max)| (min.to_string(), max.to_string()))
        .unwrap_or_else(|| ("10".to_string(), "100".to_string()));
    let mut intro_params = serde_json::Map::new();
    intro_params.insert(
        "initial_score".to_string(),
        Value::String(
            options::value("user_initial_score")
                .await
                .unwrap_or_default(),
        ),
    );
    intro_params.insert("score-from".to_string(), Value::String(min));
    intro_params.insert("score-to".to_string(), Value::String(max));
    intro_params.insert(
        "return-score".to_string(),
        Value::String(translate(
            &locale_code,
            if options::bool_or("return_score", true).await.unwrap_or(true) {
                "user.score-intro.will-return-score"
            } else {
                "user.score-intro.no-return-score"
            },
            None,
        )),
    );
    let score_intro = translate(
        &locale_code,
        "user.score-intro.introduction",
        Some(&intro_params),
    );
    let rates = json!({
        "storage": options::value("score_per_storage").await.unwrap_or_default(),
        "player": options::value("score_per_player").await.unwrap_or_default(),
        "closet": options::value("score_per_closet_item").await.unwrap_or_default(),
    });
    let unverified = options::bool_or("require_verification", false)
        .await
        .unwrap_or(false)
        && !user.verified;

    if let Some(object) = context.as_object_mut() {
        object.insert(
            "grid".to_string(),
            grid(
                json!([["md-7", "md-5"]]),
                json!([[
                    [
                        "user/widgets/email-verification.html",
                        "user/widgets/dashboard/usage.html"
                    ],
                    ["user/widgets/dashboard/announcement.html"]
                ]]),
            ),
        );
        object.insert("announcement".to_string(), Value::String(announcement_html));
        object.insert("score_intro".to_string(), Value::String(score_intro));
        object.insert("rates".to_string(), rates);
        object.insert("extra".to_string(), json!({ "unverified": unverified }));
        object.insert("locale_prefix".to_string(), Value::String(locale_prefix));
    }
    Ok(view::render("user/index.html", context))
}

/// GET /user/player
#[handler]
pub async fn player(request: Request) -> Response {
    let user = match require_login(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = page_context(&request, &user, &locale_code).await;
    let rule = options::value("player_name_rule")
        .await
        .unwrap_or_else(|_| "official".to_string());
    let min = options::int_or("player_name_length_min", 3)
        .await
        .unwrap_or(3);
    let max = options::int_or("player_name_length_max", 16)
        .await
        .unwrap_or(16);
    let count = crate::models::player::Player::filter("uid", user.uid)
        .get()
        .await
        .map(|rows| rows.len())
        .unwrap_or(0);
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "grid".to_string(),
            grid(
                json!([["md-6", "md-6"]]),
                json!([[
                    [
                        "user/widgets/email-verification.html",
                        "user/widgets/players/list.html"
                    ],
                    ["user/widgets/players/notice.html", "shared/previewer.html"]
                ]]),
            ),
        );
        object.insert(
            "extra".to_string(),
            json!({
                "count": count,
                "rule": translate(&locale_code, &format!("user.player.player-name-rule.{rule}"), None),
                "length": translate(&locale_code, "user.player.player-name-length", Some(&{
                    let mut map = serde_json::Map::new();
                    map.insert("min".to_string(), Value::String(min.to_string()));
                    map.insert("max".to_string(), Value::String(max.to_string()));
                    map
                })),
                "score": user.score,
                "cost": options::int_or("score_per_player", 100).await.unwrap_or(100),
            }),
        );
    }
    Ok(view::render("user/player.html", context))
}

/// GET /user/closet
#[handler]
pub async fn closet(request: Request) -> Response {
    let user = match require_login(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = page_context(&request, &user, &locale_code).await;
    let rule = options::value("player_name_rule")
        .await
        .unwrap_or_else(|_| "official".to_string());
    let min = options::int_or("player_name_length_min", 3)
        .await
        .unwrap_or(3);
    let max = options::int_or("player_name_length_max", 16)
        .await
        .unwrap_or(16);
    let unverified = options::bool_or("require_verification", false)
        .await
        .unwrap_or(false)
        && !user.verified;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "grid".to_string(),
            grid(
                json!([["md-8", "md-4"]]),
                json!([[
                    [
                        "user/widgets/email-verification.html",
                        "user/widgets/closet/list.html"
                    ],
                    ["shared/previewer.html"]
                ]]),
            ),
        );
        let mut params = serde_json::Map::new();
        params.insert("min".to_string(), Value::String(min.to_string()));
        params.insert("max".to_string(), Value::String(max.to_string()));
        object.insert(
            "extra".to_string(),
            json!({
                "unverified": unverified,
                "rule": translate(&locale_code, &format!("user.player.player-name-rule.{rule}"), None),
                "length": translate(&locale_code, "user.player.player-name-length", Some(&params)),
            }),
        );
    }
    Ok(view::render("user/closet.html", context))
}

/// GET /user/profile
#[handler]
pub async fn profile(request: Request) -> Response {
    let user = match require_login(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = page_context(&request, &user, &locale_code).await;
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "grid".to_string(),
            grid(
                json!([["md-6", "md-6"]]),
                json!([[
                    [
                        "user/widgets/profile/avatar.html",
                        "user/widgets/profile/password.html"
                    ],
                    [
                        "user/widgets/profile/nickname.html",
                        "user/widgets/profile/email.html",
                        "user/widgets/profile/delete-account.html"
                    ]
                ]]),
            ),
        );
    }
    Ok(view::render("user/profile.html", context))
}

/// GET /user/reports
#[handler]
pub async fn reports(request: Request) -> Response {
    let user = match require_login(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = page_context(&request, &user, &locale_code).await;
    let statuses =
        crate::support::i18n::translate_value(&locale_code, "front-end.report.status", None);
    let mut rows = Vec::new();
    for report in Report::filter("reporter", user.uid).get().await?.into_vec() {
        let label = statuses
            .get(report.status as usize)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        rows.push(json!({
            "tid": report.tid,
            "reason": report.reason,
            "status": report.status,
            "status_label": label,
            "report_at": report.report_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        }));
    }
    if let Some(object) = context.as_object_mut() {
        object.insert("reports".to_string(), Value::Array(rows));
    }
    Ok(view::render("user/report.html", context))
}

/// GET /user/oauth/manage —— OAuth2 客户端管理页。
#[handler]
pub async fn oauth(request: Request) -> Response {
    let user = match require_login(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let context = page_context(&request, &user, &locale_code).await;
    Ok(view::render("user/oauth.html", context))
}
