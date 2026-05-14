//! Settings UI на wry. Окно с embedded HTML, IPC между Rust и JS.
//! Команды: info, models_list, model_download, model_activate,
//!          login_phone, login_code, login_password, logout,
//!          hotkey_set.

use crate::{asr, audio, config, contacts as cts, hotkey, telegram};
use anyhow::Result;
use grammers_client::Client;
use grammers_client::types::PasswordToken;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tao::dpi::LogicalSize;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use tracing::{info, warn};
use wry::WebViewBuilder;

const UI_HTML: &str = include_str!("ui.html");

/// Минимальный тест: HTML который пытается отправить IPC сразу на load.
/// Если в stderr появится `[ui-ipc] raw: TEST-HELLO` — IPC работает.
/// Если нет — wry не инжектит window.ipc и нужно искать обходной путь.
pub fn run_test() -> Result<()> {
    let event_loop = EventLoopBuilder::<UiLoopEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("voicy-ipc-test")
        .with_inner_size(LogicalSize::new(500.0, 300.0))
        .build(&event_loop)?;

    let html = r#"<!DOCTYPE html><html><body style="font:13px monospace; padding:20px">
<h3>IPC sanity test</h3>
<div id="status">loading…</div>
<button onclick="send()">send TEST-BUTTON</button>
<pre id="diag" style="background:#eee; padding:8px; font-size:11px; white-space:pre-wrap;"></pre>
<script>
const diag = document.getElementById('diag');
function log(s){ diag.textContent += s + '\n'; }
log('document ready');
log('window.ipc = ' + typeof window.ipc);
log('window.ipc.postMessage = ' + (window.ipc ? typeof window.ipc.postMessage : 'n/a'));
log('window.chrome = ' + typeof window.chrome);
log('window.chrome.webview = ' + (window.chrome ? typeof window.chrome.webview : 'n/a'));
function send() {
  log('--- send clicked ---');
  try { window.ipc.postMessage('TEST-BUTTON'); log('ipc.postMessage OK'); }
  catch(e) { log('ipc.postMessage err: ' + e); }
  try { window.chrome.webview.postMessage('TEST-BUTTON-CHROME'); log('chrome.webview OK'); }
  catch(e) { log('chrome.webview err: ' + e); }
}
// Auto-send on load:
try { window.ipc.postMessage('TEST-AUTOSEND'); log('auto ipc.postMessage sent'); }
catch(e) { log('auto ipc err: ' + e); }
document.getElementById('status').textContent = 'loaded';
</script>
</body></html>"#;

    let ipc_handler = |req: wry::http::Request<String>| {
        info!("[ui-ipc-test] received: {:?}", req.body());
    };

    let _webview = WebViewBuilder::new(&window)
        .with_html(html)
        .with_ipc_handler(ipc_handler)
        .with_devtools(true)
        .build()?;

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        if let Event::WindowEvent { event: tao::event::WindowEvent::CloseRequested, .. } = event {
            *flow = ControlFlow::Exit;
        }
    });
}

#[derive(Debug, Deserialize)]
struct Msg {
    id: String,
    cmd: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Default)]
struct LoginInProgress {
    phone: Option<String>,
    token: Option<grammers_client::types::LoginToken>,
    pwd_token: Option<PasswordToken>,
    /// QR-login URL (отображается как QR-картинка)
    qr_url: Option<String>,
    /// Финальный статус: "waiting" | "authorized" | "expired" | "2fa" | "error: ..."
    qr_status: Option<String>,
}

#[derive(Default)]
struct ListenerState {
    /// running — true когда листенер активен. Установка в false → rdev сам не остановится
    /// (он крутится в своём треде с Windows hook), но pipeline на release не пройдёт.
    running: Arc<AtomicBool>,
    /// тред с rdev::listen — есть только пока работаем. На stop тред сам не выйдет,
    /// но больше ничего не будет делать.
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Кастомные события главного event loop — для вызова evaluate_script
/// из любого треда без таскания WebView через !Send-границы.
#[derive(Debug, Clone)]
enum UiLoopEvent {
    EvalJs(String),
    /// Показать overlay в режиме записи (бары-эквалайзер)
    OverlayRecording,
    /// Показать ✓ галочку (успешная отправка)
    OverlaySuccess,
    /// Показать × крестик (ошибка)
    OverlayError,
    /// Скрыть overlay
    OverlayHide,
    /// Свернуть главное окно (кнопка в кастомном titlebar)
    WindowMinimize,
    /// Закрыть приложение
    WindowClose,
    /// Начать перетаскивание окна (mousedown по titlebar)
    WindowDrag,
}

/// Загрузить иконку из встроенного .ico и сконвертить в tao::window::Icon.
fn load_icon() -> Option<tao::window::Icon> {
    let bytes = include_bytes!("../assets/voicy.ico");
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Ico).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    tao::window::Icon::from_rgba(rgba.into_raw(), w, h).ok()
}

