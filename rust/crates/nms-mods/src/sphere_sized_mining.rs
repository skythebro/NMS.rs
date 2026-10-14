//! Terrain Manipulator yield that grows with sphere size (ported from Python).
//!
//! Two hooks:
//!
//! - `ApplyTerrainEditStroke` (STROKE): remembers the edit byte of the stroke (sphere index in
//!   bits 4 to 6, edit mode in bits 0 to 2). It writes nothing.
//! - The stroke award (STROKE_AWARD, `0x1415949b0` in this build): every cap read of a Terrain
//!   Manipulator dig happens inside it. STROKE calls it directly and also hands it to the voxel
//!   edit as a functor, so it can run after STROKE has returned; hooking the award itself covers
//!   both. The detour writes the three per-dig caps (common, rare, silicate) as
//!   `max(1, round(vanilla * factor))`, calls the game, and puts the values it read back. Other
//!   users of the same globals (ship asteroid award, projectile and laser terrain hits) see vanilla
//!   caps, except one that reads them on another thread during the award itself (a window of
//!   microseconds, in which that reader sees the scaled caps).
//!
//! The factor comes from [`cap_factor`] for the sphere of the last stroke, relative to the
//! smallest one (`EditSizes[0]`):
//!
//! - `scaling` picks the curve: volume `r^3` (default), area `r^2`, linear `r`, or off.
//! - `yield_mult` multiplies the curve, but only for spheres bigger than the smallest. The small
//!   sphere (and the other edit modes) always keeps the vanilla caps, and the multiplied factor is
//!   never below 1, so a multiplier under 1 can lower a big sphere towards vanilla, not below it.
//! - `max_factor` is the upper limit.
//! - `enabled` off: the award runs with the vanilla caps.
//!
//! Every setting applies live: the caps are vanilla outside the award, and each award reads the
//! current settings.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{paths, Code, GameMod, Kind, Pattern, Range, SettingSpec, Settings};
use nms_sdk::sigs::sphere_sized_mining;
use serde_json::{json, Value};

type StrokeFn = unsafe extern "system" fn(*mut c_void, u64, *mut c_void) -> i64;
/// rcx = captured edit state, rdx = removed-voxel counts; r8 and r9 are not used by the game
/// function but are passed through unchanged (no stack arguments).
type AwardFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u64, u64) -> u64;

const SIZE_READ: [u8; 13] = [
    0x48, 0xC1, 0xE8, 0x04, 0x83, 0xE0, 0x07, 0xF3, 0x45, 0x0F, 0x10, 0x84, 0x81,
];
const SIZE_READ_AT: usize = 0xC5;
const CMOVGE: [u8; 4] = [0x44, 0x0F, 0x4D, 0x05];
const CAP_OFFSETS: [(usize, &str); 3] = [(0xE7, "common"), (0x135, "rare"), (0x1CD, "silicate")];
const OTHER_SIZES_MODE: u64 = 7;
/// `cmovge r8d, [rip + common cap]` inside the stroke award (`0x141594c01` in this build).
const STROKE_AWARD_COMMON_AT: usize = 0x251;

const MULT_MIN: f32 = 0.25;
const MULT_MAX: f32 = 4.0;
const MULT_STEP: f32 = 0.25;
const MULT_DEFAULT: f32 = 1.0;
const MAX_FACTOR_MIN: i32 = 1;
const MAX_FACTOR_MAX: i32 = 512;
const MAX_FACTOR_DEFAULT: i32 = 64;

static STROKE_ORIG: AtomicUsize = AtomicUsize::new(0);
static AWARD_ORIG: AtomicUsize = AtomicUsize::new(0);
static MINING: OnceLock<Arc<Mutex<MiningRuntime>>> = OnceLock::new();

/// How the caps grow with the sphere radius ratio `r` (size / smallest size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scaling {
    Volume,
    Area,
    Linear,
    Off,
}

/// `(key, label)` in the order the settings window shows them; index 0 is the default.
const SCALINGS: &[(&str, &str)] = &[
    ("volume", "Volume"),
    ("area", "Area"),
    ("linear", "Linear"),
    ("off", "Off"),
];

impl Scaling {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "volume" => Some(Self::Volume),
            "area" => Some(Self::Area),
            "linear" => Some(Self::Linear),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::Area => "area",
            Self::Linear => "linear",
            Self::Off => "off",
        }
    }
}

