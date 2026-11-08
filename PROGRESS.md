# NeoSkinRS — 进度快照

> 每次会话结束更新本文件，并在回复末尾给出「进度」一行。
> 最后更新：2026-11-06。
> 目标：Rust 1.94+ / Suprnova v4.6.2 迁移 `blessing-skin-server-Laravel13`（6.0.11_patch）。

## 1. 总览

| 指标 | 数值 |
| --- | --- |
| 源路由（PHP 实际） | 174 |
| 已实现路由 | 173 条声明 / 展开 227 个「方法×路径」，与 PHP 的 227 对齐（ignition 调试路由除外） |
| 数据库表 | 17 / 17（`options` 启动时自动写入默认值） |
| 阶段 | Phase 2/3 完成，Phase 4/5 页面完成，Phase 6 约 85% |

## 2. 阶段进度

| 阶段 | 进度 | 说明 |
| --- | --- | --- |
| Phase 0 项目骨架 | 100% | cargo build/migrate/serve 通过，`/__neoskin/health` 200；Windows 16 MiB 栈入口修复启动期 stack overflow |
| Phase 1 数据层 + 模型 | 95% | 16 表迁移 + 12 个模型模块（覆盖 16 表）、bcrypt/SHA-256；迁移对 PHP 存量库幂等（表/索引 `IF NOT EXISTS`）；已用 PHP SQLite 库副本验证登录与页面 |
| Phase 2 认证与授权 | 100% | Session/记住我/验证码；OAuth 管理、个人令牌、Bearer、授权码 + refresh 交换、authorize/approve/deny 全部 HTTP 验证；忘记密码/重置/邮箱验证已接通 |
| Phase 3 核心业务控制器 | 100% | 业务 API、后台管理、邮件相关流程（依赖 SMTP 配置）全部实现 |
| Phase 4 管理面板 | 98% | 14 个页面 + `/admin/chart` 数据接口 + 插件/更新端点；插件系统本体仍为 0% |
| Phase 5 安装向导 | 100% | `setup` 欢迎/数据库/信息/完成四页、安装锁、超管创建已端到端验证；`InstallMiddleware` 在版本落后时请求期执行 `update` |
| Phase 6 前端/视图 | 87% | 静态资源、`/lang`、MiniJinja 引擎与 50+ 模板；home/auth/user/skinlib/admin/setup/errors 页面已迁移并运行时验证；`?3d` 头像已补齐（与 Go 端口像素级一致） |
| Phase 7 插件系统 | 85% | 清单扫描、启停/删除、依赖冲突、README、市场；原生 Rust API（Hook 7 / Filter / 24 Event / Plugin trait）与动态库 ABI v1（`native` 清单字段、描述 JSON、filter/event 回调）已实现；不执行 PHP 插件，动态库无沙箱/卸载 |
| Phase 8 测试与验证 | 88% | `cargo test` 96 unit + 4 integration（含 3D 头像与 Go 对拍、升级版本比较/幂等 procedure、通信 HTML 清洗、更新包暂存/安装、dynamic plugin ABI、MySQL/MariaDB/PostgreSQL 矩阵与无符号 DDL 断言）；SQLite/MySQL 登录、页面、衣柜、OAuth 客户端冒烟通过 |


## 3. 环境与依赖

- Rust：**1.94.0**（`rustup default 1.94.0`，经 rsproxy 镜像安装）。
- Cargo registry：rsproxy（`~/.cargo/config.toml`）。
- Suprnova：GitHub `eas4ai/suprnova` tag `v4.6.2`（**不在 crates.io**）。
  - CLI 安装：`cargo install --path suprnova-cli --locked`（本地克隆）。
  - 应用依赖：`suprnova = { git = "https://github.com/eas4ai/suprnova.git", tag = "v4.6.2", ... }`。
- 网络：GitHub 需代理 `http://127.0.0.1:10808`；crates 走 rsproxy。
- OpenSSL（Windows）：Suprnova 强制依赖 `webauthn-rs` → `openssl-sys`；
  使用便携 zip 解压到 `D:\WORKspace\NeoSkin\tools\openssl-3.5.6`，
  构建设置 `OPENSSL_DIR`，运行把 `x64\bin` 加入 `PATH`。
  已封装为 `scripts/build.ps1` / `scripts/serve.ps1`。
- Windows 符号链接：`framework/templates/suprnova-ui` 在 Git 中是符号链接，
  Windows 检出为文本文件会让 Askama 编译失败；`scripts/build.ps1` 自动创建
  目录 junction 修复。