pub fn run(cfg: config::Config, cfg_path: PathBuf) -> Result<()> {
    let event_loop = EventLoopBuilder::<UiLoopEvent>::with_user_event().build();
    let icon = load_icon();
    // Окно — ровно по размеру внутренней карточки, без рамок и прозрачных полей,
    // чтобы не было артефактов WebView2 (белые зоны вокруг card'а).
    let mut wb = WindowBuilder::new()
        .with_title("voicy")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(720.0, 520.0))
        .with_resizable(true);
    if let Some(ico) = icon.clone() {
        wb = wb.with_window_icon(Some(ico));
    }
    let window = wb.build(&event_loop)?;

    // Overlay-окно — снизу по центру, поверх всех. Прозрачный фон —
    // halo rings + orb «плывут» над десктопом, как PNG с alpha.
    // Halo rings расходятся до scale(1.9) → 84*1.9 ≈ 160px. Берём 220×220,
    // чтобы кольца не клипались краями окна.
    let (screen_w, screen_h) = event_loop
        .primary_monitor()
        .map(|m| (m.size().width as i32, m.size().height as i32))
        .unwrap_or((1920, 1080));
    let ov_w = 220;
    let ov_h = 220;
    // Непрозрачное overlay-окно с фоном Paper. Прозрачность через WebView2
    // была ненадёжной (белый артефакт на ряде GPU), chroma-key через
    // WS_EX_LAYERED тоже не сработал (WebView2 рендерит через DirectComposition).
    let overlay_window = WindowBuilder::new()
        .with_title("voicy-overlay")
        .with_inner_size(LogicalSize::new(ov_w as f64, ov_h as f64))
        .with_position(tao::dpi::LogicalPosition::new(
            ((screen_w - ov_w) / 2) as f64,
            (screen_h - ov_h - 24) as f64,
        ))
        .with_decorations(false)
        .with_always_on_top(true)
        .with_resizable(false)
        .with_focused(false)
        .with_visible(false)
        .build(&event_loop)?;
    let overlay_webview = WebViewBuilder::new(&overlay_window)
        .with_html(crate::overlay::OVERLAY_HTML)
        .build()?;

    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );

    // Загружаем кэш диалогов с диска — мгновенно (без сети).
    let cache_path = telegram::dialog_cache_path();
    let cached_n = telegram::load_dialog_cache(&cache_path);
    if cached_n > 0 {
        info!("[boot] прелоад кэша диалогов: {} записей", cached_n);
    }

    let client_slot: Arc<Mutex<Option<Client>>> = Arc::new(Mutex::new(None));
    {
        let rt = rt.clone();
        let cfg = cfg.clone();
        let slot = client_slot.clone();
        rt.spawn(async move {
            if let Ok(c) = telegram::connect(&cfg).await {
                // На фоновом коннекте сразу обновляем auth snapshot (1 сетевой ход)
                // — все последующие cmd_info берут его из RAM моментально.
                let _ = telegram::refresh_auth_snapshot(&c).await;
                *slot.lock() = Some(c);
            }
        });
    }
    let login: Arc<Mutex<LoginInProgress>> = Arc::new(Mutex::new(LoginInProgress::default()));
    let listener: Arc<Mutex<ListenerState>> = Arc::new(Mutex::new(ListenerState::default()));

    let cfg_path_ipc = cfg_path.clone();
    let cfg_arc = Arc::new(Mutex::new(cfg));
    let rt_ipc = rt.clone();
    let client_ipc = client_slot.clone();
    let login_ipc = login.clone();
    let listener_ipc = listener.clone();
    let proxy = event_loop.create_proxy();
    let proxy_listener = proxy.clone();

    let ipc_handler = move |req: wry::http::Request<String>| {
        let body = req.into_body();
        info!("[ui-ipc] raw: {}", body);
        let msg: Msg = match serde_json::from_str(&body) {
            Ok(m) => m,
            Err(e) => {
                warn!("[ui-ipc] parse fail: {} | body={}", e, body);
                return;
            }
        };
        info!("[ui-ipc] cmd={} payload={}", msg.cmd, msg.payload);
        let cfg_path = cfg_path_ipc.clone();
        let cfg = cfg_arc.clone();
        let rt = rt_ipc.clone();
        let client = client_ipc.clone();
        let login = login_ipc.clone();
        let listener = listener_ipc.clone();
        let proxy_for_listener = proxy_listener.clone();
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let reply = dispatch(&msg, &cfg, &cfg_path, &rt, &client, &login, &listener, &proxy_for_listener);
            let reply_str = serde_json::to_string(&reply).unwrap_or_else(|_| "null".into());
            let js = format!(
                "window.voicyReply({}, {});",
                serde_json::to_string(&msg.id).unwrap(),
                reply_str
            );
            let _ = proxy.send_event(UiLoopEvent::EvalJs(js));
        });
    };

    let webview = WebViewBuilder::new(&window)
        .with_url("voicy://localhost/index.html")
        .with_custom_protocol("voicy".into(), |_req| {
            wry::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                // Разрешаем inline-скрипты (WebView2 по умолчанию для нестандартных схем
                // может ужесточать CSP).
                .header(
                    "Content-Security-Policy",
                    "default-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob:; \
                     script-src 'self' 'unsafe-inline' 'unsafe-eval'; \
                     style-src 'self' 'unsafe-inline'; \
                     connect-src *; img-src * data:",
                )
                .body(std::borrow::Cow::Borrowed(UI_HTML.as_bytes()))
                .unwrap()
        })
        .with_ipc_handler(ipc_handler)
        .with_devtools(true)
        .build()?;

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                window_id,
                event: tao::event::WindowEvent::CloseRequested,
                ..
            } => {
                // Закрытие главного окна — выход. Overlay не закрывается пользователем
                // (он без декораций), но на всякий случай — игнорим.
                if window_id == window.id() {
                    *flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(ev) => match ev {
                UiLoopEvent::EvalJs(js) => {
                    let _ = webview.evaluate_script(&js);
                }
                UiLoopEvent::OverlayRecording => {
                    let _ = overlay_window.set_visible(true);
                    let _ = overlay_webview
                        .evaluate_script("window.voicySet && window.voicySet('recording')");
                }
                UiLoopEvent::OverlaySuccess => {
                    let _ = overlay_window.set_visible(true);
                    let _ = overlay_webview
                        .evaluate_script("window.voicySet && window.voicySet('success')");
                }
                UiLoopEvent::OverlayError => {
                    let _ = overlay_window.set_visible(true);
                    let _ = overlay_webview
                        .evaluate_script("window.voicySet && window.voicySet('error')");
                }
                UiLoopEvent::OverlayHide => {
                    let _ = overlay_webview
                        .evaluate_script("window.voicySet && window.voicySet('hide')");
                    let _ = overlay_window.set_visible(false);
                }
                UiLoopEvent::WindowMinimize => {
                    window.set_minimized(true);
                }
                UiLoopEvent::WindowClose => {
                    *flow = ControlFlow::Exit;
                }
                UiLoopEvent::WindowDrag => {
                    let _ = window.drag_window();
                }
            },
            _ => {}
        }
    });
}

