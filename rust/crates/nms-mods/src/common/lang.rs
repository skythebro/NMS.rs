//! The language the player picked in the game, as one of our locale codes.
//!
//! Source: `Binaries\SETTINGS\GCUSERSETTINGSDATA.MXML`, property `Language` (a `TkLanguages`
//! block holding an inner `Language` value). The game rewrites that file itself, so a read at
//! the time the Mods page opens sees the current choice. `Default` means "follow Steam", so we
//! fall back to `language` in `steamapps\appmanifest_275850.acf` (Steam's per game language),
//! then to English. Nothing here reads game memory, so a game patch cannot break it.

use nms_core::log::Log;
use nms_core::process::ProcessImage;
use std::path::{Path, PathBuf};

/// The code last logged (compared by address: the codes are string literals).
static LAST: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const USER_SETTINGS: &str = "GCUSERSETTINGSDATA.MXML";
const APP_MANIFEST: &str = "appmanifest_275850.acf";

/// The current UI language as a locale code (`loader/locales`), "en" when unknown.
/// Cheap (one small file read, two on `Default`), safe to call each time the page opens.
pub fn detect(_image: &ProcessImage, log: &Log) -> &'static str {
    let Some(root) = nms_core::paths::game_root() else {
        return "en";
    };
    let (code, how) = detect_in(&root);
    // Logged only when it changes: this runs every couple of seconds while the Options list is open.
    if LAST.swap(code.as_ptr() as usize, std::sync::atomic::Ordering::Relaxed) != code.as_ptr() as usize {
        log.info(&format!("[lang] game language {code} ({how})"));
    }
    code
}

fn detect_in(root: &Path) -> (&'static str, &'static str) {
    let settings = root.join("Binaries").join("SETTINGS").join(USER_SETTINGS);
    if let Some(name) = std::fs::read_to_string(&settings).ok().and_then(|t| parse_settings(&t)) {
        if !name.eq_ignore_ascii_case("default") {
            if let Some(code) = from_game_name(&name) {
                return (code, "user settings");
            }
        }
    }
    let manifest: PathBuf = root.parent().and_then(|p| p.parent()).map(|p| p.join(APP_MANIFEST)).unwrap_or_default();
    if let Some(name) = std::fs::read_to_string(&manifest).ok().and_then(|t| parse_manifest(&t)) {
        if let Some(code) = from_game_name(&name) {
            return (code, "steam");
        }
    }
    ("en", "fallback")
}

