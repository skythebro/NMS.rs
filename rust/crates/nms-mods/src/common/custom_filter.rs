//! State shared by the two extra filters that are not product categories: Foods and Custom.
//!
//! SmallerInventorySlots owns the side-bar buttons and the dim caves; QuickTransferAll reads the same
//! state in `Game::highlighted`, so Quick Transfer and Deposit move only lit items.
//!
//! - Foods lights a product whose `CookingValue` (float at product data `+0x1A0`) is above 0, or a
//!   Consumable that is a cooking ingredient (`CookingIngredient` byte at `+0x2F2`): that second
//!   arm adds exactly the nine `BAIT_*` creature feeds (Creature Pellets is `BAIT_BASIC`) and no
//!   ammo, fuel or upgrade module (checked against the product table; fish are included on
//!   purpose).
//! - Custom lights the items the player marked. A mark is an item identity, the same pair Exosuit lock
//!   uses (`quick_transfer_all::inv::item_identity`): element type at element `+0x24` and the 16-byte
//!   id at element `+0`. The `custom_mark` setting picks the mode ([`set_slot_mode`]):
//!   - per slot (default): the mark also holds the inventory store and the slot (x, y), so only the
//!     clicked stack lights and moves. It is dropped when that slot no longer holds the same item
//!     (moved away, used up, swapped: [`stale_slot_marks`], checked each time the grid draws).
//!   - per item: every stack of a marked item lights, in every inventory.
//!
//!   Session only: nothing is saved, a game restart clears every mark. Unless the `custom_remember`
//!   setting is on, the marks also clear when the player leaves the Custom filter ([`LeaveWatch`])
//!   or switches the Custom filter off. Changing the mode clears them too.
//!
//! Everything here is lock-free or `try_lock`, so it is safe to call from a detour.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

/// Page filter id of the Foods button (`FILTER18`).
pub const FOODS_ID: i32 = 17;
/// Page filter id of the Custom button (`FILTER19`).
pub const CUSTOM_ID: i32 = 18;
/// `cGcProductData::CookingValue` (float) in the in-memory product record.
pub const COOKING_VALUE: usize = 0x1A0;
/// `cGcProductData::CookingIngredient` (bool byte) in the in-memory product record.
pub const COOKING_INGREDIENT: usize = 0x2F2;
/// `cGcProductCategory::Consumable`.
pub const CONSUMABLE: i32 = 1;
/// Element type inside an inventory element (the dim cave compares the same dword).
pub const ELEMENT_KIND: usize = 0x24;
/// Slot x and y inside an inventory element (`i32` each; QuickTransferAll `ELEMENT_INDEX`).
pub const ELEMENT_X: usize = 0x10;
pub const ELEMENT_Y: usize = 0x14;
/// Marks the dim cave can hold. Marking more is refused and logged.
pub const MAX_MARKS: usize = 128;
/// Bytes per dim cave table entry ([`table_entry`]).
pub const ENTRY_SIZE: usize = 0x30;

pub type Identity = (i32, [u8; 16]);

/// Where a per-slot mark sits: inventory store pointer and slot (x, y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub store: usize,
    pub x: i32,
    pub y: i32,
}

/// One mark: the item, and for a per-slot mark the slot it sits in (`None`: every stack).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub ident: Identity,
    pub slot: Option<Slot>,
}

impl Mark {
    /// The rule the dim cave and `Game::highlighted` both apply to one element.
    pub fn lights(&self, ident: &Identity, at: Slot) -> bool {
        self.ident == *ident && self.slot.is_none_or(|slot| slot == at)
    }
}

static FOODS_ON: AtomicBool = AtomicBool::new(false);
static CUSTOM_ON: AtomicBool = AtomicBool::new(false);
static SLOT_MODE: AtomicBool = AtomicBool::new(true);
static GENERATION: AtomicU32 = AtomicU32::new(0);
static MARKS: Mutex<Vec<Mark>> = Mutex::new(Vec::new());

/// The Foods rule on the raw bits of `CookingValue`: lit when the value is above 0. The dim cave
/// tests the same bits as a signed dword (`cmp dword [data+0x1A0], 0; jg lit`), which equals
/// `value > 0.0` for every number (a positive NaN lights, which no product carries).
pub fn cooking_value_lit(bits: u32) -> bool {
    (bits as i32) > 0
}

