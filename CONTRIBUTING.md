# Contributing

Two-person team workflow. Light process, no overkill.

## Branches

```
main        ← stable. Only via PR with review.
shlepa      ← @tuwulalo working branch
artem       ← @ArtemKlunduk working branch
feat/<name> ← short-lived feature branches → PR to main
```

**No direct push to main.** Use a PR with at least one approval from the other dev.

## Ownership zones (suggested)

| Module | Files | Primary owner |
|---|---|---|
| ASR + parser | `src/asr.rs`, `src/parakeet.rs`, `src/audio.rs`, `src/contacts.rs` | @tuwulalo |
| Telegram | `src/telegram.rs` | @ArtemKlunduk |
| Music download | `src/active_url.rs`, `download_via_bot` in `src/telegram.rs` | shared |
| UI | `src/ui.rs`, `src/ui.html`, `src/config.rs` | @ArtemKlunduk |
| Overlay + input | `src/native_overlay.rs`, `src/typing.rs`, `src/hotkey.rs` | @tuwulalo |
| Build / packaging | `Cargo.toml`, `build.rs`, `scripts/`, `installer/` | @tuwulalo |

"Owner" = first reviewer for PRs touching this area, not "only one allowed to edit."

## Commits

Format: `<area>: what was done`

```
parser: handle slurred names across 3 tokens
overlay: native Win32 layered window
docs: clarify Telegram credentials setup
```

Areas: `asr`, `tg`, `ui`, `overlay`, `parser`, `build`, `docs`, `release`.

This makes `git log --oneline` immediately readable.

## Pull requests

1. Create issue first (or pick existing). Move to **In Progress** on the project board.
2. Branch off main: `git checkout -b feat/my-feature main`.
3. Commit incrementally — small, focused commits.
4. Push, open PR to `main`. Request review from the other dev.
5. After approval — **Squash and merge** (clean main history).
6. Close the issue.

## Local development

```powershell
git clone git@github.com:ArtemKlunduk/Voicy.git
cd Voicy

# First-time: install MSVC Build Tools + Rust 1.75+ with MSVC target

scripts\build-release.cmd
```

Output: `target\x86_64-pc-windows-msvc\release\voicy.exe` (or wherever your `CARGO_TARGET_DIR` points).

For runtime, copy required DLLs from `assets/` next to the exe (or run `scripts\package-release.ps1` to build a distribution zip).

## Telegram credentials for development

`voicy.toml` is gitignored. Either:

1. Copy `voicy.toml.example` next to your built exe and fill in your test app credentials, OR
2. Set env vars `VOICY_TG_API_ID` + `VOICY_TG_API_HASH` (they override `voicy.toml`).

**Never commit credentials.** Pre-commit hook recommended:

```bash
git config core.hooksPath .githooks
```

(Hooks live in `.githooks/` if/when we add them.)

## Releasing

When ready to cut a new version:

1. Bump `version` in `Cargo.toml`
2. `scripts\build-release.cmd`
3. `scripts\package-release.ps1` → creates `dist/voicy-windows-x64-vX.Y.Z.zip`
4. Test the zip on a clean machine
5. Tag: `git tag vX.Y.Z && git push --tags`
6. Upload to GitHub Releases:
   ```powershell
   $env:GH_TOKEN = "ghp_..."   # tuwulalo PAT with repo scope
   gh release create vX.Y.Z dist\voicy-windows-x64-vX.Y.Z.zip --repo ArtemKlunduk/Voicy --title "Voicy vX.Y.Z" --notes-file CHANGELOG.md
   ```

## Daily sync (optional)

A 2-line message in chat: "yesterday X, today Y". Skip if `git log` already tells the story.

## What not to do

- ❌ Long-lived feature branches (>1 week) — merge hell
- ❌ Big PRs (>500 lines) — nobody can review properly
- ❌ Force-push to `main` or someone else's branch
- ❌ Commits touching 5 unrelated things
- ❌ Committing `voicy.toml`, `*.session`, or any other secrets
