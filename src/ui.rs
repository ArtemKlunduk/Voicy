//! Settings UI на wry. Окно с embedded HTML, IPC между Rust и JS.
//! Команды: info, models_list, model_download, model_activate,
//!          login_phone, login_code, login_password, logout,
//!          hotkey_set.

use crate::{asr, audio, config, contacts as cts, hotkey, startup, telegram};
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
use tracing::{debug, info, warn};
use wry::WebViewBuilder;
#[cfg(windows)]
use wry::WebViewBuilderExtWindows;

const UI_HTML: &str = include_str!("ui.html");

// ── Handoff assets (Шу mascot + logo). Эмбедятся в exe через include_bytes!
mod handoff_assets {
    use base64::Engine;

    pub const LOGO_MARK: &[u8] = include_bytes!("../assets/logo-mark.svg");
    pub const SHU_HELLO: &[u8] = include_bytes!("../assets/shu/hello.svg");
    pub const SHU_IDLE: &[u8] = include_bytes!("../assets/shu/idle.svg");
    pub const SHU_LISTENING: &[u8] = include_bytes!("../assets/shu/listening.svg");
    pub const SHU_SENDING: &[u8] = include_bytes!("../assets/shu/sending.svg");
    pub const SHU_SENT: &[u8] = include_bytes!("../assets/shu/sent.svg");
    pub const SHU_SHY: &[u8] = include_bytes!("../assets/shu/shy.svg");
    pub const SHU_SLEEP: &[u8] = include_bytes!("../assets/shu/sleep.svg");
    pub const SHU_TILT: &[u8] = include_bytes!("../assets/shu/tilt.svg");

    /// Сматчить URL path → байтовый контент SVG. None если путь не известен.
    /// wry/WebView2 может присылать path как с leading slash, так и без —
    /// нормализуем, обрезая ведущий '/'.
    pub fn lookup(path: &str) -> Option<&'static [u8]> {
        let p = path.trim_start_matches('/');
        match p {
            "assets/logo-mark.svg" => Some(LOGO_MARK),
            "assets/shu/hello.svg" => Some(SHU_HELLO),
            "assets/shu/idle.svg" => Some(SHU_IDLE),
            "assets/shu/listening.svg" => Some(SHU_LISTENING),
            "assets/shu/sending.svg" => Some(SHU_SENDING),
            "assets/shu/sent.svg" => Some(SHU_SENT),
            "assets/shu/shy.svg" => Some(SHU_SHY),
            "assets/shu/sleep.svg" => Some(SHU_SLEEP),
            "assets/shu/tilt.svg" => Some(SHU_TILT),
            _ => None,
        }
    }

    /// Сделать data: URI из SVG-байтов (base64). WebView2 custom protocol
    /// глючит для img src — data URI работает 100%.
    fn to_data_uri(bytes: &[u8]) -> String {
        format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    /// Подставить data URI'ы вместо `voicy://localhost/assets/...` ссылок
    /// в UI_HTML. Также injectit global `window.__SHU_URLS` map чтобы JS мог
    /// менять src динамически без зависимости от custom protocol (которая
    /// глючит для img src в WebView2).
    pub fn inline_data_uris(html: &str) -> String {
        let pairs: &[(&str, &str, &[u8])] = &[
            ("voicy://localhost/assets/logo-mark.svg", "logo", LOGO_MARK),
            ("voicy://localhost/assets/shu/hello.svg", "hello", SHU_HELLO),
            ("voicy://localhost/assets/shu/idle.svg", "idle", SHU_IDLE),
            ("voicy://localhost/assets/shu/listening.svg", "listening", SHU_LISTENING),
            ("voicy://localhost/assets/shu/sending.svg", "sending", SHU_SENDING),
            ("voicy://localhost/assets/shu/sent.svg", "sent", SHU_SENT),
            ("voicy://localhost/assets/shu/shy.svg", "shy", SHU_SHY),
            ("voicy://localhost/assets/shu/sleep.svg", "sleep", SHU_SLEEP),
            ("voicy://localhost/assets/shu/tilt.svg", "tilt", SHU_TILT),
        ];
        let mut out = html.to_string();
        // 1) Statический заменитель для img src в HTML
        for (key, _name, bytes) in pairs {
            out = out.replace(key, &to_data_uri(bytes));
        }
        // 2) JS-map для runtime смены src — injectim перед закрывающим </head>
        let mut js = String::from("\n<script>window.__SHU_URLS = {");
        for (_key, name, bytes) in pairs {
            js.push_str(&format!("'{}':'{}',", name, to_data_uri(bytes)));
        }
        js.push_str("};</script>\n");
        out = out.replace("</head>", &format!("{}</head>", js));
        out
    }
}

