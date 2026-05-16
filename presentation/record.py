#!/usr/bin/env python
"""
Записать MP4-видео прокрутки Voicy launch-презентации через Playwright.

Запуск:
    cd D:/claude/voicy_rs/presentation
    python -m http.server 5188 &
    python record.py

Результат: voicy-launch.mp4 (1920x1080, ~25 сек).
"""

import asyncio
import os
import shutil
from pathlib import Path

from playwright.async_api import async_playwright

ROOT = Path(__file__).parent
URL = "http://localhost:5188/index.html"
VIDEO_DIR = ROOT / "_video_raw"
OUT_MP4 = ROOT / "voicy-launch.mp4"
W, H = 1920, 1080

# Сценарий: scrollTop в каждый момент времени.
# Длина прокрутки = 9200px документа. Делаем плавный ease-out за 25 секунд.
DURATION_SECS = 25
FPS = 30


async def main() -> None:
    if VIDEO_DIR.exists():
        shutil.rmtree(VIDEO_DIR)
    VIDEO_DIR.mkdir()

    async with async_playwright() as pw:
        browser = await pw.chromium.launch(
            headless=True,
            args=[
                "--disable-gpu-vsync",
                "--enable-precise-memory-info",
                "--hide-scrollbars",
            ],
        )
        context = await browser.new_context(
            viewport={"width": W, "height": H},
            device_scale_factor=1,
            record_video_dir=str(VIDEO_DIR),
            record_video_size={"width": W, "height": H},
        )
        page = await context.new_page()
        await page.goto(URL, wait_until="networkidle")
        await page.wait_for_timeout(800)

        # Сразу включаем reveal-классы чтобы анимации не блокировали кадры
        await page.evaluate("""
            document.querySelectorAll('.reveal').forEach(el => el.classList.add('is-in'));
            // плавный scroll-behavior
            document.documentElement.style.scrollBehavior = 'auto';
        """)

        # Пауза на hero
        await page.wait_for_timeout(2000)

        # Плавный скролл — easeInOutCubic от 0 до scrollHeight - innerHeight за DURATION_SECS
        max_scroll = await page.evaluate(
            "() => document.documentElement.scrollHeight - window.innerHeight"
        )
        total_frames = DURATION_SECS * FPS
        frame_ms = int(1000 / FPS)

        for i in range(total_frames):
            t = i / (total_frames - 1)  # 0..1
            # easeInOutCubic
            ease = 4 * t * t * t if t < 0.5 else 1 - pow(-2 * t + 2, 3) / 2
            scroll_y = int(ease * max_scroll)
            await page.evaluate(f"window.scrollTo(0, {scroll_y})")
            await page.wait_for_timeout(frame_ms)

        # Финальная пауза на CTA
        await page.wait_for_timeout(2000)

        await context.close()
        await browser.close()

    # Найти первый .webm в VIDEO_DIR (playwright всегда webm), сконвертить в mp4
    webm_files = list(VIDEO_DIR.glob("*.webm"))
    if not webm_files:
        print("ERROR: no webm video was created")
        return
    webm = webm_files[0]
    print(f"Raw video: {webm} ({webm.stat().st_size / 1024 / 1024:.1f} MB)")

    # Convert to MP4 via ffmpeg
    import subprocess
    cmd = [
        "ffmpeg", "-y",
        "-i", str(webm),
        "-c:v", "libx264", "-preset", "slow", "-crf", "20",
        "-pix_fmt", "yuv420p",
        "-movflags", "+faststart",
        str(OUT_MP4),
    ]
    print(f"ffmpeg encode -> {OUT_MP4}")
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        print("ffmpeg stderr:", result.stderr[-500:])
        return
    print(f"OK MP4 ready: {OUT_MP4} ({OUT_MP4.stat().st_size / 1024 / 1024:.1f} MB)")

    # Cleanup raw webm
    shutil.rmtree(VIDEO_DIR)


if __name__ == "__main__":
    asyncio.run(main())
