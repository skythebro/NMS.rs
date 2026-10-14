//! Search in the item pickers (refiner, Nutrient Processor, Transfer Items, fossil exhibit) and
//! in the inventory craft list (empty slot, Craft: the technology and product build list).
//!
//! The picker asks the predicate
//! `0x1406C9720(popup, element, state, out)` about every card every frame; return 2 hides a card
//! and also drops it from the page count and the scroll dots. The detour calls the original first
//! and only turns a shown card (0 or 1) into a hidden one (2), so the cooker tabs keep working.
//!
//! The name a player sees comes from the game's own lookup, as its tooltip code does it
//! (`0x1406BB3B0..0x1406BB406`): product or substance record by id (`app+0x60`), the name key at
//! record `+0x270` (product) or `+0x154` (substance), then the localisation lookup
//! `(table, key, 0)`. All three are pure reads (hash finds; the loc lookup copies the key to its own
//! stack and returns the key pointer itself when the id is unknown), checked in the disassembly.
//! Each distinct element is resolved once per picker opening and cached; an unknown key is matched
//! as it is (some products carry plain English in that field). Technologies (type 1, only in the
//! craft list and the Install Technology list) use the technology lookup with its "build if missing"
//! flag off and `NameLower` at `+0x244`. The item id is searched too.
//!
//! The craft list is popup kind 2 or 3 (`0x1406CC3B0`, `BUILD_TECH`/`BUILD_PROD`). Its cards also go
//! through the predicate (no filter state), but hiding them there did not change the list in game, so
//! `craft` filters the recipes where the list adds them to its temporary inventories
//! ([`craft_keeps`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use nms_core::mem::{read_bytes, read_i32, read_u32, read_u8, readable, MIN_ADDR};

use super::{log_line, now_ms, text_box, LOG_NAMES, PICKER_SEARCH};

/// Mode of the picker (`popup+0x14488`): 1 Transfer, 0x3A refiner and cooker, 0x47/0x67 customiser.
pub const POPUP_MODE: usize = 0x14488;
/// Which popup the page host `0x1408F2200` runs (`page+0x14C28`): -1 none, 0 the item picker
/// `0x6BC160` (every picker mode), other values other popups (jump table at `0x1408F235C`).
pub const POPUP_KIND: usize = 0x14C28;
/// State of the popup's frame layer (`page+0x14A58`, state at layer `+0x198`): 0 closed, 1 opening,
/// 2 open, 3 closing (`0x1408F5F40`). The host closes kinds 2 and 3 once it reads 0.
pub const POPUP_LAYER_STATE: usize = 0x14BF0;
const PICKER_KIND: i32 = 0;
/// The inventory craft list (technology and product build list), `0x1406CC3B0`.
const CRAFT_KINDS: [i32; 2] = [2, 3];

/// A popup of any kind is up on this page (its kind is set or its frame layer is not closed).
/// While one is, the page's input byte must not be cleared: the host then clears the layer's
/// "stay open" byte (`page+0x14BE8`, `0x1408F22E4`) every frame and the layer animates out for
/// good, which hid the Transfer picker under the search box.
pub fn popup_up(page: usize) -> bool {
    page >= MIN_ADDR
        && (read_i32(page + POPUP_KIND).is_some_and(|k| k != -1)
            || read_i32(page + POPUP_LAYER_STATE).is_some_and(|s| s != 0))
}

/// A searchable list is open and shown now (its layer opening or open): the kind, 0 for the item
/// picker, 2 or 3 for the craft list.
pub fn searchable_kind(page: usize) -> Option<i32> {
    if page < MIN_ADDR || !matches!(read_i32(page + POPUP_LAYER_STATE), Some(1 | 2)) {
        return None;
    }
    read_i32(page + POPUP_KIND).filter(|&k| is_searchable(k))
}

pub fn is_craft(kind: i32) -> bool {
    CRAFT_KINDS.contains(&kind)
}

