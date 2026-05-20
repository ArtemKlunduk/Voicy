<div align="center">

<img src="logo.svg" width="80" alt="Voicy"/>

# Voicy

**Hold a key. Speak. Your message is sent.**

Voice-to-action for Windows. No typing, no opening apps — just a hotkey and your voice.

[Download](https://github.com/ArtemKlunduk/Voicy/releases) · [Install guide](docs/INSTALL.md) · [Telegram setup](docs/TELEGRAM_SETUP.md) · [Commands](docs/USAGE.md)

</div>

---

## What it does

- 🎙️ **Push-to-talk** — hold `Alt+X`, speak, release
- 🚀 **Auto-send** — message lands in Telegram in 3 seconds
- 🧠 **Smart parser** — handles slurred speech, split words, wrong triggers
- 🤖 **AI assistant** — say "give answer" + question → Voicy responds aloud
- 📺 **Browser control** — voice volume/fullscreen/play/pause for YouTube
- 👤 **Real contacts** — pulled from your Telegram with avatars
- 🪶 **3 MB binary** — pure Rust, no Electron, runs offline

## Privacy

Voicy stores your Telegram session, contacts cache, dialog cache, downloaded ASR/AI models, and settings locally on your PC. Nothing is uploaded to Voicy servers.

The bundled Telegram `api_id` / `api_hash` identify the Voicy app to Telegram, similar to other Telegram desktop clients. They are not your Telegram login. Your account authorization is stored in your local session file and should not be shared.

## Quick install

1. **Download** the latest release: [Releases page](https://github.com/ArtemKlunduk/Voicy/releases)
2. **Unzip** anywhere (e.g. `C:\Voicy\`)
3. **Run** `voicy.exe` — that's it. Telegram app credentials are pre-embedded.
4. On the **Telegram** tab → click **QR** → scan with your phone (Telegram → Settings → Devices → Link Desktop).
5. Hold `Alt+X` and say "write `<contact>` hi" to test.

> **Privacy-conscious?** Want to use your own Telegram app instead of ours? See [TELEGRAM_SETUP.md](docs/TELEGRAM_SETUP.md).

Full step-by-step: [INSTALL.md](docs/INSTALL.md)

## Quick demo

After setup, press and hold `Alt+X` and say:

> *"Write Josh — running late, will be there in 10."*

Release. Done. Message in your friend's Telegram inbox.

## Voice commands

| Say | What happens |
|---|---|
| `write <name> <message>` | Send Telegram message |
| `open YouTube <query>` | Open YouTube search |
| `play first video` | Open first search result |
| `volume up 20 percent` | YouTube volume +20% |
| `fullscreen` | Toggle fullscreen |
| `pause` / `play` | Play/pause video |
| `give answer <question>` | AI assistant replies aloud |

Full list: [USAGE.md](docs/USAGE.md)

## Build from source

```powershell
git clone git@github.com:ArtemKlunduk/Voicy.git
cd Voicy
scripts\build-release.cmd
```

Requires Rust 1.75+ with MSVC target and Visual Studio Build Tools. Details: [INSTALL.md → Building from source](docs/INSTALL.md#building-from-source).

## Tech

- **Language:** Rust 1.95 with MSVC target
- **ASR:** [Parakeet V3](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx) (NeMo, ONNX int8) or whisper.cpp
- **MTProto:** [grammers-client](https://github.com/Lonami/grammers)
- **AI:** Google Gemini API + local LLM via [candelabra](https://github.com/alan13367/candelabra) (Qwen / Llama / Gemma)
- **UI:** [wry](https://github.com/tauri-apps/wry) (WebView2) + native Win32 overlay (per-pixel alpha via `UpdateLayeredWindow`)

## Project layout

```
voicy/
├── src/              Rust source
├── assets/           Bundled DLLs + icon
├── docs/             User documentation
├── scripts/          Build & release scripts
├── Cargo.toml        Dependencies
└── README.md
```

## Contributing

PRs welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

This project is being built by two people:
- [@ArtemKlunduk](https://github.com/ArtemKlunduk) — Telegram, UI, AI integration
- [@tuwulalo](https://github.com/tuwulalo) — ASR, native overlay, build

## License

MIT — see [LICENSE](LICENSE).
