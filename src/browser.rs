//! Голосовые команды для открытия браузера.
//! Никаких API-ключей — используем прямые URL поисковиков.

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

/// Запомненный последний YouTube-запрос. Нужен чтобы команда «включи первое»
/// знала откуда взять список результатов — повторно скрейпит ту же поисковую
/// страницу и достаёт N-й videoId.
static LAST_YOUTUBE_QUERY: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn yt_query_slot() -> &'static Mutex<Option<String>> {
    LAST_YOUTUBE_QUERY.get_or_init(|| Mutex::new(None))
}

pub fn last_youtube_query() -> Option<String> {
    yt_query_slot().lock().clone()
}

pub fn set_last_youtube_query(q: &str) {
    *yt_query_slot().lock() = Some(q.to_string());
}

pub struct BrowserCmd {
    pub provider: &'static str,
    pub query: String,
    pub url: String,
}

/// Парсит текст на предмет браузерных команд.
/// Триггеры: "youtube", "ютуб", "открой youtube", "открой ютуб"
/// Формат: "[открой] <триггер>[,] <запрос>"
pub fn parse(text: &str) -> Option<BrowserCmd> {
    let normalized: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '\t' { c } else { ' ' })
        .collect();
    let t: String = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return None;
    }

    // Список триггеров: (текст_триггера, название_провайдера, URL-шаблон)
    let triggers: &[(&str, &str, &str)] = &[
        ("открой youtube", "YouTube", "https://www.youtube.com/results?search_query={}"),
        ("открой ютуб", "YouTube", "https://www.youtube.com/results?search_query={}"),
        ("youtube", "YouTube", "https://www.youtube.com/results?search_query={}"),
        ("ютуб", "YouTube", "https://www.youtube.com/results?search_query={}"),
        ("открой google", "Google", "https://www.google.com/search?q={}"),
        ("открой гугл", "Google", "https://www.google.com/search?q={}"),
        ("google", "Google", "https://www.google.com/search?q={}"),
        ("гугл", "Google", "https://www.google.com/search?q={}"),
        ("открой tiktok", "TikTok", "https://www.tiktok.com/search?q={}"),
        ("открой тикток", "TikTok", "https://www.tiktok.com/search?q={}"),
        ("tiktok", "TikTok", "https://www.tiktok.com/search?q={}"),
        ("тикток", "TikTok", "https://www.tiktok.com/search?q={}"),
        ("открой twitch", "Twitch", "https://www.twitch.tv/search?term={}"),
        ("открой твич", "Twitch", "https://www.twitch.tv/search?term={}"),
        ("twitch", "Twitch", "https://www.twitch.tv/search?term={}"),
        ("твич", "Twitch", "https://www.twitch.tv/search?term={}"),
    ];

    for &(trigger, provider, url_tpl) in triggers {
        if let Some(pos) = t.find(trigger) {
            let after = &t[pos + trigger.len()..].trim_start();
            let query = after.to_string();
            if query.is_empty() {
                // Если запроса нет — просто открываем главную страницу провайдера
                let home = match provider {
                    "YouTube" => "https://www.youtube.com",
                    "Google" => "https://www.google.com",
                    "TikTok" => "https://www.tiktok.com",
                    "Twitch" => "https://www.twitch.tv",
                    _ => "https://www.google.com",
                };
                return Some(BrowserCmd {
                    provider,
                    query: String::new(),
                    url: home.to_string(),
                });
            }
            let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
            let url = url_tpl.replace("{}", &encoded);
            // Запоминаем YouTube-запрос для команды «включи первое».
            if provider == "YouTube" && !query.is_empty() {
                set_last_youtube_query(&query);
            }
            return Some(BrowserCmd {
                provider,
                query,
                url,
            });
        }
    }

    None
}

/// Открывает URL в системном браузере Windows.
pub fn open(url: &str) -> Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .context("open browser")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────
// «Включи N-е видео» — скрейп YouTube + открытие /watch?v=...
// ────────────────────────────────────────────────────────────────────

