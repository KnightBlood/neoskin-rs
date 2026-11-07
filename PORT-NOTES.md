# NeoSkinRS — 端口偏差与决策记录

> 最后更新：2026-11-06

## 1. 框架来源

- Suprnova **未发布到 crates.io**；官方安装方式为
  `cargo install --git https://github.com/eas4ai/suprnova.git --tag v4.6.2 suprnova-cli`。
- 本机仓库级做法：克隆 `v4.6.2` 到临时目录后 `cargo install --path suprnova-cli --locked`，
  生成的项目按模板写入 `tag = "v4.6.2"` 的 git 依赖。
- Rust 工具链要求 `1.94.0+`；本机通过 rsproxy 镜像安装并设为默认。

## 2. 数据库驱动范围（待确认）

- Suprnova 的 `framework/Cargo.toml` 只声明：
  `database-sqlite`、`database-postgres`、`database-mysql`（MariaDB 走 MySQL 驱动）。
- **没有 SQL Server 驱动**，SeaORM/sqlx 也不提供 SQL Server 后端。
- 因此 AGENTS.md 的「SQLite / MySQL / MariaDB / PostgreSQL / SQL Server」五库矩阵
  在 Rust 端只能实现前四库。若必须支持 SQL Server，需要：
  1. 放弃 Suprnova/SeaORM 的 SQL Server 路径，另建 `tiberius` 直连的数据访问层；或
  2. 调整 Rust 端验收范围（推荐，等用户确认）。

## 3. 时间/哈希兼容

- 日期：Laravel 使用 `Y-m-d H:i:s`（Asia/Shanghai，naive）。Rust 侧计划使用
  `chrono::NaiveDateTime` + `datetime_cast = "naive"`，SQLite 文本格式与 PHP 一致。
- 密码：bcrypt `$2y$` 需在验证前归一化为 `$2a$`（`bcrypt` crate 不识别 `$2y$`）。
- 纹理：SHA-256 十六进制小写，与 PHP/Go 一致。

## 4. 前端

- 不采用 Suprnova 的 Inertia SPA；保持 AGENTS.md 的 legacy 前端契约
  （`public/app` + `window.blessing` + `#blessing-extra` 等）。
- 页面渲染方案待 Phase 6 定：Askama/Minijinja/Tera 转换 Go templates，或直接返回
  预渲染 HTML 片段。

## 5. Windows 构建依赖（OpenSSL 便携版）

- Suprnova 无条件依赖 `webauthn-rs`（默认启用 attestation），后者依赖
  `openssl-sys`；Windows 构建必须能找到 OpenSSL 开发库。
- 不安装 MSI：使用 FireDaemon OpenSSL 3.5.6 便携 zip，解压到
  `D:\WORKspace\NeoSkin\tools\openssl-3.5.6`（`x64/{include,lib,bin}`）。
- 构建时设置 `OPENSSL_DIR=<tools>\openssl-3.5.6\x64`；运行时把
  `<tools>\openssl-3.5.6\x64\bin` 加入 `PATH`（加载 `libssl-3-x64.dll` /
  `libcrypto-3-x64.dll`）。
- `scripts/build.ps1` 与 `scripts/serve.ps1` 已封装上述环境变量。
- **Windows 符号链接**：Suprnova 的
  `framework/templates/suprnova-ui` 是指向 `crates/suprnova-live/components`
  的 Git 符号链接。Windows 未启用符号链接时会被检出为文本文件，导致 Askama
  报 `template "suprnova-ui/search-box/island.html" not found`。
  `scripts/build.ps1` 会先 `cargo fetch`，再把该路径替换为目录 junction
  （`mklink /J`）；Linux/CI 无需处理。

## 6. 模型映射细节

- 16 张业务表全部建模；额外新增 Suprnova 会话表 `sessions`（框架 Session
  middleware 需要，不影响既有 17 表语义）。
- `textures.type` 与 `notifications.type` 是 Rust 关键字列，使用手写 SeaORM
  实体并 `#[sea_orm(column_name = "type")]`；`textures.public` 映射为
  `public_flag`。
- `user_closet` 无主键：手写实体以 `user_uid` 作为逻辑主键（仅用于查询/
  插入/删除，不做单行更新）。
