//! Move a version 3 install (Python, `NMS.py`) over to NMS.rs: `nms-rs migrate [--yes]`, the
//! "Old NMS.py setup found" panel and the Settings section.
//!
//! Order: refuse while NMS.exe runs or `NMS.rs\stock` is missing; back up everything that will
//! change into `NMS.rs\backup\migrate-<time>`; carry NMS.py user files to NMS.rs; update our mods
//! (files the user edited stay); delete the Python leftovers; strip the old Steam launch option;
//! delete the zip-root Setup wrapper and, after the path checks, the NMS.py folder. A failed
//! backup, update or clean-up stops before anything else is deleted. Running it again finds
//! nothing and says so.

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nms_core::paths;

use crate::files;
use crate::i18n::I18n;
use crate::manager_settings as mset;
use crate::mods;
use crate::steam;

pub const NMS_PY: &str = "NMS.py";
/// The 3.x zip put a small wrapper next to Binaries that calls the copy inside NMS.py.
const WRAPPERS: &[&str] = &["NMS.py Setup.cmd", "NMS.py Setup.sh"];
const WRAPPER_MAX: u64 = 16 * 1024;
/// NMS.py root files the 3.x zip shipped, or logs: not user files, so not backed up.
const SHIPPED_ROOT: &[&str] = &["readme.txt", "python.txt", "conflicts.txt", "stock.json", "merge_order.example.txt"];
const SHIPPED_EXT: &[&str] = &["cmd", "sh", "py", "pyc", "pyd", "log", "md"];
/// NMS.py folders that were shipped or generated. Only the user files in them are backed up;
/// any other folder is backed up whole.
const KNOWN_DIRS: &[&str] = &[
    "cache", "custom", "extras", "lib", "locales", "logs", "mod_saves", "nmspywindow", "parked", "presets", "runtime",
    "stock", "tools", "__pycache__",
];
/// NMS.py files carried to NMS.rs when NMS.rs has none yet (same format in both).
const CARRY: &[&str] = &["merge_order.txt", "sizes.json"];
const USAGE: &str = "usage: nms-rs migrate [--yes]";

/// Everything that touches the PC outside the game folder, so tests can stand in for it.
pub struct Hooks<'a> {
    pub game_is_running: &'a dyn Fn() -> bool,
    pub launch_options: &'a dyn Fn() -> Vec<String>,
    pub strip_launch: &'a dyn Fn() -> Result<String, String>,
}

fn prod_strip() -> Result<String, String> {
    mods::strip_launch(false)
}

pub fn production() -> Hooks<'static> {
    Hooks {
        game_is_running: &files::game_is_running,
        launch_options: &steam::launch_option_strings,
        strip_launch: &prod_strip,
    }
}

/// What is left of version 3 in this game folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Legacy {
    pub nms_py: Option<PathBuf>,
    pub nms_py_bytes: u64,
    pub nms_py_files: usize,
    /// `.py`, `.py.off` and `.pyc` in our live and parked mod folders.
    pub python_files: Vec<PathBuf>,
    pub python_mods: Vec<String>,
    pub old_option: bool,
    pub wrappers: Vec<PathBuf>,
}

impl Legacy {
    pub fn found(&self) -> bool {
        self.nms_py.is_some() || !self.python_files.is_empty() || self.old_option || !self.wrappers.is_empty()
    }
}

fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

