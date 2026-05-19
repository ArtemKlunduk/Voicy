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
    on_press: impl Fn() + Send + Sync + 'static,
    on_release: impl Fn() + Send + Sync + 'static,
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

    // Параллельный Win32 RegisterHotKey fallback — нужен потому что
    // rdev::listen использует WH_KEYBOARD_LL hook, который теоретически
    // должен ловить всё, но на практике WebView2 окно может перехватывать
    // Alt-комбинации (Alt — менюшный модификатор Windows). RegisterHotKey —
    // system-wide accelerator, работает 100% даже при focused-Voicy-окне.
    // Hook остаётся primary (он ловит press/release раздельно для hold-to-
    // record), а RegisterHotKey — даёт «click»-семантику press+release как
    // fallback на случай если hook молчит.
    #[cfg(windows)]
    {
        let hk_clone = hotkey.clone();
        let on_press_global = on_press.clone();
        let on_release_global = on_release.clone();
        std::thread::spawn(move || {
            register_hotkey_loop(hk_clone, on_press_global, on_release_global);
        });
    }

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

// ──────────────────────────────────────────────────────────────────────
// Win32 RegisterHotKey fallback
// ──────────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn vk_for_main_key(s: &str) -> Option<u32> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let s = s.to_ascii_lowercase();
    let code = match s.as_str() {
        "a" => 0x41, "b" => 0x42, "c" => 0x43, "d" => 0x44,
        "e" => 0x45, "f" => 0x46, "g" => 0x47, "h" => 0x48,
        "i" => 0x49, "j" => 0x4A, "k" => 0x4B, "l" => 0x4C,
        "m" => 0x4D, "n" => 0x4E, "o" => 0x4F, "p" => 0x50,
        "q" => 0x51, "r" => 0x52, "s" => 0x53, "t" => 0x54,
        "u" => 0x55, "v" => 0x56, "w" => 0x57, "x" => 0x58,
        "y" => 0x59, "z" => 0x5A,
        "0" => 0x30, "1" => 0x31, "2" => 0x32, "3" => 0x33,
        "4" => 0x34, "5" => 0x35, "6" => 0x36, "7" => 0x37,
        "8" => 0x38, "9" => 0x39,
        "f1" => VK_F1 as u32, "f2" => VK_F2 as u32, "f3" => VK_F3 as u32,
        "f4" => VK_F4 as u32, "f5" => VK_F5 as u32, "f6" => VK_F6 as u32,
        "f7" => VK_F7 as u32, "f8" => VK_F8 as u32, "f9" => VK_F9 as u32,
        "f10" => VK_F10 as u32, "f11" => VK_F11 as u32, "f12" => VK_F12 as u32,
        "space" => VK_SPACE as u32,
        "tab" => VK_TAB as u32,
        "enter" | "return" => VK_RETURN as u32,
        "esc" | "escape" => VK_ESCAPE as u32,
        _ => return None,
    };
    Some(code)
}

/// Регистрирует system-wide accelerator через `RegisterHotKey` и крутит
/// message-loop, ожидая `WM_HOTKEY`. На каждое сообщение вызываем
/// on_press + on_release «склейкой» (Click-семантика — RegisterHotKey
/// не различает press/release). Этого хватает для запуска pipeline
/// (запись → ASR → отправка) даже если rdev-hook молчит.
#[cfg(windows)]
fn register_hotkey_loop(
    hotkey: Hotkey,
    on_press: Arc<impl Fn() + Send + Sync + 'static>,
    on_release: Arc<impl Fn() + Send + Sync + 'static>,
) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey,
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };

    let mut mods: u32 = MOD_NOREPEAT as u32;
    let mut want_alt = false;
    let mut want_ctrl = false;
    let mut want_shift = false;
    let mut want_win = false;
    for m in &hotkey.modifiers {
        match m.to_ascii_lowercase().as_str() {
            "alt" => { mods |= MOD_ALT as u32; want_alt = true; }
            "ctrl" | "control" => { mods |= MOD_CONTROL as u32; want_ctrl = true; }
            "shift" => { mods |= MOD_SHIFT as u32; want_shift = true; }
            "win" | "meta" | "super" | "cmd" => { mods |= MOD_WIN as u32; want_win = true; }
            _ => {}
        }
    }
    let Some(vk) = vk_for_main_key(&hotkey.key) else {
        warn!("[hotkey/win32] не смог распарсить main key '{}', RegisterHotKey skip", hotkey.key);
        return;
    };

    const HOTKEY_ID: i32 = 0xC0DEC0; // уникальный ID для нашего хоткея

    unsafe {
        if RegisterHotKey(0, HOTKEY_ID, mods, vk) == 0 {
            warn!("[hotkey/win32] RegisterHotKey FAILED — fallback недоступен");
            return;
        }
    }
    info!("[hotkey/win32] RegisterHotKey OK ({} + {})", hotkey.modifiers.join("+"), hotkey.key);

    // Pump messages forever; WM_HOTKEY → trigger press, затем polling
    // GetAsyncKeyState чтобы дождаться РЕАЛЬНОГО release (а не давать
    // фиксированное окно). Это критично для hold-to-record: юзер должен
    // мочь говорить столько сколько держит клавиши.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let r = GetMessageW(&mut msg, 0, 0, 0);
            if r <= 0 {
                break;
            }
            if msg.message == WM_HOTKEY && msg.wParam == HOTKEY_ID as usize {
                info!("[hotkey/win32] WM_HOTKEY fired — start recording");
                (on_press)();
                let cb_release = on_release.clone();
                std::thread::spawn(move || {
                    poll_until_release(vk, want_alt, want_ctrl, want_shift, want_win);
                    info!("[hotkey/win32] keys released — stop recording");
                    (cb_release)();
                });
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnregisterHotKey(0, HOTKEY_ID);
    }
}

/// Polling: ждём пока ВСЕ нужные клавиши не отпустятся.
/// Опрашиваем GetAsyncKeyState каждые 20мс. Минимум 200мс держим в
/// «recording» состоянии чтобы дёрганый press дал какой-то wav (иначе
/// audio buffer пустой и whisper ломается).
#[cfg(windows)]
fn poll_until_release(vk: u32, want_alt: bool, want_ctrl: bool, want_shift: bool, want_win: bool) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    let start = std::time::Instant::now();
    const MIN_HOLD_MS: u128 = 200;
    const MAX_HOLD_MS: u128 = 30_000; // safety: 30 сек

    loop {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let elapsed = start.elapsed().as_millis();
        if elapsed >= MAX_HOLD_MS {
            return;
        }
        unsafe {
            let key_down = (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0;
            let alt_down = (GetAsyncKeyState(VK_MENU as i32) as u16) & 0x8000 != 0;
            let ctrl_down = (GetAsyncKeyState(VK_CONTROL as i32) as u16) & 0x8000 != 0;
            let shift_down = (GetAsyncKeyState(VK_SHIFT as i32) as u16) & 0x8000 != 0;
            let win_down = ((GetAsyncKeyState(VK_LWIN as i32) as u16) & 0x8000 != 0)
                || ((GetAsyncKeyState(VK_RWIN as i32) as u16) & 0x8000 != 0);

            // Если ЛЮБАЯ нужная клавиша отпущена (и прошло min hold) — стоп.
            let all_held = key_down
                && (!want_alt || alt_down)
                && (!want_ctrl || ctrl_down)
                && (!want_shift || shift_down)
                && (!want_win || win_down);
            if !all_held && elapsed >= MIN_HOLD_MS {
                return;
            }
        }
    }
}
