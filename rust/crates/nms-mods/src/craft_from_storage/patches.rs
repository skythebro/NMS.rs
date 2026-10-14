//! Live byte patches and the Quick Transfer name cave.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

use nms_core::mem::{self, alloc_exec_near, readable};
use nms_core::patch::CodePatch;
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::craft_from_storage as sigs;

use super::lists::{QUICK_ALL_NAME, QUICK_COLD_NAME};

/// Game language for the row names we add, read once at start (a change shows after a restart).
static LANG: OnceLock<&'static str> = OnceLock::new();

pub fn set_lang(lang: &'static str) {
    let _ = LANG.set(lang);
}

fn lang() -> &'static str {
    LANG.get().copied().unwrap_or("en")
}

/// `ui.<id>` in the game language, `english` when missing.
fn ui_text(id: &str, english: &str) -> String {
    let text = crate::common::menu_text::ui(lang(), id);
    if text.is_empty() { english.to_string() } else { text.to_string() }
}

/// Name of an unnamed container row (`n` from 1) in the game language.
pub fn numbered_name(n: i32) -> String {
    crate::common::menu_text::fill(&ui_text("cfs_storage_n", "Storage container {n}"), n.max(0) as usize, "")
}

/// The 32-byte row key the game shows: our two fixed rows in the game language, any other name
/// as given; upper case (Unicode, so accents stay right), cut on a character boundary, NUL-ended.
pub fn row_key(text: &str) -> [u8; 32] {
    let shown = if text == QUICK_ALL_NAME {
        ui_text("cfs_all_containers", QUICK_ALL_NAME)
    } else if text == QUICK_COLD_NAME {
        ui_text("cfs_cold_storage", QUICK_COLD_NAME)
    } else {
        text.to_string()
    };
    crate::common::menu_text::fixed(&shown.to_uppercase())
}

pub const QUICK_CAVE: &[u8] = &[
    0x83, 0xf9, 0x07, 0x75, 0x40, 0x41, 0x52, 0x41, 0x53, 0x4d, 0x6b, 0xd5, 0x70, 0x49, 0x01, 0xfa,
    0x45, 0x8b, 0x5a, 0x40, 0x41, 0x81, 0xeb, 0x00, 0x53, 0x46, 0x43, 0x41, 0x83, 0xfb, 0x40, 0x73,
    0x2f, 0x49, 0xc1, 0xe3, 0x05, 0x4c, 0x8d, 0x15, 0xd4, 0x01, 0x00, 0x00, 0x43, 0x0f, 0x10, 0x04,
    0x1a, 0x43, 0x0f, 0x10, 0x4c, 0x1a, 0x10, 0x41, 0x5b, 0x41, 0x5a, 0xff, 0x25, 0xb7, 0x01, 0x00,
    0x00, 0x41, 0x5b, 0x41, 0x5a, 0x0f, 0x10, 0x44, 0x05, 0x40, 0xff, 0x25, 0xa0, 0x01, 0x00, 0x00,
    0x83, 0x3d, 0x89, 0x01, 0x00, 0x00, 0x00, 0x74, 0xe8, 0x4c, 0x8d, 0x15, 0x60, 0x01, 0x00, 0x00,
    0x41, 0x0f, 0x10, 0x02, 0x41, 0x0f, 0x10, 0x4a, 0x10, 0x41, 0x5b, 0x41, 0x5a, 0xff, 0x25, 0x85,
    0x01, 0x00, 0x00,
];
pub const QUICK_CAVE_NORMAL: usize = 0x1F0;
pub const QUICK_CAVE_OURS: usize = 0x1F8;
pub const QUICK_CAVE_TABLE: usize = 0x200;
pub const QUICK_CAVE_SIZE: usize = 0x1000;
pub const QUICK_CAVE_ALL_KEY: usize = 0x1C0;
pub const QUICK_CAVE_ALL_FLAG: usize = 0x1E0;

