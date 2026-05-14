// ContactList.jsx — search field + scrollable rows
function ContactList({ contacts, selectedId, onSelect, query, onQuery }) {
  const filtered = contacts.filter(c =>
    !query || c.name.toLowerCase().includes(query.toLowerCase()) || c.handle.includes(query.toLowerCase())
  );
  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      <div style={{ padding: '12px 16px 8px' }}>
        <SearchField value={query} onChange={onQuery} />
      </div>
      <div style={{ flex: 1, overflowY: 'auto', padding: '4px 8px 8px' }}>
        {filtered.length === 0 ? (
          <div style={{ padding: '24px 12px', color: 'var(--ink-3)', fontSize: 13, textAlign: 'center' }}>
            Никого не нашёл
          </div>
        ) : filtered.map(c => (
          <ContactRow key={c.id} contact={c} selected={c.id === selectedId} onClick={() => onSelect(c.id)} />
        ))}
      </div>
    </div>
  );
}

function SearchField({ value, onChange }) {
  const [focus, setFocus] = React.useState(false);
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
      padding: '8px 12px',
      borderRadius: 10,
      border: '1px solid ' + (focus ? 'var(--sage-deep)' : 'var(--line)'),
      background: 'var(--paper)',
      boxShadow: focus ? '0 0 0 3px color-mix(in oklab, var(--sage-deep) 18%, transparent)' : 'none',
      transition: 'border-color 120ms, box-shadow 120ms',
    }}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--ink-3)" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>
      </svg>
      <input
        value={value}
        onChange={e => onChange(e.target.value)}
        onFocus={() => setFocus(true)}
        onBlur={() => setFocus(false)}
        placeholder="Найти контакт…"
        style={{ all: 'unset', flex: 1, fontSize: 14, color: 'var(--ink)' }}
      />
    </div>
  );
}

function ContactRow({ contact, selected, onClick }) {
  const [hover, setHover] = React.useState(false);
  const bg = selected ? 'var(--sage-soft)' : hover ? 'var(--paper-2)' : 'transparent';
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex', alignItems: 'center', gap: 12,
        padding: '8px 10px', borderRadius: 10, cursor: 'pointer',
        background: bg, transition: 'background 120ms cubic-bezier(.2,.6,.2,1)',
      }}
    >
      <div style={{
        width: 32, height: 32, borderRadius: 999,
        background: 'var(--sage-soft)', color: 'var(--moss)',
        display: 'grid', placeItems: 'center',
        fontSize: 13, fontWeight: 600, flex: 'none',
      }}>{contact.initial}</div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 14, fontWeight: 500, color: selected ? 'var(--moss)' : 'var(--ink)' }}>{contact.name}</div>
        <div style={{ fontSize: 12, color: 'var(--ink-3)', fontFamily: 'var(--font-mono)' }}>{contact.handle}</div>
      </div>
      {selected && (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--sage-deep)" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="20 6 9 17 4 12"/>
        </svg>
      )}
    </div>
  );
}

window.ContactList = ContactList;
