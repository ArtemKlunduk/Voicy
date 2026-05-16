# Voicy desktop UI kit

A faithful recreation of the **Voicy** Windows desktop app (the WebView2 content area inside `voicy.exe`).

**Important caveat:** the actual product frontend lives compiled inside the Rust binary and was not available to read. This kit is a **design recreation** from:
- the visible app icon (`assets/voicy-logo.png`)
- the `voicy.toml` schema (hotkey, language, model, telegram session)
- the product brief (Russian, friend-of-the-family tone, free + OSS, minimalist green)

If the real `index.html` / CSS becomes available, this kit should be aligned to it.

## Files

```
ui_kits/desktop/
├── README.md              ← you are here
├── index.html             ← interactive click-through (login → main → record → settings)
├── App.jsx                ← top-level state machine
├── Window.jsx             ← decorative Windows app frame for the preview
├── Header.jsx             ← logo + status dot + settings cog
├── ContactList.jsx        ← search + rows
├── RecordPanel.jsx        ← recording overlay (the orb + live waveform)
├── HotkeyHint.jsx         ← footer "Alt + X" pill
├── ConnectScreen.jsx      ← first-run Telegram session connect
├── SettingsScreen.jsx     ← hotkey / language / model / autostart
├── Icon.jsx               ← thin Lucide icon wrapper
└── data.js                ← fake contacts + state helpers
```

## What you can click through in `index.html`

1. **Connect** — first run, fake Telegram phone-code login. Type any phone, any 5-digit code, press → enters the main screen.
2. **Main screen** — pick a contact. Then press the on-screen `Alt + X` button (or the actual Alt+X keys) to start a fake recording.
3. **Recording** — orb pulses, live waveform animates, transcript appears progressively. Release / press Send to "deliver" to Telegram.
4. **Sent toast** — confirms; main screen returns.
5. **Settings (cog)** — toggle autostart, sound, change hotkey, see model + language.

Everything is fake — there is no real Telegram, no real Whisper. The point is the visual + interaction language, not the wiring.
