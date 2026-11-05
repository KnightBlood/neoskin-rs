# 构建 NeoSkinRS。
# - 使用工作区 tools 下的便携 OpenSSL（免安装）
# - 修复 Windows 上 Suprnova 符号链接被检出为文本文件的问题
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

$env:CARGO_NET_GIT_FETCH_WITH_CLI = 'true'
Push-Location $root
try {
    # 先 fetch，确保 cargo git checkout 已存在，再修复符号链接。
    & cargo fetch
    $checkouts = Join-Path $env:USERPROFILE '.cargo\git\checkouts'
    if (Test-Path $checkouts) {
        Get-ChildItem $checkouts -Directory -Filter 'suprnova-*' -ErrorAction SilentlyContinue |
            ForEach-Object {
                Get-ChildItem $_.FullName -Directory -ErrorAction SilentlyContinue |
                    ForEach-Object {
                        $checkout = $_.FullName
                        $link = Join-Path $checkout 'framework\templates\suprnova-ui'
                        $target = Join-Path $checkout 'crates\suprnova-live\components'
                        if ((Test-Path $link) -and
                            -not (Test-Path (Join-Path $link 'search-box\island.html')) -and
                            (Test-Path $target)) {
                            Remove-Item $link -Force -Recurse
                            cmd /c mklink /J "$link" "$target" | Out-Null
                            Write-Host "修复符号链接: $link -> $target"
                        }
                    }
            }
    }

    if ($Release) {
        cargo build --release
    } else {
        cargo build
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
