//! Player-facing text of Quick Transfer All (button tooltips and toasts) in the game's language.
//!
//! The strings are `ui.qta_<id>` in `common/menu_strings/*.json`. The language is read once when
//! the mod starts (`lang::detect`, the same file read as the Mods page), before the caves are built,
//! so a language change in the game options shows here after the next game start.
//!
//! Sizes:
//! - Tooltips go into the game's own 32-byte key on the side bar pass's stack (`[rbp-0x18]`, two
//!   16-byte moves in the deposit cave), so they hold at most 31 bytes. That slot cannot grow; every
//!   translation is kept short enough (CJK and Hangul are 3 bytes a character, so about 10).
//!   The game looks the key up as a localisation id and shows the raw text when it is none.
//! - Toasts go to `AddTimedMessage`, which copies up to 0x200 bytes of C string (see
//!   `refiner_guard`), so our slots are `TOAST_LEN` bytes.
//!
//! Every cut is on a UTF-8 character boundary and leaves a NUL (`menu_text::fixed`).

use crate::common::menu_text;

/// The game's tooltip key: 31 text bytes and a NUL.
pub const TIP_LEN: usize = 32;
/// Our toast slots (cave and `show_now` buffer).
pub const TOAST_LEN: usize = 128;

/// English text of every id, used when a language lacks one.
const ENGLISH: &[(&str, &str)] = &[
    ("tip_same", "Quick Deposit: matching items"),
    ("tip_all", "Quick Deposit: all highlighted"),
    ("lock_tip", "Lock mode: pin suit slots"),
    ("lock_on", "Exosuit lock is on"),
    ("lock_off", "Exosuit lock is off"),
    ("deposited_one", "Deposited {n} item"),
    ("deposited", "Deposited {n} items"),
    ("no_storage", "No storage in reach"),
    ("deposit_failed", "Could not deposit"),
    ("nothing", "Nothing to deposit"),
    ("restocked_one", "Restocked {n} item"),
    ("restocked", "Restocked {n} items"),
    ("sale_blocked", "Sale blocked"),
    ("sold", "Sold {n} surplus"),
];

/// Ids that end up in a tooltip key (`TIP_LEN`).
pub const TIP_IDS: [&str; 3] = ["tip_same", "tip_all", "lock_tip"];

/// `ui.qta_<id>` in `lang`, English when missing.
pub fn get(lang: &str, id: &str) -> &'static str {
    let text = menu_text::ui(lang, &format!("qta_{id}"));
    if !text.is_empty() {
        return text;
    }
    ENGLISH.iter().find(|(k, _)| *k == id).map(|(_, v)| *v).unwrap_or("")
}

/// A count message: `<id>_one` for 1 (when there is one), `<id>` otherwise, `{n}` filled in.
pub fn count(lang: &str, id: &str, n: i32) -> String {
    let one = if n == 1 { get(lang, &format!("{id}_one")) } else { "" };
    let form = if one.is_empty() { get(lang, id) } else { one };
    form.replace("{n}", &n.to_string())
}

/// `get` in a tooltip key buffer.
pub fn tip(lang: &str, id: &str) -> [u8; TIP_LEN] {
    menu_text::fixed(get(lang, id))
}

/// Any toast text in a toast buffer.
pub fn toast(text: &str) -> [u8; TOAST_LEN] {
    menu_text::fixed(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [&str; 15] =
        ["en", "de", "fr", "it", "es", "es-419", "ru", "pl", "nl", "pt", "pt-BR", "ja", "zh", "zh-Hant", "ko"];

    #[test]
    fn english_table_matches_en_json() {
        for (id, en) in ENGLISH {
            assert_eq!(menu_text::ui("en", &format!("qta_{id}")), *en, "{id}");
        }
    }

    #[test]
    fn tooltips_fit_the_game_key_in_every_language() {
        for lang in LANGS {
            for id in TIP_IDS {
                let text = menu_text::ui(lang, &format!("qta_{id}"));
                assert!(!text.is_empty(), "{lang} {id} missing");
                assert!(text.len() < TIP_LEN, "{lang} {id} is {} bytes: {text}", text.len());
                let buf = tip(lang, id);
                assert_eq!(&buf[..text.len()], text.as_bytes(), "{lang} {id} cut");
                assert_eq!(buf[text.len()], 0);
            }
        }
    }

    #[test]
    fn toasts_fit_with_a_big_count() {
        for lang in LANGS {
            for (id, _) in ENGLISH.iter().filter(|(id, _)| !TIP_IDS.contains(id)) {
                let text = get(lang, id).replace("{n}", "99999");
                assert!(text.len() < TOAST_LEN, "{lang} {id}");
                assert!(!menu_text::ui(lang, &format!("qta_{id}")).is_empty(), "{lang} {id} missing");
            }
        }
    }

    #[test]
    fn counts_pick_the_form() {
        assert_eq!(count("en", "deposited", 1), "Deposited 1 item");
        assert_eq!(count("en", "deposited", 3), "Deposited 3 items");
        assert_eq!(count("en", "restocked", 0), "Restocked 0 items");
        assert_eq!(count("ko", "deposited", 2), "아이템 2개 보관함");
        assert_eq!(count("en", "sold", 1), "Sold 1 surplus");
        assert_eq!(get("xx", "lock_on"), "Exosuit lock is on");
    }

    #[test]
    fn cuts_keep_whole_characters() {
        // 11 Hangul syllables are 33 bytes: the key keeps 10 (30 bytes) and a NUL.
        let long = "가나다라마바사아자차카";
        let buf: [u8; TIP_LEN] = menu_text::fixed(long);
        assert_eq!(&buf[..30], &long.as_bytes()[..30]);
        assert!(buf[30..].iter().all(|&b| b == 0));
        assert!(std::str::from_utf8(&buf[..30]).is_ok());
    }
}
