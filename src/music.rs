//! Модуль для фонового воспроизведения музыки.
//!
//! Использует YouTube Internal API для поиска треков.
//! Состояние плеера (играет/остановлено/текущий трек) хранится в static,
//! чтобы UI и голосовые команды могли его читать из разных тредов.

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::sync::OnceLock;
use tracing::info;

/// Информация о текущем треке.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub thumbnail: String,
    pub duration: String,
}

/// Состояние музыкального плеера.
#[derive(Debug, Clone, Default)]
struct PlayerState {
    current: Option<TrackInfo>,
    is_playing: bool,
    position: u32,
}

static PLAYER_STATE: OnceLock<Mutex<PlayerState>> = OnceLock::new();

fn state_slot() -> &'static Mutex<PlayerState> {
    PLAYER_STATE.get_or_init(|| Mutex::new(PlayerState::default()))
}

/// Парсит длительность вида "3:45" или "1:23:45" в секунды.
fn parse_duration(dur: &str) -> u32 {
    let parts: Vec<u32> = dur.split(':')
        .filter_map(|s| s.parse().ok())
        .collect();
    match parts.len() {
        2 => parts[0] * 60 + parts[1],
        3 => parts[0] * 3600 + parts[1] * 60 + parts[2],
        _ => 0,
    }
}

/// Проверяет, не является ли результат мусором (подкаст, обзор, мемы, реакция и т.д.).
fn is_low_quality(title: &str) -> bool {
    let t = title.to_lowercase();
    let bad = [
        "подкаст", "разбор", "обзор", "interview", "podcast",
        "review", "реакция", "talk", "беседа", "анализ",
        "breaking down", "explained", "часовой", "часа", "часов",
        "топ ", "топ-", "top ", "top-", "микс", "mix",
        "playlist", "плейлист", "radio", "радио", "live",
        "концерт", "concert", "фестиваль", "festival",
        "мем", "meme", "funny", "прикол", "lol",
        "compilation", "collection", "best of", "лучшее", "best",
        " slowed", "reverb", "bass boosted", "sped up", "8d", "nightcore",
        "tiktok", "тикток", "тренд", "trend",
        "текст песни", "текст", "lyrics", "караоке", "karaoke",
        "премьера трека", "премьера", "фрагмент", "читает", "vs", "versus",
        "cover", "кавер", "remix", "ремикс", "bootleg", "edit",
        "история песни", "как создавался", "создатель", "создавался",
        "ответы", "интервью", "бит ", " бит", "beats", "инструментал",
        "instrumental", "акапелла", "acapella", "parody", "пародия",
        " Behind The Scenes", " behind the scenes", "making of",
    ];
    bad.iter().any(|w| t.contains(w))
}

/// Простейшая транслитерация русских букв в латиницу.
fn transliterate_ru_to_lat(s: &str) -> String {
    let mut out = String::new();
    for ch in s.to_lowercase().chars() {
        let replacement = match ch {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d",
            'е' => "e", 'ё' => "yo", 'ж' => "zh", 'з' => "z", 'и' => "i",
            'й' => "y", 'к' => "k", 'л' => "l", 'м' => "m", 'н' => "n",
            'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t",
            'у' => "u", 'ф' => "f", 'х' => "kh", 'ц' => "ts", 'ч' => "ch",
            'ш' => "sh", 'щ' => "shch", 'ъ' => "", 'ы' => "y", 'ь' => "",
            'э' => "e", 'ю' => "yu", 'я' => "ya",
            _ => { out.push(ch); "" }
        };
        out.push_str(replacement);
    }
    out
}

