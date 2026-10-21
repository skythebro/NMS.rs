//! Shared hooks: one detour per game function, with an ordered list of mod callbacks.
//!
//! Two detours on one prologue are not allowed, and pyMHF let several mods hook the same function.
//! [`shared_hook!`] declares a module per game function with a typed `Args` struct; each mod calls
//! `subscribe` with a `before` and/or `after` callback. Callbacks run in subscription order; a
//! `before` may change the arguments the next callback and the original see.
//!
//! Since L2 the detour is the generic one in [`crate::detour`]: the typed callbacks are fanned out
//! from one generic subscription, and `subscribe_call` gives a mod the full generic powers on the
//! same function (skip the original, replace the result, caller address, priority).

use crate::detour::Call;

/// Most typed callbacks one shared hook accepts.
pub const MAX_SUBSCRIBERS: usize = 16;

/// A value that can be read from and written to one argument position of a hooked call.
pub trait HookArg: Copy {
    fn read(call: &Call, i: usize) -> Self;
    fn write(self, call: &mut Call, i: usize);
}

/// A return value read from a hooked call (RAX for integers, XMM0 for floats).
pub trait HookRet: Copy {
    fn read_ret(call: &Call) -> Self;
}

macro_rules! int_arg {
    ($($t:ty),*) => {$(
        impl HookArg for $t {
            fn read(call: &Call, i: usize) -> Self {
                call.arg(i) as $t
            }
            fn write(self, call: &mut Call, i: usize) {
                call.set_arg(i, self as u64);
            }
        }
        impl HookRet for $t {
            fn read_ret(call: &Call) -> Self {
                call.ret() as $t
            }
        }
    )*};
}
int_arg!(u64, i64, usize, isize, u32, i32, u16, i16, u8, i8);

impl HookArg for f32 {
    fn read(call: &Call, i: usize) -> Self {
        call.arg_f32(i)
    }
    fn write(self, call: &mut Call, i: usize) {
        call.set_arg_f32(i, self);
    }
}
impl HookRet for f32 {
    fn read_ret(call: &Call) -> Self {
        call.ret_f32()
    }
}
impl HookArg for f64 {
    fn read(call: &Call, i: usize) -> Self {
        call.arg_f64(i)
    }
    fn write(self, call: &mut Call, i: usize) {
        call.set_arg_f64(i, self);
    }
}
impl HookRet for f64 {
    fn read_ret(call: &Call) -> Self {
        call.ret_f64()
    }
}
impl HookRet for () {
    fn read_ret(_: &Call) -> Self {}
}

