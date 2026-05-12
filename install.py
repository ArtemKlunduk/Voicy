import os
import shutil


def main():
    script_dir = os.path.dirname(os.path.abspath(__file__))

    # 1. Создаём run_background.vbs в папке проекта (для ручного запуска)
    vbs_local = os.path.join(script_dir, "run_background.vbs")
    main_py = os.path.join(script_dir, "main.py")

    vbs_content = f'''Set WshShell = CreateObject("WScript.Shell")
WshShell.CurrentDirectory = "{script_dir}"
WshShell.Run "pythonw ""{main_py}""", 0, False
Set WshShell = Nothing
'''
    with open(vbs_local, "w", encoding="utf-8") as f:
        f.write(vbs_content)

    # 2. Копируем его в папку автозагрузки Windows
    startup_dir = os.path.join(
        os.environ["APPDATA"],
        "Microsoft", "Windows", "Start Menu", "Programs", "Startup"
    )
    os.makedirs(startup_dir, exist_ok=True)
    vbs_startup = os.path.join(startup_dir, "VoiceTGHelper.vbs")
    shutil.copy2(vbs_local, vbs_startup)

    print("=" * 50)
    print("Готово!")
    print("=" * 50)
    print(f"Лаунчер создан:        {vbs_local}")
    print(f"Автозагрузка Windows:  {vbs_startup}")
    print()
    print("Что дальше:")
    print("  1. Заполни API_ID и API_HASH в config.py")
    print("     (получить на https://my.telegram.org/apps)")
    print("  2. Проверь contacts.txt")
    print("  3. Дважды кликни по run_background.vbs — скрипт запустится БЕЗ консоли")
    print("  4. При первом запуске появятся окна для входа в Telegram")
    print("  5. После входа всё будет работать в фоне и стартовать с Windows")
    print()
    input("Нажми Enter для выхода…")


if __name__ == "__main__":
    main()
