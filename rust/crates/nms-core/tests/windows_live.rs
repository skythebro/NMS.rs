//! Live Windows checks outside the game: the settings pipe end to end, and a real detour.
#![cfg(windows)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nms_core::hook::Hook;
use nms_core::{ipc, GameMod, Kind, Registry, SettingSpec};
use serde_json::{json, Value};

struct Switch(Mutex<bool>);

impl GameMod for Switch {
    fn name(&self) -> &'static str {
        "Switch"
    }
    fn specs(&self) -> &'static [SettingSpec] {
        &[SettingSpec { key: "on", label: "On", kind: Kind::Bool }]
    }
    fn get(&self, _key: &str) -> Option<Value> {
        Some(json!(*self.0.lock().unwrap()))
    }
    fn set(&self, _key: &str, value: &Value) -> Result<String, String> {
        *self.0.lock().unwrap() = value.as_bool().unwrap();
        Ok("Switch applied".into())
    }
}

#[test]
fn pipe_round_trip() {
    let mut reg = Registry::default();
    reg.add(Box::new(Switch(Mutex::new(false))));
    // Own name: the game may be running and serving the real one.
    let name = format!(r"\\.\pipe\nms-rs-test-{}", std::process::id());
    ipc::serve_on(name.clone(), Arc::new(reg), |_| {});
    let mut reply = Err(String::new());
    for _ in 0..50 {
        reply = ipc::request_to(&name, r#"{"op":"set","mod":"Switch","key":"on","value":true}"#);
        if reply.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(reply.unwrap()["message"], json!("Switch applied"));
    // The server takes the next client after the first disconnects.
    let mut list = Err(String::new());
    for _ in 0..50 {
        list = ipc::request_to(&name, r#"{"op":"list"}"#);
        if list.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(list.unwrap()["mods"][0]["settings"][0]["value"], json!(true));
}

type Fn4 = extern "system" fn(i64, i64, i64, f64) -> i64;

#[inline(never)]
extern "system" fn target(a: i64, b: i64, c: i64, d: f64) -> i64 {
    std::hint::black_box(a * 100 + b * 10 + c + d as i64)
}

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);

extern "system" fn detour(a: i64, b: i64, c: i64, d: f64) -> i64 {
    CALLS.fetch_add(1, Ordering::Relaxed);
    let original: Fn4 = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Acquire)) };
    original(a, b, c, d) + 1_000_000
}

#[test]
fn detour_calls_original_with_every_argument() {
    let f: Fn4 = std::hint::black_box(target);
    assert_eq!(f(1, 2, 3, 4.0), 127);
    let hook = unsafe { Hook::new("target", target as *const () as usize, detour as *const ()) }.unwrap();
    ORIGINAL.store(hook.original() as usize, Ordering::Release);
    hook.enable().unwrap();
    assert_eq!(f(1, 2, 3, 4.0), 1_000_127);
    hook.disable().unwrap();
    assert_eq!(f(1, 2, 3, 4.0), 127);
    assert_eq!(CALLS.load(Ordering::Relaxed), 1);
}
