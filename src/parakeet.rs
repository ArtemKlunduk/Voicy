//! Parakeet ASR через `transcribe-rs` (тот же стек что у Handy).
//!
//! Загружает int8 ONNX-модель из папки и крутит инференс. Модели качаются
//! HF-репозиториев istupakov/parakeet-tdt-0.6b-v3-onnx и -v2-onnx,
//! где encoder + decoder_joint + nemo128 + vocab.txt лежат готовые.
//!
//! Mel-спектрограмма зашита в `nemo128.onnx` — мы просто кормим f32 PCM 16kHz.

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::OnceLock;
use tracing::{info, warn};

use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

/// Явная инициализация ORT. Если этого не сделать, ort инициализируется
/// лениво в первом Session::builder() — и на gnullvm + MSVC-DLL зависает
/// без какой-либо диагностики. Тут мы делаем init руками и логируем результат.
static ORT_READY: std::sync::Once = std::sync::Once::new();
fn ensure_ort() -> Result<()> {
    let mut result: Result<()> = Ok(());
    ORT_READY.call_once(|| {
        info!("[parakeet] ort::init() calling…");
        let r = std::panic::catch_unwind(|| ort::init().commit());
        match r {
            Ok(true) => info!("[parakeet] ort::init() OK (initialized)"),
            Ok(false) => info!("[parakeet] ort::init() OK (already initialized)"),
            Err(_) => {
                warn!("[parakeet] ort::init() PANICKED");
                result = Err(anyhow!("ort init panicked — onnxruntime.dll проблема"));
            }
        }
    });
    result
}

/// Кэш загруженной модели — Parakeet тяжёлый, инициализация занимает секунды.
/// Держим один экземпляр на (model_name, quant), пере-загружаем только при смене.
static MODEL_CACHE: OnceLock<Mutex<Option<CachedModel>>> = OnceLock::new();

struct CachedModel {
    name: String,
    model: ParakeetModel,
}

fn cache() -> &'static Mutex<Option<CachedModel>> {
    MODEL_CACHE.get_or_init(|| Mutex::new(None))
}

/// Прогреть модель (загрузить в RAM). Вызывать на старте listener — иначе
/// первый Alt+X будет ждать загрузку 600 MB ONNX (~10 сек).
pub fn preload(model_name: &str, model_dir: &Path) -> Result<()> {
    let mut slot = cache().lock();
    let need = match slot.as_ref() {
        Some(c) => c.name != model_name,
        None => true,
    };
    if !need {
        return Ok(());
    }
    info!("[parakeet] PRE-LOADING {} from {}", model_name, model_dir.display());
    let t0 = std::time::Instant::now();
    let model = ParakeetModel::load(model_dir, &Quantization::Int8)
        .map_err(|e| anyhow!("ParakeetModel::load: {}", e))?;
    info!("[parakeet] pre-loaded in {:.2}s", t0.elapsed().as_secs_f32());
    *slot = Some(CachedModel { name: model_name.to_string(), model });
    Ok(())
}

/// Транскрибировать сэмплы (i16 16kHz mono) через Parakeet.
pub fn transcribe_samples(model_name: &str, model_dir: &Path, samples_i16: &[i16]) -> Result<String> {
    info!("[parakeet] ENTRY model={} samples={}", model_name, samples_i16.len());

    // Конверсия i16 → f32 [-1.0, 1.0]
    let samples: Vec<f32> = samples_i16.iter().map(|&s| s as f32 / 32_768.0).collect();
    info!("[parakeet] converted i16→f32, {} samples", samples.len());

    info!("[parakeet] acquiring cache lock…");
    let mut slot = cache().lock();
    info!("[parakeet] cache lock acquired");

    let need_reload = match slot.as_ref() {
        Some(c) => c.name != model_name,
        None => true,
    };
    if need_reload {
        info!("[parakeet] ensure_ort()…");
        ensure_ort()?;
        info!("[parakeet] LOAD START: {} from {}", model_name, model_dir.display());
        let t0 = std::time::Instant::now();
        let model = ParakeetModel::load(model_dir, &Quantization::Int8)
            .map_err(|e| anyhow!("ParakeetModel::load: {}", e))?;
        info!("[parakeet] LOAD OK in {:.2}s", t0.elapsed().as_secs_f32());
        *slot = Some(CachedModel { name: model_name.to_string(), model });
    } else {
        info!("[parakeet] model already cached");
    }
    let cached = slot.as_mut().unwrap();

    info!("[parakeet] transcribe START ({} f32 samples)", samples.len());
    let t0 = std::time::Instant::now();
    let result = cached
        .model
        .transcribe_with(&samples, &ParakeetParams::default())
        .map_err(|e| anyhow!("transcribe: {}", e))?;
    let dt = t0.elapsed().as_secs_f32();
    let secs = samples.len() as f32 / 16_000.0;
    info!(
        "[parakeet] transcribe OK: {:.2}s audio in {:.2}s ({:.1}× realtime) → «{}»",
        secs, dt, if dt > 0.0 { secs / dt } else { 0.0 }, result.text
    );
    Ok(result.text.trim().to_string())
}

/// Сбросить кэш модели (например после удаления весов / смены пути).
pub fn drop_cache() {
    if let Some(lock) = MODEL_CACHE.get() {
        let mut slot = lock.lock();
        if slot.is_some() {
            warn!("[parakeet] dropping cached model");
        }
        *slot = None;
    }
}