## 4. 已知偏差 / 待确认

1. **SQL Server 不支持**：Suprnova / SeaORM 仅提供 SQLite / PostgreSQL /
   MySQL / MariaDB 驱动（`framework/Cargo.toml` 无 `database-sqlserver`），
   而 AGENTS.md 要求五库矩阵。Rust 端只能覆盖四库，需确认是否接受。
2. 前端：AGENTS.md 要求复用 legacy `public/app` + Twig 契约；Suprnova 自带
   Inertia bridge，但本项目不使用 Inertia SPA（避免改造前端）。
3. 框架 Windows 构建需要 OpenSSL 便携版与符号链接 junction 修复（见 §5）。
4. 认证 API 消息仍硬编码中文；模板已通过 `trans()` 读取 `resources/lang`，
   但后台/业务接口的提示尚未全部本地化。
5. `options` 表尚无 PHP `import_options` 的初始数据。
6. 会话存储使用 Suprnova 的 `sessions` 表（额外表，不影响 17 张业务表）；
   cookie 名已设为 `blessing_skin_session`。
7. **视图层为 MiniJinja**（Twig→Jinja 转换），不是原 Twig 引擎；
   已迁移 40+ 模板并保持 DOM 契约；Twig 的 `{% for k, v in map %}` 需写
   `map|items`，`|raw` 改为 `|safe`，`include` 改为 `{% include %}`。
8. **构建缓存**：`target/` 已重定向到
   `C:\Users\Knigh\AppData\Local\Temp\opencode\neoskin-target`。
9. **表单无方法伪造**：Suprnova 不识别 `_method`，通信编辑表单使用
   `POST /admin/communications/{id}`（另注册 `admin.communications.update.post`）。
10. **Twig 转义**：MiniJinja 默认把 `/` 转成 `&#x2f;`，已安装自定义 formatter
    仅转义 `& < > " '`，与 Twig/PHP 输出一致。
11. **安装锁与请求期升级**：`storage/install.lock` 缺失时，若库里已有用户则启动
    自动补锁；全新库则由 `POST /setup/finish` 写入。未安装时页面请求 302 `/setup`。
    已安装且 `options.version` 落后时，`InstallMiddleware` 在请求期执行
    `support::upgrade::run_if_needed()`（5.0.0 背景图迁移 + 版本号 + migrate +
    `view:clear` + 补锁 + 清缓存），并发由异步互斥锁串行化，失败下个请求重试。
12. **CLI**：`src/commands/maintenance.rs` 提供 `update`、`bs:install`、
    `options:cache`、`plugin:enable|disable`、`salt:random`，由 Suprnova
    `inventory` 自动注册到 `console` 二进制。
13. **OAuth grant**：`POST /oauth/token` 支持 `authorization_code`、`refresh_token`
    与 `client_credentials`；三种 grant 均接受 HTTP Basic 或表单客户端凭据。
    `password` grant 按 PHP 默认保持关闭。
14. **自动更新**：`GET /admin/update` 真实更新检测（spec=2、runtime/platform、
    semver）；`download` / `upload` 校验并暂存 ZIP，`install` 把 `payload/`
    写入站点根目录（运行中的二进制写为 `*.new` 并提示重启）。运行包格式见
    `support/update.rs`；PHP patch 包不会被当作 Rust 运行包。
13. **邮件**：复用 Suprnova `Mail`（`MAIL_DRIVER`）；`smtp` 需要可用的 SMTP
    服务，本地可用 `MAIL_DRIVER=log` 验证。签名链接使用 APP_KEY 的
    HMAC-SHA256，与 Laravel 算法相同；若 PHP 与 Rust 的 `APP_KEY` 不同，
    旧邮件中的链接不能跨端口复用。
14. **OAuth**：`/api/*` 按 Passport 路由声明校验 scope；`/api/*` 不启用 CSRF
    （与 Laravel `api` 中间件组一致），Session 登录仍然可用。源项目
    `Closet.Read` 的读写 scope 拼写为 `Closet.ReadWrtie`，Rust 端同时接受
    `Closet.ReadWrite` 以兼容存量令牌。
16. **插件系统**：按根目录 `DECISIONS.md` **D-001（方案 A）**，Rust 端实现
    `package.json` 清单层、市场下载/上传/wget，并提供原生插件 API
    （`support/plugin_api.rs`：Hook 7 方法、Filter、24 个 Event、`Plugin` trait）；
    **不执行 PHP 插件代码**，现有插件需改写为原生插件。插件代码编译进二进制，
    暂不加载动态库。
