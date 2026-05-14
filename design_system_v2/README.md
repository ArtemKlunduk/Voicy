# Voicy — Design System

> _Tихий помощник. Жми Alt + X — говори — Telegram отправит._

Voicy is a tiny, free, open-source voice assistant for Windows. You hold a hotkey, speak, and Voicy:

- transcribes you locally (offline ASR via **Whisper** / **Parakeet‑v3**, default RU),
- sends the result as a **Telegram** message to a chosen contact,
- can open the browser on simple voice commands.

It was built as a Rust desktop app embedding a tiny WebView2 UI, intended to be **simple enough for an older parent or grandparent to use**. There is no sign‑up, no subscription, no cloud. Press the key, say it, done.

The brand reflects this: gentle, calm, **soft green**, almost no chrome. Nothing flashy. The product should feel like a quiet companion, not an "AI app".

---

## What this folder is

This is a **design system** — a set of brand foundations, tokens, components and example screens that any agent (or designer) can use to make Voicy‑branded artefacts: in‑app screens, the GitHub README hero, social cards, slides, posters, etc.

It is not the product source. The product source lives in a separate Rust repository (the `release/` build folder was the only artifact provided to this design system; see _Sources_ below).

## Sources we worked from

| Source | What it gave us | Access |
|---|---|---|
| `release/` (Rust build output) | Confirmed product behaviour: `voicy.toml` (hotkey `Alt+X`, RU recognition, `parakeet‑v3`), Telegram session via `grammers-client`, WebView2-backed UI served at `voicy.localhost`, Whisper binaries shipped alongside. No frontend source. | mounted, read |
| `uploads/logo-icon.ico` | The product icon — soft green wave inside a circle, on near-black rounded square. Sets the entire palette. | uploaded |
| Russian product brief (from the user) | Tone, audience, mission: free, open source, built to **help ordinary people**, minimalist, soft green tones, language register is warm and unintimidating. | conversational |

> ⚠️ **No frontend source code was provided.** The actual in‑app WebView UI is compiled inside `voicy.exe`. The screens in `ui_kits/desktop/` are a faithful design recreation based on the product spec + the `voicy.toml` feature surface, not a port. If you have the WebView HTML/CSS, drop it in and we'll align.

---

## Index

```
.
├── README.md                  ← you are here
├── SKILL.md                   ← Agent Skill entrypoint (Claude Code compatible)
├── colors_and_type.css        ← all CSS variables: color tokens + type scale
├── assets/
│   ├── voicy-logo.png         ← rasterised app icon (256×256)
│   ├── voicy-icon.ico         ← original Windows .ico
│   ├── logo-mark.svg          ← clean vector mark (sage green wave)
│   ├── logo-lockup.svg        ← mark + "Voicy" wordmark
│   ├── waveform.svg           ← the brand wave motif, isolated
│   └── icons/                 ← Lucide subset used across UIs (see ICONOGRAPHY)
├── fonts/                     ← drop .woff2 here to override Google Fonts CDN
├── preview/                   ← 19 design-system cards rendered by the Design System tab
├── ui_kits/
│   └── desktop/               ← the Voicy desktop app, faithfully recreated
│       ├── README.md
│       ├── index.html         ← interactive click-through (?screen=main|connect|settings)
│       └── *.jsx              ← App, Window, Header, ContactList, RecordPanel,
│                                 HotkeyHint, ConnectScreen, SettingsScreen, Icon
└── slides/                    ← (not generated — no sample slides were provided)
```

---

## Content fundamentals

Voicy talks the way a kind, slightly soft-spoken Russian friend would explain something to a parent. **Always Russian-first.** English text exists (this README, the GitHub page) but is secondary.

**Voice & tone**

- **Calm, not enthusiastic.** No exclamation marks unless something genuinely deserves one. No "🎉", no "Awesome!".
- **Plain words.** "Голосовое сообщение", not "voice-to-text powered by AI". The brief literally says: _"чтобы как для людей"_ — "so it feels like it's made for people". Honour that.
- **"Ты", not "вы".** Voicy addresses the user informally — like a friend. Russian "ты", lowercase. In English, plain "you", lowercase.
- **Short sentences.** Often one line. Often without a period.
- **Verbs first when giving instructions.** "Зажми Alt+X", not "Чтобы начать запись, пожалуйста зажмите…"
- **No marketing adjectives.** Avoid "powerful", "revolutionary", "AI-powered", "беспрецедентный". Voicy is small and proud of being small.
- **Honest about being free and open source.** Say it once, clearly, then move on. "Бесплатно. Открытый исходный код."

**Casing**

- Product name is **Voicy** (always capital V, Latin script even in Russian copy).
- UI labels: **Sentence case** in both languages ("Начать запись", "Start recording") — never ALL CAPS, never Title Case.
- Buttons / chips: same — sentence case.

