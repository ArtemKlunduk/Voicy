//! Голосовые команды для открытия браузера.
//! Никаких API-ключей — используем прямые URL поисковиков.

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

/// Кэш последнего YouTube-поиска: (query, результаты).
/// Результаты фетчатся один раз при первом запросе и переиспользуются
/// для команд «включи N-е видео» — это устраняет A/B-racing когда
/// YouTube возвращает разный ranking при повторных запросах.
static LAST_YOUTUBE_RESULTS: OnceLock<Mutex<Option<(String, Vec<YtResult>)>>> = OnceLock::new();

fn yt_results_slot() -> &'static Mutex<Option<(String, Vec<YtResult>)>> {
    LAST_YOUTUBE_RESULTS.get_or_init(|| Mutex::new(None))
}

pub fn last_youtube_query() -> Option<String> {
    yt_results_slot().lock().as_ref().map(|(q, _)| q.clone())
}

fn set_last_youtube_results(query: &str, results: Vec<YtResult>) {
    *yt_results_slot().lock() = Some((query.to_string(), results));
}

/// Текущее открытое видео на YouTube (videoId). Нужно чтобы команды
/// «открой видео <название>» могли искать в related videos справа.
static LAST_WATCH_VIDEO: OnceLock<Mutex<Option<String>>> = OnceLock::new();


fn watch_slot() -> &'static Mutex<Option<String>> {
    LAST_WATCH_VIDEO.get_or_init(|| Mutex::new(None))
}

pub fn last_watch_video() -> Option<String> {
    watch_slot().lock().clone()
}

