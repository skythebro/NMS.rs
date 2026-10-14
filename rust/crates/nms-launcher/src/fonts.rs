//! Extra fonts for the manager window.
//!
//! egui's built-in fonts (Ubuntu-Light, Hack) cover Latin, Latin-extended, Cyrillic and Greek,
//! but no Korean, Japanese or Chinese. For those languages a Windows system font is read from
//! disk when the language is active and added as a fallback after the built-in fonts, in both
//! the proportional and the monospace family. The files are 10 to 20 MB, so they are never
//! embedded in the exe and never read for the other languages.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};
use skrifa::MetadataProvider;

/// Key of the system font in `FontDefinitions::font_data`.
pub const NAME: &str = "system-cjk";

/// Built-in font the system font is inserted after (so Latin text keeps the usual look).
const AFTER: &str = "Ubuntu-Light";

/// Font files to try for a language, best first. Empty: the built-in fonts are enough.
pub fn candidates(code: &str) -> &'static [&'static str] {
    match code {
        "ko" => &["malgun.ttf", "malgunbd.ttf", "gulim.ttc", "batang.ttc"],
        "ja" => &["YuGothM.ttc", "YuGothR.ttc", "meiryo.ttc", "msgothic.ttc", "msmincho.ttc"],
        "zh" => &["msyh.ttc", "simsun.ttc", "simhei.ttf", "Deng.ttf"],
        "zh-Hant" => &["msjh.ttc", "mingliu.ttc", "kaiu.ttf"],
        _ => &[],
    }
}

/// Text the chosen face must cover: the language's own name plus common letters. A file that
/// exists but lacks these (wrong face, broken file) is skipped.
fn probe(code: &str) -> &'static str {
    match code {
        "ko" => "한국어가설정",
        "ja" => "日本語あア設定",
        "zh" => "简体中文设置",
        "zh-Hant" => "繁體中文設定",
        _ => "",
    }
}

/// English name for log lines.
fn language(code: &str) -> &'static str {
    match code {
        "ko" => "Korean",
        "ja" => "Japanese",
        "zh" => "Chinese (Simplified)",
        "zh-Hant" => "Chinese (Traditional)",
        _ => "this language",
    }
}

/// Folders that hold system fonts: the Windows folder from the API, then from `SystemRoot` and
/// `WINDIR`, then the per-user font folder. No drive letter is assumed.
pub fn font_dirs() -> Vec<PathBuf> {
    let mut windows: Vec<PathBuf> = Vec::new();
    if let Some(dir) = windows_dir_api() {
        windows.push(dir);
    }
    for var in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            windows.push(PathBuf::from(value));
        }
    }
    let mut dirs: Vec<PathBuf> = windows.into_iter().map(|w| w.join("Fonts")).collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let key = dir.to_string_lossy().to_lowercase();
        if !out.iter().any(|d| d.to_string_lossy().to_lowercase() == key) {
            out.push(dir);
        }
    }
    out
}

#[cfg(windows)]
fn windows_dir_api() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
    let mut buf = [0u16; 512];
    // SAFETY: the buffer is valid for its stated length; the return value is the length written.
    let len = unsafe { GetWindowsDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if len == 0 || len >= buf.len() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len])))
}

#[cfg(not(windows))]
fn windows_dir_api() -> Option<PathBuf> {
    None
}

/// Number of faces in a font file (`.ttc` collections hold several, `.ttf`/`.otf` one).
fn face_count(bytes: &[u8]) -> u32 {
    if bytes.len() >= 12 && bytes.starts_with(b"ttcf") {
        u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]).min(64)
    } else {
        1
    }
}

/// First face in the file that parses and covers every character of `probe`. egui panics on a
/// font it cannot parse, so nothing reaches it without passing this check.
pub fn pick_face(bytes: &[u8], probe: &str) -> Option<u32> {
    (0..face_count(bytes)).find(|&index| {
        skrifa::FontRef::from_index(bytes, index).is_ok_and(|font| {
            let charmap = font.charmap();
            probe.chars().all(|c| charmap.map(c).is_some())
        })
    })
}

/// A system font read from disk, ready for egui.
pub struct Found {
    pub index: u32,
    pub bytes: Vec<u8>,
}

/// The first usable candidate for `code` in `dirs`. `None`: the language needs no extra font.
/// `Some(Err)`: it needs one and none was usable (the text is the log line).
pub fn load(code: &str, dirs: &[PathBuf]) -> Option<Result<Found, String>> {
    let files = candidates(code);
    if files.is_empty() {
        return None;
    }
    let probe = probe(code);
    for file in files {
        for dir in dirs {
            let path = dir.join(file);
            if let Some(found) = try_file(&path, probe) {
                return Some(Ok(found));
            }
        }
    }
    let shown: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    Some(Err(format!(
        "manager font: no {} font found (tried {} in {}); {} text shows as empty boxes",
        language(code),
        files.join(", "),
        if shown.is_empty() { "no font folder".to_string() } else { shown.join(", ") },
        language(code),
    )))
}

fn try_file(path: &Path, probe: &str) -> Option<Found> {
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let index = pick_face(&bytes, probe)?;
    Some(Found {
        index,
        bytes,
    })
}

