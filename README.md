# voicy (Rust port)

Полная замена `D:\claude\Cheen\` на Rust. **3 МБ exe** vs **120 МБ** Python.

## Build

```
$env:CARGO_HOME = "D:\rust\.cargo"
$env:Path = "D:\llvm-mingw\bin;D:\rust\.cargo\bin;$env:Path"
$env:CARGO_TARGET_DIR = "D:\rust\target_voicy"
cargo build --release
```

`voicy.exe` появится в `D:\rust\target_voicy\release\`.

## Файлы рядом с exe

| Файл | Назначение |
|---|---|
| `voicy.exe` | сам бинарь (3 МБ) |
| `voicy.toml` | конфиг: hotkey / модель / api_id |
| `voicy_session.session` | сохранённая Telegram-сессия |
| `contacts.txt` | контакты: `uid - имя1, имя2` |
| `whisper/whisper-cli.exe` | бинарь whisper.cpp (~5 МБ + DLL для BLAS) |
| `whisper/ggml-<model>.bin` | веса (tiny=75 МБ, base=140 МБ и т.д.) |
| `voicy_capture.wav` | временный файл записи |

## First-time setup

```
voicy.exe model setup            # скачивает whisper.cpp (~16 МБ)
voicy.exe model download tiny    # или base / small / medium / large-v3
voicy.exe setup                  # интерактивный Telegram login (phone+code)
```

## Running

```
voicy.exe run                    # фоновый листенер Alt+X
```

Скажи «**напиши <имя> <текст>**» зажав Alt+X — отправится в Telegram.

## CLI commands

- `voicy ui` — окно настроек (wry webview, login/hotkey/model/contacts)
- `voicy run` — фоновый hotkey-listener + overlay
- `voicy info` — показать конфиг
- `voicy record <sec>` — записать тест в `test_capture.wav`
- `voicy transcribe <wav>` — прогнать WAV через whisper
- `voicy send <uid> <text>` — отправить сообщение напрямую (для отладки)
- `voicy model setup` — скачать whisper.cpp бинарь
- `voicy model download <name>` — скачать модель
- `voicy setup` — Telegram login (CLI вариант, для серверов без GUI)

## Build deps на машине разработчика

- `D:\rust\.cargo\bin\cargo.exe` — Rust 1.95 GNU
- `D:\llvm-mingw\bin\` — LLVM-MinGW 22 (clang + lld + dlltool)
- `D:\cmake\bin\cmake.exe` — CMake 4.3 (нужен крейтам, что компилируют C-код)
- `LIBCLANG_PATH` → `C:\...\Python313\Lib\site-packages\clang\native` (нужен bindgen)

Env vars зашиты в User profile.

## Размер vs Python

| | Python | Rust |
|---|---|---|
| `voicy.exe` | 119.6 МБ | **3.58 МБ** (33×) |
| Startup | 2–3 сек | мгновенно |
| Whisper subprocess | 6–26 сек первый запуск | 0.5–1 сек (persistent через mmap) |
| Зависимости | bundled Python | системные DLL + WebView2 |

## Полный паритет ✓

| фича | python | rust |
|---|---|---|
| QR/phone Telegram login | ✓ | ✓ (phone-code в UI) |
| Hotkey listener | ✓ | ✓ |
| Запись + whisper | ✓ | ✓ |
| Парсер «напиши имя текст» + fuzzy | ✓ | ✓ |
| Overlay-эквалайзер | ✓ | ✓ |
| Settings webview | ✓ | ✓ |
| Скачивание моделей | ✓ | ✓ |
| Switch active model | ✓ | ✓ |
| Авто-restart whisper-сервиса | n/a (нет сервиса) | n/a |
| QR-login | ✓ | ✗ (только phone-code пока) |

## TODO (опциональное)

- [ ] QR-login для Telegram (grammers поддерживает через `request_login_code`+ QR)
- [ ] Trigger words / алиасы редактирование из UI
- [ ] Tray-икона
