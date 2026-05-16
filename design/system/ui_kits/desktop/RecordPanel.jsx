// RecordPanel.jsx — full-screen recording overlay inside the app window.
function RecordPanel({ contact, transcript, elapsed, onCancel, onSend, stage }) {
  // stage: 'recording' | 'sending' | 'sent'
  return (
    <div style={{
      position: 'absolute', inset: 32, // sits over the contact list, with a small inset
      zIndex: 1000,
      background: 'var(--paper)',
      borderRadius: 18,
      border: '1px solid var(--line)',
      boxShadow: '0 1px 0 rgba(27,36,33,.04), 0 12px 36px -20px rgba(27,36,33,.25)',
      padding: '20px 22px',
      display: 'flex', flexDirection: 'column', gap: 18,
      opacity: 1,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <div style={{
          width: 28, height: 28, borderRadius: 999,
          background: 'var(--sage-soft)', color: 'var(--moss)',
          display: 'grid', placeItems: 'center', fontSize: 12, fontWeight: 600,
        }}>{contact.initial}</div>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13, color: 'var(--ink-3)' }}>
            {stage === 'sent' ? 'Отправлено' : stage === 'sending' ? 'Отправляю…' : 'Кому'}
          </div>
          <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--ink)' }}>{contact.name}</div>
        </div>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--ink-3)' }}>
          {formatTime(elapsed)}
        </div>
      </div>

      {/* Big wave + orb */}
      <div style={{
        display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 18,
        padding: '14px 0 4px',
      }}>
        <Orb stage={stage} />
        <Wave active={stage === 'recording'} />
      </div>

      {/* Live transcript */}
      <div style={{
        flex: 1,
        background: 'var(--paper-2)',
        border: '1px solid var(--line)',
        borderRadius: 12,
        padding: '12px 14px',
        minHeight: 76,
        fontSize: 14, lineHeight: '22px', color: 'var(--ink)',
        overflow: 'auto',
      }}>
        {transcript ? (
          <span>{transcript}{stage === 'recording' && <span style={{ opacity: 0.4 }}>▌</span>}</span>
        ) : (
          <span style={{ color: 'var(--ink-3)' }}>Говори, я слушаю…</span>
        )}
      </div>

      {/* Actions */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10 }}>
        <button onClick={onCancel} style={panelBtn('ghost')}>
          {stage === 'sent' ? 'Закрыть' : 'Отмена'}
        </button>
        {stage !== 'sent' && (
          <button
            onClick={onSend}
            disabled={!transcript || stage === 'sending'}
            style={panelBtn('primary', !transcript || stage === 'sending')}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
              <path d="m22 2-7 20-4-9-9-4 20-7z"/>
            </svg>
            {stage === 'sending' ? 'Отправляю…' : 'Отправить'}
          </button>
        )}
      </div>
    </div>
  );
}

function Orb({ stage }) {
  const orbStyles = {
    base: {
      width: 84, height: 84, borderRadius: 999,
      display: 'grid', placeItems: 'center',
      transition: 'all 240ms cubic-bezier(.2,.6,.2,1)',
      position: 'relative',
    },
    recording: { background: 'var(--sage-soft)', color: 'var(--moss)' },
    sending:   { background: 'var(--sage-deep)', color: '#fff' },
    sent:      { background: 'var(--sage-deep)', color: '#fff' },
  };
  const halo = stage === 'recording' ? (
    <div style={{
      position: 'absolute', inset: -14, borderRadius: 999,
      background: 'var(--sage-soft)', opacity: 0.55, filter: 'blur(12px)',
      zIndex: -1,
      animation: 'vy-halo 1.6s cubic-bezier(.2,.6,.2,1) infinite',
    }}/>
  ) : null;
  return (
    <div style={{ ...orbStyles.base, ...orbStyles[stage] }}>
      {halo}
      {stage === 'recording' && (
        <svg width="32" height="32" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="3"/></svg>
      )}
      {stage === 'sending' && (
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ animation: 'vy-spin 1s linear infinite' }}>
          <circle cx="12" cy="12" r="9" strokeOpacity=".25"/>
          <path d="M21 12a9 9 0 0 0-9-9"/>
        </svg>
      )}
      {stage === 'sent' && (
        <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="20 6 9 17 4 12"/>
        </svg>
      )}
    </div>
  );
}

function Wave({ active }) {
  // Variant 05 · Halo rings — three rings expand from the orb, almost-imperceptible motion.
  if (!active) {
    return <div style={{ height: 12 }}/>;
  }
  return (
    <div style={{
      position: 'absolute',
      left: '50%', top: 'calc(50% - 8px)',
      transform: 'translate(-50%, -50%)',
      width: 84, height: 84,
      pointerEvents: 'none',
      zIndex: 0,
    }}>
      <div style={ringStyle(0)}/>
      <div style={ringStyle(0.7)}/>
      <div style={ringStyle(1.4)}/>
    </div>
  );
}
const ringStyle = (delay) => ({
  position: 'absolute', inset: 0,
  borderRadius: 999,
  border: '1.5px solid var(--sage)',
  opacity: 0,
  animation: `vy-ring 2.2s cubic-bezier(.2,.6,.2,1) ${delay}s infinite`,
});

function panelBtn(variant, disabled) {
  const base = {
    all: 'unset', cursor: disabled ? 'not-allowed' : 'pointer',
    fontFamily: 'var(--font-sans)', fontSize: 14, fontWeight: 500,
    padding: '9px 14px', borderRadius: 10,
    display: 'inline-flex', alignItems: 'center', gap: 8,
    transition: 'background 120ms cubic-bezier(.2,.6,.2,1)',
    boxSizing: 'border-box',
  };
  if (variant === 'primary') {
    return { ...base, background: disabled ? 'var(--paper-2)' : 'var(--sage-deep)', color: disabled ? 'var(--ink-3)' : '#fff' };
  }
  return { ...base, background: 'transparent', color: 'var(--ink-2)' };
}

function formatTime(ms) {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

window.RecordPanel = RecordPanel;
