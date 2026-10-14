//! `nms-rs setup [--yes] [--no-shortcut] [--force]`.
//!
//! Ports `loader/lib/setup_kit.py` `run_setup` without the Python runtime, stamp,
//! pip, Defender, or NMSpyWindow. Relocate copies this `NMS.rs` folder and sibling
//! `GAMEDATA\MODS` into the game, then re-runs the installed exe.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nms_core::paths;

use crate::files;
use crate::shortcut::{self, Spec};
use crate::steam;
use crate::store::{self, Store};

const KEEP_DIRS: &[&str] = &["logs", "cache", "tools", "parked"];
const KEEP_NAMES: &[&str] = &["merge_order.txt", "conflicts.txt"];
const KEEP_MOD_SUFFIXES: &[&str] = &["json", "log"];
const VORTEX_MARKERS: &[&str] = &["vortex.deployment.json", "__folder_managed_by_vortex", ".vortex_predeploy"];
const USAGE: &str = "usage: nms-rs setup [--yes] [--no-shortcut] [--force]";

pub(crate) struct SetupHooks {
    pub game_is_running: fn() -> bool,
    pub install: fn(&Path, bool) -> Result<String, String>,
    pub apply_steam: fn(&Path, bool) -> Result<Vec<String>, String>,
    pub launch_options: fn() -> Vec<String>,
    pub option_is_set: fn(&Path) -> bool,
    pub stdin_is_terminal: bool,
    pub ask_shortcut: fn() -> bool,
    pub create_shortcut: fn(&Spec) -> Option<String>,
}

struct Out {
    log: PathBuf,
}

impl Out {
    fn open(game: &Path) -> Self {
        let dir = paths::loader_dir(game).join("logs");
        let _ = std::fs::create_dir_all(&dir);
        Self { log: dir.join("setup.log") }
    }

    fn append(&self, message: &str) {
        let line = format!("{} {message}\n", now_stamp());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.log) {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn tick(&self, message: &str) {
        println!("+ {message}");
        self.append(message);
    }

    fn warn(&self, message: &str) {
        println!("! {message}");
        self.append(&format!("WARN: {message}"));
    }

    fn fail(&self, what: &str, fix: &str) -> ExitCode {
        fail_print(what, fix);
        self.append(&format!("FAIL: {what} | {fix}"));
        ExitCode::from(1)
    }
}

fn fail_print(what: &str, fix: &str) {
    println!("x {what}");
    println!("  Fix: {fix}");
    println!("  Send NMS.rs\\logs\\setup.log");
}

pub(crate) fn now_stamp() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        let mut t = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut t) };
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }
    #[cfg(not(windows))]
    {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        format!("{}", d.as_secs())
    }
}

fn name_eq(name: &str, want: &str) -> bool {
    name.eq_ignore_ascii_case(want)
}

fn keep_user_file(rel: &Path, dest: &Path) -> bool {
    if !dest.is_file() {
        return false;
    }
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if KEEP_NAMES.iter().any(|n| name_eq(name, n)) {
        return true;
    }
    let ext = rel.extension().and_then(|e| e.to_str()).unwrap_or("");
    ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("log")
}

pub(crate) fn vortex_owned(folder: &Path) -> bool {
    if folder.parent().map(|p| p.join("vortex.deployment.json").is_file()).unwrap_or(false) {
        return true;
    }
    VORTEX_MARKERS.iter().any(|name| folder.join(name).exists())
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    out
}

fn skip_rel(rel: &Path) -> bool {
    let parts: Vec<String> = rel.components().filter_map(|c| match c {
        std::path::Component::Normal(s) => Some(s.to_string_lossy().to_ascii_lowercase()),
        _ => None,
    }).collect();
    if parts.iter().any(|p| p == "__pycache__" || p == "zip_root") {
        return true;
    }
    parts.first().is_some_and(|p| KEEP_DIRS.iter().any(|d| *d == p.as_str()))
}