fn set_last_watch_video(id: &str) {
    *watch_slot().lock() = Some(id.to_string());
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
            // Кэшируем YouTube-результаты через youtube_internal API (стабильный JSON),
            // чтобы ordinal-команды не страдали от A/B-ranking и разницы HTML.
            if provider == "YouTube" && !query.is_empty() {
                match crate::youtube_internal::search_videos(&query, 10) {
                    Ok(items) => {
                        let results: Vec<YtResult> = items.into_iter()
                            .map(|i| YtResult { video_id: i.video_id, title: i.title })
                            .collect();
                        tracing::info!("[browser::parse] cached {} results for '{}' via youtube_internal", results.len(), query);
                        set_last_youtube_results(&query, results);
                    }
                    Err(e) => {
                        tracing::warn!("[browser::parse] youtube_internal search failed for '{}': {}, falling back to HTML scraping", query, e);
                        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
                        let search_url = format!("https://www.youtube.com/results?search_query={}&hl=en", encoded);
                        if let Some(html) = ureq::get(&search_url)
                            .set("User-Agent", YT_USER_AGENT)
                            .set("Accept-Language", "en-US,en;q=0.9,ru;q=0.8")
                            .set("Cookie", "CONSENT=YES+cb.20210328-17-p0.en+FX+555")
                            .timeout(Duration::from_secs(10))
                            .call()
                            .ok()
                            .and_then(|r| r.into_string().ok())
                        {
                            let results = extract_organic_results(&html);
                            tracing::info!("[browser::parse] cached {} results for '{}' via HTML scraping", results.len(), query);
                            set_last_youtube_results(&query, results);
                        }
                    }
                }
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

    tracing::debug!("[parse_play_nth] input='{}' normalized='{}'", text, t);

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
                tracing::info!("[parse_play_nth] matched ordinal '{}' → index {}", word, idx);
                return Some(idx);
            }
        }
    }
    tracing::debug!("[parse_play_nth] no ordinal match");
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

    tracing::info!("[parse_play_by_title] input='{}' normalized='{}'", text, t);

    // Если уже распарсилось как ordinal — не лезем
    if parse_play_nth(text).is_some() {
        tracing::info!("[parse_play_by_title] skipped — already matched as ordinal");
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
    let Some(tp) = trigger_pos else {
        tracing::info!("[parse_play_by_title] no trigger found");
        return None;
    };
    let after = t[tp + trigger_len..].trim();

    // Убираем слова «видео»/«ролик» если есть — они не часть title
    let kws: Vec<String> = after
        .split_whitespace()
        .filter(|w| !matches!(*w, "видео" | "ролик" | "это" | "то" | "на" | "ютубе"))
        .map(|s| s.to_string())
        .collect();

    if kws.is_empty() || kws.iter().all(|k| k.chars().count() < 2) {
        tracing::info!("[parse_play_by_title] keywords too short or empty");
        return None;
    }
    tracing::info!("[parse_play_by_title] matched keywords: {:?}", kws);
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

/// Открыть N-е (0-based) видео из кэшированных результатов поиска.
pub fn play_nth_youtube_result(n: usize) -> Result<String> {
    tracing::info!("[play_nth_youtube_result] requested index {}", n);
    let results = fetch_youtube_results()?;
    tracing::info!("[play_nth_youtube_result] got {} results", results.len());
    for (i, r) in results.iter().take(5).enumerate() {
        tracing::info!("[play_nth_youtube_result] result #{}: id={} title='{}'", i + 1, r.video_id, r.title);
    }
    if results.is_empty() {
        return Err(anyhow!("YouTube не вернул ни одного видео (антибот?)"));
    }
    let item = results.get(n).ok_or_else(|| {
        anyhow!("результат №{} не найден (всего {})", n + 1, results.len())
    })?;
    let watch_url = format!("https://www.youtube.com/watch?v={}&autoplay=1", item.video_id);
    tracing::info!("[play_nth_youtube_result] opening: {}", watch_url);
    set_last_watch_video(&item.video_id);
    open(&watch_url)?;
    Ok(watch_url)
}

/// Найти видео по keywords. Сначала ищем в related videos текущего watch
/// (если есть), потом — в кэшированных search-результатах.
pub fn play_youtube_by_title(keywords: &[String]) -> Result<String> {
    tracing::info!("[play_youtube_by_title] keywords: {:?}", keywords);
    // Если есть текущее видео — пробуем related videos первым делом.
    if let Some(vid) = last_watch_video() {
        tracing::info!("[play_youtube_by_title] trying related videos for current video {}", vid);
        if let Ok(url) = play_related_by_title(keywords) {
            return Ok(url);
        }
    }
    // Fallback на кэш поиска.
    let results = fetch_youtube_results()?;
    tracing::info!("[play_youtube_by_title] search cache has {} results", results.len());
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
    set_last_watch_video(&item.video_id);
    open(&watch_url)?;
    Ok(watch_url)
}

/// Найти видео по keywords в related videos текущего watch-страницы.
/// Сначала скрейпит страницу watch, достаёт compactVideoRenderer'ы справа,
/// ищет fuzzy-match по title. Если нашёл — открывает.
pub fn play_related_by_title(keywords: &[String]) -> Result<String> {
    let video_id = last_watch_video()
        .ok_or_else(|| anyhow!("нет текущего видео — сначала открой видео на YouTube"))?;
    tracing::info!("[play_related_by_title] current video={} keywords={:?}", video_id, keywords);
    let results = fetch_related_videos(&video_id)?;
    tracing::info!("[play_related_by_title] got {} related videos", results.len());
    if results.is_empty() {
        return Err(anyhow!("не удалось получить related videos"));
    }
    let kws_lower: Vec<String> = keywords.iter().map(|s| s.to_lowercase()).collect();
    let item = results.iter().find(|r| {
        let title_lower = r.title.to_lowercase();
        kws_lower.iter().all(|kw| title_lower.contains(kw))
    });
    let item = item.ok_or_else(|| anyhow!(
        "не нашёл видео с «{}» в related videos",
        keywords.join(" ")
    ))?;
    let watch_url = format!("https://www.youtube.com/watch?v={}&autoplay=1", item.video_id);
    set_last_watch_video(&item.video_id);
    open(&watch_url)?;
    Ok(watch_url)
}

// ────────────────────────────────────────────────────────────────────
// «Перейди на канал» — скрейп channelId со страницы watch и открытие
// ────────────────────────────────────────────────────────────────────

/// Парсит команды вида «перейди на канал», «открой канал автора»,
/// «канал автора», «перейди к каналу».
pub fn parse_go_to_channel(text: &str) -> bool {
    let t: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let matched = t.contains("перейди на канал")
        || t.contains("открой канал")
        || t.contains("канал автора")
        || t.contains("перейди к каналу")
        || t.contains("go to channel")
        || t.contains("открой канал автора");
    if matched {
        tracing::info!("[parse_go_to_channel] matched for '{}'", text);
    }
    matched
}

/// Перейти на канал автора текущего видео.
pub fn go_to_channel() -> Result<String> {
    let video_id = last_watch_video()
        .ok_or_else(|| anyhow!("нет текущего видео — сначала открой видео на YouTube"))?;
    tracing::info!("[go_to_channel] current video={}", video_id);
    let channel_id = fetch_channel_id(&video_id)?;
    tracing::info!("[go_to_channel] channel_id={}", channel_id);
    let url = format!("https://www.youtube.com/channel/{}", channel_id);
    open(&url)?;
    Ok(url)
}

/// Получает channelId видео. Сначала пробует YouTube Data API v3,
/// fallback на скрейпинг ytInitialData.
fn fetch_channel_id(video_id: &str) -> Result<String> {
    match crate::youtube_internal::get_video_channel_id(video_id) {
        Ok(channel_id) => return Ok(channel_id),
        Err(e) => {
            tracing::warn!("[youtube_internal] channel fallback to scraping: {}", e);
        }
    }

    // Fallback: HTML-скрейпинг
    let url = format!("https://www.youtube.com/watch?v={}", video_id);
    let html = ureq::get(&url)
        .set("User-Agent", YT_USER_AGENT)
        .set("Accept-Language", "en-US,en;q=0.9,ru;q=0.8")
        .set("Cookie", "CONSENT=YES+cb.20210328-17-p0.en+FX+555")
        .timeout(Duration::from_secs(10))
        .call()
        .context("youtube watch fetch")?
        .into_string()
        .context("read body")?;

    let needle_a = "var ytInitialData = ";
    let needle_b = "ytInitialData = ";
    let start = html.find(needle_a).map(|p| p + needle_a.len())
        .or_else(|| html.find(needle_b).map(|p| p + needle_b.len()));
    let Some(start) = start else {
        return Err(anyhow!("ytInitialData не найден"));
    };

    let bytes = html.as_bytes();
    if bytes.get(start) != Some(&b'{') {
        return Err(anyhow!("ytInitialData не начинается с {{"));
    }
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
    if end == start {
        return Err(anyhow!("не удалось найти конец ytInitialData"));
    }

    let json_str = &html[start..end];
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("ytInitialData parse: {}", e))?;

    // Walk through contents to find videoSecondaryInfoRenderer
    let contents = value
        .pointer("/contents/twoColumnWatchNextResults/results/results/contents")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("no contents in ytInitialData"))?;

    for item in contents {
        if let Some(vsir) = item.get("videoSecondaryInfoRenderer") {
            if let Some(owner) = vsir.pointer("/owner/videoOwnerRenderer/navigationEndpoint/browseEndpoint/browseId")
                .and_then(|v| v.as_str())
            {
                return Ok(owner.to_string());
            }
        }
    }

    Err(anyhow!("channelId не найден в ytInitialData"))
}