/// UI HTML с подставленными data URI вместо voicy://... ссылок.
/// Lazy — считается один раз при первом обращении.
fn ui_html_with_assets() -> &'static str {
    static CACHED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHED.get_or_init(|| handoff_assets::inline_data_uris(UI_HTML)).as_str()
}

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

    // Overlay рендерится нативно через Win32 layered window (см. native_overlay.rs).
    // WebView2 не уважает alpha-канал на ряде GPU — даёт белый прямоугольник.
    #[cfg(windows)]
    {
        crate::native_overlay::start();
    }

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
            match telegram::connect(&cfg).await {
                Ok(c) => {
                    let snap = telegram::refresh_auth_snapshot(&c).await;
                    info!("[ui-boot] telegram connected signed_in={}", snap.signed_in);
                    *slot.lock() = Some(c);
                }
                Err(e) => {
                    warn!("[ui-boot] telegram connect failed: {}", e);
                }
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

    // Клоны для graceful shutdown при закрытии окна
    let client_shutdown = client_slot.clone();
    let rt_shutdown = rt.clone();
    let cfg_shutdown = cfg_arc.clone();

    let ipc_handler = move |req: wry::http::Request<String>| {
        let body = req.into_body();
        debug!("[ui-ipc] raw: {}", body);
        let msg: Msg = match serde_json::from_str(&body) {
            Ok(m) => m,
            Err(e) => {
                warn!("[ui-ipc] parse fail: {} | body={}", e, body);
                return;
            }
        };
        debug!("[ui-ipc] cmd={} payload={}", msg.cmd, msg.payload);
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

    // --disable-gpu + GPU compositing: ставим software rendering для WebView2.
    // WebView2 на нашем стеке (Voicy v0.1.0) крашился через ucrtbase!__fastfail(7)
    // из d3d12.dll после долгого использования (BEX64 в WER). Хардварная
    // акселерация в Edge WebView2 имеет известные баги под определёнными
    // драйверами. Software rendering немного тяжелее CPU, но Settings UI
    // у нас лёгкий — разницы юзер не почувствует.
    //
    // --disable-features=msSmartScreenProtection — отключает SmartScreen-фоновую
    // проверку (она для нашего custom voicy:// протокола бессмысленна и
    // только вешает дополнительный network thread).
    let mut webview_builder = WebViewBuilder::new(&window);
    #[cfg(windows)]
    {
        webview_builder = webview_builder.with_additional_browser_args(
            "--disable-gpu --disable-gpu-compositing --disable-software-rasterizer \
             --disable-features=msSmartScreenProtection,RendererCodeIntegrity",
        );
    }
    let webview = webview_builder
        .with_url("voicy://localhost/index.html")
        .with_custom_protocol("voicy".into(), |req| {
            let path = req.uri().path();
            // /assets/* → handoff SVG assets (Шу, logo)
            if let Some(bytes) = handoff_assets::lookup(path) {
                return wry::http::Response::builder()
                    .header("Content-Type", "image/svg+xml")
                    .header("Cache-Control", "public, max-age=3600")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(std::borrow::Cow::Borrowed(bytes))
                    .unwrap();
            }
            // Diagnostic: всё что не main HTML и не asset — логируем, чтобы
            // понять какие пути приходят (на случай если wry/WebView2 шлёт
            // unexpected формат вроде authority в path).
            if path != "/" && path != "/index.html" && !path.is_empty() {
                tracing::warn!("[custom-protocol] unknown path → '{}' (full uri: {})", path, req.uri());
            }
            // Всё остальное → главный HTML (нет роутинга, single page).
            wry::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                // Разрешаем inline-скрипты (WebView2 по умолчанию для нестандартных схем
                // может ужесточать CSP).
                .header(
                    "Content-Security-Policy",
                    "default-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob:; \
                     script-src 'self' 'unsafe-inline' 'unsafe-eval' https://www.youtube.com https://s.ytimg.com; \
                     style-src 'self' 'unsafe-inline'; \
                     connect-src *; img-src * data: voicy: https://i.ytimg.com https://s.ytimg.com; frame-src https://www.youtube.com",
                )
                .body(std::borrow::Cow::Borrowed(ui_html_with_assets().as_bytes()))
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
                    graceful_shutdown(&client_shutdown, &rt_shutdown, &cfg_shutdown);
                    *flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(ev) => match ev {
                UiLoopEvent::EvalJs(js) => {
                    let _ = webview.evaluate_script(&js);
                }
                UiLoopEvent::OverlayRecording => {
                    #[cfg(windows)]
                    crate::native_overlay::send(crate::native_overlay::State::Recording);
                }
                UiLoopEvent::OverlaySuccess => {
                    #[cfg(windows)]
                    crate::native_overlay::send(crate::native_overlay::State::Success);
                }
                UiLoopEvent::OverlayError => {
                    #[cfg(windows)]
                    crate::native_overlay::send(crate::native_overlay::State::Error);
                }
                UiLoopEvent::WindowMinimize => {
                    window.set_minimized(true);
                }
                UiLoopEvent::WindowClose => {
                    graceful_shutdown(&client_shutdown, &rt_shutdown, &cfg_shutdown);
                    *flow = ControlFlow::Exit;
                }
                UiLoopEvent::WindowDrag => {
                    // tao's drag_window() использует PostMessage (async) — на момент
                    // обработки сообщения mouse button уже может быть отпущен и drag
                    // не стартует. Делаем SendMessage напрямую (синхронно) с
                    // ReleaseCapture перед ним. Передаём фактические координаты
                    // курсора в lParam — без этого OS не понимает откуда начался
                    // drag и не стартует операцию.
                    #[cfg(windows)]
                    {
                        use tao::platform::windows::WindowExtWindows;
                        use windows_sys::Win32::Foundation::{HWND, POINT};
                        use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
                        use windows_sys::Win32::UI::WindowsAndMessaging::{
                            GetCursorPos, SendMessageW, HTCAPTION, WM_NCLBUTTONDOWN,
                        };
                        let hwnd = window.hwnd() as HWND;
                        unsafe {
                            let mut pt = POINT { x: 0, y: 0 };
                            GetCursorPos(&mut pt);
                            // lParam: low word = x, high word = y (screen coords).
                            let lparam = ((pt.x & 0xFFFF) | ((pt.y & 0xFFFF) << 16)) as isize;
                            info!("[drag] SendMessage HTCAPTION at ({},{})", pt.x, pt.y);
                            ReleaseCapture();
                            SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, lparam);
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = window.drag_window();
                    }
                }
            },
            _ => {}
        }
    });
}