/// Нормализует строку для сравнения: lower-case, только буквы/цифры, разделённые пробелами.
/// Для слов на кириллице добавляет транслитерированный вариант и наоборот.
fn normalize_words(text: &str) -> Vec<String> {
    let raw: Vec<String> = text.to_lowercase()
        .split_whitespace()
        .map(|s| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|s| !s.is_empty() && s.len() >= 2)
        .collect();

    let mut expanded = Vec::new();
    for w in &raw {
        expanded.push(w.clone());
        let has_cyrillic = w.chars().any(|c| ('а'..='я').contains(&c) || c == 'ё');
        let has_latin = w.chars().any(|c| ('a'..='z').contains(&c));
        if has_cyrillic {
            expanded.push(transliterate_ru_to_lat(w));
        }
        if has_latin {
            // обратная транслитерация (грубая) — для полноты
            let ru = w.replace('a', "а").replace('b', "б").replace('c', "к").replace('d', "д")
                .replace('e', "е").replace('f', "ф").replace('g', "г").replace('h', "х")
                .replace('i', "и").replace('j', "дж").replace('k', "к").replace('l', "л")
                .replace('m', "м").replace('n', "н").replace('o', "о").replace('p', "п")
                .replace('q', "к").replace('r', "р").replace('s', "с").replace('t', "т")
                .replace('u', "у").replace('v', "в").replace('w', "в").replace('x', "кс")
                .replace('y', "й").replace('z', "з");
            if ru != *w { expanded.push(ru); }
        }
    }
    expanded
}

/// Комплексная оценка релевантности результата под запрос.
/// Чем больше — тем лучше.
fn composite_score(query: &str, r: &crate::youtube_internal::YtSearchResult) -> i32 {
    let q_words = normalize_words(query);
    let t_words = normalize_words(&r.title);
    let a_words = normalize_words(&r.channel);
    let q_joined = q_words.join(" ");
    let t_lower = r.title.to_lowercase();
    let a_lower = r.channel.to_lowercase();

    let mut score: i32 = 0;

    // 1. Exact phrase match в title — самый сильный сигнал
    if t_lower.contains(&q_joined) {
        score += 100;
    }

    // 2. Все слова query входят в title (в любом порядке)
    let title_matches = q_words.iter().filter(|w| t_lower.contains(*w)).count();
    score += title_matches as i32 * 15;

    // 2a. Бонус если ВСЕ слова query найдены в title
    let all_in_title = !q_words.is_empty() && q_words.iter().all(|w| t_lower.contains(w));
    if all_in_title {
        score += 80;
    }

    // 3. Слова title входят в query (штрафуем лишние/нерелевантные слова в title)
    let query_matches = t_words.iter().filter(|w| q_joined.contains(*w)).count();
    score += query_matches as i32 * 8;

    // 4. Совпадение артиста (умеренный бонус — не должен перевешивать title)
    let artist_matches = q_words.iter().filter(|w| a_lower.contains(*w)).count();
    score += artist_matches as i32 * 12;
    if a_words.iter().any(|w| q_joined.contains(w)) {
        score += 8;
    }

    // 5. Бонус за тип контента
    match r.music_type.as_str() {
        "Song" => score += 15,
        "Video" => score += 5,
        "Album" => score += 2,
        _ => {}
    }

    // 5a. Бонус/штраф за musicVideoType — официальные источники приоритизируем агрессивно
    match r.music_video_type.as_str() {
        "MUSIC_VIDEO_TYPE_ATV" => score += 40,
        "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC" => score += 60,
        "MUSIC_VIDEO_TYPE_UGC" => score -= 30,
        _ => {}
    }

    // 6. Если ни одно слово query не попало в title — это, скорее всего,
    //    другой трек того же исполнителя. Сильно штрафуем.
    if title_matches == 0 {
        score -= 60;
    }

    // 7. Штрафы за низкокачественный контент
    if is_low_quality(&r.title) {
        score -= 40;
    }

    // 8. Штраф за UGC / compilation / trend канал
    let ugc_hints = [
        "lyrics", "текст", "music", "topic", "audio", "beat", "tv", "channel",
        "best", "top", "mix", "тренд", "тренды", "хиты", "hits", "подборка",
        "compilation", "лучшее", "radio", "радио", "канал", "микс", "mix",
        " Muz", " Muzik", "Music ", " Music",
    ];
    if ugc_hints.iter().any(|h| a_lower.contains(h)) {
        score -= 30;
    }

    // 8a. Бонус за официальные Topic-каналы (они заливают оригинальные аудио)
    if a_lower.ends_with(" - topic") || a_lower.ends_with("topic") {
        score += 25;
    }

    // 9. Штраф за очень длинное название (>80 символов — скорее всего описание, не трек)
    if r.title.len() > 80 {
        score -= 10;
    }
    // Штраф за очень короткое название (<4 символа — скорее всего не то)
    if r.title.len() < 4 {
        score -= 10;
    }

    score
}

