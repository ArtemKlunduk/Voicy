//! Voicy — Telegram voice helper (Rust port).
//! CLI:
//!   voicy run        — главный режим: слушает hotkey, пишет аудио
//!   voicy record N   — записать N сек микрофона (диагностика)
//!   voicy info       — показать конфиг и устройства
//!   voicy            — то же что info

// В release-сборке отключаем консольное окно — иначе при двойном клике
// по .exe всплывает чёрный CMD. В debug оставляем чтобы видеть tracing-логи.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

#[cfg(windows)]
mod active_url;
mod asr;
mod audio;
mod config;
mod contacts;
mod hotkey;
mod music_index;
#[cfg(windows)]
mod native_overlay;
mod parakeet;
#[cfg(windows)]
mod startup;
mod telegram;
#[cfg(windows)]
mod typing;
mod ui;

/// Переносит данные из старых путей (рядом с exe) в новые (%APPDATA%/voicy).
fn migrate_legacy_data() {
    let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) else {
        return;
    };
    let new_dir = dirs::data_dir()
        .map(|d| d.join("voicy"))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&new_dir);

    // Миграция файлов
    let files = [
        ("voicy_session.session", "session"),
        ("contacts.txt", "contacts"),
        ("voicy_dialogs.cache", "dialog cache"),
    ];
    for (name, label) in files {
        let old = exe_dir.join(name);
        let new = new_dir.join(name);
        if old.exists() && !new.exists() {
            if let Err(e) = std::fs::copy(&old, &new) {
                warn!("[migrate] failed to copy {}: {}", label, e);
            } else {
                info!("[migrate] copied {} → {}", old.display(), new.display());
            }
        }
    }

    // Миграция папки models (Parakeet, Canary и др.)
    let old_models = exe_dir.join("models");
    let new_models = new_dir.join("models");
    // Раньше тут стоял `&& !new_models.exists()` — и если папка models уже
    // существовала (даже с пустым/частичным parakeet-v3), миграция целиком
    // пропускалась, а ASR молча падал на whisper. Теперь дозаполняем
    // недостающие файлы даже в уже существующей папке.
    if old_models.exists() && old_models.is_dir() {
        match copy_dir_missing(&old_models, &new_models) {
            Ok(()) => info!("[migrate] models dir synced {} → {}", old_models.display(), new_models.display()),
            Err(e) => warn!("[migrate] failed to sync models dir: {}", e),
        }
    }

    // Миграция папки whisper (whisper-cli + ggml-модели)
    let old_whisper = exe_dir.join("whisper");
    let new_whisper = new_dir.join("whisper");
    if old_whisper.exists() && old_whisper.is_dir() {
        match copy_dir_missing(&old_whisper, &new_whisper) {
            Ok(()) => info!("[migrate] whisper dir synced {} → {}", old_whisper.display(), new_whisper.display()),
            Err(e) => warn!("[migrate] failed to sync whisper dir: {}", e),
        }
    }
}

