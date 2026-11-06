# NeoSkinRS — 数据库矩阵

> 最后更新：2026-11-06
> 结论：SQLite / MySQL / MariaDB / PostgreSQL 通过；**SQL Server 暂不支持**（SeaORM/Suprnova 无驱动）。

## 1. 本机数据库

| 方言 | 端口 | 账号 | 备注 |
| --- | --- | --- | --- |
| SQLite | — | — | 默认 `database.sqlite` |
| MySQL | 3307 | `root` / 空 | 共享脚本 `tools/scripts/db.ps1` |
| MariaDB | 3308 | `root` / 空 | 同上 |
| PostgreSQL | 5433 | `postgres` / `123456` | 同上 |
| SQL Server（WiltonDB） | 1433 | `wilton` / `123456` | Rust 端不使用 |

启动/停止：

```powershell
D:\WORKspace\NeoSkin\tools\scripts\db.ps1 start all
D:\WORKspace\NeoSkin\tools\scripts\db.ps1 status
D:\WORKspace\NeoSkin\tools\scripts\db.ps1 stop all
```

## 2. 环境变量契约

| 变量 | 取值 |
| --- | --- |
| `DB_CONNECTION` | `sqlite` / `mysql` / `mariadb` / `pgsql` |
| `DB_HOST` | `127.0.0.1` |
| `DB_PORT` | 见上表 |
| `DB_DATABASE` | 建议 `blessing_skin` |
| `DB_USERNAME` / `DB_PASSWORD` | 见上表 |
| `NEOSKIN_STORAGE_ROOT` | 临时目录（矩阵测试用） |

## 3. 运行矩阵测试

```powershell
$env:OPENSSL_DIR='D:\WORKspace\NeoSkin\tools\openssl-3.5.6\x64'
$env:PATH="$env:OPENSSL_DIR\bin;$env:PATH"
$env:CARGO_TARGET_DIR='C:\Users\Knigh\AppData\Local\Temp\opencode\neoskin-target'

# SQLite
$env:DB_CONNECTION='sqlite'; cargo test --test db_matrix

# MySQL
$env:DB_CONNECTION='mysql'; $env:DB_PORT='3307'; $env:DB_DATABASE='blessing_skin'
$env:DB_USERNAME='root'; $env:DB_PASSWORD=''; cargo test --test db_matrix

# MariaDB
$env:DB_CONNECTION='mariadb'; $env:DB_PORT='3308'; $env:DB_DATABASE='blessing_skin'
$env:DB_USERNAME='root'; $env:DB_PASSWORD=''; cargo test --test db_matrix

# PostgreSQL
$env:DB_CONNECTION='pgsql'; $env:DB_PORT='5433'; $env:DB_DATABASE='blessing_skin'
$env:DB_USERNAME='postgres'; $env:DB_PASSWORD='123456'; cargo test --test db_matrix
```

CI：`.github/workflows/ci.yml` 的 `db-matrix` job 启动 MySQL/MariaDB/PostgreSQL 服务后跑同一命令。

## 4. 覆盖内容

- 17 张表建表/补表与索引幂等
- 用户/玩家/纹理/衣柜插入与查询
- 无符号 ID（`u32` / `StoredU64`）DDL 断言
- 时间戳格式 `Y-m-d H:i:s`
- PostgreSQL 保留字 `textures.public` 引用
- MySQL 不支持 `CREATE INDEX IF NOT EXISTS` → 先查后建

## 5. 已知差异

| 方言 | 差异 | 处理 |
| --- | --- | --- |
| MySQL/MariaDB | `CREATE INDEX IF NOT EXISTS` 不支持 | `has_index` 先查再建 |
| PostgreSQL | `public` 是关键字 | 统一用方言 `quote()` |
| PostgreSQL | `timestamp` 写入需文本转换 | SeaORM 实体 + `chrono`，已通过矩阵 |
| SQL Server | 无 SeaORM/Suprnova 驱动 | 不在支持范围；文档登记为上游阻塞 |

## 6. 结果

| 方言 | 本机 | CI |
| --- | --- | --- |
| SQLite | ✅ | ✅ |
| MySQL | ✅ | ✅ |
| MariaDB | ✅ | ✅ |
| PostgreSQL | ✅ | ✅ |
| SQL Server | ❌（无驱动） | ❌ |
