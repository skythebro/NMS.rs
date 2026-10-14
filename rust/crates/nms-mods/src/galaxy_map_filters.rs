//! Galaxy map filters.
//!
//! An optional log-only probe records the hovered-system card. The mod adds extra entries to the
//! galaxy map Filter dropdown (Gas Giants, Abandoned, Pirate, star colours). A filter lights the
//! stars that match and dims the rest.
//!
//! How it fits the game (offsets re-read in the disassembly, see the sig table):
//! - The dropdown has four entries and a mod-4 cycle. With an extra filter on, the cycle patches
//!   widen to 32 slots and the list shows only the enabled entries, packed after the vanilla four.
//!   Slot `k >= 4` maps to a filter through [`SLOT`]. With every extra filter off nothing is patched.
//! - `HasFilterScanner` (the unlock gate) says yes to every packed slot.
//! - `ApplyFilterColours` bakes the vanilla star colours for the voxel, then a second pass
//!   classifies each star and scales its colour: bright for a match, dim for the rest.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::mem::{self, read_f32, read_ptr, read_u32, read_u64, read_u8, readable};
use nms_core::process::ProcessImage;
use nms_core::{paths, Code, CodePatch, GameMod, Kind, Pattern, SettingSpec, Settings};
use nms_sdk::sigs::galaxy_map_filters as sigs;
use serde_json::{json, Value};

type PanelFn = unsafe extern "system" fn(u64) -> u64;
type ApplyFn = unsafe extern "system" fn(usize, u64, u32) -> u64;
type GateFn = unsafe extern "system" fn(usize, u32) -> u8;
type PollFn = unsafe extern "system" fn(usize) -> u64;
type SetterFn = unsafe extern "system" fn(usize, u32) -> u64;
type LookupFn = unsafe extern "system" fn(usize, *mut u64) -> usize;
type ClassifyFn = unsafe extern "system" fn(u64, *mut u8) -> u64;
type DefaultsFn = unsafe extern "system" fn(*mut u8) -> u64;

/// Field offsets on `SolarInfoPanel`.
const STATE: usize = 0x504;
const UA: usize = 0x5D0;
const DISCOVERY: usize = 0x610;
const DISTANCE: usize = 0x8A0;
const RACE: usize = 0xF30;
const WEALTH: usize = 0xF34;
const CONFLICT: usize = 0xF3C;
const STAR_TYPE: usize = 0xF50;
const WATER: usize = 0xF80;
const WORM: usize = 0xF81;
const CORRUPT: usize = 0xF82;
const GAS_GIANT: usize = 0xF83;
const VISITED: usize = 0xF86;
const PANEL_LEN: usize = 0xF90;

/// `cGcGalaxyMap::Data` and the voxel blob.
const DATA_FILTER: usize = 0x20;
const DATA_VOXEL_CACHE: usize = 0x50;
const BLOB_NODES: usize = 0x10;
const BLOB_COUNT: usize = 0x254;
const BLOB_UAS: usize = 0x258;
const BLOB_DIRTY: usize = 0x270;
const NODE_STRIDE: usize = 0x30;
const NODE_COLOUR: usize = 0x20;
/// `cGcGalaxyStarAttributesData` (size 0x6B0): colour group, abandoned, gas giant, pirate.
const ATTR_TYPE: usize = 0x6A0;
const ATTR_ABANDONED: usize = 0x6A4;
const ATTR_GAS_GIANT: usize = 0x6A5;
const ATTR_PIRATE: usize = 0x6A7;
/// Byte next to the others, set from key attribute `+0x2C` (meaning not known yet; tallied in the log).
const ATTR_UNKNOWN: usize = 0x6A6;

/// Dropdown slots: the vanilla four, then up to 28 extra. The cycle patches wrap at 32.
const SLOTS: usize = 32;
const VANILLA: usize = 4;
/// Each loc id slot in the game's table is 0x20 bytes.
const ID_LEN: usize = 0x20;
const MAX_LINES: u32 = 1500;
/// A star that does not match is painted this dark blue-grey, a tenth of its own brightness kept
/// out so it never reads as white. With `debug_paint` on, matches are green and the rest magenta.
const OFF_COLOUR: [f32; 3] = [0.035, 0.035, 0.05];
const OFF_ALPHA: f32 = 0.5;
const DEBUG_ON: [f32; 3] = [0.0, 1.0, 0.0];
const DEBUG_OFF: [f32; 3] = [1.0, 0.0, 1.0];
/// Recolour calls to log in full after a filter is picked.
const TRACE_CALLS: u32 = 12;

