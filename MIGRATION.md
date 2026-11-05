# NeoSkinRS — Rust / Suprnova 迁移计划

> **目标**：把 `blessing-skin-server-Laravel13`（6.0.11_patch）完整迁移到 Rust 1.94+ + Suprnova v4.6.2（Laravel 13 风格），保持 **HTTP 接口字节级兼容** 与 **数据库结构/数据完全兼容**。

---

## 1. 硬约束（不可破坏）

1. **HTTP 接口零变更**：173 条路由的路径、方法、请求参数名、请求体字段名、响应 JSON 结构必须与源完全一致。
2. **数据完全兼容**：17 张表 + 存量数据 + bcrypt 密码哈希 + SHA-256 纹理 hash。
3. **插件 API 契约**：Hook（7 方法）、PluginManager（13 方法）、Plugin、24 个 Event。
4. **patch 定制功能**：6.0.3 ~ 6.0.11_patch 全部保留。

---

## 2. 技术栈与版本基线

| 组件 | 选型 | 版本 | 说明 |
| --- | --- | --- | --- |
| 语言 | Rust | 1.94.0+ | Suprnova 要求 |
| 框架 | Suprnova | v4.6.2 | Laravel 13 风格，hyper/SeaORM/Tokio |
| ORM | SeaORM | 1.x | 模型宏 `#[model]`，11 种关系 |
| 数据库 | SQLite / MySQL / MariaDB / PostgreSQL / SQL Server | — | SeaORM 多数据库（与 Laravel config/database.php 一致） |
| 认证 | Suprnova Auth (sessions, guards, 2FA/TOTP) | — | 替代 Laravel Auth + Passport |
| 前端 | Inertia bridge (Svelte/React/Vue) + Vite 8 | — | Suprnova 内置 Inertia |
| 图像 | `image` crate | — | 替代 imagick |
| 队列 | Suprnova Queue | — | 替代 Laravel Queue |

---

## 3. 源系统盘点

| 维度 | 数量 |
| --- | --- |
| 数据表 | 17 |
| 路由 | 173 |
| 控制器 | 20 | +1 基类 `Controller` |
| 中间件 | 12 |
| Event | 24 |
| Listener | 9 |
| Service | 11 |
| Twig 模板 | 104 |（另有 5 个 Blade：master/mails） |
| JS/TS 源 | 131 |（含 tests 共 192） |
| 语言包 | 14 |
| 测试 | 67 |

（详见 NeoSkinGo/MIGRATION.md §3，数据相同）

---

## 4. 分层架构设计

```
NeoSkinRS/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── bootstrap/              # Suprnova bootstrap
│   ├── http/
│   │   ├── controllers/        # 20 个控制器
│   │   ├── middleware/         # 12 个中间件
│   │   └── kernel.rs
│   ├── models/                 # SeaORM entities
│   ├── events/                 # 24 个 Event
│   ├── listeners/              # 9 个 Listener
│   ├── services/               # 11 个 Service
│   ├── plugins/                # WASM/dlopen 插件
│   └── console/                # CLI 命令
├── config/
├── database/migrations/
├── routes/                     # web.rs, api.rs, static.rs
├── resources/
│   ├── views/                  # Tera/MiniJinja (或 Inertia SPA)
│   └── assets/                 # Vite 产物
├── storage/
└── tests/
```

---

## 5. 数据层迁移

### 5.1 SeaORM Entity 映射