17. **路由形状**：`/{player}.json`、`/csl/{player}.json` 由
    `/{player}`、`/csl/{player}` 在处理器内识别 `.json` 后缀（matchit 不支持
    「参数 + 字面后缀」）；PHP 的 `_ignition/*` 调试路由不移植。
18. **存量库迁移**：Rust 迁移使用 `CREATE TABLE IF NOT EXISTS` + `has_index` 先查再建，
    可直接接管 Laravel 迁移创建的 SQLite/MySQL 库；Rust 的 `seaql_migrations`
    与 Laravel 的 `migrations` 表并存，互不覆盖。已用 PHP 数据库副本 +
    复制用户验证 `$2y$` 登录成功。
19. **MySQL 无符号主键（已解决）**：PHP `increments()` 在 MySQL/MariaDB 生成
    `INT UNSIGNED`，`bigIncrements()` / `unsignedBigInteger()` 生成
    `BIGINT UNSIGNED`。Rust 端按列语义使用 `u32`（普通自增 ID）、`u64`
    （大 ID / OAuth / communications）与 `StoredU64`（`notifications.notifiable_id`），
    SeaORM 2 的 `TryGetable for u32` / `u64` 可跨库解码；迁移按后端分支：
    MySQL/MariaDB 建无符号列，PostgreSQL/SQLite 建有符号列。`user_closet`、
    `players.uid`、`textures.uploader`、`reports.*` 等 PHP `integer()` 列保持
    `i32`，跨类型访问显式转换。详见 `PORT-NOTES.md`。

## 5. 已完成功能

### 2026-11-06（后端补齐：请求期升级 + CLI + client_credentials）

- `src/support/upgrade.rs`：PHP `UpdateCommand` 的共享实现（5.0.0 背景图迁移、
  版本号、迁移、清视图缓存、补 `install.lock`、清 Cache），并发串行化；
  `InstallMiddleware` 在 `options.version` 落后时请求期调用。
- `src/commands/maintenance.rs`：`update`、`bs:install`、`options:cache`、
  `plugin:enable|disable`、`salt:random`；`console --help` 已列出。
- `POST /oauth/token`：新增 `client_credentials` grant 与 HTTP Basic 客户端认证。
- `support/communication.rs`：用 `ammonia` 补齐 PHP `CommunicationRenderer::sanitize`
  的标签/属性/URL 白名单清洗；新增 script/onerror/javascript: 单测。
- `support/skin3d.rs`：移植完整 3D 头像渲染器（等距投影、扫描线填充、
  Alex/旧皮肤、双层帽子）；`?3d` 不再回退 2D。与 Go 端口对同一合成皮肤
  做像素级对比：ratio=25 输出 318×338、45×45 均 0 差异。单测 96 全绿。
- 单元测试新增版本比较 / procedure 幂等 / CLI options 缓存导出；`cargo fmt --check`、
  `clippy -D warnings`、`cargo test` 全部通过。

- 项目骨架：`cargo build`、`cargo clippy -- -D warnings`、健康检查。
- 16 张业务表迁移（SQLite 已验证，MySQL/PostgreSQL 驱动已启用）。
- 模型：users/players/textures/options/user_closet/reports/notifications/
  jobs/language_lines/scopes/oauth_*(5)/communications。
- bcrypt `$2y$` 归一化与生成、SHA-256（单元测试 3 项）。
- 认证：`POST /auth/login`（邮箱或角色名 + bcrypt）、`POST /auth/logout`、
  `GET /api/user`；CSRF（XSRF-TOKEN / X-XSRF-TOKEN）、登录后 Session ID 轮换。
- 记住我：`remember_web_59ba36…` cookie（`uid|token|hmac`），无 Session 时恢复
  登录态；篡改拒绝；登出轮换 `remember_token` 并清除 cookie。
- 登录安全：同一 IP 失败超过 3 次后要求验证码（Cache 3600 秒）；验证码支持
  本地 SVG（session 短语）与 reCAPTCHA（`recaptcha_secretkey`）；密码长度
  6–32 与 PHP 规则一致。
- OAuth2 管理：`/oauth/scopes`、clients CRUD（bcrypt secret + 一次性
  plain_secret）、tokens 列表/撤销、personal access tokens（自动创建
  personal client）、RS256 JWT 签发。
- Bearer 鉴权：`/api/user` 支持 `Authorization: Bearer <JWT>`（校验签名、
  撤销、过期），Session/记住我优先。
