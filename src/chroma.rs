//! Chroma-key прозрачность для overlay-окна.
//!
//! WebView2 на Windows игнорирует RGBA-альфу на некоторых GPU/композиторах —
//! даёт белый прямоугольник вокруг прозрачного HTML. Обход — `WS_EX_LAYERED`
//! + `SetLayeredWindowAttributes(LWA_COLORKEY)`: Win32-компоновщик делает
//! пиксели заданного цвета полностью прозрачными.
//!
//! Цвет ключа — пурпурный `#FF00FF`. В HTML overlay body имеет такой же фон.
//! Анти-алиасинг краёв orb может дать слабый пурпурный halo (1-2 пикселя),
//! но в палитре проекта пурпурного нет — крае-эффект минимален.

/// RGB-значение chroma-ключа (пурпурный). Должно совпадать с body-фоном HTML.
pub const CHROMA_KEY_RGB: (u8, u8, u8) = (0xFF, 0x00, 0xFF);

#[cfg(windows)]
pub fn apply_chroma_key(window: &tao::window::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{COLORREF, HWND};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW,
        GWL_EXSTYLE, LWA_COLORKEY, WS_EX_LAYERED,
    };

    let handle = match window.window_handle() {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("[chroma] window_handle err: {}", e);
            return;
        }
    };
    let hwnd = match handle.as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get() as HWND,
        other => {
            tracing::warn!("[chroma] unexpected handle type: {:?}", other);
            return;
        }
    };
    // SAFETY: hwnd валиден пока существует Window. SetWindowLongPtrW/
    // SetLayeredWindowAttributes — стандартный win32 API, потокобезопасный
    // в рамках одного hwnd.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED as isize);
        let (r, g, b) = CHROMA_KEY_RGB;
        // COLORREF = 0x00BBGGRR
        let key: COLORREF = (b as u32) << 16 | (g as u32) << 8 | (r as u32);
        SetLayeredWindowAttributes(hwnd, key, 0, LWA_COLORKEY);
    }
}

#[cfg(not(windows))]
pub fn apply_chroma_key(_window: &tao::window::Window) {
    // no-op для не-Windows платформ
}
