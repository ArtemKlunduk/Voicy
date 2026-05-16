// ConnectScreen.jsx — first-run Telegram session connect.
// Two steps: phone → 5-digit code. Faked.
function ConnectScreen({ onConnected }) {
  const [step, setStep] = React.useState('phone'); // phone | code | working
  const [phone, setPhone] = React.useState('+7 ');
  const [code, setCode] = React.useState('');

  const submit = () => {
    if (step === 'phone' && phone.replace(/\D/g, '').length >= 10) {
      setStep('code');
    } else if (step === 'code' && code.length >= 4) {
      setStep('working');
      setTimeout(onConnected, 900);
    }
  };

  return (
    <div style={{
      flex: 1, display: 'flex', flexDirection: 'column',
      padding: '32px 28px 24px', gap: 22,
    }}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 16, marginTop: 12 }}>
        <img src="../../assets/logo-mark.svg" width="64" height="64" alt="" />
        <div style={{ fontSize: 22, fontWeight: 600, letterSpacing: '-0.015em', textAlign: 'center' }}>
          Подключи Telegram
        </div>
        <div style={{ fontSize: 13, color: 'var(--ink-2)', textAlign: 'center', maxWidth: 320, lineHeight: '20px' }}>
          Voicy будет отправлять расшифрованный голос в Telegram от твоего имени. Один раз вошёл — и забыл.
        </div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginTop: 8 }}>
        {step === 'phone' && (
          <Field label="Номер телефона" autoFocus value={phone} onChange={setPhone} onSubmit={submit} placeholder="+7 900 000 00 00" />
        )}
        {step === 'code' && (
          <>
            <div style={{ fontSize: 13, color: 'var(--ink-3)' }}>Код пришёл в Telegram на {phone}</div>
            <Field label="Код подтверждения" autoFocus value={code} onChange={v => setCode(v.replace(/\D/g, '').slice(0, 5))} onSubmit={submit} placeholder="12345" mono />
          </>
        )}
        {step === 'working' && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, color: 'var(--ink-2)', fontSize: 14 }}>
            <Spinner /> Соединяюсь…
          </div>
        )}
      </div>

      <div style={{ marginTop: 'auto', display: 'flex', flexDirection: 'column', gap: 10 }}>
        {step !== 'working' && (
          <button
            onClick={submit}
            disabled={step === 'phone' ? phone.replace(/\D/g, '').length < 10 : code.length < 4}
            style={{
              all: 'unset', cursor: 'pointer', textAlign: 'center',
              padding: '12px 16px', borderRadius: 10,
              background: 'var(--sage-deep)', color: '#fff',
              fontSize: 14, fontWeight: 500,
              opacity: (step === 'phone' ? phone.replace(/\D/g, '').length < 10 : code.length < 4) ? 0.5 : 1,
            }}
          >
            {step === 'phone' ? 'Получить код' : 'Войти'}
          </button>
        )}
        <div style={{ fontSize: 11, color: 'var(--ink-3)', textAlign: 'center' }}>
          Сессия Telegram хранится локально, в <span style={{ fontFamily: 'var(--font-mono)' }}>voicy_session.session</span>
        </div>
      </div>
    </div>
  );
}

function Field({ label, value, onChange, onSubmit, placeholder, autoFocus, mono }) {
  const [focus, setFocus] = React.useState(false);
  return (
    <div>
      <div style={{ fontSize: 12, color: 'var(--ink-3)', marginBottom: 6 }}>{label}</div>
      <input
        autoFocus={autoFocus}
        value={value}
        onChange={e => onChange(e.target.value)}
        onKeyDown={e => e.key === 'Enter' && onSubmit()}
        onFocus={() => setFocus(true)}
        onBlur={() => setFocus(false)}
        placeholder={placeholder}
        style={{
          all: 'unset', boxSizing: 'border-box', width: '100%',
          padding: '10px 14px', borderRadius: 10,
          border: '1px solid ' + (focus ? 'var(--sage-deep)' : 'var(--line)'),
          boxShadow: focus ? '0 0 0 3px color-mix(in oklab, var(--sage-deep) 18%, transparent)' : 'none',
          background: 'var(--paper)',
          fontSize: 15,
          fontFamily: mono ? 'var(--font-mono)' : 'var(--font-sans)',
          letterSpacing: mono ? '0.1em' : 'normal',
          color: 'var(--ink)',
          transition: 'border-color 120ms, box-shadow 120ms',
        }}
      />
    </div>
  );
}

function Spinner() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--sage-deep)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ animation: 'vy-spin 1s linear infinite' }}>
      <circle cx="12" cy="12" r="9" strokeOpacity=".25"/>
      <path d="M21 12a9 9 0 0 0-9-9"/>
    </svg>
  );
}

window.ConnectScreen = ConnectScreen;