fn copy_file(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(src, dest).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn copy_nms_rs(src_root: &Path, dest_root: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest_root).map_err(|e| e.to_string())?;
    for path in walk_files(src_root) {
        let rel = match path.strip_prefix(src_root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if skip_rel(rel) {
            continue;
        }
        let dest = dest_root.join(rel);
        if keep_user_file(rel, &dest) {
            continue;
        }
        copy_file(&path, &dest)?;
    }
    Ok(())
}

pub(crate) fn copy_mods(src_mods: &Path, dest_mods: &Path) -> Result<Vec<String>, String> {
    let mut skipped = Vec::new();
    if !src_mods.is_dir() {
        return Ok(skipped);
    }
    std::fs::create_dir_all(dest_mods).map_err(|e| e.to_string())?;
    let mut folders: Vec<_> = std::fs::read_dir(src_mods)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    for folder in folders {
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dest = dest_mods.join(&name);
        if dest.is_dir() && vortex_owned(&dest) {
            skipped.push(name);
            continue;
        }
        for path in walk_files(&folder) {
            let rel = match path.strip_prefix(&folder) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let parts: Vec<String> = rel.components().filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_string_lossy().to_ascii_lowercase()),
                _ => None,
            }).collect();
            if parts.iter().any(|p| p == "__pycache__") {
                continue;
            }
            let dest_file = dest.join(rel);
            let suffix = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if KEEP_MOD_SUFFIXES.iter().any(|s| *s == suffix.as_str()) && dest_file.is_file() {
                continue;
            }
            copy_file(&path, &dest_file)?;
        }
    }
    Ok(skipped)
}

pub(crate) fn leftover_line(options: &[String]) -> String {
    let mut notes: Vec<&str> = Vec::new();
    for current in options {
        let lower = current.replace('/', "\\").to_ascii_lowercase();
        if (lower.contains("biggeritemgrids.exe") || lower.contains("play.bat"))
            && !notes.contains(&"old 1.x launch option")
        {
            notes.push("old 1.x launch option");
        }
        if (lower.contains("nms.py\\launch.cmd") || lower.contains("nms.py\\launch.sh"))
            && !notes.contains(&r"old NMS.py\launch.cmd option")
        {
            notes.push(r"old NMS.py\launch.cmd option");
        }
    }
    if notes.is_empty() {
        "none".into()
    } else {
        notes.join(", ")
    }
}

