//! Локальный вектор-индекс музыкальной библиотеки для команды «включи <песня>».
//!
//! Идея: из выбранного канала собираем названия треков, по символьным n-граммам
//! строим TF-IDF векторы и ищем самый похожий по косинусной близости. Модели не
//! нужно: всё считается на месте, оффлайн, мультиязычно.
//!
//! Язык: и названия, и запрос транслитерируем кириллицу в латиницу, поэтому
//! английский трек находится, даже если Parakeet распознал его кириллицей
//! («бохемиан рапсоди» → bohemian rapsodi совпадает с «Bohemian Rhapsody»).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Минимальная косинусная близость, ниже которой считаем «не нашли».
const MATCH_THRESHOLD: f32 = 0.30;

/// Один трек: id сообщения в канале + человекочитаемое название (для поиска и логов).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub msg_id: i32,
    pub title: String,
}

/// Индекс: треки + их нормированные TF-IDF векторы + IDF словаря n-грамм.
pub struct MusicIndex {
    tracks: Vec<Track>,
    vectors: Vec<HashMap<String, f32>>,
    idf: HashMap<String, f32>,
}

impl MusicIndex {
    /// Построить индекс из списка треков (векторизация в RAM, быстрая).
    pub fn build(tracks: Vec<Track>) -> Self {
        let n = tracks.len().max(1) as f32;
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut tfs: Vec<HashMap<String, f32>> = Vec::with_capacity(tracks.len());
        for t in &tracks {
            let mut tf: HashMap<String, f32> = HashMap::new();
            for g in ngrams(&normalize(&t.title)) {
                *tf.entry(g).or_insert(0.0) += 1.0;
            }
            for g in tf.keys() {
                *df.entry(g.clone()).or_insert(0) += 1;
            }
            tfs.push(tf);
        }
        let idf: HashMap<String, f32> = df
            .iter()
            .map(|(g, &d)| (g.clone(), ((n + 1.0) / (d as f32 + 1.0)).ln() + 1.0))
            .collect();
        let vectors = tfs.iter().map(|tf| weight_and_norm(tf, &idf)).collect();
        Self { tracks, vectors, idf }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Найти самый похожий трек на запрос. Возвращает (msg_id, title, score) или
    /// None, если индекс пуст или ничего не дотянуло до порога.
    pub fn best_match(&self, query: &str) -> Option<(i32, String, f32)> {
        if self.tracks.is_empty() {
            return None;
        }
        let mut tf: HashMap<String, f32> = HashMap::new();
        for g in ngrams(&normalize(query)) {
            *tf.entry(g).or_insert(0.0) += 1.0;
        }
        let qv = weight_and_norm(&tf, &self.idf);
        if qv.is_empty() {
            return None;
        }
        let mut best: Option<(usize, f32)> = None;
        for (i, v) in self.vectors.iter().enumerate() {
            let score = cosine(&qv, v);
            if best.map_or(true, |(_, s)| score > s) {
                best = Some((i, score));
            }
        }
        best.and_then(|(i, s)| {
            if s >= MATCH_THRESHOLD {
                Some((self.tracks[i].msg_id, self.tracks[i].title.clone(), s))
            } else {
                None
            }
        })
    }
}

/// Грубое определение языка строки по преобладанию письменности. Транслитерация
/// и так объединяет кириллицу с латиницей, но детект полезен для логов и для
/// решения «английское название или русское».
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Lang {
    Ru,
    En,
    Other,
}

pub fn detect_lang(s: &str) -> Lang {
    let mut cyr = 0usize;
    let mut lat = 0usize;
    for c in s.chars() {
        if ('а'..='я').contains(&c) || c == 'ё' || ('А'..='Я').contains(&c) || c == 'Ё' {
            cyr += 1;
        } else if c.is_ascii_alphabetic() {
            lat += 1;
        }
    }
    if cyr == 0 && lat == 0 {
        Lang::Other
    } else if cyr >= lat {
        Lang::Ru
    } else {
        Lang::En
    }
}

/// Нормализация: lowercase, транслит кириллицы в латиницу, только буквы/цифры,
/// схлопывание пробелов. «Кино - Кукушка» → «kino kukushka».
fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let trans = translit(&lowered);
    let cleaned: String = trans
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Транслитерация кириллицы (нижний регистр) в латиницу по стандартной схеме.
/// Латиница и прочие символы проходят как есть.
fn translit(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'а' => out.push('a'),
            'б' => out.push('b'),
            'в' => out.push('v'),
            'г' => out.push('g'),
            'д' => out.push('d'),
            'е' | 'ё' | 'э' => out.push('e'),
            'ж' => out.push_str("zh"),
            'з' => out.push('z'),
            'и' | 'й' | 'ы' => out.push('i'),
            'к' => out.push('k'),
            'л' => out.push('l'),
            'м' => out.push('m'),
            'н' => out.push('n'),
            'о' => out.push('o'),
            'п' => out.push('p'),
            'р' => out.push('r'),
            'с' => out.push('s'),
            'т' => out.push('t'),
            'у' => out.push('u'),
            'ф' => out.push('f'),
            'х' => out.push('h'),
            'ц' => out.push_str("ts"),
            'ч' => out.push_str("ch"),
            'ш' => out.push_str("sh"),
            'щ' => out.push_str("sch"),
            'ю' => out.push_str("yu"),
            'я' => out.push_str("ya"),
            'ъ' | 'ь' => {}
            other => out.push(other),
        }
    }
    out
}

