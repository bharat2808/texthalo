//! Native Kokoro Japanese and Mandarin text front ends.
//!
//! Kana and Mandarin IPA mappings are ported from hexgrad/misaki (Apache-2.0;
//! Mandarin transcription also retains its original MIT attribution). Japanese
//! readings/number normalization use JPreprocess's embedded BSD NAIST dictionary;
//! Mandarin readings use pinyin-pro's embedded MIT phrase dictionary. Neither
//! path needs Python, a subprocess, eSpeak, or a runtime dictionary download.

use jpreprocess::{
    kind::JPreprocessDictionaryKind, DefaultTokenizer, JPreprocess, SystemDictionaryConfig,
};
use pinyin_pro::options::{PinyinOptions, PinyinOutput, TypeMode};
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};
use unicode_normalization::UnicodeNormalization;

static CHINESE_WORDS: LazyLock<jieba_rs::Jieba> = LazyLock::new(jieba_rs::Jieba::new);
static JAPANESE: LazyLock<Result<JPreprocess<DefaultTokenizer>, String>> = LazyLock::new(|| {
    SystemDictionaryConfig::Bundled(JPreprocessDictionaryKind::NaistJdic)
        .load()
        .map(|dictionary| JPreprocess::with_dictionaries(dictionary, None))
        .map_err(|error| format!("could not load Japanese pronunciation dictionary: {error}"))
});
static KANA: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("cjk_data/ja_kana.json"))
        .expect("bundled Misaki kana mappings")
});
static CHINESE_IPA: LazyLock<HashMap<String, HashMap<String, String>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("cjk_data/zh_ipa.json"))
        .expect("bundled Misaki Mandarin mappings")
});

pub fn supports(voice: &str) -> bool {
    voice.starts_with("jf_")
        || voice.starts_with("jm_")
        || voice.starts_with("zf_")
        || voice.starts_with("zm_")
}

pub fn preview_text(voice: &str) -> Option<&'static str> {
    if voice.starts_with('j') && supports(voice) {
        Some("こんにちは。選択した文章をこの声で読み上げます。")
    } else if voice.starts_with('z') && supports(voice) {
        Some("你好，我会用这个声音朗读你选择的文字。")
    } else {
        None
    }
}

/// Preserve mixed English through the existing English front end, rather than
/// passing Latin spelling or unconverted Han characters to the Kokoro tokenizer.
pub fn phonemize(
    voice: &str,
    text: &str,
    mut english: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    if !supports(voice) {
        return Err(format!("{voice} is not a Japanese or Mandarin voice"));
    }
    let text: String = text.nfkc().collect();
    let chars: Vec<char> = text.chars().collect();
    let mut result = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let latin = chars[start].is_ascii_alphabetic();
        let mut end = start + 1;
        while end < chars.len() && chars[end].is_ascii_alphabetic() == latin {
            end += 1;
        }
        let segment: String = chars[start..end].iter().collect();
        let phones = if latin {
            english(&segment)?
        } else if voice.starts_with('j') {
            japanese(&segment)?
        } else {
            mandarin(&segment)?
        };
        if !phones.trim().is_empty() {
            result.push(phones);
        }
        start = end;
    }
    Ok(result.join(" "))
}

fn punctuation(c: char) -> Option<&'static str> {
    match c {
        '、' | '，' | ',' => Some(","),
        '。' | '．' | '.' => Some("."),
        '！' | '!' => Some("!"),
        '？' | '?' => Some("?"),
        '：' | ':' => Some(":"),
        '；' | ';' => Some(";"),
        '「' | '『' | '《' | '“' | '«' => Some("“"),
        '」' | '』' | '》' | '”' | '»' => Some("”"),
        '（' | '(' => Some("«"),
        '）' | ')' => Some("»"),
        '—' | '-' | '〜' | '～' => Some("—"),
        '/' => Some(","),
        '…' => Some("…"),
        '\"' => Some("\""),
        '\'' => Some("'"),
        '・' => Some(" "),
        _ if c.is_whitespace() => Some(" "),
        _ => None,
    }
}