/// Рекурсивно копирует из `src` в `dst` только НЕДОСТАЮЩИЕ или отличающиеся по
/// размеру файлы. Безопасно вызывать когда `dst` уже существует (например, есть
/// пустая/частичная папка models): дозаполняет пропуски, не затирая актуальные
/// файлы и не перекопируя сотни МБ на каждом старте.
fn copy_dir_missing(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_missing(&src_path, &dst_path)?;
        } else {
            // Копируем если файла нет или его размер отличается (битая/обрезанная копия).
            let src_len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let need = match std::fs::metadata(&dst_path) {
                Ok(m) => m.len() != src_len,
                Err(_) => true,
            };
            if need {
                std::fs::copy(&src_path, &dst_path)?;
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    init_logging();
    migrate_legacy_data();
    // ort load-dynamic ищет onnxruntime.dll по ORT_DYLIB_PATH.
    // Указываем на DLL рядом с exe (или ".\\onnxruntime.dll" fallback).
    let ort_path = asr::onnxruntime_dll_path();
    std::env::set_var("ORT_DYLIB_PATH", ort_path.as_os_str());
    // Жёстко CPU-only — отрубаем автодиск GPU EP (DirectML/CUDA/etc),
    // их discovery иногда блокирует первую инициализацию ORT.
    std::env::set_var("ORT_ACCELERATOR", "cpu");
    // Также добавляем папку exe в PATH — некоторые зависимости onnxruntime.dll
    // (msvcp140/vcruntime140) грузятся по PATH, не из dir рядом с dll.
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let old_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{};{}", exe_dir.display(), old_path);
        std::env::set_var("PATH", new_path);
        info!("[boot] exe_dir prepended to PATH: {}", exe_dir.display());
    }
    let args: Vec<String> = std::env::args().collect();
    // Без аргументов — открываем UI окно (а не печатаем info в консоль).
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ui");

    let cfg_path = config::default_path();
    let cfg = match config::Config::load(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            warn!("config load fail ({}): using defaults", e);
            let mut c = config::Config::default();
            // ENV vars применяются и к fresh-defaults — иначе CI/первый запуск
            // не сможет передать api_id без файла.
            c.apply_env_overrides();
            c
        }
    };
    if !cfg.has_telegram_credentials() {
        warn!("⚠ {}", config::Config::credentials_setup_hint());
    }

    // Активируем (возможно отредактированные) списки команд-триггеров для парсера.
    contacts::set_commands(cfg.commands.clone());

    #[cfg(windows)]
    startup::sync_with_config(cfg.startup_launch);

    match cmd {
        "info" => cmd_info(&cfg, &cfg_path),
        "record" => {
            let secs: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5.0);
            cmd_record(secs)
        }
        "run" => cmd_run(cfg),
        "model" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let name = args.get(3).map(|s| s.as_str()).unwrap_or(&cfg.model);
            cmd_model(sub, name)
        }
        "transcribe" => {
            let wav = args.get(2).ok_or_else(|| anyhow::anyhow!("usage: voicy transcribe <wav>"))?;
            cmd_transcribe(wav, &cfg)
        }
        "ui" => ui::run(cfg, cfg_path.clone()),
        "ui-test" => ui::run_test(),
        "setup" => cmd_setup_tg(&cfg),
        "send" => {
            let uid: i64 = args.get(2).and_then(|s| s.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("usage: voicy send <uid> <text>"))?;
            let text = args.iter().skip(3).cloned().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                anyhow::bail!("usage: voicy send <uid> <text>");
            }
            cmd_send_tg(&cfg, uid, &text)
        }
        "parse" => {
            let text = args.iter().skip(2).cloned().collect::<Vec<_>>().join(" ");
            cmd_parse(&text)
        }
        "type" => {
            // Диагностика режима диктовки: печатает текст в активное окно через
            // Win32 SendInput (тот же путь, что и диктовка из listener'а).
            let text = args.iter().skip(2).cloned().collect::<Vec<_>>().join(" ");
            #[cfg(windows)]
            {
                println!("typing «{}» in 2s — переключись в целевое окно…", text);
                std::thread::sleep(std::time::Duration::from_secs(2));
                typing::type_text(&text);
            }
            Ok(())
        }
        "download" => {
            let url = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("usage: voicy download <url> [mp3|wav]")
            })?;
            let format = args.get(3).map(|s| s.as_str()).unwrap_or(&cfg.download_format);
            cmd_download(&cfg, url, format)
        }
        "play" => {
            let query = args.iter().skip(2).cloned().collect::<Vec<_>>().join(" ");
            cmd_play(&cfg, &query)
        }
        "match" => {
            let query = args.iter().skip(2).cloned().collect::<Vec<_>>().join(" ");
            cmd_match(&cfg, &query)
        }
        "reindex" => cmd_reindex(&cfg),
        "url" => {
            // Диагностика захвата ссылки активной вкладки: UIA адресной строки
            // с fallback на буфер. Foreground-окно читается в момент захвата,
            // поэтому даём паузу переключиться на браузер (как у `voicy type`).
            // Аргумент: число секунд задержки (по умолчанию 3, 0 = сразу).
            #[cfg(windows)]
            {
                let arg = args.get(2).map(|s| s.as_str()).unwrap_or("");
                if arg == "debug" {
                    let hwnd = args.get(3).and_then(|s| s.parse::<isize>().ok());
                    print!("{}", active_url::dump_foreground(hwnd));
                } else {
                    let delay: u64 = arg.parse().unwrap_or(3);
                    if delay > 0 {
                        println!("захват через {}s: переключись на вкладку браузера…", delay);
                        std::thread::sleep(std::time::Duration::from_secs(delay));
                    }
                    match active_url::active_url() {
                        Some(u) => println!("URL: {}", u),
                        None => println!("(ссылка не найдена: открой вкладку в браузере или скопируй URL в буфер)"),
                    }
                }
            }
            Ok(())
        }
        other => {
            eprintln!("unknown command: {}", other);
            eprintln!("usage: voicy [info|record <s>|run|model download <name>|transcribe <wav>|type <text>|url|download <url> [mp3|wav]|play <название>|reindex]");
            std::process::exit(2);
        }
    }
}

