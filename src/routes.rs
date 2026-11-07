use suprnova::{Router, any, delete, get, post, put, routes};

use crate::controllers;

routes! {
    get!("/__neoskin/health", controllers::health::index).name("health"),

    // SPA 壳与契约 v1 bootstrap（NeoSkinFrontend）
    get!("/api/bootstrap", controllers::spa::bootstrap).name("api.bootstrap"),
    get!("/app-next/{mode}", controllers::spa::shell).name("app-next.shell"),
    get!("/app-next/{mode}/", controllers::spa::shell).name("app-next.shell.index"),
    get!("/app-next/{mode}/{*path}", controllers::spa::shell).name("app-next.shell.path"),

    // 认证
    post!("/auth/login", controllers::auth::login).name("auth.login"),
    post!("/auth/register", controllers::auth::register).name("auth.register"),
    post!("/auth/logout", controllers::auth::logout).name("auth.logout"),
    post!("/auth/forgot", controllers::auth::forgot).name("auth.handle.forgot"),
    post!("/auth/reset/{uid}", controllers::auth::reset).name("auth.handle.reset"),
    post!("/auth/verify/{user}", controllers::auth::verify).name("auth.handle.verify"),
    post!("/user/email-verification", controllers::auth::send_verification_email).name("user.email-verification"),
    any!("/auth/captcha", controllers::auth::captcha_image).name("auth.captcha"),

    // 页面（Phase 6）
    get!("/", controllers::page::home).name("home"),
    get!("/auth/login", controllers::page::login_page).name("auth.login.view"),
    get!("/auth/register", controllers::page::register_page).name("auth.register.view"),
    get!("/auth/forgot", controllers::page::forgot_page).name("auth.forgot.view"),
    get!("/auth/reset/{uid}", controllers::page::reset_page).name("auth.reset.view"),
    get!("/auth/bind", controllers::page::bind_page).name("auth.bind.view"),
    post!("/auth/bind", controllers::page::fill_email).name("auth.bind"),
    get!("/auth/verify/{user}", controllers::page::verify_page).name("auth.verify.view"),

    // 用户中心页面
    get!("/user", controllers::user_page::index).name("user.home.view"),
    get!("/user/player", controllers::user_page::player).name("user.player.view"),
    get!("/user/closet", controllers::user_page::closet).name("user.closet.view"),
    get!("/user/profile", controllers::user_page::profile).name("user.profile.view"),
    get!("/user/reports", controllers::user_page::reports).name("user.reports.view"),
    get!("/api/user/reports", controllers::report::track).name("api.user.reports"),
    get!("/user/inbox", controllers::communication::inbox).name("user.inbox"),
    get!("/user/inbox/{id}", controllers::communication::message).name("user.inbox.message"),
    get!("/api/user/inbox", controllers::notification::inbox).name("api.user.inbox"),
    get!("/api/user/inbox/{id}", controllers::notification::message).name("api.user.inbox.message"),
    get!("/user/oauth/manage", controllers::user_page::oauth).name("user.oauth.manage"),

    // 当前用户（PHP：GET /api/user）
    get!("/api/user", controllers::auth::me).name("api.user"),
    get!("/api/user/notifications", controllers::notification::all).name("api.user.notifications"),
    post!("/api/user/notifications/{id}", controllers::notification::read).name("api.user.notifications.read"),

    // 用户资料 / 积分（PHP：UserController）
    get!("/user/score-info", controllers::user::score_info).name("user.score"),
    post!("/user/sign", controllers::user::sign).name("user.sign"),
    post!("/user/profile", controllers::user::handle_profile).name("user.profile.handle"),
    post!("/user/profile/avatar", controllers::user::set_avatar).name("user.profile.avatar"),
    put!("/user/dark-mode", controllers::user::toggle_dark_mode).name("user.dark-mode"),

    // Web 端 JSON 端点（与 /api 同处理器，供 legacy 前端调用）
    get!("/user/player/list", controllers::player::list).name("user.player.list"),
    post!("/user/player", controllers::player::add).name("user.player.add"),
    delete!("/user/player/{player}", controllers::player::delete).name("user.player.delete"),
    put!("/user/player/{player}/name", controllers::player::rename).name("user.player.rename"),
    put!("/user/player/{player}/textures", controllers::player::set_textures).name("user.player.set"),
    delete!("/user/player/{player}/textures", controllers::player::clear_textures).name("user.player.clear"),
    get!("/user/closet/list", controllers::closet::list).name("user.closet.list"),
    get!("/user/closet/ids", controllers::closet::ids).name("user.closet.ids"),
    post!("/user/closet", controllers::closet::add).name("user.closet.add"),
    put!("/user/closet/{tid}", controllers::closet::rename).name("user.closet.rename"),
    delete!("/user/closet/{tid}", controllers::closet::remove).name("user.closet.remove"),
    post!("/user/notifications/{id}", controllers::notification::read).name("user.notification.read"),

    // 玩家角色（PHP：PlayerController@list/add/delete/rename/setTexture/clearTexture）
    get!("/api/players", controllers::player::list).name("api.players.index"),
    post!("/api/players", controllers::player::add).name("api.players.store"),
    delete!("/api/players/{player}", controllers::player::delete).name("api.players.destroy"),
    put!("/api/players/{player}/name", controllers::player::rename).name("api.players.rename"),
    put!("/api/players/{player}/textures", controllers::player::set_textures).name("api.players.textures"),
    delete!("/api/players/{player}/textures", controllers::player::clear_textures).name("api.players.textures.clear"),

    // 衣柜（PHP：ClosetController@getClosetData/allIds/add/rename/remove）
    get!("/api/closet", controllers::closet::list).name("api.closet.index"),
    get!("/api/closet/ids", controllers::closet::ids).name("api.closet.ids"),
    post!("/api/closet", controllers::closet::add).name("api.closet.store"),
    put!("/api/closet/{tid}", controllers::closet::rename).name("api.closet.rename"),
    delete!("/api/closet/{tid}", controllers::closet::remove).name("api.closet.destroy"),

    // 纹理服务（PHP：TextureController；avatar/preview 待渲染器移植）
    get!("/textures/{hash}", controllers::texture::texture),
    get!("/csl/textures/{hash}", controllers::texture::texture),
    get!("/raw/{tid}", controllers::texture::raw),
    get!("/csl/{player}", controllers::texture::player_json),

    // 头像与预览（PHP：TextureController 的 avatar/preview 组）
    get!("/avatar/player/{name}", controllers::texture::avatar_by_player).name("avatar.player"),
    get!("/avatar/user/{uid}", controllers::texture::avatar_by_user).name("avatar.user"),
    get!("/avatar/hash/{hash}", controllers::texture::avatar_by_hash).name("avatar.hash"),
    get!("/avatar/{texture}", controllers::texture::avatar_by_texture).name("avatar.texture"),
    get!("/preview/hash/{hash}", controllers::texture::preview_by_hash).name("preview.hash"),
    get!("/preview/{texture}", controllers::texture::preview).name("preview.texture"),

    // 皮肤库（PHP：SkinlibController）
    get!("/skinlib", controllers::skinlib::home).name("skinlib.home"),
    get!("/skinlib/show/{texture}", controllers::skinlib::show_page).name("skinlib.show"),
    get!("/skinlib/upload", controllers::skinlib::upload_page).name("skinlib.upload"),
    get!("/skinlib/list", controllers::skinlib::library).name("skinlib.list"),
    get!("/texture/{texture}", controllers::skinlib::info).name("texture.info"),
    get!("/skinlib/info/{texture}", controllers::skinlib::info).name("skinlib.info"),
    post!("/texture", controllers::skinlib::handle_upload).name("texture.upload"),
    put!("/texture/{texture}/name", controllers::skinlib::rename).name("texture.rename"),
    put!("/texture/{texture}/type", controllers::skinlib::set_texture_type).name("texture.type"),
    put!("/texture/{texture}/privacy", controllers::skinlib::privacy).name("texture.privacy"),
    delete!("/texture/{texture}", controllers::skinlib::delete).name("texture.delete"),

    // 举报（PHP：ReportController）
    post!("/skinlib/report", controllers::report::submit).name("skinlib.report"),

    // 后台页面（PHP `ViewController` / `AdminController` / `OptionsController`）
    get!("/admin", controllers::admin_page::index).name("admin.view"),
    get!("/admin/chart", controllers::admin_page::chart_data).name("admin.chart"),
    get!("/admin/users", controllers::admin_page::users).name("admin.users.view"),
    get!("/admin/players", controllers::admin_page::players).name("admin.players.view"),
    get!("/admin/reports", controllers::admin_page::reports).name("admin.reports.view"),
    get!("/admin/i18n", controllers::admin_page::i18n_page).name("admin.i18n.view"),
    any!("/admin/customize", controllers::admin_page::customize_route).name("admin.customize.page"),
    any!("/admin/score", controllers::admin_page::score_route).name("admin.score.page"),
    any!("/admin/options", controllers::admin_page::options_route).name("admin.options.page"),
    any!("/admin/resource", controllers::admin_page::resource_route).name("admin.resource.page"),
    get!("/admin/status", controllers::admin_page::status).name("admin.status"),
    get!("/admin/communications", controllers::admin_page::communications).name("admin.communications.index"),
    get!("/admin/update", controllers::admin_page::update_page).name("admin.update.view"),
    get!("/admin/plugins/manage", controllers::admin_page::plugins_page).name("admin.plugins.view"),
    post!("/admin/plugins/manage", controllers::admin_page::plugins_manage).name("admin.plugins.manage"),
    get!("/admin/plugins/market", controllers::admin_page::market_page).name("admin.plugins.market.view"),
    get!("/admin/plugins/data", controllers::admin_page::plugins_data).name("admin.plugins.data"),
    get!("/admin/plugins/readme/{name}", controllers::admin_page::plugins_readme).name("admin.plugins.readme"),
    any!("/admin/plugins/config/{name}", controllers::admin_page::plugins_config).name("admin.plugins.config"),
    post!("/admin/plugins/upload", controllers::admin_page::plugins_upload).name("admin.plugins.upload"),
    post!("/admin/plugins/wget", controllers::admin_page::plugins_wget).name("admin.plugins.wget"),
    get!("/admin/plugins/market/list", controllers::admin_page::market_list).name("admin.plugins.market.list"),
    post!("/admin/plugins/market/download", controllers::admin_page::market_download).name("admin.plugins.market.download"),
    post!("/admin/update/download", controllers::admin_page::update_download).name("admin.update.download"),
    post!("/admin/update/install", controllers::admin_page::update_install).name("admin.update.install"),
    post!("/admin/update/upload", controllers::admin_page::update_upload).name("admin.update.upload"),

    // 安装向导（PHP `SetupController`）
    get!("/setup", controllers::setup::welcome).name("setup.view"),
    any!("/setup/database", controllers::setup::database).name("setup.database"),
    get!("/setup/info", controllers::setup::info).name("setup.info"),
    post!("/setup/finish", controllers::setup::finish).name("setup.finish"),

    get!("/admin/reports/list", controllers::report::manage).name("admin.reports.list"),
    put!("/admin/reports/{report}", controllers::report::review).name("admin.reports.review"),
    get!("/api/admin/reports", controllers::report::manage).name("api.admin.reports"),
    put!("/api/admin/reports/{report}", controllers::report::review).name("api.admin.reports.review"),

    // 后台：用户管理
    get!("/admin/users/list", controllers::admin::users_list).name("admin.users.list"),
    put!("/admin/users/{user}/email", controllers::admin::user_email).name("admin.users.email"),
    put!("/admin/users/{user}/verification", controllers::admin::user_verification).name("admin.users.verification"),
    put!("/admin/users/{user}/nickname", controllers::admin::user_nickname).name("admin.users.nickname"),
    put!("/admin/users/{user}/password", controllers::admin::user_password).name("admin.users.password"),
    put!("/admin/users/{user}/score", controllers::admin::user_score).name("admin.users.score"),
    put!("/admin/users/{user}/permission", controllers::admin::user_permission).name("admin.users.permission"),
    delete!("/admin/users/{user}", controllers::admin::user_delete).name("admin.users.delete"),

    // 后台：角色管理
    get!("/admin/players/list", controllers::admin::players_list).name("admin.players.list"),
    put!("/admin/players/{player}/name", controllers::admin::player_name).name("admin.players.name"),
    put!("/admin/players/{player}/owner", controllers::admin::player_owner).name("admin.players.owner"),
    put!("/admin/players/{player}/textures", controllers::admin::player_texture).name("admin.players.textures"),
    delete!("/admin/players/{player}", controllers::admin::player_delete).name("admin.players.delete"),

    // 后台：衣柜与通知
    post!("/admin/closet/{user}", controllers::admin::closet_add).name("admin.closet.add"),
    delete!("/admin/closet/{user}", controllers::admin::closet_remove).name("admin.closet.remove"),
    post!("/admin/notifications/send", controllers::admin::send_notification).name("admin.notifications.send"),
    get!("/admin/i18n/list", controllers::admin::translations_list).name("admin.i18n.list"),
    post!("/admin/i18n", controllers::admin::translation_create).name("admin.i18n.create"),
    put!("/admin/i18n/{line}", controllers::admin::translation_update).name("admin.i18n.update"),
    delete!("/admin/i18n/{line}", controllers::admin::translation_delete).name("admin.i18n.delete"),
    // 站点选项保存（POST 由 any! 路由分派）

    // 通信管理（PHP：CommunicationsController）
    post!("/admin/communications/preview", controllers::communication::preview).name("admin.communications.preview"),
    post!("/admin/communications/import", controllers::communication::import).name("admin.communications.import"),
    post!("/admin/communications", controllers::communication::store).name("admin.communications.store"),
    put!("/admin/communications/{communication}", controllers::communication::update).name("admin.communications.update"),
    // HTML 表单没有方法伪造，编辑表单直接 POST。
    post!("/admin/communications/{communication}", controllers::communication::update).name("admin.communications.update.post"),
    post!("/admin/communications/{communication}/withdraw", controllers::communication::withdraw).name("admin.communications.withdraw"),

    // API：后台别名
    get!("/api/admin/dashboard", controllers::admin::dashboard).name("api.admin.dashboard"),
    get!("/api/admin/options/{group}", controllers::admin::options_group).name("api.admin.options"),
    put!("/api/admin/options/{group}", controllers::admin::options_group_update).name("api.admin.options.update"),
    get!("/api/admin/users", controllers::admin::users_list).name("api.admin.users"),
    put!("/api/admin/users/{user}/email", controllers::admin::user_email).name("api.admin.users.email"),
    put!("/api/admin/users/{user}/verification", controllers::admin::user_verification).name("api.admin.users.verification"),
    put!("/api/admin/users/{user}/nickname", controllers::admin::user_nickname).name("api.admin.users.nickname"),
    put!("/api/admin/users/{user}/password", controllers::admin::user_password).name("api.admin.users.password"),
    put!("/api/admin/users/{user}/score", controllers::admin::user_score).name("api.admin.users.score"),
    put!("/api/admin/users/{user}/permission", controllers::admin::user_permission).name("api.admin.users.permission"),
    delete!("/api/admin/users/{user}", controllers::admin::user_delete).name("api.admin.users.delete"),
    get!("/api/admin/players", controllers::admin::players_list).name("api.admin.players"),
    put!("/api/admin/players/{player}/name", controllers::admin::player_name).name("api.admin.players.name"),
    put!("/api/admin/players/{player}/owner", controllers::admin::player_owner).name("api.admin.players.owner"),
    put!("/api/admin/players/{player}/textures", controllers::admin::player_texture).name("api.admin.players.textures"),
    delete!("/api/admin/players/{player}", controllers::admin::player_delete).name("api.admin.players.delete"),
    get!("/api/admin/closet/{user}", controllers::admin::closet_list).name("api.admin.closet"),
    post!("/api/admin/closet/{user}", controllers::admin::closet_add).name("api.admin.closet.add"),
    delete!("/api/admin/closet/{user}", controllers::admin::closet_remove).name("api.admin.closet.remove"),
    post!("/api/admin/notifications", controllers::admin::send_notification).name("api.admin.notifications"),

    // OAuth2 管理（Passport 兼容）
    get!("/oauth/scopes", controllers::oauth::scopes).name("oauth.scopes"),
    get!("/oauth/clients", controllers::oauth::clients).name("oauth.clients.index"),
    post!("/oauth/clients", controllers::oauth::store_client).name("oauth.clients.store"),
    put!("/oauth/clients/{client_id}", controllers::oauth::update_client).name("oauth.clients.update"),
    delete!("/oauth/clients/{client_id}", controllers::oauth::delete_client).name("oauth.clients.destroy"),
    get!("/oauth/tokens", controllers::oauth::tokens).name("oauth.tokens.index"),
    delete!("/oauth/tokens/{token_id}", controllers::oauth::delete_token).name("oauth.tokens.destroy"),
    get!("/oauth/personal-access-tokens", controllers::oauth::personal_tokens).name("oauth.personal.index"),
    post!("/oauth/personal-access-tokens", controllers::oauth::store_personal_token).name("oauth.personal.store"),
    delete!("/oauth/personal-access-tokens/{token_id}", controllers::oauth::delete_personal_token).name("oauth.personal.destroy"),
    get!("/oauth/authorize", controllers::oauth::authorize).name("oauth.authorize"),
    post!("/oauth/authorize", controllers::oauth::approve).name("oauth.authorize.approve"),
    delete!("/oauth/authorize", controllers::oauth::deny).name("oauth.authorize.deny"),
    post!("/oauth/token", controllers::oauth::token).name("oauth.token"),
    post!("/oauth/token/refresh", controllers::oauth::refresh_transient_token).name("oauth.token.refresh"),

    // 站点入口 / 运行时语言包
    any!("/api", controllers::home::api_root).name("api.root"),
    any!(
        "/.well-known/change-password",
        controllers::home::change_password
    )
    .name("well-known.change-password"),

    // Minecraft CustomSkinAPI 根路径 /{player}.json
    get!("/{player}", controllers::home::root_path).name("root.path"),
}

/// 核心路由 + 插件通过 Hook::addRoute() 注册的路由。
pub fn register_with_plugins() -> Router {
    crate::support::plugin_api::HOOK.apply_routes(register())
}
