//! Режим диктовки: печать произвольного Unicode-текста в окно, у которого сейчас
//! фокус, через Win32 `SendInput` с флагом `KEYEVENTF_UNICODE`.
//!
//! Почему так, а не через буфер обмена:
//!   - не затираем clipboard пользователя;
//!   - `KEYEVENTF_UNICODE` инжектит символ напрямую, не завися от раскладки и
//!     текущих модификаторов (Alt от хоткея уже отпущен к этому моменту, но
//!     даже если нет — Unicode-инжект не превратится в shortcut);
//!   - корректно работает с кириллицей (печатаем по UTF-16 code unit'ам).
#![cfg(windows)]

use tracing::info;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
};

fn kbd(scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0, // 0 + KEYEVENTF_UNICODE → символ берётся из wScan
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Напечатать `text` в активное окно как обычный клавиатурный ввод.
/// На каждый UTF-16 code unit шлём KeyDown+KeyUp с KEYEVENTF_UNICODE.
pub fn type_text(text: &str) {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        inputs.push(kbd(unit, KEYEVENTF_UNICODE));
        inputs.push(kbd(unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    if inputs.is_empty() {
        return;
    }
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    info!("[dictation] typed «{}» ({} input events accepted)", text, sent);
}
