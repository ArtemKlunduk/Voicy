//! contacts.txt парсер: `123456 - имя1, имя2, имя3` → {alias.lower(): uid}
//! + парсер голосовой команды «напиши/отправь имя текст» с fuzzy match.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub type Contacts = HashMap<String, i64>;

/// Sentinel uid возвращаемый parse_command для «себе/saved messages».
/// Вызывающий код должен подставить реальный user_id залогиненного аккаунта
/// (`client.get_me().await?.id()`). Выбран i64::MIN — реальный Telegram uid
/// никогда не отрицательный.
pub const SELF_SENTINEL_UID: i64 = i64::MIN;

/// Structured-вид контакта для UI: имя + список алиасов + uid.
/// Первый алиас в файле — это «display name», остальные — голосовые синонимы.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub uid: i64,
    pub name: String,
    pub aliases: Vec<String>,
}

fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("voicy"))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_path() -> PathBuf {
    let base = app_data_dir();
    std::fs::create_dir_all(&base).ok();
    base.join("contacts.txt")
}

pub fn load(path: &Path) -> Contacts {
    let mut out = HashMap::new();
    // Сначала кладём точные алиасы (приоритетнее стемов).
    for c in load_structured(path) {
        for a in &c.aliases {
            out.insert(a.to_lowercase(), c.uid);
        }
        out.insert(c.name.to_lowercase(), c.uid);
    }
    // Потом стемы — но только если стем НЕ конфликтует с уже существующим
    // алиасом другого контакта. Это даёт «тиме»→«тим»→uid тимы без перезаписи
    // точных совпадений.
    let snapshot: Vec<(String, i64)> = out.iter().map(|(k, &v)| (k.clone(), v)).collect();
    let mut conflicts: std::collections::HashSet<String> = Default::default();
    for (alias, uid) in &snapshot {
        let stem = russian_stem(alias);
        if stem == *alias { continue; }
        match out.get(&stem) {
            Some(existing) if *existing != *uid => { conflicts.insert(stem); }
            Some(_) => {}
            None => { out.insert(stem, *uid); }
        }
    }
    // Уберём те стемы, что оказались общими для разных контактов — неоднозначны.
    for s in conflicts {
        out.remove(&s);
    }
    out
}

/// Грубый стем русских имён: режет типичные падежные окончания.
/// «тима/тиме/тиму/тимы» → «тим». «маша/маше/машу» → «маш».
/// Не трогает слова короче 4 символов и неизвестные окончания.
pub fn russian_stem(s: &str) -> String {
    // Порядок важен: длинные окончания проверяем первыми.
    const ENDINGS: &[&str] = &[
        "ями", "ами", "иям", "иях",
        "ях", "ах", "ой", "ей", "ою", "ею",
        "ом", "ем", "ам", "ям", "ого", "его",
        "ы", "и", "у", "ю", "а", "я", "е",
    ];
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    if n < 4 {
        return s.to_string();
    }
    for end in ENDINGS {
        let end_chars: Vec<char> = end.chars().collect();
        let el = end_chars.len();
        if n >= el + 3 && chars[n - el..] == end_chars[..] {
            return chars[..n - el].iter().collect();
        }
    }
    s.to_string()
}

/// Загрузить контакты как ordered list `Contact { uid, name, aliases }`.
/// `name` — первый токен после `uid - `; `aliases` — остальные (включая сам name для матча).
pub fn load_structured(path: &Path) -> Vec<Contact> {
    let mut out = Vec::new();
    let txt = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return out,
    };
    for line in txt.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((uid_part, rest)) = line.split_once(" - ") else { continue };
        let Ok(uid) = uid_part.trim().parse::<i64>() else { continue };
        let parts: Vec<String> = rest
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            continue;
        }
        let name = parts[0].clone();
        let aliases = parts[1..].to_vec();
        out.push(Contact { uid, name, aliases });
    }
    out
}

/// Записать контакты обратно в файл в стандартном формате.
pub fn save_structured(path: &Path, contacts: &[Contact]) -> std::io::Result<()> {
    let mut lines = String::new();
    for c in contacts {
        let mut row = format!("{} - {}", c.uid, c.name.trim());
        for a in &c.aliases {
            let a = a.trim();
            if !a.is_empty() {
                row.push_str(", ");
                row.push_str(a);
            }
        }
        lines.push_str(&row);
        lines.push('\n');
    }
    std::fs::write(path, lines)
}

