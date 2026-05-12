import logging
from tkinter import simpledialog, messagebox

from telethon.sync import TelegramClient
from telethon.errors import SessionPasswordNeededError

from config import API_ID, API_HASH, SESSION_NAME

logger = logging.getLogger(__name__)


def _ask_phone():
    return simpledialog.askstring(
        "Telegram — вход",
        "Введите номер телефона:\n(пример: +79991234567)"
    )


def _ask_code():
    return simpledialog.askstring(
        "Telegram — код",
        "Введите код из Telegram:"
    )


def _ask_password():
    return simpledialog.askstring(
        "Telegram — 2FA",
        "Введите пароль двухфакторной аутентификации:",
        show="*"
    )


class TelegramSender:
    def __init__(self):
        self.client = None

    def start(self):
        if API_ID == 0 or not API_HASH:
            messagebox.showerror(
                "Ошибка конфигурации",
                "Заполните API_ID и API_HASH в файле config.py\n\n"
                "Получить их можно на https://my.telegram.org/apps"
            )
            raise RuntimeError("API_ID и API_HASH не настроены")

        self.client = TelegramClient(SESSION_NAME, API_ID, API_HASH)
        self.client.connect()

        if not self.client.is_user_authorized():
            phone = _ask_phone()
            if not phone:
                raise RuntimeError("Номер телефона не введён")

            self.client.send_code_request(phone)
            code = _ask_code()
            if not code:
                raise RuntimeError("Код не введён")

            try:
                self.client.sign_in(phone, code)
            except SessionPasswordNeededError:
                password = _ask_password()
                if not password:
                    raise RuntimeError("Пароль 2FA не введён")
                self.client.sign_in(password=password)

        logger.info("Telegram авторизация успешна")

    def send_message(self, user_id: str, message: str) -> bool:
        if self.client is None:
            raise RuntimeError("TelegramClient не инициализирован")
        try:
            uid = int(user_id)
            self.client.send_message(uid, message)
            logger.info(f"Сообщение отправлено {uid}")
            return True
        except Exception as e:
            logger.error(f"Ошибка отправки: {e}")
            return False

    def stop(self):
        if self.client:
            self.client.disconnect()
            logger.info("Telegram отключён")
