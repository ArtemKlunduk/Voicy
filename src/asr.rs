//! Распознавание через whisper.cpp prebuilt-бинарь + реестр моделей всех движков.
//!
//! Whisper — реальный inference через `whisper-cli.exe` (subprocess).
//! NeMo / Moonshine / GigaAM / SenseVoice / Breeze / Cohere — пока только
//! скачивание весов (как и в Python-версии); inference вне Rust.

use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

const WHISPER_CPP_RELEASE_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-blas-bin-x64.zip";

/// Microsoft ONNX Runtime для Win x64. Версия должна совпадать с тем что
/// требует `ort` крейт (для ort 2.0.0-rc.12 это onnxruntime 1.20.x).
const ONNXRUNTIME_ZIP_URL: &str =
    "https://github.com/microsoft/onnxruntime/releases/download/v1.20.1/onnxruntime-win-x64-1.20.1.zip";

fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("voicy"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Папка для whisper.cpp бинаря + ggml-моделей.
pub fn assets_dir() -> PathBuf {
    let base = app_data_dir();
    let _ = std::fs::create_dir_all(&base);
    base.join("whisper")
}

/// Папка для не-whisper моделей: `<app_data>/models/<name>/...`
pub fn voicy_models_dir() -> PathBuf {
    let base = app_data_dir();
    let _ = std::fs::create_dir_all(&base);
    base.join("models")
}

pub fn whisper_cli_path() -> PathBuf {
    assets_dir().join("whisper-cli.exe")
}

/// Путь к onnxruntime.dll рядом с exe (load-dynamic ort).
pub fn onnxruntime_dll_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("onnxruntime.dll")))
        .unwrap_or_else(|| PathBuf::from("onnxruntime.dll"))
}

/// Скачать onnxruntime.dll если ещё нет. Нужен для Parakeet/ONNX-моделей.
pub fn ensure_onnxruntime() -> Result<PathBuf> {
    let dst = onnxruntime_dll_path();
    if dst.exists() {
        return Ok(dst);
    }
    let exe_dir = dst.parent().unwrap_or(Path::new(".")).to_path_buf();
    std::fs::create_dir_all(&exe_dir)?;
    info!("[asr] downloading onnxruntime.dll: {}", ONNXRUNTIME_ZIP_URL);
    let zip_bytes = http_get_bytes(ONNXRUNTIME_ZIP_URL)?;
    let zip_path = exe_dir.join("onnxruntime.zip");
    std::fs::write(&zip_path, &zip_bytes)?;
    info!("[asr] extracting onnxruntime ({} bytes)…", zip_bytes.len());
    let f = std::fs::File::open(&zip_path)?;
    let mut z = zip::ZipArchive::new(f)?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i)?;
        let raw_name = entry.name().to_string();
        // Ищем lib/onnxruntime.dll внутри zip — кладём ИМЕННО его рядом с exe.
        if !raw_name.ends_with("/onnxruntime.dll") && !raw_name.ends_with("\\onnxruntime.dll") {
            continue;
        }
        let mut f = std::fs::File::create(&dst)?;
        std::io::copy(&mut entry, &mut f)?;
        break;
    }
    let _ = std::fs::remove_file(&zip_path);
    if !dst.exists() {
        return Err(anyhow!("onnxruntime.dll не найден в распакованном архиве"));
    }
    info!("[asr] onnxruntime.dll ready: {}", dst.display());
    Ok(dst)
}

/// Метаданные модели (имя, движок, размер, репо HF, локализация).
#[derive(Debug, Clone, Copy)]
pub struct ModelMeta {
    pub name: &'static str,
    /// «Семья» — общая шапка в UI: "Whisper", "Parakeet", "Canary", "Moonshine"…
    pub family: &'static str,
    /// Размер внутри семьи: "Tiny", "Base", "Small", "Medium", "Large v3", "V3", "180M Flash"…
    pub variant: &'static str,
    pub display: &'static str,
    pub size: &'static str,
    pub engine: &'static str, // "whisper" | "nemo" | "salutespeech" | "moonshine" | "funasr" | "transformers" | "cohere"
    pub lang: &'static str,
    pub desc: &'static str,
    /// HF repo (для не-whisper). У whisper свой механизм (ggml.bin с фиксированного URL).
    pub hf_repo: Option<&'static str>,
    /// Рекомендуемая модель — выделяется в UI на первый план.
    pub recommended: bool,
}

