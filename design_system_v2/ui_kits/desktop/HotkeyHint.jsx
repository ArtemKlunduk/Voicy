// HotkeyHint.jsx — bottom prompt + status footer
function HotkeyHint({ status, model, language, onTrigger, disabled }) {
  return (
    <div style={{
      borderTop: '1px solid var(--line)',
      flex: 'none',
      background: 'var(--paper)',
    }}>
      {/* The big hotkey prompt */}
      <div
        data-rec-trigger="1"
        onClick={!disabled ? onTrigger : undefined}
        style={{
          display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 10,
          padding: '16px 20px 14px',
          cursor: disabled ? 'not-allowed' : 'pointer',
          color: disabled ? 'var(--ink-3)' : 'var(--ink-2)',
          fontSize: 14,
          userSelect: 'none',
        }}
        onMouseEnter={e => { if (!disabled) e.currentTarget.style.color = 'var(--ink)'; }}
        onMouseLeave={e => { if (!disabled) e.currentTarget.style.color = 'var(--ink-2)'; }}
      >
        <Kbd>Alt</Kbd>
        <span style={{ color: 'var(--ink-3)' }}>+</span>
        <Kbd>X</Kbd>
        <span style={{ whiteSpace: 'nowrap' }}>&nbsp;и говори</span>
      </div>

      {/* Tiny status line */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '6px 16px 10px',
        fontSize: 11, color: 'var(--ink-3)',
        fontFamily: 'var(--font-mono)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{
            display: 'inline-block', width: 6, height: 6, borderRadius: 999,
            background: status === 'error' ? 'var(--warn)' : 'var(--sage-deep)',
          }}/>
          {status === 'error' ? 'нет связи с Telegram' : 'готов'}
        </div>
        <div>{model} · {language}</div>
      </div>
    </div>
  );
}

function Kbd({ children }) {
  return (
    <span style={{
      fontFamily: 'var(--font-mono)', fontSize: 12, fontWeight: 600,
      padding: '3px 6px', borderRadius: 6,
      border: '1px solid var(--line)', background: 'var(--paper)', color: 'var(--ink)',
      boxShadow: '0 1px 0 var(--line)',
      display: 'inline-flex', alignItems: 'center',
    }}>{children}</span>
  );
}

window.HotkeyHint = HotkeyHint;
window.Kbd = Kbd;
