import os

BASE_DIR = os.path.dirname(os.path.abspath(__file__))

# ── Telegram API (получить на https://my.telegram.org/apps) ──
API_ID = 0              # <-- замени на свой api_id (число)
API_HASH = ""           # <-- замени на свой api_hash (строка)
SESSION_NAME = os.path.join(BASE_DIR, "tg_session")

# ── Пути ──
CONTACTS_FILE = os.path.join(BASE_DIR, "contacts.txt")
AUDIO_FILE = os.path.join(BASE_DIR, "temp_command.wav")
EQ_PATH = os.path.join(BASE_DIR, "эквалайзер", "icons8-аудио-волна-80.gif")

# ── Распознавание речи ──
RECOGNITION_PROVIDER = "local"   # "local" или "openai"
WHISPER_MODEL = "base"           # tiny / base / small / medium / large
WHISPER_UNLOAD_TIMEOUT = 300     # секунды простоя до выгрузки модели из RAM
OPENAI_API_KEY = ""              # ключ для OpenAI Whisper API

SAMPLE_RATE = 16000


def load_contacts():
    """Загружает ID → псевдонимы из contacts.txt."""
    contacts = {}
    if not os.path.exists(CONTACTS_FILE):
        return contacts
    with open(CONTACTS_FILE, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if " - " not in line:
                continue
            parts = line.split(" - ", 1)
            if len(parts) != 2:
                continue
            user_id = parts[0].strip()
            aliases = [a.strip().lower() for a in parts[1].split(",") if a.strip()]
            for alias in aliases:
                contacts[alias] = user_id
    return contacts
