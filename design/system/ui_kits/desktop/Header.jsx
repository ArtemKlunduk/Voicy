// Header.jsx — logo + status dot + settings cog
function Header({ status = 'ready', onSettings }) {
  const headerStyles = {
    bar: {
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '16px 20px 14px',
      borderBottom: '1px solid var(--line)',
      flex: 'none',
    },
    left: { display: 'flex', alignItems: 'center', gap: 10 },
    title: { fontSize: 17, fontWeight: 600, letterSpacing: '-0.01em' },
    right: { display: 'flex', alignItems: 'center', gap: 6 },
    cog: {
      width: 32, height: 32, display: 'grid', placeItems: 'center',
      borderRadius: 8, cursor: 'pointer', color: 'var(--ink-2)',
      transition: 'background 120ms cubic-bezier(.2,.6,.2,1)',
    },
  };
  const dotColor =
    status === 'recording' ? 'var(--sage)' :
    status === 'error'     ? 'var(--warn)' :
    status === 'sending'   ? 'var(--sage)' :
    'var(--sage-deep)';
  const dotPulse = (status === 'recording' || status === 'sending');
  return (
    <div style={headerStyles.bar}>
      <div style={headerStyles.left}>
        <img src="../../assets/logo-mark.svg" width="28" height="28" alt="Voicy" />
        <div style={headerStyles.title}>Voicy</div>
      </div>
      <div style={headerStyles.right}>
        <span
          title={status}
          style={{
            display: 'inline-block', width: 8, height: 8, borderRadius: 999,
            background: dotColor, marginRight: 8,
            animation: dotPulse ? 'vy-pulse 1.4s cubic-bezier(.2,.6,.2,1) infinite' : 'none',
          }}
        />
        <div
          style={headerStyles.cog}
          onClick={onSettings}
          onMouseEnter={e => e.currentTarget.style.background = 'var(--paper-2)'}
          onMouseLeave={e => e.currentTarget.style.background = 'transparent'}
        >
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3"/>
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
          </svg>
        </div>
      </div>
    </div>
  );
}
window.Header = Header;
