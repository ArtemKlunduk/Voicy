//! Чтение URL активной вкладки браузера через UI Automation, с fallback на буфер
//! обмена. Хрупко по природе (зависит от браузера и его версии), поэтому fallback
//! на clipboard обязателен: фича «скачай» не должна падать целиком.
#![cfg(windows)]

use tracing::debug;
use uiautomation::controls::ControlType;
use uiautomation::types::{Handle, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::UIAutomation;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowTextW, IsWindowVisible,
};

/// Лучшая угаданная ссылка по порядку: (1) адресная строка активного окна,
/// (2) если фокус не на браузере, скан открытых окон браузеров (берём верхнее
/// по Z-order), (3) буфер обмена. None если нигде ничего похожего на URL.
pub fn active_url() -> Option<String> {
    if let Some(u) = url_from_foreground() {
        debug!("[url] from foreground");
        return Some(u);
    }
    if let Some(u) = url_from_any_browser() {
        debug!("[url] from browser window scan");
        return Some(u);
    }
    if let Some(u) = url_from_clipboard() {
        debug!("[url] from clipboard");
        return Some(u);
    }
    None
}

/// Грубая нормализация: похоже ли на URL. Адресная строка часто показывает без
/// схемы («soundcloud.com/artist/track»), поэтому достраиваем https://.
fn normalize_url(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s.contains(char::is_whitespace) || s.contains('@') {
        return None;
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(s.to_string());
    }
    // host[/path]: домен с буквенной зоной длиной >= 2 («example.com», «vk.ru»).
    let host = s.split('/').next().unwrap_or(s);
    let host_ok = host.contains('.')
        && host
            .rsplit('.')
            .next()
            .map_or(false, |tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
    if host_ok {
        Some(format!("https://{}", s))
    } else {
        None
    }
}

/// Пройтись по Edit-контролам конкретного окна и взять значение, похожее на URL.
/// Адресная строка браузера это контрол типа Edit.
fn url_from_window(auto: &UIAutomation, hwnd: isize) -> Option<String> {
    let root = auto.element_from_handle(Handle::from(hwnd)).ok()?;
    let cond = auto
        .create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::Edit as i32),
            None,
        )
        .ok()?;
    let edits = root.find_all(TreeScope::Descendants, &cond).ok()?;
    for e in edits {
        if let Ok(v) = e.get_property_value(UIProperty::ValueValue) {
            if let Ok(s) = v.get_string() {
                if let Some(u) = normalize_url(&s) {
                    return Some(u);
                }
            }
        }
    }
    None
}

/// URL из активного (переднего) окна.
fn url_from_foreground() -> Option<String> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == 0 {
        return None;
    }
    let auto = UIAutomation::new().ok()?;
    url_from_window(&auto, hwnd)
}

/// Класс окна (для отбора окон браузеров).
fn window_class(hwnd: isize) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Похож ли класс окна на окно браузера. Chrome/Edge/Brave/Chromium делят
/// «Chrome_WidgetWin_1» (его же используют Electron-приложения, но у них просто
/// не окажется URL-Edit, мы их молча пропустим). Firefox: «MozillaWindowClass».
fn is_browser_class(class: &str) -> bool {
    class == "Chrome_WidgetWin_1" || class.starts_with("Mozilla")
}

/// Собрать видимые окна браузеров в Z-order (верхнее первым) через EnumWindows.
unsafe extern "system" fn collect_browser_windows(hwnd: isize, lparam: isize) -> i32 {
    let out = &mut *(lparam as *mut Vec<isize>);
    if IsWindowVisible(hwnd) != 0 && is_browser_class(&window_class(hwnd)) {
        out.push(hwnd);
    }
    1 // продолжать перечисление
}

/// Когда фокус не на браузере (например пользователь в окне Voicy): пройтись по
/// открытым окнам браузеров и взять URL верхнего по Z-order (последняя активная
/// вкладка). EnumWindows отдаёт окна сверху вниз, поэтому берём первое сработавшее.
fn url_from_any_browser() -> Option<String> {
    let mut hwnds: Vec<isize> = Vec::new();
    unsafe {
        EnumWindows(Some(collect_browser_windows), &mut hwnds as *mut _ as isize);
    }
    if hwnds.is_empty() {
        return None;
    }
    let auto = UIAutomation::new().ok()?;
    for hwnd in hwnds {
        if let Some(u) = url_from_window(&auto, hwnd) {
            return Some(u);
        }
    }
    None
}

fn url_from_clipboard() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let txt = cb.get_text().ok()?;
    normalize_url(&txt)
}

fn window_title(hwnd: isize) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Диагностика (`voicy url debug [hwnd]`): вывалить окно (foreground или
/// явно переданное) и все его Edit и ComboBox контролы с именами, automation_id
/// и значениями. Нужна, чтобы понять, как браузер отдаёт адресную строку.
/// Явный hwnd снимает гонку фокуса при тестировании из фонового процесса.
pub fn dump_foreground(hwnd_override: Option<isize>) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let hwnd = hwnd_override.unwrap_or_else(|| unsafe { GetForegroundWindow() });
    if hwnd == 0 {
        return "нет foreground окна\n".into();
    }
    let _ = writeln!(out, "foreground hwnd={} title={:?}", hwnd, window_title(hwnd));
    let auto = match UIAutomation::new() {
        Ok(a) => a,
        Err(e) => return format!("{}UIA init error: {}\n", out, e),
    };
    let root = match auto.element_from_handle(Handle::from(hwnd)) {
        Ok(r) => r,
        Err(e) => return format!("{}element_from_handle error: {}\n", out, e),
    };
    for (label, ct) in [("Edit", ControlType::Edit), ("ComboBox", ControlType::ComboBox)] {
        let cond = match auto.create_property_condition(
            UIProperty::ControlType,
            Variant::from(ct as i32),
            None,
        ) {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(out, "[{}] cond error: {}", label, e);
                continue;
            }
        };
        match root.find_all(TreeScope::Descendants, &cond) {
            Ok(items) => {
                let _ = writeln!(out, "[{}] найдено {}", label, items.len());
                for (i, e) in items.iter().enumerate().take(40) {
                    let name = e.get_name().unwrap_or_default();
                    let aid = e.get_automation_id().unwrap_or_default();
                    let val = e
                        .get_property_value(UIProperty::ValueValue)
                        .ok()
                        .and_then(|v| v.get_string().ok())
                        .unwrap_or_default();
                    let _ = writeln!(out, "  #{} name={:?} aid={:?} value={:?}", i, name, aid, val);
                }
            }
            Err(e) => {
                let _ = writeln!(out, "[{}] find_all error: {}", label, e);
            }
        }
    }
    out
}
