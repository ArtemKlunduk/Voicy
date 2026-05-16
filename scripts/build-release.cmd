@echo off
REM Build voicy under MSVC target.
REM
REM Если рядом есть .creds/build-credentials.env — оттуда подцепятся
REM VOICY_BUILD_API_ID/HASH и build.rs запечёт их в exe как fallback creds.
REM Файл .creds/ в gitignore — никогда не попадёт на GitHub.
REM
REM Без .creds/build-credentials.env exe соберётся без embedded creds,
REM юзеру придётся вписать свои в voicy.toml.
setlocal

set MODE=%1
if "%MODE%"=="" set MODE=release

REM Корень репо = parent каталога этого .cmd скрипта
set REPO_ROOT=%~dp0..
set CREDS_FILE=%REPO_ROOT%\.creds\build-credentials.env

REM ── Подцепить embedded credentials если файл существует ──
if exist "%CREDS_FILE%" (
    echo [build] Loading embedded credentials from .creds/build-credentials.env
    for /f "tokens=1,2 delims==" %%a in (%CREDS_FILE%) do (
        set %%a=%%b
    )
) else (
    echo [build] .creds/build-credentials.env not found — exe will be built WITHOUT embedded creds
)

call "D:\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
if errorlevel 1 (
    echo [build] FAIL: vcvarsall.bat failed
    exit /b 1
)

set CARGO_HOME=D:\rust\.cargo
set RUSTUP_HOME=D:\rust\.rustup
set CARGO_TARGET_DIR=D:\rust\target_voicy
set PATH=D:\rust\.cargo\bin;%PATH%

cd /d "%REPO_ROOT%"

if "%MODE%"=="release" (
    cargo build --release
) else (
    cargo build
)

endlocal
