//! 后台页面（PHP `AdminController@index/status`、`OptionsController`、
//! `ViewController` 系列与 `CommunicationsController@index`）。

use serde_json::{Map, Value, json};
use suprnova::{HttpResponse, Model, Request, Response, handler};

use crate::models::communication::{Communication, KIND_ANNOUNCEMENT, KIND_BOTH, KIND_MESSAGE};
use crate::models::notification::Entity as Notification;
use crate::models::player::Player;
use crate::models::texture::Entity as Texture;
use crate::models::user::User;
use crate::support::form::{Field, Form, Group};
use crate::support::i18n;
use crate::support::{locale, options, view};
use sea_orm::EntityTrait;

async fn require_admin(request: &Request) -> Result<User, HttpResponse> {
    match crate::support::auth::current_user(request).await? {
        Some(user) if user.is_admin() => Ok(user),
        Some(_) => Err(match crate::controllers::page::forbidden(request).await {
            Ok(response) | Err(response) => response,
        }),
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

fn menus(path: &str) -> Value {
    let items = vec![
        menu_item("general.dashboard", "admin", "fa-tachometer-alt", path),
        menu_item("站内信", "admin/communications", "fa-bullhorn", path),
        menu_item("general.user-manage", "admin/users", "fa-users", path),
        menu_item("general.player-manage", "admin/players", "fa-gamepad", path),
        menu_item("general.report-manage", "admin/reports", "fa-flag", path),
        menu_item(
            "general.customize",
            "admin/customize",
            "fa-paint-brush",
            path,
        ),
        menu_item("general.i18n", "admin/i18n", "fa-globe", path),
        menu_item(
            "general.score-options",
            "admin/score",
            "fa-credit-card",
            path,
        ),
        menu_item("general.options", "admin/options", "fa-cog", path),
        menu_item("general.res-options", "admin/resource", "fa-atom", path),
        menu_item(
            "general.status",
            "admin/status",
            "fa-battery-three-quarters",
            path,
        ),
        menu_item(
            "general.plugin-manage",
            "admin/plugins/manage",
            "fa-plug",
            path,
        ),
        menu_item(
            "general.plugin-market",
            "admin/plugins/market",
            "fa-shopping-bag",
            path,
        ),
        json!({
            "title": "general.plugin-configs",
            "link": "",
            "icon": "fa-cogs",
            "active": false,
            "classes": [],
            "children": [],
        }),
        menu_item("general.check-update", "admin/update", "fa-arrow-up", path),
    ];
    json!([
        { "header": "general.manage", "items": items },
    ])
}

async fn shell(request: &Request, user: &User, locale_code: &str) -> Value {
    let token = suprnova::get_csrf_token().unwrap_or_default();
    let mut context = view::base_context(locale_code, request.path(), &token, Some(user)).await;
    let (current, langs) = super::page::page_languages(locale_code, request.path());
    let extras = super::page::page_user_extras(user).await;
    let mut menu_value = menus(request.path());
    crate::support::plugin_api::merge_menu_hooks(&mut menu_value, "admin", request.path());
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
        object.insert("notifications".to_string(), json!([]));
    }
    context
}

fn grid(layout: Value, widgets: Value) -> Value {
    json!({ "layout": layout, "widgets": widgets })
}

fn insert(context: &mut Value, key: &str, value: Value) {
    if let Some(object) = context.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

async fn flag(locale_code: &str, name: &str) -> bool {
    matches!(
        options::localized(locale_code, name)
            .await
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "true" | "1" | "on" | "yes"
    )
}

async fn text_value(locale_code: &str, name: &str) -> String {
    options::localized(locale_code, name)
        .await
        .unwrap_or_default()
}

fn translated(locale_code: &str, key: &str) -> String {
    i18n::translate(locale_code, key, None)
}

/// GET /admin
#[handler]
pub async fn index(request: Request) -> Response {
    if crate::controllers::page::frontend_mode() == "next" {
        return Ok(suprnova::HttpResponse::text("")
            .status(302)
            .header("Location", "/app-next/admin/"));
    }
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;

    let users = User::all().await.map(|rows| rows.len()).unwrap_or(0);
    let players = Player::all().await.map(|rows| rows.len()).unwrap_or(0);
    let textures = Texture::find()
        .all(suprnova::DB::connection()?.inner())
        .await;
    let (texture_count, storage) = match textures {
        Ok(rows) => (
            rows.len(),
            rows.iter()
                .map(|texture| i64::from(texture.size.max(0)))
                .sum::<i64>(),
        ),
        Err(_) => (0, 0),
    };
    insert(
        &mut context,
        "grid",
        grid(
            json!([["md-6", "md-6"]]),
            json!([[
                ["admin/widgets/dashboard/usage.html"],
                ["admin/widgets/dashboard/chart.html"]
            ]]),
        ),
    );
    insert(
        &mut context,
        "sum",
        json!({ "users": users, "players": players, "textures": texture_count, "storage": storage }),
    );
    Ok(view::render("admin/index.html", context))
}

/// GET /admin/chart —— 最近 31 天用户注册与材质上传统计。
#[handler]
pub async fn chart_data(request: Request) -> Response {
    use chrono::{Datelike, Months, Utc};

    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);

    let today = Utc::now().date_naive();
    let one_month_ago = today.checked_sub_months(Months::new(1)).unwrap_or(today);
    let days: Vec<chrono::NaiveDate> = (0..31)
        .map(|offset| today - chrono::Duration::days(30 - offset))
        .collect();
    let weekday_name = |date: chrono::NaiveDate| match date.weekday() {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    };

    let mut registrations: std::collections::HashMap<&'static str, i64> =
        std::collections::HashMap::new();
    for row in User::all().await?.into_vec() {
        let register_at = row.register_at.date_naive();
        if register_at >= one_month_ago {
            *registrations.entry(weekday_name(register_at)).or_insert(0) += 1;
        }
    }
    let mut uploads: std::collections::HashMap<&'static str, i64> =
        std::collections::HashMap::new();
    let textures = Texture::find()
        .all(suprnova::DB::connection()?.inner())
        .await
        .map_err(|error| suprnova::FrameworkError::database(error.to_string()))?;
    for row in textures {
        let upload_at = row.upload_at.date();
        if upload_at >= one_month_ago {
            *uploads.entry(weekday_name(upload_at)).or_insert(0) += 1;
        }
    }

    let x_axis: Vec<&str> = days.into_iter().map(weekday_name).collect();
    let user_series: Vec<i64> = x_axis
        .iter()
        .map(|day| registrations.get(day).copied().unwrap_or(0))
        .collect();
    let texture_series: Vec<i64> = x_axis
        .iter()
        .map(|day| uploads.get(day).copied().unwrap_or(0))
        .collect();

    let _ = user;
    Ok(HttpResponse::json(json!({
        "labels": [
            translated(&locale_code, "admin.index.user-registration"),
            translated(&locale_code, "admin.index.texture-uploads"),
        ],
        "xAxis": x_axis,
        "data": [user_series, texture_series],
    })))
}

/// GET /admin/users
#[handler]
pub async fn users(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;
    insert(
        &mut context,
        "extra",
        json!({ "currentUser": super::admin::user_json_public(&user) }),
    );
    Ok(view::render("admin/users.html", context))
}

/// GET /admin/players
#[handler]
pub async fn players(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let context = shell(&request, &user, &locale_code).await;
    Ok(view::render("admin/players.html", context))
}

/// GET /admin/reports
#[handler]
pub async fn reports(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let context = shell(&request, &user, &locale_code).await;
    Ok(view::render("admin/reports.html", context))
}

/// GET /admin/i18n
#[handler]
pub async fn i18n_page(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let context = shell(&request, &user, &locale_code).await;
    Ok(view::render("admin/i18n.html", context))
}

/// GET /admin/customize
#[handler]
pub async fn customize(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;
    let csrf = suprnova::get_csrf_token().unwrap_or_default();

    let mut copyright = Field::select(&locale_code, "homepage", "copyright_prefer")
        .value(text_value(&locale_code, "copyright_prefer").await)
        .description(&locale_code, "homepage");
    for (index, text) in crate::controllers::home::COPYRIGHT.iter().enumerate() {
        copyright = copyright.option(index.to_string(), *text);
    }
    let homepage = Form::new(&locale_code, "homepage")
        .field(
            Field::text(&locale_code, "homepage", "home_pic_url")
                .value(text_value(&locale_code, "home_pic_url").await)
                .hint(&locale_code, "homepage"),
        )
        .field(
            Field::text(&locale_code, "homepage", "favicon_url")
                .value(text_value(&locale_code, "favicon_url").await)
                .hint(&locale_code, "homepage")
                .description(&locale_code, "homepage"),
        )
        .field(
            Field::checkbox(&locale_code, "homepage", "transparent_navbar")
                .checked(flag(&locale_code, "transparent_navbar").await)
                .label(&locale_code, "homepage"),
        )
        .field(
            Field::checkbox(&locale_code, "homepage", "hide_intro")
                .checked(flag(&locale_code, "hide_intro").await)
                .label(&locale_code, "homepage"),
        )
        .field(
            Field::checkbox(&locale_code, "homepage", "fixed_bg")
                .checked(flag(&locale_code, "fixed_bg").await)
                .label(&locale_code, "homepage"),
        )
        .field(copyright)
        .field(
            Field::textarea(&locale_code, "homepage", "copyright_text")
                .value(text_value(&locale_code, "copyright_text").await)
                .rows(6)
                .description(&locale_code, "homepage"),
        );
    let custom_js_css = Form::new(&locale_code, "customJsCss")
        .auto_message(&locale_code, "info")
        .field(
            Field::textarea(&locale_code, "customJsCss", "custom_css")
                .name("CSS")
                .value(text_value(&locale_code, "custom_css").await)
                .rows(6),
        )
        .field(
            Field::textarea(&locale_code, "customJsCss", "custom_js")
                .name("JavaScript")
                .value(text_value(&locale_code, "custom_js").await)
                .rows(6),
        );

    insert(
        &mut context,
        "forms",
        json!({
            "homepage": homepage.render(&locale_code, &csrf),
            "custom_js_css": custom_js_css.render(&locale_code, &csrf),
        }),
    );
    insert(
        &mut context,
        "colors",
        json!({
            "navbar": [
                "primary", "secondary", "success", "danger", "indigo",
                "purple", "pink", "teal", "cyan", "dark", "gray",
                "fuchsia", "maroon", "olive", "navy",
                "lime", "light", "warning", "white", "orange",
            ],
            "sidebar": [
                "primary", "warning", "info", "danger", "success", "indigo",
                "navy", "purple", "fuchsia", "pink", "maroon", "orange",
                "lime", "teal", "olive",
            ],
        }),
    );
    insert(
        &mut context,
        "extra",
        json!({
            "navbar": options::value("navbar_color").await.unwrap_or_default(),
            "sidebar": options::value("sidebar_color").await.unwrap_or_default(),
        }),
    );
    Ok(view::render("admin/customize.html", context))
}

/// GET /admin/score
#[handler]
pub async fn score(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;
    let csrf = suprnova::get_csrf_token().unwrap_or_default();

    let rate = Form::new(&locale_code, "rate")
        .group(
            Group::new(&locale_code, "rate", "score_per_storage")
                .text(
                    Field::text(&locale_code, "rate", "score_per_storage")
                        .value(text_value(&locale_code, "score_per_storage").await),
                )
                .addon(&locale_code, "rate"),
        )
        .group(
            Group::new(&locale_code, "rate", "private_score_per_storage")
                .text(
                    Field::text(&locale_code, "rate", "private_score_per_storage")
                        .value(text_value(&locale_code, "private_score_per_storage").await),
                )
                .addon(&locale_code, "rate")
                .hint(&locale_code, "rate"),
        )
        .group(
            Group::new(&locale_code, "rate", "score_per_closet_item")
                .text(
                    Field::text(&locale_code, "rate", "score_per_closet_item")
                        .value(text_value(&locale_code, "score_per_closet_item").await),
                )
                .addon(&locale_code, "rate"),
        )
        .field(
            Field::checkbox(&locale_code, "rate", "return_score")
                .checked(flag(&locale_code, "return_score").await)
                .label(&locale_code, "rate"),
        )
        .group(
            Group::new(&locale_code, "rate", "score_per_player")
                .text(
                    Field::text(&locale_code, "rate", "score_per_player")
                        .value(text_value(&locale_code, "score_per_player").await),
                )
                .addon(&locale_code, "rate"),
        )
        .field(
            Field::text(&locale_code, "rate", "user_initial_score")
                .value(text_value(&locale_code, "user_initial_score").await),
        );

    let report = Form::new(&locale_code, "report")
        .field(
            Field::text(&locale_code, "report", "reporter_score_modification")
                .value(text_value(&locale_code, "reporter_score_modification").await)
                .description(&locale_code, "report"),
        )
        .field(
            Field::text(&locale_code, "report", "reporter_reward_score")
                .value(text_value(&locale_code, "reporter_reward_score").await),
        );

    let (sign_from, sign_to) = options::value("sign_score")
        .await
        .unwrap_or_default()
        .split_once(',')
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .unwrap_or_default();
    let sign = Form::new(&locale_code, "sign")
        .group(
            Group::new(&locale_code, "sign", "sign_score")
                .text(
                    Field::text(&locale_code, "sign", "sign_score_from")
                        .name(translated(&locale_code, "options.sign.sign_score.title"))
                        .value(sign_from),
                )
                .addon_text(translated(&locale_code, "options.sign.sign_score.addon1"))
                .text(
                    Field::text(&locale_code, "sign", "sign_score_to")
                        .name(translated(&locale_code, "options.sign.sign_score.title"))
                        .value(sign_to),
                )
                .addon_text(translated(&locale_code, "options.sign.sign_score.addon2")),
        )
        .group(
            Group::new(&locale_code, "sign", "sign_gap_time")
                .text(
                    Field::text(&locale_code, "sign", "sign_gap_time")
                        .value(text_value(&locale_code, "sign_gap_time").await),
                )
                .addon(&locale_code, "sign"),
        )
        .field(
            Field::checkbox(&locale_code, "sign", "sign_after_zero")
                .checked(flag(&locale_code, "sign_after_zero").await)
                .label(&locale_code, "sign")
                .hint(&locale_code, "sign"),
        );

    let sharing = Form::new(&locale_code, "sharing")
        .group(
            Group::new(&locale_code, "sharing", "score_award_per_texture")
                .text(
                    Field::text(&locale_code, "sharing", "score_award_per_texture")
                        .value(text_value(&locale_code, "score_award_per_texture").await),
                )
                .addon_text(translated(&locale_code, "general.user.score")),
        )
        .field(
            Field::checkbox(&locale_code, "sharing", "take_back_scores_after_deletion")
                .checked(flag(&locale_code, "take_back_scores_after_deletion").await)
                .label(&locale_code, "sharing"),
        )
        .group(
            Group::new(&locale_code, "sharing", "score_award_per_like")
                .text(
                    Field::text(&locale_code, "sharing", "score_award_per_like")
                        .value(text_value(&locale_code, "score_award_per_like").await),
                )
                .addon_text(translated(&locale_code, "general.user.score")),
        );

    insert(
        &mut context,
        "forms",
        json!({
            "rate": rate.render(&locale_code, &csrf),
            "report": report.render(&locale_code, &csrf),
            "sign": sign.render(&locale_code, &csrf),
            "sharing": sharing.render(&locale_code, &csrf),
        }),
    );
    Ok(view::render("admin/score.html", context))
}

/// GET /admin/options
#[handler]
pub async fn options_page(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;
    let csrf = suprnova::get_csrf_token().unwrap_or_default();

    let max_upload_hint = i18n::translate(
        &locale_code,
        "options.general.max_upload_file_size.hint",
        Some(&{
            let mut params = Map::new();
            params.insert("size".to_string(), Value::String("2M".to_string()));
            params
        }),
    );
    let general = Form::new(&locale_code, "general")
        .field(
            Field::text(&locale_code, "general", "site_name")
                .value(text_value(&locale_code, "site_name").await),
        )
        .field(
            Field::text(&locale_code, "general", "site_description")
                .value(text_value(&locale_code, "site_description").await)
                .description(&locale_code, "general"),
        )
        .field(
            Field::text(&locale_code, "general", "site_url")
                .value(text_value(&locale_code, "site_url").await)
                .hint(&locale_code, "general"),
        )
        .field(
            Field::checkbox(&locale_code, "general", "register_with_player_name")
                .checked(flag(&locale_code, "register_with_player_name").await)
                .label(&locale_code, "general"),
        )
        .field(
            Field::checkbox(&locale_code, "general", "require_verification")
                .checked(flag(&locale_code, "require_verification").await)
                .label(&locale_code, "general"),
        )
        .field(
            Field::text(&locale_code, "general", "regs_per_ip")
                .value(text_value(&locale_code, "regs_per_ip").await),
        )
        .group(
            Group::new(&locale_code, "general", "max_upload_file_size")
                .text(
                    Field::text(&locale_code, "general", "max_upload_file_size")
                        .value(text_value(&locale_code, "max_upload_file_size").await),
                )
                .addon_text("KB")
                .hint_html(format!(
                    "<i class=\"fas fa-question-circle\" title=\"{}\" data-toggle=\"tooltip\" data-placement=\"top\"></i>",
                    crate::support::communication::escape_html(&max_upload_hint)
                )),
        )
        .group(
            Group::new(&locale_code, "general", "max_texture_width")
                .text(
                    Field::text(&locale_code, "general", "max_texture_width")
                        .value(text_value(&locale_code, "max_texture_width").await),
                )
                .addon_text("px")
                .hint(&locale_code, "general"),
        )
        .field(
            {
                let mut select = Field::select(&locale_code, "general", "player_name_rule")
                    .value(text_value(&locale_code, "player_name_rule").await);
                for rule in ["official", "cjk", "utf8", "custom"] {
                    select = select.option(
                        rule,
                        translated(&locale_code, &format!("options.general.player_name_rule.{rule}")),
                    );
                }
                select
            },
        )
        .field(
            Field::text(&locale_code, "general", "custom_player_name_regexp")
                .value(text_value(&locale_code, "custom_player_name_regexp").await)
                .hint(&locale_code, "general")
                .placeholder(&locale_code, "general"),
        )
        .group(
            Group::new(&locale_code, "general", "player_name_length")
                .text(
                    Field::text(&locale_code, "general", "player_name_length_min")
                        .value(text_value(&locale_code, "player_name_length_min").await),
                )
                .addon_text("~")
                .text(
                    Field::text(&locale_code, "general", "player_name_length_max")
                        .value(text_value(&locale_code, "player_name_length_max").await),
                )
                .addon_text(translated(&locale_code, "options.general.player_name_length.suffix")),
        )
        .field(
            Field::checkbox(&locale_code, "general", "auto_del_invalid_texture")
                .checked(flag(&locale_code, "auto_del_invalid_texture").await)
                .label(&locale_code, "general")
                .hint(&locale_code, "general"),
        )
        .field(
            Field::checkbox(&locale_code, "general", "allow_downloading_texture")
                .checked(flag(&locale_code, "allow_downloading_texture").await)
                .label(&locale_code, "general"),
        )
        .field(
            Field::select(&locale_code, "general", "status_code_for_private")
                .value(text_value(&locale_code, "status_code_for_private").await)
                .option("403", "403 Forbidden")
                .option("404", "404 Not Found"),
        )
        .field(
            Field::text(&locale_code, "general", "texture_name_regexp")
                .value(text_value(&locale_code, "texture_name_regexp").await)
                .hint(&locale_code, "general")
                .placeholder(&locale_code, "general"),
        )
        .field(
            Field::textarea(&locale_code, "general", "content_policy")
                .value(text_value(&locale_code, "content_policy").await)
                .rows(3)
                .description(&locale_code, "general"),
        );

    let meta = Form::new(&locale_code, "meta")
        .field(
            Field::text(&locale_code, "meta", "meta_keywords")
                .value(text_value(&locale_code, "meta_keywords").await)
                .hint(&locale_code, "meta"),
        )
        .field(
            Field::text(&locale_code, "meta", "meta_description")
                .value(text_value(&locale_code, "meta_description").await)
                .hint(&locale_code, "meta"),
        )
        .field(
            Field::textarea(&locale_code, "meta", "meta_extras")
                .value(text_value(&locale_code, "meta_extras").await)
                .rows(6),
        );

    let recaptcha = Form::new(&locale_code, "recaptcha")
        .title("reCAPTCHA")
        .field(
            Field::text(&locale_code, "recaptcha", "recaptcha_sitekey")
                .name("sitekey")
                .value(text_value(&locale_code, "recaptcha_sitekey").await),
        )
        .field(
            Field::text(&locale_code, "recaptcha", "recaptcha_secretkey")
                .name("secretkey")
                .value(text_value(&locale_code, "recaptcha_secretkey").await),
        )
        .field(
            Field::checkbox(&locale_code, "recaptcha", "recaptcha_invisible")
                .checked(flag(&locale_code, "recaptcha_invisible").await)
                .label(&locale_code, "recaptcha"),
        );

    let update_source = std::env::var("UPDATE_SOURCE")
        .unwrap_or_else(|_| "https://skin.blessing.dreamcraft.eu.org/update.json".to_string());
    let registry = std::env::var("PLUGINS_REGISTRY").unwrap_or_else(|_| {
        "https://plugin.blessing.dreamcraft.eu.org/registry_{lang}.json".to_string()
    });
    let updates = Form::new(&locale_code, "updates")
        .title("更新与插件仓库")
        .box_type("danger")
        .message(
            "<p class=\"text-danger font-weight-bold mb-0\">请注意，修改更新 URL 可能导致您的 Blessing Skin 被恶意代码注入，进而影响您的数据、服务器或内网资产安全。<br>我们无法 100% 保证第三方更新源的安全，修改这里之前<strong>请一定要知道你在做什么！</strong></p><div class=\"mt-3 d-flex flex-wrap\"><button type=\"submit\" name=\"update_source_preset\" value=\"official\" class=\"btn btn-default mr-2 mb-2\">切换到 bs-community 官方源</button><button type=\"submit\" name=\"update_source_preset\" value=\"patch\" class=\"btn btn-primary mb-2\">切换到 DreamCraft-Tech 第三方修补源</button></div>",
            "danger",
        )
        .field(
            Field::text(&locale_code, "updates", "update_source")
                .name("主站更新检测 URL")
                .value(text_value(&locale_code, "update_source").await)
                .hint_html(format!(
                    "<i class=\"fas fa-question-circle\" title=\"{}\" data-toggle=\"tooltip\" data-placement=\"top\"></i>",
                    crate::support::communication::escape_html(&update_source)
                )),
        )
        .field(
            Field::text(&locale_code, "updates", "plugins_registry")
                .name("插件仓库 URL")
                .value(text_value(&locale_code, "plugins_registry").await)
                .hint_html(format!(
                    "<i class=\"fas fa-question-circle\" title=\"{}\" data-toggle=\"tooltip\" data-placement=\"top\"></i>",
                    crate::support::communication::escape_html(&registry)
                )),
        );

    insert(
        &mut context,
        "forms",
        json!({
            "general": general.render(&locale_code, &csrf),
            "meta": meta.render(&locale_code, &csrf),
            "recaptcha": recaptcha.render(&locale_code, &csrf),
            "updates": updates.render(&locale_code, &csrf),
        }),
    );
    Ok(view::render("admin/options.html", context))
}

/// GET /admin/resource
#[handler]
pub async fn resource(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;
    let csrf = suprnova::get_csrf_token().unwrap_or_default();

    let resources = Form::new(&locale_code, "resources")
        .box_type("primary")
        .hint(&locale_code)
        .field(
            Field::checkbox(&locale_code, "resources", "force_ssl")
                .checked(flag(&locale_code, "force_ssl").await)
                .label(&locale_code, "resources")
                .hint(&locale_code, "resources"),
        )
        .field(
            Field::checkbox(&locale_code, "resources", "auto_detect_asset_url")
                .checked(flag(&locale_code, "auto_detect_asset_url").await)
                .label(&locale_code, "resources")
                .description(&locale_code, "resources"),
        )
        .field(
            Field::text(&locale_code, "resources", "cache_expire_time")
                .value(text_value(&locale_code, "cache_expire_time").await)
                .hint(&locale_code, "resources"),
        )
        .field(
            Field::text(&locale_code, "resources", "cdn_address")
                .value(text_value(&locale_code, "cdn_address").await)
                .hint(&locale_code, "resources")
                .description(&locale_code, "resources"),
        );

    let mut cache = Form::new(&locale_code, "cache")
        .box_type("warning")
        .auto_message(&locale_code, "info")
        .field(
            Field::checkbox(&locale_code, "cache", "enable_avatar_cache")
                .checked(flag(&locale_code, "enable_avatar_cache").await)
                .label(&locale_code, "cache"),
        )
        .field(
            Field::checkbox(&locale_code, "cache", "enable_preview_cache")
                .checked(flag(&locale_code, "enable_preview_cache").await)
                .label(&locale_code, "cache"),
        )
        .button(crate::support::form::Button::link(
            "?clear-cache",
            translated(&locale_code, "options.cache.clear"),
            "warning float-right",
        ));
    if request.has_query("clear-cache") {
        cache = cache.alert(translated(&locale_code, "options.cache.cleared"), "success");
    }

    insert(
        &mut context,
        "forms",
        json!({
            "resources": resources.render(&locale_code, &csrf),
            "cache": cache.render(&locale_code, &csrf),
        }),
    );
    Ok(view::render("admin/resource.html", context))
}

/// GET /admin/status
#[handler]
pub async fn status(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;

    let connection = std::env::var("DB_CONNECTION").unwrap_or_else(|_| "sqlite".to_string());
    let db_type = match connection.as_str() {
        "mysql" | "mariadb" => "MySQL/MariaDB",
        "sqlite" => "SQLite",
        "pgsql" | "postgres" | "postgresql" => "PostgreSQL",
        other => other,
    };
    let detail = json!({
        "bs": {
            "version": crate::controllers::home::VERSION,
            "env": std::env::var("APP_ENV").unwrap_or_else(|_| "production".to_string()),
            "debug": if std::env::var("APP_DEBUG").map(|value| value == "true").unwrap_or(false) {
                translated(&locale_code, "general.yes")
            } else {
                translated(&locale_code, "general.no")
            },
            "commit": "",
            "laravel": "Suprnova 4.6.2",
        },
        "server": {
            "php": format!("Rust {}", env!("CARGO_PKG_VERSION")),
            "web": "neoskin-rs",
            "os": format!(
                "{} {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH,
                std::env::consts::FAMILY
            ),
        },
        "db": {
            "type": db_type,
            "host": std::env::var("DB_HOST").unwrap_or_default(),
            "port": std::env::var("DB_PORT").unwrap_or_default(),
            "username": std::env::var("DB_USERNAME").unwrap_or_default(),
            "database": std::env::var("DB_DATABASE").unwrap_or_default(),
            "prefix": "",
        },
    });
    insert(
        &mut context,
        "grid",
        grid(
            json!([["md-6", "md-6"]]),
            json!([[
                ["admin/widgets/status/info.html"],
                ["admin/widgets/status/plugins.html"]
            ]]),
        ),
    );
    insert(&mut context, "detail", detail);
    insert(&mut context, "plugins", json!([]));
    Ok(view::render("admin/status.html", context))
}

/// GET /admin/communications
#[handler]
pub async fn communications(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;

    let editing_id = request
        .query_param("edit")
        .and_then(|value| value.parse::<u64>().ok());
    let announcement = request
        .query_param("announcement")
        .map(|value| value == "1" || value == "true")
        .unwrap_or(false);
    let new = request
        .query_param("new")
        .map(|value| value == "1" || value == "true")
        .unwrap_or(false);

    let page = request
        .query_param("page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    let per_page = 15usize;
    let mut all: Vec<Communication> = Communication::all()
        .await?
        .into_vec()
        .into_iter()
        .filter(|item| item.kind == KIND_MESSAGE || item.kind == KIND_BOTH)
        .collect();
    all.sort_by(|left, right| right.id.cmp(&left.id));
    let total = all.len();
    let last_page = total.div_ceil(per_page).max(1);
    let page = page.min(last_page);
    let start = (page - 1) * per_page;

    let notifications = Notification::find()
        .all(suprnova::DB::connection()?.inner())
        .await
        .map_err(|error| suprnova::FrameworkError::database(error.to_string()))?;
    let mut counts: std::collections::HashMap<u64, (i64, i64)> = std::collections::HashMap::new();
    for notification in &notifications {
        let Ok(data) = serde_json::from_str::<Value>(&notification.data) else {
            continue;
        };
        let Some(id) = data.get("communication_id").and_then(Value::as_i64) else {
            continue;
        };
        let entry = counts.entry(id as u64).or_insert((0, 0));
        entry.0 += 1;
        if notification.read_at.is_some() {
            entry.1 += 1;
        }
    }

    let mut rows = Vec::new();
    for item in all.iter().skip(start).take(per_page) {
        let (delivery, read) = counts.get(&item.id).copied().unwrap_or((0, 0));
        let kind_label = match item.kind.as_str() {
            KIND_ANNOUNCEMENT => "公告",
            KIND_MESSAGE => "站内信",
            _ => "公告与站内信",
        };
        let status_label = match item.status.as_str() {
            "draft" => "草稿",
            "published" => "已发布",
            _ => "已撤回",
        };
        rows.push(json!({
            "id": item.id,
            "title": item.title,
            "kind": item.kind,
            "kind_label": kind_label,
            "status": item.status,
            "status_label": status_label,
            "read_count": read,
            "delivery_count": delivery,
            "updated_at": item.updated_at.map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default(),
        }));
    }

    let editing = match editing_id {
        Some(id) => Communication::find(id).await.ok().flatten(),
        None => None,
    };
    let editing_json = editing
        .as_ref()
        .map(|item| {
            json!({
                "id": item.id,
                "status": item.status,
                "kind": item.kind,
            })
        })
        .unwrap_or(Value::Null);

    let seed = if let Some(item) = &editing {
        json!({
            "title": item.title,
            "kind": item.kind,
            "editor": item.editor,
            "source": item.source.clone().unwrap_or_default(),
            "locale": item.locale,
            "blocks": item.blocks.as_ref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .unwrap_or(json!([])),
            "audience": item.audience.as_ref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .unwrap_or(json!({ "receiver": "all" })),
        })
    } else if announcement {
        json!({
            "title": "站点公告",
            "kind": KIND_ANNOUNCEMENT,
            "editor": "blocks",
            "source": "",
            "locale": "",
            "blocks": [],
            "audience": { "receiver": "all" },
        })
    } else {
        json!({
            "title": "",
            "kind": KIND_MESSAGE,
            "editor": "blocks",
            "source": "",
            "locale": "",
            "blocks": [],
            "audience": { "receiver": "all" },
        })
    };

    insert(&mut context, "messages", Value::Array(rows));
    insert(
        &mut context,
        "pagination",
        json!({
            "total": total,
            "current_page": page,
            "last_page": last_page,
            "has_more": page < last_page,
            "previous_url": if page > 1 { format!("/admin/communications?page={}", page - 1) } else { String::new() },
            "next_url": if page < last_page { format!("/admin/communications?page={}", page + 1) } else { String::new() },
        }),
    );
    insert(&mut context, "editing", editing_json);
    insert(&mut context, "editor_data", seed);
    insert(
        &mut context,
        "editor_open",
        json!(editing_id.is_some() || announcement || new),
    );
    let notice = suprnova::session_mut(|data| data.pull::<String>("communication_notice"))
        .flatten()
        .unwrap_or_default();
    insert(&mut context, "communication_notice", Value::String(notice));
    Ok(view::render("admin/communications.html", context))
}

/// GET /admin/update
#[handler]
pub async fn update_page(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;
    let check = crate::support::update::check().await?;
    let notice = suprnova::session_mut(|data| data.pull::<String>("update_notice"))
        .flatten()
        .unwrap_or_default();
    let update_error = suprnova::session_mut(|data| data.pull::<String>("update_error"))
        .flatten()
        .unwrap_or_default();
    let error = if check.error.is_empty() {
        update_error
    } else {
        check.error
    };
    insert(&mut context, "can_update", json!(check.can_update));
    insert(&mut context, "info", check.info);
    insert(&mut context, "error", Value::String(error));
    insert(&mut context, "update_notice", Value::String(notice));
    insert(
        &mut context,
        "upload_limits",
        json!({
            "effective": "256 MB",
            "php_upload": "—",
            "php_post": "—",
            "application": "256 MB",
            "enabled": true,
        }),
    );
    match crate::support::update::pending() {
        Some(pending) => insert(
            &mut context,
            "pending",
            serde_json::to_value(&pending).unwrap_or(Value::Null),
        ),
        None => insert(&mut context, "pending", Value::Null),
    }
    insert(&mut context, "package", Value::Null);
    insert(&mut context, "patch_releases", json!([]));
    Ok(view::render("admin/update.html", context))
}

/// GET /admin/plugins/manage
#[handler]
pub async fn plugins_page(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let context = shell(&request, &user, &locale_code).await;
    Ok(view::render("admin/plugins.html", context))
}

/// GET /admin/plugins/market
#[handler]
pub async fn market_page(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let locale_code = locale::resolve(&request);
    let context = shell(&request, &user, &locale_code).await;
    Ok(view::render("admin/market.html", context))
}

/// GET /admin/plugins/data —— 已安装插件列表（清单驱动）。
#[handler]
pub async fn plugins_data(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    Ok(HttpResponse::json(crate::support::plugin::data().await?))
}

/// GET /admin/plugins/readme/{name}
#[handler]
pub async fn plugins_readme(request: Request) -> Response {
    let user = match require_admin(&request).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };
    let Some(name) = request.param("name").ok().map(str::to_string) else {
        return Ok(HttpResponse::text("Not Found").status(404));
    };
    let Some((title, content)) = crate::support::plugin::readme(&name).await? else {
        return Ok(HttpResponse::text("插件不存在或没有 README").status(404));
    };
    let locale_code = locale::resolve(&request);
    let mut context = shell(&request, &user, &locale_code).await;
    insert(&mut context, "title", Value::String(title));
    insert(&mut context, "content", Value::String(content));
    Ok(view::render("admin/plugin/readme.html", context))
}

/// GET|POST /admin/plugins/config/{name}
#[handler]
pub async fn plugins_config(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    Ok(HttpResponse::text("插件不存在或没有配置项").status(404))
}

#[derive(serde::Deserialize, Default)]
pub struct PluginAction {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub action: String,
}

fn plugin_message(locale_code: &str, key: &str, params: &[(String, String)]) -> String {
    let mut map = serde_json::Map::new();
    for (name, value) in params {
        map.insert(name.clone(), Value::String(value.clone()));
    }
    i18n::translate(locale_code, key, Some(&map))
}

/// POST /admin/plugins/manage
#[handler]
pub async fn plugins_manage(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    let locale_code = locale::resolve(&request);
    let body: PluginAction = request.input().await?;
    let name = body.name.trim();
    if name.is_empty() {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": translated(&locale_code, "admin.plugins.operations.not-found"),
        })));
    }
    let title = match crate::support::plugin::all()
        .await?
        .into_iter()
        .find(|plugin| plugin.name == name)
    {
        Some(plugin) => plugin.title(),
        None => {
            return Ok(HttpResponse::json(json!({
                "code": 1,
                "message": translated(
                    &locale_code,
                    "admin.plugins.operations.not-found",
                ),
            })));
        }
    };
    let params = vec![("plugin".to_string(), title)];

    match body.action.trim() {
        "enable" => match crate::support::plugin::enable(name, &locale_code).await? {
            None => Ok(HttpResponse::json(json!({
                "code": 0,
                "message": plugin_message(
                    &locale_code,
                    "admin.plugins.operations.enabled",
                    &params,
                ),
            }))),
            Some(reasons) => Ok(HttpResponse::json(json!({
                "code": 1,
                "message": translated(
                    &locale_code,
                    "admin.plugins.operations.unsatisfied.notice",
                ),
                "data": { "reason": reasons },
            }))),
        },
        "disable" => {
            crate::support::plugin::disable(name).await?;
            Ok(HttpResponse::json(json!({
                "code": 0,
                "message": plugin_message(
                    &locale_code,
                    "admin.plugins.operations.disabled",
                    &params,
                ),
            })))
        }
        "delete" => {
            if crate::support::plugin::delete(name).await? {
                Ok(HttpResponse::json(json!({
                    "code": 0,
                    "message": translated(
                        &locale_code,
                        "admin.plugins.operations.deleted",
                    ),
                })))
            } else {
                Ok(HttpResponse::json(json!({
                    "code": 1,
                    "message": translated(
                        &locale_code,
                        "admin.plugins.operations.not-found",
                    ),
                })))
            }
        }
        _ => Ok(HttpResponse::json(json!({
            "code": 1,
            "message": translated(&locale_code, "admin.invalid-action"),
        }))),
    }
}

