//! Голосовые команды для открытия браузера.
//! Никаких API-ключей — используем прямые URL поисковиков.

use anyhow::{Context, Result};

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
