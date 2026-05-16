//! Конфиг приложения: hotkey, активная whisper-модель, креды Telegram,
//! путь к session-файлу, контакты. Хранится в `voicy.toml` рядом с .exe.
//!
//! # Telegram API credentials
//!
//! `api_id` / `api_hash` — это идентификатор клиентского приложения на
//! my.telegram.org. Раньше они были захардкожены в исходниках, но это
//! security issue — публичный репо палит креды, любой может писать клиент
//! от имени «Voicy» и спровоцировать бан приложения.
//!
//! Теперь источник: ENV vars > voicy.toml. Никаких дефолтов в коде.
//! Если ни там, ни там нет — Voicy fатально валится с понятной ошибкой.
//!
//! ```text
//! Способ 1 (рекомендуется для разработки):
//!     setx VOICY_TG_API_ID 12345678
//!     setx VOICY_TG_API_HASH abcdef0123456789...
//!
//! Способ 2 (для конечных пользователей):
//!     voicy.toml рядом с exe, секция [telegram]:
//!         api_id = 12345678
//!         api_hash = "abcdef0123..."
//! ```

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ENV_API_ID: &str = "VOICY_TG_API_ID";
const ENV_API_HASH: &str = "VOICY_TG_API_HASH";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// faster-whisper / whisper.cpp model name: tiny/base/small/medium/large-v3
    pub model: String,
    pub hotkey: Hotkey,
    pub telegram: Telegram,
    pub recognition_language: String,
    /// "light" | "dark" — UI theme, восстанавливается при перезапуске.
    #[serde(default = "default_theme")]
    pub ui_theme: String,
    /// Загружать ASR-модель в RAM сразу при старте listener'а.
    #[serde(default = "default_preload")]
    pub preload_model: bool,
    /// Включить голосового ИИ-ассистента (команда "дай ответ").
    #[serde(default = "default_ai_enabled")]
    pub ai_assistant_enabled: bool,
    /// Загружать ИИ-модель в RAM при старте listener'а.
    #[serde(default = "default_ai_preload")]
    pub ai_preload: bool,
    /// Запускать приложение при старте Windows.
    #[serde(default = "default_startup_launch")]
    pub startup_launch: bool,
    /// Язык интерфейса: "ru" | "en".
    #[serde(default = "default_language")]
    pub language: String,
    /// API ключ для Google Gemini (голосовой ассистент).
    #[serde(default)]
    pub gemini_api_key: String,
    /// Язык голосового ассистента: "ru" | "en".
    #[serde(default = "default_ai_language")]
    pub ai_language: String,
    /// Локальная ИИ-модель: "qwen-0.5b" | "llama-3.2-1b" | "gemma-2-2b".
    /// Дефолт — qwen-0.5b (минимальный RAM-overhead ~400 МБ).
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
}

fn default_theme() -> String { "light".into() }
fn default_preload() -> bool { true }
fn default_ai_enabled() -> bool { true }
fn default_ai_preload() -> bool { false }
fn default_startup_launch() -> bool { false }
fn default_language() -> String { "en".into() }
fn default_ai_language() -> String { "en".into() }
fn default_ai_model() -> String { "qwen-0.5b".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkey {
    pub modifiers: Vec<String>, // ["alt"] | ["ctrl","shift"] etc.
    pub key: String,            // "x"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Telegram {
    pub api_id: i32,
    pub api_hash: String,
    /// Имя session-файла без расширения (относительно exe).
    pub session: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: "parakeet-v3".into(),
            hotkey: Hotkey::default(),
            telegram: Telegram::default(),
            recognition_language: "ru".into(),
            ui_theme: "light".into(),
            preload_model: true,
            ai_assistant_enabled: true,
            ai_preload: false,
            startup_launch: false,
            language: "en".into(),
            gemini_api_key: String::new(),
            ai_language: "en".into(),
            ai_model: "qwen-0.5b".into(),
        }
    }
}

impl Default for Hotkey {
    fn default() -> Self {
        Self {
            modifiers: vec!["alt".into()],
            key: "x".into(),
        }
    }
}

impl Default for Telegram {
    fn default() -> Self {
        Self {
            api_id: 0,                  // 0 → «не задано»; load() ругнётся
            api_hash: String::new(),    // пусто → load() ругнётся
            session: "voicy_session".into(),
        }
    }
}

impl Config {
    /// Прочитать TOML-конфиг и наложить ENV var'ы поверх. ENV побеждает
    /// если задан — это позволяет dev-окружению переопределять без правки
    /// voicy.toml.
    ///
    /// Валидацию credentials НЕ делаем — UI должен запуститься и без них,
    /// чтобы юзер мог их вписать в настройках. Проверяет `has_telegram_credentials()`
    /// тот код, кто реально пытается коннектиться (telegram::connect).
    pub fn load(path: &Path) -> Result<Self> {
        let txt = std::fs::read_to_string(path)
            .with_context(|| format!("read config: {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&txt).context("parse TOML")?;
        cfg.apply_env_overrides();
        Ok(cfg)
    }

    /// Подцепить ENV var'ы поверх уже распаршенного конфига.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var(ENV_API_ID) {
            if let Ok(n) = v.parse::<i32>() {
                self.telegram.api_id = n;
            }
        }
        if let Ok(v) = std::env::var(ENV_API_HASH) {
            if !v.trim().is_empty() {
                self.telegram.api_hash = v;
            }
        }
    }

    /// Готовы ли мы коннектиться к Telegram. Если нет — связываемся-через-сеть
    /// смысла не имеет, юзеру надо сначала вписать api_id/hash в UI.
    pub fn has_telegram_credentials(&self) -> bool {
        self.telegram.api_id > 0 && !self.telegram.api_hash.trim().is_empty()
    }

    /// Человекочитаемая инструкция «как задать credentials» — для error-сообщений.
    pub fn credentials_setup_hint() -> String {
        format!(
            "Telegram API credentials не заданы.\n\
             1) Зарегистрируй приложение на https://my.telegram.org → API development\n\
             2) Скопируй api_id и api_hash. Задай одним из способов:\n   \
                ENV vars (для разработки):  {ENV_API_ID}=..., {ENV_API_HASH}=...\n   \
                voicy.toml (для пользователей):\n      \
                  [telegram]\n      api_id = <число>\n      api_hash = \"<строка>\""
        )
    }

    #[allow(dead_code)]
    pub fn save(&self, path: &Path) -> Result<()> {
        let txt = toml::to_string_pretty(self).context("serialize TOML")?;
        std::fs::write(path, txt).with_context(|| format!("write config: {}", path.display()))?;
        Ok(())
    }
}

/// `voicy.toml` лежит рядом с исполняемым файлом.
pub fn default_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("voicy.toml")))
        .unwrap_or_else(|| PathBuf::from("voicy.toml"))
}
