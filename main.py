import os
import sys
import queue
import threading
import logging

from pynput import keyboard

from config import AUDIO_FILE
from overlay import Overlay
from core import VoiceProcessor
from telegram_sender import TelegramSender
from ai_assistant import AIAssistant

# ── Логирование в файл (важно для фонового режима pythonw) ──
LOG_PATH = os.path.join(os.path.dirname(__file__), "app.log")
logging.basicConfig(
    filename=LOG_PATH,
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    encoding="utf-8",
)
# Дублируем логи и в консоль, если она есть
if sys.stdout is not None:
    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(logging.Formatter("%(asctime)s [%(levelname)s] %(message)s"))
    logging.getLogger().addHandler(handler)

logger = logging.getLogger(__name__)


class VoiceHelper:
    def __init__(self):
        self.overlay = None
        self.voice = VoiceProcessor()
        self.telegram = TelegramSender()
        self.ai = AIAssistant()
        self.recording = False
        self.stop_event = threading.Event()
        self._cancel_event = threading.Event()
        self.alt_pressed = False
        self._hotkey_pressed = False
        self._task_queue = queue.Queue()

    def start(self):
        self.overlay = Overlay()
        listener = keyboard.Listener(
            on_press=self._on_press, on_release=self._on_release
        )
        listener.start()
        logger.info(f"Keyboard listener started, alive={listener.is_alive()}")
        self.overlay.schedule(self._process_events)
        # Периодическая проверка что listener не умер
        self.overlay.schedule(lambda: self._check_listener(listener))

        logger.info("=== Голосовой помощник запущен ===")
        logger.info("Авторизуюсь в Telegram…")
        try:
            self.telegram.start()
        except Exception as e:
            logger.error(f"Не удалось запустить Telegram: {e}")
            listener.stop()
            return

        logger.info("Telegram готов. Зажмите Alt + X и говорите команду.")
        self.overlay.run()
        listener.stop()
        self.telegram.stop()

    def _check_listener(self, listener):
        if not listener.is_alive():
            logger.error("Keyboard listener died!")
        else:
            self.overlay.after(5000, lambda: self._check_listener(listener))

    def _process_events(self):
        try:
            while True:
                task = self._task_queue.get_nowait()
                task()
        except queue.Empty:
            pass
        self.overlay.after(50, self._process_events)

    def _on_press(self, key):
        try:
            vk = getattr(key, "vk", None)
            if key in (keyboard.Key.alt_l, keyboard.Key.alt_r):
                self.alt_pressed = True
                logger.info("Alt pressed")
            elif self.alt_pressed and vk == 88:
                logger.info("Hotkey Alt+X pressed — starting recording")
                if not self.recording and not self._hotkey_pressed:
                    self._hotkey_pressed = True
                    self._task_queue.put(self._start_recording)
            else:
                logger.debug(f"Key press ignored: {key}, vk={vk}")
        except Exception as e:
            logger.error(f"Exception in _on_press: {e}")

    def _on_release(self, key):
        try:
            vk = getattr(key, "vk", None)
            if key in (keyboard.Key.alt_l, keyboard.Key.alt_r):
                self.alt_pressed = False
                logger.info("Alt released")
                if self.recording:
                    self._task_queue.put(self._stop_recording)
            elif vk == 88:
                logger.info("X released")
                if self.recording:
                    self._task_queue.put(self._stop_recording)
        except Exception as e:
            logger.error(f"Exception in _on_release: {e}")

    def _start_recording(self):
        self.recording = True
        self.stop_event.clear()
        self.overlay.animate_equalizer()
        threading.Thread(target=self._record_worker, daemon=True).start()

    def _stop_recording(self):
        self.recording = False
        self._hotkey_pressed = False
        self.stop_event.set()

    def _record_worker(self):
        audio_path = self.voice.record_audio(self.stop_event)

        def on_record_done():
            self.overlay.clear()
            if not audio_path:
                self.overlay.show_error()
                return
            threading.Thread(
                target=self._process_worker, args=(audio_path,), daemon=True
            ).start()

        self.overlay.schedule(on_record_done)

    def _process_worker(self, audio_path):
        try:
            text = self.voice.transcribe(audio_path)
        except Exception as e:
            logger.error(f"[Ошибка распознавания] {e}")
            self.overlay.schedule(lambda: (self.overlay.show_error(), self._cleanup(audio_path)))
            return

        logger.info(f"[Распознано] {text}")

        def on_parse():
            if not text:
                self.overlay.show_error()
                self._cleanup(audio_path)
                return

            action, data, error = self.voice.parse_command(text)
            if error:
                logger.warning(f"[Ошибка парсинга] {error}")
                self.overlay.show_error()
                self._cleanup(audio_path)
                return

            if action == "send_message":
                user_id = data["user_id"]
                message = data["message"]
                if not message:
                    logger.warning("[Ошибка] Текст сообщения пустой")
                    self.overlay.show_error()
                    self._cleanup(audio_path)
                    return
                logger.info(f"[Отправка] -> {user_id}: {message}")
                self._send_worker(user_id, message, audio_path)
            elif action == "open_browser":
                url = data.get("url")
                logger.info(f"[Команда] Открыть браузер: {url or 'about:blank'}")
                self._open_browser(audio_path, url=url)
            elif action == "ai_answer":
                question = data["question"]
                logger.info(f"[AI Вопрос] {question}")
                threading.Thread(
                    target=self._ai_answer_worker, args=(question, audio_path), daemon=True
                ).start()
            elif action == "stop":
                logger.info("[Команда] Стоп")
                self._stop_all(audio_path)

        self.overlay.schedule(on_parse)

    def _send_worker(self, user_id, message, audio_path):
        try:
            ok = self.telegram.send_message(user_id, message)
        except Exception as e:
            logger.error(f"[Ошибка отправки] {e}")
            ok = False

        def on_sent():
            if ok:
                self.overlay.show_success()
            else:
                self.overlay.show_error()
            self._cleanup(audio_path)

        self.overlay.schedule(on_sent)

    def _open_browser(self, audio_path, url=None):
        try:
            target_url = url or "about:blank"
            browser_path = self._get_default_browser_path()
            if browser_path:
                subprocess.Popen([browser_path, target_url], shell=False)
                logger.info(f"Браузер открыт: {browser_path} -> {target_url}")
            else:
                import webbrowser
                webbrowser.open(target_url, new=2)
                logger.info(f"Браузер открыт через fallback -> {target_url}")
            self.overlay.show_success()
        except Exception as e:
            logger.error(f"[Ошибка] Не удалось открыть браузер: {e}")
            self.overlay.show_error()
        finally:
            self._cleanup(audio_path)

    def _ai_answer_worker(self, question, audio_path):
        self._cancel_event.clear()
        try:
            self.overlay.schedule(lambda: self.overlay.animate_equalizer())
            answer = self.ai.ask(question)
            if self._cancel_event.is_set():
                logger.info("[AI] Отменено пользователем до озвучки")
                return
            logger.info(f"[AI Ответ] {answer}")
            self.overlay.schedule(self.overlay.clear)
            mp3_path = self.ai.speak(answer)
            if self._cancel_event.is_set():
                logger.info("[AI] Отменено пользователем до воспроизведения")
                return
            logger.info(f"[AI TTS] Сохранено: {mp3_path}")
            self.ai.play_audio(mp3_path)
            if self._cancel_event.is_set():
                logger.info("[AI] Отменено пользователем после воспроизведения")
                return
            self.overlay.schedule(self.overlay.show_success)
        except Exception as e:
            if self._cancel_event.is_set():
                logger.info("[AI] Операция прервана")
            else:
                logger.error(f"[Ошибка AI] {e}")
                self.overlay.schedule(self.overlay.show_error)
        finally:
            self._cleanup(audio_path)

    @staticmethod
    def _get_default_browser_path():
        """Возвращает путь к браузеру по умолчанию из реестра Windows."""
        try:
            import winreg
            import re
            with winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, r"http\shell\open\command") as key:
                command, _ = winreg.QueryValueEx(key, None)
            # Команда вида: "C:\...\chrome.exe" --single-argument %1
            match = re.search(r'"([^"]+)"', command)
            if match:
                path = match.group(1)
                if os.path.exists(path):
                    return path
            # Fallback: первый токен до пробела
            first = command.split()[0].strip('"')
            if os.path.exists(first):
                return first
        except Exception:
            pass
        return None

    def _stop_all(self, audio_path=None):
        """Мгновенно останавливает текущий AI-ответ и воспроизведение."""
        logger.info("[Стоп] Отмена текущей операции")
        self._cancel_event.set()
        try:
            import pygame
            if pygame.mixer.get_init():
                pygame.mixer.music.stop()
        except Exception:
            pass
        self.overlay.schedule(self.overlay.clear)
        self.overlay.schedule(self.overlay.hide)
        if audio_path:
            self._cleanup(audio_path)

    @staticmethod
    def _cleanup(audio_path):
        try:
            if audio_path and os.path.exists(audio_path):
                os.remove(audio_path)
        except Exception:
            pass