- OAuth2 授权码流程：`GET /oauth/authorize`（校验 client/redirect/
  response_type/scope，写 session，渲染授权页）、`POST /oauth/authorize`
  （CSRF + auth_token 校验，写 `oauth_auth_codes`，302 回跳带 code/state）、
  `DELETE /oauth/authorize`（回跳 `error=access_denied`）、
  `POST /oauth/token`（authorization_code / refresh_token；token 端点按
  Passport 约定排除 CSRF）、`POST /oauth/token/refresh`（transient token）。
  已验证：批准 → code → token → Bearer、code/refresh 一次性消费、
  错误 secret 401、刷新轮换、拒绝授权回跳。
- 玩家角色 API：`GET/POST /api/players`、`DELETE /api/players/{player}`、
  `PUT /api/players/{player}/name`、`PUT/DELETE /api/players/{player}/textures`。
  校验（`official/cjk/utf8/custom` 规则、长度、唯一性）、扣分/退分、
  所有权 403、材质归属衣柜校验均已端到端验证。
- 衣柜 API：`GET /api/closet`（Laravel 分页结构）、`GET /api/closet/ids`、
  `POST /api/closet`、`PUT/DELETE /api/closet/{tid}`。私有材质、重复收藏、
  积分与 `likes` 计数、`score_award_per_like` 对齐 PHP。
- 纹理服务：`GET /{player}.json`、`GET /csl/{player}.json`（CustomSkinAPI，
  含封禁 403/Last-Modified）、`GET /textures/{hash}`、`GET /csl/textures/{hash}`、
  `GET /raw/{tid}`（`allow_downloading_texture` 开关）。
- 头像与预览：`GET /avatar/{tid|hash|player|user}`、`GET /preview/{tid|hash}`
  （2D 头部合成、皮肤正反面拼接、披风预览；默认头像内嵌 `resources/textures`）。
- 站内信：`GET /api/user/notifications`、`POST /api/user/notifications/{id}`；
  通信可见性（`published` + `message|both` + `delivery_token`）、
  标记已读、Markdown/块渲染。
- 注册：`POST /auth/register`（验证码、邮箱唯一、密码 8–32、角色名规则、
  `regs_per_ip`、初始积分、自动创建角色并登录）。
- 用户资料与积分：`GET /user/score-info`、`POST /user/sign`（签到间隔/
  跨日模式）、`POST /user/profile`（昵称/密码/邮箱/删除账号）、
  `POST /user/profile/avatar`（重置/披风拒绝/私有材质）、
  `PUT /user/dark-mode`。
- Web 端 JSON 路由：`/user/player/*`、`/user/closet/*`、`/user/notifications/{id}`
  复用 API 处理器，供 legacy 前端直接调用。
- 皮肤库：`GET /skinlib/list`（Laravel 分页 + 类型/关键词/上传者/排序筛选）、
  `GET /texture/{id}`、`GET /skinlib/info/{id}`（私有材质 403/404 可配置）、
  `POST /texture`（multipart，PNG 校验/重编码/去重/积分/自动进衣柜）、
  `PUT /texture/{id}/name|type|privacy`、`DELETE /texture/{id}`。
- 举报：`POST /skinlib/report`（重复举报、举报者扣分）、
  `GET /admin/reports/list`、`PUT /admin/reports/{id}`（reject/delete/ban、
  返还与奖励积分），并注册 `/api/admin/reports` 别名。
- 后台管理：
  - 用户：`GET /admin/users/list`、`PUT .../{user}/email|verification|nickname|
    password|score|permission`、`DELETE /admin/users/{user}`（删除用户时先删角色）。
  - 角色：`GET /admin/players/list`、`PUT .../{player}/name|owner|textures`、
    `DELETE /admin/players/{player}`。
  - 衣柜：`GET/POST/DELETE /api/admin/closet/{user}`、`POST/DELETE
    /admin/closet/{user}`。
  - 通知：`POST /admin/notifications/send`、`POST /api/admin/notifications`
    （创建 communication + 批量写入 notifications，302 回 `/admin`）。
  - 全部 `/api/admin/*` 别名已注册；后台守卫（未登录 401 / 非管理员 403）
    已端到端验证。
- 翻译覆盖：`GET /admin/i18n/list`、`POST /admin/i18n`、
  `PUT/DELETE /admin/i18n/{line}`（`text` 按 locale 存 JSON；locale 取自
  `Accept-Language`，默认 `zh_CN`）。
