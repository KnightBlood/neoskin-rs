# 启动 NeoSkinRS 开发服务器（含便携 OpenSSL 运行库）。
param(
    [switch]$Release
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$openssl = Join-Path (Split-Path -Parent $root) 'tools\openssl-3.5.6\x64'
if (Test-Path $openssl) {
    $env:OPENSSL_DIR = $openssl
    $env:PATH = "$openssl\bin;$env:PATH"
}

Push-Location $root
try {
    if ($Release) {
        cargo run --release --bin neoskin_rs
    } else {
        cargo run --bin neoskin_rs
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