fn cmd_info(cfg: &config::Config, cfg_path: &PathBuf) -> Result<()> {
    println!("voicy {}", env!("CARGO_PKG_VERSION"));
    println!("  config:    {}", cfg_path.display());
    println!(
        "  hotkey:    {} + {}",
        cfg.hotkey.modifiers.join(" + "),
        cfg.hotkey.key
    );
    println!("  model:     {}", cfg.model);
    println!("  language:  {}", cfg.recognition_language);
    println!("  api_id:    {}", cfg.telegram.api_id);
    println!("  session:   {}", cfg.telegram.session);
    Ok(())
}

fn cmd_record(secs: f32) -> Result<()> {
    info!("recording for {:.1}s…", secs);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let join = std::thread::spawn(move || audio::record(stop_clone));
    std::thread::sleep(std::time::Duration::from_secs_f32(secs));
    stop.store(true, Ordering::Release);
    let samples = join.join().expect("audio thread panic")?;
    let out = PathBuf::from("test_capture.wav");
    audio::save_wav(&out, &samples).context("save wav")?;
    info!("saved {} samples → {}", samples.len(), out.display());
    Ok(())
}

/// Главный режим: hotkey-listener в foreground, overlay рендерится нативно
/// в отдельном треде через Win32 layered window (native_overlay).
fn cmd_run(cfg: config::Config) -> Result<()> {
    #[cfg(windows)]
    let _overlay_tx = native_overlay::start();

    // helper-обёртки чтобы не таскать cfg!(windows) в каждой ветке.
    fn show_recording() {
        #[cfg(windows)]
        native_overlay::send(native_overlay::State::Recording);
    }
    fn show_success() {
        #[cfg(windows)]
        native_overlay::send(native_overlay::State::Success);
    }
    fn show_error() {
        #[cfg(windows)]
        native_overlay::send(native_overlay::State::Error);
    }

    // Один tokio-runtime на жизнь процесса; в нём — один Telegram-client.
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime"),
    );
    let client = rt.block_on(async {
        let c = telegram::connect(&cfg).await.context("tg connect")?;
        if !telegram::is_signed_in(&c).await? {
            anyhow::bail!("не залогинен в Telegram. Запусти `voicy setup`");
        }
        Result::<_>::Ok(c)
    })?;
    let client = Arc::new(client);
    info!("[tg] клиент готов");

    // ── Фоновый keepalive + периодическое сохранение сессии ──
    {
        let client = client.clone();
        let rt = rt.clone();
        let cfg = cfg.clone();
        std::thread::spawn(move || {
            let mut tick = 0u32;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                tick += 1;

                // Keepalive: лёгкий ping чтобы TCP-соединение не умирало от idle
                let alive = rt.block_on(async {
                    client.is_authorized().await.unwrap_or(false)
                });
                if !alive {
                    warn!("[tg-keepalive] client lost authorization");
                }

                // Каждые 5 минут сохраняем сессию на диск
                if tick % 5 == 0 {
                    if let Err(e) = rt.block_on(async {
                        telegram::save_session(&client, &cfg).await
                    }) {
                        warn!("[tg-keepalive] save_session failed: {}", e);
                    } else {
                        info!("[tg-keepalive] session saved");
                    }
                }
            }
        });
    }

    // Graceful shutdown: при Ctrl+C сохраняем сессию перед выходом
    {
        let client = client.clone();
        let rt = rt.clone();
        let cfg = cfg.clone();
        if let Err(e) = ctrlc::set_handler(move || {
            info!("[shutdown] Ctrl+C received, saving session…");
            let _ = rt.block_on(async {
                telegram::save_session(&client, &cfg).await
            });
            std::process::exit(0);
        }) {
            warn!("[shutdown] ctrlc handler error: {}", e);
        }
    }

    let contacts_path = contacts::default_path();
    let contact_map = contacts::load(&contacts_path);
    info!(
        "[contacts] {} alias'ов из {}",
        contact_map.len(),
        contacts_path.display()
    );
    let contact_map = Arc::new(contact_map);

    // Прогрев ASR-модели в RAM если включено в настройках
    if cfg.preload_model {
        if let Some(meta) = asr::model_meta(&cfg.model) {
            if meta.engine == "nemo" && asr::model_is_downloaded(&cfg.model) {
                let model_dir = asr::nemo_model_dir(&cfg.model);
                info!("[preload] прогрев Parakeet {}…", cfg.model);
                if let Err(e) = parakeet::preload(&cfg.model, &model_dir) {
                    warn!("[preload] {}", e);
                } else {
                    info!("[preload] Parakeet готов в RAM");
                }
            }
        }
    }

    info!(
        "[hotkey] жду {} + {}",
        cfg.hotkey.modifiers.join(" + "),
        cfg.hotkey.key
    );

    let session: Arc<Mutex<Option<RecordingSession>>> = Arc::new(Mutex::new(None));
    let session_press = session.clone();

    let on_press = move || {
        let mut slot = session_press.lock();
        if slot.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thr = stop.clone();
        let handle = std::thread::spawn(move || audio::record(stop_thr));
        *slot = Some(RecordingSession { stop, handle });
        info!("[hotkey] ▶ запись");
        show_recording();
    };

    let session_release = session.clone();
    let capture_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("voicy_capture.wav")))
        .unwrap_or_else(|| PathBuf::from("voicy_capture.wav"));
    let cfg_thr = cfg.clone();
    let client_thr = client.clone();
    let rt_thr = rt.clone();
    let contacts_thr = contact_map.clone();
    let on_release = move || {
        let s = session_release.lock().take();
        let Some(s) = s else { return };
        s.stop.store(true, Ordering::Release);
        let res = s.handle.join();
        let cap = capture_path.clone();
        let cfg = cfg_thr.clone();
        let client = client_thr.clone();
        let rt = rt_thr.clone();
        let contacts = contacts_thr.clone();
        std::thread::spawn(move || {
            let samples = match res {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    warn!("[hotkey] record err: {}", e);
                    show_error();
                    schedule_hide();
                    return;
                }
                Err(_) => {
                    warn!("[hotkey] record panic");
                    show_error();
                    schedule_hide();
                    return;
                }
            };
            if let Err(e) = audio::save_wav(&cap, &samples) {
                warn!("[hotkey] save_wav: {}", e);
                show_error();
                schedule_hide();
                return;
            }
            let dur = samples.len() as f32 / audio::TARGET_RATE as f32;
            info!("[hotkey] ⏹ {:.2}s → ASR…", dur);

            let text = match asr::transcribe_wav(&cap, &cfg.model, &cfg.recognition_language) {
                Ok(t) => t,
                Err(e) => {
                    warn!("[hotkey] transcribe: {}", e);
                    show_error();
                    schedule_hide();
                    return;
                }
            };
            debug!("[hotkey] 📝 «{}»", text);

            // Единая маршрутизация через contacts::classify (как в GUI-listener):
            // диктовка (нет триггера) → печать в активное окно; иначе Telegram.
            let (uid, message) = match contacts::classify(&text, &contacts) {
                contacts::Utterance::Dictation(dictated) if cfg.dictation_enabled => {
                    info!("[hotkey] dictation → typing ({} символов)", dictated.chars().count());
                    #[cfg(windows)]
                    typing::type_text(&dictated);
                    show_success();
                    schedule_hide();
                    return;
                }
                contacts::Utterance::Telegram { uid, message } => (uid, message),
                contacts::Utterance::Download { format, dest, to_channel } => {
                    // «Скачай это [в канал] [и скинь <контакт>]»: URL активной вкладки
                    // → бот → пересылка. Код уже в spawned-треде, долгое ожидание
                    // бота (до 120 c) не блокирует hotkey-листенер.
                    #[cfg(windows)]
                    {
                        let url = match active_url::active_url() {
                            Some(u) => u,
                            None => {
                                warn!("[hotkey] download: не нашёл URL активной вкладки");
                                show_error();
                                schedule_hide();
                                return;
                            }
                        };
                        // «в канал» → в music_source (контакт игнорируем); иначе
                        // контакт/music_dest как раньше.
                        let (dest_str, dest_override) = if to_channel {
                            if cfg.music_source.trim().is_empty() {
                                warn!("[hotkey] download в канал: music_source не задан");
                                show_error();
                                schedule_hide();
                                return;
                            }
                            (cfg.music_source.clone(), None)
                        } else {
                            (cfg.music_dest.clone(), dest)
                        };
                        let fmt = format.unwrap_or_else(|| cfg.download_format.clone());
                        info!("[hotkey] download «{}» fmt={} channel={} dest={:?}", url, fmt, to_channel, dest_override);
                        let res = rt.block_on(telegram::download_via_bot(
                            &client, &url, &fmt, &cfg.cloudpull_bot, &dest_str, dest_override,
                        ));
                        match res {
                            Ok(()) => {
                                info!("[hotkey] ✅ музыка отправлена");
                                show_success();
                            }
                            Err(e) => {
                                warn!("[hotkey] download: {}", e);
                                show_error();
                            }
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = (format, dest, to_channel);
                        show_error();
                    }
                    schedule_hide();
                    return;
                }
                contacts::Utterance::Play { query } => {
                    // «Включи <песня>»: найти трек в музыкальном канале и переслать.
                    info!("[hotkey] play «{}»", query);
                    match rt.block_on(telegram::play_track(
                        &client, &cfg.music_source, &query, &cfg.music_dest, false,
                    )) {
                        Ok(title) => {
                            info!("[hotkey] ▶ включено: {}", title);
                            show_success();
                        }
                        Err(e) => {
                            warn!("[hotkey] play: {}", e);
                            show_error();
                        }
                    }
                    schedule_hide();
                    return;
                }
                _ => {
                    warn!("[hotkey] не распознано как команда отправки");
                    show_error();
                    schedule_hide();
                    return;
                }
            };
            // SELF sentinel → Saved Messages (chat with own user_id).
            let resolved_uid = if uid == contacts::SELF_SENTINEL_UID {
                match rt.block_on(async { client.get_me().await }) {
                    Ok(me) => me.id(),
                    Err(e) => {
                        warn!("[hotkey] get_me для SELF: {}", e);
                        show_error();
                        schedule_hide();
                        return;
                    }
                }
            } else { uid };
            let uid = resolved_uid;
            let res = rt.block_on(async { telegram::send_message(&client, uid, &message).await });
            match res {
                Ok(()) => {
                    info!("[hotkey] ✅ → {} «{}»", uid, message);
                    show_success();
                    schedule_hide();
                }
                Err(e) => {
                    warn!("[hotkey] send: {}", e);
                    show_error();
                    schedule_hide();
                }
            }
        });
    };

    // hotkey-листенер блокирующий — выполняем на главном треде.
    hotkey::listen_blocking(cfg.hotkey.clone(), on_press, on_release);
    Ok(())
}