- 站点选项：`POST /admin/customize|score|options|resource` 写入 `options`
  （含 `site_url`/`cdn_address` 归一化、签到区间、本地化 `site_name_*`）。
- 通信管理：`POST /admin/communications`（草稿/发布/公告）、
  `PUT /admin/communications/{id}`（已发布时保持原受众并同步通知）、
  `POST .../withdraw`、`POST .../preview`（返回 `{html}`）、
  `POST .../import`（Markdown → blocks 的简化实现）。
- 站点入口与静态资源：`GET /api`（元信息）、`GET /.well-known/change-password`、
  `public/` 静态文件（catch-all，含指纹缓存）、
  `/lang/<locale>.js`（启动时按 `front-end.yml` + `language_lines` 生成）。
- 视图层（Phase 6）：
  - MiniJinja 2.24 引擎 + `trans()`/`url()`/`csrf_field()` 全局函数、
    `nl2br`/`tojson`/`json_encode` 过滤器，自定义 Twig 转义。
  - 已迁移 `home`、`auth/*`、`shared/*`、`assets/*`、`errors/403|404`、
    `user/base|index|player|closet|profile|report|inbox|message|oauth`、
    `skinlib/base|index|show|upload` 及 widgets、`admin/*` 全部页面与 widgets。
  - `/`、`/auth/login|register|forgot|reset|bind|verify`、`/user`、
    `/user/player|closet|profile|reports|inbox|inbox/{id}|oauth/manage`、
    `/skinlib`、`/skinlib/show/{id}`、`/skinlib/upload` 均 200。
  - 后台 `/admin`、`/admin/users|players|reports|i18n|customize|score|options|
    resource|status|communications|update|plugins/manage|plugins/market` 全部 200，
    `?new=1` / `?announcement=1` 编辑态可渲染。
  - `require_page_user` 对未登录 HTML 页面 302 到 `/auth/login`（与 PHP 一致）。
- 后台表单：`support/form.rs` 复刻 PHP `OptionForm`（text/checkbox/textarea/
  select/group/hint/description/placeholder/button），页面直接输出 HTML。
- 站内信页面：`GET /user/inbox`、`GET /user/inbox/{id}`（可见性、
  `delivery_token` 校验、已读标记、Markdown/块渲染、分页）。
- 静态资源：`support/static_middleware.rs` 处理 `/app/*`、`/lang/*`、
  `/plugins/*`，注册在 Session 中间件之前；根级 `GET /{player}` 先查
  `public/<name>`，再转 `texture::player_json_by_name`，最后 404。
- 语言解析修复：`Accept-Language` 先完整标签匹配（`zh-TW` → `zh_TW`），
  再回退语言前缀，避免 `zh-TW` 被误判为 `zh_CN`。
- 安装向导（Phase 5）：
  - `GET /setup`（欢迎）、`GET /setup/info`（管理员信息）、
    `GET|POST /setup/database`（数据库配置，可写 `.env`）、
    `POST /setup/finish`（创建超级管理员、写 `install.lock`）。
  - `storage/install.lock` 存在时 setup 页显示锁定页；
    未安装时 `InstallMiddleware` 把页面请求 302 到 `/setup`。
  - 已用临时 SQLite 库端到端验证：安装 → 主页 200 → 新管理员可登录。
- 邮件流程（Phase 2/3 收尾）：
  - `GET /auth/forgot`（未配置邮件时显示 403）、`POST /auth/forgot`
    （验证码、同 IP 180 秒限流、签名重置链接、HTML 邮件）。
  - `GET /auth/reset/{uid}` / `POST /auth/reset/{uid}`：校验 1 小时签名，
    密码 8–32 位，更新 bcrypt 哈希。
  - `GET /auth/verify/{user}` / `POST /auth/verify/{user}`：无过期签名、
    邮箱匹配、写 `verified`；成功后 302 `/user`。
  - `POST /user/email-verification`：60 秒限流、已验证拒绝、重发验证邮件。
  - 签名实现 `support/signed_url.rs`（HMAC-SHA256 + `expires`，常量时间比较），
    邮件发送 `support/mail.rs`（复用 Suprnova `Mail`）。
  - 端到端验证（`MAIL_DRIVER=log`）：重置密码并重新登录、限流 code 2、
    验证邮件 → 验证页 → `verified=1`。
