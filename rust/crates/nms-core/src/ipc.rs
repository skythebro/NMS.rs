//! The settings pipe between the game and the settings window, one JSON object per line.
//!
//! Requests: `{"op":"ping"}`, `{"op":"list"}`, `{"op":"set","mod":"X","key":"k","value":true}`.
//! Replies always carry `"ok"`, plus `"error"` when it is false.
//! The window runs in its own process, so it can never hold back the game's frame rate.

use serde_json::{json, Value};

use crate::registry::Registry;

pub const PIPE_NAME: &str = r"\\.\pipe\nms-rs";

/// The release version (`NMS_RS_VERSION`, set by `release.py rust-build`), else the crate version.
pub const VERSION: &str = match option_env!("NMS_RS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// Answer one request line.
pub fn handle(registry: &Registry, line: &str) -> String {
    let reply = match serde_json::from_str::<Value>(line) {
        Err(e) => json!({"ok": false, "error": format!("bad request: {e}")}),
        Ok(req) => match req.get("op").and_then(Value::as_str) {
            Some("ping") => json!({"ok": true, "version": VERSION}),
            Some("list") => json!({"ok": true, "mods": registry.list()}),
            Some("set") => {
                let field = |k: &str| req.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                let value = req.get("value").cloned().unwrap_or(Value::Null);
                match registry.set(&field("mod"), &field("key"), &value) {
                    Ok(message) => json!({"ok": true, "message": message}),
                    Err(error) => json!({"ok": false, "error": error}),
                }
            }
            _ => json!({"ok": false, "error": "unknown op"}),
        },
    };
    reply.to_string()
}

/// Send one request to the running game and return its reply line.
pub fn request(line: &str) -> Result<Value, String> {
    request_to(PIPE_NAME, line)
}

pub fn request_to(pipe_name: &str, line: &str) -> Result<Value, String> {
    use std::io::{BufRead, BufReader, Write};
    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name)
        .map_err(|e| format!("the game is not running with the Rust loader ({e})"))?;
    pipe.write_all(line.trim_end().as_bytes()).and_then(|_| pipe.write_all(b"\n")).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(pipe).read_line(&mut reply).map_err(|e| e.to_string())?;
    serde_json::from_str(&reply).map_err(|e| format!("bad reply: {e}"))
}

#[cfg(windows)]
pub use server::{serve, serve_on};

#[cfg(windows)]
mod server {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::io::FromRawHandle;
    use std::sync::Arc;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    use super::{handle, PIPE_NAME};
    use crate::registry::Registry;

    /// Serve the pipe forever on a background thread, one client at a time.
    pub fn serve(registry: Arc<Registry>, log: impl Fn(&str) + Send + 'static) {
        serve_on(PIPE_NAME.to_string(), registry, log)
    }

    pub fn serve_on(pipe_name: String, registry: Arc<Registry>, log: impl Fn(&str) + Send + 'static) {
        std::thread::Builder::new()
            .name("nms-rs pipe".into())
            .spawn(move || loop {
                let wide: Vec<u16> = pipe_name.encode_utf16().chain(Some(0)).collect();
                // SAFETY: plain Win32 call with a valid NUL-terminated name.
                let h = unsafe {
                    CreateNamedPipeW(
                        wide.as_ptr(),
                        PIPE_ACCESS_DUPLEX,
                        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                        1,
                        64 * 1024,
                        64 * 1024,
                        0,
                        std::ptr::null(),
                    )
                };
                if h == INVALID_HANDLE_VALUE {
                    log("settings pipe could not be created; live settings are off");
                    return;
                }
                // SAFETY: h is a valid pipe handle we own.
                let connected = unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) } != 0
                    || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                // SAFETY: File takes ownership and closes the handle on drop.
                let file = unsafe { File::from_raw_handle(h as _) };
                if !connected {
                    continue;
                }
                let mut writer = match file.try_clone() {
                    Ok(w) => w,
                    Err(_) => continue,
                };
                for line in BufReader::new(file).lines() {
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    let reply = handle(&registry, &line);
                    if writer.write_all(reply.as_bytes()).and_then(|_| writer.write_all(b"\n")).is_err() {
                        break;
                    }
                }
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{GameMod, Kind, SettingSpec};
    use std::sync::Mutex;

    struct Fake(Mutex<bool>);

    impl GameMod for Fake {
        fn name(&self) -> &'static str {
            "Fake"
        }
        fn specs(&self) -> &'static [SettingSpec] {
            &[
                SettingSpec { key: "status", label: "Status", kind: Kind::Status },
                SettingSpec { key: "on", label: "On", kind: Kind::Bool },
            ]
        }
        fn get(&self, key: &str) -> Option<Value> {
            match key {
                "status" => Some(json!("On")),
                "on" => Some(json!(*self.0.lock().unwrap())),
                _ => None,
            }
        }
        fn set(&self, _key: &str, value: &Value) -> Result<String, String> {
            *self.0.lock().unwrap() = value.as_bool().unwrap();
            Ok("Fake applied".into())
        }
    }

    fn reg() -> Registry {
        let mut r = Registry::default();
        r.add(Box::new(Fake(Mutex::new(false))));
        r
    }

    fn ask(r: &Registry, line: &str) -> Value {
        serde_json::from_str(&handle(r, line)).unwrap()
    }

    #[test]
    fn ping_and_list() {
        let r = reg();
        assert_eq!(ask(&r, r#"{"op":"ping"}"#)["ok"], json!(true));
        let list = ask(&r, r#"{"op":"list"}"#);
        assert_eq!(list["mods"][0]["name"], json!("Fake"));
        assert_eq!(list["mods"][0]["settings"][1]["value"], json!(false));
    }

    #[test]
    fn set_flips_a_switch() {
        let r = reg();
        let reply = ask(&r, r#"{"op":"set","mod":"Fake","key":"on","value":true}"#);
        assert_eq!(reply["message"], json!("Fake applied"));
        assert_eq!(ask(&r, r#"{"op":"list"}"#)["mods"][0]["settings"][1]["value"], json!(true));
    }

    #[test]
    fn refuses_bad_requests() {
        let r = reg();
        assert_eq!(ask(&r, "nope")["ok"], json!(false));
        assert_eq!(ask(&r, r#"{"op":"set","mod":"Fake","key":"status","value":"x"}"#)["ok"], json!(false));
        assert_eq!(ask(&r, r#"{"op":"set","mod":"Fake","key":"on","value":"yes"}"#)["ok"], json!(false));
        assert_eq!(ask(&r, r#"{"op":"set","mod":"Nope","key":"on","value":true}"#)["ok"], json!(false));
        assert_eq!(ask(&r, r#"{"op":"dance"}"#)["ok"], json!(false));
    }
}
