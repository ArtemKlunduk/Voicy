// SettingsScreen.jsx — hotkey, language, model, autostart, sound, about.
function SettingsScreen({ onBack, settings, setSettings }) {
  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
      <div style={{
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '12px 12px 12px 8px', borderBottom: '1px solid var(--line)',
      }}>
        <button
          onClick={onBack}
          style={{
            all: 'unset', cursor: 'pointer', padding: 6, borderRadius: 8,
            color: 'var(--ink-2)', display: 'inline-flex',
          }}
          onMouseEnter={e => e.currentTarget.style.background = 'var(--paper-2)'}
          onMouseLeave={e => e.currentTarget.style.background = 'transparent'}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
            <line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 19 5 12 12 5"/>
          </svg>
        </button>
        <div style={{ fontSize: 16, fontWeight: 600 }}>Настройки</div>
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: '14px 18px 22px' }}>
        <Section title="Сочетание клавиш">
          <Row label="Активация">
            <HotkeyEditor value={settings.hotkey} onChange={v => setSettings({ ...settings, hotkey: v })}/>
          </Row>
        </Section>

        <Section title="Распознавание">
          <Row label="Модель"><Select value={settings.model} options={['parakeet-v3', 'whisper-tiny', 'whisper-base']} onChange={v => setSettings({ ...settings, model: v })}/></Row>
          <Row label="Язык"><Select value={settings.language} options={['ru', 'en', 'auto']} onChange={v => setSettings({ ...settings, language: v })}/></Row>
        </Section>

        <Section title="Поведение">
          <Row label="Запуск с системой"><Switch on={settings.autostart} onChange={v => setSettings({ ...settings, autostart: v })}/></Row>
          <Row label="Звук подтверждения"><Switch on={settings.sound} onChange={v => setSettings({ ...settings, sound: v })}/></Row>
          <Row label="Отправлять автоматически"><Switch on={settings.autoSend} onChange={v => setSettings({ ...settings, autoSend: v })}/></Row>
        </Section>

        <Section title="О программе">
          <div style={{ fontSize: 13, color: 'var(--ink-2)', lineHeight: '20px' }}>
            Voicy — маленький голосовой помощник. Бесплатно. Открытый исходный код.
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
            <LinkBtn icon="github">GitHub</LinkBtn>
            <LinkBtn icon="info">Версия 0.4.1</LinkBtn>
          </div>
        </Section>
      </div>
    </div>
  );
}

function Section({ title, children }) {
  return (
    <div style={{ marginBottom: 20 }}>
      <div style={{
        fontSize: 11, letterSpacing: '0.06em', textTransform: 'uppercase',
        color: 'var(--ink-3)', fontWeight: 500, margin: '0 4px 8px',
      }}>{title}</div>
      <div style={{
        border: '1px solid var(--line)', borderRadius: 14, background: 'var(--paper)',
        overflow: 'hidden',
      }}>{children}</div>
    </div>
  );
}

function Row({ label, children }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '11px 14px',
      borderBottom: '1px solid var(--line)',
      fontSize: 14, color: 'var(--ink)',
      gap: 12,
    }} className="vy-row">
      <span style={{ whiteSpace: 'nowrap' }}>{label}</span>
      <span>{children}</span>
    </div>
  );
}

function HotkeyEditor({ value, onChange }) {
  // value: { mods: ['alt'], key: 'X' }
  return (
    <div style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
      {value.mods.map((m, i) => (
        <span key={i} style={kbdStyle}>{m === 'alt' ? 'Alt' : m === 'ctrl' ? 'Ctrl' : m === 'shift' ? 'Shift' : 'Win'}</span>
      ))}
      <span style={{ color: 'var(--ink-3)' }}>+</span>
      <span style={kbdStyle}>{value.key}</span>
    </div>
  );
}
const kbdStyle = {
  fontFamily: 'var(--font-mono)', fontSize: 12, fontWeight: 600,
  padding: '3px 6px', borderRadius: 6,
  border: '1px solid var(--line)', background: 'var(--paper-2)', color: 'var(--ink)',
};

function Select({ value, options, onChange }) {
  return (
    <select
      value={value}
      onChange={e => onChange(e.target.value)}
      style={{
        all: 'unset', fontFamily: 'var(--font-mono)', fontSize: 13,
        background: 'var(--paper-2)', color: 'var(--ink)',
        padding: '4px 26px 4px 10px', borderRadius: 8,
        border: '1px solid var(--line)', cursor: 'pointer',
        backgroundImage: 'url("data:image/svg+xml;utf8,<svg xmlns=%27http://www.w3.org/2000/svg%27 viewBox=%270 0 24 24%27 fill=%27none%27 stroke=%27%238A9690%27 stroke-width=%271.75%27 stroke-linecap=%27round%27 stroke-linejoin=%27round%27><polyline points=%276 9 12 15 18 9%27/></svg>")',
        backgroundRepeat: 'no-repeat',
        backgroundPosition: 'right 8px center',
        backgroundSize: '14px 14px',
      }}
    >
      {options.map(o => <option key={o} value={o}>{o}</option>)}
    </select>
  );
}

function Switch({ on, onChange }) {
  return (
    <span
      onClick={() => onChange(!on)}
      style={{
        width: 32, height: 18, borderRadius: 999,
        background: on ? 'var(--sage-deep)' : 'var(--line-strong)',
        position: 'relative', cursor: 'pointer',
        transition: 'background 120ms cubic-bezier(.2,.6,.2,1)',
        display: 'inline-block',
      }}
    >
      <span style={{
        position: 'absolute', top: 2, left: on ? 16 : 2,
        width: 14, height: 14, borderRadius: 999, background: '#fff',
        boxShadow: '0 1px 2px rgba(0,0,0,0.15)',
        transition: 'left 120ms cubic-bezier(.2,.6,.2,1)',
      }}/>
    </span>
  );
}

function LinkBtn({ icon, children }) {
  return (
    <a href="#" onClick={e => e.preventDefault()} style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      padding: '6px 10px', borderRadius: 8,
      border: '1px solid var(--line)', background: 'var(--paper)',
      color: 'var(--ink-2)', fontSize: 12, textDecoration: 'none',
    }}>
      <img src={`../../assets/icons/${icon}.svg`} width="14" height="14" alt=""
           style={{ filter: 'brightness(0) saturate(100%) invert(35%) sepia(7%) saturate(345%) hue-rotate(118deg) brightness(95%) contrast(91%)' }}/>
      {children}
    </a>
  );
}

// Remove the bottom border from the last row in each section
// (CSS shortcut via a style tag in index.html)

window.SettingsScreen = SettingsScreen;
