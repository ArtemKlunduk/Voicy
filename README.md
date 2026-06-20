<div align="center">

<img src="logo.svg" width="80" alt="Voicy"/>

# Voicy

**Hold a key. Speak. It happens.**

Voice-to-Telegram for Windows. Hold a hotkey, speak, release: your message is sent. No typing, no opening apps.

[Download](https://github.com/ArtemKlunduk/Voicy/releases) · [Install guide](docs/INSTALL.md) · [Telegram setup](docs/TELEGRAM_SETUP.md) · [Commands](docs/USAGE.md)

</div>

---

## What it does

- 🎙️ **Push-to-talk**: hold `Alt+X`, speak, release
- 🚀 **Auto-send**: your message lands in Telegram in seconds
- 🧠 **Smart parser**: handles slurred speech, split words, wrong triggers and name declensions, fully offline and rule-based
- ⌨️ **Dictation mode**: a phrase without a send-trigger is typed straight into the active window (Handy-style)
- 🎵 **Music download**: say «скачай это» with a track open in the browser and a downloader bot fetches it into Telegram (optionally «...и скинь Маше» to forward it to a contact)
- 👤 **Real contacts**: pulled from your Telegram with avatars
- 🌐 **Offline ASR**: Parakeet V3 runs locally, no audio leaves your machine

## Quick install

1. **Download** the latest release: [Releases page](https://github.com/ArtemKlunduk/Voicy/releases)
2. **Unzip** anywhere (e.g. `C:\Voicy\`)
3. **Run** `voicy.exe`. Telegram credentials are pre-embedded, so it works out of the box.
4. On the **Telegram** tab, click **QR** and scan with your phone (Telegram → Settings → Devices → Link Desktop).
5. Hold `Alt+X` and say «напиши `<контакт>` привет» to test.

> **Privacy-conscious?** Want to use your own Telegram app instead of the bundled credentials? See [TELEGRAM_SETUP.md](docs/TELEGRAM_SETUP.md).

Full step-by-step: [INSTALL.md](docs/INSTALL.md)

## Voice commands

| Say | What happens |
|---|---|
| «напиши `<имя>` `<сообщение>`» / `write <name> <message>` | Send a Telegram message |
| «напиши себе ...» / `write myself ...` | Send to your Saved Messages |
| any phrase **without** a send-trigger | Typed into the active window (dictation) |
| «скачай это» | Download the track from the active browser tab into Telegram |
| «скачай это в wav» | Same, but force WAV instead of MP3 |
| «скачай это и скинь `<имя>`» | Download, then forward the file to a contact |

Russian and English triggers both work, and the parser is forgiving of recognition slips. Full list: [USAGE.md](docs/USAGE.md)

## Music download

With a track open in your browser, say «скачай это». Voicy reads the active tab URL (via Win32 UI Automation, with a clipboard fallback), sends it to your downloader bot in Telegram, waits for the bot's audio reply, and forwards the file to your Saved Messages, or to a contact you name. Configure the bot username, default format (MP3/WAV) and destination under **Settings → Music**. Default bot: `@cloudpullbot`.

## Build from source

```powershell
git clone git@github.com:ArtemKlunduk/Voicy.git
cd Voicy
scripts\build-release.cmd
```

Requires Rust with the `x86_64-pc-windows-msvc` target and the Visual Studio Build Tools (or a portable MSVC + Windows SDK). Details: [INSTALL.md → Building from source](docs/INSTALL.md#building-from-source).

## Tech

- **Language:** Rust, MSVC target
- **ASR:** [Parakeet V3](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx) (NeMo, ONNX int8), with a whisper.cpp fallback
- **MTProto:** [grammers-client](https://github.com/Lonami/grammers) (pure-Rust userbot)
- **URL capture:** Win32 UI Automation (reads the browser address bar)
- **Dictation:** Win32 SendInput (Unicode key events)
- **UI:** [wry](https://github.com/tauri-apps/wry) (WebView2) + a native Win32 overlay (per-pixel alpha via `UpdateLayeredWindow`)

## Project layout

```
voicy/
├── src/              Rust source
├── assets/           Bundled DLLs + icon
├── docs/             User documentation
├── installer/        Inno Setup script + payload
├── scripts/          Build & release scripts
├── Cargo.toml        Dependencies
└── README.md
```

## Contributing

PRs welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

This project is built by two people:
- [@ArtemKlunduk](https://github.com/ArtemKlunduk): Telegram, UI, product
- [@tuwulalo](https://github.com/tuwulalo): ASR, native overlay, build

## License

MIT, see [LICENSE](LICENSE).