pub const MODELS: &[ModelMeta] = &[
    // ── NVIDIA Parakeet (main) ───────────────────────────────────────
    // Используем ONNX int8 конверсии istupakov — те же что в Handy.
    // .nemo (оригинал NVIDIA) требует Python+PyTorch, не катит для Rust.
    ModelMeta { name: "parakeet-v3", family: "Parakeet", variant: "V3", display: "Parakeet V3", size: "612 MB", engine: "nemo", lang: "multi", desc: "NVIDIA NeMo. Fast and accurate, multilingual.", hf_repo: Some("istupakov/parakeet-tdt-0.6b-v3-onnx"), recommended: true },
    ModelMeta { name: "parakeet-v2", family: "Parakeet", variant: "V2", display: "Parakeet V2", size: "451 MB", engine: "nemo", lang: "en",    desc: "NVIDIA NeMo. Best for English speakers.", hf_repo: Some("istupakov/parakeet-tdt-0.6b-v2-onnx"), recommended: false },

    // ── Whisper (real inference via whisper.cpp) ─────────────────────
    ModelMeta { name: "tiny",     family: "Whisper", variant: "Tiny",     display: "Whisper Tiny",      size: "75 MB",  engine: "whisper", lang: "multi", desc: "Fastest, baseline quality.", hf_repo: None, recommended: false },
    ModelMeta { name: "base",     family: "Whisper", variant: "Base",     display: "Whisper Base",      size: "142 MB", engine: "whisper", lang: "multi", desc: "Balanced. Standard for short phrases.", hf_repo: None, recommended: false },
    ModelMeta { name: "small",    family: "Whisper", variant: "Small",    display: "Whisper Small",     size: "466 MB", engine: "whisper", lang: "multi", desc: "Noticeably more accurate. A bit slower.", hf_repo: None, recommended: false },
    ModelMeta { name: "medium",   family: "Whisper", variant: "Medium",   display: "Whisper Medium",    size: "1.5 GB", engine: "whisper", lang: "multi", desc: "High accuracy on long/noisy recordings.", hf_repo: None, recommended: false },
    ModelMeta { name: "large-v3", family: "Whisper", variant: "Large v3", display: "Whisper Large v3",  size: "3.0 GB", engine: "whisper", lang: "multi", desc: "Top accuracy. Slow to load.", hf_repo: None, recommended: false },
    ModelMeta { name: "turbo",    family: "Whisper", variant: "Turbo",    display: "Whisper Turbo",     size: "1.5 GB", engine: "whisper", lang: "multi", desc: "Fast large. Good balance.", hf_repo: None, recommended: false },

    // ── NVIDIA Canary ────────────────────────────────────────────────
    ModelMeta { name: "canary-180m",  family: "Canary", variant: "180M Flash", display: "Canary 180M Flash", size: "364 MB", engine: "nemo", lang: "multi", desc: "EN, DE, ES, FR. Translation support.", hf_repo: Some("nvidia/canary-180m-flash"), recommended: false },
    ModelMeta { name: "canary-1b-v2", family: "Canary", variant: "1B v2",      display: "Canary 1B v2",       size: "691 MB", engine: "nemo", lang: "multi", desc: "Accurate multilingual. 25 EU languages.", hf_repo: Some("nvidia/canary-1b-v2"), recommended: false },

    // ── Sber GigaAM (Russian) ────────────────────────────────────────
    ModelMeta { name: "gigaam-v3", family: "GigaAM", variant: "v3", display: "GigaAM v3", size: "1.9 GB", engine: "salutespeech", lang: "ru", desc: "Sber. Russian speech recognition. Fast and accurate.", hf_repo: Some("salute-developers/GigaAM"), recommended: false },

    // ── Moonshine (Useful Sensors, EN) ───────────────────────────────
    ModelMeta { name: "moonshine-tiny",   family: "Moonshine", variant: "Tiny",   display: "Moonshine Tiny",   size: "31 MB",  engine: "moonshine", lang: "en", desc: "Super fast, English.", hf_repo: Some("UsefulSensors/moonshine-tiny"), recommended: false },
    ModelMeta { name: "moonshine-base",   family: "Moonshine", variant: "Base",   display: "Moonshine Base",   size: "55 MB",  engine: "moonshine", lang: "en", desc: "Very fast. Handles accents well.", hf_repo: Some("UsefulSensors/moonshine-base"), recommended: false },
    ModelMeta { name: "moonshine-small",  family: "Moonshine", variant: "Small",  display: "Moonshine Small",  size: "99 MB",  engine: "moonshine", lang: "en", desc: "Balanced speed and accuracy.", hf_repo: Some("UsefulSensors/moonshine-small"), recommended: false },
    ModelMeta { name: "moonshine-medium", family: "Moonshine", variant: "Medium", display: "Moonshine Medium", size: "192 MB", engine: "moonshine", lang: "en", desc: "English only. High quality.", hf_repo: Some("UsefulSensors/moonshine-medium"), recommended: false },

    // ── Other (single variants) ──────────────────────────────────────
    ModelMeta { name: "sense-voice", family: "SenseVoice", variant: "Small", display: "SenseVoice Small", size: "152 MB", engine: "funasr",       lang: "multi", desc: "Very fast. ZH, EN, JA, KO, Cantonese.", hf_repo: Some("FunAudioLLM/SenseVoiceSmall"), recommended: false },
    ModelMeta { name: "breeze-asr",  family: "Breeze ASR", variant: "25",    display: "Breeze ASR 25",    size: "320 MB", engine: "transformers", lang: "multi", desc: "Optimized for Taiwanese Mandarin.", hf_repo: Some("MediaTek-Research/Breeze-ASR-25"), recommended: false },
    ModelMeta { name: "cohere-aya",  family: "Cohere Aya", variant: "8B",    display: "Cohere Aya 8B",    size: "1.7 GB", engine: "cohere",       lang: "multi", desc: "Large, slow, but very accurate.", hf_repo: Some("CohereLabs/aya-expanse-8b"), recommended: false },
];