fn err(msg: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": msg.into() })
}

fn dispatch(
    msg: &Msg,
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    login: &Arc<Mutex<LoginInProgress>>,
    listener: &Arc<Mutex<ListenerState>>,
    proxy: &EventLoopProxy<UiLoopEvent>,
) -> serde_json::Value {
    match msg.cmd.as_str() {
        "_init" => {
            info!("[ui-ipc] JS init ping received");
            serde_json::json!({"ok": true})
        }
        "_jserror" => {
            warn!("[ui-jserror] {}", msg.payload);
            serde_json::json!({"ok": true})
        }
        "info" => cmd_info(cfg, rt, client),
        "models_list" => cmd_models_list(cfg),
        "model_download" => {
            let name = msg.payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
            cmd_model_download(name)
        }
        "model_activate" => {
            let name = msg.payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
            cmd_model_activate(cfg, cfg_path, name)
        }
        "hotkey_set" => cmd_hotkey_set(cfg, cfg_path, &msg.payload),
        "login_phone" => {
            let phone = msg.payload.get("phone").and_then(|v| v.as_str()).unwrap_or("").to_string();
            cmd_login_phone(rt, client, login, cfg, phone)
        }
        "login_code" => {
            let code = msg.payload.get("code").and_then(|v| v.as_str()).unwrap_or("").to_string();
            cmd_login_code(rt, client, login, cfg, code)
        }
        "login_password" => {
            let pwd = msg.payload.get("password").and_then(|v| v.as_str()).unwrap_or("").to_string();
            cmd_login_password(rt, client, login, cfg, pwd)
        }
        "login_qr_start" => cmd_login_qr_start(rt, client, login, cfg),
        "login_qr_status" => cmd_login_qr_status(login),
        "logout" => cmd_logout(rt, client, cfg),
        "listener_start" => cmd_listener_start(cfg, rt, client, listener, proxy),
        "listener_stop" => cmd_listener_stop(listener),
        "listener_status" => cmd_listener_status(listener),
        "contacts_get" => cmd_contacts_get(),
        "contacts_save" => cmd_contacts_save(&msg.payload),
        "telegram_dialogs" => cmd_telegram_dialogs(rt, client),
        "theme_set" => cmd_theme_set(cfg, cfg_path, &msg.payload),
        "_window_close" => {
            let _ = proxy.send_event(UiLoopEvent::WindowClose);
            serde_json::json!({ "ok": true })
        }
        "_window_minimize" => {
            let _ = proxy.send_event(UiLoopEvent::WindowMinimize);
            serde_json::json!({ "ok": true })
        }
        "_window_drag" => {
            let _ = proxy.send_event(UiLoopEvent::WindowDrag);
            serde_json::json!({ "ok": true })
        }
        _ => err(format!("unknown cmd: {}", msg.cmd)),
    }
}

fn cmd_contacts_get() -> serde_json::Value {
    let path = cts::default_path();
    let list = cts::load_structured(&path);
    serde_json::json!({ "ok": true, "contacts": list, "path": path.display().to_string() })
}

fn cmd_contacts_save(payload: &serde_json::Value) -> serde_json::Value {
    let Some(arr) = payload.get("contacts").and_then(|v| v.as_array()) else {
        return err("missing contacts array");
    };
    let mut list: Vec<cts::Contact> = Vec::new();
    for item in arr {
        let Some(uid) = item.get("uid").and_then(|v| v.as_i64()) else { continue };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let aliases: Vec<String> = item
            .get("aliases")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        list.push(cts::Contact { uid, name, aliases });
    }
    let path = cts::default_path();
    match cts::save_structured(&path, &list) {
        Ok(_) => {
            info!("[contacts] сохранено {} контактов", list.len());
            serde_json::json!({ "ok": true, "saved": list.len() })
        }
        Err(e) => err(format!("save: {}", e)),
    }
}

fn cmd_theme_set(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let theme = payload.get("theme").and_then(|v| v.as_str()).unwrap_or("light");
    if theme != "light" && theme != "dark" {
        return err("theme must be 'light' or 'dark'");
    }
    let mut c = cfg.lock();
    c.ui_theme = theme.to_string();
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save: {}", e));
    }
    serde_json::json!({ "ok": true, "theme": theme })
}