/// Общий хелпер: возвращает кэшированные результаты или фетчит заново.
/// Если задан YouTube Data API key — использует API (стабильно, не ломается
/// при изменении HTML), иначе fallback на HTML-скрейпинг.
fn fetch_youtube_results() -> Result<Vec<YtResult>> {
    let query = last_youtube_query()
        .ok_or_else(|| anyhow!("нет предыдущего YouTube-запроса — скажи сначала «открой ютуб <запрос>»"))?;
    tracing::info!("[fetch_youtube_results] query='{}'", query);

    // Проверяем кэш
    {
        let lock = yt_results_slot().lock();
        if let Some((cached_q, cached_res)) = lock.as_ref() {
            if cached_q == &query {
                tracing::info!("[fetch_youtube_results] using cache ({} results)", cached_res.len());
                return Ok(cached_res.clone());
            }
            tracing::info!("[fetch_youtube_results] cache mismatch: cached='{}' vs current='{}'", cached_q, query);
        } else {
            tracing::info!("[fetch_youtube_results] no cache");
        }
    }

    // Пробуем YouTube Internal API (youtubei/v1)
    match crate::youtube_internal::search_videos(&query, 10) {
        Ok(items) => {
            let results: Vec<YtResult> = items.into_iter()
                .map(|i| YtResult { video_id: i.video_id, title: i.title })
                .collect();
            set_last_youtube_results(&query, results.clone());
            return Ok(results);
        }
        Err(e) => {
            tracing::warn!("[youtube_internal] search fallback to scraping: {}", e);
        }
    }

    // Fallback: HTML-скрейпинг
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
    let results = extract_organic_results(&html);
    set_last_youtube_results(&query, results.clone());
    Ok(results)
}