fn japanese(text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let processor = JAPANESE.as_ref().map_err(Clone::clone)?;
    let mut njd = processor
        .text_to_njd(text)
        .map_err(|error| format!("could not read Japanese text: {error}"))?;
    njd.preprocess();
    let mut result = String::new();
    for node in &njd.nodes {
        let surface = node.get_string();
        if surface.chars().all(|c| punctuation(c).is_some()) {
            result = result.trim_end().to_string();
            for c in surface.chars() {
                result.push_str(punctuation(c).unwrap());
            }
            result.push(' ');
            continue;
        }
        let reading = node.get_pron().to_pure_string();
        if reading.is_empty() {
            return Err(format!(
                "could not determine Japanese pronunciation for “{surface}”"
            ));
        }
        let phones = kana_to_ipa(&reading)?;
        result.push_str(&phones);
        result.push(' ');
    }
    Ok(result.trim().to_string())
}

/// Misaki cutlet's kana conversion, including digraphs, moraic nasal assimilation,
/// long vowels and the glottal stop used by Kokoro for a small tsu.
fn kana_to_ipa(reading: &str) -> Result<String, String> {
    let chars: Vec<char> = reading
        .chars()
        .filter(|c| *c != '’')
        .map(|c| {
            if ('ァ'..='ヶ').contains(&c) {
                char::from_u32(c as u32 - 0x60).unwrap()
            } else {
                c
            }
        })
        .collect();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        if c == 'ー' {
            result.push('ː');
        } else if c == 'っ' {
            result.push('ʔ');
        } else if c == 'ん' {
            let following = next
                .and_then(|n| KANA.get(&n.to_string()))
                .map(String::as_str)
                .unwrap_or("");
            result.push(if following.starts_with(['m', 'p', 'b']) {
                'm'
            } else if following.starts_with(['k', 'ɡ']) {
                'ŋ'
            } else if following.starts_with(['ɲ', 'ʨ', 'ʥ']) {
                'ɲ'
            } else if following.starts_with(['n', 't', 'd', 'ɾ', 'z']) {
                'n'
            } else {
                'ɴ'
            });
        } else if let Some(pair) = next.and_then(|n| KANA.get(&format!("{c}{n}"))) {
            result.push_str(pair);
            index += 1;
        } else if let Some(phone) = KANA.get(&c.to_string()) {
            if let Some(small) = next.filter(|c| "ゃゅょぁぃぅぇぉ".contains(*c)) {
                let mut consonant = phone.clone();
                consonant.pop();
                result.push_str(&consonant);
                result.push_str(&KANA[&small.to_string()]);
                index += 1;
            } else if !"ゃゅょぁぃぅぇぉ".contains(c) {
                result.push_str(phone);
            }
        } else {
            return Err(format!("unsupported Japanese reading character: {c}"));
        }
        index += 1;
    }
    Ok(result)
}

fn mandarin(text: &str) -> Result<String, String> {
    let normalized = normalize_chinese_numbers(text);
    // Resolve readings over the full context, then apply Misaki's word spacing.
    // This preserves polyphonic phrase readings even when Jieba splits traditional text.
    let boundaries: HashSet<usize> = CHINESE_WORDS
        .cut(&normalized, false)
        .into_iter()
        .map(|word| word.end)
        .collect();
    let PinyinOutput::All(readings) = pinyin_pro::pinyin(
        &normalized,
        PinyinOptions {
            type_mode: TypeMode::All,
            traditional: true,
            tone_sandhi: false,
            ..Default::default()
        },
    ) else {
        return Err("Mandarin dictionary returned an invalid reading".into());
    };
    let mut result = String::new();
    let mut position = 0;
    for reading in readings {
        position += reading.origin.chars().count();
        if reading.is_zh {
            result.push_str(&pinyin_to_ipa(&reading.pinyin)?);
        } else {
            for c in reading.origin.chars() {
                if let Some(mark) = punctuation(c) {
                    result = result.trim_end().to_string();
                    result.push_str(mark);
                } else {
                    return Err(format!(
                        "could not determine Mandarin pronunciation for “{c}”"
                    ));
                }
            }
        }
        if boundaries.contains(&position) {
            result.push(' ');
        }
    }
    Ok(result.trim().to_string())
}