/// POST /admin/plugins/upload
#[handler]
pub async fn plugins_upload(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    #[derive(serde::Deserialize, Default)]
    struct Upload {
        #[serde(default)]
        file: Option<suprnova::UploadedFile>,
    }
    let locale_code = locale::resolve(&request);
    let body: Upload = request.input().await?;
    let Some(file) = body.file else {
        return Err(crate::support::api::validation_error("file", "file 不能为空。").into());
    };
    let data = file.bytes().await?;
    crate::support::plugin_market::install_zip(&data)?;
    Ok(HttpResponse::json(json!({
        "code": 0,
        "message": translated(&locale_code, "admin.plugins.market.install-success"),
    })))
}

/// POST /admin/plugins/wget
#[handler]
pub async fn plugins_wget(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    #[derive(serde::Deserialize, Default)]
    struct Wget {
        #[serde(default)]
        url: String,
    }
    let locale_code = locale::resolve(&request);
    let body: Wget = request.input().await?;
    let url = body.url.trim();
    if url.is_empty() {
        return Err(crate::support::api::validation_error("url", "url 不能为空。").into());
    }
    match crate::support::plugin_market::download_and_install(url).await {
        Ok(()) => Ok(HttpResponse::json(json!({
            "code": 0,
            "message": translated(&locale_code, "admin.plugins.market.install-success"),
        }))),
        Err(error) => Ok(HttpResponse::json(json!({
            "code": 1,
            "message": plugin_message(
                &locale_code,
                "admin.download.errors.download",
                &[("error".to_string(), error.to_string())],
            ),
        }))),
    }
}