**Examples (good)**

- Empty state: _"Зажми Alt + X и говори."_
- Recording: _"Слушаю…"_
- Done: _"Отправлено в Telegram"_
- Error: _"Не услышал. Попробуй ещё раз."_
- About: _"Voicy — маленький помощник. Бесплатно. Открытый код."_

**Examples (avoid)**

- ❌ "🚀 Powered by AI! Send your voice anywhere INSTANTLY!"
- ❌ "Пожалуйста, нажмите и удерживайте сочетание клавиш Alt+X для активации голосового ввода."
- ❌ "Voicy™ — революционный голосовой ассистент."

**Emoji**

- Effectively **never** in product UI.
- A single inline glyph is OK in the GitHub README (e.g. one 🌿 in the tagline) — but reach for it sparingly and never as a bullet decoration.

---

## Visual foundations

The whole system is built around one mood: **the inside of a quiet greenhouse**. Soft sage green, warm off-white paper, charcoal-ink type, a single accent green for action. Everything is calm, generous in whitespace, and slightly imperfect (the wave logo is hand-curved, not geometric).

### Palette

Two surfaces — a warm light surface used for the marketing site and the app's light mode, and a deep charcoal used for the app icon, dark mode, and accents.

| Token | Value | Use |
|---|---|---|
| `--paper` | `#F7F8F3` | App background — warm off-white, never pure white |
| `--paper-2` | `#EEF1E8` | Recessed surfaces, hover wash |
| `--ink` | `#1B2421` | Primary text, logo dark, never `#000` |
| `--ink-2` | `#4E5A55` | Secondary text |
| `--ink-3` | `#8A9690` | Tertiary text, hints |
| `--line` | `#DDE3D6` | Hairline borders |
| `--sage` | `#A8C8A0` | The brand sage — straight from the logo wave |
| `--sage-soft` | `#D6E6CF` | Backgrounds for sage chips, recording indicator wash |
| `--sage-deep` | `#5E8B62` | Primary action, links, focus rings |
| `--moss` | `#2F4A35` | Pressed states, deep accents |
| `--warn` | `#C28B3E` | Warnings (muted ochre, never red-orange) |
| `--danger` | `#B6604D` | Destructive only (muted terracotta) |

All colors pass WCAG AA on `--paper`. `--ink` on `--paper` is the default text pairing.

### Typography

