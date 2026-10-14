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