/// The inner `Language` value of the `TkLanguages` property.
pub fn parse_settings(xml: &str) -> Option<String> {
    let at = xml.find(r#"value="TkLanguages""#)?;
    let rest = &xml[at..];
    let end = rest.find("</Property>").unwrap_or(rest.len());
    let block = &rest[..end];
    let p = block.find(r#"name="Language""#)?;
    let tail = &block[p..];
    let v = tail.find(r#"value=""#)? + 7;
    let tail = &tail[v..];
    Some(tail[..tail.find('"')?].to_string())
}

/// The `language` value under `UserConfig` (else the first `language` key) of an appmanifest.
pub fn parse_manifest(acf: &str) -> Option<String> {
    let scan = |text: &str| -> Option<String> {
        for line in text.lines() {
            let mut q = line.split('"').filter(|s| !s.trim().is_empty());
            if q.next().map(|k| k.eq_ignore_ascii_case("language")) == Some(true) {
                if let Some(v) = q.next() {
                    return Some(v.to_string());
                }
            }
        }
        None
    };
    match acf.find("\"UserConfig\"") {
        Some(i) => scan(&acf[i..]).or_else(|| scan(acf)),
        None => scan(acf),
    }
}

/// Maps the game's `TkLanguages` names and Steam language names to our locale codes.
pub fn from_game_name(name: &str) -> Option<&'static str> {
    let n: String = name.trim().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    Some(match n.as_str() {
        "english" | "usenglish" | "en" | "enus" | "engb" => "en",
        "french" | "fr" => "fr",
        "italian" | "it" => "it",
        "german" | "de" => "de",
        "spanish" | "es" => "es",
        "latinamericanspanish" | "latam" | "es419" | "esmx" => "es-419",
        "russian" | "ru" => "ru",
        "polish" | "pl" => "pl",
        "dutch" | "nl" => "nl",
        "portuguese" | "pt" | "ptpt" => "pt",
        "brazilianportuguese" | "brazilian" | "ptbr" => "pt-BR",
        "japanese" | "ja" | "jp" => "ja",
        "korean" | "koreana" | "ko" => "ko",
        "simplifiedchinese" | "schinese" | "tencentchinese" | "zh" | "zhcn" | "zhhans" => "zh",
        "traditionalchinese" | "tchinese" | "zhhant" | "zhtw" | "zhhk" => "zh-Hant",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map() {
        for (n, c) in [
            ("English", "en"), ("USEnglish", "en"), ("French", "fr"), ("Italian", "it"), ("German", "de"),
            ("Spanish", "es"), ("LatinAmericanSpanish", "es-419"), ("latam", "es-419"), ("Russian", "ru"),
            ("Polish", "pl"), ("Dutch", "nl"), ("Portuguese", "pt"), ("BrazilianPortuguese", "pt-BR"),
            ("brazilian", "pt-BR"), ("Japanese", "ja"), ("Korean", "ko"), ("koreana", "ko"),
            ("SimplifiedChinese", "zh"), ("TencentChinese", "zh"), ("schinese", "zh"),
            ("TraditionalChinese", "zh-Hant"), ("tchinese", "zh-Hant"), ("zh-Hant", "zh-Hant"), ("pt-BR", "pt-BR"),
        ] {
            assert_eq!(from_game_name(n), Some(c), "{n}");
        }
        assert_eq!(from_game_name("Default"), None);
        assert_eq!(from_game_name(""), None);
    }

    #[test]
    fn settings_parse() {
        let xml = "<Property name=\"X\" value=\"Y\" />\n\t<Property name=\"Language\" value=\"TkLanguages\">\n\t\t<Property name=\"Language\" value=\"German\" />\n\t</Property>\n\t<Property name=\"Other\" value=\"Q\" />";
        assert_eq!(parse_settings(xml).as_deref(), Some("German"));
        assert_eq!(parse_settings("<Data/>"), None);
        assert_eq!(parse_settings("value=\"TkLanguages\">"), None);
    }

    #[test]
    fn manifest_parse() {
        let acf = "\"AppState\"\n{\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"french\"\n\t\t\"BetaKey\"\t\t\"public\"\n\t}\n}";
        assert_eq!(parse_manifest(acf).as_deref(), Some("french"));
        assert_eq!(parse_manifest("\"AppState\" { }"), None);
    }

    #[test]
    fn detect_in_files() {
        let dir = std::env::temp_dir().join(format!("nms_lang_test_{}", std::process::id()));
        let root = dir.join("steamapps").join("common").join("NMS");
        let set = root.join("Binaries").join("SETTINGS");
        std::fs::create_dir_all(&set).unwrap();
        let mk = |v: &str| format!("<Property name=\"Language\" value=\"TkLanguages\">\n<Property name=\"Language\" value=\"{v}\" />\n</Property>");
        std::fs::write(set.join(USER_SETTINGS), mk("Korean")).unwrap();
        assert_eq!(detect_in(&root).0, "ko");
        std::fs::write(set.join(USER_SETTINGS), mk("Default")).unwrap();
        assert_eq!(detect_in(&root).0, "en");
        std::fs::write(dir.join("steamapps").join(APP_MANIFEST), "\"UserConfig\"\n{\n\"language\" \"koreana\"\n}").unwrap();
        assert_eq!(detect_in(&root), ("ko", "steam"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
