# Voice commands

Press and hold `Alt+X`, speak, release. Voicy recognizes what you said and acts on it through your Telegram.

Re-bind the hotkey in Settings → Input → Hotkey.

## Send a Telegram message

Format: **`<trigger> <contact> <message>`**

| Triggers | Examples |
|---|---|
| Russian: `напиши` / `отправь` / `запиши` / `пиши` | `напиши тиме где ты` |
| English: `write` / `send` / `text` | `write tim on my way` |

Send to yourself (Saved Messages) by naming the recipient «себе» / «мне» / `myself`:

```
напиши себе купить хлеб
```

Recognition is forgiving: glued words («напишичинепривет»), split names («напиши чи не привет») and Russian morphology («тиме» / «тимой» both match the contact «Тима») are handled. Add extra aliases per contact in Settings → Telegram. If no contact is recognized, Voicy shows a red `✗` overlay and copies the text to the clipboard so nothing is lost.

## Dictation

A phrase **without** a send-trigger is typed straight into whatever window is focused, like a dictation pad:

```
(focus any text field) → «поехали в пятницу в шесть» → typed into the field
```

Toggle it in Settings → General.

## Download music

With a track open in your browser, say:

| Say | Result |
|---|---|
| «скачай это» | Grab the active tab URL, hand it to your downloader bot, forward the audio to Saved Messages |
| «скачай это в wav» | Same, but force WAV instead of MP3 |
| «скачай это и скинь `<имя>`» | Forward the downloaded file to a contact instead |

Configure the bot, default format and destination in Settings → Music. Keep the track's tab as the active window (Voicy reads its address bar); if it is not focused, Voicy scans your open browser windows as a fallback.

## Hotkey doesn't trigger?

1. Check the Listener badge shows listening on the Settings → Telegram tab.
2. If your Telegram session expired, sign in again; Voicy keeps listening either way.
3. Some games/apps that grab raw keyboard input can block low-level hooks. Change the hotkey in Settings → Input.
4. Antivirus / Windows Defender sometimes blocks `WH_KEYBOARD_LL` hooks. Whitelist `voicy.exe`.

## Diagnostics & logs

Voicy logs to `%APPDATA%\voicy\voicy.log` (rotated at 5 MB; content-bearing lines stay at debug level). Handy CLI commands when something misbehaves:

```powershell
voicy.exe parse "напиши тиме привет"   # dry-run the parser, prints (uid, message), no send
voicy.exe url                          # print the browser URL it would capture
voicy.exe download <url> [mp3|wav]     # run the whole music flow for a URL
voicy.exe type "текст"                 # test dictation typing into the active window
```