- OAuth2 补充（Passport 行为）：
  - `/oauth/token` 支持 `Authorization: Basic base64(client_id:client_secret)`，
    同时也接受表单里的 `client_id` / `client_secret`。
  - `prompt=login`：强制登出并跳转登录，登录后通过 `last_requested_path`
    回到原授权请求；`prompt=consent` 强制显示授权页；`prompt=none` 在未登录
    或未授权时分别回跳 `login_required` / `consent_required`。
  - 已授权相同 scope 且未要求 consent 时自动下发授权码。
  - `/api/user|players|closet|admin/*` 按 PHP 路由声明校验 scope
    （Session 登录不受 scope 限制）；无效 scope 返回 403
    `{"message":"Invalid scope(s) provided."}`。
  - `/api/*` 豁免 CSRF，Bearer 客户端可直接 POST/PUT/DELETE。
- 方法级路由对齐：
  - `/api`、`/auth/captcha`、`/.well-known/change-password`、
    `/admin/{customize,score,options,resource}` 改为 `any!`，与 PHP
    `Route::any` 一致；后台选项路径按方法分派（POST 保存，其余渲染页面）。
  - 路由差异审计（PHP `routes.json` vs `src/routes.rs`）：227 个
    「方法×路径」全部覆盖；仅 `_ignition/*` 调试路由与 `.json` 后缀的
    形状差异保留（后者在处理器内等价实现）。
- 插件清单层（Phase 7 起步）：
  - 扫描 `PLUGINS_DIR` / `NEOSKIN_PLUGINS_DIR`（逗号分隔，默认 `plugins/`）
    下的 `package.json`，解析名称/版本/标题/描述/require/conflicts/icon。
  - `GET /admin/plugins/data` 返回与 PHP 同结构的插件数组（含 `enabled`、
    `readme`、`config`、`icon`）。
  - `POST /admin/plugins/manage` 支持 `enable` / `disable` / `delete`，
    启用前做 semver 依赖与冲突检查；状态持久化到 `plugins_enabled`。
  - `GET /admin/plugins/readme/{name}` 渲染 Markdown README。
  - 插件静态资源继续由 `/plugins/*` 中间件提供；`Plugin::assets` 等价
    辅助函数已实现。
  - 插件市场：`GET /admin/plugins/market/list` 拉取注册中心并补充
    `installed` / `can_update` / `dependencies`；
    `POST .../market/download`、`POST .../upload`、`POST .../wget`
    下载或解压 zip 到插件目录，带 zip-slip 防护。
- 更新检测（Phase 4/8）：
  - `GET /admin/update` 读取 `update_source` 选项 / `UPDATE_SOURCE` /
    默认地址，3 秒连接超时、8 秒总超时。
  - 校验 `spec == 2`，识别 `latest` / `php` / `runtime` / `platform` / `url` /
    `package` / `sha256`；非 `neoskin-rs` 源明确提示不适用。
  - PHP 版本按 `8.3.0` 参与比较；`_patch` 等后缀会归一化为数字版本。
  - 页面展示最新版本、当前版本、更新可用/已是最新/连接失败。
  - 运行包：`manifest.json`（runtime/version/platform）+ `payload/`；
    `download`/`upload` 暂存并校验 SHA-256，`install` 写入站点根目录；
    路径穿越、`.env`/`storage/`/`plugins/`/数据库文件被拒绝。
- `options` 初始数据：启动时按 PHP `import_options` 写入缺失的默认项，
  `{version}` / `{year}` 已替换，补上 `max_texture_width=8192`。
- 后台数据接口：`GET /admin/chart`（最近 31 天注册/上传按星期聚合，
  与 PHP `chartData` 的 xAxis/data 结构一致）。
- 插件/更新端点：`/admin/plugins/{data,manage,readme,config,upload,wget,
  market/list,market/download}` 与 `/admin/update/{download,install,upload}`
  均已实现；插件清单/原生 API/动态库、运行包暂存与安装见上文。
- 存量库兼容验证：
  - 迁移的表与索引均使用 `IF NOT EXISTS`，可接管 Laravel 已建表的 SQLite 库。
  - 以 PHP `database/database.sqlite` 副本启动：迁移通过，`/`、`/skinlib`、
    `/auth/login`、`/setup` 均 200。
  - 将现有 `$2y$` 用户复制进该副本后，`POST /auth/login` 成功，随后
    `/user`、`/admin`、`/admin/options`、`/skinlib/upload` 均 200。