fn is_searchable(kind: i32) -> bool {
    kind == PICKER_KIND || CRAFT_KINDS.contains(&kind)
}

/// What the log calls a list.
pub fn kind_label(kind: i32, mode: i32) -> String {
    if CRAFT_KINDS.contains(&kind) {
        "craft list".to_string()
    } else {
        format!("picker mode {mode:#x}")
    }
}

/// First card shown (`popup+0x14CD8`). The game clamps it to the card count, not to the last page.
const POPUP_SCROLL: usize = 0x14CD8;
const ELEMENT_TYPE: usize = 0x24;
const SUBSTANCE_NAME: usize = 0x154;
const PRODUCT_NAME: usize = 0x270;
/// `cGcTechnology.NameLower` (`cTkFixedString0x80`; `ID` at `+0x108`, which the lookup compares at
/// `0x140EC8E33`). `+0x50` is `Group`, empty for most technologies.
const TECH_NAME: usize = 0x244;
/// The loc lookup reads at most 31 characters of a key.
const KEY_LEN: usize = 0x20;
const TEXT_MAX: usize = 256;
/// No predicate call for this long: the picker was closed (it asks every frame while open).
const REOPEN_GAP_MS: u64 = 500;
/// Name lines per picker opening and per session, so `log_names` cannot flood the log.
const NAME_LINES_PER_OPEN: u32 = 400;
const NAME_LINES_MAX: u32 = 4000;

pub static ORIG_PRED: AtomicUsize = AtomicUsize::new(0);
/// Last predicate call (ms on [`now_ms`]) and its popup object.
pub static SEEN_MS: AtomicU64 = AtomicU64::new(0);
pub static SEEN_POPUP: AtomicUsize = AtomicUsize::new(0);
/// A search text is set (fast path for the 30 000 predicate calls a second).
static FILTER_ON: AtomicBool = AtomicBool::new(false);

type PredFn = unsafe extern "system" fn(usize, usize, usize, usize) -> u32;
type LookupFn = unsafe extern "system" fn(usize, usize) -> usize;
type TechFn = unsafe extern "system" fn(usize, usize, u32) -> usize;
type LocTableFn = unsafe extern "system" fn() -> usize;
type LocFn = unsafe extern "system" fn(usize, *const u8, usize) -> usize;

/// The game's own name lookups. Optional: without them every card matches on its item id.
pub struct Lookups {
    pub product: usize,
    pub substance: usize,
    pub loc: usize,
    pub loc_table: usize,
    /// Technology lookup, 0 when its signature was not found (technologies then match on the id).
    pub tech: usize,
}
static LOOKUPS: OnceLock<Lookups> = OnceLock::new();

pub fn set_lookups(l: Lookups) {
    let _ = LOOKUPS.set(l);
}

struct Active {
    /// Lower case, what is matched.
    text: String,
    popup: usize,
    kind: i32,
    mode: i32,
    set_ms: u64,
}

struct State {
    search: Option<Active>,
    open_popup: usize,
    /// Element id to its lower-case display name, for this picker opening.
    names: HashMap<[u8; 16], String>,
    /// Element id to "matches the current search".
    matches: HashMap<[u8; 16], bool>,
    open_lines: u32,
    lines: u32,
}

fn shared() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            search: None,
            open_popup: 0,
            names: HashMap::new(),
            matches: HashMap::new(),
            open_lines: 0,
            lines: 0,
        })
    })
}

/// `try_lock` with a short spin: the only other holder is a predicate call, which is brief.
fn lock_briefly() -> Option<std::sync::MutexGuard<'static, State>> {
    for _ in 0..2000 {
        if let Ok(guard) = shared().try_lock() {
            return Some(guard);
        }
        std::hint::spin_loop();
    }
    None
}