- 日期列使用 `datetime_cast = "naive"` + `DateTime<Utc>`（Suprnova 以 naive
  文本存取），与 Laravel `Y-m-d H:i:s` 一致；手写实体用 `NaiveDateTime`。
- `jobs.created_at` 是 Unix 时间戳而非日期，模型设置 `timestamps = false`。

## 7. 认证/Session（2026-11-02）

- 会话存储使用 Suprnova `sessions` 表；cookie 名通过 `SESSION_COOKIE`
  设为 `blessing_skin_session`（与 PHP 一致）。
- 本地 HTTP 开发设置 `SESSION_SECURE=false`；CsrfMiddleware 的 XSRF cookie
  也跟随该值（`xsrf_cookie_secure`），否则浏览器不会回传 Secure cookie。
- 登录成功调用 `regenerate_session_id()`（防 fixation），session 中保存
  `neoskin_uid`；`/api/user` 未登录返回 `401 {"code":1,"message":"Unauthenticated."}`。
- 认证消息暂为硬编码中文，待接入 PHP `resources/lang` 与 `language_lines`。
- 开发种子：`cargo run --example seed_admin` 写入 `admin@example.com / secret123`。
- **记住我**：cookie `remember_web_59ba36addc2b2f9401580f014c7f58ea4e30989d`，
  值 `uid|token|hmac(sha256, password_hash, APP_KEY)`（与 Go 端口一致）。
  PHP 的 `EncryptCookies` 整体加密未复刻；HMAC 第三段保证不可伪造、改密后
  旧 cookie 立即失效。有效期 5 年、HttpOnly、SameSite=Lax；登出轮换
  `remember_token` 使旧 cookie 失效。APP_KEY 优先取 `NEOSKIN_APP_KEY`，
  否则取 Suprnova 的 `APP_KEY`。
- **登录失败/验证码**：失败计数键 `login_fails:{ip}`（Suprnova Cache，
  3600 秒），超过 3 次后要求验证码；本地验证码为 SVG（session 保存短语），
  PHP 的 Gregwar PNG 未逐像素复刻。配置 `recaptcha_secretkey` 时调用
  `recaptcha.net/api/siteverify`（reqwest）。
- **422 响应差异**：Suprnova 的 Validation 错误体在 `{message, errors}` 之外
  附带 `request_id`；前端按字段读取 `errors` 不受影响，但严格字节对比时需
  注意（后续可在错误处理器中移除）。
- **OAuth2（2026-11-03）**：
  - RSA 密钥首次使用时生成到 `NEOSKIN_STORAGE_ROOT/oauth-private.key`、
    `oauth-public.key`（默认 `storage/`）；也可直接放入 PHP Passport 的密钥。
  - 客户端 secret 使用 bcrypt 存储（`$2y$`），创建时返回一次 `plain_secret`。
  - personal access client 在首次创建个人令牌时自动生成（`user_id=NULL`,
    `personal_access_client=1`）。
  - JWT claims 与 Go 端口一致：`aud=[client_id]`、`jti`、`iat`、`nbf`、
    `exp`、`sub`、`scopes`；有效期 1 年（31536000 秒）。
  - Bearer 校验只验证签名与过期（`validate_aud=false`），再查
    `oauth_access_tokens` 的 `revoked`/`expires_at`。
  - 已实现：scopes/clients CRUD/tokens/personal tokens/authorize/approve/deny、
    `/oauth/token`（authorization_code / refresh_token）、`/oauth/token/refresh`。
  - 授权码存 `oauth_auth_codes`，TTL 600 秒，交换后立即 `revoked=1`；
    refresh 令牌一次性使用，刷新时同时撤销旧 access token。
  - `/oauth/token` 按 Passport 约定排除 CSRF（`CsrfMiddleware::except_method`）。
  - 授权页是端口自带的轻量 HTML（PHP 版 Passport 13 未发布
    `passport.authorize` 视图，原项目实际会报视图缺失）；字段与流程
    （`auth_token` + `_token`、approve POST、deny DELETE）与 Passport 对齐。
  - `laravel_token` transient cookie 为明文 HS256 JWT（PHP 版经
    `EncryptCookies` 加密），claims 与 Go 端口一致：`sub`/`csrf`/`exp`。
  - 未实现：HTTP Basic 客户端认证（当前只读表单 `client_id`/`client_secret`）、
    `prompt=consent|login|none`、PKCE、`password`/`client_credentials` grant。