fn err(msg: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": msg.into() })
}

/// Сохранить все данные при закрытии приложения: dialog cache + Telegram session.
fn graceful_shutdown(
    client: &Arc<Mutex<Option<Client>>>,
    rt: &Arc<tokio::runtime::Runtime>,
    cfg: &Arc<Mutex<config::Config>>,
) {
    // 1. Сохраняем dialog cache (синхронно)
    let cache_path = telegram::dialog_cache_path();
    match telegram::save_dialog_cache(&cache_path) {
        Ok(n) => info!("[shutdown] dialog cache saved: {} entries", n),
        Err(e) => warn!("[shutdown] dialog cache save failed: {}", e),
    }

    // 2. Сохраняем Telegram session
    let cfg_c = cfg.lock().clone();
    if let Some(c) = client.lock().as_ref() {
        match rt.block_on(telegram::save_session(c, &cfg_c)) {
            Ok(()) => info!("[shutdown] Telegram session saved"),
            Err(e) => warn!("[shutdown] Telegram session save failed: {}", e),
        }
    } else {
        info!("[shutdown] no Telegram client to save");
    }
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
        "preload_get" => cmd_preload_get(cfg),
        "preload_set" => cmd_preload_set(cfg, cfg_path, &msg.payload),
        "startup_get" => cmd_startup_get(cfg),
        "startup_set" => cmd_startup_set(cfg, cfg_path, &msg.payload),
        "rec_lang_set" => cmd_rec_lang_set(cfg, cfg_path, &msg.payload),
        "rec_lang_get" => cmd_rec_lang_get(cfg),
        "theme_get" => cmd_theme_get(cfg),
        "avatars_get" => cmd_avatars_get(rt, client, &msg.payload),
        "language_get" => cmd_language_get(cfg),
        "language_set" => cmd_language_set(cfg, cfg_path, &msg.payload),
        "feedback_config_get" => cmd_feedback_config_get(cfg),
        "feedback_config_set" => cmd_feedback_config_set(cfg, cfg_path, &msg.payload),
        "feedback_send" => cmd_feedback_send(rt, client, cfg, &msg.payload),
        "_window_close" => {
            let _ = proxy.send_event(UiLoopEvent::WindowClose);
            serde_json::json!({ "ok": true })
        }
        "_window_minimize" => {
            let _ = proxy.send_event(UiLoopEvent::WindowMinimize);
            serde_json::json!({ "ok": true })
        }
        "_window_drag" => {
            info!("[ipc] _window_drag payload={}", msg.payload);
            let _ = proxy.send_event(UiLoopEvent::WindowDrag);
            serde_json::json!({ "ok": true })
        }
        "_log_dbg" => {
            info!("[dbg] {}", msg.payload);
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

fn cmd_preload_get(cfg: &Arc<Mutex<config::Config>>) -> serde_json::Value {
    let c = cfg.lock();
    serde_json::json!({ "ok": true, "preload": c.preload_model })
}

fn cmd_preload_set(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let val = payload.get("preload").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut c = cfg.lock();
    c.preload_model = val;
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save: {}", e));
    }
    info!("[ui] preload_model updated: {}", val);
    serde_json::json!({ "ok": true, "preload": val })
}

fn cmd_startup_get(cfg: &Arc<Mutex<config::Config>>) -> serde_json::Value {
    let c = cfg.lock();
    let enabled = c.startup_launch;
    let registry = startup::is_enabled();
    serde_json::json!({ "ok": true, "startup": enabled, "registry": registry })
}

fn cmd_startup_set(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let val = payload.get("startup").and_then(|v| v.as_bool()).unwrap_or(false);
    startup::sync_with_config(val);
    let mut c = cfg.lock();
    c.startup_launch = val;
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save: {}", e));
    }
    info!("[ui] startup_launch updated: {}", val);
    serde_json::json!({ "ok": true, "startup": val })
}

