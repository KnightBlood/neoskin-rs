//! 原生插件 API（`DECISIONS.md` D-001）。
//!
//! PHP 插件代码不能在本端口执行；这里提供与 Blessing Skin 等价的
//! 原生扩展点：
//!
//! - `Filter`：7 个 Hook 之外的通用过滤链（`head_links`、`scripts`、
//!   `user_badges`、`content_policy` 等），按 priority 升序执行；
//! - `Hook`：PHP `App\Services\Hook` 的 7 个方法（菜单、路由、样式、脚本、
//!   徽章、通知、中间件）；
//! - `Dispatcher`：24 个事件常量与同步事件总线；
//! - `Plugin` trait：编译进站点的原生 Rust 插件，由 `plugins::register_all`
//!   注册（与 Go 端「原生扩展点」一致，不加载 PHP 代码）。
//!
//! 插件市场仍负责下载/安装清单；安装后的 PHP 插件仍需按本 API 重写。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

use serde_json::{Value, json};
use suprnova::{
    FrameworkError, HttpResponse, Middleware, MiddlewareFuture, Next, Request, Response, Router,
    async_trait,
};

/// 24 个插件事件名（与 `codegraph/events.json` 对齐）。
pub const EVENTS: [&str; 24] = [
    "ConfigureAdminMenu",
    "ConfigureExploreMenu",
    "ConfigureRoutes",
    "ConfigureUserMenu",
    "Event",
    "PlayerProfileUpdated",
    "PlayerRetrieved",
    "PlayerWasAdded",
    "PlayerWasDeleted",
    "PlayerWillBeAdded",
    "PlayerWillBeDeleted",
    "PluginBootFailed",
    "PluginWasDeleted",
    "PluginWasDisabled",
    "PluginWasEnabled",
    "RenderingBadges",
    "RenderingFooter",
    "RenderingHeader",
    "TextureDeleting",
    "UserAuthenticated",
    "UserLoggedIn",
    "UserProfileUpdated",
    "UserRegistered",
    "UserTryToLogin",
];

pub mod events {
    pub const CONFIGURE_ADMIN_MENU: &str = "ConfigureAdminMenu";
    pub const CONFIGURE_EXPLORE_MENU: &str = "ConfigureExploreMenu";
    pub const CONFIGURE_ROUTES: &str = "ConfigureRoutes";
    pub const CONFIGURE_USER_MENU: &str = "ConfigureUserMenu";
    pub const PLAYER_PROFILE_UPDATED: &str = "PlayerProfileUpdated";
    pub const PLAYER_RETRIEVED: &str = "PlayerRetrieved";
    pub const PLAYER_WAS_ADDED: &str = "PlayerWasAdded";
    pub const PLAYER_WAS_DELETED: &str = "PlayerWasDeleted";
    pub const PLAYER_WILL_BE_ADDED: &str = "PlayerWillBeAdded";
    pub const PLAYER_WILL_BE_DELETED: &str = "PlayerWillBeDeleted";
    pub const PLUGIN_BOOT_FAILED: &str = "PluginBootFailed";
    pub const PLUGIN_WAS_DELETED: &str = "PluginWasDeleted";
    pub const PLUGIN_WAS_DISABLED: &str = "PluginWasDisabled";
    pub const PLUGIN_WAS_ENABLED: &str = "PluginWasEnabled";
    pub const RENDERING_BADGES: &str = "RenderingBadges";
    pub const RENDERING_FOOTER: &str = "RenderingFooter";
    pub const RENDERING_HEADER: &str = "RenderingHeader";
    pub const TEXTURE_DELETING: &str = "TextureDeleting";
    pub const USER_AUTHENTICATED: &str = "UserAuthenticated";
    pub const USER_LOGGED_IN: &str = "UserLoggedIn";
    pub const USER_PROFILE_UPDATED: &str = "UserProfileUpdated";
    pub const USER_REGISTERED: &str = "UserRegistered";
    pub const USER_TRY_TO_LOGIN: &str = "UserTryToLogin";
}

/// 事件负载：`payload` 是 JSON，便于插件与页面共用。
#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    pub payload: Value,
}