fn cmd_telegram_dialogs(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
) -> serde_json::Value {
    let c = match client.lock().as_ref() {
        Some(c) => c.clone(),
        None => return err("Telegram client not connected"),
    };
    let res = rt.block_on(async {
        if !c.is_authorized().await.unwrap_or(false) {
            return Err(anyhow::anyhow!("не залогинен"));
        }
        telegram::list_dialogs(&c, 80).await
    });
    match res {
        Ok(list) => serde_json::json!({ "ok": true, "dialogs": list }),
        Err(e) => err(format!("dialogs: {}", e)),
    }
}

fn cmd_info(
    cfg: &Arc<Mutex<config::Config>>,
    _rt: &Arc<tokio::runtime::Runtime>,
    _client: &Arc<Mutex<Option<Client>>>,
) -> serde_json::Value {
    let cfg = cfg.lock().clone();
    let exe_size = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| (m.len() / 1024) as u64)
        .unwrap_or(0);

    // Auth-снапшот читается из RAM — никаких сетевых вызовов.
    // Снапшот наполняется на старте (background connect) и после login/logout.
    let snap = telegram::get_auth_snapshot();

    let cts_path = cts::default_path();
    let ctts = cts::load(&cts_path);
    let list: Vec<String> = ctts.iter().take(50).map(|(k, v)| format!("{}  →  {}", k, v)).collect();

    serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "exe_size_kb": exe_size,
        "hotkey": format!("{} + {}", cfg.hotkey.modifiers.join(" + "), cfg.hotkey.key),
        "model": cfg.model,
        "ui_theme": cfg.ui_theme,
        "signed_in": snap.signed_in,
        "user_id": snap.user_id,
        "username": snap.username,
        "contacts_count": ctts.len(),
        "contacts_list": list,
        "contacts_path": cts_path.display().to_string(),
    })
}

#[derive(Serialize)]
struct ModelEntry {
    name: String,
    family: String,
    variant: String,
    display: String,
    engine: String,
    size: String,
    lang: String,
    desc: String,
    hf_repo: Option<String>,
    downloaded: bool,
    active: bool,
    /// inference поддерживается этим движком в Rust-порте (пока только whisper)
    runnable: bool,
}

fn cmd_models_list(cfg: &Arc<Mutex<config::Config>>) -> serde_json::Value {
    let active = cfg.lock().model.clone();
    let models: Vec<ModelEntry> = asr::MODELS
        .iter()
        .map(|m| ModelEntry {
            name: m.name.to_string(),
            family: m.family.to_string(),
            variant: m.variant.to_string(),
            display: m.display.to_string(),
            engine: m.engine.to_string(),
            size: m.size.to_string(),
            lang: m.lang.to_string(),
            desc: m.desc.to_string(),
            hf_repo: m.hf_repo.map(String::from),
            downloaded: asr::model_is_downloaded(m.name),
            active: m.name == active,
            runnable: matches!(m.engine, "whisper" | "nemo"),
        })
        .collect();
    serde_json::json!({ "ok": true, "models": models })
}

fn cmd_model_download(name: &str) -> serde_json::Value {
    let Some(meta) = asr::model_meta(name) else {
        return err(format!("unknown model: {}", name));
    };
    // Whisper нуждается в whisper-cli.exe. Parakeet (nemo) — в onnxruntime.dll.
    if meta.engine == "whisper" {
        if let Err(e) = asr::ensure_whisper_cli() {
            return err(format!("whisper-cli: {}", e));
        }
    }
    if meta.engine == "nemo" {
        if let Err(e) = asr::ensure_onnxruntime() {
            return err(format!("onnxruntime.dll: {}", e));
        }
    }
    match asr::download_model(name) {
        Ok(p) => serde_json::json!({ "ok": true, "path": p.display().to_string() }),
        Err(e) => err(format!("download: {}", e)),
    }
}

fn cmd_model_activate(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    name: &str,
) -> serde_json::Value {
    let Some(meta) = asr::model_meta(name) else {
        return err(format!("unknown model: {}", name));
    };
    if !asr::model_is_downloaded(name) {
        return err(format!("модель {} не скачана", name));
    }
    let mut c = cfg.lock();
    c.model = name.to_string();
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save config: {}", e));
    }
    let note = if meta.engine != "whisper" {
        " (inference в Rust-порте — fallback на ближайший Whisper)"
    } else {
        ""
    };
    serde_json::json!({ "ok": true, "active": name, "note": note })
}

fn cmd_hotkey_set(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let mods: Vec<String> = payload
        .get("modifiers")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let key = payload.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if mods.is_empty() || key.is_empty() {
        return err("modifiers/key пустые");
    }
    let mut c = cfg.lock();
    c.hotkey.modifiers = mods;
    c.hotkey.key = key;
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save: {}", e));
    }
    info!("[ui] hotkey updated: {:?}", c.hotkey);
    serde_json::json!({ "ok": true })
}

fn cmd_login_phone(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    login: &Arc<Mutex<LoginInProgress>>,
    cfg: &Arc<Mutex<config::Config>>,
    phone: String,
) -> serde_json::Value {
    let cfg = cfg.lock().clone();
    let result: anyhow::Result<grammers_client::types::LoginToken> = rt.block_on(async {
        if client.lock().is_none() {
            let c = telegram::connect(&cfg).await?;
            *client.lock() = Some(c);
        }
        let c = client.lock().as_ref().unwrap().clone();
        let tok = c.request_login_code(&phone).await?;
        Ok(tok)
    });
    match result {
        Ok(tok) => {
            let mut lg = login.lock();
            lg.phone = Some(phone);
            lg.token = Some(tok);
            serde_json::json!({ "ok": true })
        }
        Err(e) => err(format!("phone: {}", e)),
    }
}