/// (ascent, row height) per font size unit, the way epaint measures a face.
fn face_metrics(bytes: &[u8], index: u32) -> Option<(f32, f32)> {
    let font = skrifa::FontRef::from_index(bytes, index).ok()?;
    let m = font.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
    let em = m.units_per_em as f32;
    if em <= 0.0 {
        return None;
    }
    Some((m.ascent / em, (m.ascent - m.descent + m.leading) / em))
}

/// `FontTweak::y_offset_factor` that puts the fallback face on the base font's baseline.
/// epaint places a fallback glyph at `face_ascent + (base_height - face_height) / 2`, which for
/// faces with tall line metrics (Yu Gothic, Meiryo) lifts the text above the Latin baseline.
fn baseline_shift(base: &[u8], base_index: u32, face: &[u8], face_index: u32) -> Option<f32> {
    let (base_ascent, base_height) = face_metrics(base, base_index)?;
    let (face_ascent, face_height) = face_metrics(face, face_index)?;
    let shift = base_ascent - (face_ascent + 0.5 * (base_height - face_height));
    shift.is_finite().then(|| shift.clamp(-0.5, 0.5))
}

/// egui's default fonts plus, when given, the system font as a fallback in both families.
pub fn definitions(extra: Option<Found>) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let Some(found) = extra else {
        return defs;
    };
    let shift = defs
        .font_data
        .get(AFTER)
        .and_then(|base| baseline_shift(base.font.as_ref(), base.index, &found.bytes, found.index))
        .unwrap_or(0.0);
    let mut data = FontData::from_owned(found.bytes);
    data.index = found.index;
    data.tweak.y_offset_factor = shift;
    defs.font_data.insert(NAME.to_string(), Arc::new(data));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(list) = defs.families.get_mut(&family) {
            let at = list.iter().position(|n| n == AFTER).map_or(list.len(), |i| i + 1);
            list.insert(at, NAME.to_string());
        }
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::text::{FontsImpl, TextOptions};

    /// Characters that are not drawn as glyphs (egui shows `\n` as the replacement glyph).
    fn drawn(c: char) -> bool {
        !c.is_control() && !c.is_whitespace()
    }

    fn missing(fonts: &mut FontsImpl, family: &FontFamily, text: &str) -> Vec<char> {
        let mut font = fonts.font(family);
        let mut out: Vec<char> = text.chars().filter(|&c| drawn(c) && !font.has_glyph(c)).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn font_dirs_come_from_the_system_not_a_fixed_drive() {
        let dirs = font_dirs();
        if cfg!(windows) {
            assert!(!dirs.is_empty());
            assert!(dirs.iter().all(|d| d.ends_with("Fonts")), "{dirs:?}");
        }
    }

    #[test]
    fn latin_languages_need_no_file() {
        for code in ["en", "de", "fr", "it", "es", "es-419", "ru", "pl", "nl", "pt", "pt-BR"] {
            assert!(load(code, &font_dirs()).is_none(), "{code}");
        }
    }

    #[test]
    fn missing_font_is_an_error_not_a_panic() {
        let dirs = vec![std::env::temp_dir().join("nms-rs-no-fonts-here")];
        let err = load("ko", &dirs).unwrap().err().unwrap();
        assert!(err.contains("Korean") && err.contains("malgun.ttf"), "{err}");
    }

    #[test]
    fn junk_file_is_skipped() {
        assert_eq!(pick_face(b"not a font at all", "a"), None);
        assert_eq!(pick_face(b"ttcf\0\x01\0\0\0\0\0\x02junk", "a"), None);
    }

    #[test]
    fn fallback_goes_after_ubuntu_in_both_families() {
        let builtin = FontDefinitions::default();
        let (_, data) = builtin.font_data.iter().find(|(n, _)| n.as_str() == AFTER).unwrap();
        let found = Found {
            index: 0,
            bytes: data.font.to_vec(),
        };
        let defs = definitions(Some(found));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = &defs.families[&family];
            let at = list.iter().position(|n| n == NAME).unwrap();
            assert_eq!(list[at - 1], AFTER, "{family:?} {list:?}");
        }
        assert_eq!(definitions(None), FontDefinitions::default());
    }

    /// Every string of every locale (and every picker name that is shown natively) has a glyph
    /// with the fonts that language gets. CJK cases are skipped when this machine lacks the
    /// system font (the manager then logs and shows boxes, as before).
    #[test]
    fn every_locale_string_has_glyphs() {
        let dirs = font_dirs();
        let mut problems = Vec::new();
        for (code, text) in crate::i18n::locale_texts() {
            let extra = match load(code, &dirs) {
                None => None,
                Some(Ok(found)) => Some(found),
                Some(Err(err)) => {
                    eprintln!("skip {code}: {err}");
                    continue;
                }
            };
            let mut fonts = FontsImpl::new(TextOptions::default(), definitions(extra));
            let all: String = crate::i18n::locale_values(text).join("");
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                let gone = missing(&mut fonts, &family, &all);
                if !gone.is_empty() {
                    problems.push(format!("{code} {family:?}: {gone:?}"));
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// Native picker names of the languages the built-in fonts cover render without any file.
    #[test]
    fn builtin_fonts_cover_latin_and_cyrillic_picker_names() {
        let mut fonts = FontsImpl::new(TextOptions::default(), FontDefinitions::default());
        for (code, _english, native) in crate::i18n::codes() {
            if candidates(code).is_empty() {
                let gone = missing(&mut fonts, &FontFamily::Proportional, native);
                assert!(gone.is_empty(), "{code} {native}: {gone:?}");
            }
        }
    }
}
