//! Interface language is independent of synthesis and recognition languages.
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguagePreference {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "fr-CA")]
    French,
    #[serde(rename = "es-419")]
    Spanish,
    #[default]
    #[serde(other, rename = "system")]
    System,
}

#[derive(Clone, Serialize)]
pub struct LocalizationState {
    pub preference: LanguagePreference,
    pub language: &'static str,
}

pub fn resolve(preference: LanguagePreference, languages: &[String]) -> &'static str {
    match preference {
        LanguagePreference::English => return "en",
        LanguagePreference::French => return "fr-CA",
        LanguagePreference::Spanish => return "es-419",
        LanguagePreference::System => {}
    }
    for locale in languages {
        match locale.to_lowercase().split(['-', '_']).next() {
            Some("en") => return "en",
            Some("fr") => return "fr-CA",
            Some("es") => return "es-419",
            _ => {}
        }
    }
    "en"
}

fn parse_languages(raw: &str) -> Vec<String> {
    raw.trim_matches(['(', ')', '\n', ' '])
        .split(',')
        .map(|locale| locale.trim().trim_matches('"').to_string())
        .filter(|locale| !locale.is_empty())
        .collect()
}

fn read_default(key: &str) -> Option<String> {
    let output = Command::new("/usr/bin/defaults")
        .args(["read", "-g", key])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn state(preference: LanguagePreference) -> LocalizationState {
    // AppleLanguages expresses preferred interface languages; AppleLocale expresses region.
    let languages = read_default("AppleLanguages")
        .map(|raw| parse_languages(&raw))
        .filter(|languages| !languages.is_empty())
        .or_else(|| read_default("AppleLocale").map(|locale| vec![locale]))
        .unwrap_or_default();
    LocalizationState {
        preference,
        language: resolve(preference, &languages),
    }
}

pub fn translate(source: &str, language: &str, values: &[&str]) -> String {
    static FRENCH: OnceLock<serde_json::Value> = OnceLock::new();
    static SPANISH: OnceLock<serde_json::Value> = OnceLock::new();
    let catalog = match language {
        "fr-CA" => Some(FRENCH.get_or_init(|| {
            serde_json::from_str(include_str!("../../src/locales/fr-CA.json"))
                .expect("valid French catalog")
        })),
        "es-419" => Some(SPANISH.get_or_init(|| {
            serde_json::from_str(include_str!("../../src/locales/es-419.json"))
                .expect("valid Spanish catalog")
        })),
        _ => None,
    };
    let text = catalog
        .and_then(|catalog| catalog.get(source))
        .and_then(|text| text.as_str())
        .unwrap_or(source);
    // Substitute once so values containing placeholders are preserved verbatim.
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find('{') {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        if let Some(end) = remaining.find('}') {
            if let Ok(index) = remaining[1..end].parse::<usize>() {
                if let Some(value) = values.get(index) {
                    result.push_str(value);
                    remaining = &remaining[end + 1..];
                    continue;
                }
            }
        }
        result.push('{');
        remaining = &remaining[1..];
    }
    result.push_str(remaining);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_resolve_in_order_and_manual_choice_wins() {
        let languages = vec!["de-DE".into(), "fr_CA".into(), "es-MX".into()];
        assert_eq!(resolve(LanguagePreference::System, &languages), "fr-CA");
        assert_eq!(resolve(LanguagePreference::English, &languages), "en");
        assert_eq!(
            resolve(LanguagePreference::System, &["es-MX".into()]),
            "es-419"
        );
        assert_eq!(resolve(LanguagePreference::System, &["ja-JP".into()]), "en");
        assert_eq!(resolve(LanguagePreference::System, &[]), "en");
    }
    #[test]
    fn parses_macos_preferred_language_list() {
        assert_eq!(
            parse_languages("(\n    \"fr-CA\",\n    \"en-CA\"\n)"),
            vec!["fr-CA", "en-CA"]
        );
    }
    #[test]
    fn legacy_and_unknown_preferences_use_system() {
        assert_eq!(
            serde_json::from_str::<LanguagePreference>("\"obsolete\"").unwrap(),
            LanguagePreference::System
        );
    }
    #[test]
    fn tray_translation_preserves_interpolated_values() {
        assert_eq!(translate("Settings…", "fr-CA", &[]), "Paramètres…");
        assert_eq!(
            translate("Preview {0}", "es-419", &["My {1} voice"]),
            "Escuchar My {1} voice"
        );
        assert_eq!(
            translate("technical detail", "fr-CA", &[]),
            "technical detail"
        );
    }
}
