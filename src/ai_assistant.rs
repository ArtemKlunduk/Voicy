use anyhow::{anyhow, Result};
use candelabra::{
    check_model_cached, download_model, load_tokenizer_from_repo, InferenceConfig, Model,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static CANCEL_TOKEN: AtomicBool = AtomicBool::new(false);

/// Модель и токенизатор для ИИ-ассистента.
pub struct AiAssistant {
    model: Model,
    tokenizer: tokenizers::Tokenizer,
}

/// Результат генерации: текст + метаданные.
pub struct AiResponse {
    pub text: String,
    pub tokens_per_second: f64,
}

/// Репозиторий и имя файла модели Qwen2.5-0.5B-Instruct Q4_K_M (~350 MB).
const MODEL_REPO: &str = "Qwen/Qwen2.5-0.5B-Instruct-GGUF";
const MODEL_FILE: &str = "qwen2.5-0.5b-instruct-q4_k_m.gguf";
const TOKENIZER_REPO: &str = "Qwen/Qwen2.5-0.5B-Instruct";

/// Системный промпт — вшит, заставляет отвечать кратко.
const SYSTEM_PROMPT: &str = "Ты голосовой ассистент. Отвечай кратко, по существу, максимум 2 предложения. Говори естественно, как человек.";

impl AiAssistant {
    /// Загружает модель и токенизатор. Если модели нет в кэше — вернёт None.
    pub fn load_if_cached() -> Result<Option<Self>> {
        if !check_model_cached(MODEL_REPO, MODEL_FILE) {
            return Ok(None);
        }
        let model_path = download_model(MODEL_REPO, MODEL_FILE)?;
        let tokenizer = load_tokenizer_from_repo(TOKENIZER_REPO)?;
        let model = Model::load(&model_path)?;
        Ok(Some(Self { model, tokenizer }))
    }

    /// Скачивает модель (синхронно, с прогрессом в логах).
    pub fn download_model_sync() -> Result<PathBuf> {
        let path = download_model(MODEL_REPO, MODEL_FILE)?;
        Ok(path)
    }

    /// Загружает модель после скачивания.
    pub fn load(model_path: &PathBuf) -> Result<Self> {
        let tokenizer = load_tokenizer_from_repo(TOKENIZER_REPO)?;
        let model = Model::load(model_path)?;
        Ok(Self { model, tokenizer })
    }

    /// Проверяет, скачана ли модель.
    pub fn is_model_cached() -> bool {
        check_model_cached(MODEL_REPO, MODEL_FILE)
    }

    /// Генерирует краткий ответ на вопрос.
    pub fn ask(&mut self, question: &str) -> Result<AiResponse> {
        if question.trim().is_empty() {
            return Err(anyhow!("Empty question"));
        }

        // Qwen2.5 chat template
        let prompt = format!(
            "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            SYSTEM_PROMPT, question
        );

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

        // Очистка от спецтокенов и лишних пробелов
        let cleaned = response_text
            .replace("<|im_end|>", "")
            .replace("<|im_start|>", "")
            .replace("<|endoftext|>", "")
            .trim()
            .to_string();

        Ok(AiResponse {
            text: cleaned,
            tokens_per_second: result.tokens_per_second,
        })
    }

    /// Прерывает текущую генерацию.
    pub fn cancel() {
        CANCEL_TOKEN.store(true, Ordering::Relaxed);
    }
}