fn pinyin_to_ipa(pinyin: &str) -> Result<String, String> {
    use pinyin_pro::pinyin_utils::{num_of_tone, strip_tone};
    let tone = match num_of_tone(pinyin).as_str() {
        "1" => "→",
        "2" => "↗",
        "3" => "↓",
        "4" => "↘",
        "0" | "5" => "",
        _ => return Err(format!("invalid Mandarin tone: {pinyin}")),
    };
    let plain = strip_tone(pinyin);
    for special in ["INTERJECTION_MAPPINGS", "SYLLABIC_CONSONANT_MAPPINGS"] {
        if let Some(phone) = CHINESE_IPA.get(special).and_then(|map| map.get(&plain)) {
            return Ok(phone.replace('0', tone));
        }
    }
    // Pypinyin's strict finals expand contracted endings and y/w orthography.
    let (initial, mut final_) = if let Some(rest) = plain.strip_prefix('y') {
        (
            "",
            match rest {
                "i" => "i".into(),
                "in" => "in".into(),
                "ing" => "ing".into(),
                s if s.starts_with('u') => s.replacen('u', "ü", 1),
                s => format!("i{s}"),
            },
        )
    } else if let Some(rest) = plain.strip_prefix('w') {
        (
            "",
            if rest == "u" {
                "u".into()
            } else {
                format!("u{rest}")
            },
        )
    } else {
        let initial = [
            "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
            "r", "z", "c", "s",
        ]
        .into_iter()
        .find(|initial| plain.starts_with(initial))
        .unwrap_or("");
        (initial, plain[initial.len()..].to_string())
    };
    if ["j", "q", "x"].contains(&initial) && final_.starts_with('u') {
        final_ = final_.replacen('u', "ü", 1);
    }
    final_ = match final_.as_str() {
        "iu" => "iou".into(),
        "ui" => "uei".into(),
        "un" => "uen".into(),
        _ => final_,
    };
    let onset = if initial.is_empty() {
        ""
    } else {
        CHINESE_IPA["INITIAL_MAPPING"]
            .get(initial)
            .map(String::as_str)
            .ok_or_else(|| format!("unsupported Mandarin initial: {pinyin}"))?
    };
    let nucleus = if final_ == "i" && ["zh", "ch", "sh", "r", "z", "c", "s"].contains(&initial) {
        "ɨ0"
    } else {
        CHINESE_IPA["FINAL_MAPPING"]
            .get(&final_)
            .map(String::as_str)
            .ok_or_else(|| format!("unsupported Mandarin final: {pinyin}"))?
    };
    Ok(format!("{onset}{nucleus}")
        .replace('0', tone)
        .replace('\u{032f}', ""))
}

/// Cardinal numbers and decimals are pronounced before character lookup. Years,
/// leading-zero identifiers, and long digit sequences use digit readings.
fn normalize_chinese_numbers(text: &str) -> String {
    const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        let digits: String = chars[start..index].iter().collect();
        let digitwise = (digits.len() > 1 && digits.starts_with('0'))
            || digits.len() > 16
            || chars.get(index) == Some(&'年');
        if digitwise {
            for d in digits.bytes() {
                out.push_str(DIGITS[(d - b'0') as usize]);
            }
        } else {
            out.push_str(&chinese_cardinal(digits.parse().unwrap_or(0)));
        }
        if chars.get(index) == Some(&'.') && chars.get(index + 1).is_some_and(char::is_ascii_digit)
        {
            out.push('点');
            index += 1;
            while index < chars.len() && chars[index].is_ascii_digit() {
                out.push_str(DIGITS[(chars[index] as u8 - b'0') as usize]);
                index += 1;
            }
        }
    }
    out
}