/// Фетчит related videos. Сначала пробует YouTube Data API v3,
/// fallback на HTML-скрейпинг.
fn fetch_related_videos(video_id: &str) -> Result<Vec<YtResult>> {
    match crate::youtube_internal::get_related_videos(video_id, 10) {
        Ok(items) => {
            return Ok(items.into_iter()
                .map(|i| YtResult { video_id: i.video_id, title: i.title })
                .collect());
        }
        Err(e) => {
            tracing::warn!("[youtube_internal] related fallback to scraping: {}", e);
        }
    }

    // Fallback: HTML-скрейпинг
    let url = format!("https://www.youtube.com/watch?v={}", video_id);
    let html = ureq::get(&url)
        .set("User-Agent", YT_USER_AGENT)
        .set("Accept-Language", "en-US,en;q=0.9,ru;q=0.8")
        .set("Cookie", "CONSENT=YES+cb.20210328-17-p0.en+FX+555")
        .timeout(Duration::from_secs(10))
        .call()
        .context("youtube watch fetch")?
        .into_string()
        .context("read body")?;
    Ok(extract_related_results(&html))
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

/// Достать related videos из ytInitialData на странице watch.
/// Поддерживает оба формата: старый `compactVideoRenderer` и новый
/// `lockupViewModel` (YouTube A/B-тестирует/мигрирует на него).
fn extract_related_results(html: &str) -> Vec<YtResult> {
    let needle_a = "var ytInitialData = ";
    let needle_b = "ytInitialData = ";
    let start = html.find(needle_a).map(|p| p + needle_a.len())
        .or_else(|| html.find(needle_b).map(|p| p + needle_b.len()));
    let Some(start) = start else { return Vec::new() };

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

    let results = value.pointer(
        "/contents/twoColumnWatchNextResults/secondaryResults/secondaryResults/results"
    );
    let Some(results) = results.and_then(|r| r.as_array()) else { return Vec::new() };

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Новый формат: results[0].itemSectionRenderer.contents[*].lockupViewModel
    // Старый формат: results[*].compactVideoRenderer
    for it in results {
        // Пробуем новый формат сначала
        if let Some(contents) = it
            .pointer("/itemSectionRenderer/contents")
            .and_then(|c| c.as_array())
        {
            for child in contents {
                if let Some(vm) = child.get("lockupViewModel") {
                    let ctype = vm.get("contentType").and_then(|c| c.as_str());
                    if ctype != Some("LOCKUP_CONTENT_TYPE_VIDEO") {
                        continue;
                    }
                    let Some(vid) = vm.get("contentId").and_then(|v| v.as_str()) else { continue };
                    if !seen.insert(vid.to_string()) { continue }
                    let title = vm
                        .pointer("/metadata/lockupMetadataViewModel/title/content")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    out.push(YtResult { video_id: vid.to_string(), title });
                }
            }
        }
        // Fallback на старый формат
        if let Some(vr) = it.get("compactVideoRenderer") {
            let Some(vid) = vr.get("videoId").and_then(|v| v.as_str()) else { continue };
            if !seen.insert(vid.to_string()) { continue }
            let title = vr.pointer("/title/simpleText")
                .or_else(|| vr.pointer("/title/runs/0/text"))
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