/// Глаголы-команды на отправку сообщения. Все варианты которые видим в ASR:
/// - «напиши/напишите» — основной императив
/// - «напишу» — 1л ед.ч., whisper иногда так слышит
/// - «запиши/запишите» — частая ошибка распознавания: н→з
/// - «пиши/пишите» — без приставки на-, тоже валидный императив
/// - «отправь/отправьте/отправит» — синоним
/// - «напишет/напиши-ка» — 3л и частица «-ка»
///
/// Сортируем по длине DESC при использовании, чтобы «напишите» не съело «напиши».
const TRIGGERS: &[&str] = &[
    "напиши", "напишите", "напиши-ка", "напишу",
    "запиши", "запишите",
    "пиши", "пишите",
    "отправь", "отправьте", "отправит",
    "напишет",
    // English
    "write", "right", "wright", "rite",
    "text", "send", "message",
    // Кириллический фонетический английский — Whisper/Parakeet иногда
    // транскрибирует «write to myself» как «врайт ту майселф».
    "врайт", "райт", "райте",
    "сенд", "сэнд", "сэнт",
    "тэкст", "текст",
    "месседж", "месэдж",
];

/// Распарсить распознанный текст: «<триггер> имя текст» → (uid, message) или ошибка.
///
/// ASR часто склеивает или разделяет слова не так:
///   «напишичине привет»      — trigger + name склеены  (1 слово вместо 2)
///   «напиши чинепривет»      — name + message склеены  (1 слово вместо 2)
///   «напишичинепривет»       — всё склеено             (1 слово вместо 3)
///   «напешь чине привет»     — trigger переврался     (DL distance 1)
/// Стратегия:
///   1) Триггер ищется как **префикс** первого токена (не точное совпадение).
///      Триггеры сортируются по длине DESC, чтобы «напишите» не съело «напиши».
///   2) После strip-trigger ищется самый длинный контакт-алиас как char-префикс
///      первого слова. Этого хватает чтобы расцепить «чинепривет»→«чине»+«привет».
///   3) Если шаг 1 не нашёл триггер — допускаем DL≤1 на первом токене, на случай
///      whisper'овской ослышки.
pub fn parse_command(text: &str, contacts: &Contacts) -> Result<(i64, String), String> {
    // Нормализация: пунктуация (",.!?;:" итд) → пробелы, лишние пробелы схлопываются.
    // «Напиши, тиме, привет.» → «напиши тиме привет»
    let normalized: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '\t' { c } else { ' ' })
        .collect();
    let t: String = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return Err(format!("пустой текст: «{}»", text));
    }

    // Триггеры в порядке убывания длины: «напишите» проверяется раньше «напиши»,
    // чтобы префикс-матч не поглотил длинный триггер коротким.
    let mut sorted_triggers: Vec<&&str> = TRIGGERS.iter().collect();
    sorted_triggers.sort_by_key(|s| std::cmp::Reverse(s.len()));

    // Шаг 1: префикс-матч триггера (включая склейки «напишичине»).
    let mut found: Option<(&'static str, String)> = None;
    for &&trig in &sorted_triggers {
        if t.starts_with(trig) {
            let rest = t[trig.len()..].trim_start().to_string();
            found = Some((trig, rest));
            break;
        }
    }

    // Шаг 1b: fuzzy-fallback если префикс-матч не сработал. Ослышка на 1 правку:
    // «напешь маше» → DL(«напешь», «напиши»)=2 — не пройдёт. «напиши»→«напши»=1 — да.
    // Делаем только для первого токена ≥4 символов, чтобы не путать с короткими словами.
    if found.is_none() {
        let first_tok = t.split_whitespace().next().unwrap_or("");
        if first_tok.chars().count() >= 4 {
            for &&trig in &sorted_triggers {
                if damerau_levenshtein(first_tok, trig) <= 1 {
                    let rest = t[first_tok.len()..].trim_start().to_string();
                    info!("[parse] fuzzy-trigger: '{}' ≈ '{}'", first_tok, trig);
                    found = Some((trig, rest));
                    break;
                }
            }
        }
    }

    // Шаг 1c: триггер во 2-м или 3-м токене (whisper иногда вставляет в начало
    // слова-артефакты вида «в», «и», «а» — «в райт ту майселф» вместо «врайт…»).
    // Если первые 1-2 токена короткие (≤2 символов) и за ними идёт триггер —
    // принимаем как trigger.
    if found.is_none() {
        let toks: Vec<&str> = t.split_whitespace().collect();
        // Сколько лидирующих коротких артефактов пропустить (max 2).
        let mut skip = 0usize;
        for tk in toks.iter().take(2) {
            if tk.chars().count() <= 2 {
                skip += 1;
            } else {
                break;
            }
        }
        if skip > 0 && toks.len() > skip {
            let candidate = toks[skip..].join(" ");
            for &&trig in &sorted_triggers {
                if candidate.starts_with(trig) {
                    let rest = candidate[trig.len()..].trim_start().to_string();
                    info!("[parse] skip-prefix trigger: skipped {} short tokens, '{}' matches '{}'", skip, &toks[..skip].join(" "), trig);
                    found = Some((trig, rest));
                    break;
                }
            }
        }
    }

    let Some((used, rest)) = found else {
        return Err(format!(
            "команда не распознана (ожидалось «напиши» / «отправь»): «{}»",
            text
        ));
    };
    if rest.is_empty() {
        return Err("имя получателя не указано".into());
    }

    // Шаг 2: вытащить имя из rest. Если первое слово — точный контакт или его стем,
    // берём как есть. Иначе пробуем самый длинный контакт-алиас как char-префикс
    // первого слова: «чинепривет»→«чине»+«привет».
    let (name, message) = extract_name_and_message(&rest, contacts);
    let mut name = name.trim().to_string();
    let mut message = message.trim().to_string();

    // Пропускаем распространённые предлоги: «write to dan» → «write dan»
    const PREPOSITIONS: &[&str] = &["to", "к", "в", "на", "for"];
    if PREPOSITIONS.contains(&name.as_str()) {
        // name — предлог, берём первое слово message как имя
        let msg_words: Vec<&str> = message.split_whitespace().collect();
        if !msg_words.is_empty() {
            name = msg_words[0].to_string();
            message = msg_words[1..].join(" ");
        }
    }

    if name.is_empty() {
        return Err("имя получателя не указано".into());
    }
    if name.chars().count() < 2 {
        return Err(format!(
            "имя «{}» слишком короткое — скорее всего ослышка whisper'а",
            name
        ));
    }

    info!("[parse] text='{}' (trig='{}') → name='{}' message='{}'", text, used, name, message);

    // Спец-кейс: «себе/себя/сам/saved/избранное/myself/me/...» → Saved Messages.
    // Возвращаем sentinel UID = SELF_SENTINEL_UID, вызывающий код подставит
    // реальный user_id текущего залогиненного аккаунта.
    if matches!(name.as_str(),
        "себе" | "себя" | "сам" |
        "saved" | "избранное" | "избранные" | "favorites" |
        "self" | "myself" | "myselve" | "me" |
        // Кириллический фонетический английский ASR
        "майселф" | "майсельф" | "майселв" | "мисельф" | "мисельв" |
        "ту майселф" | "ту майсельф"
    ) {
        info!("[parse] SELF match → SavedMessages");
        return Ok((SELF_SENTINEL_UID, message));
    }
    // Whisper часто разбивает «myself» на «my self» (или «my-self» → после
    // нормализации становится «my self»). Парсер вытащит name="my" message=
    // "self [rest]". Распознаём и режем «self» из message.
    if name == "my" || name == "май" {
        let msg_words: Vec<&str> = message.split_whitespace().collect();
        if !msg_words.is_empty() && matches!(msg_words[0].to_lowercase().as_str(),
            "self" | "selve" | "селф" | "сельф" | "селв"
        ) {
            let real_msg = msg_words[1..].join(" ");
            info!("[parse] SELF match (my + self split) → SavedMessages, msg='{}'", real_msg);
            return Ok((SELF_SENTINEL_UID, real_msg));
        }
    }
    // Также: «ту майселф» — «to» как соединительная частица перед получателем
    // («write TO myself»). Если name=«ту» и first message word=«майселф» —
    // считаем что это self.
    if name == "ту" || name == "to" {
        let msg_words: Vec<&str> = message.split_whitespace().collect();
        if !msg_words.is_empty() && matches!(msg_words[0].to_lowercase().as_str(),
            "myself" | "myselve" | "self" | "майселф" | "майсельф" | "мисельф" | "себе" | "себя"
        ) {
            let real_msg = msg_words[1..].join(" ");
            info!("[parse] SELF match (to + myself split) → SavedMessages, msg='{}'", real_msg);
            return Ok((SELF_SENTINEL_UID, real_msg));
        }
    }

    // Точное совпадение по алиасу
    if let Some(&uid) = contacts.get(&name) {
        info!("[parse] EXACT match → uid={}", uid);
        return Ok((uid, message));
    }

    // Совпадение по русскому стему: «тиме»→«тим», «маше»→«маш» итд.
    let stem = russian_stem(&name);
    if stem != name {
        if let Some(&uid) = contacts.get(&stem) {
            info!("[parse] STEM match: '{}' → '{}' → uid={}", name, stem, uid);
            return Ok((uid, message));
        }
        info!("[parse] stem '{}' not in contacts, → fuzzy", stem);
    }

    // Топ-5 кандидатов по фаззи-скору — пишем в лог чтобы видеть почему выбран не тот
    let mut scored: Vec<(f32, i64, &str)> = contacts
        .iter()
        .map(|(alias, &uid)| (fuzzy_score(&name, alias), uid, alias.as_str()))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<String> = scored
        .iter()
        .take(5)
        .map(|(s, uid, a)| format!("{}={:.2}({})", a, s, uid))
        .collect();
    info!("[parse] fuzzy top5: {}", top.join(", "));

    let best = scored.first().copied();
    // С DL-based fuzzy threshold можно вернуть на 0.55:
    // - «теми»→«тиме» через одну транспозицию даёт ~0.66 → проходит
    // - «имя»→всё подряд даёт <0.40 → не проходит
    let threshold = if name.chars().count() >= 3 { 0.55 } else { 0.85 };
    if let Some((s, uid, alias)) = best {
        if s >= threshold {
            warn!("[parse] fuzzy «{}» → «{}» (score {:.2})", name, alias, s);
            return Ok((uid, message));
        }
    }
    Err(format!(
        "контакт «{}» не найден (есть: {})",
        name,
        contacts.keys().cloned().collect::<Vec<_>>().join(", ")
    ))
}