static ORIG_PANEL: AtomicUsize = AtomicUsize::new(0);
static ORIG_APPLY: AtomicUsize = AtomicUsize::new(0);
static ORIG_GATE: AtomicUsize = AtomicUsize::new(0);
static ORIG_POLL: AtomicUsize = AtomicUsize::new(0);
/// Dropdown size now: 4 with no extra filter, else 4 plus the enabled ones.
static COUNT: AtomicU32 = AtomicU32::new(VANILLA as u32);
/// The list bound this mod last wrote (the vanilla 4 until then). The live byte must be this or the
/// byte read at install before it is changed again.
static LAST_BOUND: AtomicU8 = AtomicU8::new(VANILLA as u8);
/// For dropdown slot `k`: the index into [`FILTERS`] plus 1, or 0.
static SLOT: [AtomicU8; SLOTS] = [const { AtomicU8::new(0) }; SLOTS];
/// Set when the layout changed; the poll then resets a selected extra filter to No Filter.
static DIRTY: AtomicBool = AtomicBool::new(false);
static DEBUG_PAINT: AtomicBool = AtomicBool::new(false);
static TRACE: AtomicU32 = AtomicU32::new(0);
static FNS: OnceLock<Fns> = OnceLock::new();
static PROBE: OnceLock<Arc<Probe>> = OnceLock::new();

/// Tally of every star the recolour pass classified, to check what each flag really covers.
/// Logged every [`STATS_EVERY`] voxels while a filter is selected.
const STATS_EVERY: u32 = 25;
const TYPES: usize = 5;
struct Stats {
    voxels: AtomicU32,
    stars: AtomicU32,
    by_type: [AtomicU32; TYPES],
    gas_by_type: [AtomicU32; TYPES],
    abandoned_by_type: [AtomicU32; TYPES],
    pirate_by_type: [AtomicU32; TYPES],
    flag_6a6: AtomicU32,
}
static STATS: Stats = Stats {
    voxels: AtomicU32::new(0),
    stars: AtomicU32::new(0),
    by_type: [const { AtomicU32::new(0) }; TYPES],
    gas_by_type: [const { AtomicU32::new(0) }; TYPES],
    abandoned_by_type: [const { AtomicU32::new(0) }; TYPES],
    pirate_by_type: [const { AtomicU32::new(0) }; TYPES],
    flag_6a6: AtomicU32::new(0),
};

fn counts(a: &[AtomicU32; TYPES]) -> String {
    a.iter().map(|n| n.load(Ordering::Relaxed).to_string()).collect::<Vec<_>>().join("/")
}