- 数据库矩阵测试（Phase 8）：
  - 新增 `tests/db_matrix.rs`，由 `DB_CONNECTION` 门控，默认跳过。
  - 覆盖幂等迁移、用户 CRUD、`register_at` 的 `Y-m-d H:i:s` 格式、
    纹理插入/计数、随机清理；不 DropAll。
  - 2026-11-05 本机结果：MySQL 26.10（3307）、MariaDB 13.0.2（3308）、
    PostgreSQL 18.6（5433）均通过；MySQL/MariaDB 额外断言
    `users.uid`/`textures.tid` 等为 `INT UNSIGNED`，`notifications.notifiable_id`、
    `jobs.id`、`oauth_*` 为 `BIGINT UNSIGNED`，`user_closet.*` 保持有符号 `INT`。
  - MySQL 索引创建改为 `has_index` 先查再建（MySQL 不支持
    `CREATE INDEX IF NOT EXISTS`）。
  - 无符号 ID 迁移：模型按 PHP 列语义使用 `u32`/`u64`/`StoredU64`；
    迁移按后端生成无符号（MySQL）或有符号（PostgreSQL/SQLite）列。
- 测试覆盖：67 项单元测试，覆盖
  - 密码哈希（`$2y$` 归一化、随机盐、SHA-256）；
  - 记住我 cookie 解析；
  - 签名链接（过期/篡改/无过期/缺签名）；
  - `Accept-Language`（完整标签、前缀、未知语言）；
  - SVG 验证码、PNG 编码/解码、图片裁剪；
  - `OptionForm` 渲染（text/checkbox/textarea/select/group/form）；
  - 通信 Markdown/块渲染/HTML 转义；
  - 插件清单、semver、zip 解压与 zip-slip；
  - 更新版本比较与 `_patch` 后缀；
  - 角色名规则、PHP 整数转换、UUID、i18n、语言包路径等。
- 存储层：`support/storage.rs`（`TEXTURES_DIR` / `NEOSKIN_STORAGE_ROOT`，
  hash 白名单防路径穿越）。

## 6. 下一步

1. 插件运行时：原生 Hook / Filter / 24 Event / `Plugin` trait / 动态库 ABI v1
   已落地；后续补沙箱/WASM、插件重写指南与示例生态。
2. 插件市场：依赖求解（`dependencies.unsatisfied`）、前端页面联调。
3. 自动更新：运行包格式与 download/upload/install 已实现；后续补发布流水线、
   二进制重启脚本与回滚。
4. 数据库矩阵：`tests/db_matrix.rs` 已覆盖 MySQL/MariaDB/PostgreSQL，并在
   MySQL/MariaDB 上断言 PHP 无符号列类型；剩余 SQL Server 范围待框架支持。
5. PKCE：源项目 `oauth_auth_codes` 表没有 `code_challenge` 列，暂不实现。
6. 端口偏差登记到 `PORT-NOTES.md`，并更新 `AGENTS.md` 的数据库范围/插件范围。
7. **Rust 完成后**执行根目录 `CROSS-PROJECT-SYNC.md`：同步 Go/Rust/.NET/Spring
   四个项目的状态、文档与实现覆盖度（2026-11-05 已完成一次审计，见该文件 §7）。

---

## 8. 共享前端 P1（2026-11-06）

- 新增 `GET /api/bootstrap`（契约 v1）与 SPA 壳 `GET /app-next/{site,admin}`、`/app-next/{site,admin}/`、`/app-next/{site,admin}/{*path}`。
- 壳注入 `<meta name="csrf-token">`、`<meta name="neoskin-mode">` 与 `#neoskin-bootstrap`；`</script>` / `<` / `>` / `&` 已转义。
- 静态资源命中 `public/app-next/{mode}` 时设置一年 immutable 缓存；缺失的带扩展名文件返回 404。
- 实现：`src/controllers/spa.rs`、`src/routes.rs`、`src/controllers/mod.rs`。
- 构建产物已发布到 `public/app-next/{site,admin}`（含 `manifest.json`）。
- 联调验证：登录 → `/app-next/site/user` → `/app-next/admin/` 仪表盘，0 console error。

---

## 9. 共享前端 P3（2026-11-07）

- 新增用户中心契约端点（与既有 `/user/inbox` 页面端点并存）：
  - `GET /api/user/inbox?page=&per_page=`：分页 + `unread`，条目含 `preview`/`format`/`communication_id`；
  - `GET /api/user/inbox/{id}`：`{code,data}`，自动已读；
  - `GET /api/user/reports?page=&per_page=`：举报分页，含 `texture_name`。