/// Из строки `rest` (всё после триггера) вытащить (name, message).
/// Покрывает кейсы склейки/расщепления от ASR:
///   - «маше привет»        → name=маше, message=привет     (норм)
///   - «машепривет»         → name=маша, message=привет     (имя+текст склеены)
///   - «машепривет ивану»   → name=маша, message=«привет ивану»
///   - «ма ше привет»       → name=маше, message=привет     (имя расщеплено на 2)
///   - «ти м о фей привет»  → name=тимофей, message=привет  (имя расщеплено на 3)
///   - «чи не привет»       → name=чине, message=привет     (имя расщеплено на 2)
///
/// Алгоритм: пробуем склеить первые K=1..=3 токенов как один кандидат имени,
/// для каждого K проверяем точное совпадение/стем/longest-alias-prefix. Берём
/// **самое длинное** K, которое смогло сматчиться — это гарантирует что «чине»
/// не съест только «чи» из «чи не привет».
fn extract_name_and_message(rest: &str, contacts: &Contacts) -> (String, String) {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.is_empty() {
        return (String::new(), String::new());
    }
    let max_k = tokens.len().min(3);

    // Шаг 1: exact-match по склеиванию первых K токенов, HIGH→LOW.
    // Длинное склеивание приоритетнее короткого: для «ти м а привет» хотим
    // k=3 («тима» exact) а не k=2 («тим» stem) — иначе «а» уйдёт в сообщение.
    for k in (1..=max_k).rev() {
        let glued: String = tokens[..k].concat().to_lowercase();
        if contacts.contains_key(&glued) || contacts.contains_key(&russian_stem(&glued)) {
            let tail = if k < tokens.len() { tokens[k..].join(" ") } else { String::new() };
            return (glued, tail);
        }
    }

    // Шаг 2: declension-guard. Если ПЕРВЫЙ токен сам по себе матчится с
    // alias-prefix но suffix очень короткий (≤3 chars) — это скорее всего
    // declension/diminutive формы alias'а («тимке», «тимку», «чинкой»),
    // а не «alias склеенный с началом message». Не пытаемся splitить;
    // возвращаем первый токен как имя, fuzzy-match в parse_command найдёт
    // правильный alias (например «тимке» → «тима» через DL=2 → score 0.58).
    //
    // Без этого guard'а «тимке мол как дела» → k=2 glued «тимкемол»
    // даст split «тим»+«кемол», и в message уехало бы «кемол как дела».
    const MIN_SPLIT_SUFFIX: usize = 4;
    let first_glued = tokens[0].to_lowercase();
    if let Some((_name, suffix)) = longest_alias_prefix(&first_glued, contacts) {
        if suffix.chars().count() > 0 && suffix.chars().count() < MIN_SPLIT_SUFFIX {
            // первый токен = alias + короткий declension suffix → не split
            let first = tokens[0].to_string();
            let tail = tokens[1..].join(" ");
            return (first, tail);
        }
    }

    // Шаг 3: longest-alias-prefix, LOW→HIGH. Это для случаев когда имя
    // склеено с первым словом сообщения: «чинепривет» (k=1) → «чине»+«привет».
    // Низкое k приоритетнее — иначе мы бы съели в glued куски сообщения,
    // и пробелы в сообщении пропали бы.
    //
    // Split разрешён только если suffix ≥ MIN_SPLIT_SUFFIX — short suffix
    // обработан guard'ом выше.
    for k in 1..=max_k {
        let glued: String = tokens[..k].concat().to_lowercase();
        if let Some((name, suffix)) = longest_alias_prefix(&glued, contacts) {
            if suffix.chars().count() < MIN_SPLIT_SUFFIX {
                continue;
            }
            let tail = if k < tokens.len() { tokens[k..].join(" ") } else { String::new() };
            let message = if suffix.is_empty() {
                tail
            } else if tail.is_empty() {
                suffix.to_string()
            } else {
                format!("{} {}", suffix, tail)
            };
            return (name, message);
        }
    }

    // Fallback: первое слово как есть → дальше пойдёт fuzzy в parse_command.
    let first = tokens[0].to_string();
    let tail = tokens[1..].join(" ");
    (first, tail)
}

