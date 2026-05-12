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
            if self._use_count <= 0:
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

    def parse_command(self, text):
        """Парсит текст команды. Возвращает (user_id, message, error)."""
        text = text.lower().strip()
        text = text.rstrip(".!?,:;")

        if not text.startswith("напиши"):
            return None, None, "Команда не распознана (ожидалось 'напиши …')"

        rest = text[len("напиши"):].strip()
        if not rest:
            return None, None, "Имя получателя не указано"

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

        return user_id, message, None