/// Через 1.8 секунды скрыть overlay.
/// Скрытие overlay теперь выполняет сам overlay-тред (auto-hide терминальных
/// состояний Success/Error через AUTO_HIDE_MS). Оставлено пустым, чтобы не
/// трогать все вызовы и не вернуть гонку, когда устаревший таймер скрывал
/// оверлей прямо во время следующей записи. Recording держится до смены состояния.
fn schedule_hide() {}

fn cmd_model(sub: &str, name: &str) -> Result<()> {
    match sub {
        "download" => {
            asr::ensure_whisper_cli()?;
            let p = asr::download_model(name)?;
            println!("downloaded: {}", p.display());
            Ok(())
        }
        "path" => {
            println!("{}", asr::model_path(name).display());
            Ok(())
        }
        "setup" => {
            let cli = asr::ensure_whisper_cli()?;
            println!("whisper-cli: {}", cli.display());
            Ok(())
        }
        _ => {
            eprintln!("usage: voicy model [setup|download <name>|path <name>]");
            std::process::exit(2);
        }
    }
}

fn cmd_transcribe(wav: &str, cfg: &config::Config) -> Result<()> {
    let path = std::path::Path::new(wav);
    let text = asr::transcribe_wav(path, &cfg.model, &cfg.recognition_language)?;
    println!("{}", text);
    Ok(())
}