def _process_exists(pid, name="pythonw.exe"):
    try:
        import subprocess
        output = subprocess.check_output(
            f'tasklist /FI "PID eq {pid}" /NH /FO CSV',
            shell=True,
            stderr=subprocess.DEVNULL,
        ).decode("cp866", errors="ignore")
        return name.lower() in output.lower() and str(pid) in output
    except (subprocess.CalledProcessError, OSError):
        return False


def _acquire_lock():
    lock_path = os.path.join(os.path.dirname(__file__), ".voice_helper.lock")
    current_pid = os.getpid()
    if os.path.exists(lock_path):
        try:
            with open(lock_path, "r", encoding="utf-8") as f:
                old_pid = int(f.read().strip())
            if _process_exists(old_pid):
                return None, old_pid
        except (ValueError, OSError):
            pass
        try:
            os.remove(lock_path)
        except OSError:
            pass
    with open(lock_path, "w", encoding="utf-8") as f:
        f.write(str(current_pid))
    return lock_path, None


def _release_lock(lock_path):
    if lock_path and os.path.exists(lock_path):
        try:
            os.remove(lock_path)
        except OSError:
            pass


if __name__ == "__main__":
    lock_file, other_pid = _acquire_lock()
    if lock_file is None:
        import tkinter.messagebox as mb
        mb.showwarning(
            "Voice TG Helper",
            f"Скрипт уже запущен (PID {other_pid}).\n"
            "Найди процесс pythonw.exe в Диспетчере задач и заверши его, "
            "если хочешь перезапустить."
        )
        sys.exit(1)

    app = VoiceHelper()
    try:
        app.start()
    except KeyboardInterrupt:
        logger.info("Выход по Ctrl+C")
    finally:
        _release_lock(lock_file)
        sys.exit(0)