- **玩家 / 衣柜（2026-11-03）**：
  - `players` 使用 Suprnova 的 `Model`（`filter/get/find/create/update`），
    `user_closet` 与 `textures` 使用手写 SeaORM 实体；两者混用不影响契约。
  - 衣柜分页在内存完成（先取该用户全部 `user_closet` 行，再按 `texture_tid`
    批量取纹理），避免 `textures.public` 在 PostgreSQL 的保留字问题；
    `perPage` 默认 6，返回 Laravel `LengthAwarePaginator` 的完整字段。
  - 分页 `path` 使用 `APP_URL + request.path()`（PHP 使用当前请求 URL）。
  - 角色名校验支持 `official/cjk/utf8/custom` 与 PHP `/pattern/flags` 形式。
- **纹理服务（2026-11-03）**：
  - `GET /{player}.json` 与 `GET /csl/{player}.json` 共用处理器；Suprnova 的
    `matchit` 路由不支持「参数 + 字面后缀」同段，因此注册 `/{player}` 并在
    处理器内校验 `.json` 后缀（非 `.json` 返回 404）。`/{player}` 是根级
    动态段，静态路由优先匹配，不影响 `/admin`、`/api/...` 等。
  - 纹理文件根目录：`TEXTURES_DIR` → `NEOSKIN_STORAGE_ROOT/textures` →
    `storage/textures`；hash 仅允许十六进制与 `-`/`_`，防止路径穿越。
  - `avatar/*` 与 `preview/*`：2D 视图见 `support/skin.rs`（头部叠加帽子层、
    正/背面按头/身体/手脚拼接，支持 Alex 细手臂与 64×32 旧皮肤）。
  - `?3d` 头像见 `support/skin3d.rs`：移植 `Blessing\Renderer\SkinRenderer`
    （等距投影 + 多边形扫描线 + 双线性缩放）。已用同一合成皮肤与 Go 端口
    做像素级对比：`ratio=25` 输出 318×338、最终 45×45 均 **0 像素差异**。
  - 偏差：输出固定 PNG（PHP 默认 WebP）；
    预览画布尺寸与 PHP 的多边形渲染略有差异（约 429×444 vs 430×437）。
  - 备用头像使用源项目 `resources/misc/textures/avatar{2d,3d}.png`，
    通过 `include_bytes!` 内嵌。
- **站内信（2026-11-03）**：
  - 可见性规则对齐 `Communications::visible`：无 `communication_id` 的旧通知
    始终可见；有则要求 `communications.status='published'`、`kind in
    (message,both)` 且 `delivery_token` 一致。
  - Markdown 使用 `pulldown-cmark`（GFM）渲染；PHP `CommunicationRenderer`
    的 DOM 白名单清洗已用 `ammonia` 复刻（标签/属性/URL scheme 白名单、
    `rel="noopener noreferrer"`、相对链接保留）。差异：不做 `id` 前缀重写与
    `class`/`style` 值过滤，`style` 整体丢弃（PHP 只保留 font-weight/padding）。
  - `blocks` 编辑器按 PHP 的结构生成 heading/alert/collapse/divider HTML。
- **注册（2026-11-03）**：验证码、邮箱唯一、密码 8–32、角色名规则、
  `regs_per_ip`、初始积分、`register_with_player_name` 开关、注册后自动登录
  （Session ID 轮换）。忘记密码/重置/邮箱验证依赖邮件与签名 URL，待补。
- **用户资料 / 签到（2026-11-03）**：
  - `GET /user/score-info` 的 `rate.storage` 按 PHP `(int)` 语义转换：
    `score_per_storage='true'` 得到 `0`（与 PHP 行为一致，不是布尔转换）。
  - 签到 `sign_after_zero` 与 `sign_gap_time` 两种间隔计算对齐 PHP
    （`last_sign_at + gap` 或「当天 0 点 / 次日 0 点」）。
  - 改密/改邮箱后调用 `invalidate_session()`（等价 `Auth::logout()`），
    响应仍为 `code: 0` 加对应提示文案。
  - 删除账号不级联删除角色/材质（与 PHP 一致：角色会被数据库外键或业务
    清理，当前表无外键，保留原状）。
