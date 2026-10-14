//! `nms-rs <command>`
//!
//!   install [--force]   copy xinput9_1_0.dll (next to this exe) into <game>\Binaries
//!   uninstall           remove it again, only if it is ours
//!   disable | enable    create or remove <game>\NMS.rs\disabled (kill switch)
//!   status              list the loaded mods and their settings (game must be running)
//!   set <mod> <key> <true|false|text>
//!   play [<game command>...]  merge UI, start the game, stay open until it exits
//!   prepare             merge UI scripts (Proton runs this before the game)
//!   pack-logs           zip NMS.rs logs and open the zip in Explorer
//!   conflicts           write NMS.rs\\conflicts.txt (leftover UI MBIN clashes)
//!   merge --plan        list UI merge scripts and jobs, without writing
//!   setup [--yes] [--no-shortcut] [--force]
//!   steam-option status | set | remove [--yes]
//!   mods park|unpark|install|update <name> | park-all | install-all | update-all | cleanup-python | strip-launch | status
//!   sizes apply --columns N --rows auto|N --window F --slots P [--no-filters]
//!   migrate [--yes]       move an old NMS.py (version 3) setup over to NMS.rs (asks first)
//!   manager              open the out-of-game window (also the default with no args)
//!
//! The game folder is found by walking up from this exe, then from the current folder.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nms_core::{ipc, paths};
use serde_json::{json, Value};

mod cards;
mod files;
mod fonts;
mod i18n;
mod manager_settings;
mod merge;
mod migrate;
mod mods;
mod palette;
mod play;
mod setup;
mod shortcut;
mod steam;
mod style;
mod sizes;
mod store;
mod tools;
mod ui;

const DLL: &str = "xinput9_1_0.dll";
/// Exported by our proxy (`NMS_RS_PROXY_MARKER`), to tell it apart from someone else's xinput proxy.
const MARKER: &[u8] = b"NMS.rs proxy marker";

fn game_root() -> Option<PathBuf> {
    let from_exe = std::env::current_exe().ok().and_then(|e| e.parent().and_then(paths::find_game_root));
    from_exe.or_else(|| std::env::current_dir().ok().and_then(|d| paths::find_game_root(&d)))
}

fn is_ours(path: &Path) -> bool {
    std::fs::read(path).map(|b| b.windows(MARKER.len()).any(|w| w == MARKER)).unwrap_or(false)
}

pub(crate) fn install(root: &Path, force: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let src = exe.parent().ok_or("no exe folder")?.join(DLL);
    if !src.is_file() {
        return Err(format!("{} not found next to this program", DLL));
    }
    let dst = root.join("Binaries").join(DLL);
    if dst.is_file() && !is_ours(&dst) && !force {
        return Err(format!("{} already holds another {DLL}; not replacing it (use --force)", dst.display()));
    }
    std::fs::create_dir_all(paths::loader_dir(root).join("logs")).map_err(|e| e.to_string())?;
    std::fs::copy(&src, &dst).map_err(|e| format!("copy failed (is the game running?): {e}"))?;
    Ok(format!("installed {}", dst.display()))
}

fn uninstall(root: &Path) -> Result<String, String> {
    let dst = root.join("Binaries").join(DLL);
    if !dst.is_file() {
        return Ok("nothing to remove".into());
    }
    if !is_ours(&dst) {
        return Err(format!("{} is not ours; leaving it", dst.display()));
    }
    std::fs::remove_file(&dst).map_err(|e| format!("remove failed (is the game running?): {e}"))?;
    Ok(format!("removed {}", dst.display()))
}

fn kill_switch(root: &Path, disable: bool) -> Result<String, String> {
    let flag = paths::loader_dir(root).join("disabled");
    if disable {
        std::fs::create_dir_all(flag.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&flag, b"").map_err(|e| e.to_string())?;
        Ok("disabled: the game stays vanilla from the next start".into())
    } else {
        let _ = std::fs::remove_file(&flag);
        Ok("enabled from the next start".into())
    }
}