/// The player's settings as the detour uses them.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tuning {
    enabled: bool,
    scaling: Scaling,
    yield_mult: f64,
    max_factor: i32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            enabled: true,
            scaling: Scaling::Volume,
            yield_mult: f64::from(MULT_DEFAULT),
            max_factor: MAX_FACTOR_DEFAULT,
        }
    }
}

impl Tuning {
    /// Read from the settings file; anything missing, mistyped or out of range uses the default
    /// or the nearest limit, so a hand-edited file cannot push the caps somewhere odd.
    fn from_settings(s: &Settings) -> Self {
        let d = Self::default();
        let scaling = s
            .get("scaling")
            .and_then(Value::as_str)
            .and_then(|k| Scaling::from_key(k.trim()))
            .unwrap_or(d.scaling);
        let yield_mult = s
            .get("yield_mult")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .map(|v| v.clamp(f64::from(MULT_MIN), f64::from(MULT_MAX)))
            .unwrap_or(d.yield_mult);
        let max_factor = s
            .get("max_factor")
            .and_then(Value::as_i64)
            .map(|v| v.clamp(i64::from(MAX_FACTOR_MIN), i64::from(MAX_FACTOR_MAX)) as i32)
            .unwrap_or(d.max_factor);
        Self {
            enabled: s.is_not_false("enabled"),
            scaling,
            yield_mult,
            max_factor,
        }
    }

    /// The cap factor for a sphere `ratio` times the smallest; 1 when scaling is switched off.
    fn factor(&self, ratio: f64) -> f64 {
        if !self.enabled {
            return 1.0;
        }
        cap_factor(ratio, self.scaling, self.yield_mult, self.max_factor)
    }
}

/// Cap factor for a sphere `ratio` times the smallest one.
///
/// The small sphere (`ratio <= 1`, or a bad ratio) and `Scaling::Off` give exactly 1. Bigger
/// spheres give `curve(ratio) * mult`, kept between 1 and `max_factor`.
fn cap_factor(ratio: f64, scaling: Scaling, mult: f64, max_factor: i32) -> f64 {
    if !ratio.is_finite() || ratio <= 1.0 {
        return 1.0;
    }
    let curve = match scaling {
        Scaling::Volume => ratio * ratio * ratio,
        Scaling::Area => ratio * ratio,
        Scaling::Linear => ratio,
        Scaling::Off => return 1.0,
    };
    let f = curve * mult;
    if !f.is_finite() {
        return 1.0;
    }
    f.clamp(1.0, f64::from(max_factor.max(1)))
}

/// One cap written to the game: `max(1, round(vanilla * factor))`, within `i32`.
fn scaled_cap(vanilla: i32, factor: f64) -> i32 {
    let scaled = (f64::from(vanilla) * factor).round();
    if !scaled.is_finite() {
        return vanilla.max(1);
    }
    scaled.clamp(1.0, f64::from(i32::MAX)) as i32
}

struct MiningAddrs {
    stroke_rva: usize,
    stroke_award_rva: usize,
    sizes: usize,
    caps: [usize; 3],
}

struct MiningRuntime {
    addrs: MiningAddrs,
    vanilla: Option<[i32; 3]>,
    /// Factor of the last award that was logged, so the log gets one line per change.
    logged_factor: f64,
    /// Edit byte of the last stroke; the award (direct or deferred) uses its sphere.
    last_edit: Option<u64>,
    /// True while scaled caps are in the game, between `before_award` and `after_award`.
    scaled: bool,
    tuning: Tuning,
    active: bool,
    log: Arc<Log>,
    status: Arc<Mutex<String>>,
}

const SPECS: &[SettingSpec] = &[
    SettingSpec {
        key: "status",
        label: "Status",
        kind: Kind::Status,
    },
    SettingSpec {
        key: "enabled",
        label: "Scale mining yield",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "scaling",
        label: "Yield grows with",
        kind: Kind::Enum,
    },
    SettingSpec {
        key: "yield_mult",
        label: "Yield multiplier",
        kind: Kind::Float,
    },
    SettingSpec {
        key: "max_factor",
        label: "Largest factor",
        kind: Kind::Int,
    },
    SettingSpec {
        key: "write_logs",
        label: "Write log",
        kind: Kind::Bool,
    },
];

fn defaults() -> Value {
    json!({
        "write_logs": false,
        "enabled": true,
        "scaling": SCALINGS[0].0,
        "yield_mult": MULT_DEFAULT,
        "max_factor": MAX_FACTOR_DEFAULT,
    })
}