SeaORM entity 定义（`#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]`），字段名与表列名保持一致：

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub uid: u32,
    pub email: String,
    pub nickname: String,
    pub score: i32,
    pub avatar: i32,
    pub password: String,
    pub ip: String,
    pub permission: i32,
    pub last_sign_at: DateTime,
    pub register_at: DateTime,
}
```

### 5.2 密码哈希兼容

- Laravel bcrypt `$2y$` → Rust `bcrypt` crate 的 `verify` 支持 `$2y$` 前缀（需确认，部分实现仅支持 `$2a$`/`$2b$`）。
- **风险**：Rust bcrypt 库可能不识别 `$2y$`。对策：迁移时将 `$2y$` 替换为 `$2a$`（bcrypt 的 `$2y$` 与 `$2a$` 算法相同，仅前缀不同），或在验证前做前缀归一化。

### 5.3 迁移策略

- 使用 SeaORM migration，SQL 与源 migration 保持一致（CREATE TABLE 语句不变）。
- 存量数据直接读取，不转换。

---

## 6. 接口层迁移

### 6.1 路由映射

Suprnova 使用 `routes!` 宏，路径 1:1：

```rust
routes! {
    GET "/" => HomeController::index,
    // auth, user, admin, ... 逐条移植
}
```

### 6.2 中间件

Suprnova 中间件链，12 个中间件逐一实现（Authenticate, CheckRole, RejectBannedUser, EnsureEmailFilled, ...）。

### 6.3 Minecraft 客户端契约

`GET /{player}.json` 和 `GET /csl/{player}.json` 返回 JSON 字段名必须一致。

---

## 7. 认证与授权

### 7.1 Session 认证

Suprnova 内置 session（cookie-based），替代 Laravel Session。

### 7.2 OAuth2 Server

源使用 Passport。Rust 侧需自建 OAuth2 server（或使用 `oauth2` crate + 自建授权端点），保持 15 条路由的请求/响应格式。

### 7.3 Scope 机制

与源一致：`User.Read`、`Player.Read/ReadWrite`、`Closet.Read/ReadWrite`、`UsersManagement.*`、`PlayersManagement.*`、`ClosetManagement.*`、`ReportsManagement.*`、`Notification.Read/ReadWrite`。

---

## 8. 插件系统

### 8.1 挑战

Rust 是静态编译语言，不支持运行时加载任意 Rust 代码。

### 8.2 方案选型

| 方案 | 优点 | 缺点 | 推荐 |
| --- | --- | --- | --- |
| WASM (wasmtime) | 跨平台，沙箱 | 生态不成熟，API 限制 | ⚠️ |
| 动态库 (libloading) | 原生性能 | 平台相关，ABI 稳定性 | ⚠️ |
| 独立进程 + gRPC | 完全隔离，语言无关 | 复杂度高 | ✅ 推荐 |
| 脚本引擎 (rhai/lua) | 轻量 | 需插件用脚本语言重写 | ⚠️ |

**推荐方案**：插件以独立进程运行，通过 gRPC 通信；或使用 WASM 沙箱执行插件逻辑。插件 manifest（`package.json`）格式保持不变。

### 8.3 Hook 事件总线

24 个 Event → Rust 的事件分发器（tokio channel + handler 注册表）。

---

## 9. 前端与视图层

### 9.0 决策：前端直接复用 blessing 项目（不自己重写）

**前端（`resources/assets/` + `public/app/`）不重写、不换技术栈、不做 SPA 改造**，
直接复用 `blessing-skin-server-Laravel13` 的既有前端：

| 资产 | 来源 | 处理方式 |
| --- | --- | --- |
| 前端源码 | `resources/assets/`（React 17 + TypeScript + webpack 5，131 个 JS/TS 文件） | 原样复用；构建沿用源仓库的 `package.json` / `webpack.config.ts` / `yarn.lock` |
| 预编译产物 | `public/app/`（`app.<hash>.js`、`style.<hash>.css`、`spectre.*`、`home-css.*`、`home.*`、字体/图片/wasm） | 原样复用到 `public/app/`，**不重新打包**（哈希值被 `resources/views/assets/*.twig` 写死引用） |
| 运行时语言包 | `/lang/<locale>.js`（由 `JavaScript` 服务生成） | 后端生成，内容与源一致 |

**关键约束 —— 服务端模板是「前端契约」，必须逐字保住 DOM 结构。**
预编译 bundle 靠选择器定位挂载点，改 HTML 结构等于改前端。以下都是硬契约：

- `window.blessing = {...}`（`shared.head`）与 `#blessing-extra` 的 JSON；
- `#logout-button`、`data-mark="nickname"`、`.locale`、`.main-button`、`.splash*`；
- auth 页的 `<main></main>`（React 挂载点）、`#blessing-extra` 的
  `{tooManyFails, recaptcha, invisible}`；
- `data-toggle="dropdown"` / `data-communication-*` / `data-system-update-form` 等
  行为钩子。

因此 **Twig 模板是移植的规格说明书**（spec），不是可以用自由风格重写的草稿。
服务端换模板引擎时，输出结构必须与 Twig 渲染结果一致。

---

### 9.1 两条路线

**路线 A**：使用 Suprnova 内置 Inertia bridge，前端用 Svelte/React/Vue，复用现有 131 个 JS/TS 源文件。

**路线 B**：使用 Tera/MiniJinja 模板引擎，104 个 Twig → Tera 模板转换。

> 按 9.0 的决策，**路线 A 的「复用现有前端」部分成立**（不重写前端），但
> 「改成 Inertia SPA」属于前端架构改造 —— 不在复用范围内，除非另行批准。
> 服务端仍须产出与 Twig 一致的 HTML 骨架（路线 B 的模板职责），否则预编译
> bundle 的挂载点/行为钩子会全部失效。

### 9.2 静态资源

`public/app/` 的预编译产物原样拷贝到 `public/app/`（不重新 Vite/webpack 打包），
由静态文件服务提供；`/lang/<locale>.js` 由后端 `JavaScript` 等价服务在运行时生成。

---

### 9.4 前后端分离：前端开发模式与运行时契约

前端可独立运行，复用它对后端提出了 5 条硬要求（缺一条线上白屏）：

1. **URL**：一律 `${blessing.base_url}${url}`，`base_url` 来自 `shared.head` 注入的
   `window.blessing`。
2. **凭证**：`credentials: 'same-origin'` → 会话 cookie 必须同源可带。
3. **请求头**：所有请求带 `Accept: application/json`；写请求带 `X-CSRF-TOKEN`，
   值取自 `meta[name="csrf-token"]`。⚠ 因此「校验失败」必须回 **422 JSON**
   （`{message, errors: {field: [..]}}`），**不能 302**。
4. **错误分支**：422 → 取 `errors[field][0]`；419 → CSRF 提示；403/400 → 弹
   `message`；其它非 2xx → 抛异常，若 body 含 `exception` + `trace` 会**渲染堆栈**。
5. **成功体**：`Content-Type: application/json` 才按 JSON 解析；业务层再看
   `{code, message, data}` 的 `code !== 0`。

**开发模式**：`yarn dev`（webpack-dev-server）默认 `0.0.0.0:8080`、`publicPath=/app/`、
`Access-Control-Allow-Origin: *`、HMR。dev 下产物名**无哈希**（`app.js`），
`resources/views/assets/*.twig` 会由 `HtmlWebpackPlugin` 重写成
`//localhost:8080/app/`；prod 才写 `{{ cdn_base }}/app/<hash>`。
⚠ `assets/*.twig` 是**构建产物**（`assets/.gitignore` = `*` + `!.gitignore`），
所以后端模板不能把哈希路径写死，否则 dev 模式不可用 —— 需要 debug 开关切换资源根。

**前端测试**：46 个 jest 用例（ts-jest + jsdom），`tests/setup.ts` 注入
`window.blessing` 与 `i18n`（读 `resources/lang/en/front-end.yml`），各用例自行
mock `fetch`。这些 mock 断言了 URL/方法/body/响应形状，等价于一份可执行的接口契约。

**路由表生成**：`yarn build:urls` 执行
`php artisan route:list --json --columns=uri,name` 生成
`resources/assets/src/scripts/urls.ts`（已提交，复用前端时无需重生成）。若需重生成，
后端须提供等价的 `route:list --json --columns=uri,name` 输出。

## 10. 图像处理

- `image` crate 替代 imagick。
- avatar 端点返回 2D PNG。
- 纹理预览/缩略图。

---

## 11. 国际化

- 14 种语言包 → JSON/YAML 翻译文件。
- `language_lines` 表保持。

---

## 12. 分阶段实施计划

### Phase 0：项目骨架（1-2 天）
- [ ] `suprnova new` 初始化项目
- [ ] 配置数据库连接
- [ ] 移植 migration
- [ ] 验证 `cargo build`

### Phase 1：数据层 + 模型（3-5 天）
- [ ] SeaORM entity 定义（17 个）
- [ ] 密码哈希兼容（bcrypt `$2y$` → `$2a$` 归一化）
- [ ] 纹理 hash 算法
- [ ] 单元测试

### Phase 2：认证与授权（3-5 天）
- [ ] Session 认证
- [ ] OAuth2 Server（Passport 兼容）
- [ ] 中间件链（13 个）
- [ ] Scope 机制

### Phase 3：核心业务控制器（7-10 天）
- [ ] AuthController, UserController, PlayerController, ClosetController
- [ ] TextureController, SkinlibController, ReportController
- [ ] ClosetManagement, PlayersManagement, UsersManagement

### Phase 4：管理面板（5-7 天）
- [ ] AdminController, CommunicationsController, NotificationsController
- [ ] OptionsController, TranslationsController, PluginController
- [ ] UpdateController, MarketController

### Phase 5：安装向导（2-3 天）
- [ ] SetupController, CheckInstallation

### Phase 6：前端（5-7 天）
- [ ] Inertia SPA 集成（或 Twig → Tera 转换）
- [ ] 静态资源服务

### Phase 7：插件系统（5-7 天）
- [ ] 插件 manifest 解析
- [ ] 事件总线
- [ ] 插件管理

### Phase 8：测试与验证（3-5 天）
- [ ] 67 个测试用例移植
- [ ] HTTP 契约测试
- [ ] 数据兼容性测试

---

## 13. 风险与对策

| 风险 | 影响 | 对策 |
| --- | --- | --- |
| bcrypt `$2y$` 兼容 | 高 | 前缀归一化 `$2y$` → `$2a$` |
| 插件动态加载 | 高 | 进程隔离 + gRPC |
| OAuth2 Passport 兼容 | 高 | 自建端点，严格按 Passport 格式 |
| Inertia 前端迁移 | 中 | 复用现有 React 源文件 |
| Minecraft 客户端兼容 | 高 | 严格对照响应格式 |

---

## 14. 验证清单

```sh
cargo build
cargo test
cargo clippy
```

- [ ] `cargo build` 无错误
- [ ] 所有 173 条路由注册成功
- [ ] 数据库 migration 执行无错误
- [ ] 存量数据可正常读取
- [ ] bcrypt 密码验证通过
- [ ] OAuth2 端点与 Passport 响应一致
- [ ] Minecraft 客户端可正常获取皮肤/披风
- [ ] 67 个测试用例全部通过