fn ext_lower(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn name_lower(path: &Path) -> String {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Files under `dir`, never following a symlink or junction (links are skipped).
fn walk_no_links(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if is_link(&meta) {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn in_pycache(path: &Path) -> bool {
    path.iter().any(|p| p == "__pycache__") || ext_lower(path) == "pyc"
}

/// The game folder's `NMS.py` entry with its name as stored on disk (`nms.py` stays `nms.py`).
fn find_nms_py(root: &Path) -> Option<PathBuf> {
    fs::read_dir(root)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(NMS_PY))
        .map(|e| root.join(e.file_name()))
}

fn wrappers(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for name in WRAPPERS {
        let path = root.join(name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || is_link(&meta) || meta.len() > WRAPPER_MAX {
            continue;
        }
        let text = String::from_utf8_lossy(&fs::read(&path).unwrap_or_default()).into_owned();
        if text.contains(NMS_PY) {
            out.push(path);
        }
    }
    out
}

fn mod_name_of(root: &Path, path: &Path) -> Option<String> {
    for base in [mods::mods_dir(root), mods::parked_dir(root)] {
        if let Ok(rel) = path.strip_prefix(&base) {
            return rel.iter().next().map(|s| s.to_string_lossy().into_owned());
        }
    }
    None
}

pub fn detect(root: &Path, hooks: &Hooks) -> Legacy {
    let nms_py = find_nms_py(root);
    let (mut bytes, mut count) = (0, 0);
    if let Some(dir) = &nms_py {
        if fs::symlink_metadata(dir).is_ok_and(|m| !is_link(&m) && m.is_dir()) {
            for file in walk_no_links(dir) {
                bytes += fs::symlink_metadata(&file).map(|m| m.len()).unwrap_or(0);
                count += 1;
            }
        }
    }
    let python_files = mods::python_leftovers(root);
    let mut python_mods: Vec<String> = python_files.iter().filter_map(|p| mod_name_of(root, p)).collect();
    python_mods.sort();
    python_mods.dedup();
    Legacy {
        nms_py,
        nms_py_bytes: bytes,
        nms_py_files: count,
        python_files,
        python_mods,
        old_option: steam::has_old_nms_py_option(&(hooks.launch_options)()),
        wrappers: wrappers(root),
    }
}

/// The NMS.py folder may be deleted only when it is a real folder (no symlink or junction), named
/// exactly `NMS.py`, directly inside a game folder, and looks like the version 3 layout. Returns
/// the canonical path, or the locale key of the reason.
pub(crate) fn check_nms_py(root: &Path, path: &Path) -> Result<PathBuf, &'static str> {
    if !root.join("Binaries").join("NMS.exe").is_file() {
        return Err("mig_why_game_root");
    }
    if path.file_name().and_then(|n| n.to_str()) != Some(NMS_PY) {
        return Err("mig_why_name");
    }
    let meta = fs::symlink_metadata(path).map_err(|_| "mig_why_layout")?;
    if is_link(&meta) {
        return Err("mig_why_link");
    }
    if !meta.is_dir() {
        return Err("mig_why_layout");
    }
    let root_real = fs::canonicalize(root).map_err(|_| "mig_why_place")?;
    let real = fs::canonicalize(path).map_err(|_| "mig_why_place")?;
    if real.parent() != Some(root_real.as_path()) {
        return Err("mig_why_place");
    }
    // The canonical name is the one on disk, so a case-only twin (`nms.py`) is refused here too.
    if real.file_name().and_then(|n| n.to_str()) != Some(NMS_PY) {
        return Err("mig_why_name");
    }
    if !(real.join("stock").is_dir() || real.join("runtime").is_dir()) {
        return Err("mig_why_layout");
    }
    if ["Binaries", "GAMEDATA", "NMS.rs"].iter().any(|n| real.join(n).exists()) {
        return Err("mig_why_layout");
    }
    Ok(real)
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (fs::read(a), fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Files (absolute, under `root`) the migration may change or delete and that the user may own.
fn backup_list(root: &Path, py: Option<&Path>, legacy: &Legacy) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let stock = mods::stock_dir(root);
    for name in mods::our_names(root) {
        let mut folders = vec![mods::mods_dir(root).join(&name), mods::parked_dir(root).join(&name)];
        if let Some(py) = py {
            folders.push(py.join("parked").join(&name));
        }
        for folder in folders {
            for file in walk_no_links(&folder) {
                if in_pycache(&file) {
                    continue;
                }
                let Ok(rel) = file.strip_prefix(&folder) else {
                    continue;
                };
                let stock_copy = stock.join(&name).join(rel);
                let changes = stock_copy.is_file() && !same_bytes(&file, &stock_copy);
                if mods::is_python_file(&file) || ext_lower(&file) == "json" || changes {
                    out.push(file);
                }
            }
        }
    }
    if let Some(py) = py {
        out.extend(nms_py_user_files(py));
    }
    out.extend(legacy.wrappers.iter().cloned());
    out.sort();
    out.dedup();
    out
}

/// User files inside NMS.py: settings and lists at the top, parked mods, pymhf saves, the
/// window's json, size templates that differ from stock, and anything we never shipped.
fn nms_py_user_files(py: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(py) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if is_link(&meta) {
            continue;
        }
        let name = name_lower(&path);
        if !meta.is_dir() {
            if !SHIPPED_EXT.contains(&ext_lower(&path).as_str()) && !SHIPPED_ROOT.contains(&name.as_str()) {
                out.push(path);
            }
            continue;
        }
        match name.as_str() {
            "parked" | "mod_saves" => out.extend(walk_no_links(&path).into_iter().filter(|f| !in_pycache(f))),
            "nmspywindow" => out.extend(walk_no_links(&path).into_iter().filter(|f| ext_lower(f) == "json")),
            "custom" => {
                let templates = path.join("mods");
                for file in walk_no_links(&templates) {
                    let Ok(rel) = file.strip_prefix(&templates) else {
                        continue;
                    };
                    if in_pycache(&file) || same_bytes(&file, &py.join("stock").join(rel)) {
                        continue;
                    }
                    out.push(file);
                }
            }
            known if KNOWN_DIRS.contains(&known) => {}
            _ => out.extend(walk_no_links(&path)),
        }
    }
    out
}

fn backup_dir(root: &Path) -> PathBuf {
    let stamp: String = crate::setup::now_stamp()
        .chars()
        .filter_map(|c| match c {
            '-' | ':' => None,
            ' ' => Some('-'),
            c => Some(c),
        })
        .collect();
    let base = paths::loader_dir(root).join("backup");
    let mut dir = base.join(format!("migrate-{stamp}"));
    let mut n = 2;
    while dir.exists() {
        dir = base.join(format!("migrate-{stamp}-{n}"));
        n += 1;
    }
    dir
}

fn rel_text(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn copy_to_backup(root: &Path, dir: &Path, list: &[PathBuf]) -> Result<(), String> {
    for file in list {
        let rel = file.strip_prefix(root).map_err(|_| format!("{} is outside the game folder", file.display()))?;
        let dest = dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::copy(file, &dest).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

fn write_manifest(dir: &Path, root: &Path, options: &[String], list: &[PathBuf], outcome: &str) -> Result<(), String> {
    let mut text = format!(
        "NMS.rs migrate backup\nGame folder: {}\nTo undo: copy every folder here back into the game folder (same places).\n\nSteam launch options before:\n",
        root.display()
    );
    for option in options {
        text.push_str(&format!("  {option}\n"));
    }
    text.push_str(&format!("\nBacked up ({}):\n", list.len()));
    for file in list {
        text.push_str(&format!("  {}\n", rel_text(root, file)));
    }
    if !outcome.is_empty() {
        text.push_str(&format!("\nResult:\n{outcome}\n"));
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("manifest.txt"), text).map_err(|e| e.to_string())
}

/// NMS.py files that NMS.rs reads too, copied over when NMS.rs has none yet.
fn carry(root: &Path, py: &Path) -> Vec<String> {
    let mut done = Vec::new();
    let loader = paths::loader_dir(root);
    let _ = fs::create_dir_all(&loader);
    for name in CARRY {
        let (src, dest) = (py.join(name), loader.join(name));
        if src.is_file() && !dest.exists() && fs::copy(&src, &dest).is_ok() {
            done.push((*name).to_string());
        }
    }
    if !loader.join(mset::FILE).exists() && py.join(mset::FILE).is_file() {
        // `load` reads the NMS.py language while NMS.rs has no manager.json; this keeps it.
        mset::save(root, &mset::load(root));
        done.push(mset::FILE.to_string());
    }
    let parked = py.join("parked");
    let Ok(entries) = fs::read_dir(&parked) else {
        return done;
    };
    for entry in entries.flatten() {
        let src = entry.path();
        if !fs::symlink_metadata(&src).is_ok_and(|m| m.is_dir() && !is_link(&m)) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let dest = mods::parked_dir(root).join(&name);
        if dest.exists() || mods::mods_dir(root).join(&name).exists() {
            continue;
        }
        let _ = fs::create_dir_all(mods::parked_dir(root));
        if fs::rename(&src, &dest).is_ok() {
            done.push(format!("parked\\{name}"));
        }
    }
    done
}

fn remove_file_force(path: &Path) -> bool {
    if fs::remove_file(path).is_ok() {
        return true;
    }
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(path, perms);
    }
    fs::remove_file(path).is_ok()
}

/// Delete `dir` and everything in it. A symlink or junction inside is removed itself; what it
/// points at is never entered. Whatever could not be deleted is pushed to `failed`.
fn delete_tree(dir: &Path, failed: &mut Vec<PathBuf>) {
    let before = failed.len();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                failed.push(path);
                continue;
            };
            if is_link(&meta) {
                if fs::remove_dir(&path).is_err() && fs::remove_file(&path).is_err() {
                    failed.push(path);
                }
            } else if meta.is_dir() {
                delete_tree(&path, failed);
            } else if !remove_file_force(&path) {
                failed.push(path);
            }
        }
    }
    if fs::remove_dir(dir).is_err() && failed.len() == before && dir.exists() {
        failed.push(dir.to_path_buf());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Done,
    Kept,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub mark: Mark,
    pub key: &'static str,
    pub detail: String,
    /// The detail is itself a locale key (a reason).
    pub detail_key: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub lines: Vec<Line>,
    pub ok: bool,
    pub backup: Option<PathBuf>,
}

impl Report {
    fn push(&mut self, mark: Mark, key: &'static str, detail: impl Into<String>) {
        self.lines.push(Line { mark, key, detail: detail.into(), detail_key: None });
    }

    fn push_reason(&mut self, mark: Mark, key: &'static str, reason: &'static str) {
        self.lines.push(Line { mark, key, detail: String::new(), detail_key: Some(reason) });
    }

    #[cfg(test)]
    pub fn has(&self, key: &str) -> bool {
        self.lines.iter().any(|l| l.key == key)
    }

    /// One line per step: `+` done, `=` kept, `!` warning, `x` failed.
    pub fn render(&self, i18n: &I18n) -> String {
        self.lines
            .iter()
            .map(|line| {
                let mark = match line.mark {
                    Mark::Done => "+",
                    Mark::Kept => "=",
                    Mark::Warn => "!",
                    Mark::Fail => "x",
                };
                let label = i18n.t(line.key);
                let detail = line.detail_key.map(|k| i18n.t(k)).unwrap_or_else(|| line.detail.clone());
                if label.contains("{path}") {
                    format!("{mark} {}", label.replace("{path}", &detail))
                } else if detail.is_empty() {
                    format!("{mark} {label}")
                } else {
                    format!("{mark} {label}: {detail}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn fmt_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else if mb >= 0.1 {
        format!("{mb:.1} MB")
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// The "will be removed" list for the panel and the console, in the user's language.
pub fn plan_lines(legacy: &Legacy, i18n: &I18n) -> Vec<String> {
    let mut lines = Vec::new();
    if legacy.nms_py.is_some() {
        lines.push(
            i18n.t("mig_item_folder")
                .replace("{size}", &fmt_size(legacy.nms_py_bytes))
                .replace("{files}", &legacy.nms_py_files.to_string()),
        );
    }
    if !legacy.python_files.is_empty() {
        lines.push(
            i18n.t("mig_item_python")
                .replace("{count}", &legacy.python_files.len().to_string())
                .replace("{names}", &legacy.python_mods.join(", ")),
        );
    }
    if legacy.old_option {
        lines.push(i18n.t("mig_item_option"));
    }
    for wrapper in &legacy.wrappers {
        let name = wrapper.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        lines.push(i18n.t("mig_item_wrapper").replace("{name}", &name));
    }
    lines
}

fn short_list(items: &[String], limit: usize) -> String {
    if items.len() <= limit {
        items.join(", ")
    } else {
        format!("{} (+{})", items[..limit].join(", "), items.len() - limit)
    }
}

pub fn migrate(root: &Path, hooks: &Hooks) -> Report {
    let mut report = Report::default();
    let legacy = detect(root, hooks);
    if !legacy.found() {
        report.push(Mark::Done, "mig_nothing", "");
        report.ok = true;
        return report;
    }
    if (hooks.game_is_running)() {
        report.push(Mark::Fail, "close_game", "");
        return report;
    }
    if !paths::loader_dir(root).join("stock").is_dir() {
        report.push(Mark::Fail, "mig_no_stock", "");
        return report;
    }
    // Everything inside NMS.py (backup, carry, delete) needs the path checks first.
    let py_check = legacy.nms_py.as_ref().map(|p| check_nms_py(root, p));
    let py = match &py_check {
        Some(Ok(_)) => Some(root.join(NMS_PY)),
        _ => None,
    };

    // 1. Backup. Nothing has changed yet, so a failure here stops cleanly.
    let options = (hooks.launch_options)();
    let list = backup_list(root, py.as_deref(), &legacy);
    let dir = backup_dir(root);
    if let Err(e) = copy_to_backup(root, &dir, &list).and_then(|()| write_manifest(&dir, root, &options, &list, "")) {
        report.push(Mark::Fail, "mig_backup_failed", e);
        return report;
    }
    report.push(Mark::Done, "mig_backup", format!("{} ({})", rel_text(root, &dir), list.len()));
    report.backup = Some(dir.clone());

    // 2. NMS.py files NMS.rs also reads.
    if let Some(py) = &py {
        let carried = carry(root, py);
        if !carried.is_empty() {
            report.push(Mark::Done, "mig_carried", carried.join(", "));
        }
    }

    // 3. Update our mods; put back the files the user edited (the Mods tab still offers Update).
    let edited: Vec<(PathBuf, Vec<u8>)> = py
        .as_ref()
        .map(|py| mods::edited_files(root, &py.join("stock")))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| fs::read(&p).ok().map(|b| (p, b)))
        .collect();
    match mods::update_all(root, false) {
        Ok(msg) => report.push(Mark::Done, "mig_updated", msg.replace('\n', " ")),
        Err(e) => {
            report.push(Mark::Fail, "mig_updated", e);
            return finish(root, report, &options, &list, "mig_stopped");
        }
    }
    if !edited.is_empty() {
        let mut names = Vec::new();
        for (path, bytes) in &edited {
            if fs::write(path, bytes).is_ok() {
                names.push(rel_text(root, path));
            }
        }
        report.push(Mark::Kept, "mig_kept_edited", short_list(&names, 4));
    }
    let _ = mods::auto_install(root);

    // 4. Python leftovers in our mod folders (PulseTweaks and other people's mods stay).
    match mods::cleanup_python(root, false) {
        Ok(text) => {
            let n = text.lines().filter(|l| l.starts_with("deleted ")).count();
            if n > 0 {
                report.push(Mark::Done, "mig_python", n.to_string());
            }
        }
        Err(e) => {
            report.push(Mark::Fail, "mig_python", e);
            return finish(root, report, &options, &list, "mig_stopped");
        }
    }

    // 5. The old Steam launch option. While Steam still points at NMS.py, NMS.py stays.
    let mut steam_ok = true;
    if legacy.old_option {
        match (hooks.strip_launch)() {
            Ok(text) => {
                let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                report.push(Mark::Done, "mig_steam", lines.join("; "));
            }
            Err(e) => {
                report.push(Mark::Fail, "mig_steam_failed", e);
                steam_ok = false;
            }
        }
        if steam_ok && steam::has_old_nms_py_option(&(hooks.launch_options)()) {
            report.push(Mark::Fail, "mig_steam_failed", "");
            steam_ok = false;
        }
    }

    // 6. The Setup wrapper next to Binaries.
    for wrapper in &legacy.wrappers {
        let name = rel_text(root, wrapper);
        if remove_file_force(wrapper) {
            report.push(Mark::Done, "mig_deleted", name);
        } else {
            report.push(Mark::Fail, "mig_not_deleted", name);
        }
    }

    // 7. The NMS.py folder.
    let mut all_ok = steam_ok;
    match py_check {
        None => {}
        Some(Err(reason)) => {
            report.push_reason(Mark::Fail, "mig_py_refused", reason);
            all_ok = false;
        }
        Some(Ok(_)) if !steam_ok => report.push(Mark::Warn, "mig_py_kept_steam", ""),
        Some(Ok(real)) => {
            let mut failed = Vec::new();
            delete_tree(&real, &mut failed);
            if failed.is_empty() {
                report.push(Mark::Done, "mig_py_removed", fmt_size(legacy.nms_py_bytes));
            } else {
                let names: Vec<String> = failed.iter().map(|p| rel_text(root, &un_verbatim(p))).collect();
                report.push(Mark::Fail, "mig_py_partial", short_list(&names, 5));
                all_ok = false;
            }
        }
    }
    report.push(Mark::Kept, "mig_kept", "");
    report.ok = all_ok;
    finish(root, report, &options, &list, if all_ok { "mig_done" } else { "mig_partly" })
}

/// `\\?\F:\x` from `canonicalize` back to `F:\x`, so it strips against the game folder.
fn un_verbatim(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

fn finish(root: &Path, mut report: Report, options: &[String], list: &[PathBuf], last: &'static str) -> Report {
    report.push(if report.ok { Mark::Done } else { Mark::Warn }, last, "");
    if let Some(dir) = report.backup.clone() {
        report.push(Mark::Done, "mig_undo", rel_text(root, &dir));
        let english = report.render(&I18n::new("en"));
        let _ = write_manifest(&dir, root, options, list, &english);
        log(root, &english);
    }
    report
}

fn log(root: &Path, text: &str) {
    let dir = paths::loader_dir(root).join("logs");
    let _ = fs::create_dir_all(&dir);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("manager.log")) {
        let _ = writeln!(f, "{} migrate\n{text}", crate::setup::now_stamp());
    }
}

pub fn cli(root: &Path, args: &[&str]) -> ExitCode {
    if args.iter().any(|a| *a != "--yes") {
        eprintln!("{USAGE}");
        return ExitCode::from(1);
    }
    let yes = !args.is_empty();
    let i18n = I18n::new(&mset::load(root).language);
    let hooks = production();
    let legacy = detect(root, &hooks);
    if !legacy.found() {
        println!("{}", i18n.t("mig_nothing"));
        return ExitCode::SUCCESS;
    }
    println!("{}", i18n.t("mig_panel_title"));
    println!("{}", i18n.t("mig_remove_head"));
    for line in plan_lines(&legacy, &i18n) {
        println!("  - {line}");
    }
    println!("{}", i18n.t("mig_keep_head"));
    println!("  {}", i18n.t("mig_keep_text"));
    if !yes {
        if !io::stdin().is_terminal() {
            eprintln!("{USAGE} (no terminal: add --yes)");
            return ExitCode::from(1);
        }
        print!("{} [Y/N] ", i18n.t("mig_button"));
        let _ = io::stdout().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err() || !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return ExitCode::from(1);
        }
    }
    let report = migrate(root, &hooks);
    println!("{}", report.render(&i18n));
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEMP_N: AtomicU32 = AtomicU32::new(0);
    const OLD_OPTION: &str = r#""C:\Games\No Man's Sky\NMS.py\launch.cmd" %command%"#;

    fn write(path: &Path, body: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn temp(tag: &str) -> PathBuf {
        let n = TEMP_N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nms-rs-migrate-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A 3.0.4-shaped install with the 4.0 zip extracted over it (built here, never the real game).
    fn fixture(tag: &str) -> PathBuf {
        let root = temp(tag);
        write(&root.join("Binaries").join("NMS.exe"), "fake");
        let mods = root.join("GAMEDATA").join("MODS");
        let rs = root.join("NMS.rs");
        let py = root.join(NMS_PY);
        // 4.0 stock.
        write(&rs.join("stock").join("BiggerItemGrids").join("BiggerItemGrids.lua"), "-- big v4\n");
        write(&rs.join("stock").join("QuickTransferAll").join("QuickDeposit.lua"), "-- qd v4\n");
        write(&rs.join("stock").join("NMSrsMenu").join("NMSrsMenu.lua"), "-- menu\n");
        write(&rs.join("stock").join("stock.json"), "{\"BiggerItemGrids\": \"4.0.0\", \"QuickTransferAll\": \"4.0.0\", \"CraftFromStorage\": \"4.0.0\", \"NMSrsMenu\": \"4.0.0\"}\n");
        // Old NMS.py.
        write(&py.join("runtime").join("python.exe"), "py");
        write(&py.join("runtime").join("Lib").join("os.py"), "py");
        write(&py.join("lib").join("launcher.py"), "py");
        write(&py.join("launch.cmd"), "@echo off\n");
        write(&py.join("launcher.log"), "log\n");
        write(&py.join("README.txt"), "readme\n");
        write(&py.join("manager.json"), "{\"language\": \"de\"}\n");
        write(&py.join("merge_order.txt"), "OtherPersonsMod\n");
        write(&py.join("my notes.txt"), "mine\n");
        write(&py.join("mod_saves").join("save.json"), "{}\n");
        write(&py.join("NMSpyWindow").join("nmspy_window.json"), "{\"key\": \"F1\"}\n");
        write(&py.join("NMSpyWindow").join("nmspy_window.py"), "py");
        write(&py.join("stock").join("BiggerItemGrids").join("BiggerItemGrids.lua"), "-- big v3\n");
        write(&py.join("stock").join("QuickTransferAll").join("QuickDeposit.lua"), "-- qd v3\n");
        write(&py.join("stock").join("QuickTransferAll").join("quick_transfer_all.py"), "py");
        write(&py.join("parked").join("CraftFromStorage").join("craft_from_storage.json"), "{\"anywhere\": true}\n");
        write(&py.join("parked").join("CraftFromStorage").join("craft_from_storage.py"), "py");
        // Live mods: ours with Python and settings, one lua the user edited, other people's mods.
        let big = mods.join("BiggerItemGrids");
        write(&big.join("bigger_item_grids.py"), "__version__ = \"3.0.4\"\n");
        write(&big.join("bigger_item_grids.json"), "{\"write_logs\": true}\n");
        write(&big.join("BiggerItemGrids.lua"), "-- big v3\n");
        write(&big.join("__pycache__").join("bigger_item_grids.cpython-310.pyc"), "pyc");
        let qta = mods.join("QuickTransferAll");
        write(&qta.join("quick_transfer_all.py"), "py");
        write(&qta.join("quick_transfer_all.json"), "{\"key\": \"ctrl\", \"deposit\": false}\n");
        write(&qta.join("suit_lock_fe170b785039cfd4.json"), "{\"pins\": []}\n");
        write(&qta.join("QuickDeposit.lua"), "-- qd v3 with my edit\n");
        write(&mods.join("PulseTweaks").join("pulse_tweaks.py"), "keep");
        write(&mods.join("OtherPersonsMod").join("their.py"), "theirs");
        write(&mods.join("OtherPersonsMod").join("__pycache__").join("their.cpython-310.pyc"), "theirs");
        write(&mods.join("OtherPersonsMod").join("their.json"), "{}");
        write(&root.join("NMS.py Setup.cmd"), "@echo off\ncall \"%~dp0NMS.py\\NMS.py Setup.cmd\" %*\n");
        // The 3.x files are older than the 4.0 zip (Update only replaces older files).
        let year_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(365 * 24 * 3600);
        for file in walk_no_links(&mods) {
            fs::File::options().write(true).open(&file).unwrap().set_modified(year_ago).unwrap();
        }
        root
    }

    fn not_running() -> bool {
        false
    }
    fn running() -> bool {
        true
    }
    fn no_options() -> Vec<String> {
        Vec::new()
    }
    fn must_not_strip() -> Result<String, String> {
        panic!("the launch option must not be touched")
    }

    fn quiet() -> Hooks<'static> {
        Hooks { game_is_running: &not_running, launch_options: &no_options, strip_launch: &must_not_strip }
    }

    fn backup_of(report: &Report) -> PathBuf {
        report.backup.clone().expect("backup made")
    }

    #[test]
    fn detects_the_old_layout() {
        let root = fixture("detect");
        let options = || vec![OLD_OPTION.to_string()];
        let hooks = Hooks { game_is_running: &not_running, launch_options: &options, strip_launch: &must_not_strip };
        let legacy = detect(&root, &hooks);
        assert!(legacy.found());
        assert_eq!(legacy.nms_py.as_deref(), Some(root.join(NMS_PY).as_path()));
        assert!(legacy.nms_py_files >= 15, "{legacy:?}");
        assert!(legacy.nms_py_bytes > 0);
        assert!(legacy.old_option);
        assert_eq!(legacy.python_mods, vec!["BiggerItemGrids".to_string(), "QuickTransferAll".to_string()]);
        assert_eq!(legacy.python_files.len(), 3, "two .py and one .pyc: {:?}", legacy.python_files);
        assert_eq!(legacy.wrappers, vec![root.join("NMS.py Setup.cmd")]);
        let lines = plan_lines(&legacy, &I18n::new("en"));
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines[1].contains("BiggerItemGrids, QuickTransferAll"), "{lines:?}");
        // A clean 4.0 folder has nothing to migrate.
        let clean = temp("clean");
        write(&clean.join("Binaries").join("NMS.exe"), "fake");
        assert!(!detect(&clean, &quiet()).found());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&clean);
    }

    #[test]
    fn migrates_keeps_user_files_and_is_idempotent() {
        let root = fixture("full");
        let option = RefCell::new(vec![OLD_OPTION.to_string(), "-skipintro".to_string()]);
        let strips = Cell::new(0);
        let options = || option.borrow().clone();
        let strip = || -> Result<String, String> {
            strips.set(strips.get() + 1);
            option.borrow_mut().retain(|o| !o.contains("NMS.py"));
            Ok("Steam account 111: launch option removed.".to_string())
        };
        let hooks = Hooks { game_is_running: &not_running, launch_options: &options, strip_launch: &strip };
        let report = migrate(&root, &hooks);
        let text = report.render(&I18n::new("en"));
        assert!(report.ok, "{text}");
        assert_eq!(strips.get(), 1);
        let mods = root.join("GAMEDATA").join("MODS");
        let rs = root.join("NMS.rs");
        // Removed.
        assert!(!root.join(NMS_PY).exists(), "{text}");
        assert!(!root.join("NMS.py Setup.cmd").exists());
        assert!(!mods.join("BiggerItemGrids").join("bigger_item_grids.py").exists());
        assert!(!mods.join("BiggerItemGrids").join("__pycache__").exists());
        assert!(!mods.join("QuickTransferAll").join("quick_transfer_all.py").exists());
        // Kept in place: settings, the edited lua, PulseTweaks and other people's mods.
        assert_eq!(read(&mods.join("BiggerItemGrids").join("bigger_item_grids.json")), "{\"write_logs\": true}\n");
        assert_eq!(read(&mods.join("QuickTransferAll").join("quick_transfer_all.json")), "{\"key\": \"ctrl\", \"deposit\": false}\n");
        assert_eq!(read(&mods.join("QuickTransferAll").join("suit_lock_fe170b785039cfd4.json")), "{\"pins\": []}\n");
        assert_eq!(read(&mods.join("QuickTransferAll").join("QuickDeposit.lua")), "-- qd v3 with my edit\n");
        assert!(report.has("mig_kept_edited"), "{text}");
        assert_eq!(read(&mods.join("PulseTweaks").join("pulse_tweaks.py")), "keep");
        assert_eq!(read(&mods.join("OtherPersonsMod").join("their.py")), "theirs");
        assert!(mods.join("OtherPersonsMod").join("__pycache__").join("their.cpython-310.pyc").is_file());
        // Updated: the unedited old stock lua is now the 4.0 one; NMSrsMenu was auto-installed.
        assert_eq!(read(&mods.join("BiggerItemGrids").join("BiggerItemGrids.lua")), "-- big v4\n");
        assert!(mods.join("NMSrsMenu").join("NMSrsMenu.lua").is_file());
        // Carried to NMS.rs.
        assert_eq!(read(&rs.join("merge_order.txt")), "OtherPersonsMod\n");
        assert_eq!(mset::load(&root).language, "de");
        let parked = rs.join("parked").join("CraftFromStorage");
        assert_eq!(read(&parked.join("craft_from_storage.json")), "{\"anywhere\": true}\n");
        assert!(!parked.join("craft_from_storage.py").exists(), "cleanup also covers parked mods");
        // Backup mirrors the game folder; the runtime and logs are not in it.
        let backup = backup_of(&report);
        assert!(backup.starts_with(rs.join("backup")));
        for rel in [
            "GAMEDATA/MODS/BiggerItemGrids/bigger_item_grids.py",
            "GAMEDATA/MODS/BiggerItemGrids/bigger_item_grids.json",
            "GAMEDATA/MODS/BiggerItemGrids/BiggerItemGrids.lua",
            "GAMEDATA/MODS/QuickTransferAll/QuickDeposit.lua",
            "GAMEDATA/MODS/QuickTransferAll/quick_transfer_all.json",
            "GAMEDATA/MODS/QuickTransferAll/suit_lock_fe170b785039cfd4.json",
            "NMS.py/manager.json",
            "NMS.py/merge_order.txt",
            "NMS.py/my notes.txt",
            "NMS.py/mod_saves/save.json",
            "NMS.py/NMSpyWindow/nmspy_window.json",
            "NMS.py/parked/CraftFromStorage/craft_from_storage.json",
            "NMS.py/parked/CraftFromStorage/craft_from_storage.py",
            "NMS.py Setup.cmd",
        ] {
            assert!(backup.join(rel).is_file(), "missing {rel} in backup");
        }
        for rel in ["NMS.py/runtime/python.exe", "NMS.py/launcher.log", "NMS.py/README.txt", "GAMEDATA/MODS/OtherPersonsMod/their.json"] {
            assert!(!backup.join(rel).exists(), "{rel} should not be backed up");
        }
        let manifest = read(&backup.join("manifest.txt"));
        assert!(manifest.contains(OLD_OPTION) && manifest.contains("Result:"), "{manifest}");
        for key in ["mig_backup", "mig_carried", "mig_updated", "mig_python", "mig_steam", "mig_deleted", "mig_py_removed", "mig_done", "mig_undo"] {
            assert!(report.has(key), "{key} missing:\n{text}");
        }
        // Second run: nothing to do, nothing touched.
        let again = migrate(&root, &hooks);
        assert!(again.ok && again.has("mig_nothing") && again.backup.is_none(), "{}", again.render(&I18n::new("en")));
        assert_eq!(strips.get(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_while_the_game_runs() {
        let root = fixture("running");
        let hooks = Hooks { game_is_running: &running, launch_options: &no_options, strip_launch: &must_not_strip };
        let report = migrate(&root, &hooks);
        assert!(!report.ok && report.has("close_game"));
        assert!(root.join(NMS_PY).join("runtime").join("python.exe").is_file());
        assert!(root.join("GAMEDATA/MODS/BiggerItemGrids/bigger_item_grids.py").is_file());
        assert!(!root.join("NMS.rs").join("backup").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_without_nms_rs_stock() {
        let root = fixture("nostock");
        fs::remove_dir_all(root.join("NMS.rs").join("stock")).unwrap();
        let report = migrate(&root, &quiet());
        assert!(!report.ok && report.has("mig_no_stock"));
        assert!(root.join(NMS_PY).is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn steam_failure_keeps_nms_py() {
        let root = fixture("steamfail");
        let options = || vec![OLD_OPTION.to_string()];
        let fail = || -> Result<String, String> { Err("Steam did not close".to_string()) };
        let hooks = Hooks { game_is_running: &not_running, launch_options: &options, strip_launch: &fail };
        let report = migrate(&root, &hooks);
        assert!(!report.ok);
        assert!(report.has("mig_steam_failed") && report.has("mig_py_kept_steam"));
        assert!(root.join(NMS_PY).join("runtime").is_dir(), "Steam still points at it");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn path_checks_refuse_anything_but_the_old_folder() {
        let root = fixture("checks");
        let py = root.join(NMS_PY);
        assert!(check_nms_py(&root, &py).is_ok());
        // Not directly in the game folder.
        let deeper = root.join("sub").join(NMS_PY);
        write(&deeper.join("stock").join("a.txt"), "x");
        assert_eq!(check_nms_py(&root, &deeper), Err("mig_why_place"));
        let outside = temp("outside").join(NMS_PY);
        write(&outside.join("stock").join("a.txt"), "x");
        assert_eq!(check_nms_py(&root, &outside), Err("mig_why_place"));
        // Wrong name.
        assert_eq!(check_nms_py(&root, &root.join("NMS.py.old")), Err("mig_why_name"));
        // Not the old layout.
        fs::remove_dir_all(py.join("stock")).unwrap();
        fs::remove_dir_all(py.join("runtime")).unwrap();
        assert_eq!(check_nms_py(&root, &py), Err("mig_why_layout"));
        write(&py.join("runtime").join("python.exe"), "py");
        write(&py.join("GAMEDATA").join("x"), "x");
        assert_eq!(check_nms_py(&root, &py), Err("mig_why_layout"));
        fs::remove_dir_all(py.join("GAMEDATA")).unwrap();
        // No game folder.
        fs::remove_file(root.join("Binaries").join("NMS.exe")).unwrap();
        assert_eq!(check_nms_py(&root, &py), Err("mig_why_game_root"));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(outside.parent().unwrap());
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let status = nms_core::spawn::command("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(status.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&status.stdout));
    }

    #[cfg(windows)]
    #[test]
    fn junction_nms_py_is_refused_and_its_target_survives() {
        let root = fixture("junction");
        let elsewhere = temp("elsewhere");
        write(&elsewhere.join("stock").join("keep.txt"), "precious");
        write(&elsewhere.join("runtime").join("python.exe"), "precious");
        fs::remove_dir_all(root.join(NMS_PY)).unwrap();
        junction(&root.join(NMS_PY), &elsewhere);
        assert_eq!(check_nms_py(&root, &root.join(NMS_PY)), Err("mig_why_link"));
        let report = migrate(&root, &quiet());
        assert!(!report.ok && report.has("mig_py_refused"), "{}", report.render(&I18n::new("en")));
        assert_eq!(read(&elsewhere.join("stock").join("keep.txt")), "precious");
        assert!(fs::symlink_metadata(root.join(NMS_PY)).is_ok(), "the junction itself is left");
        // Nothing was read through the junction into the backup either.
        assert!(!backup_of(&report).join(NMS_PY).exists());
        let _ = fs::remove_dir(root.join(NMS_PY));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&elsewhere);
    }

    #[cfg(windows)]
    #[test]
    fn junction_inside_nms_py_is_unlinked_not_followed() {
        let root = fixture("innerlink");
        let elsewhere = temp("inner-target");
        write(&elsewhere.join("keep.txt"), "precious");
        junction(&root.join(NMS_PY).join("runtime").join("Lib").join("linked"), &elsewhere);
        let report = migrate(&root, &quiet());
        assert!(report.ok, "{}", report.render(&I18n::new("en")));
        assert!(!root.join(NMS_PY).exists());
        assert_eq!(read(&elsewhere.join("keep.txt")), "precious");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&elsewhere);
    }

    #[cfg(windows)]
    #[test]
    fn lowercase_twin_is_refused() {
        let root = fixture("case");
        fs::rename(root.join(NMS_PY), root.join("nms.py")).unwrap();
        let found = find_nms_py(&root).unwrap();
        assert_eq!(found.file_name().unwrap(), "nms.py");
        assert_eq!(check_nms_py(&root, &found), Err("mig_why_name"));
        // Through the exact name too: canonicalize reports the name on disk.
        assert_eq!(check_nms_py(&root, &root.join(NMS_PY)), Err("mig_why_name"));
        let report = migrate(&root, &quiet());
        assert!(report.has("mig_py_refused") && root.join("nms.py").is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn file_in_use_is_reported() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = fixture("inuse");
        let locked = root.join(NMS_PY).join("runtime").join("python.exe");
        let handle = fs::OpenOptions::new().read(true).share_mode(0).open(&locked).unwrap();
        let report = migrate(&root, &quiet());
        let text = report.render(&I18n::new("en"));
        assert!(!report.ok && report.has("mig_py_partial"), "{text}");
        assert!(text.contains("python.exe"), "{text}");
        assert!(!root.join(NMS_PY).join("lib").exists(), "the rest is gone");
        drop(handle);
        // Once the file is free, running again finishes the job.
        let again = migrate(&root, &quiet());
        assert!(again.ok && !root.join(NMS_PY).exists(), "{}", again.render(&I18n::new("en")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn every_locale_has_the_migrate_strings() {
        let english: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(crate::i18n::locale_texts()[0].1).unwrap();
        let keys: Vec<&String> = english.keys().filter(|k| k.starts_with("mig_")).collect();
        assert!(keys.len() >= 30, "{keys:?}");
        for (code, text) in crate::i18n::locale_texts() {
            let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).unwrap();
            for key in &keys {
                let value = map.get(*key).and_then(|v| v.as_str()).unwrap_or("");
                assert!(!value.trim().is_empty(), "{code} has no {key}");
                for ph in ["{size}", "{files}", "{count}", "{names}", "{name}", "{path}"] {
                    let want = english[*key].as_str().unwrap().contains(ph);
                    assert_eq!(value.contains(ph), want, "{code} {key} placeholder {ph}");
                }
            }
        }
        // Every reason key a check can return has text.
        let i = I18n::new("en");
        for key in ["mig_why_game_root", "mig_why_name", "mig_why_link", "mig_why_place", "mig_why_layout"] {
            assert_ne!(i.t(key), key);
        }
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(fmt_size(80_068_745), "76 MB");
        assert_eq!(fmt_size(800_000), "0.8 MB");
        assert_eq!(fmt_size(3_000), "3 KB");
    }
}
