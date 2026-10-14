//! Short help text for the setting rows of the in-game Mods page (the description panel on the
//! right). One sentence each, taken from the mod READMEs. The text lives in
//! `menu_strings/en.json` (`desc.<Mod>.<key>`, with `desc.*.<key>` for keys every mod has), the
//! same file `menu_text` reads, so there is one source for every language.

/// The English help text for one setting, if there is any. See `menu_text::desc` for other languages.
#[cfg(test)]
pub fn describe(mod_name: &str, key: &str) -> Option<&'static str> {
    super::menu_text::desc("en", mod_name, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_specific_and_common() {
        assert!(describe("HideEmptyHover", "hide_tech_hover").is_some());
        assert!(describe("SphereSizedMining", "write_logs").is_some());
        assert!(describe("HideEmptyHover", "nope").is_none());
    }
}
