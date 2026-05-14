//! contacts.txt парсер: `123456 - имя1, имя2, имя3` → {alias.lower(): uid}
//! + парсер голосовой команды «напиши/отправь имя текст» с fuzzy match.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub type Contacts = HashMap<String, i64>;

/// Structured-вид контакта для UI: имя + список алиасов + uid.
/// Первый алиас в файле — это «display name», остальные — голосовые синонимы.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub uid: i64,
    pub name: String,
    pub aliases: Vec<String>,
}

pub fn default_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("contacts.txt")))
        .unwrap_or_else(|| PathBuf::from("contacts.txt"))
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

const TRIGGERS: &[&str] = &[
    "напиши", "напишите", "напиши-ка",
    "отправь", "отправьте", "отправит",
    "напишет",
];

/// Распарсить распознанный текст: «<триггер> имя текст» → (uid, message) или ошибка.
pub fn parse_command(text: &str, contacts: &Contacts) -> Result<(i64, String), String> {
    // Нормализация: пунктуация (",.!?;:" итд) → пробелы, лишние пробелы схлопываются.
    // «Напиши, тиме, привет.» → «напиши тиме привет»
    let normalized: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '\t' { c } else { ' ' })
        .collect();
    let t: String = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    // Триггер — первый токен, если он в списке.
    let used = TRIGGERS.iter().find(|trig| {
        t.starts_with(&format!("{} ", trig)) || t == **trig
    });
    let Some(used) = used else {
        return Err(format!(
            "команда не распознана (ожидалось «напиши» / «отправь»): «{}»",
            text
        ));
    };
    let rest = t[used.len()..].trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("").trim().to_string();
    let message = parts.next().unwrap_or("").trim().to_string();

    if name.is_empty() {
        return Err("имя получателя не указано".into());
    }
    if name.chars().count() < 2 {
        return Err(format!(
            "имя «{}» слишком короткое — скорее всего ослышка whisper'а",
            name
        ));
    }

    info!("[parse] text='{}' → name='{}' message='{}'", text, name, message);

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