/// GET /admin/plugins/market/list —— 拉取并补充插件市场条目。
#[handler]
pub async fn market_list(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    let locale_code = locale::resolve(&request);
    let packages = match crate::support::plugin_market::fetch(&locale_code).await {
        Ok(packages) => packages,
        Err(error) => {
            return Ok(HttpResponse::json(json!({
                "code": 1,
                "message": plugin_message(
                    &locale_code,
                    "admin.plugins.market.connection-error",
                    &[("error".to_string(), error.to_string())],
                ),
            }))
            .status(502));
        }
    };
    let packages = crate::support::plugin_market::enrich(packages).await?;
    Ok(HttpResponse::json(Value::Array(packages)))
}

#[derive(serde::Deserialize, Default)]
pub struct MarketDownload {
    #[serde(default)]
    pub name: String,
}

/// POST /admin/plugins/market/download
#[handler]
pub async fn market_download(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    let locale_code = locale::resolve(&request);
    let body: MarketDownload = request.input().await?;
    let name = body.name.trim().to_string();
    let packages = match crate::support::plugin_market::fetch(&locale_code).await {
        Ok(packages) => packages,
        Err(error) => {
            return Ok(HttpResponse::json(json!({
                "code": 1,
                "message": plugin_message(
                    &locale_code,
                    "admin.plugins.market.connection-error",
                    &[("error".to_string(), error.to_string())],
                ),
            })));
        }
    };
    let Some(package) = crate::support::plugin_market::find_package(&packages, &name) else {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": plugin_message(
                &locale_code,
                "admin.plugins.market.non-existent",
                &[("plugin".to_string(), name)],
            ),
        })));
    };
    let Some(url) = crate::support::plugin_market::package_url(package) else {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": plugin_message(
                &locale_code,
                "admin.plugins.market.non-existent",
                &[("plugin".to_string(), name)],
            ),
        })));
    };
    match crate::support::plugin_market::download_and_install(url).await {
        Ok(()) => Ok(HttpResponse::json(json!({
            "code": 0,
            "message": translated(&locale_code, "admin.plugins.market.install-success"),
        }))),
        Err(error) => Ok(HttpResponse::json(json!({
            "code": 1,
            "message": plugin_message(
                &locale_code,
                "admin.download.errors.download",
                &[("error".to_string(), error.to_string())],
            ),
        }))),
    }
}