/// Самый длинный контакт-алиас в `contacts`, являющийся char-префиксом `s`
/// (case-insensitive). Возвращает (alias, suffix).
///   `longest_alias_prefix("чинепривет", {"чин", "чине"}) = Some(("чине", "привет"))`
fn longest_alias_prefix(s: &str, contacts: &Contacts) -> Option<(String, String)> {
    let s_chars: Vec<char> = s.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut best: Option<(String, usize)> = None;
    for alias in contacts.keys() {
        // Алиасы короче 2 символов — слишком ложноположительны.
        let a_chars: Vec<char> = alias.chars().collect();
        if a_chars.len() < 2 || a_chars.len() > s_chars.len() {
            continue;
        }
        if s_chars[..a_chars.len()] == a_chars[..] {
            let n = a_chars.len();
            if best.as_ref().map_or(true, |(_, bn)| n > *bn) {
                best = Some((alias.clone(), n));
            }
        }
    }
    let (name, n_chars) = best?;
    // Найдём byte-offset n-го char'а в `s` чтобы корректно нарезать UTF-8.
    let suffix_byte = s.char_indices().nth(n_chars).map(|(b, _)| b).unwrap_or(s.len());
    Some((name, s[suffix_byte..].to_string()))
}

fn fuzzy_score(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    // Substring-bonus только если короткая сторона ≥3 символов
    // (чтобы «и в тимофей» не давало 0.85).
    let shorter = a_len.min(b_len);
    if shorter >= 3 && (a.contains(b) || b.contains(a)) {
        return 0.85;
    }
    // Damerau-Levenshtein: расстояние с учётом транспозиции соседей.
    // «теми» vs «тиме»: одна транспозиция и-е → расстояние 1 → высокий score.
    let dl = damerau_levenshtein(a, b);
    let max_len = a_len.max(b_len).max(1);
    let dl_sim = 1.0 - (dl as f32 / max_len as f32);

    // Дополнительные сигналы: общий префикс + общий char-set Jaccard.
    let common_prefix = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let pref_score = common_prefix as f32 / max_len as f32;
    let sa: std::collections::HashSet<char> = a.chars().collect();
    let sb: std::collections::HashSet<char> = b.chars().collect();
    let inter = sa.intersection(&sb).count();
    let union = sa.union(&sb).count().max(1);
    let char_score = inter as f32 / union as f32;

    // DL — основной сигнал (вес 0.6), префикс + char-overlap — добавки.
    (0.6 * dl_sim + 0.25 * pref_score + 0.15 * char_score).clamp(0.0, 1.0)
}