fn chinese_cardinal(value: u64) -> String {
    const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if value == 0 {
        return "零".into();
    }
    for (unit, name) in [(100_000_000, "亿"), (10_000, "万")] {
        if value >= unit {
            let remainder = value % unit;
            let mut result = chinese_cardinal(value / unit) + name;
            if remainder > 0 {
                if remainder < unit / 10 {
                    result.push('零');
                }
                result.push_str(&chinese_cardinal(remainder));
            }
            return result;
        }
    }
    let mut out = String::new();
    let mut zero = false;
    for (unit, name) in [(1000, "千"), (100, "百"), (10, "十"), (1, "")] {
        let digit = (value / unit % 10) as usize;
        if digit == 0 {
            zero = !out.is_empty();
            continue;
        }
        if zero {
            out.push('零');
            zero = false;
        }
        if !(unit == 10 && digit == 1 && out.is_empty()) {
            out.push_str(DIGITS[digit]);
        }
        out.push_str(name);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kana_matches_misaki_cutlet_inventory() {
        assert_eq!(kana_to_ipa("コンニチワ").unwrap(), "koɲɲiʨiβa");
        assert_eq!(kana_to_ipa("キャット").unwrap(), "kʲaʔto");
        assert_eq!(kana_to_ipa("コーヒー").unwrap(), "koːçiː");
        assert_eq!(kana_to_ipa("シンブン").unwrap(), "ɕimbɯɴ");
        assert_eq!(kana_to_ipa("キィ").unwrap(), "kʲi");
    }
    #[test]
    fn mandarin_ipa_matches_misaki_tones_and_apical_vowels() {
        for (pinyin, ipa) in [
            ("nǐ", "ni↓"),
            ("hǎo", "xau↓"),
            ("zhōng", "ꭧʊ→ŋ"),
            ("shì", "ʂɨ↘"),
            ("xué", "ɕɥe↗"),
            ("yuán", "ɥɛ↗n"),
            ("guì", "kwei↘"),
            ("yún", "y↗n"),
        ] {
            assert_eq!(pinyin_to_ipa(pinyin).unwrap(), ipa, "{pinyin}");
        }
    }
    #[test]
    fn japanese_kanji_kana_and_numbers_work_with_embedded_dictionary() {
        for text in [
            "こんにちは。",
            "日本語を話します。",
            "今日は2026年10月6日です。",
            "ｺｰﾋｰを３杯ください。",
        ] {
            let phones = phonemize("jf_alpha", text, |_| panic!("no English in fixture")).unwrap();
            assert!(!phones.is_empty());
            assert!(!phones
                .chars()
                .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        }
    }
    #[test]
    fn mandarin_polyphones_numbers_and_mixed_english_use_their_own_paths() {
        assert_eq!(
            normalize_chinese_numbers("2026年有13个，3.14元和007号"),
            "二零二六年有十三个，三点一四元和零零七号"
        );
        let phones = phonemize(
            "zf_xiaobei",
            "银行行长说你好，价格13元。",
            |_| panic!("no English in fixture"),
        )
        .unwrap();
        assert!(phones.contains("xa↗ŋ"));
        assert!(phones.contains("ꭧa↓ŋ"));
        let mixed = phonemize("zf_xiaobei", "你好TextHalo。", |word| {
            assert_eq!(word, "TextHalo");
            Ok("ENGLISH".into())
        })
        .unwrap();
        assert!(mixed.contains("ENGLISH"));
        assert!(mixed.contains("ni↓"));
    }
    #[test]
    fn empty_input_is_empty_and_unsupported_readings_do_not_silently_drop_text() {
        assert_eq!(phonemize("jf_alpha", "  ", |_| unreachable!()).unwrap(), "");
        assert_eq!(phonemize("zf_xiaobei", "", |_| unreachable!()).unwrap(), "");
        assert!(phonemize("af_heart", "hello", |_| unreachable!()).is_err());
        assert!(phonemize("zf_xiaobei", "🙂", |_| unreachable!()).is_err());
    }
}
