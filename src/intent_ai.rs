//! AI-fallback для понимания нечётких команд.
//!
//! Когда rule-based парсеры (browser, browser_action, contacts) вернули
//! None — отправляем текст в Gemini с промптом-классификатором и получаем
//! структурированное действие. Юзер может сказать любую вариацию
//! («погромче сделай», «врубай ютубчик котики», «напиши Тиме чё там»)
//! и Voicy поймёт.
//!
//! Используется ТОЛЬКО Gemini API (не локальная LLM) — local Qwen 0.5B
//! слишком медленный для интерактивных команд (30+ сек). Без ключа
//! Gemini fallback отключён, юзер получит обычную parse-error.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const GEMINI_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";

/// Результат классификации — что Voicy должен сделать.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Intent {
    /// Видео-плеер: громче. n = сколько раз нажать ↑ (1 = ~5%).
    VolumeUp { n: u8 },
    /// Видео-плеер: тише.
    VolumeDown { n: u8 },
    /// F — полный экран.
    Fullscreen,
    /// Space — пауза/играй.
    PlayPause,
    /// → — перемотать вперёд. n = сколько раз нажать (1 = ~5s).
    SeekForward { n: u8 },
    /// ← — перемотать назад.
    SeekBackward { n: u8 },
    /// M — mute/unmute.
    Mute,
    /// Открыть поисковик с запросом. provider = "youtube" | "google" | "tiktok" | "twitch".
    OpenUrl { provider: String, query: String },
    /// Включить N-е видео в последнем YouTube-поиске (0-based).
    PlayNth { index: usize },
    /// Включить видео содержащее эти слова в title.
    PlayByTitle { keywords: Vec<String> },
    /// Отправить сообщение в Telegram. contact — имя или alias.
    SendTelegram { contact: String, message: String },
    /// Задать вопрос AI-ассистенту.
    AskAi { question: String },
    /// Команда не понята.
    Unknown,
}

/// Контекст для AI: имена доступных контактов + был ли недавний YouTube запрос.
/// Помогает AI правильно disambiguate (например выбрать имя контакта).
pub struct AiContext {
    pub contact_names: Vec<String>,
    pub has_recent_youtube: bool,
}

const SYSTEM_PROMPT: &str = r#"You are an intent classifier for a voice assistant "Voicy". Convert user voice command (in Russian or English, possibly with ASR errors) into a JSON action.

You MUST respond with ONLY a single JSON object, no markdown, no explanation. Use this exact schema:

ACTIONS:
- {"action":"volume_up","n":N}      — louder, N = number of ↑ presses (1 = ~5%)
- {"action":"volume_down","n":N}    — quieter
- {"action":"fullscreen"}            — toggle F11 / fullscreen
- {"action":"play_pause"}            — Space / play / pause
- {"action":"seek_forward","n":N}    — skip ahead N×5 seconds
- {"action":"seek_backward","n":N}   — skip back N×5 seconds
- {"action":"mute"}                  — mute toggle
- {"action":"open_url","provider":"youtube|google|tiktok|twitch","query":"..."}
- {"action":"play_nth","index":N}    — open Nth video in last YouTube search (0-based!)
- {"action":"play_by_title","keywords":["w1","w2"]} — find video by title words
- {"action":"send_telegram","contact":"name","message":"..."}
- {"action":"ask_ai","question":"..."}  — user wants AI to answer
- {"action":"unknown"}                — if intent is unclear

RULES:
1. For Russian: «погромче»/«сделай громче»/«громче» → volume_up. «потише» → volume_down. «пауза»/«стоп»/«играй» → play_pause.
2. «открой ютуб X» / «врубай ютубчик X» → open_url with provider="youtube", query=X.
3. «включи второе видео» → play_nth, index=1 (zero-based!). «третье» → 2. «первое» → 0.
4. «включи асмр одноклассница» (without ordinal) → play_by_title with keywords.
5. «напиши Тиме привет» / «отправь Маше пока» → send_telegram. CONTACT must match one of the provided names if possible.
6. «дай ответ X» / «спроси X» / «ответь X» → ask_ai.
7. Default volume_up/down n=2 (~10%) if unspecified. Default seek n=2 if unspecified.