- **皮肤库（2026-11-03）**：
  - 上传走 Suprnova multipart（`UploadedFile` 字段），用 `image` crate 解码
    并重新编码 PNG；PHP 用 Imagick（`png:compression-*`），因此新上传文件的
    SHA-256 与 PHP 可能不同。存量 hash 的读取/下载不受影响。
  - 上传校验对齐 PHP：`max_texture_width`、宽高必须为 64/32 的倍数、
    比例规则（皮肤 1 或 2、Alex 必须 1、披风必须 2）。
  - 积分：`KB * (public ? score_per_storage : private_score_per_storage)
    + score_per_closet_item - score_award_per_texture`；`option()` 的
    `'true'/'false'` 先转布尔再取整（新增 `options::int_value`）。
  - 列表在内存过滤/分页（PHP 用 SQL join users）；返回字段与
    `paginate(20)` 的 `data` 行一致（含 `nickname`）。
- **举报（2026-11-03）**：`usingSearchString` 简化为 reason/ID 子串匹配；
  `reject/delete/ban` 的积分返还、奖励、封禁权限比较均已对齐。
- **后台管理（2026-11-03）**：
  - 用户/角色列表在内存过滤与分页（PHP 用 SQL `usingSearchString`），
    返回字段与 Eloquent 序列化一致（含 `ip`、`verification_token`）。
  - 权限规则对齐：目标用户 `permission >=` 操作者且非本人时 403；
    把他人设为 `ADMIN` 需要 `SUPER_ADMIN`；不能修改自己的权限。
  - 删除用户会先删除其全部角色（PHP `User::delete()`）。
  - 发送通知直接创建 `communications`（kind=message、source 编辑器）并为
    每个接收者写入 `notifications`（UUID v4）；web/API 都返回 302 `/admin`
    与 PHP 一致。
  - `/admin/*` 页面尚未实现（无 Twig 视图），因此通知发送后的跳转会 404；
    后端契约（创建数据）已完成。
- **翻译覆盖（2026-11-03）**：`language_lines.text` 存
  `{"locale": "text"}` JSON；更新时合并当前 locale。locale 解析用
  `Accept-Language`（`en-US` → `en_US`），默认 `zh_CN`；PHP 的
  `DetectLocale` 中间件（session/cookie）尚未接入。
  `POST /admin/i18n` 返回 302 `/admin/i18n`（页面未实现，跟随会 405）。
- **站点选项（2026-11-03）**：`options::set` 先查再插入/更新（`options`
  表数据量小）；复选框统一写 `true`/`false`，文本只在提交时覆盖；
  `site_name`/`site_description`/`content_policy` 同时写 `_<locale>`。
  `?clear-cache` 暂不执行（未启用 Suprnova 缓存驱动）。
- **通信管理（2026-11-03）**：
  - 渲染器抽到 `support/communication.rs`，通知与通信共用。
  - 投递受众与 PHP `Communications::deliver` 一致：`all` 只发给
    `permission >= 0` 的用户，`normal` 只发 `permission = 0`。
  - 已发布内容更新时同步现有通知的 `data`（按 `communication_id` +
    `delivery_token` 匹配），不重复投递；撤回后通知不可见（可见性规则）。
  - `import` 是简化版 Html→blocks：识别标题、分隔线、alert，其余归为文本块。
  - `both` 类型的公告副本以 `audience.communication_id` 关联，撤回时一并撤回。
  - 消息/公告页面（index/inbox/message）仍待视图层。
- **静态资源与视图基础（2026-11-03）**：
  - `public/app/` 原样拷贝（`.gitignore` 已忽略），由 catch-all 路由
    `GET /{*path}` 提供；指纹文件（`name.<hash>.ext`）使用一年 immutable 缓存。
  - `GET /{player}.json` 不再单独注册：Suprnova 的 `matchit` 不允许
    `/{player}` 与 `/{*path}` 共存，改由静态处理器识别根级 `*.json` 并转发。
  - `resources/lang/<locale>/front-end.yml` 启动时生成 `public/lang/<locale>.js`；
    数据库 `language_lines`（group=front-end）按键路径合并覆盖，
    翻译 API 变更后自动重新生成。`group` 是 SQL 保留字，查询改为
    `all()` 后内存过滤。
  - `/api` 返回 `blessing_skin`/`spec`/`copyright`/`site_name`；
    `/.well-known/change-password` 302 到 `/user/profile`。