pub fn model_meta(name: &str) -> Option<&'static ModelMeta> {
    MODELS.iter().find(|m| m.name == name)
}

/// Путь к ggml-файлу whisper.cpp. Используется только для whisper-движка.
pub fn model_path(name: &str) -> PathBuf {
    assets_dir().join(format!("ggml-{}.bin", name))
}

/// Папка с весами не-whisper модели.
pub fn nemo_model_dir(name: &str) -> PathBuf {
    voicy_models_dir().join(name)
}

/// Скачана ли модель. Для whisper — наличие ggml-файла. Для Parakeet —
/// ВСЕ 4 нужных ONNX-файла, иначе ParakeetModel::load запаникует.
pub fn model_is_downloaded(name: &str) -> bool {
    let Some(meta) = model_meta(name) else { return false };
    match meta.engine {
        "whisper" => model_path(name).exists(),
        "nemo" => {
            // Parakeet int8 — то, что грузит transcribe-rs::onnx::parakeet::ParakeetModel
            const REQUIRED: &[&str] = &[
                "encoder-model.int8.onnx",
                "decoder_joint-model.int8.onnx",
                "nemo128.onnx",
                "vocab.txt",
            ];
            let dir = nemo_model_dir(name);
            REQUIRED.iter().all(|f| {
                let p = dir.join(f);
                p.exists() && std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false)
            })
        }
        _ => {
            let d = nemo_model_dir(name);
            std::fs::read_dir(&d)
                .map(|it| it.flatten().any(|e| {
                    e.metadata().map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
                }))
                .unwrap_or(false)
        }
    }
}

pub fn hf_model_url(name: &str) -> String {
    format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
        name
    )
}

/// Скачать и распаковать whisper.cpp бинарь, если ещё нет.
pub fn ensure_whisper_cli() -> Result<PathBuf> {
    let cli = whisper_cli_path();
    if cli.exists() {
        return Ok(cli);
    }
    std::fs::create_dir_all(assets_dir())?;
    info!("[asr] downloading whisper.cpp: {}", WHISPER_CPP_RELEASE_URL);
    let zip_bytes = http_get_bytes(WHISPER_CPP_RELEASE_URL)?;
    let zip_path = assets_dir().join("whisper-bin-x64.zip");
    std::fs::write(&zip_path, &zip_bytes)?;
    info!("[asr] extracting {} bytes…", zip_bytes.len());
    let f = std::fs::File::open(&zip_path)?;
    let mut z = zip::ZipArchive::new(f)?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i)?;
        let raw_name = entry.name().to_string();
        let name = Path::new(&raw_name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(raw_name);
        if name.is_empty() || entry.is_dir() {
            continue;
        }
        let out = assets_dir().join(&name);
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut f)?;
    }
    let _ = std::fs::remove_file(&zip_path);
    if !cli.exists() {
        let alt = assets_dir().join("main.exe");
        if alt.exists() {
            std::fs::rename(&alt, &cli)?;
        }
    }
    if !cli.exists() {
        return Err(anyhow!(
            "после распаковки whisper-cli.exe не найден в {}",
            assets_dir().display()
        ));
    }
    info!("[asr] whisper.cpp ready: {}", cli.display());
    Ok(cli)
}