/// Deezer search result (lightweight).
#[derive(Debug, Clone)]
struct DeezerTrack {
    artist: String,
    title: String,
}

/// Search Deezer API for canonical artist + title. No API key needed.
fn search_deezer(query: &str) -> Option<DeezerTrack> {
    let url = format!(
        "https://api.deezer.com/search?q={}&limit=5",
        urlencoding::encode(query)
    );
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .ok()?;
    let body: serde_json::Value = resp.into_json().ok()?;
    let data = body.get("data")?.as_array()?;
    for item in data {
        let artist = item
            .pointer("/artist/name")
            .and_then(|v| v.as_str())?
            .to_string();
        let title = item
            .pointer("/title")
            .and_then(|v| v.as_str())?
            .to_string();
        if artist.len() >= 2 && title.len() >= 2 {
            return Some(DeezerTrack { artist, title });
        }
    }
    None
}

/// Искать трек по запросу.
/// Новая логика: Deezer → точный запрос на YouTube → fallback на прямой поиск.
pub fn search_track(query: &str) -> Result<TrackInfo> {
    // Stage 1: спрашиваем Deezer — там нет мемов и каверов, только официальные релизы.
    // Если Deezer дал точный artist + title, ищем на YouTube уже по чистому запросу.
    if let Some(dz) = search_deezer(query) {
        let clean_query = format!("{} - {} official audio", dz.artist, dz.title);
        info!("[music] Deezer resolved '{}' → '{}'", query, clean_query);
        if let Ok(mut web_results) = crate::youtube_internal::search_results(&clean_query, 10) {
            // Уточняем артиста через WEB_REMIX
            if let Ok(remix) = crate::youtube_internal::search_music_remix(&clean_query, 8) {
                let remix_map: std::collections::HashMap<String, (String, String)> = remix
                    .into_iter()
                    .map(|r| (r.video_id.clone(), (r.channel, r.music_type)))
                    .collect();
                for r in &mut web_results {
                    if let Some((artist, mtype)) = remix_map.get(&r.video_id) {
                        if !artist.trim().is_empty() && !artist.eq_ignore_ascii_case("video") && !artist.eq_ignore_ascii_case("song") {
                            r.channel = artist.clone();
                        }
                        if !mtype.trim().is_empty() {
                            r.music_type = mtype.clone();
                        }
                    }
                }
            }
            if !web_results.is_empty() {
                let item = pick_best(&clean_query, web_results)?;
                return Ok(TrackInfo {
                    video_id: item.video_id,
                    title: item.title,
                    artist: item.channel,
                    thumbnail: item.thumbnail,
                    duration: item.duration,
                });
            }
        }
    }

    // Stage 2: fallback — прямой поиск на YouTube (старая логика)
    info!("[music] Deezer miss or empty results, falling back to direct YouTube search for '{}'", query);
    let mut web_results = crate::youtube_internal::search_results(query, 15)
        .context("music search (WEB)")?;

    if let Ok(remix) = crate::youtube_internal::search_music_remix(query, 10) {
        let remix_map: std::collections::HashMap<String, (String, String)> = remix
            .into_iter()
            .map(|r| (r.video_id.clone(), (r.channel, r.music_type)))
            .collect();
        for r in &mut web_results {
            if let Some((artist, mtype)) = remix_map.get(&r.video_id) {
                if !artist.trim().is_empty() && !artist.eq_ignore_ascii_case("video") && !artist.eq_ignore_ascii_case("song") {
                    r.channel = artist.clone();
                }
                if !mtype.trim().is_empty() {
                    r.music_type = mtype.clone();
                }
            }
        }
    }

    if web_results.is_empty() {
        anyhow::bail!("не найдено ни одного трека");
    }

    let item = pick_best(query, web_results)?;
    Ok(TrackInfo {
        video_id: item.video_id,
        title: item.title,
        artist: item.channel,
        thumbnail: item.thumbnail,
        duration: item.duration,
    })
}