fn cmd_setup_tg(cfg: &config::Config) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let _client = telegram::interactive_login(cfg).await?;
        println!("\nГотово. Запусти `voicy run` чтобы листенер заработал.");
        Result::<()>::Ok(())
    })
}

fn cmd_send_tg(cfg: &config::Config, uid: i64, text: &str) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let client = telegram::connect(cfg).await?;
        if !telegram::is_signed_in(&client).await? {
            anyhow::bail!("не залогинен. Запусти `voicy setup`");
        }
        telegram::send_message(&client, uid, text).await?;
        info!("отправлено → {}", uid);
        Result::<()>::Ok(())
    })
}

/// Диагностика музыкального флоу: прогнать URL через бота напрямую, без голоса и
/// браузера. `voicy download <url> [mp3|wav]`. Шлёт боту `/<формат> <url>`, ждёт
/// файл и пересылает в music_dest (как продакшн-путь download_via_bot).
fn cmd_download(cfg: &config::Config, url: &str, format: &str) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let client = telegram::connect(cfg).await?;
        if !telegram::is_signed_in(&client).await? {
            anyhow::bail!("не залогинен. Запусти `voicy setup`");
        }
        println!(
            "→ бот @{}: /{} {}  (жду файл, пересылка в {})",
            cfg.cloudpull_bot.trim_start_matches('@'),
            format,
            url,
            if cfg.music_dest.trim().is_empty() { "Saved Messages" } else { &cfg.music_dest }
        );
        telegram::download_via_bot(&client, url, format, &cfg.cloudpull_bot, &cfg.music_dest, None).await?;
        println!("✅ готово");
        Result::<()>::Ok(())
    })
}

