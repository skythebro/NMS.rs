//! Pure list helpers: kinds, chest filters, Quick Transfer row order and window.

use nms_core::mem::{read_i32, read_u32, read_u64, MIN_ADDR};

pub const LIST_COUNT: usize = 0x4;
pub const LIST_DATA: usize = 0x8;
pub const CHESTS: std::ops::Range<i32> = 0x0C..0x16;
pub const PANTRY_KIND: i32 = 0x1A;
pub const FISH_KIND: i32 = 0x1D;
pub const PRODUCT: i32 = 2;
pub const FISH_CATEGORY: i32 = 9;
pub const ELEMENT_TYPE: usize = 0x24;
pub const PRODUCT_CATEGORY: usize = 0x1E8;
pub const POPUP_VECTOR_MAX: u32 = 16;
pub const POPUP_MODES: [i32; 2] = [0x47, 0x67];
pub const FEED_MODE: i32 = 0x41;
pub const VIEW_OFFSET: u64 = 0xC240;
pub const STORE_NAME_AT: usize = 0x104;
// Build 183791: page fields moved (0x14CE0 -> 0x14D20, 0x14CD8 -> 0x14D18, 0x19650 -> 0x196B0).
pub const QUICK_STATE_AT: usize = 0x14D20;
pub const QUICK_FOCUS_AT: usize = 0x14D18;
pub const QUICK_VISIBLE: i32 = 10;
pub const QUICK_SOURCE_AT: usize = 0x196B0;
pub const QUICK_ROW_SIZE: usize = 0x70;
pub const QUICK_MARK_AT: usize = 0x40;
pub const QUICK_MAGIC: u32 = 0x4346_5300;
pub const QUICK_SLOTS: i32 = 64;
pub const QUICK_ALL_NAME: &str = "All Containers";
pub const QUICK_COLD_NAME: &str = "Cold Storage";
pub const REACH_SECONDS: f64 = 0.5;
pub const CHARGE_LINGER_SECONDS: f64 = 0.5;
pub const BUILD_LINGER_SECONDS: f64 = 1.0;
pub const WATCH_SECONDS: f64 = 0.25;
pub const HOOKS_WAIT_SECONDS: f64 = 120.0;
pub const POPUP_MODE_AT: usize = 0x14488;
pub const POPUP_SOURCE_TAB: usize = 0xB18;

pub fn context_ok(context: i32) -> bool {
    matches!(context, -1 | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 0x18)
}

pub fn kinds(choices: usize) -> Vec<i32> {
    if choices < MIN_ADDR {
        return Vec::new();
    }
    let Some(count) = read_i32(choices.wrapping_add(LIST_COUNT)) else {
        return Vec::new();
    };
    if !(0..=256).contains(&count) {
        return Vec::new();
    }
    let Some(data) = read_u64(choices.wrapping_add(LIST_DATA)) else {
        return Vec::new();
    };
    if data < MIN_ADDR as u64 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for n in 0..count {
        let off = (data as usize).wrapping_add((n as usize).wrapping_mul(4));
        if let Some(kind) = read_i32(off) {
            out.push(kind);
        }
    }
    out
}

pub fn chest_kinds(choices: usize) -> Vec<i32> {
    filter_chests(&kinds(choices))
}

pub fn filter_chests(all: &[i32]) -> Vec<i32> {
    all.iter().copied().filter(|k| CHESTS.contains(k)).collect()
}

