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

/// Парс «включи [видео] X Y Z» / «найди [видео] X Y Z» — где X Y Z это
/// часть названия видео для disambiguation. Возвращает Vec<keyword>.
///
/// Полезно когда ordinal'ы ненадёжны: YouTube ranking меняется между
/// запросами (A/B), и «второе видео» может оказаться разным. Если юзер
/// помнит часть названия — «включи асмр одноклассница» точно найдёт.
///
/// Триггер: «включи/запусти/проиграй/плей/найди» + слово «видео» опционально.
/// НЕ матчится если в тексте есть ordinal (тогда parse_play_nth справится).
pub fn parse_play_by_title(text: &str) -> Option<Vec<String>> {
    let t: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Если уже распарсилось как ordinal — не лезем
    if parse_play_nth(text).is_some() {
        return None;
    }

    let triggers = ["включи", "запусти", "проиграй", "плей", "найди"];
    let mut trigger_pos = None;
    let mut trigger_len = 0;
    for &tr in &triggers {
        if let Some(p) = t.find(tr) {
            if trigger_pos.is_none() || p < trigger_pos.unwrap() {
                trigger_pos = Some(p);
                trigger_len = tr.len();
            }
        }
    }
    let Some(tp) = trigger_pos else { return None };
    let after = t[tp + trigger_len..].trim();

    // Убираем слова «видео»/«ролик» если есть — они не часть title
    let kws: Vec<String> = after
        .split_whitespace()
        .filter(|w| !matches!(*w, "видео" | "ролик" | "это" | "то" | "на" | "ютубе"))
        .map(|s| s.to_string())
        .collect();

    if kws.is_empty() || kws.iter().all(|k| k.chars().count() < 2) {
        return None;
    }
    Some(kws)
}

