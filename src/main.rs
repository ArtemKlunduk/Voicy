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
use tracing::{info, warn};

mod ai_assistant;
mod asr;
mod gemini;
mod audio;
mod browser;
mod browser_action;
mod config;
mod contacts;
mod hotkey;
mod intent_ai;
#[cfg(windows)]
mod native_overlay;
mod parakeet;
#[cfg(windows)]
mod startup;
mod telegram;
mod tts;
mod ui;
mod youtube_internal;
mod music;

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
    if old_models.exists() && old_models.is_dir() && !new_models.exists() {
        if let Err(e) = copy_dir_all(&old_models, &new_models) {
            warn!("[migrate] failed to copy models dir: {}", e);
        } else {
            info!("[migrate] copied models {} → {}", old_models.display(), new_models.display());
        }
    }

    // Миграция папки whisper (whisper-cli + ggml-модели)
    let old_whisper = exe_dir.join("whisper");
    let new_whisper = new_dir.join("whisper");
    if old_whisper.exists() && old_whisper.is_dir() && !new_whisper.exists() {
        if let Err(e) = copy_dir_all(&old_whisper, &new_whisper) {
            warn!("[migrate] failed to copy whisper dir: {}", e);
        } else {
            info!("[migrate] copied whisper {} → {}", old_whisper.display(), new_whisper.display());
        }
    }
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Спросить ИИ-ассистента. Локальная модель — первый выбор (нет API-ключей).
/// Gemini используется только если пользователь явно настроил ключ
/// и локальная модель недоступна/упала.
pub(crate) fn ask_ai_assistant(
    cfg: &config::Config,
    ai_slot: &Arc<Mutex<Option<ai_assistant::AiAssistant>>>,
    question: &str,
) -> anyhow::Result<String> {
    // 1. Локальная модель (офлайн, не требует ключей — основной путь для массового пользователя)
    let kind = ai_assistant::AiModel::from_config_str(&cfg.ai_model);
    let mut ai_guard = ai_slot.lock();
    if let Some(ref a) = *ai_guard {
        if a.kind() != kind {
            info!("[ai] config changed: {} → {}, reloading", a.kind().id(), kind.id());
            *ai_guard = None;
        }
    }
    if ai_guard.is_none() {
        if !ai_assistant::AiAssistant::is_model_cached(kind) {
            info!("[ai] локальная модель {} не найдена, скачиваем…", kind.id());
            ai_assistant::AiAssistant::download_model_sync(kind)
                .map_err(|e| anyhow::anyhow!("download: {}", e))?;
        }
        let assistant = ai_assistant::AiAssistant::load_if_cached(kind)
            .map_err(|e| anyhow::anyhow!("load: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("model not available after download"))?;
        *ai_guard = Some(assistant);
    }

    if let Some(ref mut assistant) = *ai_guard {
        let system = ai_assistant::system_prompt(&cfg.ai_language);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assistant.ask(&system, question)
        }));
        match result {
            Ok(Ok(response)) => {
                info!("[ai] local ответ: {} ({} tok/s)", response.text, response.tokens_per_second);
                return Ok(response.text);
            }
            Ok(Err(e)) => {
                warn!("[ai] local inference failed: {}", e);
            }
            Err(panic_payload) => {
                *ai_guard = None;
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".into()
                };
                warn!("[ai] local LLM panic caught: {}", msg);
            }
        }
    }

    // 2. Fallback: Gemini API (только если пользователь явно настроил ключ)
    if !cfg.gemini_api_key.is_empty() {
        warn!("[ai] falling back to Gemini API");
        match gemini::ask(&cfg.gemini_api_key, question, &cfg.ai_language) {
            Ok(text) => {
                info!("[ai] Gemini ответ: {}", text);
                return Ok(text);
            }
            Err(e) => {
                warn!("[ai] Gemini also failed: {}", e);
            }
        }
    }

    Err(anyhow::anyhow!(
        "ИИ-ассистент недоступен. Убедись, что модель скачалась (Settings → AI Model), либо настрой Gemini API key."
    ))
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
        other => {
            eprintln!("unknown command: {}", other);
            eprintln!("usage: voicy [info|record <s>|run|model download <name>|transcribe <wav>]");
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
    fn hide_overlay() {
        #[cfg(windows)]
        native_overlay::send(native_overlay::State::Hidden);
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

    // ИИ-ассистент: ленивая загрузка (только при первом использовании)
    let ai_assistant: Arc<Mutex<Option<ai_assistant::AiAssistant>>> = Arc::new(Mutex::new(None));
    let ai_assistant_thr = ai_assistant.clone();

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

    // Состояние ИИ-ассистента: ждём вопрос после команды "дай ответ"
    let waiting_for_ai = Arc::new(AtomicBool::new(false));
    let waiting_for_ai_release = waiting_for_ai.clone();

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
    let ai_assistant_release = ai_assistant_thr.clone();
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
        let waiting = waiting_for_ai_release.clone();
        let ai = ai_assistant_release.clone();
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
            info!("[hotkey] 📝 «{}»", text);

            // --- ИИ-ассистент ---
            if cfg.ai_assistant_enabled {
                let ai_trigger = ["дай ответ", "ответь", "вопрос", "спроси",
                                    "give answer", "give an answer", "give me answer",
                                    "answer", "question", "ask"];
                // Нормализация ASR-текста: пунктуация → пробелы, схлопываем дубли.
                // Это ловит случаи когда whisper/parakeet выдаёт запятую или тире
                // между словами: «дай, ответ» → «дай ответ».
                let normalized: String = text
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == ' ' || c == '\t' { c } else { ' ' })
                    .collect();
                let text_lower: String = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

                // Проверяем: сказали ли триггер + вопрос сразу (одной фразой)
                let mut question_immediate: Option<String> = None;
                info!("[ai] checking triggers against: '{}'", text_lower);
                for trigger in &ai_trigger {
                    if let Some(pos) = text_lower.find(trigger) {
                        let after = text_lower[pos + trigger.len()..].trim();
                        info!("[ai] trigger matched: '{}' → after='{}'", trigger, after);
                        if !after.is_empty() {
                            question_immediate = Some(after.to_string());
                        }
                        break;
                    }
                }
                if question_immediate.is_none() {
                    info!("[ai] no immediate question found");
                }

                // Если триггер + вопрос сразу — отправляем без режима ожидания
                if let Some(question) = question_immediate {
                    info!("[ai] вопрос сразу: {}", question);
                    #[cfg(windows)]
                    native_overlay::send(native_overlay::State::AiThinking);

                    match ask_ai_assistant(&cfg, &ai, &question) {
                        Ok(response) => {
                            info!("[ai] ответ: {}", response);
                            #[cfg(windows)]
                            native_overlay::send(native_overlay::State::Success);
                            if let Err(e) = tts::speak_with_lang(&response, &cfg.ai_language) {
                                warn!("[ai] tts error: {}", e);
                            }
                            schedule_hide();
                        }
                        Err(e) => {
                            warn!("[ai] generation failed: {}", e);
                            show_error();
                            schedule_hide();
                        }
                    }
                    return;
                }

                // Если мы уже ждём вопрос — отправляем его в LLM
                if waiting.load(Ordering::Relaxed) {
                    waiting.store(false, Ordering::Relaxed);
                    #[cfg(windows)]
                    native_overlay::send(native_overlay::State::AiThinking);

                    match ask_ai_assistant(&cfg, &ai, &text) {
                        Ok(response) => {
                            info!("[ai] ответ: {}", response);
                            #[cfg(windows)]
                            native_overlay::send(native_overlay::State::Success);
                            if let Err(e) = tts::speak_with_lang(&response, &cfg.ai_language) {
                                warn!("[ai] tts error: {}", e);
                            }
                            schedule_hide();
                        }
                        Err(e) => {
                            warn!("[ai] generation failed: {}", e);
                            show_error();
                            schedule_hide();
                        }
                    }
                    return;
                }

                // Только триггер без вопроса — активируем режим ожидания
                if ai_trigger.iter().any(|t| text_lower.contains(t)) {
                    waiting.store(true, Ordering::Relaxed);
                    info!("[ai] активирован режим вопроса");
                    #[cfg(windows)]
                    native_overlay::send(native_overlay::State::AiListening);
                    schedule_hide();
                    return;
                }
            }

            // --- Видео-плеер управление (громче, тише, пауза, etc.) ---
            if let Some(action) = browser_action::parse(&text) {
                info!("[hotkey] browser_action → {:?}", action);
                browser_action::dispatch(action);
                show_success();
                schedule_hide();
                return;
            }

            // --- Включи X (универсальный: ordinal / YouTube / Music) ---
            if let Some(req) = browser::parse_play_request(&text) {
                match req {
                    browser::PlayRequest::Ordinal(idx) => {
                        info!("[hotkey] play_nth → index {}", idx);
                        match browser::play_nth_youtube_result(idx) {
                            Ok(url) => {
                                info!("[hotkey] opened {}", url);
                                show_success();
                            }
                            Err(e) => {
                                warn!("[hotkey] play_nth: {}", e);
                                show_error();
                            }
                        }
                    }
                    browser::PlayRequest::YouTube(kws) => {
                        info!("[hotkey] play_by_title (YouTube) → {:?}", kws);
                        match browser::play_youtube_by_title(&kws) {
                            Ok(url) => {
                                info!("[hotkey] opened {}", url);
                                show_success();
                            }
                            Err(e) => {
                                warn!("[hotkey] play_by_title: {}", e);
                                show_error();
                            }
                        }
                    }
                    browser::PlayRequest::Music(kws) => {
                        let query = kws.join(" ");
                        info!("[hotkey] play_music → {}", query);
                        match music::search_and_play(&query) {
                            Ok(track) => {
                                // В standalone-режиме нет WebView — открываем в браузере
                                let url = music::watch_url(&track.video_id);
                                if let Err(e) = browser::open(&url) {
                                    warn!("[hotkey] music open: {}", e);
                                    show_error();
                                } else {
                                    info!("[hotkey] opened {}", url);
                                    show_success();
                                }
                            }
                            Err(e) => {
                                warn!("[hotkey] music: {}", e);
                                show_error();
                            }
                        }
                    }
                }
                schedule_hide();
                return;
            }

            // --- Перейди на канал автора ---
            if browser::parse_go_to_channel(&text) {
                info!("[hotkey] go_to_channel");
                match browser::go_to_channel() {
                    Ok(url) => {
                        info!("[hotkey] opened {}", url);
                        show_success();
                    }
                    Err(e) => {
                        warn!("[hotkey] go_to_channel: {}", e);
                        show_error();
                    }
                }
                schedule_hide();
                return;
            }

            // --- Поисковые запросы (открой ютуб/google/tiktok/twitch) ---
            if let Some(cmd) = browser::parse(&text) {
                info!("[hotkey] browser → {}: {}", cmd.provider, cmd.query);
                if let Err(e) = browser::open(&cmd.url) {
                    warn!("[hotkey] browser open: {}", e);
                    show_error();
                } else {
                    show_success();
                }
                schedule_hide();
                return;
            }

            // --- AI intent fallback (Gemini) ---
            if !cfg.gemini_api_key.is_empty() {
                let ai_ctx = intent_ai::AiContext {
                    contact_names: contacts.keys().cloned().collect(),
                    has_recent_youtube: browser::last_youtube_query().is_some(),
                };
                match intent_ai::classify(&cfg.gemini_api_key, &text, &ai_ctx) {
                    Ok(intent_ai::Intent::VolumeUp { n }) => {
                        info!("[hotkey] AI intent → volume_up {}", n);
                        browser_action::dispatch(browser_action::BrowserAction::VolumeUp(n));
                        show_success();
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::VolumeDown { n }) => {
                        info!("[hotkey] AI intent → volume_down {}", n);
                        browser_action::dispatch(browser_action::BrowserAction::VolumeDown(n));
                        show_success();
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::Fullscreen) => {
                        info!("[hotkey] AI intent → fullscreen");
                        browser_action::dispatch(browser_action::BrowserAction::Fullscreen);
                        show_success();
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::PlayPause) => {
                        info!("[hotkey] AI intent → play_pause");
                        browser_action::dispatch(browser_action::BrowserAction::PlayPause);
                        show_success();
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::SeekForward { n }) => {
                        info!("[hotkey] AI intent → seek_forward {}", n);
                        browser_action::dispatch(browser_action::BrowserAction::SeekForward(n));
                        show_success();
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::SeekBackward { n }) => {
                        info!("[hotkey] AI intent → seek_backward {}", n);
                        browser_action::dispatch(browser_action::BrowserAction::SeekBackward(n));
                        show_success();
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::Mute) => {
                        info!("[hotkey] AI intent → mute");
                        browser_action::dispatch(browser_action::BrowserAction::Mute);
                        show_success();
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::OpenUrl { provider, query }) => {
                        info!("[hotkey] AI intent → open_url {}: {}", provider, query);
                        let url = match provider.as_str() {
                            "youtube" | "ютуб" => {
                                let enc = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
                                format!("https://www.youtube.com/results?search_query={}", enc)
                            }
                            "google" | "гугл" => {
                                let enc = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
                                format!("https://www.google.com/search?q={}", enc)
                            }
                            "tiktok" | "тикток" => {
                                let enc = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
                                format!("https://www.tiktok.com/search?q={}", enc)
                            }
                            "twitch" | "твич" => {
                                let enc = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
                                format!("https://www.twitch.tv/search?term={}", enc)
                            }
                            _ => {
                                let enc = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
                                format!("https://www.google.com/search?q={}", enc)
                            }
                        };
                        if let Err(e) = browser::open(&url) {
                            warn!("[hotkey] AI open_url: {}", e);
                            show_error();
                        } else {
                            show_success();
                        }
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::PlayNth { index }) => {
                        info!("[hotkey] AI intent → play_nth {}", index);
                        match browser::play_nth_youtube_result(index) {
                            Ok(url) => {
                                info!("[hotkey] opened {}", url);
                                show_success();
                            }
                            Err(e) => {
                                warn!("[hotkey] AI play_nth: {}", e);
                                show_error();
                            }
                        }
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::PlayByTitle { keywords }) => {
                        info!("[hotkey] AI intent → play_by_title {:?}", keywords);
                        match browser::play_youtube_by_title(&keywords) {
                            Ok(url) => {
                                info!("[hotkey] opened {}", url);
                                show_success();
                            }
                            Err(e) => {
                                warn!("[hotkey] AI play_by_title: {}", e);
                                show_error();
                            }
                        }
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::GoToChannel) => {
                        info!("[hotkey] AI intent → go_to_channel");
                        match browser::go_to_channel() {
                            Ok(url) => {
                                info!("[hotkey] opened {}", url);
                                show_success();
                            }
                            Err(e) => {
                                warn!("[hotkey] AI go_to_channel: {}", e);
                                show_error();
                            }
                        }
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::SendTelegram { contact, message }) => {
                        info!("[hotkey] AI intent → send_telegram {}: {}", contact, message);
                        // Пытаемся найти контакт по имени из AI
                        let found_uid = contacts.get(&contact.to_lowercase()).copied();
                        if let Some(uid) = found_uid {
                            let res = rt.block_on(async {
                                telegram::send_message(&client, uid, &message).await
                            });
                            match res {
                                Ok(()) => {
                                    info!("[hotkey] ✅ → {} «{}»", uid, message);
                                    show_success();
                                }
                                Err(e) => {
                                    warn!("[hotkey] send: {}", e);
                                    show_error();
                                }
                            }
                        } else {
                            warn!("[hotkey] AI contact '{}' not found", contact);
                            show_error();
                        }
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::AskAi { question }) => {
                        info!("[hotkey] AI intent → ask_ai: {}", question);
                        #[cfg(windows)]
                        native_overlay::send(native_overlay::State::AiThinking);
                        match ask_ai_assistant(&cfg, &ai, &question) {
                            Ok(response) => {
                                info!("[ai] ответ: {}", response);
                                #[cfg(windows)]
                                native_overlay::send(native_overlay::State::Success);
                                if let Err(e) = tts::speak_with_lang(&response, &cfg.ai_language) {
                                    warn!("[ai] tts error: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("[ai] generation failed: {}", e);
                                show_error();
                            }
                        }
                        schedule_hide();
                        return;
                    }
                    Ok(intent_ai::Intent::Unknown) => {
                        // Не распознано AI — падаем в обычный parse (Telegram)
                    }
                    Err(e) => {
                        warn!("[hotkey] intent_ai classify error: {}", e);
                        // Падаем в обычный parse
                    }
                }
            }

            info!("[hotkey] trying contacts::parse_command with text='{}'", text);
            let (uid, message) = match contacts::parse_command(&text, &contacts) {
                Ok(x) => {
                    info!("[hotkey] contacts parsed: uid={} msg='{}'", x.0, x.1);
                    x
                }
                Err(e) => {
                    warn!("[hotkey] {}", e);
                    show_error();
                    schedule_hide();
                    return;
                }
            };
            if message.is_empty() {
                warn!("[hotkey] пустое сообщение");
                show_error();
                schedule_hide();
                return;
            }
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
fn schedule_hide() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(1800));
        #[cfg(windows)]
        native_overlay::send(native_overlay::State::Hidden);
    });
}

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
