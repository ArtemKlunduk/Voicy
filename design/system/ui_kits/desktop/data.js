// Fake data for the Voicy click-through. Lives in window scope.
window.VOICY_CONTACTS = [
  { id: 'mom',   name: 'Мама',       handle: '@mama_voicy',   last: '2 часа назад', initial: 'М' },
  { id: 'dad',   name: 'Папа',       handle: '@dad_pavel_72', last: 'вчера',        initial: 'П' },
  { id: 'sister',name: 'Серёжа',     handle: '@seryozha',     last: '3 дня назад',  initial: 'С' },
  { id: 'work',  name: 'Юля',        handle: '@yulia.work',   last: '5 дней назад', initial: 'Ю' },
  { id: 'doc',   name: 'Доктор Лена',handle: '@dr_lena',      last: 'неделю назад', initial: 'Д' },
  { id: 'misha', name: 'Миша',       handle: '@misha_drug',   last: 'давно',        initial: 'М' },
];

// Fake transcripts shown progressively while "recording"
window.VOICY_FAKE_TRANSCRIPTS = [
  'Привет! Я уже выхожу, буду минут через двадцать.',
  'Не забудь купить хлеб и творог, пожалуйста.',
  'Позвоню тебе вечером, сейчас в дороге.',
  'Спасибо за подарок, очень понравился.',
  'Завтра встречаемся в восемь у метро.',
];
