# Installation

Two ways to install Voicy: ready-to-run binary (recommended), or build from source.

## Option A — Pre-built binary (3 minutes)

### 1. Download

Go to **[Releases](https://github.com/ArtemKlunduk/Voicy/releases)** → grab the latest `voicy-windows-x64-vX.Y.Z.zip`.

### 2. Unzip

Extract anywhere you like. Suggested:

```
C:\Voicy\
  ├── voicy.exe
  ├── voicy.toml.example
  ├── WebView2Loader.dll
  ├── msvcp140.dll
  ├── vcruntime140.dll
  ├── msvcp140_1.dll
  ├── vcruntime140_1.dll
  └── README.txt
```

### 3. Run

Double-click `voicy.exe`. 

First run:
- Settings UI opens
- On the **Telegram** tab → click **QR** → scan with your phone (Telegram → Settings → Devices → Link Desktop Device)
- Voicy downloads Parakeet ASR model (~670 MB, one-time)
- Hold `Alt+X` and say "write <contact> hi" to test

### 4. (Optional) Run at startup

Settings → System → toggle **Run at startup**. Voicy will launch in background every time you log in.

### 5. (Optional) Use your own Telegram app credentials

Voicy ships with Telegram app credentials embedded in the binary, so normal users do not need to register anything. These credentials identify the Voicy app to Telegram; they are not your Telegram account login.

If you want to use your own Telegram app identity instead, see **[TELEGRAM_SETUP.md](TELEGRAM_SETUP.md)**. It takes about 2 minutes to register an app at my.telegram.org and override the bundled credentials via `voicy.toml`.

---

## Option B — Build from source

For developers or if you want to modify Voicy.

### Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.75+ with MSVC target | `rustup target add x86_64-pc-windows-msvc` |
| Visual Studio Build Tools | 2022 with C++ workload | [aka.ms/vs/17/release/vs_BuildTools.exe](https://aka.ms/vs/17/release/vs_BuildTools.exe) |
| Git | any | for cloning |

### Build

```powershell
git clone https://github.com/ArtemKlunduk/Voicy.git
cd Voicy
scripts\build-release.cmd
```

`voicy.exe` lands in `target\x86_64-pc-windows-msvc\release\voicy.exe` (or wherever `CARGO_TARGET_DIR` points).

For the bundled distribution layout (with all DLLs in one folder), run `scripts\package-release.ps1` after building.

### Troubleshooting

- **`link.exe not found`** → MSVC Build Tools not in PATH. Run `scripts\build-release.cmd` (it sets up `vcvarsall.bat` automatically).
- **`+crt-static mismatch`** → ensure `.cargo/config.toml` doesn't have `+crt-static` for `x86_64-pc-windows-msvc`.
- **`AUTH_KEY_UNREGISTERED`** → your Telegram session expired. Delete `%APPDATA%\voicy\voicy_session.session` and re-login.
- **WebView2 missing** → install [WebView2 runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) (built into Windows 11, may be missing on older Windows 10).

---

## Where Voicy stores your data

`%APPDATA%\voicy\` — that's `C:\Users\<you>\AppData\Roaming\voicy\`. Open via `Win+R → %APPDATA%\voicy`.

- `voicy_session.session` — Telegram login (don't share)
- `contacts.txt` — your contact list
- `voicy_dialogs.cache` — cached Telegram dialogs
- `models/` — Parakeet / Canary ONNX weights
- `whisper/` — whisper.cpp binaries + ggml weights

Voicy does not send this local data to any Voicy server. Telegram messages are sent directly through Telegram, and optional Gemini features call Google's Gemini API only when you configure a Gemini API key.

To start completely fresh: stop Voicy, delete `%APPDATA%\voicy\`, restart.

---

## Uninstall

1. Stop Voicy (right-click tray icon → Quit, or Task Manager → End task `voicy.exe`)
2. If "Run at startup" was on → Settings → System → toggle off OR delete the registry key `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Voicy`
3. Delete the `C:\Voicy\` folder (or wherever you unzipped)
4. Delete `%APPDATA%\voicy\` to wipe all user data

That's it. No system-wide installer, no leftover files anywhere else.
