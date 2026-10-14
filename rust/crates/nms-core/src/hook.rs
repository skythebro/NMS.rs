//! Function hooks (detours) on game code.
//!
//! Hard rule carried over from pyMHF: the detour's `extern "system" fn` type must declare every
//! argument the game function takes, stack arguments included, with floats as `f32`/`f64` in their
//! real positions (x64 passes them in XMM registers). Too few arguments crashed the game before.
//! Call the original through [`Hook::original`] with the same type.

use retour::RawDetour;

pub struct Hook {
    name: &'static str,
    detour: RawDetour,
}

// SAFETY: RawDetour only holds addresses and the trampoline it owns; enable/disable are guarded by
// the callers (one toggle at a time from the settings pipe).
unsafe impl Send for Hook {}
unsafe impl Sync for Hook {}

impl Hook {
    /// Prepare a detour from `target` (absolute address in NMS.exe) to `detour`. Not enabled yet.
    ///
    /// # Safety
    /// `target` must be the first byte of a function whose signature matches `detour` exactly.
    pub unsafe fn new(name: &'static str, target: usize, detour: *const ()) -> Result<Self, String> {
        let detour = RawDetour::new(target as *const (), detour).map_err(|e| format!("{name}: {e}"))?;
        Ok(Self { name, detour })
    }

    /// Create the detour, publish its trampoline to `original`, then enable it. The detour reads
    /// `original` to call the game function, so it must be set before the first call can arrive.
    ///
    /// # Safety
    /// As [`Hook::new`].
    pub unsafe fn install(
        name: &'static str,
        target: usize,
        detour: *const (),
        original: &std::sync::atomic::AtomicUsize,
    ) -> Result<Self, String> {
        let hook = Self::new(name, target, detour)?;
        original.store(hook.original() as usize, std::sync::atomic::Ordering::Release);
        hook.enable()?;
        Ok(hook)
    }

    /// Unlike MinHook (pyMHF), retour does not suspend other threads while it rewrites the prologue.
    /// Enable once at install; for a live toggle keep the hook on and gate the detour body instead.
    pub fn enable(&self) -> Result<(), String> {
        // SAFETY: the detour was created over a matching function in `new`.
        unsafe { self.detour.enable() }.map_err(|e| format!("{}: {e}", self.name))
    }

    pub fn disable(&self) -> Result<(), String> {
        // SAFETY: as above.
        unsafe { self.detour.disable() }.map_err(|e| format!("{}: {e}", self.name))
    }

    pub fn is_enabled(&self) -> bool {
        self.detour.is_enabled()
    }

    /// The trampoline that runs the original function. Transmute it to the detour's fn type.
    pub fn original(&self) -> *const () {
        self.detour.trampoline() as *const ()
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}
