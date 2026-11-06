# NeoSkinRS — 部署指南

> 最后更新：2026-11-06

## 1. 构建环境

| 组件 | 版本 / 说明 |
| --- | --- |
| Rust | 1.94+ |
| Suprnova | v4.6.2（git tag） |
| OpenSSL | Windows 需便携版；Linux 用系统包 |
| 目标 | Windows x64 / Linux x64 |

Windows 本地构建：

```powershell
$env:OPENSSL_DIR='D:\WORKspace\NeoSkin\tools\openssl-3.5.6\x64'
$env:PATH="$env:OPENSSL_DIR\bin;$env:PATH"
$env:CARGO_TARGET_DIR='C:\Users\Knigh\AppData\Local\Temp\opencode\neoskin-target'
cargo build --release
```

产物：`target/release/neoskin_rs.exe`（服务）与 `console.exe`（CLI）。

## 2. 配置（`.env` / 环境变量）

| 变量 | 说明 |
| --- | --- |
| `APP_URL` | 站点根 URL（如 `https://skin.example.com`） |
| `APP_KEY` | Laravel 兼容密钥（`base64:...`）；用于 cookie/CSRF/签名 URL |
| `DB_CONNECTION` | `sqlite` / `mysql` / `mariadb` / `pgsql` |
| `DB_HOST` / `DB_PORT` / `DB_DATABASE` / `DB_USERNAME` / `DB_PASSWORD` | 数据库连接 |
| `NEOSKIN_STORAGE_ROOT` | 存储根目录（默认 `storage/`） |
| `TEXTURES_DIR` | 纹理目录（优先于 `NEOSKIN_STORAGE_ROOT/textures`） |
| `SERVER_PORT` / `APP_PORT` | 监听端口（默认 3000/8080，按 Suprnova 配置） |
| `UPDATE_SOURCE` | 运行包更新源 |
| `PLUGINS_REGISTRY` | 插件市场 registry |

注意：`NEOSKIN_STORAGE_ROOT` 必须可写；`install.lock` 写在 `<storage>/install.lock`。

## 3. 数据库初始化与升级

首次安装：

```powershell
./console.exe migrate
./console.exe bs:install admin@example.com secret123 Admin
```

日常升级：

```powershell
./console.exe update
```

请求期自动升级：已安装且 `options.version < app.version` 时，`InstallMiddleware`
会执行同一套 `update`（5.0.0 背景图迁移 + 迁移 + 版本号 + 补锁 + 清缓存）。
也可显式执行 `./console.exe update`。

## 4. 前端静态资源

1. 将源项目 `public/app/` 复制到部署目录 `public/app/`。
2. `/lang/<locale>.js` 由后端即时生成，无需额外构建。
3. 保留 `resources/views/` 模板目录（MiniJinja 运行时读取）。

## 5. 启动

```powershell
$env:SERVER_PORT='8080'
./neoskin_rs.exe serve
```

Linux systemd 示例：

```ini
[Unit]
Description=NeoSkinRS
After=network.target

[Service]
WorkingDirectory=/srv/neoskin
ExecStart=/srv/neoskin/neoskin_rs serve
Restart=always
Environment=SERVER_PORT=8080

[Install]
WantedBy=multi-user.target
```

## 6. 反向代理

Nginx 示例：

```nginx
location / {
    proxy_pass http://127.0.0.1:8080;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

需要透传 `Host`，否则绝对 URL / 邮件链接会使用错误域名。

## 7. 在线更新

Rust 端使用独立运行包：

```text
manifest.json    {"runtime":"neoskin-rs","version":"6.0.12","platform":"any"}
payload/...      与站点根目录同构
```

- 管理端 `/admin/update`：检查 → 下载/上传 → 安装
- 安装时写入 `payload/`；正在运行的二进制写为 `*.new`，重启后生效
- 有 `sha256` 时校验整包；PHP patch 包不会被识别为 Rust 运行包

## 8. 从 PHP 迁移

1. 备份 PHP 站点与数据库。
2. 配置相同的 `APP_KEY`、数据库连接、`NEOSKIN_STORAGE_ROOT`。
3. 把 `public/app/` 与 `storage/textures/` 复制到部署目录。
4. 运行 `./console.exe migrate`；已有用户但缺 `install.lock` 时启动会自动补锁。
5. 首次请求会按 `options.version` 自动执行 `update`（也可手动执行）。
6. 验证登录、皮肤库、衣柜、管理端与 Minecraft `.json` 端点。

## 9. 常见问题

| 现象 | 处理 |
| --- | --- |
| Windows 启动 stack overflow | 确认使用项目提供的 16 MiB 栈入口修复 |
| 页面 302 `/setup` | 缺少 `storage/install.lock`；完成安装或从 PHP 迁移 |
| 升级后表结构旧 | 执行 `console update`，或检查 `options.version` |
| 更新安装后仍是旧版本 | 运行中的二进制写成了 `*.new`，重启服务 |
| SQL Server 连接失败 | 当前版本不支持 sqlsrv（上游驱动缺失） |