impl Stats {
    fn note(&self, attrs: &Attrs) {
        let ty = u32::from_le_bytes([attrs.0[ATTR_TYPE], attrs.0[ATTR_TYPE + 1], attrs.0[ATTR_TYPE + 2], attrs.0[ATTR_TYPE + 3]]);
        let t = (ty as usize).min(TYPES - 1);
        self.stars.fetch_add(1, Ordering::Relaxed);
        self.by_type[t].fetch_add(1, Ordering::Relaxed);
        if attrs.0[ATTR_GAS_GIANT] != 0 {
            self.gas_by_type[t].fetch_add(1, Ordering::Relaxed);
        }
        if attrs.0[ATTR_ABANDONED] != 0 {
            self.abandoned_by_type[t].fetch_add(1, Ordering::Relaxed);
        }
        if attrs.0[ATTR_PIRATE] != 0 {
            self.pirate_by_type[t].fetch_add(1, Ordering::Relaxed);
        }
        if attrs.0[ATTR_UNKNOWN] != 0 {
            self.flag_6a6.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// One log line per [`STATS_EVERY`] voxels. Counts are per star colour group, yellow/green/blue/red/purple.
    fn voxel_done(&self) {
        let n = self.voxels.fetch_add(1, Ordering::Relaxed) + 1;
        if n % STATS_EVERY != 0 {
            return;
        }
        if let Some(p) = PROBE.get() {
            p.log.info(&format!(
                "star tally after {n} voxels: stars={} by_type={} gas_giant={} abandoned={} pirate={} flag_6a6={}",
                self.stars.load(Ordering::Relaxed),
                counts(&self.by_type),
                counts(&self.gas_by_type),
                counts(&self.abandoned_by_type),
                counts(&self.pirate_by_type),
                self.flag_6a6.load(Ordering::Relaxed),
            ));
        }
    }
}

struct Fns {
    setter: usize,
    lookup: usize,
    classify: usize,
    defaults: usize,
}

struct Probe {
    log: Arc<Log>,
    on: AtomicBool,
    last_key: AtomicU64,
    lines: AtomicU32,
}

#[derive(Clone, Copy)]
enum Test {
    GasGiant,
    Abandoned,
    Pirate,
    /// `cGcGalaxyStarTypes`: 0 Yellow, 1 Green, 2 Blue, 3 Red, 4 Purple.
    Colour(u32),
}

struct Filter {
    /// Colour of a star that matches: vivid, so it reads against the dark ones.
    colour: [f32; 3],
    key: &'static str,
    /// Dropdown text; at most 31 bytes. Used as the loc id, so keep it plain ASCII.
    label: &'static str,
    test: Test,
}

/// One row per extra filter, in dropdown order. Adding a filter is a row here, a `SettingSpec`
/// below and the strings in `menu_strings`.
const FILTERS: &[Filter] = &[
    Filter { colour: [0.25, 0.55, 1.0], key: "filter_blue", label: "Blue Stars", test: Test::Colour(2) },
    Filter { colour: [0.25, 1.0, 0.35], key: "filter_green", label: "Green Stars", test: Test::Colour(1) },
    Filter { colour: [1.0, 0.2, 0.15], key: "filter_red", label: "Red Stars", test: Test::Colour(3) },
    Filter { colour: [1.0, 0.9, 0.15], key: "filter_yellow", label: "Yellow Stars", test: Test::Colour(0) },
    Filter { colour: [0.75, 0.3, 1.0], key: "filter_purple", label: "Purple Stars", test: Test::Colour(4) },
    Filter { colour: [1.0, 0.1, 0.1], key: "filter_pirate", label: "Pirate Systems", test: Test::Pirate },
    Filter { colour: [0.1, 1.0, 0.9], key: "filter_abandoned", label: "Abandoned Systems", test: Test::Abandoned },
    Filter { colour: [0.75, 0.3, 1.0], key: "filter_gas_giant", label: "Gas Giants", test: Test::GasGiant },
];

const SPECS: &[SettingSpec] = &[
    SettingSpec { key: "status", label: "Status", kind: Kind::Status },
    SettingSpec { key: "filter_blue", label: "Blue Stars filter", kind: Kind::Bool },
    SettingSpec { key: "filter_green", label: "Green Stars filter", kind: Kind::Bool },
    SettingSpec { key: "filter_red", label: "Red Stars filter", kind: Kind::Bool },
    SettingSpec { key: "filter_yellow", label: "Yellow Stars filter", kind: Kind::Bool },
    SettingSpec { key: "filter_purple", label: "Purple Stars filter", kind: Kind::Bool },
    SettingSpec { key: "filter_pirate", label: "Pirate Systems filter", kind: Kind::Bool },
    SettingSpec { key: "filter_abandoned", label: "Abandoned Systems filter", kind: Kind::Bool },
    SettingSpec { key: "filter_gas_giant", label: "Gas Giants filter", kind: Kind::Bool },
    SettingSpec { key: "debug_paint", label: "Debug paint", kind: Kind::Bool },
    SettingSpec { key: "probe", label: "Log hovered systems", kind: Kind::Bool },
    SettingSpec { key: "write_logs", label: "Write log", kind: Kind::Bool },
];

/// Where the game's filter list is built, and our copy of its loc id table.
struct Layout {
    image: ProcessImage,
    /// RVA of the `lea rsi,[ids]` signature start; its disp32 is at +7.
    ids_rva: usize,
    orig_disp: i32,
    /// RVA of the `cmp ebx, 4` signature start; its imm8 is at +8.
    bound_rva: usize,
    /// The bound byte read at install (checked to be `VANILLA`); written back on undo.
    orig_bound: u8,
    table: usize,
}

pub struct GalaxyMapFilters {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    layout: Option<Layout>,
    /// Game language code for the dropdown labels (`menu_strings`).
    lang: &'static str,
    hooks: Vec<Hook>,
}

fn set_status(status: &Mutex<String>, text: &str) {
    if let Ok(mut s) = status.lock() {
        *s = text.to_string();
    }
}

fn find(image: &ProcessImage, text: &str) -> Result<usize, String> {
    let pat = Pattern::parse(text).map_err(|e| e.to_string())?;
    image
        .find_unique(&pat)
        .unique()
        .ok_or_else(|| "game code not found (game update?)".to_string())
}

impl GalaxyMapFilters {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "GalaxyMapFilters");
        let mut defaults = serde_json::Map::new();
        defaults.insert("write_logs".into(), json!(false));
        defaults.insert("probe".into(), json!(false));
        defaults.insert("debug_paint".into(), json!(false));
        for f in FILTERS {
            defaults.insert(f.key.into(), json!(false));
        }
        let settings = Arc::new(Mutex::new(Settings::load(
            &dir.join("galaxy_map_filters.json"),
            Value::Object(defaults),
        )));
        let (probe, write_logs) = settings
            .lock()
            .map(|s| (s.is_true("probe"), s.is_true("write_logs")))
            .unwrap_or((false, false));
        DEBUG_PAINT.store(settings.lock().map(|s| s.is_true("debug_paint")).unwrap_or(false), Ordering::Release);
        let log = Arc::new(Log::open(&dir.join("galaxy_map_filters.log"), probe || write_logs));
        let status = Arc::new(Mutex::new("Off, see log".to_string()));

        let lang = super::common::lang::detect(&ctx.image, &log);
        let mut me = Self { settings, status, log, layout: None, lang, hooks: Vec::new() };
        match install(&ctx.image, &me.log, probe) {
            Ok((hooks, layout)) => {
                me.hooks = hooks;
                me.layout = Some(layout);
                set_status(&me.status, "On");
                me.log.info("Galaxy map filters ready (hovered-system card, filter list, star colours)");
                me.apply_layout();
            }
            Err(reason) => me.log.info(&format!("Galaxy map filters not changed: {reason}")),
        }
        me
    }