/// Set (non-empty) or clear (empty) the search. Called from the `Update` hook when the box closes.
pub fn set(text: &str, popup: usize, kind: i32, mode: i32) {
    let text = text.trim().to_lowercase();
    let Some(mut st) = lock_briefly() else {
        log_line("search not changed: picker busy, try again");
        return;
    };
    honour_pending_clear(&mut st);
    st.matches.clear();
    if text.is_empty() {
        let had = st.search.take().is_some();
        FILTER_ON.store(false, Ordering::Release);
        if had {
            reset_scroll(popup, kind);
        }
        log_line("search cleared");
        return;
    }
    st.search = Some(Active { text: text.clone(), popup, kind, mode, set_ms: now_ms() });
    if CRAFT_KINDS.contains(&kind) {
        CRAFT_KEPT.store(0, Ordering::Relaxed);
        CRAFT_DROPPED.store(0, Ordering::Relaxed);
        CRAFT_REPORTED.store(false, Ordering::Release);
    }
    FILTER_ON.store(true, Ordering::Release);
    reset_scroll(popup, kind);
    log_line(&format!(
        "search set to \"{text}\" ({}); cards whose name does not contain it are hidden",
        kind_label(kind, mode)
    ));
}

/// The state still holds a search that `clear` could not drop (the game thread held the lock).
static CLEAR_PENDING: AtomicBool = AtomicBool::new(false);

/// Drop the search (switch turned off; may run on the settings thread). Never blocks: the filter is
/// off at once (every detour checks `FILTER_ON` first), and if the game thread holds the state the
/// next `expire` or `set` on the game thread drops it.
pub fn clear() {
    FILTER_ON.store(false, Ordering::Release);
    match shared().try_lock() {
        Ok(mut st) => {
            st.search = None;
            st.matches.clear();
        }
        Err(_) => CLEAR_PENDING.store(true, Ordering::Release),
    }
}

fn honour_pending_clear(st: &mut State) {
    if CLEAR_PENDING.swap(false, Ordering::AcqRel) {
        st.search = None;
        st.matches.clear();
    }
}

/// Every frame: drop the search when its picker closed (popup kind or layer state, or no
/// predicate call for a while) or changed mode.
pub fn expire(now: u64) {
    if CLEAR_PENDING.load(Ordering::Acquire) {
        if let Ok(mut st) = shared().try_lock() {
            honour_pending_clear(&mut st);
        }
    }
    if !FILTER_ON.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut st) = shared().try_lock() else { return };
    let Some((popup, kind, mode, set_ms)) = st.search.as_ref().map(|a| (a.popup, a.kind, a.mode, a.set_ms)) else {
        return;
    };
    if CRAFT_KINDS.contains(&kind) {
        craft_report(set_ms, now);
    }
    let last = SEEN_MS.load(Ordering::Acquire).max(set_ms);
    // A craft search can leave no card at all (no predicate calls), so there only the popup kind and
    // its layer state count.
    let stale = !CRAFT_KINDS.contains(&kind) && now.saturating_sub(last) > REOPEN_GAP_MS;
    let closed = stale || searchable_kind(popup) != Some(kind);
    let changed = !closed && kind == PICKER_KIND && read_i32(popup + POPUP_MODE) != Some(mode);
    if closed || changed {
        st.search = None;
        st.matches.clear();
        FILTER_ON.store(false, Ordering::Release);
        log_line(if closed { "picker closed: search cleared" } else { "picker changed: search cleared" });
    }
}

/// Back to the first card, so a short result list is not shown from a page past its end. Only
/// while the same searchable list is still open and shown: the box may have closed after the
/// picker did, and then the offset is not the picker's any more.
fn reset_scroll(popup: usize, kind: i32) {
    if searchable_kind(popup) != Some(kind) {
        return;
    }
    if popup >= MIN_ADDR && read_i32(popup + POPUP_SCROLL).is_some_and(|v| v != 0) {
        // SAFETY: the picker's own scroll offset; 0 is always a valid value (the first card).
        unsafe {
            let _ = nms_core::mem::write(popup + POPUP_SCROLL, 0i32);
        }
    }
}

