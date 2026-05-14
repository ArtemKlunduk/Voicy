//! Конфиг приложения: hotkey, активная whisper-модель, креды Telegram,
//! путь к session-файлу, контакты. Хранится в `voicy.toml` рядом с .exe.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_API_ID: i32 = 32_825_589;
const DEFAULT_API_HASH: &str = "3886c6500e7c4a3628d4743671c24804";

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
}

fn default_theme() -> String { "light".into() }

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
            model: "base".into(),
            hotkey: Hotkey::default(),
            telegram: Telegram::default(),
            recognition_language: "ru".into(),
            ui_theme: "light".into(),
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
            api_id: DEFAULT_API_ID,
            api_hash: DEFAULT_API_HASH.into(),
            session: "voicy_session".into(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let txt = std::fs::read_to_string(path)
            .with_context(|| format!("read config: {}", path.display()))?;
        let cfg: Config = toml::from_str(&txt).context("parse TOML")?;
        Ok(cfg)
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