/// Declare a shared hook module. List every argument, stack ones included, with floats in their
/// real positions (`f32`/`f64`): the count decides how many stack slots are forwarded and the types
/// decide whether an argument is read from a general or an XMM register.
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
            use $crate::detour::{self, Call, Flow};
            use $crate::shared::{HookArg as _, HookRet as _, MAX_SUBSCRIBERS};

            #[derive(Clone, Copy, Debug, PartialEq)]
            pub struct Args {
                $(pub $arg: $ty,)*
            }

            /// Runs before the original. May change the arguments.
            pub type Before = fn(&mut Args);
            /// Runs after the original with the arguments it received and its return value.
            pub type After = fn(&Args, $ret);

            /// Arguments the game function takes.
            pub const N_ARGS: usize = {
                let names: &[&str] = &[$(stringify!($arg)),*];
                names.len()
            };

            static SUB: OnceLock<detour::SubRef> = OnceLock::new();
            static ADD: Mutex<()> = Mutex::new(());
            static COUNT: AtomicUsize = AtomicUsize::new(0);
            static BEFORE: [AtomicUsize; MAX_SUBSCRIBERS] = [const { AtomicUsize::new(0) }; MAX_SUBSCRIBERS];
            static AFTER: [AtomicUsize; MAX_SUBSCRIBERS] = [const { AtomicUsize::new(0) }; MAX_SUBSCRIBERS];

            #[allow(unused_assignments, unused_mut, unused_variables)]
            fn read(call: &Call) -> Args {
                let mut i = 0usize;
                $(let $arg = <$ty>::read(call, i); i += 1;)*
                Args { $($arg,)* }
            }

            /// Write back only the arguments a callback changed: an untouched argument keeps the
            /// exact bits the game passed (a narrow type would otherwise drop the upper bits).
            #[allow(unused_assignments, unused_mut, unused_variables, clippy::float_cmp)]
            fn write_changed(before: &Args, args: &Args, call: &mut Call) {
                let mut i = 0usize;
                $(if args.$arg != before.$arg { args.$arg.write(call, i); } i += 1;)*
            }

            fn fan_before(call: &mut Call) -> Flow {
                let count = COUNT.load(Ordering::Acquire);
                if !BEFORE.iter().take(count).any(|s| s.load(Ordering::Acquire) != 0) {
                    return Flow::Continue;
                }
                let mut args = read(call);
                let seen = args;
                for slot in BEFORE.iter().take(count) {
                    let f = slot.load(Ordering::Acquire);
                    if f != 0 {
                        // SAFETY: only `add` stores here, always a `Before`.
                        let f = unsafe { std::mem::transmute::<usize, Before>(f) };
                        f(&mut args);
                    }
                }
                write_changed(&seen, &args, call);
                Flow::Continue
            }

            fn fan_after(call: &mut Call) {
                let count = COUNT.load(Ordering::Acquire);
                if !AFTER.iter().take(count).any(|s| s.load(Ordering::Acquire) != 0) {
                    return;
                }
                let args = read(call);
                let ret = <$ret>::read_ret(call);
                for slot in AFTER.iter().take(count) {
                    let f = slot.load(Ordering::Acquire);
                    if f != 0 {
                        // SAFETY: only `add` stores here, always an `After`.
                        let f = unsafe { std::mem::transmute::<usize, After>(f) };
                        f(&args, ret);
                    }
                }
            }

            /// Find the function by its unique pattern, hook it on first use, add the callbacks.
            pub fn subscribe(
                image: &$crate::process::ProcessImage,
                pattern: &str,
                before: Option<Before>,
                after: Option<After>,
            ) -> Result<(), String> {
                use $crate::Code as _;
                if SUB.get().is_some() {
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
                    let _guard = ADD.lock().map_err(|_| format!("{}: subscribe lock poisoned", $label))?;
                    if SUB.get().is_none() {
                        let sub = detour::subscribe_at(
                            $label,
                            target,
                            N_ARGS,
                            detour::Options::default(),
                            Some(fan_before),
                            Some(fan_after),
                        )?;
                        let _ = SUB.set(sub);
                    }
                }
                add(before, after)
            }

            /// Subscribe a generic callback to the same function: it may skip the original
            /// (`Flow::Skip`), replace the result (`Call::set_ret`) and read `Call::caller`. The typed
            /// callbacks of this module run at priority 0; pick a lower priority to run before them.
            ///
            /// # Safety
            /// The pattern must find the first byte of a function with exactly this signature.
            pub unsafe fn subscribe_call(
                image: &$crate::process::ProcessImage,
                pattern: &str,
                opts: detour::Options,
                before: Option<detour::Before>,
                after: Option<detour::After>,
            ) -> Result<detour::SubRef, String> {
                detour::subscribe(image, $label, pattern, N_ARGS, opts, before, after)
            }

            /// Whether the game function is hooked.
            pub fn installed() -> bool {
                SUB.get().is_some()
            }

            /// The game function without any subscriber (call it from a callback to avoid recursion).
            pub fn original() -> usize {
                SUB.get().map_or(0, |s| s.original())
            }

            fn add(before: Option<Before>, after: Option<After>) -> Result<(), String> {
                let _guard = ADD.lock().map_err(|_| format!("{}: subscribe lock poisoned", $label))?;
                let i = COUNT.load(Ordering::Acquire);
                if i >= MAX_SUBSCRIBERS {
                    return Err(format!("{}: more than {MAX_SUBSCRIBERS} subscribers", $label));
                }
                BEFORE[i].store(before.map_or(0, |f| f as usize), Ordering::Release);
                AFTER[i].store(after.map_or(0, |f| f as usize), Ordering::Release);
                COUNT.store(i + 1, Ordering::Release);
                Ok(())
            }
        }
    };
}