/// The whole Foods rule: `CookingValue > 0`, or a Consumable flagged `CookingIngredient` (creature
/// pellets and the other bait feeds, which carry a cooking value of 0). The dim cave runs the same
/// three tests in the same order.
pub fn foods_lit(cooking_bits: u32, ingredient: u8, category: i32) -> bool {
    cooking_value_lit(cooking_bits) || (ingredient != 0 && category == CONSUMABLE)
}

/// Foods is switched on and its dim test is installed (set by SmallerInventorySlots).
pub fn foods_on() -> bool {
    FOODS_ON.load(Ordering::Relaxed)
}

pub fn set_foods_on(on: bool) {
    FOODS_ON.store(on, Ordering::Relaxed);
}

/// Custom is switched on and its dim test is installed (set by SmallerInventorySlots).
pub fn custom_on() -> bool {
    CUSTOM_ON.load(Ordering::Relaxed)
}

pub fn set_custom_on(on: bool) {
    CUSTOM_ON.store(on, Ordering::Relaxed);
}

/// Bumped on every change of the marks, so the dim cave table is rewritten only when needed.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// Result of a mark click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Marked,
    Unmarked,
    Full,
    Busy,
}

/// Marks are per slot (only the clicked stack), not per item id.
pub fn slot_mode() -> bool {
    SLOT_MODE.load(Ordering::Relaxed)
}

/// Sets the mark mode. A change drops every mark (an item mark and a slot mark do not mix in one
/// click language). Returns how many marks were dropped.
pub fn set_slot_mode(on: bool) -> usize {
    if SLOT_MODE.swap(on, Ordering::Relaxed) == on {
        return 0;
    }
    clear()
}