fn read_yes(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn ask_shortcut_stdin() -> bool {
    println!();
    println!("  Create a Desktop shortcut that starts the modded game?");
    println!("  (Play in GOG Galaxy starts the game without the extras.)");
    read_yes("  [Y/N] ")
}

fn prod_running() -> bool {
    files::game_is_running()
}

/// The launch option points at `<game>\NMS.rs\nms-rs.exe`, so that copy must exist even when Setup runs from elsewhere.
fn prod_install(root: &Path, force: bool) -> Result<String, String> {
    let msg = crate::install(root, force)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dest = paths::loader_dir(root).join("nms-rs.exe");
    let same = match (exe.canonicalize(), dest.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        std::fs::copy(&exe, &dest).map_err(|e| format!("could not copy nms-rs.exe to {}: {e}", dest.display()))?;
    }
    Ok(msg)
}

fn prod_apply(exe: &Path, ask: bool) -> Result<Vec<String>, String> {
    steam::apply_for(exe, true, ask)
}

fn prod_options() -> Vec<String> {
    steam::launch_option_strings()
}

fn prod_option_set(exe: &Path) -> bool {
    steam::option_is_set_for(exe)
}

fn prod_create(spec: &Spec) -> Option<String> {
    shortcut::create(spec)
}

fn production_hooks() -> SetupHooks {
    SetupHooks {
        game_is_running: prod_running,
        install: prod_install,
        apply_steam: prod_apply,
        launch_options: prod_options,
        option_is_set: prod_option_set,
        stdin_is_terminal: io::stdin().is_terminal(),
        ask_shortcut: ask_shortcut_stdin,
        create_shortcut: prod_create,
    }
}

fn maybe_shortcut(out: &Out, game: &Path, offer: bool, direct: bool, hooks: &SetupHooks) {
    if !offer || !direct {
        return;
    }
    if !hooks.stdin_is_terminal || !(hooks.ask_shortcut)() {
        out.append("desktop shortcut: skipped");
        return;
    }
    let spec = shortcut::spec_for_game(game, &shortcut::desktop_dir());
    if let Some(err) = (hooks.create_shortcut)(&spec) {
        out.warn(&format!("Desktop shortcut: {err}"));
        return;
    }
    out.tick(&format!("Desktop shortcut: {}", shortcut::SHORTCUT_NAME));
}

pub(crate) fn run_in_game(game: &Path, yes: bool, no_shortcut: bool, force: bool, hooks: &SetupHooks) -> ExitCode {
    let out = Out::open(game);
    out.tick(&format!("Game: {}", game.display()));
    let store = store::kind(game);
    let direct = store::uses_direct_exe(game);
    if store == Store::GamePass {
        return out.fail(
            "Xbox Game Pass is not supported.",
            "Grid, slot and tooltip files in GAMEDATA\\MODS can still load. Setup stops here.",
        );
    }
    if store == Store::Gog {
        out.tick("GOG: Play starts Binaries\\NMS.exe");
    } else if direct {
        out.tick("No Steam: Play starts Binaries\\NMS.exe");
    }
    if (hooks.game_is_running)() {
        return out.fail("The game is running.", "Close No Man's Sky, then run setup again.");
    }
    match (hooks.install)(game, force) {
        Ok(msg) => out.tick(&msg),
        Err(e) => return out.fail("Could not install the proxy DLL.", &e),
    }
    let installed = paths::loader_dir(game).join("nms-rs.exe");
    if !direct {
        let options = (hooks.launch_options)();
        let leftovers = leftover_line(&options);
        match (hooks.apply_steam)(&installed, !yes) {
            Ok(lines) => {
                for line in &lines {
                    out.append(line);
                }
            }
            Err(e) => return out.fail("Could not set the Steam launch option.", &e),
        }
        if !(hooks.option_is_set)(&installed) {
            return out.fail(
                "Could not set the Steam launch option.",
                &format!(
                    "Set it by hand: Steam > No Man's Sky > Properties > Launch Options: {}",
                    steam::launch_option(&installed)
                ),
            );
        }
        out.tick("Steam launch option");
        out.tick(&format!("Old leftovers: {leftovers}"));
    } else {
        out.append(&format!("store={store:?}: skip Steam launch option"));
        out.tick("Old leftovers: none");
    }
    let (level, line) = files::game_build_status(game);
    if level != "ok" {
        out.warn(&line);
    } else {
        out.tick(&line);
    }
    files::write_conflict_report(game, None, &std::collections::BTreeMap::new(), true);
    let clashes = files::mbin_owners(game, false).into_iter().filter(|(_, names)| names.len() >= 2).count();
    if clashes > 0 {
        out.warn(&format!("{clashes} UI file(s) still clash. See NMS.rs\\conflicts.txt."));
    } else {
        out.tick("No leftover UI file clashes.");
    }
    maybe_shortcut(&out, game, !no_shortcut, direct, hooks);
    println!();
    if direct {
        println!("All set. Press Play in the Mod Manager, or the Desktop shortcut (not in GOG Galaxy).");
    } else {
        println!("All set. Press Play in Steam.");
    }
    out.append("All set.");
    ExitCode::SUCCESS
}

fn fail_no_log(what: &str, fix: &str) -> ExitCode {
    fail_print(what, fix);
    ExitCode::from(1)
}

fn extract_fix(game: &Path) -> String {
    format!(
        "Extract the zip into {}, then run nms-rs setup there (Steam: right-click No Man's Sky, Manage, Browse local files).",
        game.display()
    )
}

fn relocate_and_reexec(here: &Path, game: &Path, flags: &[&str]) -> ExitCode {
    let dest = paths::loader_dir(game);
    println!("  No Man's Sky is at {}", game.display());
    match copy_nms_rs(here, &dest) {
        Ok(()) => {}
        Err(e) => return fail_no_log("Could not find No Man's Sky.", &format!("Could not copy into the game folder ({e}).")),
    }
    let src_mods = here.parent().unwrap_or(here).join("GAMEDATA").join("MODS");
    let skipped = match copy_mods(&src_mods, &game.join("GAMEDATA").join("MODS")) {
        Ok(s) => s,
        Err(e) => return fail_no_log("Could not find No Man's Sky.", &format!("Could not copy into the game folder ({e}).")),
    };
    let out = Out::open(game);
    out.tick(&format!("Copied NMS.rs to {}", dest.display()));
    if !skipped.is_empty() {
        out.warn(&format!("Left {} (managed by Vortex).", skipped.join(", ")));
    }
    let child = dest.join("nms-rs.exe");
    if !child.is_file() {
        return out.fail("Could not find No Man's Sky.", &format!("Missing {}", child.display()));
    }
    let mut cmd = Command::new(&child);
    cmd.arg("setup");
    cmd.args(flags);
    match cmd.status() {
        Ok(st) => ExitCode::from(st.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(e) => out.fail("Could not find No Man's Sky.", &format!("Could not start {}: {e}", child.display())),
    }
}

pub(crate) fn cli(args: &[&str]) -> ExitCode {
    for a in args {
        if !matches!(*a, "--yes" | "--no-shortcut" | "--force") {
            eprintln!("{USAGE}");
            return ExitCode::from(1);
        }
    }
    let yes = args.iter().any(|a| *a == "--yes");
    let no_shortcut = args.iter().any(|a| *a == "--no-shortcut");
    let force = args.iter().any(|a| *a == "--force");
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return fail_no_log("Could not find this program.", &e.to_string()),
    };
    let here = match exe.parent() {
        Some(p) => p.to_path_buf(),
        None => return fail_no_log("Could not find this program.", "no exe folder"),
    };
    if let Some(game) = paths::find_game_root(&here) {
        return run_in_game(&game, yes, no_shortcut, force, &production_hooks());
    }
    let steam = match steam::steam_dir() {
        Ok(s) => s,
        Err(e) => return fail_no_log("Could not find No Man's Sky.", &e),
    };
    let game = match steam::nms_install_dir(&steam) {
        Ok(g) => g,
        Err(e) => return fail_no_log("Could not find No Man's Sky.", &e),
    };
    if !yes && !read_yes("  Copy NMS.rs and the mods there? [Y/N] ") {
        return fail_no_log("Could not find No Man's Sky.", &extract_fix(&game));
    }
    relocate_and_reexec(&here, &game, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEMP_N: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let n = TEMP_N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nms-rs-t025-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, body: &[u8]) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn never_running() -> bool {
        false
    }

    fn panic_install(_: &Path, _: bool) -> Result<String, String> {
        panic!("install must not run");
    }

    fn ok_install(root: &Path, _: bool) -> Result<String, String> {
        write(&root.join("installed.flag"), b"1");
        Ok("installed proxy".into())
    }

    fn panic_steam(_: &Path, _: bool) -> Result<Vec<String>, String> {
        panic!("steam must not run");
    }

    fn no_opts() -> Vec<String> {
        Vec::new()
    }

    fn never_set(_: &Path) -> bool {
        false
    }

    fn no_ask() -> bool {
        false
    }

    fn no_shortcut(_: &Spec) -> Option<String> {
        Some("shortcut must not run".into())
    }

    fn gog_hooks() -> SetupHooks {
        SetupHooks {
            game_is_running: never_running,
            install: ok_install,
            apply_steam: panic_steam,
            launch_options: no_opts,
            option_is_set: never_set,
            stdin_is_terminal: false,
            ask_shortcut: no_ask,
            create_shortcut: no_shortcut,
        }
    }

    fn game_pass_hooks() -> SetupHooks {
        SetupHooks {
            game_is_running: never_running,
            install: panic_install,
            apply_steam: panic_steam,
            launch_options: no_opts,
            option_is_set: never_set,
            stdin_is_terminal: false,
            ask_shortcut: no_ask,
            create_shortcut: no_shortcut,
        }
    }

    #[test]
    fn relocate_copies_loader_and_mods_keeping_json_and_logs() {
        let src_zip = temp_dir("src");
        let src_rs = src_zip.join("NMS.rs");
        write(&src_rs.join("nms-rs.exe"), b"new-exe");
        write(&src_rs.join("xinput9_1_0.dll"), b"dll");
        write(&src_rs.join("readme.txt"), b"hi");
        write(&src_rs.join("sizes.json"), b"from-zip");
        write(&src_rs.join("logs").join("old.log"), b"src-log");
        write(&src_zip.join("GAMEDATA").join("MODS").join("Foo").join("a.MBIN"), b"mbin");
        write(&src_zip.join("GAMEDATA").join("MODS").join("Foo").join("user.json"), b"new-json");
        write(&src_zip.join("GAMEDATA").join("MODS").join("VortexMod").join("new.MBIN"), b"v");

        let dest_game = temp_dir("dest");
        let dest_rs = dest_game.join("NMS.rs");
        write(&dest_rs.join("sizes.json"), b"user-json");
        write(&dest_rs.join("logs").join("keep.log"), b"keep");
        write(&dest_game.join("GAMEDATA").join("MODS").join("Foo").join("user.json"), b"old-json");
        write(
            &dest_game.join("GAMEDATA").join("MODS").join("VortexMod").join("__folder_managed_by_vortex"),
            b"",
        );
        write(&dest_game.join("GAMEDATA").join("MODS").join("VortexMod").join("old.MBIN"), b"old");

        copy_nms_rs(&src_rs, &dest_rs).unwrap();
        let skipped = copy_mods(&src_zip.join("GAMEDATA").join("MODS"), &dest_game.join("GAMEDATA").join("MODS")).unwrap();

        assert_eq!(std::fs::read(dest_rs.join("nms-rs.exe")).unwrap(), b"new-exe");
        assert_eq!(std::fs::read(dest_rs.join("readme.txt")).unwrap(), b"hi");
        assert_eq!(std::fs::read(dest_rs.join("sizes.json")).unwrap(), b"user-json");
        assert_eq!(std::fs::read(dest_rs.join("logs").join("keep.log")).unwrap(), b"keep");
        assert!(!dest_rs.join("logs").join("old.log").is_file());
        assert_eq!(std::fs::read(dest_game.join("GAMEDATA").join("MODS").join("Foo").join("user.json")).unwrap(), b"old-json");
        assert_eq!(std::fs::read(dest_game.join("GAMEDATA").join("MODS").join("Foo").join("a.MBIN")).unwrap(), b"mbin");
        assert_eq!(skipped, vec!["VortexMod".to_string()]);
        assert!(!dest_game.join("GAMEDATA").join("MODS").join("VortexMod").join("new.MBIN").is_file());
        assert!(dest_game.join("GAMEDATA").join("MODS").join("VortexMod").join("old.MBIN").is_file());

        let _ = std::fs::remove_dir_all(&src_zip);
        let _ = std::fs::remove_dir_all(&dest_game);
    }

    #[test]
    fn leftover_labels() {
        assert_eq!(leftover_line(&[]), "none");
        assert_eq!(
            leftover_line(&[r#""C:\mods\BiggerItemGrids.exe" %command%"#.into()]),
            "old 1.x launch option"
        );
        assert_eq!(
            leftover_line(&[r#""C:\mods\Play.bat" %command%"#.into()]),
            "old 1.x launch option"
        );
        assert_eq!(
            leftover_line(&[r#""D:\Games\Steam\steamapps\common\No Man's Sky\NMS.py\launch.cmd" %command%"#.into()]),
            r"old NMS.py\launch.cmd option"
        );
        let both = leftover_line(&[
            r#""C:\mods\BiggerItemGrids.exe" %command%"#.into(),
            r#""C:\NMS.py\launch.cmd" %command%"#.into(),
        ]);
        assert!(both.contains("old 1.x launch option"), "{both}");
        assert!(both.contains(r"old NMS.py\launch.cmd option"), "{both}");
    }

    #[test]
    fn game_pass_stops_before_install_or_steam() {
        let game = temp_dir("gp");
        write(&game.join("Binaries").join("NMS.exe"), b"fake");
        write(&game.join("appxmanifest.xml"), b"");
        write(&game.join("Binaries").join("xinput9_1_0.dll"), b"leave-me");
        let _code = run_in_game(&game, true, true, false, &game_pass_hooks());
        assert_eq!(std::fs::read(game.join("Binaries").join("xinput9_1_0.dll")).unwrap(), b"leave-me");
        assert!(!game.join("installed.flag").is_file());
        let log = std::fs::read_to_string(paths::loader_dir(&game).join("logs").join("setup.log")).unwrap();
        assert!(log.contains("Xbox Game Pass is not supported."), "{log}");
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn gog_skips_the_launch_option() {
        let game = temp_dir("gog");
        write(&game.join("Binaries").join("NMS.exe"), b"fake");
        write(&game.join("Binaries").join("Galaxy64.dll"), b"");
        let _code = run_in_game(&game, true, true, false, &gog_hooks());
        assert!(game.join("installed.flag").is_file());
        let log = std::fs::read_to_string(paths::loader_dir(&game).join("logs").join("setup.log")).unwrap();
        assert!(log.contains("skip Steam launch option"), "{log}");
        assert!(log.contains("GOG: Play starts Binaries\\NMS.exe"), "{log}");
        let _ = std::fs::remove_dir_all(&game);
    }
}