/// Унифицированная точка входа: скачивает модель по движку.
pub fn download_model(name: &str) -> Result<PathBuf> {
    let meta = model_meta(name).ok_or_else(|| anyhow!("unknown model: {}", name))?;
    match meta.engine {
        "whisper" => download_whisper_ggml(name),
        "nemo" => {
            // Parakeet: качаем только нужные int8 файлы, не всё подряд.
            // HF-репо istupakov содержит ещё и FP32 версию на 2.4 GB.
            let repo = meta.hf_repo.ok_or_else(|| anyhow!("no hf_repo for {}", name))?;
            download_parakeet_int8(name, repo)
        }
        _ => {
            let repo = meta
                .hf_repo
                .ok_or_else(|| anyhow!("no hf_repo for {}", name))?;
            download_hf_repo(name, repo)
        }
    }
}

/// Целевая загрузка Parakeet int8 — только файлы, которые реально нужны
/// transcribe_rs::onnx::parakeet::ParakeetModel::load с `Quantization::Int8`.
/// Без этого фильтра пользователь качал бы 3+ GB вместо 670 MB.
fn download_parakeet_int8(name: &str, repo: &str) -> Result<PathBuf> {
    const FILES: &[&str] = &[
        "encoder-model.int8.onnx",
        "decoder_joint-model.int8.onnx",
        "nemo128.onnx",
        "vocab.txt",
        "config.json",
    ];
    let target = nemo_model_dir(name);
    std::fs::create_dir_all(&target)?;
    info!("[asr] downloading Parakeet int8 from {} → {}", repo, target.display());
    for file in FILES {
        let url = format!("https://huggingface.co/{}/resolve/main/{}", repo, file);
        let dst = target.join(file);
        if dst.exists() && std::fs::metadata(&dst).map(|m| m.len() > 0).unwrap_or(false) {
            info!("[asr]   skip {} (cached)", file);
            continue;
        }
        info!("[asr]   ↓ {}", file);
        let tmp = dst.with_extension("part");
        http_stream_to_file(&url, &tmp)?;
        std::fs::rename(&tmp, &dst)?;
    }
    Ok(target)
}

fn download_whisper_ggml(name: &str) -> Result<PathBuf> {
    let dst = model_path(name);
    if dst.exists() {
        info!("[asr] {} already exists", dst.display());
        return Ok(dst);
    }
    std::fs::create_dir_all(assets_dir())?;
    let url = hf_model_url(name);
    info!("[asr] downloading {} from {}", name, url);
    let tmp = dst.with_extension("bin.tmp");
    http_stream_to_file(&url, &tmp)?;
    std::fs::rename(&tmp, &dst)?;
    info!("[asr] saved → {}", dst.display());
    Ok(dst)
}

