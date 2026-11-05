# NeoSkinRS

Rust / Suprnova 实现的 Minecraft 皮肤托管平台后端，迁移自 `blessing-skin-server-Laravel13`（6.0.11_patch）。

## 技术栈

- **语言**：Rust 1.94+
- **框架**：Suprnova v4.6.2（Laravel 13 风格）
- **ORM**：SeaORM
- **数据库**：SQLite / MySQL / MariaDB / PostgreSQL / SQL Server（与 Laravel 一致）
- **认证**：Suprnova Auth (sessions, guards, 2FA/TOTP) + OAuth2
- **前端**：Inertia (Svelte/React/Vue) + Vite 8
- **图像**：`image` crate

## 项目结构

```
NeoSkinRS/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── controllers/         # 控制器（auth/user/player/skinlib/admin/...）
│   ├── models/              # SeaORM entities
│   ├── migrations/          # 17 张表（含 seaql_migrations）
│   ├── support/             # PHP 语义、OAuth、视图、插件 API（Filter/Hook/Event）
│   ├── plugins/             # 编译期注册的原生 Rust 插件
│   └── routes.rs
├── resources/views/         # Twig 契约的 MiniJinja 模板
├── storage/                 # 运行时数据（不入库）
└── tests/                   # 单元 + 数据库矩阵
```

## 快速开始

```sh
# 1. 安装依赖
cargo build

# 2. 配置数据库
cp .env.example .env

# 3. 运行迁移
cargo run -- migrate

# 4. 启动服务
cargo run -- serve
```

### 当前可用命令（Phase 0–2）

```powershell
# Windows：OpenSSL 便携版 + 符号链接修复 + 构建
powershell -ExecutionPolicy Bypass -File scripts\build.ps1

# 迁移（SQLite：database.sqlite）
suprnova migrate

# 启动
powershell -ExecutionPolicy Bypass -File scripts\serve.ps1

# 写入开发管理员：admin@example.com / secret123
cargo run --example seed_admin

# 质量检查
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

已实现端点（Phase 2）：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/__neoskin/health` | 健康检查 |
| POST | `/auth/login` | `identification`（邮箱/角色名）+ `password`，`keep` 记住我 |
| POST | `/auth/logout` | 清除 Session 与 remember cookie |
| GET | `/api/user` | 当前用户；未登录 401 JSON |

## 文档

- [MIGRATION.md](./MIGRATION.md) — 迁移计划与阶段划分
- [AGENTS.md](./AGENTS.md) — agent 行为约束

## 硬约束

1. HTTP 接口零变更（路径/参数/响应结构）
2. 数据完全兼容（17 张表 + 存量数据 + bcrypt 密码）
3. 插件 API 契约保持
4. patch 定制功能保留

## 许可证

与源项目一致。
