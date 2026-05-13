# -*- coding: utf-8 -*-
"""AI-ассистент: OpenAI / Pollinations AI для ответов + Edge TTS для озвучки."""

import asyncio
import logging
import os
import time
import tempfile
import httpx
from urllib.parse import quote
from openai import OpenAI
from config import (
    OPENAI_API_KEY,
    AI_MODEL,
    AI_SYSTEM_PROMPT,
    VOICE_API_KEY,
    VOICE_API_BASE,
    VOICE_API_VOICE_ID,
    VOICE_API_MODEL_ID,
)

POLLINATIONS_URL = "https://text.pollinations.ai/"
POLLINATIONS_PROMPT = "Кратко: {question}"

EDGE_TTS_VOICE = "ru-RU-SvetlanaNeural"


class AIAssistant:
    """Голосовой AI-ассистент.

    Работает без настройки из коробки:
      - Текстовые ответы через бесплатный Pollinations AI (без API ключей).
      - Если добавлен OPENAI_API_KEY — используется OpenAI (лучшее качество).
      - Озвучка через Edge TTS (бесплатно, качество нейросетей Microsoft).
      - Fallback: VoiceAPI (ElevenLabs) если есть ключ, иначе Windows SAPI.
    """

    def __init__(self):
        self._openai = OpenAI(api_key=OPENAI_API_KEY) if OPENAI_API_KEY else None
        self._openai_failed = False
        self._voice_api_available = bool(VOICE_API_KEY)
        self._voice_headers = {
            "X-API-Key": VOICE_API_KEY,
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        }

    def ask(self, question: str) -> str:
        """Получает текстовый ответ на вопрос. Сначала Pollinations (быстрее), fallback OpenAI."""
        try:
            return self._ask_pollinations(question)
        except Exception as exc:
            logging.warning(f"[AI] Pollinations недоступен ({exc}), fallback на OpenAI")
        if self._openai and not self._openai_failed:
            try:
                return self._ask_openai(question)
            except Exception as exc:
                if "quota" in str(exc).lower() or "429" in str(exc):
                    self._openai_failed = True
                logging.warning(f"[AI] OpenAI недоступен ({exc})")
        raise RuntimeError("Все AI-сервисы недоступны")

    def _ask_openai(self, question: str) -> str:
        response = self._openai.chat.completions.create(
            model=AI_MODEL,
            messages=[
                {"role": "system", "content": AI_SYSTEM_PROMPT},
                {"role": "user", "content": question},
            ],
            max_tokens=500,
            temperature=0.7,
        )
        return response.choices[0].message.content.strip()

    @staticmethod
    def _ask_pollinations(question: str) -> str:
        prompt = POLLINATIONS_PROMPT.format(question=question)
        url = f"{POLLINATIONS_URL}{quote(prompt)}?model=openai-fast"
        with httpx.Client(timeout=15) as client:
            resp = client.get(url)
            resp.raise_for_status()
            return resp.text.strip()

    @staticmethod
    def _clean_text_for_tts(text: str) -> str:
        """Убирает Markdown, спецсимволы и мусор из текста перед озвучкой."""
        import re

        # Блочные элементы
        text = re.sub(r'^#{1,6}\s*', '', text, flags=re.MULTILINE)
        text = re.sub(r'^\s*[-*+]\s+', '', text, flags=re.MULTILINE)
        text = re.sub(r'^\s*>\s*', '', text, flags=re.MULTILINE)
        text = re.sub(r'^---+\s*$', '', text, flags=re.MULTILINE)
        text = re.sub(r'^\*\*\*+\s*$', '', text, flags=re.MULTILINE)

        # Изображения
        text = re.sub(r'!\[([^\]]*)\]\([^)]*\)', r'\1', text)
        # Ссылки
        text = re.sub(r'\[([^\]]+)\]\([^)]*\)', r'\1', text)
        # Сноски [1], [2] и т.д.
        text = re.sub(r'\[\d+\]', '', text)
        # Жирный / курсив
        text = re.sub(r'\*\*(.*?)\*\*', r'\1', text)
        text = re.sub(r'__(.*?)__', r'\1', text)
        text = re.sub(r'(?<!\w)\*(.*?)\*(?!\w)', r'\1', text)
        text = re.sub(r'(?<!\w)_(.*?)_(?!\w)', r'\1', text)
        # Инлайн-код
        text = re.sub(r'`([^`]+)`', r'\1', text)
        # Зачёркивание
        text = re.sub(r'~~(.*?)~~', r'\1', text)

        # Финальная зачистка — удаляем ВСЕ оставшиеся спецсимволы Markdown
        text = re.sub(r'\*+', '', text)
        text = re.sub(r'_+', '', text)
        text = re.sub(r'`+', '', text)
        text = re.sub(r'~+', '', text)
        text = re.sub(r'\|', '', text)

        # Убираем лишние переводы строк и пробелы
        text = re.sub(r'\n+', ' ', text)
        text = re.sub(r'\s+', ' ', text)
        text = text.strip()
        return text

    def speak(self, text: str) -> str:
        """Синтезирует речь. Возвращает путь к аудио-файлу."""
        original = text
        text = self._clean_text_for_tts(text)
        if text != original:
            logging.info(f"[TTS clean] '{original}' → '{text}'")
        try:
            return self._speak_edge(text)
        except Exception as exc:
            logging.warning(f"[TTS] Edge TTS недоступен ({exc}), fallback")
        if self._voice_api_available:
            return self._speak_voiceapi(text)
        return self._speak_windows(text)

    @staticmethod
    def _speak_edge(text: str) -> str:
        """Основной TTS: Edge TTS (бесплатно, нейросетевые голоса Microsoft)."""
        import edge_tts

        fd, path = tempfile.mkstemp(suffix=".mp3")
        os.close(fd)
        logging.info(f"[_speak_edge] Сохраняю в {path}")

        async def _save():
            communicate = edge_tts.Communicate(text, voice=EDGE_TTS_VOICE)
            await communicate.save(path)

        loop = asyncio.new_event_loop()
        try:
            loop.run_until_complete(_save())
            size = os.path.getsize(path)
            logging.info(f"[_speak_edge] Файл создан, размер {size} байт")
        except Exception as exc:
            logging.error(f"[_speak_edge] Ошибка Edge TTS: {exc}")
            raise
        finally:
            loop.close()
        return path

    def _speak_voiceapi(self, text: str) -> str:
        payload = {
            "text": text,
            "template": {
                "voice_id": VOICE_API_VOICE_ID,
                "model_id": VOICE_API_MODEL_ID,
                "public_owner_id": None,
                "voice_settings": {
                    "stability": 0.85,
                    "similarity_boost": 0.75,
                    "use_speaker_boost": True,
                    "style": 0.0,
                    "speed": 1.0,
                },
            },
        }

        with httpx.Client(timeout=30) as client:
            resp = client.post(
                f"{VOICE_API_BASE}/tasks",
                json=payload,
                headers=self._voice_headers,
            )
            resp.raise_for_status()
            task_id = resp.json()["task_id"]

            for _ in range(60):
                time.sleep(2)
                r = client.get(
                    f"{VOICE_API_BASE}/tasks/{task_id}/status",
                    headers=self._voice_headers,
                )
                r.raise_for_status()
                status = r.json()["status"]
                if status == "ending":
                    break
                if status in ("error", "error_handled"):
                    raise RuntimeError(f"Ошибка синтеза речи: {status}")
            else:
                raise RuntimeError("Таймаут синтеза речи")

            r = client.get(
                f"{VOICE_API_BASE}/tasks/{task_id}/result",
                headers=self._voice_headers,
            )
            r.raise_for_status()
            fd, path = tempfile.mkstemp(suffix=".mp3")
            with os.fdopen(fd, "wb") as f:
                f.write(r.content)
            return path

    @staticmethod
    def _speak_windows(text: str) -> str:
        """Крайний fallback: Windows SAPI с русским голосом Irina."""
        import win32com.client

        speaker = win32com.client.Dispatch("SAPI.SpVoice")
        for voice in speaker.GetVoices():
            if "Irina" in voice.GetDescription():
                speaker.Voice = voice
                break

        fd, path = tempfile.mkstemp(suffix=".wav")
        os.close(fd)

        stream = win32com.client.Dispatch("SAPI.SpFileStream")
        stream.Open(path, 3)
        speaker.AudioOutputStream = stream
        speaker.Speak(text)
        stream.Close()
        return path

    @staticmethod
    def play_audio(path: str):
        """Воспроизводит аудио через pygame mixer (стабильно в фоне)."""
        import time
        logging.info(f"[play_audio] Начинаю воспроизведение: {path}")
        try:
            import pygame
            if pygame.mixer.get_init():
                pygame.mixer.quit()
            pygame.mixer.init()
            logging.info("[play_audio] pygame mixer инициализирован")
            pygame.mixer.music.load(path)
            logging.info("[play_audio] Файл загружен")
            pygame.mixer.music.play()
            logging.info("[play_audio] Воспроизведение начато")
            time.sleep(0.1)
            while pygame.mixer.music.get_busy():
                time.sleep(0.3)
            logging.info("[play_audio] Воспроизведение завершено")
        except Exception as exc:
            logging.error(f"[play_audio] Ошибка pygame: {exc}")
            # Fallback: WMP
            try:
                import win32com.client
                player = win32com.client.Dispatch("WMPlayer.OCX")
                player.URL = os.path.abspath(path)
                player.controls.play()
                while player.playState != 1:
                    time.sleep(0.3)
            except Exception as exc2:
                logging.error(f"[play_audio] Ошибка WMP: {exc2}")
                os.startfile(os.path.abspath(path))