fn cmd_rec_lang_set(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let val = payload.get("language").and_then(|v| v.as_str()).unwrap_or("auto").to_string();
    let mut c = cfg.lock();
    c.recognition_language = val.clone();
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save: {}", e));
    }
    info!("[ui] recognition_language updated: {}", val);
    serde_json::json!({ "ok": true, "language": val })
}

fn cmd_rec_lang_get(cfg: &Arc<Mutex<config::Config>>) -> serde_json::Value {
    let c = cfg.lock();
    serde_json::json!({ "ok": true, "language": c.recognition_language })
}

fn cmd_theme_get(cfg: &Arc<Mutex<config::Config>>) -> serde_json::Value {
    let c = cfg.lock();
    serde_json::json!({ "ok": true, "theme": c.ui_theme })
}

fn cmd_feedback_config_get(cfg: &Arc<Mutex<config::Config>>) -> serde_json::Value {
    let c = cfg.lock();
    serde_json::json!({
        "ok": true,
        "username": c.feedback_dev_username,
        "uid": c.feedback_dev_uid,
    })
}

fn cmd_feedback_config_set(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let username = payload.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let uid = payload.get("uid").and_then(|v| v.as_i64()).unwrap_or(882983468);
    let mut c = cfg.lock();
    c.feedback_dev_username = username;
    c.feedback_dev_uid = uid;
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save: {}", e));
    }
    serde_json::json!({ "ok": true })
}

fn cmd_feedback_send(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    cfg: &Arc<Mutex<config::Config>>,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if text.trim().is_empty() {
        return err("empty feedback text");
    }
    let cfg_c = cfg.lock().clone();
    let dev_uid = cfg_c.feedback_dev_uid;
    if dev_uid == 0 {
        return err("developer uid not configured");
    }
    let client_opt = client.lock().clone();
    let Some(c) = client_opt else {
        return err("telegram client not connected");
    };
    match rt.block_on(async {
        if !crate::telegram::is_signed_in(&c).await? {
            return Err(anyhow::anyhow!("not signed in"));
        }
        crate::telegram::send_message(&c, dev_uid, &text).await
    }) {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(e) => err(format!("send failed: {}", e)),
    }
}

