#!/usr/bin/env pwsh
# Собрать дистрибутив voicy-windows-x64-vX.Y.Z.zip из release-build'а.
#
# Что попадает в zip:
#   voicy.exe                — главный бинарь
#   WebView2Loader.dll       — нужен wry для embed-браузера
#   msvcp140.dll             — MSVC runtime, нужен для ort/parakeet
#   vcruntime140.dll          ↑
#   msvcp140_1.dll            ↑
#   vcruntime140_1.dll        ↑
#   voicy.toml.example       — шаблон конфига с placeholder'ами
#   README.txt               — quick-start инструкция
#   LICENSE                  — MIT

$ErrorActionPreference = "Stop"

# ── 1. Пути ─────────────────────────────────────────────────────────────────
$Root        = Split-Path -Parent $PSScriptRoot   # ../  →  D:\claude\voicy_rs
$ExeSource   = "D:\rust\target_voicy\x86_64-pc-windows-msvc\release\voicy.exe"
$AssetsDir   = Join-Path $Root "assets"
$DistRoot    = Join-Path $Root "dist"

# Версия из Cargo.toml
$CargoToml = Get-Content (Join-Path $Root "Cargo.toml") -Raw
if ($CargoToml -match 'version\s*=\s*"([\d.]+)"') {
    $Version = $Matches[1]
} else {
    Write-Host "ERROR: не нашёл version в Cargo.toml" -ForegroundColor Red
    exit 1
}

$ZipName     = "voicy-windows-x64-v$Version.zip"
$StagingDir  = Join-Path $DistRoot "voicy-v$Version"
$ZipPath     = Join-Path $DistRoot $ZipName

Write-Host "→ Voicy release packager v$Version" -ForegroundColor Cyan
Write-Host "  exe:     $ExeSource"
Write-Host "  output:  $ZipPath"
Write-Host ""

if (-not (Test-Path $ExeSource)) {
    Write-Host "ERROR: $ExeSource не существует. Собери: scripts\build-release.cmd" -ForegroundColor Red
    exit 1
}

# ── 2. Чистка + staging dir ─────────────────────────────────────────────────
if (Test-Path $StagingDir) { Remove-Item -Recurse -Force $StagingDir }
New-Item -ItemType Directory -Path $StagingDir | Out-Null
if (-not (Test-Path $DistRoot)) { New-Item -ItemType Directory -Path $DistRoot | Out-Null }

# ── 3. Копируем exe ─────────────────────────────────────────────────────────
Copy-Item $ExeSource (Join-Path $StagingDir "voicy.exe")
$ExeSize = (Get-Item (Join-Path $StagingDir "voicy.exe")).Length / 1MB
Write-Host ("  + voicy.exe ({0:N2} MB)" -f $ExeSize) -ForegroundColor Green

# ── 4. Копируем runtime DLL'ы ───────────────────────────────────────────────
$Dlls = @(
    "WebView2Loader.dll",
    "msvcp140.dll", "vcruntime140.dll",
    "msvcp140_1.dll", "vcruntime140_1.dll"
)
foreach ($dll in $Dlls) {
    $src = Join-Path $AssetsDir $dll
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $StagingDir $dll)
        Write-Host "  + $dll" -ForegroundColor Green
    } else {
        Write-Host "  ! $dll не найден в assets/" -ForegroundColor Yellow
    }
}

# ── 5. voicy.toml.example ───────────────────────────────────────────────────
$TomlExample = @'
# Voicy configuration file.
# Optional: place this file next to voicy.exe (rename from voicy.toml.example to voicy.toml)
# if you want to override defaults.
#
# Telegram app credentials are embedded in the official release build.
# You only need to fill [telegram] if you want to use your own Telegram app identity.
# Get your own credentials free at:
#   https://my.telegram.org → API development tools → Create application
# See docs/TELEGRAM_SETUP.md for the full walkthrough.

model = "parakeet-v3"            # ASR engine: parakeet-v3 / parakeet-v2 / tiny / base / small / medium / large-v3
recognition_language = "en"      # "en" / "ru"
ui_theme = "dark"                # "light" / "dark"
language = "en"                  # UI language: "en" / "ru"
ai_language = "en"               # AI assistant reply language
ai_assistant_enabled = true
ai_model = "qwen-0.5b"           # "qwen-0.5b" / "llama-3.2-1b" / "gemma-2-2b"
gemini_api_key = ""              # optional: https://aistudio.google.com/app/apikey for fast AI

[hotkey]
modifiers = ["alt"]
key = "x"

[telegram]
api_id = 0                       # optional: your api_id from my.telegram.org
api_hash = ""                    # optional: your api_hash
session = "voicy_session"
'@
Set-Content (Join-Path $StagingDir "voicy.toml.example") -Value $TomlExample -Encoding UTF8
Write-Host "  + voicy.toml.example" -ForegroundColor Green

# ── 6. README.txt ───────────────────────────────────────────────────────────
$Readme = @"
Voicy v$Version — Voice-to-Telegram for Windows
===============================================

QUICK START (5 minutes):

1. Double-click voicy.exe.

2. In the Settings UI → Telegram tab → click QR → scan with your phone.

3. Voicy stores your session, contacts cache, dialog cache, settings, and
   downloaded models locally on this PC. Do not share voicy_session.session.

4. Hold Alt+X and say "write <contact> hi" to test.

OPTIONAL:
  The official release includes Telegram app credentials. If you want to use
  your own Telegram app identity, rename voicy.toml.example → voicy.toml and
  fill the [telegram] section. See:
  https://github.com/ArtemKlunduk/Voicy/blob/main/docs/TELEGRAM_SETUP.md


DOCS:
  https://github.com/ArtemKlunduk/Voicy
  https://github.com/ArtemKlunduk/Voicy/blob/main/docs/INSTALL.md
  https://github.com/ArtemKlunduk/Voicy/blob/main/docs/USAGE.md

LICENSE: MIT (see LICENSE file)
"@
Set-Content (Join-Path $StagingDir "README.txt") -Value $Readme -Encoding UTF8
Write-Host "  + README.txt" -ForegroundColor Green

# ── 7. LICENSE ──────────────────────────────────────────────────────────────
Copy-Item (Join-Path $Root "LICENSE") (Join-Path $StagingDir "LICENSE")
Write-Host "  + LICENSE" -ForegroundColor Green

# ── 8. Создаём zip ──────────────────────────────────────────────────────────
if (Test-Path $ZipPath) { Remove-Item $ZipPath }
Compress-Archive -Path "$StagingDir\*" -DestinationPath $ZipPath -CompressionLevel Optimal
$ZipSize = (Get-Item $ZipPath).Length / 1MB
Write-Host ""
Write-Host ("✓ Готов: $ZipPath ({0:N2} MB)" -f $ZipSize) -ForegroundColor Cyan

# ── 9. Подсказка как залить в Releases ─────────────────────────────────────
Write-Host ""
Write-Host "Чтобы опубликовать в GitHub Releases:" -ForegroundColor Yellow
Write-Host '  $env:GH_TOKEN = "ghp_..."   # tuwulalo PAT с repo scope'
Write-Host "  gh release create v$Version $ZipPath --repo ArtemKlunduk/Voicy --title 'Voicy v$Version' --notes 'See CHANGELOG.md'"