fn status() -> Result<String, String> {
    let reply = ipc::request(&json!({"op": "list"}).to_string())?;
    let mut out = String::new();
    for m in reply["mods"].as_array().into_iter().flatten() {
        out.push_str(&format!("{}\n", m["name"].as_str().unwrap_or("?")));
        for s in m["settings"].as_array().into_iter().flatten() {
            out.push_str(&format!("  {:<12} {:<28} {}\n", s["key"].as_str().unwrap_or(""), s["label"].as_str().unwrap_or(""), s["value"]));
        }
    }
    Ok(out.trim_end().to_string())
}

fn set(mod_name: &str, key: &str, raw: &str) -> Result<String, String> {
    let value = match raw {
        "true" | "on" => Value::Bool(true),
        "false" | "off" => Value::Bool(false),
        text => Value::String(text.to_string()),
    };
    let reply = ipc::request(&json!({"op": "set", "mod": mod_name, "key": key, "value": value}).to_string())?;
    if reply["ok"] == json!(true) {
        Ok(reply["message"].as_str().unwrap_or("done").to_string())
    } else {
        Err(reply["error"].as_str().unwrap_or("failed").to_string())
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let manager = words.is_empty() || words == ["manager"];
    let needs_root = manager
        || matches!(words.first(), Some(&"install" | &"uninstall" | &"disable" | &"enable" | &"play" | &"prepare" | &"pack-logs" | &"conflicts" | &"merge" | &"mods" | &"sizes" | &"migrate"));
    let root = if needs_root {
        match game_root() {
            Some(r) => Some(r),
            None => {
                eprintln!("game folder not found (no Binaries\\NMS.exe above this program or the current folder)");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    if manager {
        let Some(r) = root else {
            return ExitCode::from(2);
        };
        return ui::run(r);
    }
    let result = match (words.as_slice(), &root) {
        (["install"], Some(r)) => install(r, false),
        (["install", "--force"], Some(r)) => install(r, true),
        (["uninstall"], Some(r)) => uninstall(r),
        (["disable"], Some(r)) => kill_switch(r, true),
        (["enable"], Some(r)) => kill_switch(r, false),
        (["status"], _) => status(),
        (["set", m, k, v], _) => set(m, k, v),
        (["play", cmd @ ..], Some(r)) => return play::cli_play(r, cmd),
        (["prepare"], Some(r)) => return play::cli_prepare(r),
        (["pack-logs"], Some(r)) => files::pack_logs(r).map(|path| {
            let _ = nms_core::spawn::gui("explorer.exe").arg(format!("/select,{}", path.display())).spawn();
            path.display().to_string()
        }),
        (["conflicts"], Some(r)) => {
            let path = files::write_conflict_report(r, None, &std::collections::BTreeMap::new(), true);
            Ok(path.display().to_string())
        }
        (["merge", "--plan"], Some(r)) => {
            crate::merge::Merge::for_game(r).print_plan();
            return ExitCode::SUCCESS;
        }
        (["setup", rest @ ..], _) => return setup::cli(rest),
        (["steam-option", rest @ ..], _) => steam::cli(rest),
        (["mods", rest @ ..], Some(r)) => crate::mods::cli(r, rest),
        (["sizes", rest @ ..], Some(r)) => crate::sizes::apply::cli(r, rest),
        (["migrate", rest @ ..], Some(r)) => return migrate::cli(r, rest),
        _ => Err("usage: nms-rs [manager] | install [--force] | uninstall | disable | enable | status | set <mod> <key> <value> | play [<game command>...] | prepare | pack-logs | conflicts | merge --plan | setup [--yes] [--no-shortcut] [--force] | steam-option status | set | remove [--yes] | mods park|unpark|install|update <name> | park-all | install-all | update-all | cleanup-python | strip-launch | status | sizes apply --columns N --rows auto|N --window F --slots P [--no-filters] | migrate [--yes]".into()),
    };
    match result {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(text) => {
            eprintln!("{text}");
            ExitCode::FAILURE
        }
    }
}
