---
name: voicy-design
description: Use this skill to generate well-branded interfaces and assets for Voicy — the tiny, free, open-source voice → Telegram assistant. Contains essential design guidelines, soft-green color palette, type system (Onest + JetBrains Mono, Cyrillic-ready), brand assets, icons, and a faithful desktop UI kit for prototyping in-app screens, GitHub READMEs, social cards, or anything else Voicy-shaped.
user-invocable: true
---

# Voicy design skill

Read **README.md** first — it has the brand brief, content fundamentals, visual foundations, and an index of every file in this folder.

Key files at a glance:
- `README.md` — brand bible (palette, type, motion, voice & tone, content rules, iconography).
- `colors_and_type.css` — the only source of truth for color + type tokens. Always import this.
- `assets/` — logo (`.png`, `.ico`, `.svg` mark + lockup), waveform motif, Lucide icon subset, Telegram glyph.
- `fonts/` — drop `.woff2` files here to override the Google Fonts CDN if needed.
- `preview/` — 19 small design-system cards demonstrating tokens + components.
- `ui_kits/desktop/` — the Voicy desktop app, faithfully recreated as React components (`App.jsx`, `Header.jsx`, `ContactList.jsx`, `RecordPanel.jsx`, `HotkeyHint.jsx`, `ConnectScreen.jsx`, `SettingsScreen.jsx`, `Window.jsx`). `index.html` is an interactive click-through.

## How to use

If the user asks for a **visual artefact** (mock, slide, social card, throwaway prototype, GitHub README hero, screen recreation):
1. Copy `colors_and_type.css` and the assets you need into your output folder.
2. Pull components from `ui_kits/desktop/*.jsx` rather than hand-rolling new ones.
3. Output static HTML (or React, if interactive). Russian-first copy; English secondary.

If the user is working on **production code**:
- Read the README, then mirror the tokens and rules. Don't reinvent.
- Match the voice: calm, "ты", lowercase, no emoji, no marketing language.

## The five non-negotiables

1. **Calm green, never neon.** Sage `#A8C8A0` is the brand. Avoid mint, lime, emerald, gradients.
2. **Borders, not shadows.** Hairline `#DDE3D6`. Exactly one shadow exists in the whole system.
3. **Russian-first, "ты", lowercase, sentence case.** No emoji in product UI.
4. **No "AI" framing.** Voicy is a tiny utility for ordinary people, not a futuristic assistant.
5. **When in doubt — less.** Whitespace is part of the design.

## If the user invokes this skill bare

Ask 2–3 short questions:
- What are we making? (deck / mock / readme / new screen / brand asset)
- For who? (the GitHub audience, an in-app screen, a friend who wants to try it)
- Russian, English, or both?

Then act as an expert designer and produce HTML artefacts (or production code) that honour the system in this folder.
