//! A shared hook with two subscribers on a real function, float argument included.
#![cfg(windows)]

use std::sync::atomic::{AtomicU64, Ordering};

type Target = extern "system" fn(i64, i32, i64, i64, f32) -> i64;

#[inline(never)]
extern "system" fn target(a: i64, b: i32, c: i64, d: i64, e: f32) -> i64 {
    std::hint::black_box(a * 10_000 + b as i64 * 1_000 + c * 100 + d * 10 + e as i64)
}

nms_core::shared_hook! {
    mod shared_target = "target": fn(a: i64, b: i32, c: i64, d: i64, e: f32) -> i64;
}

static TRACE: AtomicU64 = AtomicU64::new(0);

fn push(digit: u64) {
    TRACE.store(TRACE.load(Ordering::SeqCst) * 10 + digit, Ordering::SeqCst);
}

fn first_before(args: &mut shared_target::Args) {
    push(1);
    args.b += 1;
}

fn second_before(args: &mut shared_target::Args) {
    push(2);
    assert_eq!(args.b, 3, "second subscriber sees the first one's change");
    args.e += 2.0;
}

fn first_after(args: &shared_target::Args, ret: i64) {
    push(3);
    assert_eq!(args.e, 6.0);
    assert_eq!(ret, 13_346);
}

#[test]
fn subscribers_run_in_order_and_change_arguments() {
    let f: Target = std::hint::black_box(target);
    assert_eq!(f(1, 2, 3, 4, 4.0), 12_344);
    unsafe {
        shared_target::subscribe_at(target as *const () as usize, Some(first_before), Some(first_after)).unwrap();
        shared_target::subscribe_at(target as *const () as usize, Some(second_before), None).unwrap();
    }
    assert!(shared_target::installed());    assert_eq!(f(1, 2, 3, 4, 4.0), 13_346);
    assert_eq!(TRACE.load(Ordering::SeqCst), 123);
}


#[inline(never)]
extern "system" fn second_target(a: i64, b: i64) -> i64 {
    std::hint::black_box(a * 2 + b)
}

nms_core::shared_hook! {
    mod shared_second = "second_target": fn(a: i64, b: i64) -> i64;
}

static TYPED_AFTER_RET: AtomicU64 = AtomicU64::new(0);

fn typed_after(_: &shared_second::Args, ret: i64) {
    TYPED_AFTER_RET.store(ret as u64, Ordering::SeqCst);
}

fn skip_and_answer(c: &mut nms_core::detour::Call) -> nms_core::detour::Flow {
    c.set_ret(77);
    nms_core::detour::Flow::Skip
}

#[test]
fn a_generic_subscriber_skips_the_original_and_typed_afters_see_its_result() {
    let target = second_target as *const () as usize;
    unsafe { shared_second::subscribe_at(target, None, Some(typed_after)).unwrap() };
    let f: extern "system" fn(i64, i64) -> i64 = std::hint::black_box(second_target);
    assert_eq!(f(5, 1), 11);
    assert_eq!(TYPED_AFTER_RET.load(Ordering::SeqCst), 11);
    let generic = unsafe {
        nms_core::detour::subscribe_at(
            "second_target",
            target,
            shared_second::N_ARGS,
            nms_core::detour::Options { priority: -10 },
            Some(skip_and_answer),
            None,
        )
        .unwrap()
    };
    assert_eq!(f(5, 1), 77, "skipped, result replaced");
    assert_eq!(TYPED_AFTER_RET.load(Ordering::SeqCst), 77, "typed after saw the replaced result");
    generic.set_enabled(false);
    assert_eq!(f(5, 1), 11, "toggle off: vanilla again");
    let raw: extern "system" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(shared_second::original()) };
    assert_eq!(raw(1, 1), 3);
}


#[inline(never)]
extern "system" fn wide_target(a: u64, b: u64) -> u64 {
    std::hint::black_box(a ^ b)
}

nms_core::shared_hook! {
    // Declared narrower than the game passes: untouched arguments must keep their full bits.
    mod shared_wide = "wide_target": fn(a: u8, b: u32) -> u64;
}

fn change_b_only(args: &mut shared_wide::Args) {
    args.b = 7;
}

#[test]
fn untouched_narrow_arguments_keep_their_full_value() {
    let target = wide_target as *const () as usize;
    unsafe { shared_wide::subscribe_at(target, Some(change_b_only), None).unwrap() };
    let f: extern "system" fn(u64, u64) -> u64 = std::hint::black_box(wide_target);
    assert_eq!(f(0x1234_5678_9ABC_DEF0, 0xFFFF_FFFF_0000_0001), 0x1234_5678_9ABC_DEF0 ^ 7);
}