/// Получить avatar'ы для списка UID. Возвращает `{uid: data_url}` для тех,
/// у кого есть аватарка (или уже в кэше, или скачали сейчас). Отсутствующие
/// просто не попадают в ответ.
///
/// Кэшируется на диске в %APPDATA%/voicy/avatars/<uid>.jpg — повторные вызовы
/// мгновенные. Первый вызов на холодный кэш — может занимать секунды.
fn cmd_avatars_get(
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let uids: Vec<i64> = match payload.get("uids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_i64()).collect(),
        None => return err("payload.uids (array) required"),
    };
    let client_opt = client.lock().as_ref().cloned();
    let Some(client) = client_opt else {
        return err("not signed in");
    };

    let mut result = serde_json::Map::new();
    for uid in uids {
        // Сначала смотрим кэш — это быстро (нет сети).
        let cached = telegram::avatar_cache_path(uid);
        let path_opt = if cached.exists()
            && std::fs::metadata(&cached).map(|m| m.len() > 0).unwrap_or(false)
        {
            Some(cached)
        } else {
            // Холодный кэш — качаем синхронно из tokio.
            rt.block_on(telegram::fetch_avatar(&client, uid))
        };
        if let Some(path) = path_opt {
            if let Ok(bytes) = std::fs::read(&path) {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                result.insert(
                    uid.to_string(),
                    serde_json::Value::String(format!("data:image/jpeg;base64,{}", b64)),
                );
            }
        }
    }
    serde_json::json!({ "ok": true, "avatars": result })
}

fn cmd_language_get(cfg: &Arc<Mutex<config::Config>>) -> serde_json::Value {
    let c = cfg.lock();
    serde_json::json!({ "ok": true, "language": c.language })
}

fn cmd_language_set(
    cfg: &Arc<Mutex<config::Config>>,
    cfg_path: &PathBuf,
    payload: &serde_json::Value,
) -> serde_json::Value {
    let val = payload.get("language").and_then(|v| v.as_str()).unwrap_or("ru").to_string();
    let mut c = cfg.lock();
    c.language = val.clone();
    if let Err(e) = c.save(cfg_path) {
        return err(format!("save: {}", e));
    }
    info!("[ui] language updated: {}", val);
    serde_json::json!({ "ok": true, "language": val })
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
    /// Рекомендуемая модель — выделяется в UI
    recommended: bool,
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
            recommended: m.recommended,
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
        let c = match client.lock().as_ref() {
            Some(c) => c.clone(),
            None => return Err(anyhow::anyhow!("Telegram-клиент недоступен")),
        };
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
            match rt.block_on(async {
                telegram::refresh_auth_snapshot(&c).await;
                telegram::save_session(&c, &cfg).await?;
                Ok::<_, anyhow::Error>(())
            }) {
                Ok(()) => info!("[ui-login] session saved after sign_in"),
                Err(e) => warn!("[ui-login] session save failed after sign_in: {}", e),
            }
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
            match rt.block_on(async {
                telegram::refresh_auth_snapshot(&c).await;
                telegram::save_session(&c, &cfg).await?;
                Ok::<_, anyhow::Error>(())
            }) {
                Ok(()) => info!("[ui-login] session saved after check_password"),
                Err(e) => warn!("[ui-login] session save failed after check_password: {}", e),
            }
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
    // Показываем терминальное состояние (Success/Error). Скрытие — задача самого
    // overlay-треда (auto-hide через AUTO_HIDE_MS): новый Recording сбрасывает
    // его таймер и тем самым отменяет отложенное скрытие. Раньше здесь спавнился
    // поток со sleep+OverlayHide, и устаревший таймер скрывал оверлей во время
    // следующей записи (значок пропадал, хотя запись ещё шла).
    let _ = proxy.send_event(ev);
}