/// Скачать все файлы HF-репо в `<exe_dir>/models/<name>/`.
/// Берём дерево через HF API, далее GET каждого файла.
fn download_hf_repo(name: &str, repo: &str) -> Result<PathBuf> {
    let target = nemo_model_dir(name);
    std::fs::create_dir_all(&target)?;

    let tree_url = format!(
        "https://huggingface.co/api/models/{}/tree/main?recursive=true",
        repo
    );
    info!("[asr] HF tree: {}", tree_url);
    let body = http_get_bytes(&tree_url)?;
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&body)
        .context("parse HF tree")?;

    let files: Vec<(String, u64)> = entries
        .iter()
        .filter_map(|e| {
            let kind = e.get("type")?.as_str()?;
            if kind != "file" {
                return None;
            }
            let path = e.get("path")?.as_str()?.to_string();
            let size = e.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            Some((path, size))
        })
        .collect();

    if files.is_empty() {
        return Err(anyhow!("в репо {} не найдено файлов", repo));
    }
    let total: u64 = files.iter().map(|(_, s)| *s).sum();
    info!(
        "[asr] downloading {} files ({:.1} MB) → {}",
        files.len(),
        total as f64 / 1_048_576.0,
        target.display()
    );

    for (path, size) in &files {
        // Пропускаем тяжёлые опциональные дубликаты (FP32 веса, если есть FP16).
        let lower = path.to_lowercase();
        if lower.ends_with(".gitattributes") || lower.ends_with(".md") {
            continue;
        }
        let file_url = format!("https://huggingface.co/{}/resolve/main/{}", repo, path);
        let dst = target.join(path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if dst.exists() {
            if let Ok(m) = std::fs::metadata(&dst) {
                if *size > 0 && m.len() == *size {
                    info!("[asr]   skip {} (cached)", path);
                    continue;
                }
            }
        }
        info!(
            "[asr]   ↓ {} ({:.1} MB)",
            path,
            *size as f64 / 1_048_576.0
        );
        let tmp = dst.with_extension("part");
        http_stream_to_file(&file_url, &tmp)?;
        std::fs::rename(&tmp, &dst)?;
    }
    Ok(target)
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url).call().context("http GET")?;
    let mut reader = resp.into_reader();
    let mut buf = Vec::new();
    std::io::copy(&mut reader, &mut buf).context("read body")?;
    Ok(buf)
}

/// Стримим body в файл (без накопления в RAM — важно для 1+ GB моделей).
fn http_stream_to_file(url: &str, dst: &Path) -> Result<()> {
    let resp = ureq::get(url).call().context("http GET")?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dst)?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).context("read body")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("write file")?;
    }
    file.flush()?;
    Ok(())
}

use std::io::Read;

/// Транскрибировать WAV (16kHz mono). Сначала пробует Parakeet (если выбран
/// и веса скачаны), при таймауте/ошибке тихо падает в whisper-cli.
pub fn transcribe_wav(wav: &Path, model: &str, language: &str) -> Result<String> {
    let meta = model_meta(model).ok_or_else(|| anyhow!("unknown model: {}", model))?;
    match meta.engine {
        "whisper" => transcribe_wav_whisper(wav, model, language),
        "nemo" => {
            // Пытаемся Parakeet, при провале — на whisper-fallback.
            match transcribe_wav_parakeet(wav, model) {
                Ok(text) => Ok(text),
                Err(e) => {
                    warn!("[asr] Parakeet failed ({}), fallback на whisper", e);
                    let fallback = pick_fallback_whisper();
                    match fallback {
                        Some(w) => transcribe_wav_whisper(wav, &w, language),
                        None => Err(anyhow!(
                            "Parakeet не сработал ({}), а Whisper не скачан. \
                             Открой Models → Whisper → Base.",
                            e
                        )),
                    }
                }
            }
        }
        other => Err(anyhow!("движок {} не поддерживается для inference", other)),
    }
}

/// Найти лучшую скачанную whisper-модель для fallback.
fn pick_fallback_whisper() -> Option<String> {
    for name in &["large-v3", "turbo", "medium", "small", "base", "tiny"] {
        if model_is_downloaded(name) {
            return Some(name.to_string());
        }
    }
    None
}

/// Parakeet через transcribe-rs (ONNX runtime).
fn transcribe_wav_parakeet(wav: &Path, model: &str) -> Result<String> {
    info!("[asr] dispatch → parakeet model={} wav={}", model, wav.display());
    let model_dir = nemo_model_dir(model);
    if !model_dir.exists() {
        return Err(anyhow!(
            "модель {} не скачана — открой выпадашку и нажми download",
            model
        ));
    }
    const REQUIRED: &[&str] = &[
        "encoder-model.int8.onnx",
        "decoder_joint-model.int8.onnx",
        "nemo128.onnx",
        "vocab.txt",
    ];
    let missing: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|f| !model_dir.join(f).exists())
        .collect();
    if !missing.is_empty() {
        return Err(anyhow!(
            "Parakeet incomplete: нет {} в {}. Удали папку и скачай заново.",
            missing.join(", "),
            model_dir.display()
        ));
    }
    if !onnxruntime_dll_path().exists() {
        return Err(anyhow!(
            "onnxruntime.dll отсутствует рядом с exe."
        ));
    }

    // Жёсткий timeout: запускаем инференс в отдельном треде, ждём ≤90 секунд.
    // Если ort ABI на gnullvm подвис — пайплайн всё равно выйдет с ошибкой,
    // overlay не залипнет.
    let wav_owned = wav.to_path_buf();
    let model_owned = model.to_string();
    let model_dir_owned = model_dir.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            info!("[asr] parakeet thread: reading WAV…");
            let samples_f32 = transcribe_rs::audio::read_wav_samples(&wav_owned)
                .map_err(|e| anyhow!("read_wav_samples: {}", e))?;
            info!("[asr] parakeet thread: WAV read, {} f32 samples", samples_f32.len());
            let samples_i16: Vec<i16> = samples_f32
                .iter()
                .map(|&s| (s * 32_767.0).clamp(-32_768.0, 32_767.0) as i16)
                .collect();
            crate::parakeet::transcribe_samples(&model_owned, &model_dir_owned, &samples_i16)
        }));
        let _ = tx.send(result);
    });

    // 60 сек — graph optimization на 622 MB encoder на медленном CPU + AV
    // может занять до минуты на первой загрузке. После этого модель кэшируется.
    match rx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(Ok(Ok(text))) => Ok(text),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Err(_)) => Err(anyhow!("parakeet panicked — см. voicy_panic.log")),
        Err(_) => Err(anyhow!("parakeet timeout 60s")),
    }
}

