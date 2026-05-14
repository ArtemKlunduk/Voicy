//! Запись с микрофона через cpal. Stop по сигналу, сохранение в 16kHz mono WAV.
//! Whisper.cpp ожидает 16kHz mono — мы конвертим прямо при записи.

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

pub const TARGET_RATE: u32 = 16_000;

/// Записать аудио пока `stop` не выставлен в `true`.
/// Возвращает (samples_16k_mono_i16, sample_rate=16000).
pub fn record(stop: Arc<AtomicBool>) -> Result<Vec<i16>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("default input device not found"))?;
    let name = device.name().unwrap_or_else(|_| "<unknown>".into());

    // Пытаемся попросить 16kHz mono. Если не выйдет — берём дефолт и ресемплим.
    let supported = device
        .default_input_config()
        .context("default_input_config")?;
    let src_rate = supported.sample_rate().0;
    let src_channels = supported.channels();
    let sample_format = supported.sample_format();
    info!(
        "[audio] device='{}' rate={} ch={} fmt={:?}",
        name, src_rate, src_channels, sample_format
    );

    let config = cpal::StreamConfig {
        channels: src_channels,
        sample_rate: cpal::SampleRate(src_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let buf = Arc::new(Mutex::new(Vec::<i16>::new()));
    let buf_cb = buf.clone();

    let err_cb = |err| warn!("[audio] stream err: {}", err);

    let stream = match sample_format {
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                let mut b = buf_cb.lock();
                fold_to_mono_i16(data, src_channels, &mut b);
            },
            err_cb,
            None,
        )?,
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                let mut b = buf_cb.lock();
                fold_to_mono_f32(data, src_channels, &mut b);
            },
            err_cb,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _| {
                let mut b = buf_cb.lock();
                fold_to_mono_u16(data, src_channels, &mut b);
            },
            err_cb,
            None,
        )?,
        f => return Err(anyhow!("unsupported sample format: {:?}", f)),
    };

    stream.play().context("stream.play")?;
    info!("[audio] recording…");
    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(stream); // остановит поток
    info!("[audio] stopped, raw samples: {}", buf.lock().len());

    let raw = std::mem::take(&mut *buf.lock());
    let resampled = resample_linear(&raw, src_rate, TARGET_RATE);
    info!(
        "[audio] resampled {}→{} ({} samples, ~{:.1}s)",
        src_rate,
        TARGET_RATE,
        resampled.len(),
        resampled.len() as f32 / TARGET_RATE as f32
    );
    let normalized = normalize_peak(resampled);
    Ok(normalized)
}

/// Auto-gain: масштабирует samples так, чтобы пик был на 80% от i16::MAX.
/// Whisper-tiny плохо распознаёт тихую речь — нормализация заметно помогает.
/// Не трогает уже громкие записи и шумы (если пик < ~5% полной шкалы — скорее
/// всего тишина, не трогаем чтобы не усилить фоновый гул до неузнаваемости).
pub fn normalize_peak(mut samples: Vec<i16>) -> Vec<i16> {
    if samples.is_empty() {
        return samples;
    }
    let peak: i32 = samples.iter().map(|&s| s.unsigned_abs() as i32).max().unwrap_or(0);
    if peak == 0 {
        return samples;
    }
    let target_peak: i32 = (i16::MAX as i32 * 80) / 100; // 26214
    // Если речь и так громкая — не трогаем (избегаем клиппинга).
    if peak >= target_peak {
        return samples;
    }
    // Слишком тихо (< 5% шкалы) — почти наверняка тишина или плотный фоновый
    // гул, не имеет смысла усиливать в 30+ раз.
    let min_signal: i32 = (i16::MAX as i32 * 5) / 100;
    if peak < min_signal {
        info!("[audio] auto-gain skip: peak={} too quiet (< 5% scale)", peak);
        return samples;
    }
    let gain = target_peak as f32 / peak as f32;
    info!("[audio] auto-gain ×{:.2} (peak {}→{})", gain, peak, target_peak);
    for s in samples.iter_mut() {
        let scaled = (*s as f32) * gain;
        *s = scaled.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
    samples
}

pub fn save_wav(path: &Path, samples: &[i16]) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec)?;
    for &s in samples {
        w.write_sample(s)?;
    }
    w.finalize()?;
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────

fn fold_to_mono_i16(data: &[i16], channels: u16, out: &mut Vec<i16>) {
    if channels <= 1 {
        out.extend_from_slice(data);
        return;
    }
    let n = channels as usize;
    for chunk in data.chunks_exact(n) {
        let sum: i32 = chunk.iter().map(|&s| s as i32).sum();
        out.push((sum / n as i32) as i16);
    }
}

fn fold_to_mono_f32(data: &[f32], channels: u16, out: &mut Vec<i16>) {
    let n = channels.max(1) as usize;
    for chunk in data.chunks_exact(n) {
        let avg: f32 = chunk.iter().sum::<f32>() / n as f32;
        let s = (avg * 32_767.0).clamp(-32_768.0, 32_767.0);
        out.push(s as i16);
    }
}

fn fold_to_mono_u16(data: &[u16], channels: u16, out: &mut Vec<i16>) {
    let n = channels.max(1) as usize;
    for chunk in data.chunks_exact(n) {
        let sum: i64 = chunk.iter().map(|&s| s as i64).sum();
        let avg = sum / n as i64;
        out.push((avg - 32_768) as i16);
    }
}

/// Простой линейный ресемплер. Достаточно для whisper (он терпит лёгкие артефакты).
fn resample_linear(input: &[i16], src: u32, dst: u32) -> Vec<i16> {
    if src == dst || input.is_empty() {
        return input.to_vec();
    }
    let ratio = src as f64 / dst as f64;
    let out_len = ((input.len() as f64) / ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let a = input.get(idx).copied().unwrap_or(0) as f64;
        let b = input.get(idx + 1).copied().unwrap_or(a as i16) as f64;
        out.push((a + (b - a) * frac) as i16);
    }
    out
}
