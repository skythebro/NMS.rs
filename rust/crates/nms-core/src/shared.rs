//! Shared hooks: one detour per game function, with an ordered list of mod callbacks.
//!
//! Two retour detours on one prologue are not allowed, and pyMHF let several mods hook the same
//! function. [`shared_hook!`] declares a module per game function; each mod calls `subscribe`
//! with a `before` and/or `after` callback. Callbacks run in subscription order; a `before` may
//! change the arguments the next callback and the original see. The callback table is lock-free so
//! the detour never blocks or allocates on the game thread.

/// Most callbacks one shared hook accepts.
pub const MAX_SUBSCRIBERS: usize = 8;

/// Declare a shared hook module. The signature follows the [`crate::hook`] rule: every argument,
/// stack ones included, floats in their real positions. The return type must be `Copy`.
///
/// ```ignore
/// nms_core::shared_hook! {
///     pub mod inventory_sort = "InventorySort": fn(inventory: u64, order: i32) -> u64;
/// }
/// inventory_sort::subscribe(&image, sigs::SORT, Some(before), None)?;
/// ```
#[macro_export]
macro_rules! shared_hook {
    ($(#[$meta:meta])* $vis:vis mod $name:ident = $label:literal: fn($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty;) => {
        $(#[$meta])*
        $vis mod $name {
            use std::sync::atomic::{AtomicUsize, Ordering};
            use std::sync::{Mutex, OnceLock};
            use $crate::shared::MAX_SUBSCRIBERS;

            #[derive(Clone, Copy, Debug)]
            pub struct Args {
                $(pub $arg: $ty,)*
            }

            /// Runs before the original. May change the arguments.
            pub type Before = fn(&mut Args);
            /// Runs after the original with the arguments it received and its return value.
            pub type After = fn(&Args, $ret);

            type Raw = unsafe extern "system" fn($($ty),*) -> $ret;

            static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
            static HOOK: OnceLock<$crate::hook::Hook> = OnceLock::new();
            static INSTALL: Mutex<()> = Mutex::new(());
            static COUNT: AtomicUsize = AtomicUsize::new(0);
            static BEFORE: [AtomicUsize; MAX_SUBSCRIBERS] = [const { AtomicUsize::new(0) }; MAX_SUBSCRIBERS];
            static AFTER: [AtomicUsize; MAX_SUBSCRIBERS] = [const { AtomicUsize::new(0) }; MAX_SUBSCRIBERS];

            /// Find the function by its unique pattern, hook it on first use, add the callbacks.
            pub fn subscribe(
                image: &$crate::process::ProcessImage,
                pattern: &str,
                before: Option<Before>,
                after: Option<After>,
            ) -> Result<(), String> {
                use $crate::Code as _;
                if HOOK.get().is_some() {
                    return add(before, after);
                }
                let pat = $crate::Pattern::parse(pattern).map_err(|e| format!("{}: {e:?}", $label))?;
                let rva = image
                    .find_unique(&pat)
                    .unique()
                    .ok_or_else(|| format!("{} not found (game update?)", $label))?;
                // SAFETY: the pattern is unique and points at the function this signature declares.
                unsafe { subscribe_at(image.va(rva), before, after) }
            }

            /// As [`subscribe`] with the function's absolute address.
            ///
            /// # Safety
            /// `target` must be the first byte of a function with exactly this signature.
            pub unsafe fn subscribe_at(target: usize, before: Option<Before>, after: Option<After>) -> Result<(), String> {
                {
                    let _guard = INSTALL.lock().map_err(|_| format!("{}: install lock poisoned", $label))?;
                    if HOOK.get().is_none() {
                        let hook = $crate::hook::Hook::install($label, target, detour as *const (), &ORIGINAL)?;
                        let _ = HOOK.set(hook);
                    }
                }
                add(before, after)
            }

            /// Whether the game function is hooked.
            pub fn installed() -> bool {
                HOOK.get().is_some()
            }
            fn add(before: Option<Before>, after: Option<After>) -> Result<(), String> {
                let _guard = INSTALL.lock().map_err(|_| format!("{}: install lock poisoned", $label))?;
                let i = COUNT.load(Ordering::Acquire);
                if i >= MAX_SUBSCRIBERS {
                    return Err(format!("{}: more than {MAX_SUBSCRIBERS} subscribers", $label));
                }
                BEFORE[i].store(before.map_or(0, |f| f as usize), Ordering::Release);
                AFTER[i].store(after.map_or(0, |f| f as usize), Ordering::Release);
                COUNT.store(i + 1, Ordering::Release);
                Ok(())
            }

            #[allow(clippy::too_many_arguments)]
            unsafe extern "system" fn detour($($arg: $ty),*) -> $ret {
                let mut args = Args { $($arg,)* };
                let count = COUNT.load(Ordering::Acquire);
                for slot in &BEFORE[..count] {
                    let f = slot.load(Ordering::Acquire);
                    if f != 0 {
                        let f: Before = std::mem::transmute(f);
                        f(&mut args);
                    }
                }
                let original: Raw = std::mem::transmute(ORIGINAL.load(Ordering::Acquire));
                let ret = original($(args.$arg),*);
                for slot in &AFTER[..count] {
                    let f = slot.load(Ordering::Acquire);
                    if f != 0 {
                        let f: After = std::mem::transmute(f);
                        f(&args, ret);
                    }
                }
                ret
            }
        }
    };
}