fn cmd_login_code(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    login: &Arc<Mutex<LoginInProgress>>,
    cfg: &Arc<Mutex<config::Config>>,
    code: String,
) -> serde_json::Value {
    let tok = match login.lock().token.take() {
        Some(t) => t,
        None => return err("login_phone сначала"),
    };
    let cfg = cfg.lock().clone();
    let c = match client.lock().as_ref() {
        Some(c) => c.clone(),
        None => return err("client not connected"),
    };
    let res = rt.block_on(async { c.sign_in(&tok, &code).await });
    match res {
        Ok(_user) => {
            let _ = rt.block_on(async {
                // get_me() сначала чтобы grammers закоммитил session state.
                telegram::refresh_auth_snapshot(&c).await;
                telegram::save_session(&c, &cfg).await?;
                Ok::<_, anyhow::Error>(())
            });
            serde_json::json!({ "ok": true })
        }
        Err(grammers_client::SignInError::PasswordRequired(pwd_tok)) => {
            login.lock().pwd_token = Some(pwd_tok);
            serde_json::json!({ "ok": false, "need_2fa": true })
        }
        Err(e) => err(format!("sign_in: {}", e)),
    }
}

fn cmd_login_password(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    login: &Arc<Mutex<LoginInProgress>>,
    cfg: &Arc<Mutex<config::Config>>,
    pwd: String,
) -> serde_json::Value {
    let pwd_tok = match login.lock().pwd_token.take() {
        Some(t) => t,
        None => return err("сначала login_code"),
    };
    let cfg = cfg.lock().clone();
    let c = match client.lock().as_ref() {
        Some(c) => c.clone(),
        None => return err("client not connected"),
    };
    let res = rt.block_on(async { c.check_password(pwd_tok, &pwd).await });
    match res {
        Ok(_) => {
            let _ = rt.block_on(async {
                telegram::refresh_auth_snapshot(&c).await;
                telegram::save_session(&c, &cfg).await?;
                Ok::<_, anyhow::Error>(())
            });
            serde_json::json!({ "ok": true })
        }
        Err(e) => err(format!("check_password: {}", e)),
    }
}

fn cmd_listener_status(listener: &Arc<Mutex<ListenerState>>) -> serde_json::Value {
    let running = listener.lock().running.load(Ordering::Acquire);
    serde_json::json!({ "ok": true, "running": running })
}

fn cmd_listener_stop(listener: &Arc<Mutex<ListenerState>>) -> serde_json::Value {
    let st = listener.lock();
    st.running.store(false, Ordering::Release);
    // rdev::listen блокирует тред навсегда — мы просто игнорируем его события,
    // пока running=false. Поток умрёт когда процесс завершится. Это acceptable.
    serde_json::json!({ "ok": true })
}

fn push_event(proxy: &EventLoopProxy<UiLoopEvent>, kind: &str, text: &str) {
    let js = format!(
        "window.voicyEvent && window.voicyEvent({}, {});",
        serde_json::to_string(kind).unwrap_or_default(),
        serde_json::to_string(text).unwrap_or_default(),
    );
    let _ = proxy.send_event(UiLoopEvent::EvalJs(js));
}

fn flash_overlay(proxy: &EventLoopProxy<UiLoopEvent>, ev: UiLoopEvent) {
    let _ = proxy.send_event(ev);
    let p = proxy.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1800));
        let _ = p.send_event(UiLoopEvent::OverlayHide);
    });
}