- **页面层补齐（2026-11-04）**：
  - 后台 14 个页面已实现并返回 200：`/admin`、users、players、reports、
    i18n、customize、score、options、resource、status、communications、
    update、plugins/manage、plugins/market；此前“页面未实现会 404”的记录作废。
  - `support/form.rs` 对应 PHP `OptionForm`：checkbox 勾选写
    `checked="true"`，select 选中写 `selected=selected`；表单整体 HTML
    由控制器拼接后以 `{{ forms.x|safe }}` 输出。
  - Suprnova 不支持 `_method` 伪造，通信编辑表单提交到
    `POST /admin/communications/{communication}`（路由名
    `admin.communications.update.post`），与 PUT 共用处理器。
  - 用户中心新增 `/user/inbox`、`/user/inbox/{id}`、`/user/oauth/manage`；
    站内信按 PHP `Communications::visible` 规则过滤（published + message/both
    + `delivery_token` 相等），阅读时写入 `read_at`。
  - 皮肤库页面新增 `/skinlib`、`/skinlib/show/{texture}`、`/skinlib/upload`；
    详情页检查文件存在（`auto_del_invalid_texture` 开启时删除记录），
    私有材质沿用 403/404 可配置行为。
  - 未登录访问需登录的 HTML 页面返回 302 `/auth/login`（`require_page_user`），
    而非 API 的 401 JSON。
  - Twig 转义对齐：自定义 formatter 只转义 `& < > " '`，不转 `/`；
    `nl2br`、`tojson` 过滤器补全。Twig 的 `{% for k, v in map %}` 在
    MiniJinja 中必须写 `map|items`。
  - `Accept-Language` 解析修正：先完整标签匹配再回退语言前缀，
    `zh-TW` 不再被误判为 `zh_CN`。

- **安装与初始数据（2026-11-04）**：
  - `options` 默认值在启动时按 PHP `import_options` 补齐（只插入缺失键），
    `version=6.0.11_patch`、`announcement`/`copyright_text` 占位符已替换，
    新增 `max_texture_width=8192`。
  - `storage/install.lock` 与 PHP `CheckInstallation` 对齐；库中已有用户但
    没有锁时启动自动补锁，避免从 PHP 迁移的存量库被导向安装向导。
  - `InstallMiddleware` 在未安装时把非 `setup/*`、非静态资源请求 302 到
    `/setup`（对应 `RedirectToSetup`）。**未复刻** PHP 在
    `config('app.version') > option('version')` 时调用 `Artisan::call('update')`
    的请求期编排；升级后需显式运行迁移。
  - `POST /setup/database` 只改写 `.env` 的 `DB_*` 行，不改运行时连接
    （Suprnova 以 `DATABASE_URL` 为准，需重启生效）。
  - `POST /setup/finish` 创建 `SUPER_ADMIN` 用户、写 `site_name`/`site_url`
    与锁文件；用临时 SQLite 库已端到端验证。
  - `GET /admin/chart` 按 PHP `chartData` 输出 `labels`/`xAxis`/`data`；
    Carbon `isoFormat('l')` 固定为英文星期名。
  - 更新安装端点：`/admin/update/{download,install,upload}` 已实现
    运行包暂存/安装；插件清单/启停/市场与原生 API 已实现（见下文）。

- **邮件与签名链接（2026-11-04）**：
  - 邮件走 Suprnova `Mail`（`MAIL_DRIVER=log/smtp/...`）；本地无 SMTP 时用
    `MAIL_DRIVER=log` 可完整验证，真实发信需配置 `MAIL_SMTP_*` 与 `MAIL_FROM`。
  - 重置密码链接：`/auth/reset/{uid}?expires=<unix>&signature=<hex>`，
    1 小时有效；校验 `path + ? + 排序后的非 signature 查询` 的
    HMAC-SHA256（APP_KEY），常量时间比较。
  - 邮箱验证链接：`/auth/verify/{uid}?signature=<hex>`，无过期时间。
  - 忘记密码限流按 IP 180 秒（Cache `last_mail:<ip>`）；重发验证邮件
    按用户 60 秒（Cache `last_mail_time:<uid>`），与 PHP 的 session/Cache
    语义对齐但存储位置不同。
  - 验证页为普通表单提交：成功 302 `/user`，邮箱不匹配时 flash
    `errorMessage` 并 302 回签名 URL。
  - 与 PHP 的差异：若两端 `APP_KEY` 不同，旧邮件里的链接不能跨端口通用；
    其余响应结构与 PHP 一致。

