# Voice commands

Press and hold `Alt+X`, speak, release. Voicy will recognize what you said and dispatch the right action.

You can re-bind the hotkey in Settings → Input → Hotkey.

## Send a Telegram message

Format: **`<trigger> <contact> <message>`**

| Triggers | Examples |
|---|---|
| `write` / `send` | `write tim on my way` |
| `tell` | `tell mom I'll call later` |
| Russian: `напиши` / `отправь` / `запиши` / `пиши` | `напиши тиме где ты` |

**Smart parsing handles:**

- **Slurred speech** — `writechinehi` → `write` `Chine` `hi`
- **Split names** — `write chi ne hi` → `write` `Chine` `hi`
- **Multiple aliases** — set up "tim", "tima", "timka" for the same person in Settings → Telegram
- **Russian morphology** — `тиме` / `тимы` / `тимой` all match contact "Тима"

If no recognized name found, Voicy shows a red `✗` overlay and logs the parse failure.

## Browser control

Voicy sends keyboard shortcuts to whatever browser tab is currently focused. Works on YouTube, Twitch, most video sites.

| Command | What it does |
|---|---|
| `open YouTube <query>` | Open YouTube search page |
| `play first video` / `play second` / ... | Open the Nth search result with autoplay |
| `volume up [N percent]` | YouTube ↑ × ceil(N/5), default ~10% |
| `volume down [N percent]` | YouTube ↓ |
| `fullscreen` / `full screen` | Toggle F |
| `pause` / `play` / `stop` | Space |
| `skip forward [N seconds]` | YouTube → × ceil(N/5) |
| `skip back [N seconds]` | YouTube ← |
| `mute` / `unmute` | M |

The YouTube tab needs to be focused for shortcuts to register. Voicy doesn't auto-switch tabs.

## AI assistant

Trigger: `give answer <question>` (or just `give answer` followed by your question after a pause).

```
"Give answer — what is the capital of Mongolia"
→ Voicy: "Ulaanbaatar."
```

**Backend priority:**
1. **Gemini API** (~1-2 sec) if you set `gemini_api_key` in `voicy.toml` (free 1500 requests/day at https://aistudio.google.com/app/apikey)
2. **Local LLM** (Qwen 0.5B / Llama 3.2 1B / Gemma 2 2B) — slower (~30s) but works offline. Configured in Settings → AI Assistant.

Voicy speaks the answer back via Windows TTS (the system voice for your `ai_language` setting).

## Open URLs / search

| Command | Result |
|---|---|
| `open Google <query>` | Google search |
| `open YouTube <query>` | YouTube search |
| `open TikTok <query>` | TikTok search |
| `open Twitch <query>` | Twitch search |

URL opens in your default browser.

## Hotkey doesn't trigger?

1. Check that the Listener badge shows **🟢 listening** in Settings → Telegram tab
2. If your Telegram session expired, sign in again — Voicy keeps listening either way
3. Some games/apps that grab raw keyboard input may prevent low-level hooks. Switch to a different hotkey via Settings → Input → change (try `Ctrl+Space` or `F8`)
4. Antivirus / Windows Defender sometimes blocks WH_KEYBOARD_LL hooks. Whitelist `voicy.exe` in your security software.

## Logs

Voicy writes to stderr. To see logs:

```powershell
voicy.exe ui 2> voicy.log
```

Or just run from a terminal — output is live. Useful when debugging parser misses or Telegram errors.
