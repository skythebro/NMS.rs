//! Quick Transfer All: send-all, Quick Deposit, and Exosuit lock.

mod caves;
mod deposit;
mod game;
mod hooks;
mod inv;
mod lock_inv;
mod lock_shop;
mod lock_ui;
mod notify;
mod runtime;
mod send_all;
mod text;

use std::sync::{Arc, Mutex, OnceLock};

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::{paths, GameMod, Kind, Range, SettingSpec, Settings};
use serde_json::{json, Value};

use game::Game;
use runtime::{Runtime, VK_ALT, VK_CTRL, VK_SHIFT};

static RUNTIME: OnceLock<Arc<Mutex<Runtime>>> = OnceLock::new();

/// The keys that can be held while pressing Quick Transfer: `(setting value, label)`.
const HOLD_KEYS: &[(&str, &str)] = &[("shift", "Shift"), ("ctrl", "Control"), ("alt", "Alt")];

const SPECS: &[SettingSpec] = &[
    SettingSpec {
        key: "status",
        label: "Status",
        kind: Kind::Status,
    },
    SettingSpec {
        key: "key",
        label: "Hold key",
        kind: Kind::Enum,
    },
    SettingSpec {
        key: "deposit",
        label: "Deposit",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "same_item",
        label: "Same item only",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "suit_lock",
        label: "Exosuit lock mode",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "suit_lock_follow",
        label: "Lock follows item",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "write_logs",
        label: "Write log",
        kind: Kind::Bool,
    },
];

fn hold_vk(key: &str) -> Option<i32> {
    match key {
        "shift" => Some(VK_SHIFT),
        "ctrl" => Some(VK_CTRL),
        "alt" => Some(VK_ALT),
        _ => None,
    }
}

fn parse_hold_key(value: &Value) -> Result<String, String> {
    let raw = value
        .as_str()
        .ok_or_else(|| "expected shift, ctrl, or alt".to_string())?;
    let key = raw.trim().to_ascii_lowercase();
    if hold_vk(&key).is_some() {
        Ok(key)
    } else {
        Err("key must be one of shift, ctrl, alt".into())
    }
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn with_runtime<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Runtime) -> R,
{
    let rt = RUNTIME.get()?;
    let Ok(mut state) = rt.try_lock() else { return None };
    Some(f(&mut state))
}

pub struct QuickTransferAll {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    #[expect(dead_code)]
    hooks: Vec<Arc<Hook>>,
}

impl QuickTransferAll {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "QuickTransferAll");
        let defaults = json!({
            "key": "shift",
            "write_logs": false,
            "deposit": true,
            "same_item": true,
            "suit_lock": true,
            "suit_lock_follow": false,
        });
        let settings = Arc::new(Mutex::new(Settings::load(
            &dir.join("quick_transfer_all.json"),
            defaults,
        )));
        let write_logs = settings
            .lock()
            .map(|s| s.is_not_false("write_logs"))
            .unwrap_or(false);
        let log = Arc::new(Log::open(&dir.join("quick_transfer_all.log"), write_logs));
        let status = Arc::new(Mutex::new("Off, see log".to_string()));

        let hold = settings
            .lock()
            .ok()
            .and_then(|s| s.get("key").and_then(|v| v.as_str()).map(str::to_string))
            .unwrap_or_else(|| "shift".into());
        let hold = if hold_vk(&hold).is_some() {
            hold
        } else {
            "shift".into()
        };

        let mut runtime = Runtime::new(log.clone(), dir.clone());
        runtime.qt_vk = hold_vk(&hold);
        let flag = |key: &str| {
            settings
                .lock()
                .map(|s| s.is_not_false(key))
                .unwrap_or(true)
        };
        runtime.deposit = flag("deposit");
        runtime.same_item = flag("same_item");
        runtime.suit_lock = flag("suit_lock");
        runtime.suit_lock_follow = settings
            .lock()
            .map(|s| s.is_true("suit_lock_follow"))
            .unwrap_or(false);

        let mut on = false;
        match Game::new(&ctx.image) {
            Ok(mut game) => {
                game.finish_locates(&ctx.image, &log);
                if game.nearby_ids != 0 {
                    log.info(&format!("Deposit: {}", on_off(runtime.deposit)));
                } else {
                    log.info("Deposit is off: the nearby-storage lookup was not found (game update?)");
                }
                if runtime.suit_lock_follow {
                    if game.follow_ok {
                        log.info("suit lock follow: on");
                    }
                } else if game.follow_ok {
                    log.info("suit lock follow: off");
                }
                log.info(&format!(
                    "Quick transfer all: hold {hold}, or a controller button bound to the Steam Input menu action Unbound, while picking a Quick Transfer destination"
                ));
                // Before the caves: their tooltip and toast texts are built in this language.
                runtime.lang = crate::common::lang::detect(&ctx.image, &log);
                log.info(&format!("button text language: {}", runtime.lang));
                runtime.apply_game(game);
                runtime.install_features(&ctx.image);
                on = true;
            }
            Err(error) => {
                log.info(&format!("Quick transfer all is off: {error}"));
            }
        }

        let rt = Arc::new(Mutex::new(runtime));
        let _ = RUNTIME.set(rt);

        let installed = hooks::install(&ctx.image, &log);
        if on && installed.all_ok {
            if let Ok(mut s) = status.lock() {
                *s = "On".into();
            }
        } else if on && !installed.all_ok {
            log.info("Quick transfer all is off: a required hook was not found (game update?)");
        }

        Self {
            settings,
            status,
            log,
            hooks: installed.kept,
        }
    }
}