const QUICK_TEXT_AT: usize = 0x1F;
const QUICK_TEXT_OLD: &[u8] = &[0x0F, 0x10, 0x44, 0x05, 0x40];
const QUICK_FREIGHTER_AT: usize = 9;
const QUICK_FREIGHTER_OLD: &[u8] = &[
    0x8B, 0x45, 0x94, 0x48, 0x8B, 0x5D, 0x98, 0x3B, 0x45, 0x90, 0x73, 0x38, 0x0F, 0xB6, 0x8D, 0x38, 0x04, 0x00, 0x00,
    0x0F, 0x57, 0xC0, 0x48, 0x6B, 0xD0, 0x70, 0x0F, 0xB7, 0x45, 0xA4, 0x88, 0x4C,
];
const QUICK_FREIGHTER_HEAD: &[u8] = &[
    0x44, 0x88, 0xAD, 0x30, 0x04, 0x00, 0x00, 0x4C, 0x8D, 0x85, 0x30, 0x04, 0x00, 0x00, 0x48, 0x8D, 0x55, 0xA4, 0x48,
    0x8D, 0x4D, 0x90,
];
const QUICK_FREIGHTER_JOIN: usize = 0xFD;

pub static CAVE_ADDR: AtomicUsize = AtomicUsize::new(0);
pub static SHOW_ALL: AtomicBool = AtomicBool::new(true);

pub const REFINER: CodePatch = CodePatch {
    label: "Refiner Storage tab",
    sig: sigs::REFINER_TAB.sig,
    at: sigs::REFINER_TAB.at,
    old: sigs::REFINER_TAB.old,
    new: sigs::REFINER_TAB.new,
};

pub const COOKER_TAB: CodePatch = CodePatch {
    label: "Cooker Processed tab",
    sig: sigs::COOKER_TAB_PATCH.sig,
    at: sigs::COOKER_TAB_PATCH.at,
    old: sigs::COOKER_TAB_PATCH.old,
    new: sigs::COOKER_TAB_PATCH.new,
};

pub const COOKER_FILTER: CodePatch = CodePatch {
    label: "Cooker filter",
    sig: sigs::COOKER_FILTER_PATCH.sig,
    at: sigs::COOKER_FILTER_PATCH.at,
    old: sigs::COOKER_FILTER_PATCH.old,
    new: sigs::COOKER_FILTER_PATCH.new,
};

pub const COOKER_STRIP: CodePatch = CodePatch {
    label: "Cooker inventory tabs",
    sig: sigs::COOKER_STRIP_PATCH.sig,
    at: sigs::COOKER_STRIP_PATCH.at,
    old: sigs::COOKER_STRIP_PATCH.old,
    new: sigs::COOKER_STRIP_PATCH.new,
};

pub const ORDER: CodePatch = CodePatch {
    label: "All tab order",
    sig: sigs::ORDER_PATCH.sig,
    at: sigs::ORDER_PATCH.at,
    old: sigs::ORDER_PATCH.old,
    new: sigs::ORDER_PATCH.new,
};

fn find_sig(image: &ProcessImage, sig: &str) -> Option<usize> {
    Pattern::parse(sig).ok().and_then(|p| image.find_unique(&p).unique())
}

fn write_bytes(addr: usize, bytes: &[u8]) -> bool {
    if !readable(addr, bytes.len()) {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
    }
    true
}

pub fn cave_or_alloc(image: &ProcessImage) -> Option<usize> {
    let have = CAVE_ADDR.load(Ordering::Acquire);
    if have != 0 {
        return Some(have);
    }
    let cave = alloc_exec_near(image.base(), QUICK_CAVE_SIZE)?;
    CAVE_ADDR.store(cave, Ordering::Release);
    Some(cave)
}

pub fn quick_set_name(slot: i32, text: &str) {
    let cave = CAVE_ADDR.load(Ordering::Acquire);
    if cave == 0 || !(0..64).contains(&slot) {
        return;
    }
    let raw = row_key(text);
    let addr = cave.wrapping_add(QUICK_CAVE_TABLE).wrapping_add(32usize.wrapping_mul(slot as usize));
    let _ = write_bytes(addr, &raw);
}

pub fn quick_set_all_containers(show: bool) {
    SHOW_ALL.store(show, Ordering::Release);
    let cave = CAVE_ADDR.load(Ordering::Acquire);
    if cave == 0 {
        return;
    }
    let raw = row_key(QUICK_ALL_NAME);
    let _ = write_bytes(cave.wrapping_add(QUICK_CAVE_ALL_KEY), &raw);
    let flag: u32 = if show { 1 } else { 0 };
    let _ = unsafe { mem::write(cave.wrapping_add(QUICK_CAVE_ALL_FLAG), flag) };
}

