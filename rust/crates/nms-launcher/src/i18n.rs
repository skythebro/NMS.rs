//! Manager strings. `loader/locales/en.json` is the source; other files are translations.
//! A missing key falls back to English, then to the key itself.

use serde_json::Value;
use std::collections::HashMap;

pub const DEFAULT: &str = "en";

/// (code, English name, native name, file text). The picker shows the native name when the
/// active fonts can draw it (CJK names need a system font, see `fonts`), else the English one.
const LOCALES: &[(&str, &str, &str, &str)] = &[
    ("en", "English", "English", include_str!("../../../../loader/locales/en.json")),
    ("de", "German", "Deutsch", include_str!("../../../../loader/locales/de.json")),
    ("fr", "French", "Français", include_str!("../../../../loader/locales/fr.json")),
    ("it", "Italian", "Italiano", include_str!("../../../../loader/locales/it.json")),
    ("es", "Spanish", "Español", include_str!("../../../../loader/locales/es.json")),
    ("es-419", "Spanish (Latin America)", "Español (Latinoamérica)", include_str!("../../../../loader/locales/es-419.json")),
    ("ru", "Russian", "Русский", include_str!("../../../../loader/locales/ru.json")),
    ("pl", "Polish", "Polski", include_str!("../../../../loader/locales/pl.json")),
    ("nl", "Dutch", "Nederlands", include_str!("../../../../loader/locales/nl.json")),
    ("pt", "Portuguese", "Português", include_str!("../../../../loader/locales/pt.json")),
    ("pt-BR", "Portuguese (Brazil)", "Português (Brasil)", include_str!("../../../../loader/locales/pt-BR.json")),
    ("ja", "Japanese", "日本語", include_str!("../../../../loader/locales/ja.json")),
    ("zh", "Chinese (Simplified)", "简体中文", include_str!("../../../../loader/locales/zh.json")),
    ("zh-Hant", "Chinese (Traditional)", "繁體中文", include_str!("../../../../loader/locales/zh-Hant.json")),
    ("ko", "Korean", "한국어", include_str!("../../../../loader/locales/ko.json")),
];

/// Typographic characters the manager font may lack, mapped to plain ones (same list as the
/// Python manager). Em and en dashes become a colon so no dash reaches the user.
const PLAIN: &[(&str, &str)] = &[
    ("\u{2018}", "'"),
    ("\u{2019}", "'"),
    ("\u{201a}", "'"),
    ("\u{201b}", "'"),
    ("\u{02bc}", "'"),
    ("\u{201c}", "\""),
    ("\u{201d}", "\""),
    ("\u{201e}", "\""),
    ("\u{201f}", "\""),
    ("\u{00ab}", "\""),
    ("\u{00bb}", "\""),
    ("\u{2039}", "'"),
    ("\u{203a}", "'"),
    ("\u{2014}", ": "),
    ("\u{2013}", ": "),
    ("\u{2212}", "-"),
    ("\u{00d7}", "x"),
    ("\u{2026}", "..."),
    ("\u{00a0}", " "),
    ("\u{202f}", " "),
];

fn plain(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in PLAIN {
        out = out.replace(from, to);
    }
    out
}

fn parse(text: &str) -> HashMap<String, String> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
        return HashMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, plain(s))))
        .collect()
}

pub struct I18n {
    code: &'static str,
    english: HashMap<String, String>,
    strings: HashMap<String, String>,
}

impl I18n {
    pub fn new(code: &str) -> Self {
        let english = parse(LOCALES[0].3);
        let mut this = Self {
            code: DEFAULT,
            english,
            strings: HashMap::new(),
        };
        this.set(code);
        this
    }

    pub fn code(&self) -> &str {
        self.code
    }

    pub fn set(&mut self, code: &str) {
        let row = LOCALES.iter().find(|(c, _, _, _)| *c == code).unwrap_or(&LOCALES[0]);
        self.code = row.0;
        self.strings = if row.0 == DEFAULT { HashMap::new() } else { parse(row.3) };
    }

    pub fn t(&self, key: &str) -> String {
        self.strings
            .get(key)
            .or_else(|| self.english.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// Short text under a mod name. Empty when the mod has none.
    pub fn mod_description(&self, name: &str) -> String {
        let key = format!("mod.{name}");
        let text = self.t(&key);
        if text == key {
            String::new()
        } else {
            text
        }
    }
}

/// (code, English name, native name) for every language, in picker order.
pub fn codes() -> Vec<(&'static str, &'static str, &'static str)> {
    LOCALES.iter().map(|(c, e, n, _)| (*c, *e, *n)).collect()
}

/// Picker text for a language: the native name when `can_draw` says the fonts cover it.
pub fn picker_name(english: &'static str, native: &'static str, can_draw: impl FnOnce(&str) -> bool) -> &'static str {
    if native == english || can_draw(native) {
        native
    } else {
        english
    }
}

/// (code, file text) of every locale, for the font coverage test.
#[cfg(test)]
pub fn locale_texts() -> Vec<(&'static str, &'static str)> {
    LOCALES.iter().map(|(c, _, _, t)| (*c, *t)).collect()
}

/// Every string the user can see from one locale file, after `plain`.
#[cfg(test)]
pub fn locale_values(text: &str) -> Vec<String> {
    parse(text).into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_has_every_mod_description() {
        let i = I18n::new("en");
        for name in crate::mods::OUR_MODS {
            assert!(!i.mod_description(name).is_empty(), "no description for {name}");
        }
    }

    #[test]
    fn every_locale_file_parses() {
        for (code, _, _, text) in LOCALES {
            assert!(!parse(text).is_empty(), "{code} did not parse");
        }
    }

    #[test]
    fn missing_key_falls_back_to_english_then_key() {
        let mut i = I18n::new("de");
        assert_ne!(i.t("play"), "play");
        i.strings.remove("play");
        assert_eq!(i.t("play"), "Play");
        assert_eq!(i.t("no.such.key"), "no.such.key");
    }

    #[test]
    fn picker_falls_back_to_english_name() {
        assert_eq!(picker_name("Korean", "한국어", |_| false), "Korean");
        assert_eq!(picker_name("Korean", "한국어", |_| true), "한국어");
        assert_eq!(picker_name("English", "English", |_| false), "English");
    }

    #[test]
    fn no_dash_reaches_the_user() {
        for (code, _, _, text) in LOCALES {
            for (key, value) in parse(text) {
                assert!(!value.contains('\u{2014}'), "{code} {key}");
            }
        }
    }
}