impl GameMod for QuickTransferAll {
    fn name(&self) -> &'static str {
        "QuickTransferAll"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn range(&self, key: &str) -> Option<Range> {
        (key == "key").then_some(Range::Enum { choices: HOLD_KEYS, default: 0 })
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            "key" => Some(json!(self
                .settings
                .lock()
                .ok()?
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("shift")
                .trim()
                .to_ascii_lowercase())),
            "write_logs" => Some(json!(self.settings.lock().ok()?.is_not_false("write_logs"))),
            "deposit" => Some(json!(self.settings.lock().ok()?.is_not_false("deposit"))),
            "same_item" => Some(json!(self.settings.lock().ok()?.is_not_false("same_item"))),
            "suit_lock" => Some(json!(self.settings.lock().ok()?.is_not_false("suit_lock"))),
            "suit_lock_follow" => Some(json!(self.settings.lock().ok()?.is_true("suit_lock_follow"))),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "status" => None,
            // The Settings default is "shift", the same as HOLD_KEYS[0].
            _ => self.settings.lock().ok()?.default(key).cloned(),
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        match key {
            "write_logs" => {
                let on = value
                    .as_bool()
                    .ok_or_else(|| "expected true or false".to_string())?;
                self.settings
                    .lock()
                    .map_err(|_| "settings lock poisoned")?
                    .set(key, json!(on))?;
                self.log.set_enabled(on);
                Ok(format!("Write log {}", on_off(on)))
            }
            "key" => {
                let hold = parse_hold_key(value)?;
                let vk = hold_vk(&hold);
                self.settings
                    .lock()
                    .map_err(|_| "settings lock poisoned")?
                    .set(key, json!(hold.clone()))?;
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        state.qt_vk = vk;
                    }
                }
                Ok(format!("Hold key {hold}"))
            }
            "deposit" | "same_item" | "suit_lock" | "suit_lock_follow" => {
                let on = value
                    .as_bool()
                    .ok_or_else(|| "expected true or false".to_string())?;
                self.settings
                    .lock()
                    .map_err(|_| "settings lock poisoned")?
                    .set(key, json!(on))?;
                let mut follow_missing = false;
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        match key {
                            "deposit" => state.deposit = on,
                            "same_item" => state.same_item = on,
                            "suit_lock" => state.suit_lock = on,
                            "suit_lock_follow" => state.suit_lock_follow = on,
                            _ => {}
                        }
                        state.apply_live_toggles();
                        follow_missing = key == "suit_lock_follow" && on && !state.follow_ok;
                    }
                }
                let line = match key {
                    "deposit" => format!("Deposit: {}", on_off(on)),
                    "same_item" => format!(
                        "Deposit matches stored items: {}",
                        if on { "yes" } else { "no, everything lit" }
                    ),
                    "suit_lock" => format!("Exosuit lock mode: {}", on_off(on)),
                    "suit_lock_follow" if follow_missing => "suit lock follow not changed".into(),
                    "suit_lock_follow" => format!("suit lock follow: {}", on_off(on)),
                    _ => format!("{key} {}", on_off(on)),
                };
                self.log.info(&line);
                Ok(line)
            }
            _ => Err(format!("unknown setting {key:?}")),
        }
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(QuickTransferAll::new(ctx))
}
