// Window.jsx — decorative Windows-11-ish window chrome for the preview.
// Not a real OS frame; just enough so the kit reads as a desktop utility.
function Window({ children, width = 460, height = 640 }) {
  const windowStyles = {
    outer: {
      width, height,
      borderRadius: 10,
      background: 'var(--paper)',
      border: '1px solid var(--line-strong)',
      boxShadow: '0 2px 0 rgba(27,36,33,.03), 0 24px 60px -28px rgba(27,36,33,.28)',
      overflow: 'hidden',
      display: 'flex', flexDirection: 'column',
      fontFamily: 'var(--font-sans)',
      color: 'var(--ink)',
      position: 'relative',
    },
    titlebar: {
      height: 32, flex: 'none',
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '0 8px 0 12px',
      borderBottom: '1px solid var(--line)',
      background: 'var(--paper)',
      userSelect: 'none',
    },
    title: {
      fontSize: 12, color: 'var(--ink-3)', letterSpacing: 0.2,
      display: 'flex', alignItems: 'center', gap: 6,
    },
    btns: { display: 'flex', alignItems: 'center', gap: 0 },
    btn: {
      width: 36, height: 26, display: 'grid', placeItems: 'center',
      color: 'var(--ink-3)', cursor: 'default', borderRadius: 4,
    },
    inner: { flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', background: 'var(--paper)' },
  };
  return (
    <div style={windowStyles.outer}>
      <div style={windowStyles.titlebar}>
        <div style={windowStyles.title}>
          <img src="../../assets/logo-mark.svg" width="14" height="14" alt="" />
          Voicy
        </div>
        <div style={windowStyles.btns}>
          <div style={windowStyles.btn}>
            <svg viewBox="0 0 10 10" width="10" height="10"><path d="M2 5 H8" stroke="currentColor" strokeWidth="1" fill="none"/></svg>
          </div>
          <div style={windowStyles.btn}>
            <svg viewBox="0 0 10 10" width="10" height="10"><rect x="2" y="2" width="6" height="6" stroke="currentColor" strokeWidth="1" fill="none"/></svg>
          </div>
          <div style={{ ...windowStyles.btn, color: 'var(--ink-2)' }}>
            <svg viewBox="0 0 10 10" width="10" height="10"><path d="M2 2 L8 8 M8 2 L2 8" stroke="currentColor" strokeWidth="1" fill="none"/></svg>
          </div>
        </div>
      </div>
      <div style={windowStyles.inner}>{children}</div>
    </div>
  );
}
window.Window = Window;
