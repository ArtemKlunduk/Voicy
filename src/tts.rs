use anyhow::{anyhow, Result};
use std::io::Cursor;
use std::process::{Command, Stdio};

/// Озвучивает текст.
/// Приоритет: Edge TTS (Azure neural, ~натуральный) → Windows OneCore (SAPI,
/// «телефонный») → Google Translate TTS fallback (network).
///
/// `lang`: "ru" | "en" — определяет какой голос выбрать.
pub fn speak(text: &str) -> Result<()> {
    speak_with_lang(text, "ru")
}

pub fn speak_with_lang(text: &str, lang: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let text = if text.len() > 4000 { &text[..4000] } else { text };

    // 1) Edge TTS (Azure neural) — высокое качество, нужен python+edge-tts
    if let Ok(()) = speak_edge_tts(text, lang) {
        return Ok(());
    }

    // 2) Windows OneCore (SAPI 5 локально, всегда есть)
    if let Ok(()) = speak_windows(text, lang) {
        return Ok(());
    }

    // 3) Google Translate TTS (network fallback)
    speak_gtts(text, lang)
}

// ── Edge TTS via Python edge-tts ─────────────────────────────────────────
fn speak_edge_tts(text: &str, lang: &str) -> Result<()> {
    let voice = match lang {
        "en" => "en-US-AvaMultilingualNeural",
        _ => "ru-RU-SvetlanaNeural",  // ru default — женский, тёплый
    };

    // Скрипт лежит в scripts/edge_tts_speak.py относительно exe parent.
    // На dev машине exe в D:\rust\target_voicy\..., скрипт в D:\claude\voicy_rs\scripts\.
    // Поэтому ищем в нескольких местах.
    let script = locate_edge_script()?;

    let mut cmd = Command::new("python");
    cmd.arg(&script)
        .arg(voice)
        .arg(text)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // CREATE_NO_WINDOW = 0x08000000 — без флага Python subprocess открывает
    // мигающее cmd-окно при каждом TTS вызове (раздражает в работе).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd
        .output()
        .map_err(|e| anyhow!("python spawn: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("edge_tts.py exit {}: {}", output.status, err.trim()));
    }
    if output.stdout.len() < 100 {
        return Err(anyhow!("edge_tts.py returned empty audio"));
    }
    tracing::info!("[tts] edge-tts ok: {} bytes ({})", output.stdout.len(), voice);
    play_audio(&output.stdout)
}

fn locate_edge_script() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe().map_err(|e| anyhow!("current_exe: {}", e))?;
    let exe_dir = exe.parent().ok_or_else(|| anyhow!("exe has no parent"))?;
    let candidates = [
        exe_dir.join("scripts").join("edge_tts_speak.py"),
        exe_dir.join("edge_tts_speak.py"),
        // dev-машина: src в репо
        std::path::PathBuf::from("D:/claude/voicy_rs/scripts/edge_tts_speak.py"),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(anyhow!("edge_tts_speak.py not found in {:?}", candidates))
}

// ── Windows OneCore SpeechSynthesizer ──
#[cfg(windows)]
fn speak_windows(text: &str, lang: &str) -> Result<()> {
    let synth = windows::Media::SpeechSynthesis::SpeechSynthesizer::new()
        .map_err(|e| anyhow!("SpeechSynthesizer: {:?}", e))?;

    let voices = windows::Media::SpeechSynthesis::SpeechSynthesizer::AllVoices()
        .map_err(|e| anyhow!("AllVoices: {:?}", e))?;

    let lang_prefix = if lang.trim().to_lowercase().starts_with("ru") {
        "ru"
    } else {
        "en"
    };

    let voice = voices
        .into_iter()
        .find(|v| {
            v.Language()
                .map(|l| l.to_string().starts_with(lang_prefix))
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("No {} voice found", lang_prefix))?;

    synth
        .SetVoice(&voice)
        .map_err(|e| anyhow!("SetVoice: {:?}", e))?;

    let htext = windows::core::HSTRING::from(text);

    let stream = synth
        .SynthesizeTextToStreamAsync(&htext)
        .map_err(|e| anyhow!("SynthesizeTextToStreamAsync: {:?}", e))?
        .get()
        .map_err(|e| anyhow!("Synthesis: {:?}", e))?;

    // Создаём DataReader из потока
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream)
        .map_err(|e| anyhow!("CreateDataReader: {:?}", e))?;

    // ВАЖНО: загружаем данные из потока в DataReader перед чтением
    let stream_size = stream
        .Size()
        .map_err(|e| anyhow!("stream.Size: {:?}", e))?;
    let load_op = reader
        .LoadAsync(stream_size as u32)
        .map_err(|e| anyhow!("LoadAsync: {:?}", e))?;
    load_op.get().map_err(|e| anyhow!("LoadAsync get: {:?}", e))?;

    let mut audio_buffer = Vec::new();
    loop {
        let available = reader
            .UnconsumedBufferLength()
            .map_err(|e| anyhow!("UnconsumedBufferLength: {:?}", e))?;
        if available == 0 {
            break;
        }
        let mut chunk = vec![0u8; available as usize];
        reader
            .ReadBytes(&mut chunk)
            .map_err(|e| anyhow!("ReadBytes: {:?}", e))?;
        audio_buffer.extend_from_slice(&chunk);
    }

    if audio_buffer.is_empty() {
        return Err(anyhow!("Empty audio buffer from TTS"));
    }

    play_audio(&audio_buffer)
}

#[cfg(not(windows))]
fn speak_windows(_text: &str, _lang: &str) -> Result<()> {
    Err(anyhow!("Windows TTS not available"))
}

// ── Google Translate TTS fallback ──
fn speak_gtts(text: &str, lang: &str) -> Result<()> {
    tracing::info!("[tts] using Google Translate TTS fallback");

    let tl = if lang.trim().to_lowercase().starts_with("ru") {
        "ru"
    } else {
        "en"
    };

    // Google Translate TTS endpoint (неофициальный, но стабильный)
    let encoded = urlencoding::encode(text);
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&q={}&tl={}&client=tw-ob&ttsspeed=1",
        encoded, tl
    );

    let resp = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .call()
        .map_err(|e| anyhow!("gTTS request: {}", e))?;

    let mut audio_buffer = Vec::new();
    resp.into_reader()
        .read_to_end(&mut audio_buffer)
        .map_err(|e| anyhow!("gTTS read: {}", e))?;

    if audio_buffer.len() < 100 {
        return Err(anyhow!("gTTS returned empty/too small audio"));
    }

    play_audio(&audio_buffer)
}

// ── Воспроизведение через rodio ──
fn play_audio(audio_buffer: &[u8]) -> Result<()> {
    let (_stream, stream_handle) = rodio::OutputStream::try_default()
        .map_err(|e| anyhow!("OutputStream: {}", e))?;
    let sink = rodio::Sink::try_new(&stream_handle)
        .map_err(|e| anyhow!("Sink: {}", e))?;

    let cursor = Cursor::new(audio_buffer.to_vec());
    let source = rodio::Decoder::new(cursor)
        .map_err(|e| anyhow!("Decoder: {}", e))?;

    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

/// Проверяет, доступен ли голос указанного языка в системе.
pub fn is_voice_available(lang: &str) -> bool {
    #[cfg(windows)]
    {
        if let Ok(voices) = windows::Media::SpeechSynthesis::SpeechSynthesizer::AllVoices() {
            let prefix = if lang.trim().to_lowercase().starts_with("ru") { "ru" } else { "en" };
            voices.into_iter().any(|v| {
                v.Language()
                    .map(|l| l.to_string().starts_with(prefix))
                    .unwrap_or(false)
            })
        } else {
            false
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[allow(dead_code)]
pub fn is_russian_voice_available() -> bool {
    is_voice_available("ru")
}