/// Выбрать лучший результат из списка с учётом фильтров и релевантности.
fn pick_best(query: &str, results: Vec<crate::youtube_internal::YtSearchResult>) -> Result<crate::youtube_internal::YtSearchResult> {
    // Фильтруем явный мусор
    let mut filtered: Vec<_> = results.iter()
        .filter(|r| {
            let not_junk = !is_low_quality(&r.title);
            // Для WEB-поиска duration есть и работает; для WEB_REMIX часто пустая — не фильтруем
            let secs = parse_duration(&r.duration);
            let duration_ok = r.duration.is_empty() || secs <= 900;
            not_junk && duration_ok
        })
        .cloned()
        .collect();

    if filtered.is_empty() {
        // Если после фильтрации ничего не осталось — берём лучшее из исходного списка
        // (чтобы не возвращать "не найдено" из-за слишком строгих фильтров)
        filtered = results;
    }

    // Сортируем по комплексной оценке (убывание)
    filtered.sort_by(|a, b| {
        let score_b = composite_score(query, b);
        let score_a = composite_score(query, a);
        score_b.cmp(&score_a)
    });

    filtered.into_iter().next().ok_or_else(|| anyhow::anyhow!("не удалось выбрать лучший результат (список пуст)"))
}

/// Пытается вытащить имя артиста из названия трека вида "Artist - Title".
pub fn extract_artist_from_title(title: &str) -> Option<String> {
    let t = title.trim();
    // Ищем разделитель: " - ", " — ", " – ", " | ", " / "
    for sep in [" - ", " — ", " – ", " | ", " / "] {
        if let Some(pos) = t.find(sep) {
            let artist = t[..pos].trim();
            if !artist.is_empty() && artist.len() >= 2 {
                return Some(artist.to_string());
            }
        }
    }
    None
}

/// Определяет, похоже ли имя артиста на UGC/фан-канал (не настоящий артист).
pub fn is_ugc_artist(artist: &str) -> bool {
    let a = artist.to_lowercase();
    let hints = [
        "lyrics", "текст", "music", "topic", "audio", "beat", "tv", "channel",
        "best", "top", "mix", "тренд", "тренды", "хиты", "hits", "подборка",
        "compilation", "лучшее", "radio", "радио", "канал", "микс", "mix",
        "clips", "клип", "official", "fan", "фан", "edit", "slowed", "reverb",
        "version", "версия", "cover", "кавер",
    ];
    hints.iter().any(|h| a.contains(h))
}

/// Найти случайную песню того же исполнителя.
pub fn play_random_by_artist(artist: &str, exclude_video_id: Option<&str>) -> Result<TrackInfo> {
    // Используем WEB-поиск — у него реальная длительность
    let mut results = crate::youtube_internal::search_results(artist, 15)
        .context("artist search (WEB)")?;

    // Пытаемся уточнить артиста/тип через WEB_REMIX
    if let Ok(remix) = crate::youtube_internal::search_music_remix(artist, 10) {
        let remix_map: std::collections::HashMap<String, (String, String)> = remix
            .into_iter()
            .map(|r| (r.video_id.clone(), (r.channel, r.music_type)))
            .collect();
        for r in &mut results {
            if let Some((art, mtype)) = remix_map.get(&r.video_id) {
                if !art.trim().is_empty() && !art.eq_ignore_ascii_case("video") && !art.eq_ignore_ascii_case("song") {
                    r.channel = art.clone();
                }
                if !mtype.trim().is_empty() {
                    r.music_type = mtype.clone();
                }
            }
        }
    }

    let mut candidates: Vec<_> = results.into_iter()
        .filter(|r| {
            if let Some(exclude) = exclude_video_id {
                if r.video_id == exclude { return false; }
            }
            let secs = parse_duration(&r.duration);
            let duration_ok = r.duration.is_empty() || secs <= 900;
            let not_junk = !is_low_quality(&r.title);
            duration_ok && not_junk
        })
        .collect();

    if candidates.is_empty() {
        anyhow::bail!("не найдено других песен исполнителя '{}'", artist);
    }

    // Сортируем по релевантности к имени артиста, чтобы среди случайных
    // с большей вероятностью попадались официальные треки
    candidates.sort_by(|a, b| {
        let score_b = composite_score(artist, b);
        let score_a = composite_score(artist, a);
        score_b.cmp(&score_a)
    });

    // Берём топ-5 и перемешиваем — так сохраняем случайность,
    // но не выдаём совсем нерелевантные результаты
    let top_n = candidates.len().min(5);
    let mut top = candidates.into_iter().take(top_n).collect::<Vec<_>>();

    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();
    top.shuffle(&mut rng);

    let item = top.into_iter().next().ok_or_else(|| anyhow::anyhow!("не удалось выбрать следующий трек (список пуст)"))?;
    let track = TrackInfo {
        video_id: item.video_id,
        title: item.title,
        artist: item.channel,
        thumbnail: item.thumbnail,
        duration: item.duration,
    };
    play(track.clone());
    Ok(track)
}