/// Marks the item (per item mode) or the stack in that slot (per slot mode), or unmarks it when it
/// was marked already.
pub fn toggle(kind: i32, id: [u8; 16], at: Slot) -> Toggle {
    let Ok(mut marks) = MARKS.try_lock() else {
        return Toggle::Busy;
    };
    let result = toggle_in(&mut marks, (kind, id), slot_mode().then_some(at));
    if matches!(result, Toggle::Marked | Toggle::Unmarked) {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
    result
}

fn toggle_in(marks: &mut Vec<Mark>, ident: Identity, slot: Option<Slot>) -> Toggle {
    let same_place = |m: &Mark| match slot {
        Some(_) => m.slot == slot,
        None => m.slot.is_none() && m.ident == ident,
    };
    if let Some(at) = marks.iter().position(same_place) {
        if marks[at].ident == ident {
            marks.remove(at);
            return Toggle::Unmarked;
        }
        // A slot mark left over from another item in that slot: the click marks the new one.
        marks[at].ident = ident;
        return Toggle::Marked;
    }
    if marks.len() >= MAX_MARKS {
        return Toggle::Full;
    }
    marks.push(Mark { ident, slot });
    Toggle::Marked
}

/// Per-slot marks of one store as (x, y, item), for the stale check after the grid draws.
pub fn slot_marks_in(store: usize) -> Vec<(i32, i32, Identity)> {
    MARKS
        .try_lock()
        .map(|marks| {
            marks
                .iter()
                .filter_map(|m| m.slot.filter(|s| s.store == store).map(|s| (s.x, s.y, m.ident)))
                .collect()
        })
        .unwrap_or_default()
}

/// The per-slot marks of `store` whose slot no longer holds the marked item. `now` gives what the
/// slot holds now (`None`: empty).
pub fn stale_slot_marks(
    marks: &[(i32, i32, Identity)],
    mut now: impl FnMut(i32, i32) -> Option<Identity>,
) -> Vec<(i32, i32)> {
    marks
        .iter()
        .filter(|(x, y, ident)| now(*x, *y) != Some(*ident))
        .map(|(x, y, _)| (*x, *y))
        .collect()
}

/// Drops the per-slot marks at these slots of `store`. Returns how many went.
pub fn unmark_slots(store: usize, slots: &[(i32, i32)]) -> usize {
    let Ok(mut marks) = MARKS.try_lock() else {
        return 0;
    };
    let before = marks.len();
    marks.retain(|m| !m.slot.is_some_and(|s| s.store == store && slots.contains(&(s.x, s.y))));
    let gone = before - marks.len();
    if gone > 0 {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
    gone
}

/// Drops every mark. Returns how many there were (0 also when the lock is busy).
pub fn clear() -> usize {
    let Ok(mut marks) = MARKS.try_lock() else {
        return 0;
    };
    let n = marks.len();
    if n > 0 {
        marks.clear();
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
    n
}

/// How long the page filter must stay off Custom before the marks clear. Vanilla resets the filter
/// to All for a frame or two on a tab switch, and "Keep filter per tab" may then restore Custom;
/// that blip must not count as leaving.
pub const LEAVE_MS: u64 = 400;

/// Watches the page filter and says when the player has left the Custom filter: it was Custom on
/// a screen (the page's tab: Exosuit, Starship, a container...), then that same screen stayed on
/// another filter for [`LEAVE_MS`]. A change of screen is not a leave (vanilla resets the filter
/// to All there); coming back to the screen on Custom carries on. Fed from the inventory draw, so a
/// closed menu is not seen; reopening the same screen on another filter counts as leaving.
#[derive(Clone, Copy, Debug, Default)]
pub struct LeaveWatch {
    custom_screen: Option<i32>,
    left_at: Option<u64>,
}

impl LeaveWatch {
    /// One sample of the page filter and screen at `now_ms`. Returns true once per leave: clear the
    /// marks now.
    pub fn step(&mut self, filter: i32, screen: i32, now_ms: u64) -> bool {
        if filter == CUSTOM_ID {
            self.custom_screen = Some(screen);
            self.left_at = None;
            return false;
        }
        if self.custom_screen != Some(screen) {
            self.left_at = None;
            return false;
        }
        match self.left_at {
            None => {
                self.left_at = Some(now_ms);
                false
            }
            Some(at) if now_ms.saturating_sub(at) >= LEAVE_MS => {
                *self = Self::default();
                true
            }
            Some(_) => false,
        }
    }
}

/// Whether the item in slot `at` lights under Custom. A busy lock reads as not marked (one frame
/// at most).
pub fn lit(kind: i32, id: &[u8; 16], at: Slot) -> bool {
    MARKS
        .try_lock()
        .map(|marks| marks.iter().any(|m| m.lights(&(kind, *id), at)))
        .unwrap_or(false)
}

/// A copy of the marks for the dim cave table, or `None` when the lock is busy.
pub fn snapshot() -> Option<Vec<Mark>> {
    MARKS.try_lock().ok().map(|marks| marks.clone())
}

pub fn len() -> usize {
    MARKS.try_lock().map(|marks| marks.len()).unwrap_or(0)
}

/// The dim cave table entry for one mark: 16-byte id, element type (`+0x10`), slot x and y (`+0x14`,
/// `+0x18`), store pointer (`+0x20`, 0 for a per-item mark, which skips the slot test), padding.
pub fn table_entry(mark: &Mark) -> [u8; ENTRY_SIZE] {
    let mut entry = [0u8; ENTRY_SIZE];
    entry[..16].copy_from_slice(&mark.ident.1);
    entry[16..20].copy_from_slice(&mark.ident.0.to_le_bytes());
    if let Some(slot) = mark.slot {
        entry[20..24].copy_from_slice(&slot.x.to_le_bytes());
        entry[24..28].copy_from_slice(&slot.y.to_le_bytes());
        entry[32..40].copy_from_slice(&(slot.store as u64).to_le_bytes());
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &[u8]) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..name.len()].copy_from_slice(name);
        out
    }

    #[test]
    fn foods_rule_matches_float_compare() {
        // Product-table facts: every FOOD_* item and every fish has a value
        // above 0; reactor cores, bait, fuels, upgrade modules and NIPNIPBUDS have exactly 0.
        let food = [1.0f32, 2.5, 16.0, 0.001, 300.0];
        let not_food = [0.0f32, -0.0, -1.0];
        for v in food {
            assert!(cooking_value_lit(v.to_bits()), "{v} should light");
            assert!(v > 0.0);
        }
        for v in not_food {
            assert!(!cooking_value_lit(v.to_bits()), "{v} should stay dim");
        }
        for v in [f32::MIN_POSITIVE, f32::MAX, f32::INFINITY, -f32::INFINITY, f32::MIN] {
            assert_eq!(cooking_value_lit(v.to_bits()), v > 0.0);
        }
    }

    #[test]
    fn foods_rule_adds_pellets_not_ammo() {
        // Rows from NMS_REALITY_GCPRODUCTTABLE (category ordinals: Consumable 1, Fish 9, SpaceSalvage 11,
        // Curiosity 3): (CookingValue, CookingIngredient, category, lit).
        let rows: [(&str, f32, u8, i32, bool); 8] = [
            ("FOOD_P_POOP", 12.0, 1, 1, true),
            ("F_FISH (fish)", 20.0, 1, 9, true),
            ("BAIT_BASIC (Creature Pellets)", 0.0, 1, 1, true),
            ("BAIT_MEAT_1", 0.0, 1, 1, true),
            ("AMMO", 0.0, 0, 1, false),
            ("U_SHIELDBOOST (upgrade module)", 0.0, 0, 1, false),
            ("HULK_REACTOR", 0.0, 1, 11, false),
            ("NIPNIPBUDS", 0.0, 1, 3, false),
        ];
        for (name, value, ingredient, category, lit) in rows {
            assert_eq!(foods_lit(value.to_bits(), ingredient, category), lit, "{name}");
        }
    }

    fn item(ident: Identity) -> Mark {
        Mark { ident, slot: None }
    }

    const SUIT: usize = 0x7FF6_0000_1000;
    const SHIP: usize = 0x7FF6_0000_2000;

    fn at(store: usize, x: i32, y: i32) -> Slot {
        Slot { store, x, y }
    }

    #[test]
    fn toggle_marks_and_unmarks() {
        let mut marks = Vec::new();
        assert_eq!(toggle_in(&mut marks, (2, id(b"FOOD_P_POOP")), None), Toggle::Marked);
        assert_eq!(toggle_in(&mut marks, (0, id(b"FUEL1")), None), Toggle::Marked);
        assert_eq!(marks.len(), 2);
        assert_eq!(toggle_in(&mut marks, (2, id(b"FOOD_P_POOP")), None), Toggle::Unmarked);
        assert_eq!(marks, vec![item((0, id(b"FUEL1")))]);
        // Same id, other element type, is another item.
        assert_eq!(toggle_in(&mut marks, (2, id(b"FUEL1")), None), Toggle::Marked);
        let mut full: Vec<Mark> = (0..MAX_MARKS as i32).map(|n| item((n, id(b"X")))).collect();
        assert_eq!(toggle_in(&mut full, (-1, id(b"Y")), None), Toggle::Full);
        assert_eq!(toggle_in(&mut full, (0, id(b"X")), None), Toggle::Unmarked);
    }

    #[test]
    fn per_item_mark_lights_every_stack() {
        // Reported case: Ion Battery 5/200 and 2/200 in two slots. A per-item mark lights both, in
        // any inventory.
        let battery = (2, id(b"POWERCELL"));
        let mark = item(battery);
        assert!(mark.lights(&battery, at(SUIT, 0, 0)));
        assert!(mark.lights(&battery, at(SUIT, 3, 1)));
        assert!(mark.lights(&battery, at(SHIP, 0, 0)));
        assert!(!mark.lights(&(2, id(b"FUEL1")), at(SUIT, 0, 0)));
    }

    #[test]
    fn per_slot_mark_lights_only_the_clicked_stack() {
        let battery = (2, id(b"POWERCELL"));
        let mut marks = Vec::new();
        assert_eq!(toggle_in(&mut marks, battery, Some(at(SUIT, 0, 0))), Toggle::Marked);
        let lit = |ident: &Identity, slot: Slot| marks.iter().any(|m| m.lights(ident, slot));
        assert!(lit(&battery, at(SUIT, 0, 0)));
        // The other stack of the same item, the same slot of another inventory, another item there.
        assert!(!lit(&battery, at(SUIT, 3, 1)));
        assert!(!lit(&battery, at(SHIP, 0, 0)));
        assert!(!lit(&(2, id(b"FUEL1")), at(SUIT, 0, 0)));
        // Marking the second stack keeps both; clicking the first again unmarks only it.
        assert_eq!(toggle_in(&mut marks, battery, Some(at(SUIT, 3, 1))), Toggle::Marked);
        assert_eq!(toggle_in(&mut marks, battery, Some(at(SUIT, 0, 0))), Toggle::Unmarked);
        assert_eq!(marks, vec![Mark { ident: battery, slot: Some(at(SUIT, 3, 1)) }]);
        // A slot now holding another item: the click re-marks that slot for the new item.
        let fuel = (0, id(b"FUEL1"));
        assert_eq!(toggle_in(&mut marks, fuel, Some(at(SUIT, 3, 1))), Toggle::Marked);
        assert_eq!(marks, vec![Mark { ident: fuel, slot: Some(at(SUIT, 3, 1)) }]);
    }

    #[test]
    fn stale_slot_marks_follow_the_slot() {
        let battery = (2, id(b"POWERCELL"));
        let fuel = (0, id(b"FUEL1"));
        let marks = [(0, 0, battery), (1, 0, battery), (2, 0, fuel)];
        // Slot 0 still holds the battery (fewer of them is fine), slot 1 is empty now (moved away or
        // used up), slot 2 holds another item (swap).
        let now = |x: i32, _y: i32| match x {
            0 => Some(battery),
            2 => Some(battery),
            _ => None,
        };
        assert_eq!(stale_slot_marks(&marks, now), vec![(1, 0), (2, 0)]);
        assert!(stale_slot_marks(&marks[..1], now).is_empty());
    }

    #[test]
    fn mode_change_clears_marks() {
        // The only test that touches the shared statics.
        let was = slot_mode();
        assert!(was, "per slot is the default mode");
        assert_eq!(set_slot_mode(was), 0);
        assert_eq!(toggle(2, id(b"MODE_TEST"), at(SUIT, 4, 0)), Toggle::Marked);
        assert_eq!(slot_marks_in(SUIT), vec![(4, 0, (2, id(b"MODE_TEST")))]);
        assert!(lit(2, &id(b"MODE_TEST"), at(SUIT, 4, 0)));
        assert!(!lit(2, &id(b"MODE_TEST"), at(SUIT, 5, 0)));
        assert_eq!(unmark_slots(SUIT, &[(4, 0)]), 1);
        assert_eq!(toggle(2, id(b"MODE_TEST"), at(SUIT, 4, 0)), Toggle::Marked);
        assert_eq!(set_slot_mode(!was), 1);
        assert_eq!(len(), 0);
        assert_eq!(toggle(2, id(b"MODE_TEST"), at(SUIT, 4, 0)), Toggle::Marked);
        assert!(lit(2, &id(b"MODE_TEST"), at(SHIP, 9, 9)));
        assert!(slot_marks_in(SUIT).is_empty());
        assert_eq!(set_slot_mode(was), 1);
    }

    #[test]
    fn leave_watch_clears_once_after_a_steady_leave() {
        let mut w = LeaveWatch::default();
        // Never on Custom: nothing to clear.
        assert!(!w.step(0, 0, 0));
        assert!(!w.step(0, 0, 10_000));
        // Custom on screen 0, then a short All blip (filter memory restores Custom): no clear.
        assert!(!w.step(CUSTOM_ID, 0, 10_000));
        assert!(!w.step(0, 0, 10_016));
        assert!(!w.step(0, 0, 10_016 + LEAVE_MS - 1));
        assert!(!w.step(CUSTOM_ID, 0, 10_016 + LEAVE_MS));
        // Another screen on All for a long time (vanilla reset on a tab change): no clear.
        assert!(!w.step(0, 1, 11_000));
        assert!(!w.step(0, 1, 20_000));
        // Back on screen 0, still Custom: still no clear. Then another filter there: clears once.
        assert!(!w.step(CUSTOM_ID, 0, 21_000));
        assert!(!w.step(5, 0, 22_000));
        assert!(!w.step(5, 0, 22_000 + LEAVE_MS / 2));
        assert!(w.step(5, 0, 22_000 + LEAVE_MS));
        assert!(!w.step(5, 0, 30_000));
        assert!(!w.step(0, 0, 40_000));
    }

    #[test]
    fn table_entry_layout() {
        let entry = table_entry(&item((2, id(b"CASING"))));
        assert_eq!(&entry[..6], b"CASING");
        assert_eq!(&entry[16..20], &2i32.to_le_bytes());
        // Store 0: the cave skips the slot test (per-item mark).
        assert!(entry[20..].iter().all(|b| *b == 0));
        let slot = Mark { ident: (2, id(b"CASING")), slot: Some(at(SHIP, 7, 3)) };
        let entry = table_entry(&slot);
        assert_eq!(&entry[20..24], &7i32.to_le_bytes());
        assert_eq!(&entry[24..28], &3i32.to_le_bytes());
        assert_eq!(&entry[28..32], &[0, 0, 0, 0]);
        assert_eq!(&entry[32..40], &(SHIP as u64).to_le_bytes());
        assert!(entry[40..].iter().all(|b| *b == 0));
    }

    #[test]
    fn ids_follow_the_side_bar() {
        assert_eq!(FOODS_ID, 17);
        assert_eq!(CUSTOM_ID, FOODS_ID + 1);
    }
}
