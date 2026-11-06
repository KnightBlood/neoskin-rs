# AGENTS.md — NeoSkinRS (Rust / Suprnova) 迁移约束

> 最后更新：2026-11-06

本文件约束所有 agent 在 `NeoSkinRS/` 目录内的行为。

---

## 0. 开工前检查（硬性）

每轮开始、动手之前，先读以下文件；涉及数据库时还必须读第 3 项：

1. `../CROSS-PROJECT-SYNC.md`：四项目状态/待办、各项目数据库测试入口、当前快照。
2. `../DECISIONS.md`：跨项目决策（例如 D-001：插件采用原生 API + 重写）。
3. `../DATABASE-MATRIX-PLAYBOOK.md`（涉及 DB 时）：本机数据库、环境变量契约、
   矩阵测试必须覆盖的项、各库坑位。
4. 本项目 `PROGRESS.md`、`PORT-NOTES.md`：当前进度与已知差异。

禁止在未读上述文件的情况下临时发明流程；已有约定优先于临场做法。

## 1. 目标与基线

- **目标**：将 `blessing-skin-server-Laravel13`（6.0.11_patch）完整迁移到 Rust 1.94+ + Suprnova v4.6.2。
- **基线**：`D:\WORKspace\NeoSkin\blessing-skin-server-Laravel13`（只读参考）。

## 2. 硬约束（不可破坏）

1. **HTTP 接口零变更**：173 条路由的路径/方法/参数/响应结构与源完全一致。
2. **数据完全兼容**：17 张表 + 存量数据 + bcrypt 密码（需 `$2y$` → `$2a$` 归一化）+ SHA-256 纹理 hash。
3. **插件 API 契约**：Hook（7 方法）、PluginManager（13 方法）、Plugin、24 个 Event。
4. **patch 功能保留**：6.0.3 ~ 6.0.11_patch 全部功能。
5. **禁止修改源目录**。
6. **前端直接复用，不自己重写**：`resources/assets/`（前端源码）与
   `public/app/`（预编译产物）原样复用源项目，**不重新打包、不换技术栈、
   不做 SPA 改造**。静态产物拷贝到 `public/app/`，`/lang/<locale>.js`
   由后端生成。服务端模板（Twig）是**前端契约**：`window.blessing`、
   `#blessing-extra`、`#logout-button`、`data-mark="nickname"`、`.locale`、
   `<main></main>`、`data-*` 钩子必须逐字保住。详见 `MIGRATION.md §9.0`。

## 3. 技术栈锁定

| 组件 | 版本 |
| --- | --- |
| Rust | 1.94.0+ |
| Suprnova | v4.6.2 |
| ORM | SeaORM |
| 数据库 | SQLite / MySQL / MariaDB / PostgreSQL；SQL Server 待框架支持 |
| 前端 | Inertia (Svelte/React/Vue) + Vite 8 |

> 插件策略（D-001）：PHP 插件不可执行；实现原生 Hook/Filter/24 Event，
> 现有 PHP 插件需重写。清单层（扫描/启停/市场）保持兼容。

## 4. 禁止事项

1. 禁止改变数据库表结构语义。
2. 禁止破坏插件 API。
3. 禁止改动 `.env` 运行时行为。
4. 禁止提交 `target/`、`node_modules/`、`.env`。

## 5. 代码风格

- 遵循 Rust 标准（rustfmt + clippy）。
- 注释与文档使用中文，代码标识符使用英文。

## 6. 验证清单

```sh
cargo build
cargo test
cargo clippy -- -D warnings
```

- [ ] `cargo build` 无错误
- [ ] 所有 174 条路由注册成功（展开 227 个「方法×路径」，`_ignition/*` 除外）
- [ ] 数据库 migration 执行无错误
- [ ] 存量数据可正常读取
- [ ] bcrypt 密码验证通过（含 `$2y$` 前缀）
- [ ] OAuth2 端点与 Passport 响应一致
- [ ] Minecraft 客户端可正常获取皮肤/披风
- [ ] 67 个测试用例全部通过

## 7. 进度报告格式

agent 完成一个阶段后必须报告：
1. 已完成的 MIGRATION.md 阶段编号。
2. 修改的文件清单。
3. 失败/阻塞项与原因。
4. 下一阶段待办。

---

## 8. 源代码图谱（CodeGraph）

源项目 `D:\WORKspace\NeoSkin\blessing-skin-server-Laravel13`（只读）已生成机读代码图谱，移植时**优先读取它**，不要靠反复 grep 源码推断契约。

| 文件 | 内容 | 用途 |
| --- | --- | --- |
| `CODEGRAPH.md` | 架构总览、17 张表、路由分组、插件 API | 先读这一份建立全局认知 |
| `codegraph/routes.json` | 173 条路由（method / uri / name / action / middleware） | 1:1 生成路由注册代码 |
| `codegraph/controllers.json` | 20 个控制器类名 | 对照拆分 Controller |
| `codegraph/models.json` | 6 个 app 层 Eloquent 模型 | 对照生成 Entity/Model |
| `codegraph/events.json` | 24 个 Event + 9 个 Listener | 事件总线移植 |
| `codegraph/services.json` | 11 个 Service | 业务层移植 |
| `codegraph/middleware.json` | 12 个中间件 | 过滤器/中间件链移植 |

使用纪律：

1. 路由以 `routes.json` 为唯一权威，**不得增删改**路径、方法或参数名。
2. 表结构以 `database/migrations/` 为准（`codegraph` 不含字段级 DDL），字段名/类型/默认值必须逐字对齐。
3. 若发现 `codegraph` 与源码不一致，以源码为准，并在本文件记录偏差。
4. 迁移过程中新增的任何映射关系，追加到本目录 `PORT-NOTES.md`，不回写源项目。
