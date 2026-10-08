//! Local passage retrieval followed by ordered, bounded OCR alignment.
//! Retrieval scores select candidates; only alignment supplies source offsets.
use super::Book;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone)]
pub(super) struct TextWord {
    pub(super) key: String,
    pub(super) start: usize,
    pub(super) end: usize,
}
pub(super) fn text_words(text: &str) -> Vec<TextWord> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if ch.is_whitespace() {
            if let Some(left) = start.take() {
                let key = text[left..index]
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                if !key.is_empty() {
                    words.push(TextWord {
                        key,
                        start: left,
                        end: index,
                    });
                }
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    words
}
fn distance(left: &[TextWord], right: &[TextWord], limit: usize) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut before_previous = previous.clone();
    for (i, a) in left.iter().enumerate() {
        let mut row = vec![i + 1; right.len() + 1];
        for (j, b) in right.iter().enumerate() {
            let mut score = (previous[j] + usize::from(a.key != b.key))
                .min(previous[j + 1] + 1)
                .min(row[j] + 1);
            if i > 0
                && left[i - 1].key.len() + a.key.len() == b.key.len()
                && b.key.starts_with(&left[i - 1].key)
                && b.key.ends_with(&a.key)
            {
                score = score.min(before_previous[j] + 1);
            }
            if j > 0
                && right[j - 1].key.len() + b.key.len() == a.key.len()
                && a.key.starts_with(&right[j - 1].key)
                && a.key.ends_with(&b.key)
            {
                score = score.min(previous[j - 1] + 1);
            }
            row[j + 1] = score.min(limit + 1);
        }
        before_previous = previous;
        previous = row;
    }
    previous[right.len()]
}
fn one_character_change(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let a = left.chars().collect::<Vec<_>>();
    let b = right.chars().collect::<Vec<_>>();
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        if a.len() >= b.len() {
            i += 1;
        }
        if b.len() >= a.len() {
            j += 1;
        }
    }
    edits + a.len() - i + b.len() - j <= 1
}
fn boundary_matches(context: &[TextWord], candidate: &[TextWord], tail: bool) -> bool {
    let context = if tail {
        &context[context.len() - 6..]
    } else {
        &context[..6]
    };
    let expected = context.iter().map(|w| w.key.as_str()).collect::<String>();
    (4..=8.min(candidate.len())).any(|length| {
        let words = if tail {
            &candidate[candidate.len() - length..]
        } else {
            &candidate[..length]
        };
        let actual = words.iter().map(|w| w.key.as_str()).collect::<String>();
        one_character_change(&expected, &actual)
    })
}
fn triples(words: &[TextWord]) -> std::collections::HashSet<String> {
    words
        .windows(3)
        .map(|w| format!("{} {} {}", w[0].key, w[1].key, w[2].key))
        .collect()
}
fn terms(words: &[TextWord]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for word in words {
        *counts.entry(format!("w:{}", word.key)).or_default() += 1;
        let chars = word.key.chars().collect::<Vec<_>>();
        for gram in chars.windows(3) {
            *counts
                .entry(format!("g:{}", gram.iter().collect::<String>()))
                .or_default() += 1;
        }
    }
    counts
}
struct Passage {
    start: usize,
    end: usize,
    terms: HashMap<String, usize>,
}
struct Index {
    words: Vec<TextWord>,
    passages: Vec<Passage>,
    frequencies: HashMap<String, usize>,
    average: f64,
}
#[derive(Clone, Copy)]
enum Boundary {
    Start,
    End,
    Anywhere,
}
#[derive(Clone, Copy)]
struct Match {
    start: usize,
    end: usize,
}
impl Index {
    fn new(text: &str) -> Self {
        let words = text_words(text);
        let passages = (0..words.len())
            .step_by(32)
            .map(|start| {
                let end = (start + 160).min(words.len());
                Passage {
                    start,
                    end,
                    terms: terms(&words[start..end]),
                }
            })
            .collect::<Vec<_>>();
        let mut frequencies = HashMap::new();
        for passage in &passages {
            for term in passage.terms.keys() {
                *frequencies.entry(term.clone()).or_default() += 1;
            }
        }
        let average = passages
            .iter()
            .map(|p| (p.end - p.start) as f64)
            .sum::<f64>()
            / passages.len().max(1) as f64;
        Self {
            words,
            passages,
            frequencies,
            average,
        }
    }
    fn candidates(&self, context: &[TextWord]) -> BTreeSet<usize> {
        let query = terms(context);
        let count = self.passages.len() as f64;
        let mut ranked = self
            .passages
            .iter()
            .map(|passage| {
                let norm = 1.2
                    * (0.25 + 0.75 * (passage.end - passage.start) as f64 / self.average.max(1.));
                let score = query
                    .keys()
                    .map(|term| {
                        let tf = *passage.terms.get(term).unwrap_or(&0) as f64;
                        let df = *self.frequencies.get(term).unwrap_or(&0) as f64;
                        let idf = (1. + (count - df + 0.5) / (df + 0.5)).ln();
                        let weight = if term.starts_with("w:") { 1. } else { 0.15 };
                        weight * idf * tf * 2.2 / (tf + norm)
                    })
                    .sum::<f64>();
                (passage, score)
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let threshold = ranked.first().map_or(0., |(_, score)| score * 0.35);
        // Keep plausible runners-up as well as the winner, so repeated passages
        // cannot silently become a confident resume location.
        ranked
            .into_iter()
            .filter(|(_, score)| *score > 0. && *score >= threshold)
            .flat_map(|(p, _)| p.start..p.end)
            .collect()
    }
    fn align(&self, context: &[TextWord], boundary: Boundary) -> Result<Option<Match>, String> {
        if context.len() < 16 {
            return Ok(None);
        }
        let tolerance = ((context.len() as f64 * 0.08).floor() as usize)
            .max(if context.len() >= 24 { 2 } else { 1 })
            .min(8);
        let mut matches = HashMap::<usize, (usize, usize)>::new();
        // Exact matches also inspect the whole view, independent of retrieval rank.
        for window in self.words.windows(context.len()) {
            if window.iter().zip(context).all(|(a, b)| a.key == b.key) {
                let key = if matches!(boundary, Boundary::Start) {
                    window[0].start
                } else {
                    window.last().unwrap().end
                };
                matches.insert(key, (0, window[0].start));
            }
        }
        if matches.len() > 1 {
            return Err(ambiguous());
        }
        if let Some((&key, &(_, start))) = matches.iter().next() {
            return Ok(Some(Match {
                start,
                end: if matches!(boundary, Boundary::Start) {
                    self.words.iter().find(|w| w.start == key).unwrap().end
                } else {
                    key
                },
            }));
        }
        let mut best = tolerance + 1;
        for start in self.candidates(context) {
            if matches!(boundary, Boundary::Start) && self.words[start].key != context[0].key {
                continue;
            }
            for length in context.len().saturating_sub(tolerance)..=context.len() + tolerance {
                if length == 0 || start + length > self.words.len() {
                    continue;
                }
                let candidate = &self.words[start..start + length];
                if matches!(boundary, Boundary::End)
                    && candidate.last().unwrap().key != context.last().unwrap().key
                {
                    continue;
                }
                if matches!(boundary, Boundary::End) && !boundary_matches(context, candidate, true)
                {
                    continue;
                }
                if matches!(boundary, Boundary::Start)
                    && !boundary_matches(context, candidate, false)
                {
                    continue;
                }
                let prefix = &candidate[..candidate.len().min(6 + tolerance)];
                if context
                    .iter()
                    .take(6)
                    .filter(|w| !prefix.iter().any(|c| c.key == w.key))
                    .count()
                    > 2 * tolerance
                {
                    continue;
                }
                let score = distance(context, candidate, tolerance);
                let key = if matches!(boundary, Boundary::Start) {
                    candidate[0].start
                } else {
                    candidate.last().unwrap().end
                };
                if score < best {
                    best = score;
                    matches.clear();
                }
                if score == best && score <= tolerance {
                    matches.insert(key, (score, candidate[0].start));
                }
            }
        }
        if matches.len() > 1 {
            return Err(ambiguous());
        }
        Ok(matches
            .into_iter()
            .next()
            .map(|(key, (_, start))| Match { start, end: key }))
    }
}
fn ambiguous() -> String {
    "The saved passage appears more than once. Locate your saved position or choose Listen from this page.".into()
}
fn contexts(book: &Book, head: bool) -> Vec<Vec<TextWord>> {
    let mut values = if head {
        book.sentences
            .iter()
            .take(5)
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
    } else {
        book.sentences
            .iter()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
    };
    if !head || values.is_empty() {
        values.extend(book.pending.iter().map(|p| p.text.as_str()));
    }
    (1..=values.len())
        .rev()
        .map(|length| {
            let text = if head {
                values[..length].join(" ")
            } else {
                values[values.len() - length..].join(" ")
            };
            let words = text_words(&text);
            if head {
                words[..words.len().min(96)].to_vec()
            } else {
                words[words.len().saturating_sub(96)..].to_vec()
            }
        })
        .collect()
}
pub(super) fn continuation_offset(book: &Book, text: &str) -> Result<Option<usize>, String> {
    if book.pages.last().is_some_and(|p| p.text == text) {
        return Ok(Some(text.len()));
    }
    let index = Index::new(text);
    for context in contexts(book, false) {
        if let Some(found) = index.align(&context, Boundary::End)? {
            return Ok(Some(found.end));
        }
    }
    Ok(None)
}
// Earlier groups establish location only; their offsets are never append offsets.
pub(super) fn earlier_anchor(book: &Book, text: &str) -> Result<Option<(usize, usize)>, String> {
    let index = Index::new(text);
    let count = book.sentences.len();
    for end in (count.saturating_sub(24)..count.saturating_sub(1)).rev() {
        for length in (1..=5.min(end + 1)).rev() {
            let text = book.sentences[end + 1 - length..=end]
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let words = text_words(&text);
            let context = &words[words.len().saturating_sub(96)..];
            if let Some(found) = index.align(context, Boundary::End)? {
                return Ok(Some((end, found.end)));
            }
        }
    }
    Ok(None)
}
pub(super) fn head_offset(book: &Book, text: &str) -> Result<Option<usize>, String> {
    let index = Index::new(text);
    for context in contexts(book, true) {
        if let Some(found) = index.align(&context, Boundary::Start)? {
            return Ok(Some(found.start));
        }
    }
    Ok(None)
}
pub(super) fn page_matches(saved: &str, text: &str) -> Result<bool, String> {
    if saved == text {
        return Ok(true);
    }
    let words = text_words(saved);
    let index = Index::new(text);
    // Page metadata is only a navigation anchor. Verify an ordered passage,
    // preserving source offsets separately rather than rewriting saved pages.
    for context in [
        words[..words.len().min(96)].to_vec(),
        words[words.len().saturating_sub(96)..].to_vec(),
    ] {
        if index.align(&context, Boundary::Anywhere)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn contains_earlier_context(book: &Book, text: &str) -> bool {
    let index = Index::new(text);
    let incoming = triples(&index.words);
    let mut candidates = book
        .sentences
        .windows(2)
        .filter_map(|pair| {
            let words = text_words(&format!("{} {}", pair[0].text, pair[1].text));
            let context = words[words.len().saturating_sub(48)..].to_vec();
            if context.len() < 16 {
                return None;
            }
            let phrases = triples(&context);
            let shared = phrases.intersection(&incoming).count();
            (shared >= 2 && shared * 2 >= phrases.len()).then_some((shared, context))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(shared, _)| std::cmp::Reverse(*shared));
    // If many nearly identical saved passages are plausible, stop conservatively
    // rather than doing unbounded alignment under the library lock.
    if candidates.len() > 12 {
        return true;
    }
    candidates
        .into_iter()
        .any(|(_, context)| !matches!(index.align(&context, Boundary::Anywhere), Ok(None)))
}
