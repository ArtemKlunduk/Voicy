//! Overlay-окно: halo rings + orb-микрофон во время записи,
//! галочка-orb после успеха, крест-orb на ошибке. Прозрачное, поверх всех окон.
//!
//! HTML встроен в exe через include_str!. Wry загружает его как html-строку.
//! Состоянием рулим из других тредов через EventLoopProxy<UiEvent>.

use anyhow::Result;
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Запись пошла — показать бары
    Recording,
    /// Успешно отправлено — зелёная галочка → скрыть через 1.8s
    Success,
    /// Ошибка — красный крест → скрыть через 1.8s
    Error,
    /// Скрыть оверлей сейчас
    Hide,
    /// Выйти (Ctrl+C из консоли и т.п.)
    Quit,
}

pub const OVERLAY_HTML: &str = include_str!("overlay.html");

/// Запустить overlay-окно. На вход — proxy для приёма UiEvent.
/// Эта функция блокирует тред (event loop).
pub fn run<F>(event_loop: tao::event_loop::EventLoop<UiEvent>, on_start: F) -> Result<()>
where
    F: FnOnce(EventLoopProxy<UiEvent>) + Send + 'static,
{
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || on_start(proxy));

    // Размер окна и позиция (внизу по центру). Halo rings расходятся до
    // scale(1.9) → 84*1.9 ≈ 160px, плюс место под кружок → 220x220.
    let win_w = 220;
    let win_h = 220;
    let monitor = event_loop.primary_monitor();
    let (screen_w, screen_h) = if let Some(m) = monitor {
        let s = m.size();
        (s.width as i32, s.height as i32)
    } else {
        (1920, 1080)
    };
    let pos_x = (screen_w - win_w) / 2;
    let pos_y = screen_h - win_h - 24;

    // Непрозрачное окно с фоном Paper — см. комментарий в ui.rs.
    let window = WindowBuilder::new()
        .with_title("voicy-overlay")
        .with_inner_size(LogicalSize::new(win_w as f64, win_h as f64))
        .with_position(LogicalPosition::new(pos_x as f64, pos_y as f64))
        .with_decorations(false)
        .with_always_on_top(true)
        .with_resizable(false)
        .with_focused(false)
        .with_visible(false)
        .build(&event_loop)?;

    let webview = WebViewBuilder::new(&window)
        .with_html(OVERLAY_HTML)
        .build()?;

    event_loop.run(move |event, _target, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::UserEvent(ev) => match ev {
                UiEvent::Recording => {
                    let _ = window.set_visible(true);
                    let _ = webview.evaluate_script("window.voicySet && window.voicySet('recording')");
                }
                UiEvent::Success => {
                    let _ = window.set_visible(true);
                    let _ = webview.evaluate_script("window.voicySet && window.voicySet('success')");
                }
                UiEvent::Error => {
                    let _ = window.set_visible(true);
                    let _ = webview.evaluate_script("window.voicySet && window.voicySet('error')");
                }
                UiEvent::Hide => {
                    let _ = window.set_visible(false);
                }
                UiEvent::Quit => {
                    *flow = ControlFlow::Exit;
                }
            },
            _ => {}
        }
    });
}

fn _no_dead_warn(_: &EventLoopWindowTarget<UiEvent>) {}