/// Начать воспроизведение трека (сохранить в состояние).
pub fn play(track: TrackInfo) {
    let mut st = state_slot().lock();
    st.current = Some(track);
    st.is_playing = true;
    st.position = 0;
}

/// Остановить воспроизведение и сбросить состояние.
pub fn stop() {
    let mut st = state_slot().lock();
    st.is_playing = false;
    st.current = None;
    st.position = 0;
}

/// Поставить на паузу (состояние paused).
pub fn pause() {
    state_slot().lock().is_playing = false;
}

/// Продолжить воспроизведение.
pub fn resume() {
    let mut st = state_slot().lock();
    if st.current.is_some() {
        st.is_playing = true;
    }
}

/// Воспроизводится ли сейчас музыка?
pub fn is_playing() -> bool {
    state_slot().lock().is_playing
}

/// Есть ли активный трек (включая paused)?
pub fn has_track() -> bool {
    state_slot().lock().current.is_some()
}

/// Получить текущий трек.
pub fn current_track() -> Option<TrackInfo> {
    state_slot().lock().current.clone()
}

/// Перемотать на указанную позицию (в секундах).
pub fn seek(seconds: u32) {
    state_slot().lock().position = seconds;
}

/// Получить текущую позицию (в секундах).
pub fn position() -> u32 {
    state_slot().lock().position
}

/// Удобный wrapper: искать по запросу и сразу начать воспроизведение.
/// Возвращает TrackInfo для отправки в UI.
pub fn search_and_play(query: &str) -> Result<TrackInfo> {
    let track = search_track(query)?;
    play(track.clone());
    Ok(track)
}

/// URL для открытия трека в браузере (fallback когда нет UI WebView).
pub fn watch_url(video_id: &str) -> String {
    format!("https://music.youtube.com/watch?v={}", video_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_track() -> TrackInfo {
        TrackInfo {
            video_id: "abc123".into(),
            title: "Test Song".into(),
            artist: "Test Artist".into(),
            thumbnail: "https://i.ytimg.com/vi/abc123/hqdefault.jpg".into(),
            duration: "3:45".into(),
        }
    }

    #[test]
    fn state_play_stop() {
        stop(); // reset
        assert!(!is_playing());
        assert!(!has_track());
        play(dummy_track());
        assert!(is_playing());
        assert!(has_track());
        stop();
        assert!(!is_playing());
        assert!(!has_track());
    }

    #[test]
    fn state_pause_resume() {
        stop();
        play(dummy_track());
        assert!(is_playing());
        pause();
        assert!(!is_playing());
        assert!(has_track());
        resume();
        assert!(is_playing());
        stop();
        assert!(!is_playing());
    }

    #[test]
    fn state_seek_and_position() {
        stop();
        play(dummy_track());
        assert_eq!(position(), 0);
        seek(42);
        assert_eq!(position(), 42);
        stop();
    }

    #[test]
    fn state_current_track() {
        stop();
        let track = dummy_track();
        play(track.clone());
        let cur = current_track().unwrap();
        assert_eq!(cur.video_id, track.video_id);
        assert_eq!(cur.title, track.title);
        stop();
        assert!(current_track().is_none());
    }


}