pub struct SphereSizedMining {
    settings: Mutex<Settings>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    /// Keep the hooks (stroke, stroke award) installed for the process lifetime.
    #[expect(dead_code)]
    hooks: Option<(Hook, Hook)>,
}

impl SphereSizedMining {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "SphereSizedMining");
        let settings = Settings::load(&dir.join("sphere_sized_mining.json"), defaults());
        let write_logs = settings.is_not_false("write_logs");
        let tuning = Tuning::from_settings(&settings);
        let log = Arc::new(Log::open(&dir.join("sphere_sized_mining.log"), write_logs));
        let status = Arc::new(Mutex::new("Off, see log".to_string()));

        // The runtime is in place before either hook is enabled; until both are, the detours
        // find `active == false` and only pass through.
        let hooks = match locate(&ctx.image)
            .and_then(|addrs| prepare_hooks(&ctx.image, log.clone(), addrs, status.clone(), tuning))
        {
            Ok((stroke, award, runtime)) => {
                let state = MINING.get_or_init(|| Arc::new(Mutex::new(runtime)));
                match stroke.enable().and_then(|()| award.enable()) {
                    Ok(()) => {
                        if let Ok(mut g) = state.lock() {
                            g.active = true;
                        }
                        if let Ok(mut s) = status.lock() {
                            *s = "On".to_string();
                        }
                        log.info("Mining yield grows with the Terrain Manipulator sphere");
                        log.info(&describe(&tuning));
                    }
                    Err(e) => {
                        log.info(&format!(
                            "Mining yield is vanilla: could not enable terrain hooks ({e})"
                        ));
                    }
                }
                // Kept either way: a hook that did enable must stay alive while it is patched in.
                Some((stroke, award))
            }
            Err(reason) => {
                log.info(&format!("Mining yield is vanilla: {reason}"));
                None
            }
        };

        Self {
            settings: Mutex::new(settings),
            status,
            log,
            hooks,
        }
    }

    /// Hand the new settings to the detour's state; the next award uses them.
    fn apply_tuning(&self, tuning: Tuning) {
        let Some(state) = MINING.get() else {
            return;
        };
        // Not a detour. The award detour holds this lock while the game's award runs (no call
        // back into this module), so the wait is short.
        if let Ok(mut g) = state.lock() {
            g.tuning = tuning;
        }
    }
}

/// One log line with the current settings.
fn describe(t: &Tuning) -> String {
    if !t.enabled || t.scaling == Scaling::Off {
        return "Scaling off: vanilla caps for every sphere".to_string();
    }
    format!(
        "Scaling by {}, multiplier {:.2}, largest factor {}",
        t.scaling.key(),
        t.yield_mult,
        t.max_factor
    )
}

fn locate(image: &ProcessImage) -> Result<MiningAddrs, String> {
    let stroke_pat = Pattern::parse(sphere_sized_mining::STROKE).map_err(|e| e.to_string())?;
    let award_pat = Pattern::parse(sphere_sized_mining::AWARD).map_err(|e| e.to_string())?;
    let Some(stroke_rva) = image.find_unique(&stroke_pat).unique() else {
        return Err("the game code it relies on was not found (game update?)".into());
    };
    let Some(award_rva) = image.find_unique(&award_pat).unique() else {
        return Err("the game code it relies on was not found (game update?)".into());
    };
    let stroke_award_pat =
        Pattern::parse(sphere_sized_mining::STROKE_AWARD).map_err(|e| e.to_string())?;
    let Some(stroke_award_rva) = image.find_unique(&stroke_award_pat).unique() else {
        return Err("the game code it relies on was not found (game update?)".into());
    };

    let read_rva = stroke_rva + SIZE_READ_AT;
    let read_bytes = image
        .read(read_rva, SIZE_READ.len() + 4)
        .ok_or_else(|| "the sphere size lookup is not where it was (game update?)".to_string())?;
    if read_bytes[..SIZE_READ.len()] != SIZE_READ {
        return Err("the sphere size lookup is not where it was (game update?)".into());
    }
    let disp = i32_from_le(&read_bytes[SIZE_READ.len()..])?;
    let sizes = image.base().wrapping_add(disp as usize);

    let mut caps = [0usize; 3];
    for (i, (offset, name)) in CAP_OFFSETS.iter().enumerate() {
        let at_rva = award_rva + offset;
        let at_bytes = image
            .read(at_rva, CMOVGE.len())
            .ok_or_else(|| cap_err(name))?;
        if at_bytes != CMOVGE {
            return Err(cap_err(name));
        }
        let at_va = image.va(at_rva);
        let disp_bytes = image.read(at_rva + 4, 4).ok_or_else(|| cap_err(name))?;
        let cap_disp = i32_from_le(&disp_bytes)?;
        caps[i] = at_va.wrapping_add(8).wrapping_add(cap_disp as usize);
    }

    // The stroke award must read the same common cap inline, or it is not the award we think.
    let inline_rva = stroke_award_rva + STROKE_AWARD_COMMON_AT;
    let inline_err = || "the stroke award does not read the caps where it did (game update?)".to_string();
    let inline_bytes = image.read(inline_rva, CMOVGE.len() + 4).ok_or_else(inline_err)?;
    if inline_bytes[..CMOVGE.len()] != CMOVGE {
        return Err(inline_err());
    }
    let inline_disp = i32_from_le(&inline_bytes[CMOVGE.len()..])?;
    if image.va(inline_rva).wrapping_add(8).wrapping_add(inline_disp as usize) != caps[0] {
        return Err(inline_err());
    }

    Ok(MiningAddrs {
        stroke_rva,
        stroke_award_rva,
        sizes,
        caps,
    })
}

