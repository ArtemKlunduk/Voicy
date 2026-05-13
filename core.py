import logging
import os
import wave
import threading
import gc

import numpy as np
import sounddevice as sd

from config import (
    AUDIO_FILE,
    SAMPLE_RATE,
    WHISPER_MODEL,
    RECOGNITION_PROVIDER,
    OPENAI_API_KEY,
    WHISPER_UNLOAD_TIMEOUT,
    WHISPER_KEEP_LOADED,
    load_contacts,
)

logger = logging.getLogger(__name__)


class VoiceProcessor:
    def __init__(self):
        self.provider = RECOGNITION_PROVIDER.lower().strip()
        self._model = None
        self._client = None
        self._unload_timer = None
        self._model_lock = threading.Lock()
        self._use_count = 0
        self.contacts = load_contacts()
        logger.info(f"Провайдер распознавания: {self.provider}")

        if self.provider == "openai":
            if not OPENAI_API_KEY:
                raise RuntimeError(
                    "OPENAI_API_KEY не задан. Получи ключ на https://platform.openai.com/api-keys"
                )
            try:
                from openai import OpenAI

                self._client = OpenAI(api_key=OPENAI_API_KEY)
                logger.info("OpenAI клиент готов.")
            except ImportError:
                raise RuntimeError("Установи библиотеку: pip install openai")
        elif self.provider != "local":
            raise RuntimeError(f"Неизвестный провайдер: {self.provider}")

        logger.info(f"Контактов загружено: {len(self.contacts)}")

    # ── Локальная модель: загрузка / выгрузка ──

    def _load_model(self):
        from faster_whisper import WhisperModel

        logger.info(f"Загрузка faster-whisper ({WHISPER_MODEL}, int8)…")
        self._model = WhisperModel(
            WHISPER_MODEL,
            device="cpu",
            compute_type="int8",
        )
        logger.info("Модель загружена.")

    def _unload_model(self):
        logger.info("Выгрузка модели для освобождения RAM…")
        self._model = None
        gc.collect()
        logger.info("Модель выгружена. RAM освобождена.")

    def _do_unload(self):
        with self._model_lock:
            if self._use_count > 0:
                logger.debug("Модель всё ещё используется, откладываю выгрузку…")
                self._schedule_unload()
                return
            if self._model is not None:
                self._unload_model()

    def _schedule_unload(self):
        if WHISPER_KEEP_LOADED:
            return
        if self._unload_timer is not None:
            self._unload_timer.cancel()
        if WHISPER_UNLOAD_TIMEOUT > 0:
            self._unload_timer = threading.Timer(
                WHISPER_UNLOAD_TIMEOUT, self._do_unload
            )
            self._unload_timer.daemon = True
            self._unload_timer.start()

    def ensure_model_loaded(self):
        with self._model_lock:
            if self._model is None:
                self._load_model()
            self._use_count += 1

    def release_model(self):
        with self._model_lock:
            self._use_count -= 1
            if self._use_count <= 0 and not WHISPER_KEEP_LOADED:
                self._schedule_unload()

    # ── Запись и распознавание ──

    def record_audio(self, stop_event):
        """Записывает аудио с микрофона пока stop_event не установлен."""
        frames = []

        def callback(indata, frame_count, time_info, status):
            frames.append(indata.copy())

        stream = sd.InputStream(
            samplerate=SAMPLE_RATE,
            channels=1,
            dtype=np.int16,
            callback=callback,
        )
        stream.start()
        while not stop_event.is_set():
            sd.sleep(50)
        stream.stop()
        stream.close()

        if not frames:
            return None

        recording = np.concatenate(frames, axis=0)
        with wave.open(AUDIO_FILE, "wb") as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)
            wf.setframerate(SAMPLE_RATE)
            wf.writeframes(recording.tobytes())
        return AUDIO_FILE

    def transcribe(self, audio_path):
        """Распознаёт речь через выбранный провайдер."""
        if not os.path.exists(audio_path):
            return ""

        if self.provider == "local":
            self.ensure_model_loaded()
            try:
                segments, info = self._model.transcribe(
                    audio_path, language="ru", beam_size=5
                )
                text = " ".join(segment.text.strip() for segment in segments)
                return text.strip()
            finally:
                self.release_model()

        if self.provider == "openai":
            with open(audio_path, "rb") as audio_file:
                response = self._client.audio.transcriptions.create(
                    model="whisper-1",
                    file=audio_file,
                    language="ru",
                )
            return response.text.strip()

        return ""

    @staticmethod
    def _normalize_cyrillic(text):
        """Заменяет похожие латинские буквы на кириллические.
        Whisper иногда выдаёт латинские look-alike символы."""
        table = str.maketrans({
            'a': 'а', 'e': 'е', 'o': 'о', 'p': 'р', 'c': 'с',
            'x': 'х', 'y': 'у', 'k': 'к', 'm': 'м', 'h': 'н',
            't': 'т', 'b': 'в', 'r': 'г', 'u': 'у', 'i': 'и',
            'j': 'й',
        })
        return text.translate(table)

    def parse_command(self, text):
        """Парсит текст команды. Возвращает (action, data, error)."""
        text = text.lower().strip()
        text = text.rstrip(".!?,:;")
        text = self._normalize_cyrillic(text)

        if text.startswith("напиши"):
            return self._parse_send(text[len("напиши"):].strip())
        elif text.startswith("открой"):
            return self._parse_open(text[len("открой"):].strip())
        elif text.startswith("дай ответ"):
            return self._parse_ai_answer(text[len("дай ответ"):].strip())
        elif text.startswith("стоп"):
            return "stop", {}, None
        else:
            return None, None, "Команда не распознана (ожидалось 'напиши …', 'открой …', 'дай ответ …' или 'стоп')"

    def _parse_send(self, rest):
        if not rest:
            return None, None, "Имя получателя не указано"
        rest = self._normalize_cyrillic(rest)
        parts = rest.split(None, 1)
        name = parts[0].strip()
        message = parts[1].strip() if len(parts) > 1 else ""

        user_id = self.contacts.get(name)
        if not user_id:
            for alias, uid in self.contacts.items():
                if alias.startswith(name) or name.startswith(alias):
                    user_id = uid
                    break

        if not user_id:
            return None, None, f"Контакт '{name}' не найден"

        return "send_message", {"user_id": user_id, "message": message}, None

    # Карта голосовых команд → URL. Легко расширять.
    OPEN_COMMANDS = {
        "браузер": "about:blank",
        "ютуб": "https://www.youtube.com",
        "youtube": "https://www.youtube.com",
        "уoutube": "https://www.youtube.com",
        "уоutube": "https://www.youtube.com",
        "уоутуве": "https://www.youtube.com",
        "гугл": "https://www.google.com",
        "google": "https://www.google.com",
        "gоoglе": "https://www.google.com",
        "вк": "https://vk.com",
        "вконтакте": "https://vk.com",
        "vk": "https://vk.com",
        "яндекс": "https://yandex.ru",
        "yandex": "https://yandex.ru",
        "mail": "https://mail.ru",
        "мейл": "https://mail.ru",
        "telegram": "https://web.telegram.org",
        "телеграм": "https://web.telegram.org",
        "github": "https://github.com",
        "githuв": "https://github.com",
        "гитхаб": "https://github.com",
    }

    def _parse_open(self, rest):
        if not rest:
            return None, None, "Что открыть? (например: 'открой ютуб')"
        target = rest.strip()
        url = self.OPEN_COMMANDS.get(target)
        if url:
            return "open_browser", {"url": url}, None
        return None, None, f"Неизвестная команда: '{target}'"

    def _parse_ai_answer(self, rest):
        if not rest:
            return None, None, "Вопрос не задан (скажи: 'дай отвер что такое дефибрилятор')"
        return "ai_answer", {"question": rest}, None
