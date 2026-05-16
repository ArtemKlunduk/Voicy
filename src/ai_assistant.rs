//! Локальный ИИ-ассистент через `candelabra` (llama.cpp bindings).
//!
//! Поддерживает 3 GGUF-модели на выбор — компромисс между RAM и качеством:
//!
//! | Модель        | RAM (q4) | Скорость   | Качество           |
//! |---------------|----------|------------|--------------------|
//! | Qwen 2.5 0.5B | ~400 МБ  | очень быст | хватает на классиф |
//! | Llama 3.2 1B  | ~800 МБ  | быстро     | хорошо             |
//! | Gemma 2 2B    | ~1.5 ГБ  | средне     | отлично            |
//!
//! Все скачиваются с HuggingFace в `%APPDATA%/voicy/models/`.
//!
//! Выбор модели приходит из `Config::ai_model` (строка "qwen-0.5b", "llama-3.2-1b",
//! "gemma-2-2b"). По умолчанию — Qwen для минимального оверхеда.

use anyhow::{anyhow, Result};
use candelabra::{
    check_model_cached, download_model, load_tokenizer_from_repo, InferenceConfig, Model,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static CANCEL_TOKEN: AtomicBool = AtomicBool::new(false);

/// Системный промпт — общий для всех моделей. Заставляет отвечать кратко.
const SYSTEM_PROMPT: &str = "Ты голосовой ассистент. Отвечай кратко, по существу, максимум 2 предложения. Говори естественно, как человек.";

/// Перечень моделей. Каждая знает свой repo, файл, токенизатор и chat template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiModel {
    /// Qwen 2.5 0.5B Instruct (Q4_K_M, ~350 МБ).
    Qwen05B,
    /// Llama 3.2 1B Instruct (Q4_K_M, ~770 МБ).
    Llama32_1B,
    /// Gemma 2 2B IT (Q4_K_M, ~1.6 ГБ).
    Gemma2_2B,
}

impl AiModel {
    pub fn from_config_str(s: &str) -> Self {
        match s.to_lowercase().replace(['_', ' '], "-").as_str() {
            "qwen-0.5b" | "qwen2.5-0.5b" | "qwen" => Self::Qwen05B,
            "llama-3.2-1b" | "llama-1b" | "llama3.2-1b" | "llama" => Self::Llama32_1B,
            "gemma-2-2b" | "gemma2-2b" | "gemma-2b" | "gemma" => Self::Gemma2_2B,
            _ => Self::Qwen05B, // дефолт — самая лёгкая
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Qwen05B => "qwen-0.5b",
            Self::Llama32_1B => "llama-3.2-1b",
            Self::Gemma2_2B => "gemma-2-2b",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Qwen05B => "Qwen 2.5 0.5B",
            Self::Llama32_1B => "Llama 3.2 1B",
            Self::Gemma2_2B => "Gemma 2 2B",
        }
    }

    /// Примерный размер скачивания / RAM-overhead (для UI).
    pub fn approx_ram_mb(self) -> u32 {
        match self {
            Self::Qwen05B => 400,
            Self::Llama32_1B => 800,
            Self::Gemma2_2B => 1500,
        }
    }

