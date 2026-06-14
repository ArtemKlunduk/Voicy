//! Нативный overlay через Win32 WS_EX_LAYERED + UpdateLayeredWindow.
//!
//! WebView2 на ряде GPU/композиторов не уважает alpha-канал и даёт белый
//! прямоугольник. Поэтому overlay рисуется векторно через tiny-skia в
//! RGBA-битмап и пушится в layered-окно с per-pixel alpha. Работает на
//! любом GPU без зависимости от WebView2 транспаранси.
//!
//! Состояние:
//!   Recording  — orb (sage-soft) + 3 расходящихся halo rings + микрофон
//!   Success    — orb (sage-deep) с галочкой, без колец
//!   Error      — orb (danger) с крестом, без колец
//!   Hidden     — окно скрыто
//!
//! API:
//!   start() -> mpsc::Sender<State> — запускает тред с win32 message loop.
//!   Шли в Sender любые State, окно перерисуется и покажется/скроется.

#![cfg(windows)]

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::Instant;
use tiny_skia::{
    Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform,
};
use tracing::{info, warn};
use windows_sys::core::PCWSTR;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, PeekMessageW,
    RegisterClassW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow, HWND_TOPMOST,
    MSG, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, ULW_ALPHA, WM_QUIT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const WIN_W: i32 = 220;
const WIN_H: i32 = 220;
const ORB_R: f32 = 42.0; // 84px diameter
const CENTER: f32 = (WIN_W as f32) / 2.0;

const RING_DURATION_MS: u32 = 2200;
const RING_DELAYS_MS: [u32; 3] = [0, 700, 1400];

/// Через сколько мс терминальные состояния (Success/Error) сами скрываются.
/// Recording НЕ авто-скрывается — анимация держится пока длится запись.
const AUTO_HIDE_MS: u32 = 1800;

// Палитра design system
fn sage_soft() -> Color { Color::from_rgba8(0xD6, 0xE6, 0xCF, 0xFF) }
fn sage() -> Color { Color::from_rgba8(0xA8, 0xC8, 0xA0, 0xFF) }
fn sage_deep() -> Color { Color::from_rgba8(0x5E, 0x8B, 0x62, 0xFF) }
fn moss() -> Color { Color::from_rgba8(0x2F, 0x4A, 0x35, 0xFF) }
fn danger() -> Color { Color::from_rgba8(0xB6, 0x60, 0x4D, 0xFF) }
fn white() -> Color { Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Hidden,
    Recording,
    Success,
    Error,
}

static SENDER: OnceLock<Sender<State>> = OnceLock::new();

/// Запустить overlay-тред. Возвращает Sender для смены состояния.
/// Можно вызывать несколько раз — фактический spawn только однажды.
pub fn start() -> Sender<State> {
    SENDER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            thread::Builder::new()
                .name("voicy-overlay".into())
                .spawn(move || overlay_thread(rx))
                .expect("spawn overlay thread");
            tx
        })
        .clone()
}

pub fn send(state: State) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(state);
    }
}