fn id_of(element: usize) -> Option<[u8; 16]> {
    read_bytes(element, 16).and_then(|b| <[u8; 16]>::try_from(b).ok())
}

/// The printable part of an item id (`FUEL1`, `^PROC#123`).
fn id_text(id: &[u8; 16]) -> String {
    let end = id.iter().position(|&b| b == 0).unwrap_or(16);
    id[..end].iter().filter(|b| b.is_ascii_graphic()).map(|&b| b as char).collect()
}

/// A NUL-terminated string at `addr`, at most `max` bytes, never reading past readable memory.
fn cstr(addr: usize, max: usize) -> Option<Vec<u8>> {
    if addr < MIN_ADDR {
        return None;
    }
    let mut bytes = if readable(addr, max) {
        read_bytes(addr, max)?
    } else {
        let mut out = Vec::new();
        for i in 0..max {
            match read_u8(addr + i) {
                Some(b) => out.push(b),
                None => break,
            }
        }
        out
    };
    if let Some(end) = bytes.iter().position(|&b| b == 0) {
        bytes.truncate(end);
    }
    Some(bytes)
}

/// Text without the game's `<TAG>` markup.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// `(display text, name key, resolved)`. Calls only the game's pure lookups, once per element.
fn resolve(element: usize, id: &[u8; 16]) -> (String, String, bool) {
    let fallback = id_text(id);
    let Some(l) = LOOKUPS.get() else { return (fallback, String::new(), false) };
    let Some(app) = text_box::app() else { return (fallback, String::new(), false) };
    let (record, offset) = match read_u32(element + ELEMENT_TYPE) {
        // SAFETY: `lookup(app+0x60, id*) -> record`, the same call the game's tooltip and this very
        // predicate make for the same element; a pure hash find.
        Some(0) => (unsafe { std::mem::transmute::<usize, LookupFn>(l.substance)(app + 0x60, element) }, SUBSTANCE_NAME),
        Some(2) => (unsafe { std::mem::transmute::<usize, LookupFn>(l.product)(app + 0x60, element) }, PRODUCT_NAME),
        // SAFETY: `tech(app+0x60, id*, 0) -> record` as the slot filler calls it (`0x1406C8E3B`),
        // with "build a missing procedural technology" off: a hash find and a list walk only.
        Some(1) if l.tech != 0 => (unsafe { std::mem::transmute::<usize, TechFn>(l.tech)(app + 0x60, element, 0) }, TECH_NAME),
        _ => return (fallback, String::new(), false),
    };
    if record < MIN_ADDR {
        return (fallback, String::new(), false);
    }
    let Some(raw) = read_bytes(record + offset, KEY_LEN) else { return (fallback, String::new(), false) };
    let end = raw.iter().position(|&b| b == 0).unwrap_or(KEY_LEN - 1).min(KEY_LEN - 1);
    if end == 0 {
        return (fallback, String::new(), false);
    }
    let key = String::from_utf8_lossy(&raw[..end]).into_owned();
    // Our own NUL-terminated copy, so the lookup reads nothing but this buffer.
    let mut buf = [0u8; KEY_LEN + 1];
    buf[..end].copy_from_slice(&raw[..end]);
    // SAFETY: the loc table accessor returns a static built at boot; the lookup is
    // `(table, key, 0)` exactly as the game calls it at 0x1406BB400, and only reads.
    let table = unsafe { std::mem::transmute::<usize, LocTableFn>(l.loc_table)() };
    if table < MIN_ADDR {
        return (key.clone(), key, false);
    }
    let text = unsafe { std::mem::transmute::<usize, LocFn>(l.loc)(table, buf.as_ptr(), 0) };
    if text == buf.as_ptr() as usize {
        return (key.clone(), key, false);
    }
    match cstr(text, TEXT_MAX).map(|b| strip_tags(&String::from_utf8_lossy(&b))) {
        Some(t) if !t.is_empty() => (t, key, true),
        _ => (key.clone(), key, false),
    }
}

