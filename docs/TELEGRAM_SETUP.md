# Telegram API setup (optional)

> **TL;DR — you don't need this for normal use.** Voicy ships with built-in Telegram credentials. Just run `voicy.exe` and log in.
>
> This document is for people who want to use their **own** Telegram app credentials instead — either for privacy, or because the shipped ones got banned/rotated.

## What is stored locally

Voicy stores your Telegram authorization session, contact cache, dialog cache, settings, and downloaded models locally in `%APPDATA%\voicy\`.

The bundled `api_id` / `api_hash` only identify the Voicy application to Telegram. They do not grant access to your Telegram account by themselves. Your actual account authorization is the local `voicy_session.session` file, and you should never share it.

## Why have your own credentials?

Telegram tracks abuse per-application. The credentials shipped with Voicy are shared across all users — if someone abuses the API through their own client masquerading as "Voicy", Telegram might ban our `api_id` and everyone's install temporarily breaks until we rotate.

With your own credentials:
- You control your own quota
- Telegram can't bulk-ban you with other Voicy users
- Privacy: Telegram can't correlate your app traffic with other Voicy users
- Free, no payment, no limit on legitimate use

## Step-by-step

### 1. Go to my.telegram.org

Open **https://my.telegram.org** in any browser.

### 2. Log in

Enter your phone number (the same one your Telegram account uses). You'll receive a code in Telegram (the app messages it to you in **Telegram** → search for **Telegram** chat). Enter the code.

### 3. Open API development tools

Click **API development tools** on the main page.

### 4. Fill in the application form

| Field | Value |
|---|---|
| **App title** | `Voicy` |
| **Short name** | `voicy` (5–32 lowercase chars, only this user's namespace) |
| **URL** | `https://github.com/ArtemKlunduk/Voicy` (optional but recommended) |
| **Platform** | ⚫ **Desktop** |
| **Description** | `Voice-to-Telegram helper, hold a hotkey and speak.` |

Click **Create application**.

### 5. Copy your credentials

You'll see a page with:

- **App api_id** — an integer like `12345678`
- **App api_hash** — a 32-character hex string like `a1b2c3d4e5f6...`

Treat them as app identity, not as your Telegram password. Anyone with both can write a client that identifies as your app, so do not publish personal app credentials unless you intentionally want them shared.

### 6. Paste into voicy.toml

Open `voicy.toml` (next to `voicy.exe` if pre-built, or `%APPDATA%\voicy\voicy.toml` after first run). Find the `[telegram]` section:

```toml
[telegram]
api_id = 12345678                       # ← your number
api_hash = "a1b2c3d4e5f6..."            # ← your hash (in quotes!)
session = "voicy_session"
```

Save (`Ctrl+S`). Restart Voicy.

### 7. Log in to your account

Open Voicy → Telegram tab → click **QR**. Open Telegram on your phone → Settings → Devices → Link Desktop Device → scan the QR shown in Voicy.

Your session is saved to `%APPDATA%\voicy\voicy_session.session`. You won't need to re-login unless you sign out manually.

## Alternative — environment variables

If you'd rather not put credentials in a file (e.g. for CI/CD or shared machines):

```powershell
[Environment]::SetEnvironmentVariable("VOICY_TG_API_ID", "12345678", "User")
[Environment]::SetEnvironmentVariable("VOICY_TG_API_HASH", "a1b2c3d4...", "User")
```

Reopen any shell for vars to take effect. Voicy reads env vars **on top of** `voicy.toml` — env wins if both are set.

## Can I delete an app I created?

**No** — Telegram doesn't allow deleting API applications. They live forever. If you ever need to rotate (e.g. credentials leaked), just:

1. Create a new application with a different `Short name` (e.g. `voicy2`)
2. Rename the old one to `Voicy (deprecated)` so it's clear which to ignore
3. Use the new `api_id` / `api_hash` everywhere

## Troubleshooting

- **`FLOOD_WAIT_X`** — Telegram rate-limited you. Voicy retries automatically. If persistent (>1 min), wait it out.
- **`AUTH_KEY_UNREGISTERED`** — session file got invalidated (you signed out elsewhere, or `voicy_session.session` is corrupt). Delete `%APPDATA%\voicy\voicy_session.session` and re-login.
- **`API_ID_INVALID`** — wrong `api_id` or `api_hash` in `voicy.toml`. Double-check on my.telegram.org.
- **`SESSION_PASSWORD_NEEDED`** — your account has 2FA enabled. Voicy supports it — the password prompt should appear in the QR / phone login flow.
