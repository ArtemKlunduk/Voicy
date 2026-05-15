use anyhow::{anyhow, Result};
use std::io::Cursor;

/// Озвучивает текст. Порядок: Windows OneCore → Google Translate TTS fallback.
pub fn speak(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let text = if text.len() > 4000 {
        &text[..4000]
    } else {
        text
    };

    // Пробуем Windows OneCore
    if let Ok(()) = speak_windows(text) {
        return Ok(());
    }

    // Fallback: Google Translate TTS
    speak_gtts(text)
}

// ── Windows OneCore SpeechSynthesizer ──
#[cfg(windows)]
fn speak_windows(text: &str) -> Result<()> {
    let synth = windows::Media::SpeechSynthesis::SpeechSynthesizer::new()
        .map_err(|e| anyhow!("SpeechSynthesizer: {:?}", e))?;

    let voices = windows::Media::SpeechSynthesis::SpeechSynthesizer::AllVoices()
        .map_err(|e| anyhow!("AllVoices: {:?}", e))?;

    let ru_voice = voices
        .into_iter()
        .find(|v| {
            v.Language()
                .map(|l| l.to_string().starts_with("ru"))
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("No Russian voice found"))?;

    synth
        .SetVoice(&ru_voice)
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
fn speak_windows(_text: &str) -> Result<()> {
    Err(anyhow!("Windows TTS not available"))
}

// ── Google Translate TTS fallback ──
fn speak_gtts(text: &str) -> Result<()> {
    tracing::info!("[tts] using Google Translate TTS fallback");

    // Google Translate TTS endpoint (неофициальный, но стабильный)
    let encoded = urlencoding::encode(text);
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&q={}&tl=ru&client=tw-ob&ttsspeed=1",
        encoded
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

/// Проверяет, доступен ли русский голос в системе.
pub fn is_russian_voice_available() -> bool {
    if let Ok(voices) = windows::Media::SpeechSynthesis::SpeechSynthesizer::AllVoices() {
        voices.into_iter().any(|v| {
            v.Language()
                .map(|l| l.to_string().starts_with("ru"))
                .unwrap_or(false)
        })
    } else {
        false
    }
}