/// Диагностика «включи»: найти трек по запросу в музыкальном канале и переслать
/// в music_dest. `voicy play <название>`.
fn cmd_play(cfg: &config::Config, query: &str) -> Result<()> {
    if query.trim().is_empty() {
        anyhow::bail!("usage: voicy play <название>");
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async {
        let client = telegram::connect(cfg).await?;
        if !telegram::is_signed_in(&client).await? {
            anyhow::bail!("не залогинен. Запусти `voicy setup`");
        }
        let title =
            telegram::play_track(&client, &cfg.music_source, query, &cfg.music_dest, false).await?;
        println!("▶ {}", title);
        Result::<()>::Ok(())
    })
}

/// Диагностика матчинга «включи» без сети и пересылки: `voicy match <запрос>`.
/// Показывает лучший трек из кэша и его скор (даже если ниже порога).
fn cmd_match(cfg: &config::Config, query: &str) -> Result<()> {
    if query.trim().is_empty() {
        anyhow::bail!("usage: voicy match <запрос>");
    }
    match telegram::match_cached(&cfg.music_source, query) {
        Some((title, score, ok)) => {
            let mark = if ok { "✓ сыграет" } else { "✗ ниже порога" };
            println!("{}  score={:.3}  «{}»", mark, score, title);
        }
        None => println!("(нет кэша индекса, запусти `voicy reindex`)"),
    }
    Ok(())
}