EXAMPLES:
User: "погромче немного"
{"action":"volume_up","n":1}

User: "сделай погромче процентов на 20"
{"action":"volume_up","n":4}

User: "врубай ютубчик котики"
{"action":"open_url","provider":"youtube","query":"котики"}

User: "ну включи там второе"
{"action":"play_nth","index":1}

User: "напиши Тиме чё там как дела"
{"action":"send_telegram","contact":"Тима","message":"чё там как дела"}

User: "ничего непонятного"
{"action":"unknown"}
"#;

/// Классификация intent через Gemini. Возвращает Intent::Unknown если AI
/// не справился. Возвращает Err только при network/parse-ошибках.
pub fn classify(api_key: &str, text: &str, ctx: &AiContext) -> Result<Intent> {
    if api_key.trim().is_empty() {
        return Err(anyhow!("Gemini API key empty — intent fallback disabled"));
    }
    if text.trim().is_empty() {
        return Ok(Intent::Unknown);
    }

    // Контекстный hint для AI — какие контакты есть, есть ли свежий YT-запрос.
    // Помогает с disambiguation типа «напиши Тиме» → найти точное имя в списке.
    let ctx_block = format!(
        "AVAILABLE CONTACTS: {}\nRECENT YOUTUBE SEARCH: {}\n",
        if ctx.contact_names.is_empty() {
            "(none)".to_string()
        } else {
            ctx.contact_names.join(", ")
        },
        if ctx.has_recent_youtube { "yes" } else { "no — play_nth/play_by_title не сработают без open_url" }
    );

    let user_msg = format!("{}\nUSER COMMAND: \"{}\"\n\nRespond with ONLY JSON.", ctx_block, text);

    let url = format!("{}?key={}", GEMINI_URL, api_key.trim());
    let body = json!({
        "systemInstruction": { "parts": [{ "text": SYSTEM_PROMPT }] },
        "contents": [{ "role": "user", "parts": [{ "text": user_msg }] }],
        "generationConfig": {
            "maxOutputTokens": 200,
            "temperature": 0.1,         // максимально детерминированно
            "responseMimeType": "application/json"
        }
    });

    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(8))
        .send_json(&body)
        .map_err(|e| anyhow!("Gemini HTTP: {}", e))?;
    let resp_json: serde_json::Value = resp.into_json().map_err(|e| anyhow!("Gemini JSON: {}", e))?;

    let text_out = resp_json
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Gemini no text in response: {}", resp_json))?;

    // Иногда Gemini оборачивает в ```json ... ```. Срежем.
    let cleaned = strip_markdown_fences(text_out).trim().to_string();

    let intent: Intent = serde_json::from_str(&cleaned)
        .map_err(|e| anyhow!("intent JSON parse: {} — raw: {}", e, cleaned))?;

    Ok(intent)
}

fn strip_markdown_fences(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_volume_up() {
        let j = r#"{"action":"volume_up","n":2}"#;
        let i: Intent = serde_json::from_str(j).unwrap();
        assert_eq!(i, Intent::VolumeUp { n: 2 });
    }

    #[test]
    fn parse_send_telegram() {
        let j = r#"{"action":"send_telegram","contact":"Тима","message":"привет"}"#;
        let i: Intent = serde_json::from_str(j).unwrap();
        assert_eq!(i, Intent::SendTelegram {
            contact: "Тима".into(),
            message: "привет".into(),
        });
    }

    #[test]
    fn parse_unknown() {
        let j = r#"{"action":"unknown"}"#;
        let i: Intent = serde_json::from_str(j).unwrap();
        assert_eq!(i, Intent::Unknown);
    }

    #[test]
    fn strip_fences_basic() {
        assert_eq!(strip_markdown_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_markdown_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_markdown_fences("{\"a\":1}"), "{\"a\":1}");
    }
}