fn cap_err(name: &str) -> String {
    format!("the {name} cap is not read where it was (game update?)")
}

fn i32_from_le(bytes: &[u8]) -> Result<i32, String> {
    let b: [u8; 4] = bytes.try_into().map_err(|_| "short read".to_string())?;
    Ok(i32::from_le_bytes(b))
}

fn prepare_hooks(
    image: &ProcessImage,
    log: Arc<Log>,
    addrs: MiningAddrs,
    status: Arc<Mutex<String>>,
    tuning: Tuning,
) -> Result<(Hook, Hook, MiningRuntime), String> {
    // SAFETY: signature is the function start; stroke_detour declares all three arguments.
    let stroke = unsafe {
        Hook::new(
            "ApplyTerrainEditStroke",
            image.va(addrs.stroke_rva),
            stroke_detour as *const (),
        )
    }
    .map_err(|e| e.to_string())?;
    // SAFETY: signature is the function start; the game function takes rcx and rdx only (no
    // stack arguments), award_detour declares all four register arguments and passes them on.
    let award = unsafe {
        Hook::new(
            "TerrainStrokeAward",
            image.va(addrs.stroke_award_rva),
            award_detour as *const (),
        )
    }
    .map_err(|e| e.to_string())?;
    STROKE_ORIG.store(stroke.original() as usize, Ordering::Release);
    AWARD_ORIG.store(award.original() as usize, Ordering::Release);
    let runtime = MiningRuntime {
        addrs,
        vanilla: None,
        logged_factor: 1.0,
        last_edit: None,
        scaled: false,
        tuning,
        // Set once both hooks are enabled.
        active: false,
        log: log.clone(),
        status,
    };
    Ok((stroke, award, runtime))
}

impl MiningRuntime {
    fn on_stroke(&mut self, edit: u64) {
        if self.active {
            self.last_edit = Some(edit & 0xFF);
        }
    }

    /// Before the game's award: write the caps for the last stroke's sphere. Nothing is written
    /// for factor 1 (small sphere, other edit modes, scaling off).
    fn before_award(&mut self) {
        if !self.active || self.scaled {
            return;
        }
        let Some(edit_byte) = self.last_edit else {
            return;
        };
        let ratio = if edit_byte & 7 == OTHER_SIZES_MODE {
            1.0
        } else {
            match self.sphere_ratio(edit_byte) {
                Ok(r) => r,
                Err(()) => {
                    self.fail();
                    return;
                }
            }
        };
        let factor = self.tuning.factor(ratio);
        if self.write_scaled(factor, ratio).is_err() {
            self.fail();
        }
    }

    /// After the game's award: put the vanilla caps back.
    fn after_award(&mut self) {
        if !self.scaled {
            return;
        }
        if self.write_vanilla().is_err() {
            self.fail();
        }
    }