/// What a search is matched against: the shown name and the item id, lower case.
fn searched_text(text: &str, raw_id: &str) -> String {
    if text.eq_ignore_ascii_case(raw_id) { text.to_lowercase() } else { format!("{text}\n{raw_id}").to_lowercase() }
}

impl State {
    fn new_open(&mut self, popup: usize) {
        self.open_popup = popup;
        self.names.clear();
        self.matches.clear();
        self.open_lines = 0;
    }

    fn note_name(&mut self, element: usize, id: [u8; 16], log: bool) {
        if self.names.contains_key(&id) {
            return;
        }
        let (text, key, resolved) = resolve(element, &id);
        if log && self.open_lines < NAME_LINES_PER_OPEN && self.lines < NAME_LINES_MAX {
            self.open_lines += 1;
            self.lines += 1;
            log_line(&format!(
                "name id={} type={} key=\"{key}\" text=\"{text}\" ({})",
                id_text(&id),
                read_u32(element + ELEMENT_TYPE).map_or_else(|| "?".to_string(), |t| t.to_string()),
                if resolved { "resolved" } else { "not resolved, raw key used" }
            ));
        }
        // The shown name and the item id are both searched (some ids have no name key).
        self.names.insert(id, searched_text(&text, &id_text(&id)));
    }

    /// False when a search is set for this popup and the element's name does not contain it.
    fn passes(&mut self, popup: usize, id: [u8; 16]) -> bool {
        let Some(active) = &self.search else { return true };
        if active.popup != popup {
            return true;
        }
        if let Some(&hit) = self.matches.get(&id) {
            return hit;
        }
        let hit = self.names.get(&id).is_some_and(|n| n.contains(&active.text));
        self.matches.insert(id, hit);
        hit
    }
}

/// Recipes the craft list kept and dropped since the search was set (for one log line).
static CRAFT_KEPT: AtomicU64 = AtomicU64::new(0);
static CRAFT_DROPPED: AtomicU64 = AtomicU64::new(0);
static CRAFT_REPORTED: AtomicBool = AtomicBool::new(true);

/// The craft list is about to add the recipe `element` (id `+0`, type `+0x24`) to its temporary
/// inventory: false when a craft-list search is set and the recipe's name does not contain it.
/// Pass-through (true, no lock, no lookup) when no craft-list search is active.
pub fn craft_keeps(element: usize) -> bool {
    if !FILTER_ON.load(Ordering::Acquire) || !PICKER_SEARCH.load(Ordering::Acquire) || element < MIN_ADDR {
        return true;
    }
    let Ok(mut st) = shared().try_lock() else { return true };
    let Some(popup) = st.search.as_ref().filter(|a| CRAFT_KINDS.contains(&a.kind)).map(|a| a.popup) else { return true };
    let Some(id) = id_of(element) else { return true };
    let log = LOG_NAMES.load(Ordering::Acquire);
    st.note_name(element, id, log);
    let keep = st.passes(popup, id);
    if keep {
        CRAFT_KEPT.fetch_add(1, Ordering::Relaxed);
    } else {
        CRAFT_DROPPED.fetch_add(1, Ordering::Relaxed);
    }
    keep
}

/// Once, about a second after a craft-list search was set: how many recipe adds it let through.
fn craft_report(set_ms: u64, now: u64) {
    if now.saturating_sub(set_ms) < 1000 || CRAFT_REPORTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let (kept, dropped) = (CRAFT_KEPT.load(Ordering::Relaxed), CRAFT_DROPPED.load(Ordering::Relaxed));
    log_line(&if kept + dropped == 0 {
        "craft list: no recipe was built since the search was set (filter not reached)".to_string()
    } else {
        format!("craft list: {kept} recipe adds kept, {dropped} dropped in the first second")
    });
}