/// Damerau-Levenshtein distance — обычный Levenshtein + операция «транспозиция
/// двух соседних символов» как 1 правка. Хорошо ловит whisper-ослышки типа
/// «теми»↔«тиме», «маню»↔«ману», где буквы переставлены местами.
fn damerau_levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 { return m; }
    if m == 0 { return n; }
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n { d[i][0] = i; }
    for j in 0..=m { d[0][j] = j; }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            d[i][j] = (d[i - 1][j] + 1)              // удаление
                .min(d[i][j - 1] + 1)                 // вставка
                .min(d[i - 1][j - 1] + cost);         // замена
            if i > 1 && j > 1
                && a[i - 1] == b[j - 2]
                && a[i - 2] == b[j - 1]
            {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1); // транспозиция
            }
        }
    }
    d[n][m]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_contacts() -> Contacts {
        let mut c = Contacts::new();
        c.insert("чине".into(), 1001);
        c.insert("чина".into(), 1001);
        c.insert("чин".into(), 1001);   // стем
        c.insert("маша".into(), 2002);
        c.insert("маше".into(), 2002);
        c.insert("маш".into(), 2002);
        c.insert("тима".into(), 3003);
        c.insert("тиме".into(), 3003);
        c.insert("тим".into(), 3003);
        c
    }

    #[test]
    fn happy_path() {
        let c = sample_contacts();
        let (uid, msg) = parse_command("Напиши Чине привет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn punctuation_stripped() {
        let c = sample_contacts();
        let (uid, msg) = parse_command("Напиши, Чине, привет.", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn trigger_glued_to_name() {
        // ASR склеил trigger + name: «напишичине привет»
        let c = sample_contacts();
        let (uid, msg) = parse_command("напишичине привет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn name_glued_to_message() {
        // ASR склеил name + message: «напиши чинепривет»
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши чинепривет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn everything_glued() {
        // Всё одним словом: «напишичинепривет»
        let c = sample_contacts();
        let (uid, msg) = parse_command("напишичинепривет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn longest_alias_wins() {
        // «чине» должно матчиться раньше «чин» (стема), чтобы не съесть «е» из «епривет».
        let c = sample_contacts();
        let (uid, msg) = parse_command("напишичинепривет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn message_with_spaces() {
        let c = sample_contacts();
        let (uid, msg) = parse_command("напишичине как дела", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "как дела");
    }

    #[test]
    fn name_with_trailing_message_in_first_word_plus_more() {
        // «напиши чинепривет от меня» — splice имени с первым словом сообщения,
        // остальные слова идут следом.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши чинепривет от меня", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет от меня");
    }

    #[test]
    fn fuzzy_trigger_one_typo() {
        // «напши» = «напиши» с одной правкой (удаление 'и') → должно матчиться.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напши чине привет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn no_trigger_fails() {
        let c = sample_contacts();
        assert!(parse_command("привет чине", &c).is_err());
    }

    #[test]
    fn unknown_contact_fails() {
        let c = sample_contacts();
        assert!(parse_command("напиши неизвестный текст", &c).is_err());
    }

    #[test]
    fn longer_trigger_first() {
        // «напишите» должно матчиться раньше «напиши», иначе «напишите маше» съест
        // только «напиши» и оставит «те маше» — это сломает имя.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напишите маше привет", &c).unwrap();
        assert_eq!(uid, 2002);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn name_split_two_words() {
        // ASR расщепил «чине» на два слова: «чи не».
        // k=2: glued=«чине» — exact match.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши чи не привет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn name_split_three_words() {
        // ASR расщепил «тима» на три слова: «ти м а» (пограничный случай).
        // k=3: glued=«тима» — exact match.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши ти м а привет", &c).unwrap();
        assert_eq!(uid, 3003);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn name_split_with_glued_message() {
        // Имя расщеплено + сообщение склеено с последним куском.
        // «напиши чи непривет» — k=1 «чи» fail. k=2 glued=«чинепривет»,
        // longest_alias_prefix → name=«чине», suffix=«привет», tail="" → msg=«привет».
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши чи непривет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn trigger_zapishi() {
        // Whisper часто слышит «напиши» как «запиши» (н→з).
        let c = sample_contacts();
        let (uid, msg) = parse_command("запиши чине привет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn trigger_napishu() {
        // «напишу» — первое лицо ед.ч., тоже валидный триггер.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напишу чине привет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn trigger_pishi() {
        // «пиши» без приставки на-.
        let c = sample_contacts();
        let (uid, msg) = parse_command("пиши маше как дела", &c).unwrap();
        assert_eq!(uid, 2002);
        assert_eq!(msg, "как дела");
    }

    #[test]
    fn longest_glue_wins() {
        // Если рассыпать «тиме» на «ти ме», k=1 даст «ти» (нет в контактах),
        // но k=2 даст «тиме» (exact). Берём более длинное склеивание.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши ти ме привет", &c).unwrap();
        assert_eq!(uid, 3003);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn declension_not_split_short_suffix() {
        // Bug fix: «тимке» не должен жадно сматчиться как «тим»+«ке»
        // (где «ке» уезжает в сообщение). С MIN_SPLIT_SUFFIX=4 такой
        // split запрещён, name=тимке, дальше fuzzy найдёт «тима» (DL=2).
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши тимке мол как дела", &c).unwrap();
        assert_eq!(uid, 3003);  // = тима через fuzzy
        assert_eq!(msg, "мол как дела");  // НЕ «ке мол как дела»
    }

    #[test]
    fn declension_chinkoy() {
        // Аналогично: «чинкой» (instrumental от «Чинка/Чина») не должен
        // дать «чин»+«кой».
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши чинкой привет", &c).unwrap();
        assert_eq!(uid, 1001);  // = чине/чина через fuzzy
        assert_eq!(msg, "привет");
    }

    #[test]
    fn long_suffix_still_splits() {
        // «чинепривет»: suffix «привет» = 6 chars ≥ 4 → split разрешён.
        // Покрывается существующим тестом name_glued_to_message; перепроверяем.
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши чинепривет", &c).unwrap();
        assert_eq!(uid, 1001);
        assert_eq!(msg, "привет");
    }

    #[test]
    fn trigger_write_english() {
        let mut c = sample_contacts();
        c.insert("dan".into(), 4004);
        let (uid, msg) = parse_command("write dan hello there", &c).unwrap();
        assert_eq!(uid, 4004);
        assert_eq!(msg, "hello there");
    }

    #[test]
    fn trigger_send_english() {
        let mut c = sample_contacts();
        c.insert("alice".into(), 5005);
        let (uid, msg) = parse_command("send alice how are you", &c).unwrap();
        assert_eq!(uid, 5005);
        assert_eq!(msg, "how are you");
    }

    #[test]
    fn trigger_text_english() {
        let mut c = sample_contacts();
        c.insert("bob".into(), 6006);
        let (uid, msg) = parse_command("text bob meeting at 5", &c).unwrap();
        assert_eq!(uid, 6006);
        assert_eq!(msg, "meeting at 5");
    }

    #[test]
    fn trigger_message_english() {
        let mut c = sample_contacts();
        c.insert("carol".into(), 7007);
        let (uid, msg) = parse_command("message carol see you soon", &c).unwrap();
        assert_eq!(uid, 7007);
        assert_eq!(msg, "see you soon");
    }

    #[test]
    fn english_trigger_fuzzy_one_typo() {
        // «wrote» = «write» с одной правкой (транспозиция)
        let mut c = sample_contacts();
        c.insert("dan".into(), 4004);
        let (uid, msg) = parse_command("wrote dan hello", &c).unwrap();
        assert_eq!(uid, 4004);
        assert_eq!(msg, "hello");
    }

    #[test]
    fn preposition_skip_english() {
        // «write to dan» → предлог "to" пропускается
        let mut c = sample_contacts();
        c.insert("dan".into(), 4004);
        let (uid, msg) = parse_command("write to dan hello there", &c).unwrap();
        assert_eq!(uid, 4004);
        assert_eq!(msg, "hello there");
    }

    #[test]
    fn preposition_skip_russian() {
        // «напиши к тиме» → предлог "к" пропускается
        let c = sample_contacts();
        let (uid, msg) = parse_command("напиши к тиме привет", &c).unwrap();
        assert_eq!(uid, 3003);
        assert_eq!(msg, "привет");
    }
}