fn cmd_listener_start(
    cfg: &Arc<Mutex<config::Config>>,
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    listener: &Arc<Mutex<ListenerState>>,
    proxy: &EventLoopProxy<UiLoopEvent>,
) -> serde_json::Value {
    let mut cfg_c = cfg.lock().clone();

    // Active = whisper или nemo (Parakeet). transcribe_wav сама свалится на whisper если nemo не сработал.
    let active_ok = asr::model_meta(&cfg_c.model)
        .filter(|m| matches!(m.engine, "whisper" | "nemo") && asr::model_is_downloaded(m.name))
        .map(|m| m.name.to_string());
    let chosen = active_ok.or_else(|| {
        const PREFERRED: &[&str] = &[
            "parakeet-v3", "parakeet-v2",
            "large-v3", "turbo", "medium", "small", "base", "tiny",
        ];
        PREFERRED
            .iter()
            .find(|name| asr::model_is_downloaded(name))
            .map(|s| s.to_string())
    });
    let Some(inference_model) = chosen else {
        return err(
            "Для inference нужна скачанная модель. Открой выпадашку → Models, \
             выбери Parakeet V3 (лучше всего) или Whisper Base."
                .to_string(),
        );
    };

    // whisper-cli нужен только если inference пойдёт через whisper. Если
    // активный движок — Parakeet (nemo), whisper-cli может отсутствовать.
    // Проверка whisper-cli происходит уже в transcribe_wav_whisper, не тут.
    if inference_model != cfg_c.model {
        push_event(
            proxy,
            "log",
            &format!(
                "ℹ inference fallback: «{}» → «{}»",
                cfg_c.model, inference_model
            ),
        );
    }
    cfg_c.model = inference_model;

    // Login check — НЕ блокирует старт листенера.
    // Хоткей-перехват и запись должны работать всегда; отсутствие логина даст
    // понятную ошибку только на шаге отправки.
    let c_opt = client.lock().as_ref().cloned();
    let signed_in = if let Some(ref c) = c_opt {
        rt.block_on(async { c.is_authorized().await.unwrap_or(false) })
    } else {
        false
    };
    if !signed_in {
        push_event(
            proxy,
            "log",
            "⚠ listener запущен без Telegram-логина — запись и распознавание работают, но отправка упадёт. Жми Login.",
        );
    }

    // Прогреваем dialog-cache фоном если залогинены.
    // Оборачиваем в catch_unwind: grammers 0.7 иногда паникует
    // «tried to query self_id before it's known» если апдейт сервера
    // пришёл до set_self_user. Не хотим валить всё приложение.
    if let Some(c) = &c_opt {
        let c_warm = c.clone();
        let rt_warm = rt.clone();
        let proxy_warm = proxy.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rt_warm.block_on(telegram::warm_dialog_cache(&c_warm))
            }));
            match result {
                Ok(Ok(n)) => push_event(&proxy_warm, "log", &format!("ℹ кэш диалогов: {} записей", n)),
                Ok(Err(e)) => push_event(&proxy_warm, "log", &format!("⚠ warm cache: {}", e)),
                Err(_) => push_event(&proxy_warm, "log", "⚠ warm cache panicked (grammers bug) — пропустили"),
            }
        });
    }

    let mut st = listener.lock();
    if st.running.load(Ordering::Acquire) {
        return serde_json::json!({ "ok": true, "already": true });
    }
    let running = st.running.clone();
    running.store(true, Ordering::Release);
    let proxy_listener = proxy.clone();

    let recording: Arc<Mutex<Option<(Arc<AtomicBool>, std::thread::JoinHandle<anyhow::Result<Vec<i16>>>)>>> =
        Arc::new(Mutex::new(None));
    // Single-flight: пока pipeline крутится (запись→ASR→TG), новые Alt+X
    // игнорятся. Иначе нажав 5 раз подряд получаем 5 параллельных тредов
    // на один и тот же микрофон и кэш модели.
    let processing: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let rec_press = recording.clone();
    let running_press = running.clone();
    let processing_press = processing.clone();
    let proxy_press = proxy_listener.clone();
    let hotkey_label = format!(
        "{} + {}",
        cfg_c.hotkey.modifiers.join(" + "),
        cfg_c.hotkey.key
    );
    let on_press = move || {
        if !running_press.load(Ordering::Acquire) {
            return;
        }
        if processing_press.load(Ordering::Acquire) {
            info!("[listener] busy, ignoring Alt+X (previous still processing)");
            return;
        }
        let mut slot = rec_press.lock();
        if slot.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thr = stop.clone();
        let h = std::thread::spawn(move || audio::record(stop_thr));
        *slot = Some((stop, h));
        info!("[listener] ▶ запись");
        push_event(&proxy_press, "activity", "▶ запись…");
        push_event(&proxy_press, "log", &format!("▶ {} нажат — запись", hotkey_label));
        let _ = proxy_press.send_event(UiLoopEvent::OverlayRecording);
    };

    let rec_release = recording.clone();
    let running_release = running.clone();
    let processing_release = processing.clone();
    let cfg_thr = cfg_c.clone();
    let client_slot_thr = client.clone();
    let rt_thr = rt.clone();
    let proxy_release = proxy_listener.clone();
    let on_release = move || {
        if !running_release.load(Ordering::Acquire) {
            return;
        }
        let pair = rec_release.lock().take();
        let Some((stop, h)) = pair else { return };
        stop.store(true, Ordering::Release);
        let res = h.join();
        let cfg = cfg_thr.clone();
        let client_slot = client_slot_thr.clone();
        let rt = rt_thr.clone();
        let proxy = proxy_release.clone();
        push_event(&proxy, "activity", "⏹ распознаю…");
        let proxy_panic = proxy.clone();
        // Поднимаем флаг — следующие Alt+X будут игнорироваться пока этот
        // pipeline не завершится (или не упадёт в panic guard ниже).
        processing_release.store(true, Ordering::Release);
        let processing_done = processing_release.clone();
        std::thread::spawn(move || {
            // Внешний catch_unwind гарантирует что даже если pipeline где-то
            // паникует (например ParakeetModel::load c битыми весами),
            // overlay-бары не залипают — мы покажем ✗ крест и скроем.
            let pipeline = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let samples = match res {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    warn!("[listener] record: {}", e);
                    push_event(&proxy, "log", &format!("✗ record: {}", e));
                    push_event(&proxy, "activity", "");
                    flash_overlay(&proxy, UiLoopEvent::OverlayError);
                    return;
                }
                Err(_) => {
                    warn!("[listener] record panic");
                    push_event(&proxy, "activity", "");
                    flash_overlay(&proxy, UiLoopEvent::OverlayError);
                    return;
                }
            };
            let cap = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("voicy_capture.wav")))
                .unwrap_or_else(|| PathBuf::from("voicy_capture.wav"));
            if let Err(e) = audio::save_wav(&cap, &samples) {
                warn!("[listener] save_wav: {}", e);
                push_event(&proxy, "log", &format!("✗ save: {}", e));
                push_event(&proxy, "activity", "");
                flash_overlay(&proxy, UiLoopEvent::OverlayError);
                return;
            }
            let dur = samples.len() as f32 / audio::TARGET_RATE as f32;
            push_event(&proxy, "log", &format!("⏹ {:.2}s → whisper…", dur));

            let text = match asr::transcribe_wav(&cap, &cfg.model, &cfg.recognition_language) {
                Ok(t) => t,
                Err(e) => {
                    warn!("[listener] asr: {}", e);
                    push_event(&proxy, "log", &format!("✗ asr: {}", e));
                    push_event(&proxy, "activity", "");
                    flash_overlay(&proxy, UiLoopEvent::OverlayError);
                    return;
                }
            };
            info!("[listener] 📝 «{}» (через {})", text, cfg.model);
            push_event(&proxy, "log", &format!("📝 «{}» · engine: {}", text, cfg.model));

            let contacts = cts::load(&cts::default_path());
            let (uid, message) = match cts::parse_command(&text, &contacts) {
                Ok(x) => x,
                Err(e) => {
                    warn!("[listener] {}", e);
                    // Контакт не нашли → копируем распознанный текст в буфер
                    // обмена, чтобы пользователь мог вставить его куда хочет.
                    // Это спасает кейсы когда whisper-tiny исковеркал имя.
                    let to_copy = text.trim().to_string();
                    let copy_ok = arboard::Clipboard::new()
                        .and_then(|mut c| c.set_text(to_copy.clone()))
                        .is_ok();
                    if copy_ok {
                        push_event(&proxy, "log", &format!("✗ {} · текст в буфере: «{}»", e, to_copy));
                        push_event(&proxy, "activity", "✗ контакт не найден — текст в буфере");
                    } else {
                        push_event(&proxy, "log", &format!("✗ parse: {}", e));
                        push_event(&proxy, "activity", "");
                    }
                    flash_overlay(&proxy, UiLoopEvent::OverlayError);
                    return;
                }
            };
            if message.is_empty() {
                warn!("[listener] пустое сообщение");
                push_event(&proxy, "log", "✗ пустое сообщение");
                push_event(&proxy, "activity", "");
                flash_overlay(&proxy, UiLoopEvent::OverlayError);
                return;
            }
            push_event(&proxy, "activity", &format!("→ {} «{}»…", uid, message));
            let client = match client_slot.lock().as_ref() {
                Some(c) => c.clone(),
                None => {
                    warn!("[listener] нет Telegram-клиента — send skip");
                    push_event(&proxy, "log", "✗ не залогинен в Telegram — жми Login");
                    push_event(&proxy, "activity", "");
                    flash_overlay(&proxy, UiLoopEvent::OverlayError);
                    return;
                }
            };
            let res = rt.block_on(async { telegram::send_message(&client, uid, &message).await });
            match res {
                Ok(()) => {
                    info!("[listener] ✅ → {} «{}»", uid, message);
                    push_event(&proxy, "log", &format!("✅ → {} «{}»", uid, message));
                    push_event(&proxy, "activity", &format!("✅ → {}", uid));
                    flash_overlay(&proxy, UiLoopEvent::OverlaySuccess);
                }
                Err(e) => {
                    warn!("[listener] send: {}", e);
                    push_event(&proxy, "log", &format!("✗ send: {}", e));
                    push_event(&proxy, "activity", "");
                    flash_overlay(&proxy, UiLoopEvent::OverlayError);
                }
            }
            })); // end catch_unwind
            // Если pipeline паникнул — overlay всё ещё показывает бары.
            // Принудительно гасим красным крестом.
            if pipeline.is_err() {
                warn!("[listener] PIPELINE PANIC — гасим overlay");
                push_event(&proxy_panic, "log", "✗ pipeline panic (см. voicy_panic.log)");
                push_event(&proxy_panic, "activity", "");
                flash_overlay(&proxy_panic, UiLoopEvent::OverlayError);
            }
            // Снимаем флаг — следующие Alt+X снова работают.
            processing_done.store(false, Ordering::Release);
        });
    };

    let hk = cfg_c.hotkey.clone();
    let h = std::thread::spawn(move || {
        hotkey::listen_blocking(hk, on_press, on_release);
    });
    st.thread = Some(h);

    info!("[listener] started");
    serde_json::json!({ "ok": true })
}

