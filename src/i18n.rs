//! The app's languages: its `locales/` files, the saved choice, and the
//! names the language menu shows.
use nsh_core::{i18n::Translations, platform::Locale, storage::Storage};
use std::collections::HashMap;

/// Where the chosen language is kept (`system` follows the OS).
pub const LANG_KEY: &str = "sessiontree.lang";

/// The language the menu shows as "follow the system".
pub const SYSTEM: &str = "system";

/// The languages the app ships, by BCP 47 tag and their own names.
pub const LANGUAGES: [(&str, &str); 3] = [("en", "English"), ("zh", "简体中文"), ("ja", "日本語")];

/// The app's translations, merged from `locales/<tag>.json`.
pub fn translations() -> Translations {
    let mut map = HashMap::new();
    for (tag, file) in [
        ("en", include_str!("../locales/en.json")),
        ("zh", include_str!("../locales/zh.json")),
        ("ja", include_str!("../locales/ja.json")),
    ] {
        let texts = serde_json::from_str(file)
            .unwrap_or_else(|error| panic!("locales/{tag}.json: {error}"));
        map.insert(tag.to_owned(), texts);
    }
    Translations::from_map(map)
}

/// The language to show on launch: `SESSIONTREE_LANG`, the saved choice, or
/// the system's.
pub fn initial_tag() -> String {
    if let Ok(tag) = std::env::var("SESSIONTREE_LANG") {
        return normalize(&tag);
    }
    Storage::get_item(LANG_KEY)
        .map(|tag| normalize(&tag))
        .unwrap_or_else(|| SYSTEM.to_owned())
}

/// Shows the app in `tag` (`system` follows the OS) and remembers it.
pub fn choose(tag: &str) {
    let tag = normalize(tag);
    let _ = Storage::set_item(LANG_KEY, &tag);
    activate(&tag);
}

/// Shows the app in `tag` without saving it.
pub fn activate(tag: &str) {
    let locale = if normalize(tag) == SYSTEM {
        None
    } else {
        Locale::parse(&tag)
    };
    nsh_core::i18n::set_locale(locale);
}

/// The language actually shown: what the system and the saved choice
/// resolve to, as a full tag (`en-US`, `zh`, `ja`).
pub fn resolved_tag() -> String {
    let translations = nsh_core::element::global::<Translations>().get();
    let settings = nsh_core::element::locale_settings();
    translations
        .pick(settings.languages.iter())
        .unwrap_or_else(nsh_core::element::locale)
        .to_string()
}

/// Whether `current` (a tag `resolved_tag` gives) is the language `tag`.
pub fn matches(current: &str, tag: &str) -> bool {
    current == tag
        || current
            .strip_prefix(tag)
            .is_some_and(|rest| rest.starts_with('-'))
}

/// The short code the toolbar shows for a language tag.
pub fn short_code(tag: &str) -> &'static str {
    match tag.split('-').next().unwrap_or_default() {
        "zh" => "中",
        "ja" => "日",
        _ => "EN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_english_key() {
        let english: HashMap<String, String> =
            serde_json::from_str(include_str!("../locales/en.json")).unwrap();
        for (tag, file) in [
            ("zh", include_str!("../locales/zh.json")),
            ("ja", include_str!("../locales/ja.json")),
        ] {
            let texts: HashMap<String, String> = serde_json::from_str(file).unwrap();
            let mut missing: Vec<&String> = english
                .keys()
                .filter(|key| !texts.contains_key(*key))
                .collect();
            missing.sort();
            assert!(missing.is_empty(), "{tag} is missing {missing:?}");
        }
    }

    #[test]
    fn tags_match_their_regions_and_short_codes() {
        assert!(matches("zh-Hans-CN", "zh"));
        assert!(matches("en-US", "en"));
        assert!(!matches("en-US", "zh"));
        assert_eq!(short_code("zh-Hant-TW"), "中");
        assert_eq!(short_code("ja-JP"), "日");
        assert_eq!(normalize("zh-CN"), "zh");
        assert_eq!(normalize("fr"), SYSTEM);
    }
}

/// `tag` as one we ship, else `system`.
fn normalize(tag: &str) -> String {
    let tag = tag.trim();
    LANGUAGES
        .iter()
        .map(|(tag, _)| *tag)
        .find(|known| matches(tag, known))
        .map(str::to_owned)
        .unwrap_or_else(|| SYSTEM.to_owned())
}