const YT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                              (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// Один результат поиска: videoId + title для title-based matching.
#[derive(Debug, Clone)]
pub struct YtResult {
    pub video_id: String,
    pub title: String,
}

/// Открыть N-е (0-based) видео из последнего YouTube-поиска.
/// Скрейпит результаты повторно, достаёт videoId, открывает /watch?v=ID&autoplay=1.
pub fn play_nth_youtube_result(n: usize) -> Result<String> {
    let results = fetch_youtube_results()?;
    if results.is_empty() {
        return Err(anyhow!("YouTube не вернул ни одного видео (антибот?)"));
    }
    let item = results.get(n).ok_or_else(|| {
        anyhow!("результат №{} не найден (всего {})", n + 1, results.len())
    })?;
    let watch_url = format!("https://www.youtube.com/watch?v={}&autoplay=1", item.video_id);
    open(&watch_url)?;
    Ok(watch_url)
}

/// Найти первое видео, у которого title содержит все слова из `keywords`
/// (case-insensitive). Возвращает watch-URL.
///
/// Это спасает от «random selector» проблемы с ordinal'ами: ranking
/// YouTube'а меняется между запросами (A/B testing), и «второе» сегодня
/// может оказаться другим видео чем «второе» 5 минут назад. Если юзер
/// помнит часть названия — «включи асмр одноклассница» найдёт правильное.
pub fn play_youtube_by_title(keywords: &[String]) -> Result<String> {
    let results = fetch_youtube_results()?;
    if results.is_empty() {
        return Err(anyhow!("YouTube не вернул ни одного видео"));
    }
    let kws_lower: Vec<String> = keywords.iter().map(|s| s.to_lowercase()).collect();
    let item = results.iter().find(|r| {
        let title_lower = r.title.to_lowercase();
        kws_lower.iter().all(|kw| title_lower.contains(kw))
    });
    let item = item.ok_or_else(|| anyhow!(
        "не нашёл видео с «{}» в первых {} результатах",
        keywords.join(" "), results.len()
    ))?;
    let watch_url = format!("https://www.youtube.com/watch?v={}&autoplay=1", item.video_id);
    open(&watch_url)?;
    Ok(watch_url)
}

/// Общий хелпер: фетчит результаты последнего YouTube-запроса.
fn fetch_youtube_results() -> Result<Vec<YtResult>> {
    let query = last_youtube_query()
        .ok_or_else(|| anyhow!("нет предыдущего YouTube-запроса — скажи сначала «открой ютуб <запрос>»"))?;
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let search_url = format!("https://www.youtube.com/results?search_query={}&hl=en", encoded);

    let html = ureq::get(&search_url)
        .set("User-Agent", YT_USER_AGENT)
        .set("Accept-Language", "en-US,en;q=0.9,ru;q=0.8")
        .set("Cookie", "CONSENT=YES+cb.20210328-17-p0.en+FX+555")
        .timeout(Duration::from_secs(10))
        .call()
        .context("youtube search")?
        .into_string()
        .context("read body")?;

    Ok(extract_organic_results(&html))
}

/// Достать организические результаты поиска из ytInitialData.
/// Парсим JSON правильно (а не regex по всему HTML) — это игнорирует
/// Mix/Shorts/Music-карусели и другой не-search контент.
///
/// JSON path: `contents.twoColumnSearchResultsRenderer.primaryContents.sectionListRenderer.contents[*].itemSectionRenderer.contents[*].videoRenderer`
fn extract_organic_results(html: &str) -> Vec<YtResult> {
    // Найти `var ytInitialData = {...};` или `ytInitialData = {...};`
    let needle_a = "var ytInitialData = ";
    let needle_b = "ytInitialData = ";
    let start = html.find(needle_a).map(|p| p + needle_a.len())
        .or_else(|| html.find(needle_b).map(|p| p + needle_b.len()));
    let Some(start) = start else { return Vec::new() };

    // Найти конец JSON — балансируя фигурные скобки.
    let bytes = html.as_bytes();
    if bytes.get(start) != Some(&b'{') { return Vec::new(); }
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut escape = false;
    let mut end = start;
    for (i, &b) in bytes[start..].iter().enumerate() {
        if escape { escape = false; continue; }
        if in_str {
            if b == b'\\' { escape = true; }
            else if b == b'"' { in_str = false; }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 { end = start + i + 1; break; }
            }
            _ => {}
        }
    }
    if end == start { return Vec::new(); }

    let json_str = &html[start..end];
    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    // Walk: contents → twoColumnSearchResultsRenderer → primaryContents
    //       → sectionListRenderer → contents[*] → itemSectionRenderer
    //       → contents[*] → videoRenderer
    let sections = value.pointer(
        "/contents/twoColumnSearchResultsRenderer/primaryContents/sectionListRenderer/contents"
    );
    let Some(sections) = sections.and_then(|s| s.as_array()) else { return Vec::new() };

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for sec in sections {
        let Some(items) = sec.pointer("/itemSectionRenderer/contents").and_then(|c| c.as_array()) else { continue };
        for it in items {
            let Some(vr) = it.get("videoRenderer") else { continue };
            let Some(vid) = vr.get("videoId").and_then(|v| v.as_str()) else { continue };
            if !seen.insert(vid.to_string()) { continue }
            // Title: либо runs[0].text, либо simpleText.
            let title = vr.pointer("/title/runs/0/text")
                .or_else(|| vr.pointer("/title/simpleText"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            out.push(YtResult { video_id: vid.to_string(), title });
        }
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
    fn extract_organic_from_minimal_json() {
        // Минимальный валидный ytInitialData с одним organic + одним
        // мусором в неправильном path (должно быть проигнорировано).
        let html = r#"<script nonce>var ytInitialData = {
            "contents": {
              "twoColumnSearchResultsRenderer": {
                "primaryContents": {
                  "sectionListRenderer": {
                    "contents": [
                      {
                        "itemSectionRenderer": {
                          "contents": [
                            {"videoRenderer": {"videoId": "abc12345678", "title": {"runs": [{"text": "First Video"}]}}},
                            {"shelfRenderer": {"videoId": "shouldSkip00", "title": {"simpleText": "Mix carousel"}}},
                            {"videoRenderer": {"videoId": "DEFghijklmn", "title": {"simpleText": "Second Video"}}}
                          ]
                        }
                      }
                    ]
                  }
                }
              }
            }
          };</script>"#;
        let results = extract_organic_results(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].video_id, "abc12345678");
        assert_eq!(results[0].title, "First Video");
        assert_eq!(results[1].video_id, "DEFghijklmn");
        assert_eq!(results[1].title, "Second Video");
    }

    #[test]
    fn extract_returns_empty_on_missing_data() {
        assert!(extract_organic_results("<html>no data here</html>").is_empty());
    }

    #[test]
    fn title_parser_basic() {
        let kws = parse_play_by_title("включи асмр одноклассница").unwrap();
        assert_eq!(kws, vec!["асмр", "одноклассница"]);
    }

    #[test]
    fn title_parser_strips_filler() {
        let kws = parse_play_by_title("найди видео про котиков").unwrap();
        assert_eq!(kws, vec!["про", "котиков"]);
    }

    #[test]
    fn title_parser_skips_when_ordinal_present() {
        // Если есть «первое» + «видео» — это работа для parse_play_nth, не title
        assert_eq!(parse_play_by_title("включи первое видео асмр"), None);
    }

    #[test]
    fn title_parser_requires_trigger() {
        assert_eq!(parse_play_by_title("асмр одноклассница"), None);
    }
}