    /// Chosen sphere size divided by the smallest one (1 when it is not bigger).
    fn sphere_ratio(&self, edit_byte: u64) -> Result<f64, ()> {
        let index = (edit_byte >> 4) & 7;
        let smallest = read_f32(self.addrs.sizes)?;
        if smallest <= 0.0 {
            return Err(());
        }
        let byte_off = 4usize.checked_mul(index as usize).ok_or(())?;
        let size_addr = self.addrs.sizes.checked_add(byte_off).ok_or(())?;
        let size = read_f32(size_addr)?;
        if size > smallest {
            Ok(f64::from(size / smallest))
        } else {
            Ok(1.0)
        }
    }

    /// Write `vanilla * factor` into the caps (factor above 1 only) and mark them scaled.
    fn write_scaled(&mut self, factor: f64, ratio: f64) -> Result<(), ()> {
        // Outside an award the caps hold the game's own values, so read them fresh each time
        // (respects anything else that changed them) and put exactly these back afterwards.
        let vanilla = self.read_caps()?;
        if self.vanilla != Some(vanilla) {
            self.log.info(&format!(
                "Vanilla caps per dig: {{'common': {}, 'rare': {}, 'silicate': {}}}",
                vanilla[0], vanilla[1], vanilla[2]
            ));
        }
        self.vanilla = Some(vanilla);
        let caps = vanilla.map(|v| scaled_cap(v, factor));
        if (factor - self.logged_factor).abs() >= f64::EPSILON {
            self.logged_factor = factor;
            self.log.info(&format!(
                "Sphere x{ratio:.2} of the smallest, factor {factor:.2}: caps per dig {{'common': {}, 'rare': {}, 'silicate': {}}}",
                caps[0], caps[1], caps[2]
            ));
        }
        if caps == vanilla {
            return Ok(());
        }
        self.scaled = true;
        for (cap, value) in self.addrs.caps.iter().zip(caps) {
            write_i32(*cap, value)?;
        }
        Ok(())
    }

    fn write_vanilla(&mut self) -> Result<(), ()> {
        let vanilla = self.vanilla.ok_or(())?;
        for (cap, value) in self.addrs.caps.iter().zip(vanilla) {
            write_i32(*cap, value)?;
        }
        self.scaled = false;
        Ok(())
    }

    fn read_caps(&self) -> Result<[i32; 3], ()> {
        let mut out = [0i32; 3];
        for (i, cap) in self.addrs.caps.iter().enumerate() {
            out[i] = read_i32(*cap)?;
        }
        Ok(out)
    }

    fn fail(&mut self) {
        self.log.info("Mining yield: turned off");
        if self.scaled {
            let _ = self.write_vanilla();
        }
        self.active = false;
        if let Ok(mut s) = self.status.lock() {
            *s = "Stopped, see log".to_string();
        }
    }
}

fn read_f32(addr: usize) -> Result<f32, ()> {
    if addr == 0 {
        return Err(());
    }
    // SAFETY: game memory at addresses found from its own code.
    let v = unsafe { std::ptr::read_unaligned(addr as *const f32) };
    if v.is_finite() {
        Ok(v)
    } else {
        Err(())
    }
}

fn read_i32(addr: usize) -> Result<i32, ()> {
    if addr == 0 {
        return Err(());
    }
    // SAFETY: cap globals in the running game.
    Ok(unsafe { std::ptr::read_unaligned(addr as *const i32) })
}

fn write_i32(addr: usize, value: i32) -> Result<(), ()> {
    if addr == 0 {
        return Err(());
    }
    // SAFETY: cap globals in the running game.
    unsafe { std::ptr::write_unaligned(addr as *mut i32, value) };
    Ok(())
}

unsafe extern "system" fn stroke_detour(this: *mut c_void, edit: u64, impact: *mut c_void) -> i64 {
    if let Some(state) = MINING.get() {
        // try_lock: if an award or a settings change holds the lock, this stroke keeps the
        // previous sphere (the next stroke corrects it).
        if let Ok(mut g) = state.try_lock() {
            g.on_stroke(edit);
        }
    }
    // SAFETY: set from the hook's trampoline before the hook is enabled; same signature.
    let original: StrokeFn = std::mem::transmute(STROKE_ORIG.load(Ordering::Acquire));
    original(this, edit, impact)
}

