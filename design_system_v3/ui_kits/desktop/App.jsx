// App.jsx — top-level state machine for the Voicy desktop click-through.
function App() {
  // Screens: 'connect' | 'main' | 'settings'.
  // Default to 'main' so the kit preview is useful at a glance.
  // Use the bottom switcher in index.html to navigate.
  const [screen, setScreen] = React.useState(
    new URLSearchParams(location.search).get('screen') || 'main'
  );
  // Modal recording state, lives over 'main'
  const [recState, setRecState] = React.useState(null); // null | { stage, transcript, elapsedMs, startedAt }
  const [selectedId, setSelectedId] = React.useState('mom');
  const [query, setQuery] = React.useState('');
  const [toast, setToast] = React.useState(null);

  const [settings, setSettings] = React.useState({
    hotkey: { mods: ['alt'], key: 'X' },
    model: 'parakeet-v3',
    language: 'ru',
    autostart: true,
    sound: false,
    autoSend: false,
  });

  const selected = window.VOICY_CONTACTS.find(c => c.id === selectedId);

  // Drive fake recording: build up transcript over time.
  React.useEffect(() => {
    if (!recState || recState.stage !== 'recording') return;
    const target = recState.target;
    let i = recState.transcript.length;
    const tick = setInterval(() => {
      i = Math.min(i + 2, target.length);
      const now = Date.now();
      setRecState(rs => rs && rs.stage === 'recording' ? {
        ...rs,
        transcript: target.slice(0, i),
        elapsedMs: now - rs.startedAt,
      } : rs);
      if (i >= target.length) clearInterval(tick);
    }, 60);
    return () => clearInterval(tick);
  }, [recState && recState.stage]);

  // Global hotkey listener (so user can press Alt+X for real)
  React.useEffect(() => {
    const handler = (e) => {
      if (e.altKey && (e.key === 'x' || e.key === 'X' || e.code === 'KeyX')) {
        e.preventDefault();
        if (screen === 'main' && !recState && selected) {
          startRecording();
        }
      }
      if (e.key === 'Escape' && recState) {
        setRecState(null);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [screen, recState, selected]);

  const startRecording = () => {
    const target = window.VOICY_FAKE_TRANSCRIPTS[Math.floor(Math.random() * window.VOICY_FAKE_TRANSCRIPTS.length)];
    setRecState({ stage: 'recording', transcript: '', elapsedMs: 0, target, startedAt: Date.now() });
  };

  const send = () => {
    setRecState(rs => rs ? { ...rs, stage: 'sending' } : rs);
    setTimeout(() => {
      setRecState(rs => rs ? { ...rs, stage: 'sent' } : rs);
      setTimeout(() => {
        setRecState(null);
        setToast({ kind: 'ok', title: 'Отправлено', sub: `${selected.name} · ${Math.round((recState ? recState.elapsedMs : 1500) / 1000)} с голоса` });
        setTimeout(() => setToast(null), 2400);
      }, 700);
    }, 800);
  };

  // Header status
  const headerStatus =
    !recState ? 'ready' :
    recState.stage === 'recording' ? 'recording' :
    recState.stage === 'sending' ? 'sending' :
    'ready';

  return (
    <Window>
      {screen === 'connect' && (
        <ConnectScreen onConnected={() => setScreen('main')} />
      )}

      {screen === 'main' && (
        <>
          <Header status={headerStatus} onSettings={() => setScreen('settings')} />
          <ContactList
            contacts={window.VOICY_CONTACTS}
            selectedId={selectedId}
            onSelect={setSelectedId}
            query={query}
            onQuery={setQuery}
          />
          {recState && (
            <RecordPanel
              contact={selected}
              transcript={recState.transcript}
              elapsed={recState.elapsedMs}
              stage={recState.stage}
              onCancel={() => setRecState(null)}
              onSend={send}
            />
          )}
          <HotkeyHint
            status="ready"
            model={settings.model}
            language={settings.language}
            disabled={!selected || !!recState}
            onTrigger={startRecording}
          />
          {toast && <ToastView toast={toast} />}
        </>
      )}

      {screen === 'settings' && (
        <SettingsScreen
          onBack={() => setScreen('main')}
          settings={settings}
          setSettings={setSettings}
        />
      )}
    </Window>
  );
}

function ToastView({ toast }) {
  return (
    <div style={{
      position: 'absolute', left: 16, right: 16, bottom: 84,
      display: 'flex', alignItems: 'center', gap: 10,
      padding: '10px 14px', borderRadius: 12,
      background: 'var(--paper)', border: '1px solid var(--line)',
      boxShadow: '0 1px 0 rgba(27,36,33,.04), 0 8px 24px -16px rgba(27,36,33,.18)',
      animation: 'vy-toast 240ms cubic-bezier(.2,.6,.2,1) backwards',
      zIndex: 50,
    }}>
      <div style={{
        width: 26, height: 26, borderRadius: 999, flex: 'none',
        background: 'var(--sage-soft)', color: 'var(--moss)',
        display: 'grid', placeItems: 'center',
      }}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="20 6 9 17 4 12"/>
        </svg>
      </div>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 13, fontWeight: 600 }}>{toast.title}</div>
        <div style={{ fontSize: 11, color: 'var(--ink-3)' }}>{toast.sub}</div>
      </div>
    </div>
  );
}

window.VoicyApp = App;