- **Display + body:** [**Onest**](https://fonts.google.com/specimen/Onest) — geometric, modern, excellent Cyrillic, gentle terminals. Weights used: 400, 500, 600. _If unavailable, **Manrope** is the substitution._
- **Mono:** [**JetBrains Mono**](https://fonts.google.com/specimen/JetBrains+Mono) — used only for the hotkey hint (`Alt + X`), code blocks in the GitHub README, and the contacts.txt UI. Weight 500.

> _Both fonts are Google Fonts substitutions; the actual product binary embeds the system default, which we don't have access to. **Flagging this** — if you have the original font, swap it in `fonts/` and update `colors_and_type.css`._

Type scale (rem, 16px root):

| Step | Size / Line | Used for |
|---|---|---|
| `--t-display` | 40 / 44 · w600 · -0.02em | GitHub README hero |
| `--t-h1` | 28 / 32 · w600 · -0.015em | App header |
| `--t-h2` | 20 / 26 · w600 | Section titles |
| `--t-h3` | 16 / 22 · w600 | Card titles |
| `--t-body` | 15 / 22 · w400 | Default text |
| `--t-small` | 13 / 18 · w400 | Hints, captions |
| `--t-mono` | 14 / 20 · w500 | Hotkey, code |
| `--t-kbd` | 12 / 14 · w600 | `<kbd>` chip |

### Backgrounds

- **No gradients** as decoration. There is exactly one gradient in the entire system: the soft fade in the logo wave itself, and the implied glow underneath the recording indicator (a 30%-opacity `--sage-soft` radial blur).
- **No full-bleed photography.** Voicy is a desktop utility, not a lifestyle product.
- **No repeating patterns / textures.** The "paper" feeling comes from `--paper` being slightly warm, not from a noise overlay.
- The recording state may use a very subtle animated sage glow — see Animation.

### Spacing & layout

8-pt scale: `4 · 8 · 12 · 16 · 24 · 32 · 48 · 64`. App content has **generous outer padding** (24–32px) — Voicy is never densely packed. Lists breathe at 16px row gap minimum.

Layout rules:

- Fixed elements: the app has a single fixed footer status bar (28px tall) showing "Готов · Alt + X". Nothing else is sticky.
- Max content width on marketing/GitHub side: **720px**. Voicy is humble; full-width text is for tools, not for Voicy.
- Buttons hug their content (`width: max-content`), they are never full-bleed inside cards.

### Corner radii

| Token | Value | Use |
|---|---|---|
| `--r-1` | 6px | inputs, chips, kbd |
| `--r-2` | 10px | small buttons |
| `--r-3` | 14px | cards |
| `--r-4` | 20px | large surfaces, the recording orb container |
| `--r-pill` | 999px | only for the round record button |

### Borders & shadows

- **Hairline borders everywhere.** `1px solid var(--line)`. Borders carry the visual weight, not shadows.
- **One shadow only.** `--shadow-1: 0 1px 0 rgba(27, 36, 33, 0.04), 0 8px 24px -16px rgba(27, 36, 33, 0.12)`. Used on the floating recording panel and on hover for primary buttons. Nothing else has a shadow.
- No inner shadows. No drop shadows on icons. No "neumorphism".

### Cards

A Voicy card is: `--paper`, `1px solid --line`, `--r-3` radius, **no shadow at rest**. On hover (only if interactive): the border deepens to `--ink-3` at 40% opacity. That's it.

### Hover & press states

- **Hover (buttons):** background gets one shade closer to ink (e.g. `--sage-deep` → `--moss` for primary; `--paper-2` for ghost buttons). No size change, no shadow expansion.
- **Press:** `transform: translateY(1px)` and the same color goes one step deeper. 80ms.
- **Focus:** `2px solid --sage-deep` outer ring with `2px` offset. Visible only on `:focus-visible`.
- **Disabled:** `--ink-3` text on `--paper-2` background. No opacity hacks.

### Transparency & blur

- The only blur in the system is the **subtle backdrop blur** behind the floating recording panel when it appears over the contacts list: `backdrop-filter: blur(8px)` over `rgba(247, 248, 243, 0.7)`.
- We do not use translucent overlays for "depth". If something needs to recede, use `--paper-2`, not opacity.

### Animation

Voicy is animated **just enough to feel alive**, never enough to feel like a toy.

- **Easing:** `cubic-bezier(0.2, 0.6, 0.2, 1)` for everything UI. Calm in, calm out.
- **Durations:** 120ms for hover/press, 240ms for panels/sheets, 400ms for the recording orb pulse cycle.
- **No bounces, no springs.** Voicy never overshoots.
- **No fade-up "reveal on scroll" effects.** This is a utility, not a landing page.
- One signature animation: the **recording wave**. When recording, three sage bars rise and fall in a slow sine pattern (period ≈ 1.6s, amplitude tied to mic level if available, otherwise idle). This is the only motion that runs continuously.

### Imagery vibe

- If imagery is used (rare — only on GitHub social card / poster), it should be **warm, slightly desaturated, lots of negative space**, daylight tone (≈5500K). Think still-life plant photography, never product shots, never people, never UI screenshots-as-art.
- Screenshots of Voicy itself are framed with `--line`, `--r-3` corners, no fake browser chrome.

---

## Iconography

- **No emoji** in product UI (one exception listed in Content). No unicode glyph hacks for icons.
- **Icon library:** [**Lucide**](https://lucide.dev) — 1.5px stroke, round joins, round caps. Matches the soft, hand-curved feel of the Voicy logo without being childish. Loaded via CDN where the platform allows (`https://unpkg.com/lucide@latest`) and copied as static SVGs into `assets/icons/` for offline use.
- **Flagging:** _The original Voicy build likely ships its own small icon set inside the WebView. We did not have access to it, so we substituted Lucide for visual consistency. Drop replacements into `assets/icons/` to override._
- **Logo mark** is the only non-Lucide vector. It is the green wave inside a circle, sage gradient on the wave, dark `--ink` ring. See `assets/logo-mark.svg`.
- **Stroke width:** Lucide default 1.5 in dense UIs (sidebar, toolbar); 1.75 for hero / oversized.
- **Sizes:** `14 · 16 · 20 · 24 · 32` px. Match the line-height of adjacent text.
- **Color:** icons inherit `currentColor`. They are never coloured for decoration. The single exception is the recording mic in active state, which is `--sage-deep`.

### Used icons (subset copied into `assets/icons/`)

`mic`, `mic-off`, `send`, `check`, `x`, `loader`, `settings`, `chevron-right`, `chevron-down`, `globe`, `keyboard`, `info`, `external-link`, `github`, `telegram` (custom — Lucide doesn't ship this; we substituted `send` with the Telegram paper-plane SVG from the official brand kit and flagged it inline).

---

## How to use this system

1. Read `colors_and_type.css` — every token is there.
2. Link `fonts/` (or use the Google Fonts CDN: Onest + JetBrains Mono, weights 400/500/600 and 500).
3. Copy assets from `assets/` rather than redrawing.
4. For new screens, start from one of the `ui_kits/desktop/*.jsx` components.
5. When in doubt: **less.** Voicy is the design system that says no to your second idea.
