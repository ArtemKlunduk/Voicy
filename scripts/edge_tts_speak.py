"""Спик через edge-tts → MP3 в stdout. Voicy подхватывает и проигрывает через rodio.

Usage:
    python edge_tts_speak.py "<voice>" "<text>"

Voice list — `edge-tts --list-voices`. Популярные:
    ru-RU-SvetlanaNeural   — женский, тёплый
    ru-RU-DmitryNeural     — мужской, нейтральный
    en-US-AvaMultilingualNeural — женский, поддерживает много языков
    en-US-AndrewNeural     — мужской

Stdout: MP3 bytes (binary). Stderr: ошибки/диагностика.
"""
import asyncio
import sys

import edge_tts


async def main():
    if len(sys.argv) < 3:
        print("usage: edge_tts_speak.py <voice> <text>", file=sys.stderr)
        sys.exit(2)
    voice = sys.argv[1]
    text = sys.argv[2]

    # На Windows stdout по умолчанию текстовый — переключаем в binary
    try:
        sys.stdout.reconfigure(newline="", line_buffering=False)
    except Exception:
        pass

    out = sys.stdout.buffer if hasattr(sys.stdout, "buffer") else sys.stdout

    communicate = edge_tts.Communicate(text, voice)
    async for chunk in communicate.stream():
        if chunk.get("type") == "audio":
            data = chunk.get("data")
            if data:
                out.write(data)
    out.flush()


if __name__ == "__main__":
    asyncio.run(main())