/// Выбрать ASR-модель для inference: сначала проверяем скачанность
/// сконфигурированной, затем идём по списку fallback'ов.
fn resolve_inference_model(configured: &str, proxy: &EventLoopProxy<UiLoopEvent>) -> String {
    let active_ok = asr::model_meta(configured)
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
    let inference_model = chosen.unwrap_or_else(|| {
        warn!("[listener] no ASR model downloaded — using configured model anyway");
        configured.to_string()
    });
    if inference_model != configured {
        push_event(
            proxy,
            "log",
            &format!(
                "ℹ inference fallback: «{}» → «{}»",
                configured, inference_model
            ),
        );
    }
    inference_model
}

fn cmd_listener_start(
    cfg: &Arc<Mutex<config::Config>>,
    rt: &Arc<tokio::runtime::Runtime>,
    client: &Arc<Mutex<Option<Client>>>,
    listener: &Arc<Mutex<ListenerState>>,
    proxy: &EventLoopProxy<UiLoopEvent>,
) -> serde_json::Value {
    info!("[listener] cmd_listener_start called");
    let mut cfg_c = cfg.lock().clone();

    {
        let st = listener.lock();
        if st.running.load(Ordering::Acquire) {
            info!("[listener] already running, returning already=true");
            return serde_json::json!({ "ok": true, "already": true });
        }
    }

    // Active = whisper или nemo (Parakeet). transcribe_wav сама свалится на whisper если nemo не сработал.
    let inference_model = resolve_inference_model(&cfg_c.model, proxy);
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

    // Прогрев ASR-модели в RAM если включено в настройках
    if cfg_c.preload_model {
        if let Some(meta) = asr::model_meta(&cfg_c.model) {
            if meta.engine == "nemo" && asr::model_is_downloaded(&cfg_c.model) {
                let model_dir = asr::nemo_model_dir(&cfg_c.model);
                let model_name = cfg_c.model.clone();
                info!("[preload-ui] прогрев Parakeet {}…", model_name);
                std::thread::spawn(move || {
                    if let Err(e) = crate::parakeet::preload(&model_name, &model_dir) {
                        warn!("[preload-ui] {}", e);
                    } else {
                        info!("[preload-ui] Parakeet готов в RAM");
                    }
                });
            }
        }
    }

    let mut st = listener.lock();
    if st.running.load(Ordering::Acquire) {
        return serde_json::json!({ "ok": true, "already": true });
    }
    // ВАЖНО: rdev::listen блокирует тред навсегда (нет clean shutdown API).
    // Если предыдущий listener уже создавал тред — НЕ спавним новый, иначе
    // получим N тредов с одним и тем же AtomicBool и Alt+X вызовет pipeline
    // N раз → дубли сообщений в Telegram. Стоимость такого решения: смена
    // хоткея в UI не подхватится без перезапуска приложения (старый тред
    // помнит старый хоткей в captured closure).
    if st.thread.is_some() {
        st.running.store(true, Ordering::Release);
        info!("[listener] reusing existing rdev thread (hotkey change requires app restart)");
        return serde_json::json!({ "ok": true, "resumed": true });
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
    let cfg_arc = cfg.clone();
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
        let client_slot = client_slot_thr.clone();
        let rt = rt_thr.clone();
        let proxy = proxy_release.clone();
        push_event(&proxy, "activity", "⏹ распознаю…");
        let proxy_panic = proxy.clone();
        // Поднимаем флаг — следующие Alt+X будут игнорироваться пока этот
        // pipeline не завершится (или не упадёт в panic guard ниже).
        processing_release.store(true, Ordering::Release);
        let processing_done = processing_release.clone();
        let cfg_for_thread = cfg_arc.clone();
        let proxy_for_thread = proxy.clone();
        std::thread::spawn(move || {
            // Читаем актуальный конфиг из Arc — чтобы runtime-изменения
            // (ai_assistant_enabled, ai_model, gemini_key и т.д.) подхватывались
            // без перезапуска listener'а.
            let mut cfg = cfg_for_thread.lock().clone();
            cfg.model = resolve_inference_model(&cfg.model, &proxy_for_thread);
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
            debug!("[listener] 📝 «{}» (через {})", text, cfg.model);
            push_event(&proxy, "log", &format!("📝 «{}» · engine: {}", text, cfg.model));

            let contacts = cts::load(&cts::default_path());

            info!("[listener] trying contacts::parse_command with text='{}'", text);
            // Silent no-op для онбординг-теста: «test»/«тест»/«check»/«проверка»
            // одним словом — это просто проверка ASR работает. Не показываем
            // красный × «команда не распознана», иначе на 1-м шаге гайда мигает
            // ошибка прямо при успешной транскрипции.
            let trimmed_lower = text.trim().to_lowercase();
            let single = trimmed_lower.trim_end_matches(|c: char| !c.is_alphanumeric()).to_string();
            if matches!(single.as_str(), "test" | "тест" | "check" | "проверка" | "hello" | "привет") {
                info!("[listener] silent no-op for onboarding test word: '{}'", single);
                push_event(&proxy, "activity", "");
                return;
            }

            // Единая маршрутизация (cts::classify): диктовка (нет триггера) →
            // печать в активное окно; Telegram-команда → отправка ниже; пусто/
            // не распознано → текст в буфер обмена. Тот же classify зовёт voicy run.
            let (uid, message) = match cts::classify(&text, &contacts) {
                cts::Utterance::Dictation(dictated) if cfg.dictation_enabled => {
                    debug!("[listener] dictation → typing «{}»", dictated);
                    crate::typing::type_text(&dictated);
                    push_event(&proxy, "log", &format!("⌨ напечатано: «{}»", dictated));
                    push_event(&proxy, "activity", "⌨ продиктовано");
                    flash_overlay(&proxy, UiLoopEvent::OverlaySuccess);
                    return;
                }
                cts::Utterance::Telegram { uid, message } => (uid, message),
                cts::Utterance::Empty => {
                    warn!("[listener] пустое сообщение");
                    push_event(&proxy, "log", "✗ пустое сообщение");
                    push_event(&proxy, "activity", "");
                    flash_overlay(&proxy, UiLoopEvent::OverlayError);
                    return;
                }
                // Диктовка выключена ИЛИ контакт/команда не распознаны → в буфер обмена.
                other => {
                    let reason = match other {
                        cts::Utterance::Unrecognized(e) => e,
                        _ => "команда не распознана".to_string(),
                    };
                    warn!("[listener] {}", reason);
                    let to_copy = text.trim().to_string();
                    let copy_ok = arboard::Clipboard::new()
                        .and_then(|mut c| c.set_text(to_copy.clone()))
                        .is_ok();
                    if copy_ok {
                        push_event(&proxy, "log", &format!("✗ {} · текст в буфере: «{}»", reason, to_copy));
                        push_event(&proxy, "activity", "✗ не распознано — текст в буфере");
                    } else {
                        push_event(&proxy, "log", &format!("✗ parse: {}", reason));
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
            // Резолвим SELF_SENTINEL_UID в реальный user_id залогиненного юзера
            // (Saved Messages / «избранное» — это чат с самим собой в Telegram).
            let resolved_uid = if uid == cts::SELF_SENTINEL_UID {
                match rt.block_on(async { client.get_me().await }) {
                    Ok(me) => me.id(),
                    Err(e) => {
                        warn!("[listener] get_me для SELF: {}", e);
                        push_event(&proxy, "log", &format!("✗ get_me: {}", e));
                        push_event(&proxy, "activity", "");
                        flash_overlay(&proxy, UiLoopEvent::OverlayError);
                        return;
                    }
                }
            } else { uid };
            // Pre-send лог с УЖЕ резолвнутым uid (положительные цифры) — onboarding
            // pattern /^→ \d+ «/ матчится сразу, не ждём пока Telegram реально отправит.
            push_event(&proxy, "log", &format!("→ {} «{}»…", resolved_uid, message));
            push_event(&proxy, "activity", &format!("→ {} «{}»…", resolved_uid, message));
            let res = rt.block_on(async { telegram::send_message(&client, resolved_uid, &message).await });
            let uid = resolved_uid; // для логов ниже
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
        match slot.as_ref() {
            Some(c) => c.clone(),
            None => return err("Telegram-клиент недоступен"),
        }
    };

    if rt.block_on(c.is_authorized()).unwrap_or(false) {
        rt.block_on(telegram::refresh_auth_snapshot(&c));
        if let Err(e) = rt.block_on(telegram::save_session(&c, &cfg_c)) {
            warn!("[ui-login-url] save_session failed: {}", e);
        }
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
            if let Err(e) = rt.block_on(telegram::save_session(&c, &cfg_c)) {
                warn!("[ui-login-url] save_session failed on token success: {}", e);
            }
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


// force rebuild 1721589229

// rebuild 1548951036