- **OAuth 兼容补充（2026-11-04）**：
  - `/oauth/token` 客户端认证顺序：`Authorization: Basic` 优先，其次
    `client_id`/`client_secret` 表单字段；错误统一 401 `invalid_client`。
  - `prompt` 支持 `none` / `login` / `consent`，多个值以空格分隔；
    `login` 用 `prompted_for_login` 会话标记避免循环。
  - 登录成功后 `redirectTo` 改为读取会话 `last_requested_path`（默认
    `/user`），`GET /auth/login?redirect=` 会写入该键（仅接受站内路径）。
  - 自动批准：存在同用户、同 client、未撤销未过期且 scope 覆盖新请求的
    access token 时，直接下发 authorization code。
  - `/api/*` 按 `routes/api.php` 的 `scope:` 声明校验；无效 scope 返回
    403 `{"message":"Invalid scope(s) provided."}`。Session 登录绕过 scope。
  - `/api/*` 从 CSRF 中间件豁免（Laravel `api` 组无 CSRF）。
  - 拼写兼容：`Closet.ReadWrtie`（源项目拼写）与 `Closet.ReadWrite` 均接受。
  - PKCE 未实现：源项目 `oauth_auth_codes` 没有 `code_challenge` 列。

- **路由方法对齐（2026-11-04）**：
  - `Route::any` 的路径改为 Suprnova `any!`：`/api`、`/auth/captcha`、
    `/.well-known/change-password`、`/admin/{customize,score,options,resource}`。
  - 后台选项路径使用分派器：`POST` 交给 `save_*`，其余方法渲染页面
    （PHP `OptionForm::handle` 只处理 POST）。
  - `/api` 本身也加入 CSRF 豁免（`/api` + `/api/*`）。
  - 用脚本逐条比对 `codegraph/routes.json`：展开方法后 227/227 覆盖；
    `_ignition/*` 为 Laravel 调试路由，不移植。
  - `/{player}.json`、`/csl/{player}.json` 依赖处理器识别 `.json` 后缀，
    因为 `matchit` 不允许「参数 + 字面后缀」与纯参数路由共存。

- **插件清单层（2026-11-04）**：
  - 扫描目录：`PLUGINS_DIR` / `NEOSKIN_PLUGINS_DIR`（逗号分隔），默认
    `plugins/`；每个插件为含 `package.json` 的子目录。
  - 清单字段：`name`/`version`/`title`/`description`/`require`/
    `enchants.conflicts`/`enchants.icon`/`enchants.config`/`config`。
  - `plugins_enabled` 存 JSON 数组 `[{name, version}]`，与 PHP 一致；
    空字符串/非法 JSON 按空数组处理。
  - 依赖检查：`blessing-skin-server` 用本端口版本，`php` 固定 `8.3.0`，
    其他插件取启用列表中的版本；无法解析的版本（如 `6.0.11_patch`）
    视为满足，避免误锁。
  - **不执行 PHP 插件代码**；原生扩展点见下。ServiceProvider/Bootstrap
    不会运行 PHP 插件逻辑。
  - `GET /admin/plugins/config/{name}` 仍返回 404（原生插件无 PHP 配置页）。
  - 市场：注册中心地址取 `plugins_registry` 选项，为空时用
    `PLUGINS_REGISTRY`（默认官方地址），`{lang}` 仅支持 `en` / `zh_CN`；
    多个地址用逗号分隔。
  - `market/list` 对每个包补充 `installed`（版本或 false）、`can_update`、
    `dependencies.all`；`dependencies.unsatisfied` 暂为空（未做完整依赖求解）。
  - `market/download` / `wget` / `upload` 使用 `zip` crate 解压；
    通过 `ZipArchive::enclosed_name` 拒绝绝对路径与 `..`（zip slip）；
    下载超时 20 秒、单包上限 128MB；解压目标为第一个插件目录。
  - 原 PHP 的 `Unzip` 会处理权限/符号链接等细节，Rust 端只做文件与目录写入。