/// POST /admin/update/download —— 下载并暂存运行包。
#[handler]
pub async fn update_download(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    let wants_json = request.expects_json();
    match crate::support::update::download_and_stage().await {
        Ok(pending) => update_success(
            wants_json,
            format!("运行包 {} 已下载并校验，请确认后安装。", pending.version),
            serde_json::to_value(&pending).unwrap_or(Value::Null),
        ),
        Err(error) => update_failure(wants_json, error.to_string()),
    }
}

/// POST /admin/update/upload —— 上传运行包并暂存。
#[handler]
pub async fn update_upload(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    let wants_json = request.expects_json();
    #[derive(serde::Deserialize, Default)]
    struct Upload {
        #[serde(default)]
        package: Option<suprnova::UploadedFile>,
    }
    let body: Upload = request.input().await?;
    let Some(file) = body.package else {
        return Err(crate::support::api::validation_error("package", "package 不能为空。").into());
    };
    let bytes = file.bytes().await?;
    match crate::support::update::upload_and_stage(&bytes) {
        Ok(pending) => update_success(
            wants_json,
            format!("运行包 {} 已暂存，请确认后安装。", pending.version),
            serde_json::to_value(&pending).unwrap_or(Value::Null),
        ),
        Err(error) => update_failure(wants_json, error.to_string()),
    }
}