fn overlay_thread(rx: Receiver<State>) {
    let hwnd = match unsafe { create_window() } {
        Ok(h) => h,
        Err(e) => {
            warn!("[overlay] create_window failed: {}", e);
            return;
        }
    };
    info!("[overlay] window created hwnd={:p}", hwnd as *mut ());
    position_bottom_center(hwnd);

    let mut state = State::Hidden;
    let mut state_started = Instant::now();
    let mut last_rendered = Instant::now() - std::time::Duration::from_secs(1);

    loop {
        // Дренаж очереди состояний — берём самое последнее.
        let mut got_state = false;
        while let Ok(s) = rx.try_recv() {
            state = s;
            got_state = true;
        }
        if got_state {
            state_started = Instant::now();
            unsafe {
                if state == State::Hidden {
                    ShowWindow(hwnd, SW_HIDE);
                } else {
                    // SetWindowPos с HWND_TOPMOST + SWP_SHOWWINDOW + SWP_NOACTIVATE:
                    // 1) принудительно поднимает overlay поверх всех окон (даже если
                    //    наше main-окно Voicy активно сверху)
                    // 2) делает видимым без кражи фокуса у текущего окна юзера
                    SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0, 0, 0, 0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
            }
            last_rendered = Instant::now() - std::time::Duration::from_secs(1); // force redraw
        }

        // Авто-скрытие терминальных состояний (Success/Error) через AUTO_HIDE_MS.
        // Recording никогда не авто-скрывается — держим анимацию пока идёт запись.
        // Любое новое состояние сбрасывает state_started, поэтому новый Recording
        // сам отменяет отложенное скрытие. Скрытие делается ЗДЕСЬ, а не внешним
        // таймером — иначе устаревший таймер от прошлого сообщения скрывал бы
        // оверлей прямо во время следующей записи.
        if matches!(state, State::Success | State::Error)
            && state_started.elapsed().as_millis() as u32 >= AUTO_HIDE_MS
        {
            state = State::Hidden;
            unsafe { ShowWindow(hwnd, SW_HIDE); }
        }

        // Прокачиваем win32-сообщения (нужно для корректного поведения окна).
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Рендер: только когда не hidden, и не чаще ~30fps.
        if state != State::Hidden && last_rendered.elapsed().as_millis() >= 33 {
            let elapsed_ms = state_started.elapsed().as_millis() as u32;
            unsafe {
                render_and_push(hwnd, state, elapsed_ms);
            }
            last_rendered = Instant::now();
        }

        thread::sleep(std::time::Duration::from_millis(16));
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, w, l)
}

unsafe fn create_window() -> Result<HWND, String> {
    let hinstance = GetModuleHandleW(std::ptr::null());
    let class_name: Vec<u16> = "VoicyNativeOverlay\0".encode_utf16().collect();

    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: 0,
        hCursor: 0,
        hbrBackground: 0,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };
    let atom = RegisterClassW(&wc);
    if atom == 0 {
        // Класс уже зарегистрирован — это ок, продолжаем.
    }

    let title: Vec<u16> = "voicy-overlay\0".encode_utf16().collect();
    let ex_style = WS_EX_LAYERED
        | WS_EX_TOOLWINDOW
        | WS_EX_TOPMOST
        | WS_EX_NOACTIVATE
        | WS_EX_TRANSPARENT;
    let hwnd = CreateWindowExW(
        ex_style,
        class_name.as_ptr() as PCWSTR,
        title.as_ptr() as PCWSTR,
        WS_POPUP,
        0,
        0,
        WIN_W,
        WIN_H,
        0,
        0,
        hinstance,
        std::ptr::null(),
    );
    if hwnd == 0 {
        return Err("CreateWindowExW failed".into());
    }
    Ok(hwnd)
}

fn position_bottom_center(hwnd: HWND) {
    unsafe {
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let x = (sw - WIN_W) / 2;
        let y = sh - WIN_H - 24;
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            WIN_W,
            WIN_H,
            SWP_NOACTIVATE | SWP_NOSIZE,
        );
        // вторым вызовом гарантируем что size применился
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, WIN_W, WIN_H, SWP_NOACTIVATE);
    }
}

/// Отрисовать текущий кадр в Pixmap и запушить в layered-окно через
/// UpdateLayeredWindow с per-pixel alpha.
unsafe fn render_and_push(hwnd: HWND, state: State, elapsed_ms: u32) {
    let mut pixmap = match Pixmap::new(WIN_W as u32, WIN_H as u32) {
        Some(p) => p,
        None => return,
    };
    pixmap.fill(Color::TRANSPARENT);

    match state {
        State::Recording => {
            draw_glow(&mut pixmap, CENTER, CENTER, sage_soft(), ORB_R + 10.0, 0.18);
            draw_halo_rings(&mut pixmap, elapsed_ms, sage());
            draw_orb(&mut pixmap, sage_soft());
            draw_mic_icon(&mut pixmap, moss());
        }
        State::Success => {
            draw_glow(&mut pixmap, CENTER, CENTER, sage_deep(), ORB_R + 10.0, 0.18);
            draw_orb(&mut pixmap, sage_deep());
            draw_check_icon(&mut pixmap, white());
        }
        State::Error => {
            draw_glow(&mut pixmap, CENTER, CENTER, danger(), ORB_R + 10.0, 0.18);
            draw_orb(&mut pixmap, danger());
            draw_x_icon(&mut pixmap, white());
        }
        State::Hidden => return,
    }

    push_pixmap_to_window(hwnd, &pixmap);
}

