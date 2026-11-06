# NeoSkinRS — 验收清单

> 最后更新：2026-11-06
> 基准：`blessing-skin-server-Laravel13`（6.0.11_patch，只读）
> 当前结论：路由/数据/认证/OAuth/视图契约/插件清单/更新流程/浏览器 E2E 均通过；SQL Server 因上游驱动缺失暂不支持。

## 1. 路由

| 项 | 结果 |
| --- | --- |
| PHP 源路由 | 174 条 / 145 唯一路径 / 227 个「方法×路径」 |
| Rust 注册 | 173 条声明 / 227 个「方法×路径」（`_ignition/*` 除外） |
| 未接通 | 0 |
| 备注 | `/{player}.json`、`/csl/{player}.json` 共用根级动态段处理器 |

验收命令：

```powershell
cargo test --lib routes
```

## 2. 数据与兼容性

| 项 | 结果 |
| --- | --- |
| 表 | 17 / 17（16 业务表 + `migrations`） |
| 主键 | `u32` 无符号（`increments`）；`u64` 大无符号走 `StoredU64`；PHP `integer()` 外键保持 `i32` |
| 密码 | bcrypt `$2y$` / `$2a$` / `$2b$` 验证通过 |
| 纹理 | SHA-256 十六进制 hash |
| 时间 | `Y-m-d H:i:s`，Asia/Shanghai |
| 存量 PHP 库 | SQLite 副本登录/页面/衣柜冒烟通过；迁移表/索引 `IF NOT EXISTS` 幂等 |

## 3. 认证与授权

- Session、记住我、验证码、CSRF（`XSRF-TOKEN` / `X-XSRF-TOKEN`）
- 登录 / 注册 / 登出 / 找回 / 重置 / 邮箱验证 / 绑定邮箱
- `auth:web`、`guest`、`role:*`、`verified`、`auth:oauth`、`scope:*`
- 插件 Filter：`client_ip`、`can_login`、`can_register`、`verify_password`、`user_password`

## 4. OAuth2（Passport 兼容）

- scopes、clients CRUD、tokens CRUD、personal access tokens
- `authorize` / `approve` / `deny`
- `POST /oauth/token`：`authorization_code`、`refresh_token`、`client_credentials`
- HTTP Basic 或表单客户端凭据；refresh token 一次性使用
- `laravel_token` transient cookie（HS256 JWT）
- 未实现：PKCE（源库无 `code_challenge` 列）、`password` grant（PHP 默认关闭）

## 5. 视图 / 前端契约

- `window.blessing`、`#blessing-extra`、`#logout-button`、`data-mark="nickname"`
- `.locale`、`<main></main>`、`data-*` 钩子
- `/lang/<locale>.js` 由后端生成
- 模板：82 个 HTML（含 partials；与 PHP 的 include 结构不同但 DOM 契约一致）
- `?3d` 头像：`support/skin3d.rs`，与 Go 端口像素级一致（318×338 / 45×45 均 0 差异）

## 6. 插件

- 清单扫描、`plugins_enabled`、依赖/冲突、启停/删除、README、市场
- 原生 Rust API：Hook 7 方法、Filter（优先级）、24 Event、`Plugin` trait、`PluginMiddleware`
- 动态库 ABI v1：`neoskin_plugin_api_version` / `_describe` / `_filter` / `_event` / `_free`
- 不执行 PHP 插件（D-001）；动态库无沙箱/卸载

## 7. 更新

- `GET /admin/update` 检测（spec=2、runtime/platform、semver）
- `download` / `upload` 校验 SHA-256 并暂存 ZIP
- `install` 写入 `payload/`；运行中的二进制写为 `*.new` 并提示重启
- PHP patch 包不会被当作 Rust 运行包

## 8. 测试与 CI

| 项 | 结果 |
| --- | --- |
| `cargo test --lib --tests` | 96 unit + 4 integration，全绿 |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| 浏览器 E2E | `NEOSKIN_BROWSER_E2E=1 cargo test --test browser` 通过 |
| 数据库矩阵 | SQLite / MySQL / MariaDB / PostgreSQL（CI `db-matrix`） |
| GitHub Actions | build / test / clippy / db-matrix / browser-e2e |

## 9. 已知限制

1. **SQL Server 不支持**：SeaORM 2.0.4 与 Suprnova v4.6.2 均无 `sqlx-sqlserver` / `database-sqlserver`。
2. 动态插件无沙箱、无法安全卸载；更新包无回滚/签名。
3. 模板引擎为 MiniJinja（非 Twig），部分 Twig 语法已转换。
4. 认证接口的部分提示仍为中文；后台/业务接口未完全本地化。
5. 输出固定 PNG（PHP 默认 WebP）。