    /// HuggingFace repo с GGUF файлом.
    fn model_repo(self) -> &'static str {
        match self {
            Self::Qwen05B => "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
            Self::Llama32_1B => "unsloth/Llama-3.2-1B-Instruct-GGUF",
            Self::Gemma2_2B => "bartowski/gemma-2-2b-it-GGUF",
        }
    }

    /// Имя GGUF-файла внутри репо.
    fn model_file(self) -> &'static str {
        match self {
            Self::Qwen05B => "qwen2.5-0.5b-instruct-q4_k_m.gguf",
            Self::Llama32_1B => "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            Self::Gemma2_2B => "gemma-2-2b-it-Q4_K_M.gguf",
        }
    }

    /// Репо с токенизатором (отдельный от GGUF).
    /// ВАЖНО: google/gemma-* и meta-llama/Llama-* — gated на HF (401 без
    /// токена). Используем non-gated зеркала unsloth, которые публикуют
    /// идентичный tokenizer.json без license-acceptance.
    fn tokenizer_repo(self) -> &'static str {
        match self {
            Self::Qwen05B => "Qwen/Qwen2.5-0.5B-Instruct",
            Self::Llama32_1B => "unsloth/Llama-3.2-1B-Instruct",
            Self::Gemma2_2B => "unsloth/gemma-2-2b-it",
        }
    }

    /// Собрать prompt в chat-template'е модели.
    fn build_prompt(self, system: &str, user: &str) -> String {
        match self {
            // Qwen: <|im_start|>role\n...<|im_end|>\n
            Self::Qwen05B => format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                system, user
            ),
            // Llama 3.2: <|start_header_id|>role<|end_header_id|>\n\n...<|eot_id|>
            Self::Llama32_1B => format!(
                "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{}<|eot_id|>\
                <|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|>\
                <|start_header_id|>assistant<|end_header_id|>\n\n",
                system, user
            ),
            // Gemma 2: <start_of_turn>role\n...<end_of_turn>\n
            // Gemma не поддерживает system role — клеим в user-турн.
            Self::Gemma2_2B => format!(
                "<start_of_turn>user\n{}\n\n{}<end_of_turn>\n<start_of_turn>model\n",
                system, user
            ),
        }
    }

    /// Спецтокены, которые надо вырезать из ответа.
    fn cleanup_tokens(self) -> &'static [&'static str] {
        match self {
            Self::Qwen05B => &[
                "<|im_end|>",
                "<|im_start|>",
                "<|endoftext|>",
            ],
            Self::Llama32_1B => &[
                "<|eot_id|>",
                "<|end_of_text|>",
                "<|start_header_id|>",
                "<|end_header_id|>",
                "<|begin_of_text|>",
            ],
            Self::Gemma2_2B => &[
                "<start_of_turn>",
                "<end_of_turn>",
                "<eos>",
            ],
        }
    }
}

/// Загруженная модель + токенизатор.
pub struct AiAssistant {
    model: Model,
    tokenizer: tokenizers::Tokenizer,
    kind: AiModel,
}

pub struct AiResponse {
    pub text: String,
    pub tokens_per_second: f64,
}

impl AiAssistant {
    /// Загрузить если уже в кэше, иначе None.
    pub fn load_if_cached(kind: AiModel) -> Result<Option<Self>> {
        if !check_model_cached(kind.model_repo(), kind.model_file()) {
            return Ok(None);
        }
        let model_path = download_model(kind.model_repo(), kind.model_file())?;
        let tokenizer = load_tokenizer_from_repo(kind.tokenizer_repo())?;
        let model = Model::load(&model_path)?;
        Ok(Some(Self { model, tokenizer, kind }))
    }

    /// Скачать GGUF-файл (синхронно).
    pub fn download_model_sync(kind: AiModel) -> Result<PathBuf> {
        let path = download_model(kind.model_repo(), kind.model_file())?;
        Ok(path)
    }

    /// Загрузить уже скачанную модель.
    #[allow(dead_code)]
    pub fn load(kind: AiModel, model_path: &PathBuf) -> Result<Self> {
        let tokenizer = load_tokenizer_from_repo(kind.tokenizer_repo())?;
        let model = Model::load(model_path)?;
        Ok(Self { model, tokenizer, kind })
    }

    /// Проверить кэш на диске.
    pub fn is_model_cached(kind: AiModel) -> bool {
        check_model_cached(kind.model_repo(), kind.model_file())
    }

    /// Сгенерировать ответ.
    pub fn ask(&mut self, question: &str) -> Result<AiResponse> {
        if question.trim().is_empty() {
            return Err(anyhow!("Empty question"));
        }
        let prompt = self.kind.build_prompt(SYSTEM_PROMPT, question);

        let mut config = InferenceConfig::default();
        config.prompt = prompt;
        config.max_tokens = 128;
        config.temperature = 0.7;

        let cancel = Arc::new(AtomicBool::new(false));
        let mut response_text = String::new();

        let result = candelabra::run_inference(
            &mut self.model,
            &self.tokenizer,
            &config,
            cancel,
            |token| {
                response_text.push_str(&token);
                Ok(())
            },
        )?;

        // Чистим спецтокены конкретной модели.
        let mut cleaned = response_text;
        for tok in self.kind.cleanup_tokens() {
            cleaned = cleaned.replace(tok, "");
        }
        let cleaned = cleaned.trim().to_string();

        Ok(AiResponse {
            text: cleaned,
            tokens_per_second: result.tokens_per_second,
        })
    }

    pub fn kind(&self) -> AiModel {
        self.kind
    }

    pub fn cancel() {
        CANCEL_TOKEN.store(true, Ordering::Relaxed);
    }
}