pub fn patch_refiner_tab(image: &mut ProcessImage, apply: bool) -> String {
    REFINER.set(image, apply).message
}

pub fn patch_cooker_substances(image: &mut ProcessImage, apply: bool) -> String {
    COOKER_TAB.set(image, apply).message
}

pub fn patch_cooker_tabs(image: &mut ProcessImage, apply: bool) -> String {
    let first = COOKER_FILTER.set(image, apply).message;
    let second = COOKER_STRIP.set(image, apply).message;
    format!("{first}; {second}")
}

pub fn patch_list_order(image: &mut ProcessImage, apply: bool) -> String {
    ORDER.set(image, apply).message
}

fn find_text_rva(image: &ProcessImage) -> Option<usize> {
    if let Some(rva) = find_sig(image, sigs::QUICK_TEXT) {
        return Some(rva);
    }
    let tokens: Vec<&str> = sigs::QUICK_TEXT.split_whitespace().collect();
    if QUICK_TEXT_AT + 5 > tokens.len() {
        return None;
    }
    let mut patched = tokens;
    patched[QUICK_TEXT_AT..QUICK_TEXT_AT + 5].copy_from_slice(&["E9", "?", "?", "?", "?"]);
    find_sig(image, &patched.join(" "))
}

pub fn patch_quick_text(image: &ProcessImage, live: &mut ProcessImage, apply: bool) -> String {
    let label = "Quick Transfer container names";
    let Some(found) = find_text_rva(image) else {
        return format!("{label} not changed: game code not found (game update?)");
    };
    let rva = found.wrapping_add(QUICK_TEXT_AT);
    let address = live.va(rva);
    let Some(current) = live.read(rva, 5) else {
        return format!("{label} not changed: unexpected game code (game update?)");
    };
    let new = if !apply {
        if current == QUICK_TEXT_OLD {
            return format!("{label} already turned off");
        }
        QUICK_TEXT_OLD.to_vec()
    } else {
        if current != QUICK_TEXT_OLD {
            return if current.first() == Some(&0xE9) {
                format!("{label} already applied")
            } else {
                format!("{label} not changed: unexpected game code (game update?)")
            };
        }
        let Some(cave) = cave_or_alloc(image) else {
            return format!("{label} not applied: no executable memory near the game code");
        };
        let mut buf = vec![0u8; QUICK_CAVE_SIZE];
        if let Some(dst) = buf.get_mut(..QUICK_CAVE.len()) {
            dst.copy_from_slice(QUICK_CAVE);
        }
        let normal = (address.wrapping_add(5) as u64).to_le_bytes();
        let ours = (address.wrapping_add(10) as u64).to_le_bytes();
        if let Some(dst) = buf.get_mut(QUICK_CAVE_NORMAL..QUICK_CAVE_NORMAL + 8) {
            dst.copy_from_slice(&normal);
        }
        if let Some(dst) = buf.get_mut(QUICK_CAVE_OURS..QUICK_CAVE_OURS + 8) {
            dst.copy_from_slice(&ours);
        }
        if !write_bytes(cave, &buf) {
            return format!("{label} not changed: could not unprotect the code");
        }
        quick_set_all_containers(SHOW_ALL.load(Ordering::Acquire));
        let Some(rel) = mem::rel32(address.wrapping_add(5), cave) else {
            return format!("{label} not changed: unexpected game code (game update?)");
        };
        let mut jmp = vec![0xE9];
        jmp.extend_from_slice(&rel.to_le_bytes());
        jmp
    };
    if live.write(rva, &new).is_err() {
        return format!("{label} not changed: could not unprotect the code");
    }
    format!("{label} {}", if apply { "applied" } else { "turned off" })
}

fn find_freighter_rva(image: &ProcessImage) -> Option<usize> {
    if let Some(rva) = find_sig(image, sigs::QUICK_FREIGHTER) {
        return Some(rva);
    }
    let mut wild = sigs::QUICK_FREIGHTER.to_string();
    for byte in QUICK_FREIGHTER_HEAD {
        wild.push_str(&format!(" {byte:02X}"));
    }
    wild.push_str(" E8 ? ? ? ? E9 ? ? ? ?");
    find_sig(image, &wild)
}