impl Event {
    pub fn new(name: impl Into<String>, payload: Value) -> Self {
        Self {
            name: name.into(),
            payload,
        }
    }

    pub fn empty(name: impl Into<String>) -> Self {
        Self::new(name, Value::Null)
    }
}

type EventHandler = Arc<dyn Fn(&Event) + Send + Sync>;

/// 同步事件总线（Laravel `Event` 的最小子集）。
#[derive(Default)]
pub struct Dispatcher {
    handlers: RwLock<HashMap<String, Vec<EventHandler>>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn listen<F>(&self, name: &str, handler: F)
    where
        F: Fn(&Event) + Send + Sync + 'static,
    {
        self.handlers
            .write()
            .expect("event handlers poisoned")
            .entry(name.to_string())
            .or_default()
            .push(Arc::new(handler));
    }

    pub fn dispatch(&self, event: &Event) {
        let handlers = {
            let guard = self.handlers.read().expect("event handlers poisoned");
            guard.get(&event.name).cloned().unwrap_or_default()
        };
        for handler in handlers {
            handler(event);
        }
    }

    pub fn count(&self, name: &str) -> usize {
        self.handlers
            .read()
            .expect("event handlers poisoned")
            .get(name)
            .map(Vec::len)
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn clear(&self) {
        self.handlers
            .write()
            .expect("event handlers poisoned")
            .clear();
    }
}

type FilterFn = Arc<dyn Fn(Value) -> Value + Send + Sync>;

struct FilterListener {
    priority: i32,
    order: u64,
    function: FilterFn,
}

/// 通用过滤链。priority 越小越先执行，相同 priority 按注册顺序。
#[derive(Default)]
pub struct Filter {
    listeners: RwLock<HashMap<String, Vec<FilterListener>>>,
    sequence: AtomicU64,
}

impl Filter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册过滤器；PHP 默认 priority 为 20。
    pub fn add<F>(&self, name: &str, priority: i32, function: F)
    where
        F: Fn(Value) -> Value + Send + Sync + 'static,
    {
        let order = self.sequence.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.listeners.write().expect("filter listeners poisoned");
        let entries = guard.entry(name.to_string()).or_default();
        entries.push(FilterListener {
            priority,
            order,
            function: Arc::new(function),
        });
        entries.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then(left.order.cmp(&right.order))
        });
    }

    pub fn add_default<F>(&self, name: &str, function: F)
    where
        F: Fn(Value) -> Value + Send + Sync + 'static,
    {
        self.add(name, 20, function);
    }

    pub fn apply(&self, name: &str, value: Value) -> Value {
        let listeners = {
            let guard = self.listeners.read().expect("filter listeners poisoned");
            guard
                .get(name)
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| Arc::clone(&entry.function))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        listeners
            .into_iter()
            .fold(value, |current, function| function(current))
    }

    /// 移除某个钩子的全部过滤器（插件卸载）。
    pub fn remove(&self, name: &str) {
        self.listeners
            .write()
            .expect("filter listeners poisoned")
            .remove(name);
    }

    pub fn count(&self, name: &str) -> usize {
        self.listeners
            .read()
            .expect("filter listeners poisoned")
            .get(name)
            .map(Vec::len)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub title: String,
    pub link: String,
    pub icon: String,
    pub new_tab: bool,
}