/// Кольца расходятся с задержками 0 / 0.7s / 1.4s, цикл 2.2s.
/// scale: 0.55 → 1.9, opacity: 0 → 0.7 (на 20% времени) → 0.
fn draw_halo_rings(pixmap: &mut Pixmap, elapsed_ms: u32, base: Color) {
    for &delay in &RING_DELAYS_MS {
        let phase = ((elapsed_ms + RING_DURATION_MS - delay % RING_DURATION_MS) % RING_DURATION_MS) as f32
            / RING_DURATION_MS as f32;

        let scale = 0.55 + (1.9 - 0.55) * phase;
        let opacity = if phase < 0.2 {
            0.7 * (phase / 0.2)
        } else {
            0.7 * (1.0 - (phase - 0.2) / 0.8)
        }
        .clamp(0.0, 1.0);

        let radius = ORB_R * scale;
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(
            base.red(),
            base.green(),
            base.blue(),
            opacity,
        ).unwrap_or(base));
        paint.anti_alias = true;

        let mut pb = PathBuilder::new();
        pb.push_circle(CENTER, CENTER, radius);
        let path = match pb.finish() {
            Some(p) => p,
            None => continue,
        };
        let mut stroke = Stroke::default();
        stroke.width = 1.5;
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_orb(pixmap: &mut Pixmap, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    let mut pb = PathBuilder::new();
    pb.push_circle(CENTER, CENTER, ORB_R);
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Мягкое свечение вокруг orb — скрывает рваные края на прозрачном фоне.
fn draw_glow(pixmap: &mut Pixmap, cx: f32, cy: f32, color: Color, max_radius: f32, strength: f32) {
    let steps = 6;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let radius = ORB_R + t * (max_radius - ORB_R);
        let opacity = strength * (1.0 - t) * (1.0 - t);
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(
            color.red(),
            color.green(),
            color.blue(),
            opacity,
        ).unwrap_or(color));
        paint.anti_alias = true;
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, radius);
        if let Some(path) = pb.finish() {
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
}

fn draw_mic_icon(pixmap: &mut Pixmap, color: Color) {
    // SVG-микрофон: капсула 6x12 в центре + дуга + ножка.
    // Перерисовываем в координатах 24x24 с масштабом до 32x32 пикселей.
    let icon_size = 32.0;
    let scale = icon_size / 24.0;
    let ox = CENTER - icon_size / 2.0;
    let oy = CENTER - icon_size / 2.0;

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = 1.5 * scale;
    stroke.line_cap = tiny_skia::LineCap::Round;
    stroke.line_join = tiny_skia::LineJoin::Round;

    // Капсула «12,2 a3,3 0 0 0 -3,3 v6 a3,3 0 0 0 6,0 v-6 a3,3 0 0 0 -3,-3»
    // = прямоугольник со скруглениями. Просто рисуем закруглённую капсулу 6x12 (стек ±3 от центра).
    let mut pb = PathBuilder::new();
    let cx = ox + 12.0 * scale;
    let top = oy + 2.0 * scale;
    let bot = oy + 14.0 * scale;
    let r = 3.0 * scale;
    pb.move_to(cx - r, top + r);
    pb.cubic_to(cx - r, top, cx - r + r * 0.45, top, cx, top);
    pb.cubic_to(cx + r - r * 0.45, top, cx + r, top, cx + r, top + r);
    pb.line_to(cx + r, bot - r);
    pb.cubic_to(cx + r, bot, cx + r - r * 0.45, bot, cx, bot);
    pb.cubic_to(cx - r + r * 0.45, bot, cx - r, bot, cx - r, bot - r);
    pb.close();

    // Дуга снизу — путь M19 10 v1 a7 7 0 0 1 -14 0 v-1
    pb.move_to(ox + 19.0 * scale, oy + 10.0 * scale);
    pb.line_to(ox + 19.0 * scale, oy + 11.0 * scale);
    // полудуга радиуса 7
    let arc_cx = ox + 12.0 * scale;
    let arc_cy = oy + 11.0 * scale;
    let arc_r = 7.0 * scale;
    pb.cubic_to(
        arc_cx + arc_r,
        arc_cy + arc_r * 0.55,
        arc_cx + arc_r * 0.55,
        arc_cy + arc_r,
        arc_cx,
        arc_cy + arc_r,
    );
    pb.cubic_to(
        arc_cx - arc_r * 0.55,
        arc_cy + arc_r,
        arc_cx - arc_r,
        arc_cy + arc_r * 0.55,
        arc_cx - arc_r,
        arc_cy + arc_r * 0.0,
    );
    pb.line_to(ox + 5.0 * scale, oy + 10.0 * scale);

    // Ножка M12 18 v3
    pb.move_to(ox + 12.0 * scale, oy + 18.0 * scale);
    pb.line_to(ox + 12.0 * scale, oy + 21.0 * scale);

    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_check_icon(pixmap: &mut Pixmap, color: Color) {
    // SVG: polyline 20 6 9 17 4 12
    let icon_size = 32.0;
    let scale = icon_size / 24.0;
    let ox = CENTER - icon_size / 2.0;
    let oy = CENTER - icon_size / 2.0;

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = 2.5 * scale;
    stroke.line_cap = tiny_skia::LineCap::Round;
    stroke.line_join = tiny_skia::LineJoin::Round;

    let mut pb = PathBuilder::new();
    pb.move_to(ox + 20.0 * scale, oy + 6.0 * scale);
    pb.line_to(ox + 9.0 * scale, oy + 17.0 * scale);
    pb.line_to(ox + 4.0 * scale, oy + 12.0 * scale);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_x_icon(pixmap: &mut Pixmap, color: Color) {
    // SVG: M18 6 L6 18 + M6 6 L18 18
    let icon_size = 32.0;
    let scale = icon_size / 24.0;
    let ox = CENTER - icon_size / 2.0;
    let oy = CENTER - icon_size / 2.0;

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = 2.5 * scale;
    stroke.line_cap = tiny_skia::LineCap::Round;

    let mut pb = PathBuilder::new();
    pb.move_to(ox + 18.0 * scale, oy + 6.0 * scale);
    pb.line_to(ox + 6.0 * scale, oy + 18.0 * scale);
    pb.move_to(ox + 6.0 * scale, oy + 6.0 * scale);
    pb.line_to(ox + 18.0 * scale, oy + 18.0 * scale);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// Скопировать tiny-skia пиксели в DIB и обновить layered-окно.
/// tiny-skia даёт RGBA non-premultiplied. Windows UpdateLayeredWindow с
/// AC_SRC_ALPHA требует **premultiplied BGRA**. Делаем конверсию.
unsafe fn push_pixmap_to_window(hwnd: HWND, pixmap: &Pixmap) {
    let screen_dc: HDC = GetDC(0);
    let mem_dc: HDC = CreateCompatibleDC(screen_dc);

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = WIN_W;
    bmi.bmiHeader.biHeight = -WIN_H; // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB as u32;

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let bmp: HBITMAP = CreateDIBSection(
        mem_dc,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits,
        0,
        0,
    );
    if bmp == 0 || bits.is_null() {
        ReleaseDC(0, screen_dc);
        DeleteDC(mem_dc);
        return;
    }
    let old = SelectObject(mem_dc, bmp);

    // Конвертируем RGBA non-premul → BGRA premul + feather по краям окна.
    let src = pixmap.data(); // &[u8] длины W*H*4 в порядке RGBA
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, src.len());
    let win_w = WIN_W as usize;
    let win_h = WIN_H as usize;
    let feather_px = 10.0f32;
    for i in (0..src.len()).step_by(4) {
        let pixel = i / 4;
        let x = (pixel % win_w) as i32;
        let y = (pixel / win_w) as i32;

        let dx = f32::min(x as f32, (win_w - 1 - x as usize) as f32);
        let dy = f32::min(y as f32, (win_h - 1 - y as usize) as f32);
        let dist = f32::min(dx, dy);

        let r = src[i] as u32;
        let g = src[i + 1] as u32;
        let b = src[i + 2] as u32;
        let a = src[i + 3] as u32;

        let a_f = if dist < feather_px {
            (a as f32 * (dist / feather_px)).max(0.0).min(255.0)
        } else {
            a as f32
        };
        let a_u = a_f as u8;

        let pr = (r * a_u as u32 / 255) as u8;
        let pg = (g * a_u as u32 / 255) as u8;
        let pb = (b * a_u as u32 / 255) as u8;
        dst[i] = pb;
        dst[i + 1] = pg;
        dst[i + 2] = pr;
        dst[i + 3] = a_u;
    }

    let size = windows_sys::Win32::Foundation::SIZE { cx: WIN_W, cy: WIN_H };
    let src_pt = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        screen_dc,
        std::ptr::null(),
        &size,
        mem_dc,
        &src_pt,
        0,
        &blend,
        ULW_ALPHA,
    );

    SelectObject(mem_dc, old);
    DeleteObject(bmp);
    DeleteDC(mem_dc);
    ReleaseDC(0, screen_dc);
}
