//! Глобальный hotkey listener через rdev.
//! Парсит конфиг (`["alt"] + "x"`) → подходящие rdev::Key.
//! При нажатии комбо вызывает on_press, при отпускании — on_release.

use crate::config::Hotkey;
use parking_lot::Mutex;
use rdev::{listen, Event, EventType, Key};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Mod {
    Alt,
    Ctrl,
    Shift,
    Win,
}

fn modifier_of(key: Key) -> Option<Mod> {
    use Key::*;
    match key {
        Alt | AltGr => Some(Mod::Alt),
        ControlLeft | ControlRight => Some(Mod::Ctrl),
        ShiftLeft | ShiftRight => Some(Mod::Shift),
        MetaLeft | MetaRight => Some(Mod::Win),
        _ => None,
    }
}

fn parse_modifier(s: &str) -> Option<Mod> {
    match s.to_ascii_lowercase().as_str() {
        "alt" => Some(Mod::Alt),
        "ctrl" | "control" => Some(Mod::Ctrl),
        "shift" => Some(Mod::Shift),
        "win" | "meta" | "super" | "cmd" => Some(Mod::Win),
        _ => None,
    }
}

/// "x" → Key::KeyX, "f5" → Key::F5, "space" → Key::Space, etc.
/// Возвращает None если строка нераспознана.
fn parse_main_key(s: &str) -> Option<Key> {
    let s = s.to_ascii_lowercase();
    use Key::*;
    match s.as_str() {
        "a" => Some(KeyA), "b" => Some(KeyB), "c" => Some(KeyC), "d" => Some(KeyD),
        "e" => Some(KeyE), "f" => Some(KeyF), "g" => Some(KeyG), "h" => Some(KeyH),
        "i" => Some(KeyI), "j" => Some(KeyJ), "k" => Some(KeyK), "l" => Some(KeyL),
        "m" => Some(KeyM), "n" => Some(KeyN), "o" => Some(KeyO), "p" => Some(KeyP),
        "q" => Some(KeyQ), "r" => Some(KeyR), "s" => Some(KeyS), "t" => Some(KeyT),
        "u" => Some(KeyU), "v" => Some(KeyV), "w" => Some(KeyW), "x" => Some(KeyX),
        "y" => Some(KeyY), "z" => Some(KeyZ),
        "0" => Some(Num0), "1" => Some(Num1), "2" => Some(Num2), "3" => Some(Num3),
        "4" => Some(Num4), "5" => Some(Num5), "6" => Some(Num6), "7" => Some(Num7),
        "8" => Some(Num8), "9" => Some(Num9),
        "f1" => Some(F1), "f2" => Some(F2), "f3" => Some(F3), "f4" => Some(F4),
        "f5" => Some(F5), "f6" => Some(F6), "f7" => Some(F7), "f8" => Some(F8),
        "f9" => Some(F9), "f10" => Some(F10), "f11" => Some(F11), "f12" => Some(F12),
        "space" => Some(Space),
        "tab" => Some(Tab),
        "enter" | "return" => Some(Return),
        "esc" | "escape" => Some(Escape),
        "backspace" => Some(Backspace),
        _ => None,
    }
}

/// Начать слушать клавиатуру глобально. **Блокирует** текущий тред — крути
/// его в отдельном треде.
pub fn listen_blocking(
    hotkey: Hotkey,
    on_press: impl Fn() + Send + 'static,
    on_release: impl Fn() + Send + 'static,
) {
    let required_mods: HashSet<Mod> = hotkey
        .modifiers
        .iter()
        .filter_map(|m| parse_modifier(m))
        .collect();
    let target_key = match parse_main_key(&hotkey.key) {
        Some(k) => k,
        None => {
            warn!("[hotkey] неизвестная клавиша '{}', fallback на X", hotkey.key);
            Key::KeyX
        }
    };
    info!(
        "[hotkey] listener active: {} + {}",
        hotkey.modifiers.join(" + "),
        hotkey.key
    );

    let state = Arc::new(Mutex::new(HotkeyState {
        mods_held: HashSet::new(),
        recording: false,
    }));
    let on_press = Arc::new(on_press);
    let on_release = Arc::new(on_release);

    let cb_state = state.clone();
    let cb_press = on_press.clone();
    let cb_release = on_release.clone();
    let req_mods = required_mods.clone();

    info!("[hotkey] entering rdev::listen loop…");
    if let Err(e) = listen(move |event: Event| {
        // Глобальный low-level hook видит ВСЕ нажатия клавиатуры — иначе никак,
        // так работает Windows WH_KEYBOARD_LL. Но мы ничего не логируем и не
        // сохраняем — только проверяем совпадение с настроенным хоткеем и
        // вызываем callback при match. Все остальные клавиши просто игнорятся.
        match event.event_type {
            EventType::KeyPress(k) => {
                if let Some(m) = modifier_of(k) {
                    cb_state.lock().mods_held.insert(m);
                    return;
                }
                if k == target_key {
                    let mut st = cb_state.lock();
                    if req_mods.is_subset(&st.mods_held) && !st.recording {
                        st.recording = true;
                        drop(st);
                        info!("[hotkey] ▶ combo triggered");
                        cb_press();
                    }
                }
            }
            EventType::KeyRelease(k) => {
                if let Some(m) = modifier_of(k) {
                    let mut st = cb_state.lock();
                    st.mods_held.remove(&m);
                    if st.recording && req_mods.contains(&m) {
                        st.recording = false;
                        drop(st);
                        info!("[hotkey] ⏹ combo released");
                        cb_release();
                    }
                    return;
                }
                if k == target_key {
                    let mut st = cb_state.lock();
                    if st.recording {
                        st.recording = false;
                        drop(st);
                        info!("[hotkey] ⏹ combo released");
                        cb_release();
                    }
                }
            }
            _ => {}
        }
    }) {
        warn!("[hotkey] listen FAILED to start: {:?}", e);
    } else {
        warn!("[hotkey] listen exited cleanly (unexpected)");
    }
}

struct HotkeyState {
    mods_held: HashSet<Mod>,
    recording: bool,
}