    fn sync_log(&self) {
        let on = self
            .settings
            .lock()
            .map(|s| s.is_true("probe") || s.is_true("write_logs"))
            .unwrap_or(false);
        self.log.set_enabled(on);
    }

    /// Rebuild the dropdown from the switches that are on. Safe to call at any time.
    fn apply_layout(&self) {
        let Some(layout) = &self.layout else { return };
        let enabled: Vec<usize> = match self.settings.lock() {
            Ok(s) => (0..FILTERS.len()).filter(|&i| s.is_true(FILTERS[i].key)).collect(),
            Err(_) => return,
        };
        let enabled = &enabled[..enabled.len().min(SLOTS - VANILLA)];
        let count = VANILLA + enabled.len();
        let before = COUNT.load(Ordering::Acquire) as usize;

        let mut live = layout.image.clone();
        let extra = count > VANILLA;
        let patches = [sigs::POLL_PREV, sigs::POLL_PREV_TRIES, sigs::POLL_NEXT, sigs::POLL_NEXT_TRIES];
        if extra {
            // 1. Labels and slot map first (our own table), so nothing reads a half-built entry.
            for k in VANILLA..SLOTS {
                let text = enabled.get(k - VANILLA).map_or("", |&i| list_label(self.lang, &FILTERS[i]));
                layout.write_label(k, text);
                SLOT[k].store(enabled.get(k - VANILLA).map_or(0, |&i| i as u8 + 1), Ordering::Release);
            }
            // 2. The game's list. If its bytes are not what this mod knows, nothing else changes:
            // the extra slots stay locked and the cycle stays vanilla.
            if !layout.point_list(&mut live, true, count as u8, &self.log) {
                COUNT.store(VANILLA as u32, Ordering::Release);
                return;
            }
            COUNT.store(count as u32, Ordering::Release);
            DIRTY.store(true, Ordering::Release);
            // 3. The cycle over the longer list.
            for p in &patches {
                self.report(p, &mut live, true);
            }
        } else {
            // Vanilla: the detours go back to the game's answers first, then the code.
            COUNT.store(VANILLA as u32, Ordering::Release);
            DIRTY.store(true, Ordering::Release);
            for slot in &SLOT[VANILLA..] {
                slot.store(0, Ordering::Release);
            }
            for p in &patches {
                self.report(p, &mut live, false);
            }
            layout.point_list(&mut live, false, layout.orig_bound, &self.log);
        }
        if before != count {
            self.log.info(&format!("Galaxy map filter list: {count} entries ({} extra)", count - VANILLA));
        }
    }