fn cmd_login_qr_start(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    login: &Arc<Mutex<LoginInProgress>>,
    cfg: &Arc<Mutex<config::Config>>,
) -> serde_json::Value {
    use base64::Engine;
    use grammers_client::grammers_tl_types as tl;

    let cfg_c = cfg.lock().clone();

    // Если client уже есть и НЕ авторизован — он работает на старом
    // (битом) auth_key. Дропаем клиент + удаляем session-файл, чтобы
    // новый QR-запрос начался с чистого ключа.
    {
        let mut slot = client.lock();
        if let Some(c) = slot.as_ref() {
            let authed = rt.block_on(c.is_authorized()).unwrap_or(false);
            if !authed {
                info!("[qr] dropping stale client + session before fresh QR");
                let _ = std::fs::remove_file(telegram::session_path(&cfg_c));
                *slot = None;
            }
        }
    }

    let c = {
        let mut slot = client.lock();
        if slot.is_none() {
            match rt.block_on(telegram::connect(&cfg_c)) {
                Ok(cl) => *slot = Some(cl),
                Err(e) => return err(format!("connect: {}", e)),
            }
        }
        slot.as_ref().unwrap().clone()
    };

    if rt.block_on(c.is_authorized()).unwrap_or(false) {
        rt.block_on(telegram::refresh_auth_snapshot(&c));
        let _ = rt.block_on(telegram::save_session(&c, &cfg_c));
        login.lock().qr_status = Some("authorized".into());
        return serde_json::json!({ "ok": true, "already_authorized": true });
    }

    let (token_bytes, expires) = match rt.block_on(c.invoke(&tl::functions::auth::ExportLoginToken {
        api_id: cfg_c.telegram.api_id,
        api_hash: cfg_c.telegram.api_hash.clone(),
        except_ids: vec![],
    })) {
        Ok(tl::enums::auth::LoginToken::Token(t)) => (t.token, t.expires),
        Ok(tl::enums::auth::LoginToken::Success(_)) => {
            rt.block_on(telegram::refresh_auth_snapshot(&c));
            let _ = rt.block_on(telegram::save_session(&c, &cfg_c));
            login.lock().qr_status = Some("authorized".into());
            return serde_json::json!({ "ok": true, "already_authorized": true });
        }
        Ok(tl::enums::auth::LoginToken::MigrateTo(_)) => {
            return err("аккаунт на другом DC — войди через Phone + code");
        }
        Err(e) => return err(format!("export_login_token: {}", e)),
    };

    let token_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&token_bytes);
    let qr_url = format!("tg://login?token={}", token_b64);
    let token_hex: String = token_bytes.iter().take(8).map(|b| format!("{:02x}", b)).collect();
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i32;
    info!(
        "[qr] api_id={} token={}B token_first8={} b64_len={} expires_at={} expires_in={}s",
        cfg_c.telegram.api_id,
        token_bytes.len(),
        token_hex,
        token_b64.len(),
        expires,
        expires - now_unix,
    );
    info!("[qr] FULL_URL = {}", qr_url);

    let svg = match qrcode::QrCode::new(qr_url.as_bytes()) {
        Ok(qr) => qr
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(220, 220)
            .quiet_zone(true)
            .dark_color(qrcode::render::svg::Color("#0e0e10"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
        Err(e) => return err(format!("qr render: {}", e)),
    };
    let qr_data_url = format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
    );

    {
        let mut lg = login.lock();
        lg.qr_url = Some(qr_url.clone());
        lg.qr_status = Some("waiting".into());
    }

    let c_poll = c.clone();
    let cfg_poll = cfg_c.clone();
    let login_poll = login.clone();
    let our_url = qr_url.clone();

    rt.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            {
                let lg = login_poll.lock();
                if lg.qr_url.as_deref() != Some(our_url.as_str()) {
                    return;
                }
                if lg.qr_status.as_deref() != Some("waiting") {
                    return;
                }
            }
            let res = c_poll
                .invoke(&tl::functions::auth::ExportLoginToken {
                    api_id: cfg_poll.telegram.api_id,
                    api_hash: cfg_poll.telegram.api_hash.clone(),
                    except_ids: vec![],
                })
                .await;
            match res {
                Ok(tl::enums::auth::LoginToken::Token(_)) => continue,
                Ok(tl::enums::auth::LoginToken::Success(_)) => {
                    // ВАЖНО порядок: сначала get_me() — он триггерит
                    // grammers'у обновить внутреннее состояние session
                    // (после raw auth.exportLoginToken оно не апдейтится).
                    // Потом save_session — иначе на диск пишется пустой файл.
                    match telegram::refresh_auth_snapshot(&c_poll).await {
                        snap if snap.signed_in => info!("[qr] me={:?} @{:?}", snap.user_id, snap.username),
                        _ => warn!("[qr] auth snapshot говорит signed_in=false после Success ?!"),
                    }
                    match telegram::save_session(&c_poll, &cfg_poll).await {
                        Ok(()) => info!("[qr] session saved to {}", telegram::session_path(&cfg_poll).display()),
                        Err(e) => warn!("[qr] save_session FAILED: {}", e),
                    }
                    let mut lg = login_poll.lock();
                    if lg.qr_url.as_deref() == Some(our_url.as_str()) {
                        lg.qr_status = Some("authorized".into());
                    }
                    info!("[qr] authorized");
                    return;
                }
                Ok(tl::enums::auth::LoginToken::MigrateTo(_)) => {
                    let mut lg = login_poll.lock();
                    if lg.qr_url.as_deref() == Some(our_url.as_str()) {
                        lg.qr_status =
                            Some("error: аккаунт на другом DC — войди через Phone + code".into());
                    }
                    return;
                }
                Err(e) => {
                    let s = e.to_string();
                    warn!("[qr] poll err: {}", s);
                    let mut lg = login_poll.lock();
                    if lg.qr_url.as_deref() == Some(our_url.as_str()) {
                        if s.contains("SESSION_PASSWORD_NEEDED") {
                            lg.qr_status = Some("2fa".into());
                        } else if s.contains("AUTH_TOKEN_EXPIRED") || s.contains("expired") {
                            lg.qr_status = Some("expired".into());
                        } else {
                            lg.qr_status = Some(format!("error: {}", s));
                        }
                    }
                    return;
                }
            }
        }
    });

    serde_json::json!({
        "ok": true,
        "url": qr_url,
        "qr_data_url": qr_data_url,
    })
}

fn cmd_login_qr_status(login: &Arc<Mutex<LoginInProgress>>) -> serde_json::Value {
    let status = login
        .lock()
        .qr_status
        .clone()
        .unwrap_or_else(|| "idle".into());
    serde_json::json!({ "ok": true, "status": status })
}

fn cmd_logout(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    cfg: &Arc<Mutex<config::Config>>,
) -> serde_json::Value {
    let cfg_c = cfg.lock().clone();
    let c = match client.lock().as_ref() {
        Some(c) => c.clone(),
        None => return serde_json::json!({ "ok": true }),
    };
    let _ = rt.block_on(async { c.sign_out().await });
    let session_p = telegram::session_path(&cfg_c);
    let _ = std::fs::remove_file(session_p);
    let _ = std::fs::remove_file(telegram::dialog_cache_path());
    *client.lock() = None;
    telegram::set_auth_snapshot(telegram::AuthSnapshot::default());
    serde_json::json!({ "ok": true })
}