/// Scaled caps exist only while the game's award runs. The lock is held across the call so a
/// settings change cannot swap the state mid-award; a second award that arrives meanwhile (another
/// thread, or re-entry) fails `try_lock` and runs with whatever caps are in place.
unsafe extern "system" fn award_detour(state_ptr: *mut c_void, counts: *mut c_void, r8: u64, r9: u64) -> u64 {
    let mut guard = MINING.get().and_then(|s| s.try_lock().ok());
    if let Some(g) = guard.as_mut() {
        g.before_award();
    }
    // SAFETY: set from the hook's trampoline before the hook is enabled; same register arguments.
    let original: AwardFn = std::mem::transmute(AWARD_ORIG.load(Ordering::Acquire));
    let ret = original(state_ptr, counts, r8, r9);
    if let Some(g) = guard.as_mut() {
        g.after_award();
    }
    ret
}

impl GameMod for SphereSizedMining {
    fn name(&self) -> &'static str {
        "SphereSizedMining"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn range(&self, key: &str) -> Option<Range> {
        match key {
            "scaling" => Some(Range::Enum {
                choices: SCALINGS,
                default: 0,
            }),
            "yield_mult" => Some(Range::Float {
                min: MULT_MIN,
                max: MULT_MAX,
                step: MULT_STEP,
                default: MULT_DEFAULT,
            }),
            "max_factor" => Some(Range::Int {
                min: MAX_FACTOR_MIN,
                max: MAX_FACTOR_MAX,
                step: 1,
                default: MAX_FACTOR_DEFAULT,
            }),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<Value> {
        if key == "status" {
            return Some(json!(self.status.lock().ok()?.clone()));
        }
        let s = self.settings.lock().ok()?;
        let t = Tuning::from_settings(&s);
        match key {
            "write_logs" => Some(json!(s.is_not_false("write_logs"))),
            "enabled" => Some(json!(t.enabled)),
            "scaling" => Some(json!(t.scaling.key())),
            "yield_mult" => Some(json!(t.yield_mult)),
            "max_factor" => Some(json!(t.max_factor)),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "status" => None,
            _ => self.settings.lock().ok()?.default(key).cloned(),
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        let (stored, line) = match key {
            "write_logs" | "enabled" => {
                let on = value
                    .as_bool()
                    .ok_or_else(|| "expected true or false".to_string())?;
                let what = if key == "write_logs" { "Write log" } else { "Scale mining yield" };
                (json!(on), format!("{what} {}", if on { "on" } else { "off" }))
            }
            "scaling" => {
                let choice = value
                    .as_str()
                    .and_then(|k| Scaling::from_key(k.trim()))
                    .ok_or_else(|| "expected volume, area, linear or off".to_string())?;
                (json!(choice.key()), format!("Yield grows with {}", choice.key()))
            }
            "yield_mult" => {
                let v = value
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| "expected a number".to_string())?;
                if v < f64::from(MULT_MIN) || v > f64::from(MULT_MAX) {
                    return Err(format!("yield_mult must be {MULT_MIN} to {MULT_MAX}"));
                }
                (json!(v), format!("Yield multiplier {v:.2}"))
            }
            "max_factor" => {
                let v = value
                    .as_i64()
                    .ok_or_else(|| "expected a whole number".to_string())?;
                if v < i64::from(MAX_FACTOR_MIN) || v > i64::from(MAX_FACTOR_MAX) {
                    return Err(format!(
                        "max_factor must be {MAX_FACTOR_MIN} to {MAX_FACTOR_MAX}"
                    ));
                }
                (json!(v), format!("Largest factor {v}"))
            }
            _ => return Err(format!("unknown setting {key:?}")),
        };
        let tuning = {
            let mut s = self
                .settings
                .lock()
                .map_err(|_| "settings lock poisoned".to_string())?;
            s.set(key, stored)?;
            Tuning::from_settings(&s)
        };
        if key == "write_logs" {
            self.log.set_enabled(value.as_bool().unwrap_or(false));
        } else {
            self.log.info(&describe(&tuning));
            self.apply_tuning(tuning);
        }
        Ok(line)
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(SphereSizedMining::new(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn vanilla_sizes_match_the_python_mod() {
        // 0.8 / 1.6 / 3.2: ratios 1, 2 and 4.
        assert!(close(cap_factor(1.0, Scaling::Volume, 1.0, 64), 1.0));
        assert!(close(cap_factor(2.0, Scaling::Volume, 1.0, 64), 8.0));
        assert!(close(cap_factor(4.0, Scaling::Volume, 1.0, 64), 64.0));
        assert_eq!(scaled_cap(8, 8.0), 64);
        assert_eq!(scaled_cap(2, 64.0), 128);
    }

    #[test]
    fn curves() {
        assert!(close(cap_factor(4.0, Scaling::Area, 1.0, 512), 16.0));
        assert!(close(cap_factor(4.0, Scaling::Linear, 1.0, 512), 4.0));
        assert!(close(cap_factor(4.0, Scaling::Off, 4.0, 512), 1.0));
    }

    #[test]
    fn small_sphere_stays_vanilla() {
        for s in [Scaling::Volume, Scaling::Area, Scaling::Linear, Scaling::Off] {
            for mult in [0.25, 1.0, 4.0] {
                assert!(close(cap_factor(1.0, s, mult, 512), 1.0));
                assert!(close(cap_factor(0.5, s, mult, 512), 1.0));
                assert!(close(cap_factor(f64::NAN, s, mult, 512), 1.0));
                assert!(close(cap_factor(f64::INFINITY, s, mult, 512), 1.0));
            }
        }
    }

    #[test]
    fn multiplier_and_limits() {
        assert!(close(cap_factor(2.0, Scaling::Volume, 2.0, 512), 16.0));
        assert!(close(cap_factor(2.0, Scaling::Volume, 0.25, 512), 2.0));
        // Never below vanilla.
        assert!(close(cap_factor(2.0, Scaling::Linear, 0.25, 512), 1.0));
        // Upper limit.
        assert!(close(cap_factor(4.0, Scaling::Volume, 4.0, 64), 64.0));
        assert!(close(cap_factor(8.0, Scaling::Volume, 1.0, 512), 512.0));
        assert!(close(cap_factor(4.0, Scaling::Volume, 1.0, 1), 1.0));
        // A bad limit behaves as 1.
        assert!(close(cap_factor(4.0, Scaling::Volume, 1.0, 0), 1.0));
    }

    #[test]
    fn caps_round_and_stay_at_least_one() {
        assert_eq!(scaled_cap(2, 1.25), 3); // 2.5 rounds away from zero
        assert_eq!(scaled_cap(8, 1.0), 8);
        assert_eq!(scaled_cap(0, 64.0), 1);
        assert_eq!(scaled_cap(-3, 2.0), 1);
        assert_eq!(scaled_cap(i32::MAX, 512.0), i32::MAX);
        assert_eq!(scaled_cap(8, f64::NAN), 8);
    }

    #[test]
    fn tuning_switch_and_defaults() {
        let t = Tuning::default();
        assert!(close(t.factor(4.0), 64.0));
        let off = Tuning { enabled: false, ..t };
        assert!(close(off.factor(4.0), 1.0));
        let area = Tuning { scaling: Scaling::Area, yield_mult: 2.0, ..t };
        assert!(close(area.factor(2.0), 8.0));
    }

    #[test]
    fn tuning_from_bad_file_uses_limits_and_defaults() {
        let dir = std::env::temp_dir().join(format!("ssm_test_{}", std::process::id()));
        let path = dir.join("sphere_sized_mining.json");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            &path,
            r#"{"enabled": false, "scaling": "cube", "yield_mult": 99, "max_factor": -5}"#,
        )
        .unwrap();
        let s = Settings::load(&path, defaults());
        let t = Tuning::from_settings(&s);
        assert!(!t.enabled);
        assert_eq!(t.scaling, Scaling::Volume);
        assert!(close(t.yield_mult, 4.0));
        assert_eq!(t.max_factor, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn defaults_match_ranges() {
        let d = defaults();
        assert_eq!(d["scaling"], json!(SCALINGS[0].0));
        assert_eq!(d["max_factor"], json!(MAX_FACTOR_DEFAULT));
        assert!(close(d["yield_mult"].as_f64().unwrap(), f64::from(MULT_DEFAULT)));
        assert_eq!(Tuning::from_settings_default(), Tuning::default());
        for (k, _) in SCALINGS {
            assert_eq!(Scaling::from_key(k).map(Scaling::key), Some(*k));
        }
    }

    impl Tuning {
        fn from_settings_default() -> Self {
            let dir = std::env::temp_dir().join(format!("ssm_def_{}", std::process::id()));
            let s = Settings::load(&dir.join("none.json"), defaults());
            let t = Self::from_settings(&s);
            let _ = std::fs::remove_dir_all(&dir);
            t
        }
    }
}
