@echo off
REM Build voicy under MSVC target.
setlocal

set MODE=%1
if "%MODE%"=="" set MODE=release

call "D:\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
if errorlevel 1 (
    echo [build-msvc] FAIL: vcvarsall.bat failed
    exit /b 1
)

set CARGO_HOME=D:\rust\.cargo
set RUSTUP_HOME=D:\rust\.rustup
set CARGO_TARGET_DIR=D:\rust\target_voicy
set PATH=D:\rust\.cargo\bin;%PATH%

cd /d "%~dp0"

if "%MODE%"=="release" (
    cargo build --release
) else (
    cargo build
)

endlocal