/// Символьные n-граммы по словам (с граничным маркером `#`), плюс само слово как
/// отдельный признак. Маркеры дают вес началу/концу слова, целое слово усиливает
/// точные совпадения. «tim» → ["=tim", "#ti", "tim", "im#"].
fn ngrams(normalized: &str) -> Vec<String> {
    let mut grams = Vec::new();
    for word in normalized.split_whitespace() {
        grams.push(format!("={}", word));
        let padded: Vec<char> = format!("#{}#", word).chars().collect();
        if padded.len() >= 3 {
            for w in padded.windows(3) {
                grams.push(w.iter().collect());
            }
        } else {
            grams.push(padded.iter().collect());
        }
    }
    grams
}

/// tf * idf по словарю + L2-нормировка. n-граммы запроса, которых нет в корпусе
/// (нет idf), отбрасываем: матчиться им всё равно не с чем.
fn weight_and_norm(
    tf: &HashMap<String, f32>,
    idf: &HashMap<String, f32>,
) -> HashMap<String, f32> {
    let mut v: HashMap<String, f32> = HashMap::new();
    for (g, &c) in tf {
        if let Some(&w) = idf.get(g) {
            v.insert(g.clone(), c * w);
        }
    }
    let norm = v.values().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.values_mut() {
            *x /= norm;
        }
    }
    v
}

/// Косинус двух уже нормированных разреженных векторов = их скалярное произведение.
fn cosine(a: &HashMap<String, f32>, b: &HashMap<String, f32>) -> f32 {
    let (small, big) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    small
        .iter()
        .filter_map(|(k, va)| big.get(k).map(|vb| va * vb))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MusicIndex {
        MusicIndex::build(vec![
            Track { msg_id: 1, title: "Imagine Dragons - Believer".into() },
            Track { msg_id: 2, title: "Queen - Bohemian Rhapsody".into() },
            Track { msg_id: 3, title: "Кино - Кукушка".into() },
            Track { msg_id: 4, title: "Леонид Агутин - Хоп Хей Ла Ла Лей".into() },
            Track { msg_id: 5, title: "Daft Punk - Get Lucky".into() },
        ])
    }

    #[test]
    fn exact_english_word() {
        let idx = sample();
        assert_eq!(idx.best_match("believer").map(|m| m.0), Some(1));
        assert_eq!(idx.best_match("get lucky").map(|m| m.0), Some(5));
    }

    #[test]
    fn russian_title_match() {
        let idx = sample();
        assert_eq!(idx.best_match("кукушка").map(|m| m.0), Some(3));
        assert_eq!(idx.best_match("кино кукушка").map(|m| m.0), Some(3));
    }

    #[test]
    fn english_heard_in_cyrillic() {
        // Parakeet распознал английское название кириллицей: всё равно матчим.
        let idx = sample();
        assert_eq!(idx.best_match("бохемиан рапсоди").map(|m| m.0), Some(2));
        assert_eq!(idx.best_match("куин богемиан").map(|m| m.0), Some(2));
    }

    #[test]
    fn nonsense_below_threshold() {
        let idx = sample();
        assert!(idx.best_match("zzzz qqqq wxyz").is_none());
    }

    #[test]
    fn empty_index() {
        let idx = MusicIndex::build(vec![]);
        assert!(idx.best_match("anything").is_none());
    }

    #[test]
    fn lang_detection() {
        assert_eq!(detect_lang("Bohemian Rhapsody"), Lang::En);
        assert_eq!(detect_lang("Кукушка"), Lang::Ru);
        assert_eq!(detect_lang("12345"), Lang::Other);
    }
}