pub fn patch_quick_freighter_row(image: &ProcessImage, live: &mut ProcessImage, apply: bool) -> String {
    let label = "Quick Transfer freighter storage row";
    let Some(row) = find_sig(image, sigs::QUICK_ROW) else {
        return format!("{label} not changed: game code not found (game update?)");
    };
    let Some(site) = find_freighter_rva(image) else {
        return format!("{label} not changed: game code not found (game update?)");
    };
    let call_from = site
        .wrapping_add(QUICK_FREIGHTER_AT)
        .wrapping_add(QUICK_FREIGHTER_HEAD.len())
        .wrapping_add(5);
    let join_from = site
        .wrapping_add(QUICK_FREIGHTER_AT)
        .wrapping_add(QUICK_FREIGHTER_HEAD.len())
        .wrapping_add(10);
    let join_to = site.wrapping_add(QUICK_FREIGHTER_AT).wrapping_add(QUICK_FREIGHTER_JOIN);
    let Some(call_rel) = i32::try_from(row as i64 - call_from as i64).ok() else {
        return format!("{label} not changed: unexpected game code (game update?)");
    };
    let Some(join_rel) = i32::try_from(join_to as i64 - join_from as i64).ok() else {
        return format!("{label} not changed: unexpected game code (game update?)");
    };
    let mut patched = QUICK_FREIGHTER_HEAD.to_vec();
    patched.push(0xE8);
    patched.extend_from_slice(&call_rel.to_le_bytes());
    patched.push(0xE9);
    patched.extend_from_slice(&join_rel.to_le_bytes());
    if patched.len() != QUICK_FREIGHTER_OLD.len() {
        return format!("{label} not changed: unexpected game code (game update?)");
    }
    let rva = site.wrapping_add(QUICK_FREIGHTER_AT);
    let Some(current) = live.read(rva, QUICK_FREIGHTER_OLD.len()) else {
        return format!("{label} not changed: unexpected game code (game update?)");
    };
    let new = if !apply {
        if current == QUICK_FREIGHTER_OLD {
            return format!("{label} already turned off");
        }
        if current != patched {
            return format!("{label} not changed: unexpected game code (game update?)");
        }
        QUICK_FREIGHTER_OLD.to_vec()
    } else {
        if current == patched {
            return format!("{label} already applied");
        }
        if current != QUICK_FREIGHTER_OLD {
            return format!("{label} not changed: unexpected game code (game update?)");
        }
        patched
    };
    if live.write(rva, &new).is_err() {
        return format!("{label} not changed: could not unprotect the code");
    }
    format!("{label} {}", if apply { "applied" } else { "turned off" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_keys_are_upper_case_utf8_and_fit() {
        assert_eq!(&row_key(QUICK_ALL_NAME)[..15], b"ALL CONTAINERS ");
        assert_eq!(&row_key("Kühllager")[..11], "KÜHLLAGER ".as_bytes());
        // 11 Hangul syllables are 33 bytes: 10 stay, then a NUL.
        let long = row_key("가나다라마바사아자차카");
        assert!(std::str::from_utf8(&long[..30]).is_ok());
        assert!(long[30..].iter().all(|&b| b == 0));
        for lang in ["en", "de", "fr", "it", "es", "es-419", "ru", "pl", "nl", "pt", "pt-BR", "ja", "zh", "zh-Hant", "ko"] {
            for id in ["cfs_all_containers", "cfs_cold_storage", "cfs_storage_n"] {
                let text = crate::common::menu_text::ui(lang, id).replace("{n}", "10").to_uppercase();
                assert!(!text.is_empty() && text.len() < 32, "{lang} {id}");
            }
        }
        assert_eq!(numbered_name(3), "Storage container 3");
    }

    #[test]
    fn cave_bytes_match_python_hex() {
        assert_eq!(QUICK_CAVE.len(), 115);
        assert_eq!(QUICK_CAVE[0], 0x83);
        assert_eq!(QUICK_CAVE[QUICK_CAVE.len() - 1], 0x00);
        assert_eq!(QUICK_FREIGHTER_HEAD.len() + 10, QUICK_FREIGHTER_OLD.len());
    }
}