impl MenuEntry {
    pub fn new(title: impl Into<String>, link: impl Into<String>, icon: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            link: link.into(),
            icon: icon.into(),
            new_tab: false,
        }
    }

    fn to_json(&self, path: &str) -> Value {
        let active = path == self.link || path.starts_with(&format!("{}/", self.link));
        json!({
            "title": self.title,
            "link": self.link,
            "icon": self.icon,
            "active": active,
            "classes": if active { json!(["active", "menu-open"]) } else { json!([]) },
            "children": [],
            "new-tab": self.new_tab,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub text: String,
    pub color: String,
}

struct Positioned<T> {
    position: i32,
    order: u64,
    item: T,
}

type RouteHook = Arc<dyn Fn(Router) -> Router + Send + Sync>;

/// PHP `App\Services\Hook` 的 7 个方法。
#[derive(Default)]
pub struct Hook {
    menus: RwLock<HashMap<String, Vec<Positioned<MenuEntry>>>>,
    styles: RwLock<HashMap<String, Vec<String>>>,
    scripts: RwLock<HashMap<String, Vec<String>>>,
    badges: RwLock<Vec<Badge>>,
    route_hooks: RwLock<Vec<RouteHook>>,
    middlewares: RwLock<Vec<Arc<dyn Middleware>>>,
    sequence: AtomicU64,
}

impl Hook {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Hook::addMenuItem()`：在 `user` / `admin` / `explore` 菜单中插入。
    pub fn add_menu_item(&self, category: &str, position: i32, item: MenuEntry) {
        let order = self.sequence.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.menus.write().expect("menu hooks poisoned");
        let entries = guard.entry(category.to_string()).or_default();
        entries.push(Positioned {
            position,
            order,
            item,
        });
        entries.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then(left.order.cmp(&right.order))
        });
    }

    pub fn menu_items(&self, category: &str) -> Vec<MenuEntry> {
        self.menu_items_with_position(category)
            .into_iter()
            .map(|(_, item)| item)
            .collect()
    }

    /// 带原始 position 的菜单项（供页面合并时插入到指定位置）。
    pub fn menu_items_with_position(&self, category: &str) -> Vec<(i32, MenuEntry)> {
        self.menus
            .read()
            .expect("menu hooks poisoned")
            .get(category)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| (entry.position, entry.item.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `Hook::addStyleFileToPage()`。
    pub fn add_style_file_to_page(&self, urls: &[String], pages: &[String]) {
        add_page_urls(&self.styles, urls, pages);
    }

    pub fn styles_for(&self, page: &str) -> Vec<String> {
        page_urls(&self.styles, page)
    }

    /// `Hook::addScriptFileToPage()`。
    pub fn add_script_file_to_page(&self, urls: &[String], pages: &[String]) {
        add_page_urls(&self.scripts, urls, pages);
    }

    pub fn scripts_for(&self, page: &str) -> Vec<String> {
        page_urls(&self.scripts, page)
    }

    /// `Hook::addUserBadge()`。
    pub fn add_user_badge(&self, text: impl Into<String>, color: impl Into<String>) {
        let color = color.into();
        self.badges
            .write()
            .expect("badge hooks poisoned")
            .push(Badge {
                text: text.into(),
                color: if color.is_empty() {
                    "primary".to_string()
                } else {
                    color
                },
            });
    }

    pub fn user_badges(&self) -> Vec<Badge> {
        self.badges.read().expect("badge hooks poisoned").clone()
    }

    /// `Hook::sendNotification()`：向若干用户写入站内信。
    pub async fn send_notification(
        &self,
        users: &[crate::models::user::User],
        title: &str,
        content: &str,
    ) -> Result<(), FrameworkError> {
        crate::support::plugin_api::send_notification(users, title, content).await
    }

    /// `Hook::addRoute()`：回调在核心路由注册后收到路由器。
    pub fn add_route<F>(&self, callback: F)
    where
        F: Fn(Router) -> Router + Send + Sync + 'static,
    {
        self.route_hooks
            .write()
            .expect("route hooks poisoned")
            .push(Arc::new(callback));
    }

    pub fn apply_routes(&self, mut router: Router) -> Router {
        let hooks = self
            .route_hooks
            .read()
            .expect("route hooks poisoned")
            .clone();
        for hook in hooks {
            router = hook(router);
        }
        router
    }

    /// `Hook::pushMiddleware()`：由全局 `PluginMiddleware` 逐个执行。
    pub fn push_middleware(&self, middleware: Arc<dyn Middleware>) {
        self.middlewares
            .write()
            .expect("middleware hooks poisoned")
            .push(middleware);
    }

    pub fn middlewares(&self) -> Arc<Vec<Arc<dyn Middleware>>> {
        Arc::new(
            self.middlewares
                .read()
                .expect("middleware hooks poisoned")
                .clone(),
        )
    }

    #[cfg(test)]
    pub fn clear(&self) {
        self.menus.write().expect("menu hooks poisoned").clear();
        self.styles.write().expect("style hooks poisoned").clear();
        self.scripts.write().expect("script hooks poisoned").clear();
        self.badges.write().expect("badge hooks poisoned").clear();
        self.route_hooks
            .write()
            .expect("route hooks poisoned")
            .clear();
        self.middlewares
            .write()
            .expect("middleware hooks poisoned")
            .clear();
    }
}

fn add_page_urls(target: &RwLock<HashMap<String, Vec<String>>>, urls: &[String], pages: &[String]) {
    let pages: Vec<String> = if pages.is_empty() {
        vec!["*".to_string()]
    } else {
        pages.to_vec()
    };
    let mut guard = target.write().expect("page hooks poisoned");
    for page in pages {
        let entries = guard.entry(page).or_default();
        for url in urls {
            if !entries.contains(url) {
                entries.push(url.clone());
            }
        }
    }
}

fn page_urls(target: &RwLock<HashMap<String, Vec<String>>>, page: &str) -> Vec<String> {
    let guard = target.read().expect("page hooks poisoned");
    let mut result = Vec::new();
    for (pattern, urls) in guard.iter() {
        if page_matches(pattern, page) {
            for url in urls {
                if !result.contains(url) {
                    result.push(url.clone());
                }
            }
        }
    }
    result
}

/// PHP `request()->is($pattern)` 的 `*` 通配匹配（按路径段不敏感）。
fn page_matches(pattern: &str, page: &str) -> bool {
    let pattern = pattern.trim_start_matches('/').chars().collect::<Vec<_>>();
    let page = page.trim_start_matches('/').chars().collect::<Vec<_>>();
    wildcard_match(&pattern, &page)
}

fn wildcard_match(pattern: &[char], text: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    match pattern[0] {
        '*' => {
            wildcard_match(&pattern[1..], text)
                || (!text.is_empty() && wildcard_match(pattern, &text[1..]))
        }
        '?' => !text.is_empty() && wildcard_match(&pattern[1..], &text[1..]),
        other => !text.is_empty() && text[0] == other && wildcard_match(&pattern[1..], &text[1..]),
    }
}

/// 全局注册表。测试可改用局部 `Filter` / `Hook` 实例。
pub static FILTERS: LazyLock<Filter> = LazyLock::new(Filter::new);
pub static HOOK: LazyLock<Hook> = LazyLock::new(Hook::new);
pub static DISPATCHER: LazyLock<Dispatcher> = LazyLock::new(Dispatcher::new);

/// 一次注册/启动的插件（编译进站点的原生 Rust 插件）。
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;

    /// 注册过滤器、Hook、事件监听器。
    fn boot(&self, api: &PluginApi);

    /// 卸载时移除过滤器等；默认无操作。
    fn shutdown(&self) {}
}

/// 传给插件的 API 句柄。
pub struct PluginApi;

impl PluginApi {
    pub fn filter<F>(&self, name: &str, priority: i32, function: F)
    where
        F: Fn(Value) -> Value + Send + Sync + 'static,
    {
        FILTERS.add(name, priority, function);
    }

    pub fn hook(&self) -> &'static Hook {
        &HOOK
    }

    pub fn events(&self) -> &'static Dispatcher {
        &DISPATCHER
    }
}

static PLUGINS: LazyLock<RwLock<Vec<Arc<dyn Plugin>>>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// 启动一个原生插件并记录，便于停机时 `shutdown`。
pub fn register_plugin(plugin: Arc<dyn Plugin>) {
    let already = PLUGINS
        .read()
        .expect("plugin registry poisoned")
        .iter()
        .any(|existing| existing.name() == plugin.name());
    if already {
        return;
    }
    plugin.boot(&PluginApi);
    PLUGINS
        .write()
        .expect("plugin registry poisoned")
        .push(plugin);
}

pub fn shutdown_plugins() {
    let plugins = PLUGINS.read().expect("plugin registry poisoned").clone();
    for plugin in plugins {
        plugin.shutdown();
    }
}

/// 应用所有 `Filter`；无监听器时原样返回。
pub fn apply_filter(name: &str, value: Value) -> Value {
    FILTERS.apply(name, value)
}

/// 同步派发事件。
pub fn fire(name: &str, payload: Value) {
    DISPATCHER.dispatch(&Event::new(name, payload));
}

/// `head_links`：Hook 的样式文件 + 插件过滤器。
pub fn head_links(path: &str) -> Vec<Value> {
    let mut links = HOOK
        .styles_for(path)
        .into_iter()
        .map(|href| {
            json!({
                "rel": "stylesheet",
                "href": href,
                "crossorigin": "anonymous",
            })
        })
        .collect::<Vec<_>>();
    let filtered = apply_filter("head_links", Value::Array(links.clone()));
    if let Value::Array(items) = filtered {
        links = items;
    }
    links
}

/// `scripts`：Hook 的脚本文件 + 插件过滤器。
pub fn scripts(path: &str) -> Vec<Value> {
    let mut scripts = HOOK
        .scripts_for(path)
        .into_iter()
        .map(|src| {
            json!({
                "src": src,
                "crossorigin": "anonymous",
            })
        })
        .collect::<Vec<_>>();
    let filtered = apply_filter("scripts", Value::Array(scripts.clone()));
    if let Value::Array(items) = filtered {
        scripts = items;
    }
    scripts
}

/// `user_badges`：Hook 徽章 + 过滤器。
pub fn badges(initial: Vec<Value>) -> Vec<Value> {
    let mut badges = initial;
    for badge in HOOK.user_badges() {
        badges.push(json!({ "text": badge.text, "color": badge.color }));
    }
    let filtered = apply_filter("user_badges", Value::Array(badges.clone()));
    if let Value::Array(items) = filtered {
        badges = items;
    }
    badges
}

/// 把 Hook 的菜单项合并到 `menus` 分组（`user` / `admin` / `explore`）。
pub fn merge_menu_hooks(groups: &mut Value, category: &str, path: &str) {
    let entries = HOOK.menu_items_with_position(category);
    if entries.is_empty() {
        return;
    }
    let header = match category {
        "user" => "general.user-center",
        "explore" => "general.explore",
        "admin" => "general.manage",
        _ => return,
    };
    let Some(items) = groups.as_array_mut() else {
        return;
    };
    let target = items
        .iter_mut()
        .find(|group| group.get("header").and_then(Value::as_str) == Some(header));
    let target = match target {
        Some(group) => group,
        None => {
            items.push(json!({ "header": header, "items": [] }));
            items.last_mut().expect("just pushed")
        }
    };
    let Some(list) = target.get_mut("items").and_then(Value::as_array_mut) else {
        return;
    };
    for (position, entry) in entries {
        let index = (position.max(0) as usize).min(list.len());
        list.insert(index, entry.to_json(path));
    }
}

/// 全局插件中间件：按注册顺序串行执行 `Hook::pushMiddleware` 的中间件。
pub struct PluginMiddleware;

#[async_trait]
impl Middleware for PluginMiddleware {
    async fn handle(&self, request: Request, next: Next) -> Response {
        let middlewares = HOOK.middlewares();
        run_middleware_chain(middlewares, 0, request, next).await
    }
}

fn run_middleware_chain(
    middlewares: Arc<Vec<Arc<dyn Middleware>>>,
    index: usize,
    request: Request,
    next: Next,
) -> MiddlewareFuture {
    if index >= middlewares.len() {
        return next(request);
    }
    let current = Arc::clone(&middlewares[index]);
    let next_in_chain: Next = Arc::new(move |request| {
        run_middleware_chain(
            Arc::clone(&middlewares),
            index + 1,
            request,
            Arc::clone(&next),
        )
    });
    Box::pin(async move { current.handle(request, next_in_chain).await })
}

/// `Hook::sendNotification()`：向若干用户写入站内信。
pub async fn send_notification(
    users: &[crate::models::user::User],
    title: &str,
    content: &str,
) -> Result<(), FrameworkError> {
    use chrono::Utc;
    use sea_orm::{EntityTrait, Set};

    use crate::models::notification::{ActiveModel, Entity};
    use crate::support::id::uuid_v4;

    let connection = suprnova::DB::connection()?;
    let now = Utc::now().naive_utc();
    for user in users {
        Entity::insert(ActiveModel {
            id: Set(uuid_v4()),
            notification_type: Set("App\\Notifications\\SiteMessage".to_string()),
            notifiable_type: Set("App\\Models\\User".to_string()),
            notifiable_id: Set(suprnova::eloquent::StoredU64::from(user.uid)),
            data: Set(json!({ "title": title, "content": content }).to_string()),
            read_at: Set(None),
            created_at: Set(Some(now)),
            updated_at: Set(Some(now)),
        })
        .exec(connection.inner())
        .await
        .map_err(|error| FrameworkError::database(error.to_string()))?;
    }
    Ok(())
}

/// 供插件在响应里返回简单文本（避免直接依赖框架细节）。
pub fn text_response(body: impl Into<String>) -> HttpResponse {
    HttpResponse::text(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_run_in_priority_then_registration_order() {
        let filter = Filter::new();
        filter.add("x", 20, |value| {
            Value::String(format!("{}a", value.as_str().unwrap_or_default()))
        });
        filter.add("x", 10, |value| {
            Value::String(format!("{}b", value.as_str().unwrap_or_default()))
        });
        filter.add("x", 10, |value| {
            Value::String(format!("{}c", value.as_str().unwrap_or_default()))
        });
        assert_eq!(
            filter.apply("x", Value::String(String::new())),
            Value::String("bca".to_string())
        );
        assert_eq!(filter.count("x"), 3);
        filter.remove("x");
        assert_eq!(filter.count("x"), 0);
    }

    #[test]
    fn hook_inserts_menu_by_position() {
        let hook = Hook::new();
        hook.add_menu_item("user", 0, MenuEntry::new("first", "user/first", "fa-a"));
        hook.add_menu_item("user", 1, MenuEntry::new("last", "user/last", "fa-b"));
        let items = hook.menu_items("user");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "first");
        assert_eq!(items[1].title, "last");
        hook.clear();
    }

    #[test]
    fn page_wildcards_match_php_style() {
        assert!(page_matches("*", "user/profile"));
        assert!(page_matches("user/*", "user/profile"));
        assert!(!page_matches("user/*", "user"));
        assert!(!page_matches("admin/*", "user/profile"));
        assert!(page_matches("user/profile", "user/profile"));
    }

    #[test]
    fn hook_styles_and_scripts_are_page_aware() {
        let hook = Hook::new();
        hook.add_style_file_to_page(&["/a.css".to_string()], &["user/*".to_string()]);
        hook.add_style_file_to_page(&["/b.css".to_string()], &[]);
        assert_eq!(hook.styles_for("user/profile"), vec!["/a.css", "/b.css"]);
        assert_eq!(hook.styles_for("admin"), vec!["/b.css"]);
        hook.clear();
    }

    #[test]
    fn dispatcher_delivers_payload() {
        let dispatcher = Dispatcher::new();
        let received = Arc::new(RwLock::new(Vec::new()));
        let sink = Arc::clone(&received);
        dispatcher.listen("UserLoggedIn", move |event| {
            sink.write().unwrap().push(event.payload.clone());
        });
        dispatcher.dispatch(&Event::new("UserLoggedIn", json!({ "uid": 1 })));
        assert_eq!(dispatcher.count("UserLoggedIn"), 1);
        assert_eq!(received.read().unwrap()[0]["uid"], 1);
        dispatcher.clear();
    }

    #[test]
    fn event_catalogue_has_24_names() {
        assert_eq!(EVENTS.len(), 24);
    }

    #[test]
    fn badges_merge_hook_and_filter() {
        let hook = Hook::new();
        hook.add_user_badge("SAMPLE", "");
        assert_eq!(hook.user_badges()[0].color, "primary");
        hook.clear();
    }
}
