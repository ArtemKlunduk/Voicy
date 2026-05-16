//! Build script — копирует runtime DLL'и рядом с собранным `voicy.exe`.
//!
//! Под `x86_64-pc-windows-gnullvm` (наш case) ряд крейтов линкуется
//! к Windows DLL'ам динамически. Чтобы exe запустился без установленных
//! на машине рантаймов, кладём DLL'и в bundle.
//!
//! Список:
//! - `WebView2Loader.dll`   — для wry/webview2-com-sys
//! - `msvcp140.dll`         — VC++ STL runtime, нужна для onnxruntime
//! - `vcruntime140.dll`     — VC++ C runtime
//! - `vcruntime140_1.dll`   — VC++ exception handling
//! - `msvcp140_1.dll`       — расширения STL
//!
//! Все они лежат в `assets/` и должны коммититься в репозиторий.

use std::env;
use std::fs;
use std::path::PathBuf;

const DLLS: &[&str] = &[
    "WebView2Loader.dll",
    "msvcp140.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "msvcp140_1.dll",
];

fn main() {
    for dll in DLLS {
        println!("cargo:rerun-if-changed=assets/{}", dll);
    }
    println!("cargo:rerun-if-changed=build.rs");

    // ── Embed Telegram API credentials ──────────────────────────────────
    // Если build запущен с VOICY_BUILD_API_ID / VOICY_BUILD_API_HASH (через
    // scripts/build-release.cmd, который читает .creds/build-credentials.env),
    // мы emit'им их как cargo:rustc-env → option_env! в config.rs подхватит
    // их через embedded_credentials().
    //
    // Без этих env vars (например при `cargo build` напрямую без cmd-скрипта)
    // exe собирается без embedded creds — юзеру придётся вписать в voicy.toml.
    println!("cargo:rerun-if-env-changed=VOICY_BUILD_API_ID");
    println!("cargo:rerun-if-env-changed=VOICY_BUILD_API_HASH");
    if let Ok(api_id) = env::var("VOICY_BUILD_API_ID") {
        if !api_id.trim().is_empty() {
            println!("cargo:rustc-env=VOICY_EMBEDDED_API_ID={}", api_id);
        }
    }
    if let Ok(api_hash) = env::var("VOICY_BUILD_API_HASH") {
        if !api_hash.trim().is_empty() {
            println!("cargo:rustc-env=VOICY_EMBEDDED_API_HASH={}", api_hash);
        }
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_os != "windows" || target_arch != "x86_64" {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let profile_dir = match out_dir.ancestors().nth(3) {
        Some(p) => p.to_path_buf(),
        None => {
            eprintln!("voicy/build.rs: can't derive profile dir from OUT_DIR");
            return;
        }
    };

    for dll in DLLS {
        let src = manifest_dir.join("assets").join(dll);
        if !src.exists() {
            eprintln!(
                "voicy/build.rs: assets/{} not found — пропускаю",
                dll
            );
            continue;
        }
        let dst = profile_dir.join(dll);
        let need_copy = match (fs::metadata(&src), fs::metadata(&dst)) {
            (Ok(s), Ok(d)) => s.modified().ok() > d.modified().ok() || s.len() != d.len(),
            (Ok(_), Err(_)) => true,
            _ => false,
        };
        if need_copy {
            if let Err(e) = fs::copy(&src, &dst) {
                eprintln!("voicy/build.rs: copy {} → {} failed: {}", src.display(), dst.display(), e);
            } else {
                eprintln!("voicy/build.rs: copied {} → {}", dll, profile_dir.display());
            }
        }
    }
}