/// Парсит команды вида «включи первое [видео]», «запусти второе», «третье».
/// Возвращает 0-based индекс желаемого результата.
///
/// Поддерживаем 1..=10 на словах + цифрами.
pub fn parse_play_nth(text: &str) -> Option<usize> {
    let t: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Триггеры действия — необязательны, имя порядкового числа уже достаточно.
    let has_trigger = ["включи", "запусти", "открой", "проиграй", "плей"]
        .iter()
        .any(|t_str| t.contains(t_str));

    let ordinals: &[(&str, usize)] = &[
        ("первое", 0), ("первый", 0), ("первая", 0), ("1", 0),
        ("второе", 1), ("второй", 1), ("вторая", 1), ("2", 1),
        ("третье", 2), ("третий", 2), ("третья", 2), ("3", 2),
        ("четвёртое", 3), ("четвертое", 3), ("четвёртый", 3), ("четвертый", 3), ("4", 3),
        ("пятое", 4), ("пятый", 4), ("пятая", 4), ("5", 4),
        ("шестое", 5), ("шестой", 5), ("6", 5),
        ("седьмое", 6), ("седьмой", 6), ("7", 6),
        ("восьмое", 7), ("восьмой", 7), ("8", 7),
        ("девятое", 8), ("девятый", 8), ("9", 8),
        ("десятое", 9), ("десятый", 9), ("10", 9),
    ];

    for &(word, idx) in ordinals {
        // Ищем как отдельное слово, чтобы «1» не матчила в произвольном числе.
        let needle = format!(" {} ", word);
        let padded = format!(" {} ", t);
        if padded.contains(&needle) {
            // Требуем либо триггер, либо явное слово «видео»/«ролик».
            if has_trigger || t.contains("видео") || t.contains("ролик") {
                return Some(idx);
            }
        }
    }
    None
}

const YT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                              (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Открыть N-е (0-based) видео из последнего YouTube-поиска.
/// Скрейпит результаты повторно, достаёт videoId, открывает /watch?v=ID&autoplay=1.
pub fn play_nth_youtube_result(n: usize) -> Result<String> {
    let query = last_youtube_query()
        .ok_or_else(|| anyhow!("нет предыдущего YouTube-запроса — скажи сначала «открой ютуб <запрос>»"))?;
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let search_url = format!("https://www.youtube.com/results?search_query={}&hl=en", encoded);

    let html = ureq::get(&search_url)
        .set("User-Agent", YT_USER_AGENT)
        .timeout(Duration::from_secs(10))
        .call()
        .context("youtube search")?
        .into_string()
        .context("read body")?;

    let ids = extract_video_ids(&html);
    if ids.is_empty() {
        return Err(anyhow!("YouTube не вернул ни одного видео (антибот?)"));
    }
    let id = ids.get(n).ok_or_else(|| {
        anyhow!(
            "результат №{} не найден (всего {})",
            n + 1,
            ids.len()
        )
    })?;
    let watch_url = format!("https://www.youtube.com/watch?v={}&autoplay=1", id);
    open(&watch_url)?;
    Ok(watch_url)
}

/// Найти videoId-ы органических результатов в HTML страницы поиска YouTube.
/// Ищем pattern `"videoRenderer":{"videoId":"XXX"` — это даёт topические видео,
/// не рекламу и не «people also watched».
fn extract_video_ids(html: &str) -> Vec<String> {
    let needle = r#""videoRenderer":{"videoId":""#;
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(idx) = html[pos..].find(needle) {
        let abs = pos + idx + needle.len();
        // videoId — 11 chars: a-zA-Z0-9_-
        let slice: String = html[abs..]
            .chars()
            .take(11)
            .collect();
        if slice.len() == 11 && slice.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            if !out.contains(&slice) {
                out.push(slice);
            }
        }
        pos = abs + 11;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_nth_basic() {
        assert_eq!(parse_play_nth("включи первое видео"), Some(0));
        assert_eq!(parse_play_nth("запусти второе"), Some(1));
        assert_eq!(parse_play_nth("открой третий ролик"), Some(2));
        assert_eq!(parse_play_nth("плей пятое"), Some(4));
        assert_eq!(parse_play_nth("первое видео"), Some(0));
    }

    #[test]
    fn play_nth_with_digits() {
        assert_eq!(parse_play_nth("включи 1 видео"), Some(0));
        assert_eq!(parse_play_nth("запусти 3"), Some(2));
    }

    #[test]
    fn play_nth_requires_trigger_or_keyword() {
        // Без триггера и без слова «видео»/«ролик» — не должно матчиться.
        assert_eq!(parse_play_nth("первое"), None);
        assert_eq!(parse_play_nth("второе сегодня"), None);
    }

    #[test]
    fn play_nth_unrelated() {
        assert_eq!(parse_play_nth("привет мир"), None);
        assert_eq!(parse_play_nth(""), None);
        assert_eq!(parse_play_nth("открой ютуб котики"), None);
    }

    #[test]
    fn extract_ids_simple() {
        let html = r#"prelude "videoRenderer":{"videoId":"abc12345678","title":...
                      ad blob "videoRenderer":{"videoId":"DEFghijklmn","title":...
                      "promotedVideoRenderer":{"videoId":"shouldskip0","title""#;
        let ids = extract_video_ids(html);
        assert_eq!(ids, vec!["abc12345678", "DEFghijklmn"]);
    }
}
