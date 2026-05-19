//! Google Gemini 2.0 Flash API — HTTP клиент через ureq.
//! Быстрые ответы (~1-2 сек), отличный русский язык.

use anyhow::{anyhow, Result};
use serde_json::json;

const API_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";

const SYSTEM_PROMPT_RU: &str = "Ты голосовой ассистент. ВСЕГДА отвечай на РУССКОМ языке. Отвечай кратко, по существу, максимум 2 предложения. Говори естественно, как человек.";
const SYSTEM_PROMPT_EN: &str = "You are a voice assistant. ALWAYS answer in ENGLISH only — never French, German, or any other language. Answer briefly, max 2 sentences. Speak naturally, like a human.";

/// Спросить Gemini. Возвращает готовый текст ответа.
pub fn ask(api_key: &str, question: &str, language: &str) -> Result<String> {
    if api_key.trim().is_empty() {
        return Err(anyhow!("Gemini API key is empty"));
    }
    if question.trim().is_empty() {
        return Err(anyhow!("Empty question"));
    }

    let url = format!("{}?key={}", API_URL, api_key.trim());

    let body = json!({
        "systemInstruction": {
            "parts": [{ "text": if language == "ru" { SYSTEM_PROMPT_RU } else { SYSTEM_PROMPT_EN } }]
        },
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": question }]
            }
        ],
        "generationConfig": {
            "maxOutputTokens": 128,
            "temperature": 0.7
        }
    });

    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| anyhow!("Gemini HTTP error: {}", e))?;

    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| anyhow!("Gemini JSON parse: {}", e))?;

    // Извлекаем текст ответа
    let text = json
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| {
            // Логгируем полный ответ для диагностики
            anyhow!(
                "Gemini response has no text. Full response: {}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            )
        })?;

    Ok(text.trim().to_string())
}

/// Проверить валидность ключа простым ping-запросом (пустой prompt).
/// Не гарантирует что ключ работает для generation, но ловит очевидные ошибки.
pub fn check_key(api_key: &str) -> Result<bool> {
    if api_key.trim().is_empty() {
        return Ok(false);
    }
    let url = format!("{}?key={}", API_URL, api_key.trim());
    let body = json!({
        "contents": [{"role":"user","parts":[{"text":"Hi"}]}],
        "generationConfig": {"maxOutputTokens": 1}
    });

    match ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_json(&body)
    {
        Ok(r) => {
            let status = r.status();
            Ok(status == 200)
        }
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            tracing::warn!("[gemini] key check failed: HTTP {} — {}", code, body);
            Ok(false)
        }
        Err(e) => {
            tracing::warn!("[gemini] key check network error: {}", e);
            Ok(false)
        }
    }
}