- 实现：`src/controllers/notification.rs`（`inbox`/`message`/内容渲染与预览清洗）、
  `src/controllers/report.rs`（`track`）、`src/routes.rs`。
- 验证：`cargo check` 通过；`cargo test` 全绿（含 doctest）；CI 验证待推送。

### 2026-11-07（P4 增量）
- 新增 `GET /api/admin/dashboard`：用户/角色/材质数量与存储空间。
- 实现：`src/controllers/admin.rs`（`dashboard`）、`src/routes.rs`。
- 验证：`cargo check` 通过；`cargo test` 全绿（含 doctest）。

- `POST /admin/i18n`：当 `Accept: application/json` 时返回 `{code:0,message}`（共享前端直接调用），否则保持原 302 跳转。

- 新增 `GET|PUT /api/admin/options/{group}`（`options|customize|score|resource`）：返回/保存站点配置、配色、积分、资源选项，供管理端 SPA 使用；布尔项统一 `"true"/"false"`。

- 新增 `GET /api/admin/communications`、`GET /api/admin/communications/{id}`（列表 / 编辑器数据）；`POST|PUT /admin/communications*` 与撤回在 `Accept: application/json` 时返回 JSON（否则保持 302）。

- 管理端 SPA 通过请求头 `X-Neoskin-Spa: 1` 标识；`POST|PUT /admin/communications*` 与撤回据此返回 JSON，其他请求保持 302（不改变 legacy 行为）。

- 新增 `GET /api/admin/update/info`：返回 `latest/current/error/can_update/package/upload_limits`，管理端更新页直接展示最新版本与可更新状态。

### 2026-11-08（bootstrap 开关 + 邮箱验证门禁）

- `src/controllers/spa.rs` 的 bootstrap options 增加 `allow_downloading_texture`、
  `require_verification`、`player_name_length_min/max`、`user_initial_score`、`return_score`、
  `sign_after_zero`、`sign_gap_time`、`reporter_score_modification`、`reporter_reward_score`。
- 新增 `support/api.rs::require_verified`；`controllers/player.rs` 的 `verified_guard` 仅对
  `/user/*` 生效（`/api/players` 不门禁），并覆盖角色 6 个处理器。
- `controllers/skinlib.rs`（`/texture*`、`POST /skinlib`）、`controllers/report.rs`
  （`POST /skinlib/report`）、`controllers/user_page.rs::oauth` 接入门禁。
- `src/lib.rs` 增加 `#![recursion_limit = "256"]`，修复新增校验分支后的宏展开上限。
- 174 条 PHP 路由保持不变。
- 验证：`cargo check` 通过；`cargo test --lib` 96 passed，doctest 27 ignored。
- 共享前端产物已同步到 `public/app-next`（site `index-pnUYroLe.js` / admin `index-8f1tgAaz.js`）。

### 2026-11-08（P6 切换验收，Rust 本机）

- `FRONTEND_MODE=next`：`GET /` → 302 `/app-next/site/`，`GET /admin` → 302 `/app-next/admin/`，
  两个 SPA 壳与 `/api/bootstrap` 均 200；站点/管理端页面 0 console error。
- `FRONTEND_MODE=legacy` 回滚：`GET /` 返回 legacy 页面（含 `#blessing-extra`），
  `/app-next/site/` 仍可访问。
- 本机启动注意：Suprnova 读取 `SERVER_PORT` / `DATABASE_URL`（不是 `APP_PORT`/`DB_*`），
  需在仓库根目录运行以命中 `public/app-next`；`APP_KEY` 必须是 URL-safe base64、32 字节、无填充。
- Playwright 浏览器 E2E（`NEOSKIN_BROWSER_E2E=1`）通过。

### 2026-11-08（NeoSkin 品牌与发布）

- 默认站点名/描述/公告、安装向导与更新包提示改为 NeoSkin；版权选项与页脚署名保留
  Blessing Skin Server 上游归属（见 `PORT-NOTES.md`）。
- 新增 `.github/workflows/release.yml`：推送 `v*` tag 后在 GitHub runner 构建并发布
  `neoskin-rs_<version>_<platform>.zip`，包含 二进制、`resources`、`public/app-next` 与部署文档。
- 共享前端产物更新为 site `index-pnUYroLe.js` / admin `index-8f1tgAaz.js`。
- 验证：`cargo test --lib --tests` 全绿。
- GitHub Release `v6.0.11` 已发布：6 个 `neoskin-rs_6.0.11_*.zip`；Windows vendored OpenSSL 通过 Strawberry Perl 构建。