/// Принудительно пересобрать индекс музыкального канала. `voicy reindex`.
fn cmd_reindex(cfg: &config::Config) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async {
        let client = telegram::connect(cfg).await?;
        if !telegram::is_signed_in(&client).await? {
            anyhow::bail!("не залогинен. Запусти `voicy setup`");
        }
        let n = telegram::reindex_music(&client, &cfg.music_source).await?;
        println!("проиндексировано {} треков из «{}»", n, cfg.music_source);
        Result::<()>::Ok(())
    })
}

/// Диагностика: прогнать текст через contacts::parse_command (продакшн-путь
/// голосового конвейера) и напечатать (uid, грамотно оформленное сообщение).
/// Без отправки, безопасно для прогона множества тестовых фраз.
fn cmd_parse(text: &str) -> Result<()> {
    let contacts = contacts::load(&contacts::default_path());
    match contacts::parse_command(text, &contacts) {
        Ok((uid, msg)) => {
            let target = if uid == contacts::SELF_SENTINEL_UID {
                "SELF (Saved Messages)".to_string()
            } else {
                uid.to_string()
            };
            println!("OK   uid={:<22} message=«{}»", target, msg);
        }
        Err(e) => println!("ERR  {}", e),
    }
    Ok(())
}

struct RecordingSession {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<Result<Vec<i16>>>,
}

fn init_logging() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("voicy=info,info"));

    // В release с windows_subsystem="windows" stderr ниоткуда не виден.
    // Поэтому всегда дублируем логи в файл `<appdata>/voicy/voicy.log`.
    // В debug logs идут в обычный stderr.
    let log_path = log_file_path();
    // Ротация по размеру на старте: если лог разросся (>5 МБ), отодвигаем его в
    // voicy.log.1 (один бэкап) и начинаем заново. Без этого voicy.log рос
    // безгранично (раздувался до сотен МБ) — логировались все IPC и тексты.
    const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
    if std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0) > MAX_LOG_BYTES {
        let backup = log_path.with_file_name("voicy.log.1");
        let _ = std::fs::remove_file(&backup);
        let _ = std::fs::rename(&log_path, &backup);
    }
    let file_writer = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();

    if let Some(f) = file_writer {
        let f = std::sync::Mutex::new(f);
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .compact()
            .with_writer(move || -> Box<dyn std::io::Write + Send> {
                Box::new(MutexWriter(f.lock().unwrap().try_clone().unwrap()))
            })
            .init();
        eprintln!("[boot] logging to: {}", log_path.display());
    } else {
        // fallback: stderr (для debug-сборок).
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .compact()
            .init();
    }

    // Глобальный panic-hook → дамп в `<appdata>/voicy/voicy_panic.log`.
    // Чтобы при следующем краше осталась полная трасса для разбора.
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let bt = std::backtrace::Backtrace::force_capture();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let msg = format!("[ts={}] PANIC: {}\nbacktrace:\n{}\n\n", ts, info, bt);
        eprintln!("{}", msg);
        // Пишем в %APPDATA%/voicy/voicy_panic.log (приоритет) и рядом с exe (fallback).
        let paths = [
            dirs::data_dir().map(|d| d.join("voicy").join("voicy_panic.log")),
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("voicy_panic.log"))),
        ];
        for p in paths.into_iter().flatten() {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
            {
                let _ = f.write_all(msg.as_bytes());
                break;
            }
        }
    }));
}

/// Путь к лог-файлу: %APPDATA%/voicy/voicy.log (fallback — рядом с exe).
fn log_file_path() -> PathBuf {
    let dir = dirs::data_dir()
        .map(|d| d.join("voicy"))
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("voicy.log")
}

/// Тонкий wrapper над `File` чтобы trait `MakeWriter` мог дать новые
/// handle'ы на каждый write — мы клонируем дескриптор через `try_clone`.
struct MutexWriter(std::fs::File);
impl std::io::Write for MutexWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.write(b) }
    fn flush(&mut self) -> std::io::Result<()> { self.0.flush() }
}
