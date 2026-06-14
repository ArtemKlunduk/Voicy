Voicy 0.1.0 - Voice-to-Telegram for Windows
===========================================

Hold a hotkey, speak, and Voicy transcribes your voice and sends it as a
Telegram message to the contact you name. Fully offline speech recognition
(Parakeet / Whisper); only Telegram delivery uses the network.

SETUP
-----
1. Get free Telegram API credentials:
   https://my.telegram.org  ->  API development tools  ->  Create application.
   Copy your api_id and api_hash.

2. Rename "voicy.toml.example" to "voicy.toml" (next to voicy.exe) and put your
   api_id and api_hash into the [telegram] section.

3. Launch Voicy (Start Menu or desktop shortcut).

4. Open the Telegram tab -> click QR -> scan it with Telegram on your phone
   (Settings -> Devices -> Link Desktop Device).

5. Add contacts and give each a short alias, then hold Alt+X and say
   "напиши <alias> привет" (or "write <alias> hi"). To message yourself, say
   "напиши себе ...".

NOTES
-----
- Config (voicy.toml) lives next to voicy.exe. The Telegram session and the
  downloaded ASR model are stored in %APPDATA%\voicy and are kept on uninstall.
- The recognition model (~0.6 GB for Parakeet) downloads automatically on first
  use.

LICENSE: MIT (see LICENSE.txt)