/// The picker predicate: original first, then hide what the search does not match.
pub unsafe extern "system" fn pred_detour(popup: usize, element: usize, state: usize, out: usize) -> u32 {
    let original: PredFn = std::mem::transmute(ORIG_PRED.load(Ordering::Acquire));
    let ret = original(popup, element, state, out);
    let now = now_ms();
    let prev = SEEN_MS.swap(now, Ordering::AcqRel);
    SEEN_POPUP.store(popup, Ordering::Release);
    if ret == 2 || element < MIN_ADDR {
        return ret;
    }
    let filter = FILTER_ON.load(Ordering::Acquire) && PICKER_SEARCH.load(Ordering::Acquire);
    let log = LOG_NAMES.load(Ordering::Acquire);
    if !filter && !log {
        return ret;
    }
    // No filter state: the craft list (searched) or another popup (the recharge list, left alone).
    if state < MIN_ADDR && !read_i32(popup + POPUP_KIND).is_some_and(|k| CRAFT_KINDS.contains(&k)) {
        return ret;
    }
    let Ok(mut st) = shared().try_lock() else { return ret };
    if now.saturating_sub(prev) > REOPEN_GAP_MS || st.open_popup != popup {
        st.new_open(popup);
    }
    let Some(id) = id_of(element) else { return ret };
    st.note_name(element, id, log);
    if filter && !st.passes(popup, id) {
        return 2;
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_removed_and_ids_printed() {
        assert_eq!(strip_tags("<TECHNOLOGY>Ion Battery<>"), "Ion Battery");
        assert_eq!(strip_tags("Di-hydrogen"), "Di-hydrogen");
        let mut id = [0u8; 16];
        id[..5].copy_from_slice(b"FUEL1");
        assert_eq!(id_text(&id), "FUEL1");
    }

    #[test]
    fn a_search_matches_on_the_cached_name() {
        let mut st = State {
            search: Some(Active { text: "carbon".into(), popup: 7, kind: 0, mode: 0x3A, set_ms: 0 }),
            open_popup: 7,
            names: HashMap::new(),
            matches: HashMap::new(),
            open_lines: 0,
            lines: 0,
        };
        let a = [1u8; 16];
        let b = [2u8; 16];
        st.names.insert(a, "condensed carbon".into());
        st.names.insert(b, "ferrite dust".into());
        assert!(st.passes(7, a));
        assert!(!st.passes(7, b));
        // Another popup object is never filtered.
        assert!(st.passes(8, b));
    }

    #[test]
    fn name_and_id_are_both_searched() {
        let s = searched_text("Oxygen Recycler", "UT_ENERGY");
        assert!(s.contains("oxy") && s.contains("ut_en"));
        assert_eq!(searched_text("SUIT_REFINER", "SUIT_REFINER"), "suit_refiner");
        // cGcTechnology: NameLower at +0x244 (nmspy exported_types), not Group at +0x50.
        assert_eq!(TECH_NAME, 0x244);
    }

    #[test]
    fn popup_fields_sit_where_the_host_reads_them() {
        // Host 0x1408F2200: layer at page+0x14A58, its state at +0x198, its "stay open" byte at
        // +0x190 (= page+0x14BE8, cleared when page+0x14C44 is 0).
        assert_eq!(0x14A58 + 0x198, POPUP_LAYER_STATE);
        assert_eq!(0x14A58 + 0x190, 0x14BE8);
        assert_eq!(POPUP_KIND, 0x14C28);
        // Null pages are never open.
        assert_eq!(searchable_kind(0), None);
        assert!(!popup_up(0));
    }

    #[test]
    fn the_picker_and_the_craft_list_are_searchable() {
        assert!(is_searchable(0));
        assert!(is_searchable(2) && is_searchable(3));
        // Recharge list (4), other popups and "none" are not.
        assert!(!is_searchable(4) && !is_searchable(1) && !is_searchable(-1));
        assert_eq!(kind_label(3, 0), "craft list");
        assert_eq!(kind_label(0, 0x3A), "picker mode 0x3a");
    }
}