/// POST /admin/update/install —— 安装已暂存的运行包。
#[handler]
pub async fn update_install(request: Request) -> Response {
    if let Err(response) = require_admin(&request).await {
        return Ok(response);
    }
    let wants_json = request.expects_json();
    match crate::support::update::install() {
        Ok(report) => {
            let suffix = if report.binary_updated {
                "；二进制已写入 *.new，请重启服务完成替换"
            } else {
                "；重启服务后生效"
            };
            update_success(
                wants_json,
                format!("系统程序已更新到 {}{suffix}", report.version),
                report.to_json(),
            )
        }
        Err(error) => update_failure(wants_json, error.to_string()),
    }
}

fn update_success(wants_json: bool, message: String, data: Value) -> Response {
    if wants_json {
        return Ok(HttpResponse::json(json!({
            "code": 0,
            "message": message,
            "data": data,
        })));
    }
    suprnova::session_mut(|session| session.put("update_notice", message));
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/update"))
}

fn update_failure(wants_json: bool, message: String) -> Response {
    if wants_json {
        return Ok(HttpResponse::json(json!({
            "code": 1,
            "message": message,
        })));
    }
    suprnova::session_mut(|session| session.put("update_error", message));
    Ok(HttpResponse::text("")
        .status(302)
        .header("Location", "/admin/update"))
}

/// `ANY /admin/customize` —— POST 保存选项，其余方法渲染页面。
#[handler]
pub async fn customize_route(request: Request) -> Response {
    if request.method().as_str() == "POST" {
        super::admin::save_customize(request).await
    } else {
        customize(request).await
    }
}

/// `ANY /admin/score`
#[handler]
pub async fn score_route(request: Request) -> Response {
    if request.method().as_str() == "POST" {
        super::admin::save_score(request).await
    } else {
        score(request).await
    }
}

/// `ANY /admin/options`
#[handler]
pub async fn options_route(request: Request) -> Response {
    if request.method().as_str() == "POST" {
        super::admin::save_options(request).await
    } else {
        options_page(request).await
    }
}

/// `ANY /admin/resource`
#[handler]
pub async fn resource_route(request: Request) -> Response {
    if request.method().as_str() == "POST" {
        super::admin::save_resource(request).await
    } else {
        resource(request).await
    }
}