fn transcribe_wav_whisper(wav: &Path, model: &str, language: &str) -> Result<String> {
    let cli = whisper_cli_path();
    if !cli.exists() {
        return Err(anyhow!("whisper-cli не установлен. Запусти `voicy setup`"));
    }
    let model_p = model_path(model);
    if !model_p.exists() {
        return Err(anyhow!(
            "модель {} не скачана. Запусти `voicy model download {}`",
            model,
            model
        ));
    }
    let t0 = std::time::Instant::now();
    // Параметры на качество:
    //   -bs 5    beam search size = 5 (вместо greedy bs=1) — заметно лучше
    //                 точность ценой ~30% времени.
    //   -bo 2    best-of 2 при temperature fallback — ещё немного точности.
    //   -nf      no fallback (отключаем fallback на temperature>0, чтобы не
    //                 выдавал галюны при низком confidence).
    //   -et 2.4  entropy threshold чуть выше дефолта — режет «нет речи».
    //   -lpt -1  logprob threshold — отбрасывает совсем неуверенные фразы.
    let out = Command::new(&cli)
        .arg("-m").arg(&model_p)
        .arg("-l").arg(language)
        .arg("-bs").arg("5")
        .arg("-bo").arg("2")
        .arg("-et").arg("2.4")
        .arg("-lpt").arg("-1.0")
        .arg("-otxt")
        .arg("-of").arg(wav.with_extension(""))
        .arg("-np")
        .arg(wav)
        .output()
        .context("run whisper-cli")?;
    if !out.status.success() {
        warn!(
            "[asr] whisper-cli stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return Err(anyhow!("whisper-cli rc={:?}", out.status.code()));
    }
    let txt_path = wav.with_extension("txt");
    let text = std::fs::read_to_string(&txt_path)
        .with_context(|| format!("read {}", txt_path.display()))?;
    let _ = std::fs::remove_file(&txt_path);
    let dt = t0.elapsed();
    info!("[asr] transcribed in {:.2}s", dt.as_secs_f32());
    Ok(normalize_cyrillic(text.trim()))
}

/// Заменяет латинские look-alike-буквы на кириллические.
/// Whisper иногда выдаёт `napishi mashe` латиницей вместо `напиши маше` —
/// после этой замены контакт находится. Портировано из dev-ветки Python.
pub fn normalize_cyrillic(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'a' => 'а', 'e' => 'е', 'o' => 'о', 'p' => 'р', 'c' => 'с',
            'x' => 'х', 'y' => 'у', 'k' => 'к', 'm' => 'м', 'h' => 'н',
            't' => 'т', 'b' => 'в', 'r' => 'г', 'u' => 'у', 'i' => 'и',
            'j' => 'й',
            'A' => 'А', 'E' => 'Е', 'O' => 'О', 'P' => 'Р', 'C' => 'С',
            'X' => 'Х', 'Y' => 'У', 'K' => 'К', 'M' => 'М', 'H' => 'Н',
            'T' => 'Т', 'B' => 'В', 'R' => 'Г', 'U' => 'У', 'I' => 'И',
            'J' => 'Й',
            other => other,
        })
        .collect()
}
