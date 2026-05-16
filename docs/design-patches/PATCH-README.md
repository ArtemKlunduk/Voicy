# Voicy — readability patch (для Claude Code)

В тёмной теме все «зелёные» элементы UI используют один и тот же приём:
pastel-sage заливка + pastel-sage текст. Контраст ~2.5 — нечитаемо.

Эта порция CSS лечит проблему системно. Внутри 6 классов на разный
функциональный уровень (callout, metadata, chip, button, status, select).

## Что внутри

| Класс | Где применять | Контраст |
|---|---|---|
| `.vy-badge.vy-badge--solid` | `Recommended`, `Active`, любые state-callout-ы | 9.4 |
| `.vy-badge.vy-badge--soft` | `Multilingual`, `English only`, `cached` — метаданные | 11.2 |
| `.vy-badge.vy-badge--ghost` | `Beta`, `Локально` — тихие outline-метки | 5.8 |
| `.vy-chip` (+ `.vy-chip__x`) | алиас-теги на строках контактов | 9.4 |
| `.vy-btn--primary` | `QR`, `↓ из Telegram` — основная зелёная кнопка | 9.4 |
| `.vy-pill--status` | `online`, `recording`, `sent` — статусные пилюли | 9.4 |
| `.vy-active-now` | мелкое «active now» внутри dropdown-ов | ≥ 7 |
| `.vy-select` (v3) | кастомный dropdown для settings rows | 9.4 |

## Установка

1. Кинь `voicy-style-patch.css` в любое место (например `src/styles/`).
2. Импортируй в корневой стиль / `main.tsx`:
   ```ts
   import './styles/voicy-style-patch.css';
   ```
3. Поменяй разметку проблемных мест на классы выше.

Если у тебя сейчас классы вроде `.btn-green`, `.tag-recommended`, `.alias-chip` —
проще всего открыть их и заменить весь блок стилей на `@extend .vy-btn--primary` /
скопировать содержимое нужного `.vy-*`-класса. Никакого JS/React не трогаем.

## Что под капотом

Два правила, которые делают всю работу:

- **На solid sage фоне** (`#A8C8A0`) текст всегда `#0E1614`, вес ≥ 600.
- **На sage-soft фоне** (`#D6E6CF`) текст всегда `#2F4A35`, вес ≥ 600.

Pastel-on-pastel запрещён вообще, нигде.

## Кейсы из скриншотов

- `Recommended` (Models tab, на выбранной карточке) → `vy-badge--solid`
- `ACTIVE` (Models tab, правый верх карточки) → `vy-badge--solid.upper`
- `Multilingual`, `English only` (метки модели) → `vy-badge--soft`
- `cached` рядом с AI-model picker'ом → `vy-badge--soft` с иконкой галочки
- `Active now` (внутри dropdown) → `vy-active-now`
- `тимофей коко ×`, `тима ×` (Telegram → Contacts) → `vy-chip` + `vy-chip__x`
- `QR`, `↓ из Telegram` (Telegram → Account) → `vy-btn--primary`
- `online` (Telegram → Account верхний правый угол) → `vy-pill--status`
- `+ добавить`, `Phone` (вторичные кнопки) — **не трогаем**, они уже outline
  и читаются нормально на тёмном.

## v3 — кастомный dropdown `.vy-select`

Native `<select>` на тёмной теме рисуется системным стилем (серая
подложка + чёрные option-ы), что выпадает из brand palette. Замена:

* **Триггер** — настоящий контрол: `1px solid --vy-line-dark`, скруглённый,
  при hover → sage-deep граница, при open → sage граница + развёрнутый шеврон
* Иконка ✓ внутри триггера тоже sage (тонкая, не дублирует значение)
* Меню сидит на тёмно-зелёной `#1B2723` поверхности, не на серой системной
* Айтемы со скруглёнными краями, имя + размер двумя строчками
  (метаданные mono-шрифтом)
* Selected-айтем: тонкая sage-подложка + sage-текст + чек справа
* `cached` рядом с триггером — `vy-badge--soft` с иконкой галочки

Разметка:

```html
<div class="vy-select">
  <button class="vy-select__trigger" aria-expanded="true">
    Qwen 2.5 0.5B · 400 MB
    <span class="check"><svg>…✓…</svg></span>
    <svg class="chev"><polyline points="6 9 12 15 18 9"/></svg>
  </button>
  <div class="vy-select__menu">
    <div class="vy-select__item" aria-selected="true">
      <span>Qwen 2.5 0.5B
        <span class="vy-select__meta">400 MB</span>
      </span>
      <span class="check"><svg>…✓…</svg></span>
    </div>
    <div class="vy-select__item">
      <span>Llama 3.2 1B
        <span class="vy-select__meta">800 MB · скачается при выборе</span>
      </span>
    </div>
  </div>
</div>
```

**JS-кубик** (минимальный):
```js
const trigger = document.querySelector('.vy-select__trigger');
const menu = document.querySelector('.vy-select__menu');

trigger.addEventListener('click', e => {
  e.stopPropagation();
  const open = menu.hidden === false;
  menu.hidden = open;
  trigger.setAttribute('aria-expanded', !open);
});

menu.addEventListener('click', e => {
  const item = e.target.closest('.vy-select__item');
  if (!item) return;
  // do something with item.dataset.id
  menu.hidden = true;
  trigger.setAttribute('aria-expanded', false);
});

document.addEventListener('click', e => {
  if (!e.target.closest('.vy-select')) {
    menu.hidden = true;
    trigger.setAttribute('aria-expanded', false);
  }
});
document.addEventListener('keydown', e => {
  if (e.key === 'Escape') {
    menu.hidden = true;
    trigger.setAttribute('aria-expanded', false);
  }
});
```

## Что не входит в патч

- Тёмная цветовая схема в целом — патч это не трогает.
- Иконки в кнопках/пилюлях рисуй с `stroke="currentColor"` и stroke-width 2+ —
  иначе на solid-зелёном будут «тонкими» и потеряются.
- Если используешь Tailwind, можно переписать всё в
  `bg-[#A8C8A0] text-[#0E1614] font-semibold` — скажи.

— Voicy Design
