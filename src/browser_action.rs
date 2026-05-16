//! Голосовые команды для управления видео-плеером в браузере.
//!
//! YouTube/Twitch имеют богатый набор горячих клавиш, которые работают пока
//! плеер в фокусе. Мы парсим голосовую команду в `BrowserAction`, потом
//! шлём соответствующий keystroke через Win32 SendInput в активное окно.
//!
//! Архитектура:
//!   `parse(text) -> Option<BrowserAction>`  — распознать команду
//!   `dispatch(action)`                       — выполнить
//!
//! Никакого CDP / WebDriver не используется — только клавиатурные горячие
//! клавиши. Это самый стабильный способ: YouTube/Twitch держат их годами,
//! независимо от того как меняется DOM.

use std::thread;
use std::time::Duration;

#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_DOWN, VK_F,
    VK_LEFT, VK_M, VK_RIGHT, VK_SPACE, VK_UP,
};

/// Действие, которое можно выполнить в браузере с активным видео-плеером.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    /// Увеличить громкость. YouTube: одно нажатие ↑ = +5%. N — сколько раз нажать.
    VolumeUp(u8),
    /// Уменьшить громкость. N — сколько раз нажать.
    VolumeDown(u8),
    /// Полный экран (F).
    Fullscreen,
    /// Play/pause (Space).
    PlayPause,
    /// Перемотать вперёд. YouTube: → = +5s. N — сколько раз нажать.
    SeekForward(u8),
    /// Перемотать назад.
    SeekBackward(u8),
    /// Mute toggle (M).
    Mute,
}

/// Парсит распознанный текст. Возвращает Some(action) если команда матчится.
///
/// Грамматика:
///   "громче [на N процентов]"     → VolumeUp(N / 5 если указано, иначе 1)
///   "тише [на N процентов]"        → VolumeDown
///   "полный экран" / "фуллскрин"  → Fullscreen
///   "пауза" / "стоп" / "играй"    → PlayPause
///   "перемотай вперёд [на N сек]" → SeekForward(N / 5)
///   "перемотай назад [на N сек]"   → SeekBackward
///   "выключи звук" / "mute"        → Mute
pub fn parse(text: &str) -> Option<BrowserAction> {
    let t = normalize(text);
    if t.is_empty() {
        return None;
    }

    // Mute — ловим раньше "выключи" чтобы не путать с pause.
    if t.contains("выключи звук")
        || t.contains("включи звук")
        || t == "mute"
        || t.contains("без звука")
    {
        return Some(BrowserAction::Mute);
    }

    // Volume
    if let Some(n) = extract_percent(&t, &["громче", "сделай громче", "увеличь громкость", "погромче"]) {
        // YouTube ↑ = +5%. N% / 5 = количество нажатий, минимум 1.
        let presses = ((n + 4) / 5).max(1).min(20) as u8;
        return Some(BrowserAction::VolumeUp(presses));
    }
    if t.contains("громче") || t.contains("погромче") {
        return Some(BrowserAction::VolumeUp(2)); // ~10% по умолчанию
    }
    if let Some(n) = extract_percent(&t, &["тише", "сделай тише", "уменьши громкость", "потише"]) {
        let presses = ((n + 4) / 5).max(1).min(20) as u8;
        return Some(BrowserAction::VolumeDown(presses));
    }
    if t.contains("тише") || t.contains("потише") {
        return Some(BrowserAction::VolumeDown(2));
    }

    // Fullscreen
    if t.contains("полный экран")
        || t.contains("во весь экран")
        || t.contains("фуллскрин")
        || t.contains("на весь экран")
    {
        return Some(BrowserAction::Fullscreen);
    }

    // Seek
    if let Some(n) = extract_seconds(&t, &["перемотай вперёд", "перемотай вперед", "вперёд на", "вперед на"]) {
        let presses = ((n + 4) / 5).max(1).min(60) as u8;
        return Some(BrowserAction::SeekForward(presses));
    }
    if let Some(n) = extract_seconds(&t, &["перемотай назад", "назад на"]) {
        let presses = ((n + 4) / 5).max(1).min(60) as u8;
        return Some(BrowserAction::SeekBackward(presses));
    }
    if t.contains("перемотай вперёд") || t.contains("перемотай вперед") {
        return Some(BrowserAction::SeekForward(2)); // ~10s
    }
    if t.contains("перемотай назад") {
        return Some(BrowserAction::SeekBackward(2));
    }

    // Play/pause — самый общий, проверяем последним.
    if t == "пауза"
        || t == "стоп"
        || t == "остановись"
        || t == "играй"
        || t == "продолжай"
        || t == "запусти"
        || t.starts_with("пауза")
        || t.starts_with("стоп ")
        || t.contains("поставь на паузу")
    {
        return Some(BrowserAction::PlayPause);
    }

    None
}

/// Выполнить действие — отправить keystroke в активное окно.
#[cfg(windows)]
pub fn dispatch(action: BrowserAction) {
    match action {
        BrowserAction::VolumeUp(n) => press_repeat(VK_UP, n),
        BrowserAction::VolumeDown(n) => press_repeat(VK_DOWN, n),
        BrowserAction::Fullscreen => press_once(VK_F),
        BrowserAction::PlayPause => press_once(VK_SPACE),
        BrowserAction::SeekForward(n) => press_repeat(VK_RIGHT, n),
        BrowserAction::SeekBackward(n) => press_repeat(VK_LEFT, n),
        BrowserAction::Mute => press_once(VK_M),
    }
}