pub fn dedup_keep(all: &[i32]) -> Vec<i32> {
    let mut out = Vec::new();
    for &k in all {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

pub fn chests_usable_text(chests: &[i32]) -> String {
    if chests.is_empty() {
        return "none".into();
    }
    let nums: Vec<i32> = chests.iter().map(|k| k - CHESTS.start).collect();
    format!("{nums:?}")
}

pub fn picker_reach_text(chests: &[i32]) -> String {
    if chests.is_empty() {
        return "none".into();
    }
    let parts: Vec<String> = chests
        .iter()
        .map(|&k| {
            if k == PANTRY_KIND {
                "'pantry'".into()
            } else {
                format!("{}", k - CHESTS.start)
            }
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

/// Slide the Quick Transfer box window the same way Python does.
/// Returns (offset, focus, window).
pub fn quick_window(
    kinds: &[Option<i32>],
    first_index: i32,
    page_ok: bool,
    offset: i32,
    prev_focus: Option<i32>,
    focus: i32,
    stale: bool,
) -> (i32, i32, Vec<Option<i32>>) {
    let size = QUICK_VISIBLE.saturating_sub(first_index);
    if size < 1 || kinds.len() <= size as usize || !page_ok {
        return (0, focus, kinds.to_vec());
    }
    let mut offset = if stale { 0 } else { offset };
    let top = first_index;
    let last = first_index.saturating_add(size).saturating_sub(1);
    let most = (kinds.len() as i32).saturating_sub(size);
    offset = offset.clamp(0, most);
    let mut focus = focus;
    if let Some(before) = prev_focus {
        if focus != before {
            if focus == last && before == last.saturating_sub(1) && offset < most {
                offset = offset.saturating_add(1);
                focus = last.saturating_sub(1);
            } else if focus == top && before == top.saturating_add(1) && offset > 0 {
                offset = offset.saturating_sub(1);
                focus = top.saturating_add(1);
            }
        }
    }
    let start = offset.max(0) as usize;
    let end = start.saturating_add(size.max(0) as usize).min(kinds.len());
    let window = kinds.get(start..end).unwrap_or(&[]).to_vec();
    (offset, focus, window)
}

/// One Quick Transfer dest row used only for reordering tests and the live sorter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestRow {
    pub typ: i32,
    pub kind: i32,
    pub raw: Vec<u8>,
}

pub fn parse_dest_kind(mark: u32, kinds: &[(i32, i32)]) -> i32 {
    let slot = mark.wrapping_sub(QUICK_MAGIC);
    if slot < QUICK_SLOTS as u32 {
        kinds
            .iter()
            .find(|(s, _)| *s == slot as i32)
            .map(|(_, k)| *k)
            .unwrap_or(0)
    } else {
        0
    }
}

/// Ingredient Storage (type 9) then Cold Storage, after personal dests, above All Containers.
pub fn place_cooker_dests(rows: &[DestRow]) -> Vec<DestRow> {
    let mut prefix = Vec::new();
    let mut pantry = Vec::new();
    let mut cold = Vec::new();
    let mut rest = Vec::new();
    let mut seen = false;
    for row in rows {
        if row.typ == 9 {
            pantry.push(row.clone());
            seen = true;
        } else if row.typ == 7 && row.kind == FISH_KIND {
            cold.push(row.clone());
            seen = true;
        } else if !seen {
            prefix.push(row.clone());
        } else {
            rest.push(row.clone());
        }
    }
    let mut ordered = prefix;
    ordered.extend(pantry);
    ordered.extend(cold);
    ordered.extend(rest);
    ordered
}

pub fn vector_count(vector: usize) -> Option<u32> {
    if vector < MIN_ADDR {
        return None;
    }
    read_u32(vector.wrapping_add(4))
}

pub fn shown_pairs_text(shown: &[(ShownKind, &str)]) -> String {
    let parts: Vec<String> = shown
        .iter()
        .map(|(k, name)| match k {
            ShownKind::All => format!("('all', '{name}')"),
            ShownKind::Cold => format!("('cold', '{name}')"),
            ShownKind::Box(n) => format!("({n}, '{name}')"),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShownKind {
    All,
    Cold,
    Box(i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_storage_kinds_only() {
        let all = [0, 4, 0x0C, 0x15, 0x1A, 0x1D, 0x16];
        assert_eq!(filter_chests(&all), vec![0x0C, 0x15]);
    }

    #[test]
    fn dedup_keeps_first() {
        assert_eq!(dedup_keep(&[12, 13, 12, 14]), vec![12, 13, 14]);
    }

    #[test]
    fn usable_text_none_or_indexes() {
        assert_eq!(chests_usable_text(&[]), "none");
        assert_eq!(chests_usable_text(&[0x0C, 0x0E]), "[0, 2]");
    }

    #[test]
    fn picker_lists_pantry_as_word() {
        assert_eq!(picker_reach_text(&[0x0C, PANTRY_KIND]), "[0, 'pantry']");
    }

    #[test]
    fn context_set_matches_python() {
        assert!(context_ok(-1));
        assert!(context_ok(0x18));
        assert!(!context_ok(0x1A));
    }

    #[test]
    fn cooker_dests_go_after_personal() {
        let rows = [
            DestRow { typ: 0, kind: 0, raw: vec![0] },
            DestRow { typ: 4, kind: 0, raw: vec![1] },
            DestRow { typ: 7, kind: 0, raw: vec![2] },
            DestRow { typ: 9, kind: PANTRY_KIND, raw: vec![3] },
            DestRow { typ: 7, kind: FISH_KIND, raw: vec![4] },
            DestRow { typ: 7, kind: 0, raw: vec![5] },
        ];
        let ordered = place_cooker_dests(&rows);
        assert_eq!(
            ordered.iter().map(|r| r.raw[0]).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        let swapped = [
            DestRow { typ: 0, kind: 0, raw: vec![0] },
            DestRow { typ: 7, kind: FISH_KIND, raw: vec![4] },
            DestRow { typ: 9, kind: PANTRY_KIND, raw: vec![3] },
            DestRow { typ: 7, kind: 0, raw: vec![5] },
        ];
        let ordered = place_cooker_dests(&swapped);
        assert_eq!(
            ordered.iter().map(|r| r.raw[0]).collect::<Vec<_>>(),
            vec![0, 3, 4, 5]
        );
    }

    #[test]
    fn window_slides_at_ends() {
        let kinds: Vec<Option<i32>> = (0..8).map(Some).collect();
        let (off, focus, win) = quick_window(&kinds, 2, true, 0, Some(2), 3, false);
        assert_eq!(off, 0);
        assert_eq!(focus, 3);
        assert_eq!(win.len(), 8);

        let kinds: Vec<Option<i32>> = (0..20).map(Some).collect();
        let size = QUICK_VISIBLE - 2;
        let last = 2 + size - 1;
        let (off, focus, win) = quick_window(&kinds, 2, true, 0, Some(last - 1), last, false);
        assert_eq!(off, 1);
        assert_eq!(focus, last - 1);
        assert_eq!(win.len(), size as usize);
        assert_eq!(win[0], Some(1));

        let (off, focus, _) = quick_window(&kinds, 2, true, 3, Some(3), 2, false);
        assert_eq!(off, 2);
        assert_eq!(focus, 3);
    }

    #[test]
    fn shown_text_matches_python_pairs() {
        let shown = [
            (ShownKind::Cold, QUICK_COLD_NAME),
            (ShownKind::Box(0), "Storage container 1"),
            (ShownKind::All, QUICK_ALL_NAME),
        ];
        assert_eq!(
            shown_pairs_text(&shown),
            "[('cold', 'Cold Storage'), (0, 'Storage container 1'), ('all', 'All Containers')]"
        );
    }

    #[test]
    fn parse_mark_slot() {
        assert_eq!(parse_dest_kind(QUICK_MAGIC + 3, &[(3, FISH_KIND)]), FISH_KIND);
        assert_eq!(parse_dest_kind(0, &[(0, 12)]), 0);
    }
}