    fn report(&self, patch: &CodePatch, image: &mut ProcessImage, apply: bool) {
        let r = patch.set(image, apply);
        if !r.outcome.ok() {
            self.log.info(&r.message);
        }
    }
}

/// Dropdown text of a filter in the game's language. A slot holds 31 bytes, so a longer
/// translation falls back to the English label rather than being cut mid-character.
fn list_label(lang: &str, filter: &Filter) -> &'static str {
    match super::common::menu_text::choice(lang, "GalaxyMapFilters", "list", filter.key) {
        Some(text) if text.len() < ID_LEN => text,
        _ => filter.label,
    }
}

impl Layout {
    fn write_label(&self, slot: usize, text: &str) {
        let mut bytes = [0u8; ID_LEN];
        let n = text.len().min(ID_LEN - 1);
        bytes[..n].copy_from_slice(&text.as_bytes()[..n]);
        // SAFETY: `table` is our own allocation of SLOTS * ID_LEN bytes; `slot < SLOTS`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), (self.table + slot * ID_LEN) as *mut u8, ID_LEN) };
    }

    /// Aim the list builder at our id table and set its loop bound, or put both back. Returns
    /// whether the list is now in the asked state. Both operands are read first: the displacement
    /// must be the vanilla one or ours, the bound the byte read at install or the one this mod
    /// last wrote. Anything else (another mod, a game change) is left alone and logged.
    fn point_list(&self, image: &mut ProcessImage, ours: bool, bound: u8, log: &Log) -> bool {
        let end = image.va(self.ids_rva) + 4 + 7;
        let Ok(our_disp) = i32::try_from(self.table as i64 - end as i64) else {
            log.info("Galaxy map filter list not changed: table out of reach");
            return false;
        };
        let disp_now = mem::read_bytes(image.va(self.ids_rva + 7), 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(i32::from_le_bytes);
        let bound_now = mem::read_bytes(image.va(self.bound_rva + 8), 1).map(|b| b[0]);
        let last = LAST_BOUND.load(Ordering::Acquire);
        let disp_ok = matches!(disp_now, Some(d) if d == self.orig_disp || d == our_disp);
        let bound_ok = matches!(bound_now, Some(b) if b == self.orig_bound || b == last);
        if !disp_ok || !bound_ok {
            log.info(&format!(
                "Galaxy map filter list not changed: found disp {disp_now:?} bound {bound_now:?}, expected disp {:#x} or {our_disp:#x}, bound {:#04x} or {last:#04x}",
                self.orig_disp, self.orig_bound
            ));
            return false;
        }
        let disp = if ours { our_disp } else { self.orig_disp };
        if let Err(e) = image.write(self.ids_rva + 7, &disp.to_le_bytes()) {
            log.info(&format!("Galaxy map filter list not changed: {e}"));
            return false;
        }
        if let Err(e) = image.write(self.bound_rva + 8, &[bound]) {
            log.info(&format!("Galaxy map filter list bound not changed: {e}"));
            // Put the displacement back so list and bound stay a vanilla pair.
            let _ = image.write(self.ids_rva + 7, &self.orig_disp.to_le_bytes());
            return false;
        }
        LAST_BOUND.store(bound, Ordering::Release);
        true
    }
}

fn install(image: &ProcessImage, log: &Arc<Log>, probe: bool) -> Result<(Vec<Hook>, Layout), String> {
    let panel = find(image, sigs::UPDATE_PANEL_UI)?;
    let apply = find(image, sigs::APPLY_COLOURS)?;
    let gate = find(image, sigs::FILTER_GATE)?;
    let poll = find(image, sigs::FILTER_POLL)?;
    let ids_rva = find(image, sigs::LIST_IDS)?;
    let bound_rva = find(image, sigs::LIST_BOUND)?;
    // The bound is a wildcard in the signature: require the vanilla 4 before anything is hooked.
    let orig_bound = image.read(bound_rva + 8, 1).ok_or("could not read the filter list bound")?[0];
    if orig_bound != VANILLA as u8 {
        return Err(format!("filter list bound is {orig_bound}, expected {VANILLA} (game update?)"));
    }
    let fns = Fns {
        setter: image.va(find(image, sigs::FILTER_SETTER)?),
        lookup: image.va(find(image, sigs::LOOKUP_STARS)?),
        classify: image.va(find(image, sigs::CLASSIFY_STAR)?),
        defaults: image.va(find(image, sigs::SET_DEFAULTS)?),
    };
    // The four vanilla loc ids, read before anything is patched.
    let disp_bytes = image.read(ids_rva + 7, 4).ok_or("could not read the filter list")?;
    let orig_disp = i32::from_le_bytes([disp_bytes[0], disp_bytes[1], disp_bytes[2], disp_bytes[3]]);
    let ids_va = (image.va(ids_rva) + 4 + 7).wrapping_add_signed(orig_disp as isize);
    let vanilla_ids = mem::read_bytes(ids_va, VANILLA * ID_LEN).ok_or("could not read the filter ids")?;
    let table = mem::alloc_exec_near(image.base(), SLOTS * ID_LEN).ok_or("no memory near the game for the filter ids")?;
    // SAFETY: fresh allocation of SLOTS * ID_LEN bytes; the copy is VANILLA * ID_LEN.
    unsafe { std::ptr::copy_nonoverlapping(vanilla_ids.as_ptr(), table as *mut u8, VANILLA * ID_LEN) };
    let layout = Layout { image: image.clone(), ids_rva, orig_disp, bound_rva, orig_bound, table };

    let _ = FNS.set(fns);
    let _ = PROBE.set(Arc::new(Probe {
        log: log.clone(),
        on: AtomicBool::new(probe),
        last_key: AtomicU64::new(0),
        lines: AtomicU32::new(0),
    }));

    let mut hooks = Vec::new();
    let targets: [(&'static str, usize, *const (), &AtomicUsize); 4] = [
        ("UpdatePanelUI", panel, panel_detour as *const (), &ORIG_PANEL),
        ("ApplyFilterColours", apply, apply_detour as *const (), &ORIG_APPLY),
        ("HasFilterScanner", gate, gate_detour as *const (), &ORIG_GATE),
        ("FilterPoll", poll, poll_detour as *const (), &ORIG_POLL),
    ];
    for (name, rva, detour, original) in targets {
        // SAFETY: each pattern is a function start and each detour declares the game's arguments.
        match unsafe { Hook::install(name, image.va(rva), detour, original) } {
            Ok(hook) => hooks.push(hook),
            Err(e) => {
                // Hooks already enabled must stay alive: dropping one frees its trampoline while
                // the prologue still jumps to it (retour only disables in a debug_assert). Their
                // detours pass through while COUNT stays at VANILLA, which it does on this path.
                std::mem::forget(hooks);
                return Err(e);
            }
        }
    }
    Ok((hooks, layout))
}

// ---- detours -------------------------------------------------------------------------------

unsafe extern "system" fn panel_detour(panel: u64) -> u64 {
    let original: PanelFn = std::mem::transmute(ORIG_PANEL.load(Ordering::Acquire));
    let ret = original(panel);
    if let Some(p) = PROBE.get() {
        if p.on.load(Ordering::Acquire) {
            p.record(panel as usize);
        }
    }
    ret
}

/// An extra slot is unlocked; everything else is the game's own answer.
unsafe extern "system" fn gate_detour(this: usize, index: u32) -> u8 {
    if index >= VANILLA as u32 && index < COUNT.load(Ordering::Acquire) {
        return 1;
    }
    let original: GateFn = std::mem::transmute(ORIG_GATE.load(Ordering::Acquire));
    original(this, index)
}

/// Once per map frame: remember the map data, and drop a selected extra filter whose slot is gone.
unsafe extern "system" fn poll_detour(data: usize) -> u64 {
    if DIRTY.swap(false, Ordering::AcqRel) {
        if let (Some(index), Some(f)) = (read_u32(data + DATA_FILTER), FNS.get()) {
            if index >= VANILLA as u32 {
                let setter: SetterFn = std::mem::transmute(f.setter);
                setter(data, 0);
            }
        }
    }
    let original: PollFn = std::mem::transmute(ORIG_POLL.load(Ordering::Acquire));
    original(data)
}

/// Bake the vanilla colours, then light the matches and dim the rest for an extra slot.
unsafe extern "system" fn apply_detour(data: usize, voxel: u64, index: u32) -> u64 {
    let original: ApplyFn = std::mem::transmute(ORIG_APPLY.load(Ordering::Acquire));
    if index < VANILLA as u32 || index >= COUNT.load(Ordering::Acquire) {
        return original(data, voxel, index);
    }
    let ret = original(data, voxel, 0);
    let filter = SLOT[index as usize % SLOTS].load(Ordering::Acquire);
    if filter != 0 {
        if let Some(f) = FILTERS.get(filter as usize - 1) {
            recolour(data, voxel, f);
        }
    }
    ret
}

#[repr(C, align(16))]
struct Attrs([u8; 0x700]);

fn matches(test: Test, attrs: &Attrs) -> bool {
    let at = |off: usize| attrs.0[off];
    match test {
        Test::GasGiant => at(ATTR_GAS_GIANT) != 0,
        Test::Abandoned => at(ATTR_ABANDONED) != 0,
        Test::Pirate => at(ATTR_PIRATE) != 0,
        Test::Colour(group) => {
            u32::from_le_bytes([at(ATTR_TYPE), at(ATTR_TYPE + 1), at(ATTR_TYPE + 2), at(ATTR_TYPE + 3)]) == group
        }
    }
}

/// Second pass over one voxel: classify every star again and scale the colour already baked on its node.
unsafe fn recolour(data: usize, voxel: u64, filter: &Filter) {
    let Some(f) = FNS.get() else { return };
    let mut coord = voxel;
    let lookup: LookupFn = std::mem::transmute(f.lookup);
    let blob = lookup(data + DATA_VOXEL_CACHE, &mut coord);
    if blob < mem::MIN_ADDR {
        return;
    }
    let (Some(count), Some(uas), Some(nodes)) =
        (read_u32(blob + BLOB_COUNT), read_ptr(blob + BLOB_UAS), read_ptr(blob + BLOB_NODES))
    else {
        return;
    };
    let count = count as usize;
    if count == 0 || count > 50_000 || !readable(uas, count * 8) || !readable(nodes, count * NODE_STRIDE) {
        return;
    }
    let defaults: DefaultsFn = std::mem::transmute(f.defaults);
    let classify: ClassifyFn = std::mem::transmute(f.classify);
    let debug = DEBUG_PAINT.load(Ordering::Relaxed);
    let mut attrs = Attrs([0; 0x700]);
    let mut hits = 0u32;
    for i in 0..count {
        let Some(ua) = read_u64(uas + i * 8) else { return };
        defaults(attrs.0.as_mut_ptr());
        classify(ua, attrs.0.as_mut_ptr());
        STATS.note(&attrs);
        let hit = matches(filter.test, &attrs);
        hits += u32::from(hit);
        let rgb = match (debug, hit) {
            (true, true) => DEBUG_ON,
            (true, false) => DEBUG_OFF,
            (false, true) => filter.colour,
            (false, false) => OFF_COLOUR,
        };
        let alpha = if hit || debug { 1.0f32 } else { OFF_ALPHA };
        let colour = nodes + i * NODE_STRIDE + NODE_COLOUR;
        for (channel, v) in rgb.iter().enumerate() {
            mem::write(colour + channel * 4, *v);
        }
        mem::write(colour + 12, alpha);
        if let Some(n) = read_u32(blob + BLOB_DIRTY) {
            mem::write(blob + BLOB_DIRTY, n.wrapping_add(1));
        }
    }
    if TRACE.fetch_add(1, Ordering::Relaxed) < TRACE_CALLS {
        if let Some(p) = PROBE.get() {
            p.log.info(&format!(
                "recolour {} voxel=0x{voxel:012X} stars={count} matched={hits} debug={debug}",
                filter.label
            ));
        }
    }
    STATS.voxel_done();
}

// ---- probe ---------------------------------------------------------------------------------

impl Probe {
    /// Read the card after the game filled it. Reads only; never panics.
    fn record(&self, panel: usize) {
        if panel < mem::MIN_ADDR || !readable(panel, PANEL_LEN) {
            return;
        }
        let (Some(ua), Some(star), Some(state)) =
            (read_u64(panel + UA), read_u32(panel + STAR_TYPE), read_u32(panel + STATE))
        else {
            return;
        };
        let flag = |off: usize| read_u8(panel + off).unwrap_or(0xFF);
        let (water, worm, corrupt, gas, visited) =
            (flag(WATER), flag(WORM), flag(CORRUPT), flag(GAS_GIANT), flag(VISITED));
        // One line per distinct system and flag set, so a re-draw of the same card stays quiet.
        let flags = u64::from(water)
            | u64::from(worm) << 8
            | u64::from(corrupt) << 16
            | u64::from(gas) << 24
            | u64::from(visited) << 32;
        let key = (ua ^ flags.rotate_left(17)).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        if self.last_key.swap(key, Ordering::AcqRel) == key {
            return;
        }
        if self.lines.fetch_add(1, Ordering::AcqRel) >= MAX_LINES {
            return;
        }
        let dist = read_f32(panel + DISTANCE).unwrap_or(-1.0);
        self.log.info(&format!(
            "card ua=0x{ua:016X} state={state} dist={dist:.0}LY star_type={star} race={} wealth={} conflict={} \
             water={water} worm={worm} corrupt={corrupt} gas_giant={gas} visited={visited} disc=0x{:X}",
            read_u32(panel + RACE).unwrap_or(u32::MAX),
            read_u32(panel + WEALTH).unwrap_or(u32::MAX),
            read_u32(panel + CONFLICT).unwrap_or(u32::MAX),
            read_u32(panel + DISCOVERY).unwrap_or(u32::MAX),
        ));
    }
}

impl GameMod for GalaxyMapFilters {
    fn name(&self) -> &'static str {
        "GalaxyMapFilters"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            _ if is_switch(key) => Some(json!(self.settings.lock().ok()?.is_true(key))),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        is_switch(key).then(|| self.settings.lock().ok()?.default(key).cloned()).flatten()
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        if !is_switch(key) {
            return Err(format!("unknown setting {key:?}"));
        }
        let on = value.as_bool().ok_or_else(|| "expected true or false".to_string())?;
        self.settings
            .lock()
            .map_err(|_| "settings lock poisoned")?
            .set(key, json!(on))?;
        let label = SPECS.iter().find(|s| s.key == key).map_or(key, |s| s.label);
        if key == "probe" {
            if let Some(p) = PROBE.get() {
                p.on.store(on, Ordering::Release);
            }
        }
        if key == "debug_paint" {
            DEBUG_PAINT.store(on, Ordering::Release);
            TRACE.store(0, Ordering::Release);
        }
        if key.starts_with("filter_") {
            if self.layout.is_none() {
                return Err(format!("{label} cannot apply: game code not found (see the log)"));
            }
            self.apply_layout();
        }
        self.sync_log();
        Ok(format!("{label} {}", if on { "on" } else { "off" }))
    }
}

fn is_switch(key: &str) -> bool {
    key == "probe" || key == "write_logs" || key == "debug_paint" || FILTERS.iter().any(|f| f.key == key)
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(GalaxyMapFilters::new(ctx))
}