- **原生插件 API（2026-11-05，D-001）**：
  - `support/plugin_api.rs`：
    - `Filter`：`add(name, priority, fn)` / `apply` / `remove` / `count`；
      priority 越小越先执行，同优先级按注册顺序；默认 20。
    - `Hook`：PHP `App\Services\Hook` 的 7 个方法——`add_menu_item`、
      `add_route`、`add_style_file_to_page`、`add_script_file_to_page`、
      `add_user_badge`、`send_notification`、`push_middleware`。
    - 24 个事件常量 + `Dispatcher`（`listen` / `dispatch` / `count`）。
    - `Plugin` trait + `PluginApi`：原生 Rust 插件在 `plugins::register_all`
      中注册；插件代码编译进站点二进制，不执行 PHP。
  - 已接入：`head_links`/`scripts`（`view::base_context`）、
    `user_badges`（用户/管理/皮肤库详情）、`content_policy`（上传页）、
    `user`/`admin`/`explore` 菜单、`PluginWasEnabled/Disabled/Deleted`、
    `UserTryToLogin/UserLoggedIn/UserRegistered`、玩家增删改、纹理删除、
    `UserProfileUpdated`；认证过滤器 `can_login` / `can_register` /
    `verify_password` / `user_password` / `client_ip`。
  - `Hook::addRoute` 在 `routes::register_with_plugins` 中应用；
    `Hook::pushMiddleware` 由全局 `PluginMiddleware` 串行执行。
  - 动态库插件（ABI v1）：
    - `package.json` 的 `native` 字段可为字符串或 `{ "library": "…" }`；
      文件相对插件目录。
    - 导出 `neoskin_plugin_api_version` / `neoskin_plugin_describe` /
      `neoskin_plugin_filter`（可选）/ `neoskin_plugin_event`（可选）/
      `neoskin_plugin_free`。
    - 描述 JSON 支持 filters、events、menus、styles、scripts、badges；
      Filter 对返回 JSON 做解析，非法时回退原值。
    - 示例：`samples/native-plugin/`；集成测试 `tests/native_plugin.rs`
      （设置 `NEOSKIN_NATIVE_PLUGIN` 运行）。
  - 限制：动态库是**可信原生代码，无沙箱**；启用/禁用后需重启才能卸载；
    `PluginManager` 的 `boot`/`registerPlugin`/`bootPlugin` 等价物是
    `plugins::register_all` + `register_plugin`；插件市场仍只分发清单，
    不承诺 PHP 插件可执行。

- **更新检测（2026-11-04）**：
  - 更新源优先级：`update_source` 选项 → `UPDATE_SOURCE` 环境变量 →
    `https://skin.blessing.dreamcraft.eu.org/update.json`。
  - 仅接受 `spec == 2`；连接超时 3 秒、总超时 8 秒；失败时页面显示
    `更新源连接失败` / `HTTP status code: N` / `不支持当前的更新源。`。
  - `php` 字段与固定运行时版本 `8.3.0` 比较；`latest` 与 `6.0.11_patch`
    比较时把后缀归一化为 `6.0.11`，避免误判。
  - 运行包格式（`support/update.rs`）：
    ```text
    manifest.json   {"runtime":"neoskin-rs","version":"6.0.11","platform":"any"}
    payload/...     与站点根目录同构
    ```
  - `download` / `upload` 只校验并暂存（`storage/framework/update/`），
    `install` 才写入站点根目录；SHA-256 在暂存/安装两处校验。
  - 路径穿越、`.env`、`storage/`、`plugins/`、数据库文件会被拒绝；
    正在运行的二进制写成 `*.new` 并返回 `restart_required`。
  - 更新源必须是 `runtime: neoskin-rs`；PHP patch 源会提示“不适用”，
    不会把 PHP 包覆盖到 Rust 站点。
  - 页面提供下载/上传/安装表单，也支持 `Accept: application/json`
    的 API 调用。

- **存量库迁移兼容（2026-11-04）**：
  - `create_core_tables` 的 16 张表使用 `IF NOT EXISTS`；10 个索引改为
    `has_index` 先查再建（MySQL 不支持 `CREATE INDEX IF NOT EXISTS`），
    因此可直接对 Laravel 已建表的数据库执行。
  - Rust 迁移记录写入 `seaql_migrations`，Laravel 的 `migrations` 表保持
    原样；两套记录并存，不会互相覆盖。
  - 已验证：复制 PHP `database/database.sqlite` → 启动成功 → 页面 200；
    复制 `$2y$` 用户后登录成功（bcrypt 前缀归一化生效）。
  - PHP 的 `options` 表已有数据时，启动期 `seed_defaults` 只补缺失键，
    不覆盖现有值。

