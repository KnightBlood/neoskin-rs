#!/usr/bin/env python3
"""NeoSkinRS 多数据库矩阵冒烟测试。

前置：`tools\\scripts\\db.cmd init <target>` 与 `start <target>` 已执行。
用法：
    python scripts/matrix_db.py            # sqlite + mysql + mariadb + pgsql
    python scripts/matrix_db.py mysql pgsql

每个后端使用独立临时存储目录与端口，依次验证：
启动 → 安装向导 → 创建超管 → 登录 → 主要页面。
"""

from __future__ import annotations

import http.cookiejar
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "target" / "debug" / "neoskin_rs.exe"
if not APP.is_file():
    APP = Path(
        r"C:\Users\Knigh\AppData\Local\Temp\opencode\neoskin-target\debug\neoskin_rs.exe"
    )

BACKENDS = {
    "sqlite": None,
    "mysql": "mysql://blessing:123456@127.0.0.1:3307/blessing_skin",
    "mariadb": "mysql://blessing:123456@127.0.0.1:3308/blessing_skin",
    "pgsql": "postgres://postgres:123456@127.0.0.1:5433/blessing_skin",
}


def build_opener() -> tuple[urllib.request.OpenerDirector, http.cookiejar.CookieJar]:
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    return opener, jar


def call(opener, base, method, path, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(base + path, data=data, method=method)
    request.add_header("Accept", "application/json")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    for key, value in (headers or {}).items():
        request.add_header(key, value)
    try:
        with opener.open(request, timeout=30) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def cookie_value(jar: http.cookiejar.CookieJar, name: str) -> str:
    return next((cookie.value for cookie in jar if cookie.name == name), "")


def wait_health(opener, base: str, timeout: float = 90.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            status, _ = call(opener, base, "GET", "/__neoskin/health")
            if status == 200:
                return True
        except Exception:
            pass
        time.sleep(0.5)
    return False


def run_backend(name: str, port: int) -> tuple[bool, str]:
    url = BACKENDS[name]
    storage = Path(tempfile.mkdtemp(prefix=f"neoskin-{name}-"))
    env = os.environ.copy()
    env["NEOSKIN_STORAGE_ROOT"] = str(storage)
    env["SERVER_PORT"] = str(port)
    env["MAIL_DRIVER"] = "log"
    env["UPDATE_SOURCE"] = "http://127.0.0.1:9/update.json"
    if url:
        env["DATABASE_URL"] = url
    elif "DATABASE_URL" in env:
        del env["DATABASE_URL"]

    log_path = storage / "app.log"
    log = log_path.open("wb")
    process = subprocess.Popen(
        [str(APP)],
        cwd=str(ROOT),
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    base = f"http://127.0.0.1:{port}"
    opener, jar = build_opener()
    try:
        if not wait_health(opener, base):
            return False, "health check failed"

        status, page = call(opener, base, "GET", "/setup")
        if status != 200:
            return False, f"/setup -> {status}"

        status, page = call(opener, base, "GET", "/setup/info")
        if status != 200:
            return False, f"/setup/info -> {status}"
        match = re.search(r'name="_token" value="([^"]+)"', page)
        if not match:
            return False, "setup token missing"
        token = match.group(1)

        form = urllib.parse.urlencode(
            {
                "_token": token,
                "email": "matrix@example.com",
                "nickname": "Matrix",
                "password": "secret123",
                "password_confirmation": "secret123",
                "site_name": "Matrix",
            }
        ).encode()
        request = urllib.request.Request(base + "/setup/finish", data=form, method="POST")
        request.add_header("Content-Type", "application/x-www-form-urlencoded")
        with opener.open(request, timeout=30) as response:
            finish_page = response.read().decode("utf-8", "replace")
            if response.status != 200 or "toast-error" in finish_page:
                return False, "setup finish failed"

        xsrf = cookie_value(jar, "XSRF-TOKEN")
        status, body = call(
            opener,
            base,
            "POST",
            "/auth/login",
            {"identification": "matrix@example.com", "password": "secret123"},
            {"X-XSRF-TOKEN": xsrf},
        )
        if status != 200 or '"code":0' not in body:
            return False, f"login -> {status} {body[:120]}"

        for path in ("/", "/user", "/admin", "/admin/chart", "/admin/options", "/skinlib/upload"):
            status, _ = call(opener, base, "GET", path)
            if status != 200:
                return False, f"{path} -> {status}"
        return True, "ok"
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
        log.close()
        shutil.rmtree(storage, ignore_errors=True)


def main() -> int:
    requested = sys.argv[1:] or ["sqlite", "mysql", "mariadb", "pgsql"]
    failures = 0
    for index, name in enumerate(requested):
        if name not in BACKENDS:
            print(f"{name:8} skipped (unknown)")
            continue
        ok, message = run_backend(name, 8800 + index)
        print(f"{name:8} {'PASS' if ok else 'FAIL'} - {message}")
        failures += 0 if ok else 1
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
