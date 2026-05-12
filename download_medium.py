import whisper

print("Скачивание модели medium (~1.5 GB). Это может занять несколько минут…")
model = whisper.load_model("medium")
print("Готово! Модель medium скачана и готова к использованию.")
print("Теперь можешь поменять WHISPER_MODEL = 'medium' в config.py")
input("Нажми Enter для выхода…")