#[cfg(not(windows))]
pub fn dispatch(_action: BrowserAction) {
    // no-op для не-Windows
}

// ────────────────────────────────────────────────────────────────────
// Хелперы парсинга
// ────────────────────────────────────────────────────────────────────

/// Нормализация: lowercase + пунктуация → пробелы + сжатие пробелов.
fn normalize(text: &str) -> String {
    let lower: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '\t' { c } else { ' ' })
        .collect();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ищет «<trigger> на N процентов» (или просто «<trigger> N процентов»).
/// Возвращает Some(N) если нашли число рядом с одним из триггеров и словом «процент».
fn extract_percent(text: &str, triggers: &[&str]) -> Option<u32> {
    for trig in triggers {
        if let Some(pos) = text.find(trig) {
            let after = &text[pos + trig.len()..];
            // Должно встречаться "процент" чтобы это была команда с %
            if !after.contains("процент") && !after.contains("процентов") {
                continue;
            }
            if let Some(n) = find_number(after) {
                return Some(n);
            }
        }
    }
    None
}

/// Ищет «<trigger> N секунд» (или «<trigger> на N секунд»).
fn extract_seconds(text: &str, triggers: &[&str]) -> Option<u32> {
    for trig in triggers {
        if let Some(pos) = text.find(trig) {
            let after = &text[pos + trig.len()..];
            if let Some(n) = find_number(after) {
                return Some(n);
            }
        }
    }
    None
}

/// Найти первое целое число в строке. «на 10 секунд» → Some(10).
fn find_number(s: &str) -> Option<u32> {
    let mut current = String::new();
    let mut result = None;
    for c in s.chars() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if !current.is_empty() {
            if let Ok(n) = current.parse::<u32>() {
                result = Some(n);
                break;
            }
            current.clear();
        }
    }
    if result.is_none() && !current.is_empty() {
        if let Ok(n) = current.parse::<u32>() {
            result = Some(n);
        }
    }
    result
}

// ────────────────────────────────────────────────────────────────────
// Win32 SendInput хелперы
// ────────────────────────────────────────────────────────────────────

#[cfg(windows)]
unsafe fn send_key(vk: VIRTUAL_KEY, key_up: bool) {
    let mut input: INPUT = std::mem::zeroed();
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: vk,
        wScan: 0,
        dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
        time: 0,
        dwExtraInfo: 0,
    };
    SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
}

#[cfg(windows)]
fn press_once(vk: VIRTUAL_KEY) {
    unsafe {
        send_key(vk, false);
        thread::sleep(Duration::from_millis(30));
        send_key(vk, true);
    }
}

#[cfg(windows)]
fn press_repeat(vk: VIRTUAL_KEY, count: u8) {
    for _ in 0..count {
        press_once(vk);
        thread::sleep(Duration::from_millis(40));
    }
}

// ────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_variants() {
        assert_eq!(parse("полный экран"), Some(BrowserAction::Fullscreen));
        assert_eq!(parse("Включи полный экран!"), Some(BrowserAction::Fullscreen));
        assert_eq!(parse("на весь экран"), Some(BrowserAction::Fullscreen));
        assert_eq!(parse("фуллскрин"), Some(BrowserAction::Fullscreen));
    }

    #[test]
    fn volume_with_percent() {
        // 10% / 5% per press = 2 presses
        assert_eq!(parse("сделай громче на 10 процентов"), Some(BrowserAction::VolumeUp(2)));
        // 25% / 5 = 5 presses
        assert_eq!(parse("громче на 25 процентов"), Some(BrowserAction::VolumeUp(5)));
        // tише
        assert_eq!(parse("тише на 10 процентов"), Some(BrowserAction::VolumeDown(2)));
    }

    #[test]
    fn volume_no_percent() {
        // Дефолт — ~10% (2 нажатия)
        assert_eq!(parse("громче"), Some(BrowserAction::VolumeUp(2)));
        assert_eq!(parse("сделай громче"), Some(BrowserAction::VolumeUp(2)));
        assert_eq!(parse("тише"), Some(BrowserAction::VolumeDown(2)));
        assert_eq!(parse("потише"), Some(BrowserAction::VolumeDown(2)));
    }

    #[test]
    fn play_pause() {
        assert_eq!(parse("пауза"), Some(BrowserAction::PlayPause));
        assert_eq!(parse("стоп"), Some(BrowserAction::PlayPause));
        assert_eq!(parse("играй"), Some(BrowserAction::PlayPause));
        assert_eq!(parse("поставь на паузу"), Some(BrowserAction::PlayPause));
    }

    #[test]
    fn mute_variants() {
        assert_eq!(parse("выключи звук"), Some(BrowserAction::Mute));
        assert_eq!(parse("без звука"), Some(BrowserAction::Mute));
        assert_eq!(parse("mute"), Some(BrowserAction::Mute));
    }

    #[test]
    fn seek_with_seconds() {
        // 30s / 5s per press = 6
        assert_eq!(parse("перемотай вперёд на 30 секунд"), Some(BrowserAction::SeekForward(6)));
        assert_eq!(parse("перемотай назад на 15 секунд"), Some(BrowserAction::SeekBackward(3)));
    }

    #[test]
    fn seek_no_seconds() {
        // дефолт ~10s = 2 нажатия
        assert_eq!(parse("перемотай вперёд"), Some(BrowserAction::SeekForward(2)));
        assert_eq!(parse("перемотай назад"), Some(BrowserAction::SeekBackward(2)));
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(parse("привет мир"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("открой ютуб котики"), None);
    }
}