- **数据库矩阵测试与无符号 ID（2026-11-05）**：
  - `tests/db_matrix.rs`：`DB_CONNECTION=mysql|mariadb|pgsql` 门控，
    默认跳过；覆盖幂等迁移、用户插入/查询、`register_at` 格式、纹理插入/计数、
    随机清理。MySQL/MariaDB/PostgreSQL 本机均通过。
  - MySQL/MariaDB 增加 DDL 断言：直接查 `information_schema.COLUMNS`，验证
    `users.uid`、`players.pid`、`textures.tid`、`reports.id`、`options.id`、
    `language_lines.id`、`scopes.id` 为 `INT UNSIGNED`；
    `notifications.notifiable_id`、`jobs.id`、`communications.id`、
    `oauth_clients.id`、`oauth_access_tokens.user_id` 为 `BIGINT UNSIGNED`；
    `jobs.attempts` 为 `TINYINT UNSIGNED`；`user_closet.user_uid`、
    `textures.uploader`、`players.uid`、`reports.*` 仍为有符号 `INT`。
    MariaDB 的 `int(10) unsigned` 显示宽度在比较前归一化。
  - MySQL 不支持 `CREATE INDEX IF NOT EXISTS`；迁移改为
    `has_index` 先查再建（SQLite/PostgreSQL 同样适用）。
  - **无符号策略（已实现）**：严格保留 PHP schema，不把 MySQL ID 改 signed。
    - `u32`：`users.uid`、`players.pid`、`textures.tid`、`options.id`、
      `reports.id`、`scopes.id`、`language_lines.id`、`textures.likes`、
      `communications.author_id`、`jobs.available_at`/`created_at`/`reserved_at`。
    - `u8`：`jobs.attempts`。
    - `u64`：`jobs.id`、`communications.id`、OAuth 5 表的 `id`/`user_id`/`client_id`。
    - `StoredU64`：`notifications.notifiable_id`（MySQL 无符号，PG/SQLite 有符号）。
    - 保持 `i32`：PHP `integer()` 列，包括 `players.uid`、`textures.uploader`、
      `reports.tid/uploader/reporter`、`user_closet.user_uid/texture_tid`。
      跨类型查询/写入显式 `as i32` / `as u32`，控制器层统一处理。
  - 迁移按后端分支：MySQL/MariaDB 生成 `UNSIGNED`，PostgreSQL/SQLite 生成
    同宽有符号列；`manager.get_database_backend()` 传入所有建表 helper。
  - SeaORM 2.0.2 自带 `TryGetable for u32` 与 `TryGetable for u64`
    （MySQL 无符号列、PG/SQLite 有符号列均可解码），无需自定义 `TryGetable`。
  - SQLite/MySQL 冒烟：登录、`/user`、`/admin/users`、`/api/admin/users`、
    `/api/closet`、`/api/user/notifications`、OAuth 客户端创建/删除均通过；
    MySQL 额外验证衣柜 add/rename/remove 与 `textures.likes` 增减。



## 9. 共享前端新增契约端点（2026-11-07）

- 为共享前端 P3 增加 3 个**站内信/举报 JSON** 端点（PHP 原版只有 HTML 页面）：
  `GET /api/user/inbox`、`GET /api/user/inbox/{id}`、`GET /api/user/reports`。
- 这些是 NeoSkinFrontend 契约 v1 的增量端点，不计入 PHP 174 条路由；
  注册在 `src/routes.rs`，与 `/user/inbox` HTML 页面端点并存。
- 返回结构与 `NeoSkinFrontend/docs/BACKEND-CONTRACT.md` §7.2 一致；
  匿名访问返回 `401 application/json`。

### P4 增量端点（2026-11-07）

- 新增 GET /api/admin/dashboard（管理端统计：用户/角色/材质/存储空间），
  属于 NeoSkinFrontend 契约 v1 增量端点，不计入 PHP 174 条路由；`src/controllers/admin.rs` 的 `dashboard`，复用 `require_admin`。
- 匿名 401、非管理员 403；图表仍复用既有 GET /admin/chart。


- `POST /admin/i18n` 增加 JSON 响应分支（`Accept: application/json` → `{code,message}`；否则 302），供管理端 SPA 使用。
