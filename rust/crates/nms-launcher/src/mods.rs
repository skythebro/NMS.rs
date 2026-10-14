//! Park, install, update, Python cleanup. Library for the manager window.

use nms_core::paths;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::files;
use crate::steam;

pub const COMBINED_NAME: &str = "z_Combined Mods";
pub const BUSY: &str = "Close the game first. The manager cannot change files while NMS.exe is running.";

/// Python `OUR_MODS` (card order). Cleanup never deletes PulseTweaks Python.
pub const OUR_MODS: &[&str] = &[
    "BiggerItemGrids",
    "SmallerInventorySlots",
    "CompactTooltips",
    "CompactQuickTransfer",
    "CraftFromStorage",
    "QuickTransferAll",
    "TradeUITweaks",
    "SphereSizedMining",
    "QuickRecharge",
    "HideEmptyHover",
    "GalaxyMapFilters",
    "PickerTweaks",
    "RefinerGuard",
    "ToolModeBinds",
];

/// Mods the manager keeps installed on its own (the in-game pause-menu row). Installed when
/// neither live nor parked; kept up to date; parking one is respected and never undone.
pub const AUTO_MODS: &[&str] = &["NMSrsMenu"];

const SIZE_MODS: &[&str] = &["BiggerItemGrids", "SmallerInventorySlots"];

/// Mods that still run as Python and so keep their `.py` files on install and update.
const PYTHON_MODS: &[&str] = &["PulseTweaks"];

pub fn parked_dir(root: &Path) -> PathBuf {
    paths::loader_dir(root).join("parked")
}

pub fn mods_dir(root: &Path) -> PathBuf {
    root.join("GAMEDATA").join("MODS")
}

pub fn stock_dir(root: &Path) -> PathBuf {
    let rs = paths::loader_dir(root).join("stock");
    if rs.is_dir() {
        rs
    } else {
        root.join("NMS.py").join("stock")
    }
}

fn log_line(root: &Path, message: &str) {
    let dir = paths::loader_dir(root).join("logs");
    let _ = fs::create_dir_all(&dir);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("manager.log")) {
        let _ = writeln!(f, "{message}");
    }
}

fn busy(running: bool) -> Result<(), String> {
    if running {
        Err(BUSY.into())
    } else {
        Ok(())
    }
}

fn dir_names(folder: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return names;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names
}

/// Stock folders plus the mods named in `stock.json`. A ported mod whose only stock files
/// were Python has no folder in the zip but still has a row (an empty live folder is the switch).
pub fn stock_names(root: &Path) -> Vec<String> {
    let dir = stock_dir(root);
    let mut names = dir_names(&dir);
    let listed = fs::read_to_string(dir.join("stock.json")).ok();
    if let Some(Ok(serde_json::Value::Object(map))) = listed.map(|t| serde_json::from_str(&t)) {
        for key in map.keys() {
            if !names.iter().any(|n| n == key) {
                names.push(key.clone());
            }
        }
    }
    names.sort();
    names
}

fn skip_update_file(mod_name: &str, rel: &Path, skip_size_ui: bool) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    if name.ends_with(".json") || name.ends_with(".log") {
        return true;
    }
    if rel.iter().any(|p| p == "__pycache__") {
        return true;
    }
    // Ported mods run from the proxy DLL; the Python source in stock is history.
    if name.ends_with(".py") && !PYTHON_MODS.contains(&mod_name) {
        return true;
    }
    // Size-built files live in the mod's top-level UI folder. TEXTURES/UI/... (the filter
    // glyphs) is stock content and must be copied, so only the first component counts.
    if skip_size_ui && SIZE_MODS.contains(&mod_name) && rel.iter().next().is_some_and(|p| p == "UI") {
        return true;
    }
    false
}

fn rel_posix(rel: &Path) -> String {
    rel.iter().map(|s| s.to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rec(&path, out);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    rec(root, &mut out);
    out
}

fn hash_file(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(&bytes);
    format!("{:x}", h.finalize())
}

fn extract_version(text: &str) -> Option<String> {
    let key = "__version__";
    let mut search = text;
    while let Some(at) = search.find(key) {
        let rest = search[at + key.len()..].trim_start();
        if let Some(eq) = rest.strip_prefix('=') {
            let rest = eq.trim_start();
            let mut chars = rest.chars();
            let q = chars.next()?;
            if q == '"' || q == '\'' {
                let inner: String = chars.take_while(|&c| c != q).collect();
                return Some(inner);
            }
        }
        search = &search[at + key.len()..];
    }
    None
}

fn file_version(folder: &Path) -> String {
    if !folder.is_dir() {
        return String::new();
    }
    let mut pys: Vec<PathBuf> = fs::read_dir(folder)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("py"))
        .collect();
    pys.sort();
    for py in pys {
        if let Ok(text) = fs::read_to_string(&py) {
            if let Some(v) = extract_version(&text) {
                return v;
            }
        }
    }
    String::new()
}

fn version_key(text: &str) -> Vec<i64> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse() {
                parts.push(n);
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(n) = cur.parse() {
            parts.push(n);
        }
    }
    if parts.is_empty() {
        vec![0]
    } else {
        parts
    }
}

fn fingerprint(folder: &Path, mod_name: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    if !folder.is_dir() {
        return found;
    }
    for path in walk_files(folder) {
        let Ok(rel) = path.strip_prefix(folder) else {
            continue;
        };
        if skip_update_file(mod_name, rel, true) {
            continue;
        }
        found.insert(rel_posix(rel), hash_file(&path));
    }
    found
}

pub fn is_outdated(root: &Path, name: &str, folder: &Path) -> bool {
    let src = stock_dir(root).join(name);
    if !src.is_dir() || !folder.is_dir() {
        return false;
    }
    let wanted = fingerprint(&src, name);
    if wanted.is_empty() {
        return false;
    }
    let live = fingerprint(folder, name);
    let mismatches: Vec<&String> = wanted
        .iter()
        .filter(|(rel, digest)| live.get(*rel) != Some(digest))
        .map(|(rel, _)| rel)
        .collect();
    if mismatches.is_empty() {
        return false;
    }
    let live_ver = file_version(folder);
    let stock_ver = file_version(&src);
    if !live_ver.is_empty() && !stock_ver.is_empty() && version_key(&live_ver) < version_key(&stock_ver) {
        return true;
    }
    for rel in mismatches {
        let live_path = folder.join(rel);
        let stock_path = src.join(rel);
        if !live_path.is_file() {
            return true;
        }
        let live_m = live_path.metadata().and_then(|m| m.modified()).ok();
        let stock_m = stock_path.metadata().and_then(|m| m.modified()).ok();
        match (live_m, stock_m) {
            (Some(l), Some(s)) if l < s => return true,
            (None, _) => return true,
            _ => {}
        }
    }
    false
}

fn copy_stock_files(src: &Path, dest: &Path, name: &str, skip_size_ui: bool) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for path in walk_files(src) {
        let Ok(rel) = path.strip_prefix(src) else {
            continue;
        };
        if skip_update_file(name, rel, skip_size_ui) {
            continue;
        }
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(&path, &out).map_err(|e| format!("copy {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Copy the stock files a mod folder is missing (never overwrites). `sizes apply` builds the
/// UI files into a folder that may not exist yet; this adds the script files beside them.
pub fn ensure_stock_files(root: &Path, name: &str) -> Result<(), String> {
    let src = stock_dir(root).join(name);
    let dest = mods_dir(root).join(name);
    if !src.is_dir() || !dest.is_dir() {
        return Ok(());
    }
    for path in walk_files(&src) {
        let Ok(rel) = path.strip_prefix(&src) else {
            continue;
        };
        if skip_update_file(name, rel, true) {
            continue;
        }
        let out = dest.join(rel);
        if out.exists() {
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(&path, &out).map_err(|e| format!("copy {}: {e}", path.display()))?;
    }
    Ok(())
}

pub fn park_mod(root: &Path, name: &str, running: bool) -> Result<String, String> {
    busy(running)?;
    if name == COMBINED_NAME {
        return Err(format!("{COMBINED_NAME} cannot be parked."));
    }
    let src = mods_dir(root).join(name);
    let dest = parked_dir(root).join(name);
    if !src.is_dir() {
        return Err(format!("{name} is not in GAMEDATA\\MODS."));
    }
    if dest.exists() {
        return Err(format!("{name} is already parked."));
    }
    fs::create_dir_all(parked_dir(root)).map_err(|e| e.to_string())?;
    fs::rename(&src, &dest).map_err(|e| e.to_string())?;
    let msg = format!("parked {name}");
    log_line(root, &msg);
    Ok(msg)
}

pub fn unpark_mod(root: &Path, name: &str, running: bool) -> Result<String, String> {
    busy(running)?;
    let src = parked_dir(root).join(name);
    let dest = mods_dir(root).join(name);
    if !src.is_dir() {
        return Err(format!("{name} is not parked."));
    }
    if dest.exists() {
        return Err(format!("{name} is already in GAMEDATA\\MODS."));
    }
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::rename(&src, &dest).map_err(|e| e.to_string())?;
    let msg = format!("restored {name}");
    log_line(root, &msg);
    Ok(msg)
}

pub fn park_all(root: &Path, running: bool) -> Result<String, String> {
    busy(running)?;
    let stock = stock_names(root);
    let live = mods_dir(root);
    let mut done = Vec::new();
    for name in dir_names(&live) {
        if name == COMBINED_NAME {
            continue;
        }
        if !(stock.iter().any(|s| s == &name) || OUR_MODS.contains(&name.as_str())) {
            continue;
        }
        park_mod(root, &name, false)?;
        done.push(name);
    }
    if done.is_empty() {
        Ok("nothing to park".into())
    } else {
        Ok(format!("parked {}", done.join(", ")))
    }
}

pub fn install_mod(root: &Path, name: &str, running: bool) -> Result<String, String> {
    busy(running)?;
    let live = mods_dir(root).join(name);
    let parked = parked_dir(root).join(name);
    if live.is_dir() {
        return Ok(format!("{name} is already in GAMEDATA\\MODS."));
    }
    if parked.is_dir() {
        return unpark_mod(root, name, false);
    }
    let src = stock_dir(root).join(name);
    if src.is_dir() {
        copy_stock_files(&src, &live, name, false)?;
    } else if stock_names(root).iter().any(|n| n == name) {
        fs::create_dir_all(&live).map_err(|e| e.to_string())?;
    } else {
        return Err(format!("No replacement files for {name}."));
    }
    let msg = format!("installed {name}");
    log_line(root, &msg);
    Ok(msg)
}

/// Install or update the `AUTO_MODS` from stock. Safe while the game runs (plain file copies).
pub fn auto_install(root: &Path) -> Vec<String> {
    let mut done = Vec::new();
    for name in AUTO_MODS {
        if !stock_dir(root).join(name).is_dir() {
            continue;
        }
        let live = mods_dir(root).join(name);
        if live.is_dir() {
            if is_outdated(root, name, &live) {
                if let Ok(m) = update_mod(root, name, false) {
                    done.push(m);
                }
            }
        } else if !parked_dir(root).join(name).is_dir() {
            if let Ok(m) = install_mod(root, name, false) {
                done.push(m);
            }
        }
    }
    done
}

/// Element ids that only a side bar of layout 14 or newer has (`filter_bar` LAYOUT_VERSION 14:
/// the Foods and Custom buttons). Layout 15 also needs every icon in
/// [`filter_bar::OWN_ICONS`](crate::sizes::filter_bar::OWN_ICONS) (egg, bone and treasure glyphs);
/// the MBIN stores image paths as plain bytes.
const SIDE_BAR_MARKERS: [&[u8]; 2] = [b"FILTER18", b"FILTER19"];
/// Shown by `update`, `update-all`, `status` and the manager while [`side_bar_outdated`] is true.
pub const SIDE_BAR_NOTE: &str =
    "Press Apply sizes to add the new filter buttons (Foods, Custom) and the new icons.";

/// The installed Smaller Inventory Slots side bar was built before the current buttons and icons.
///
/// Update keeps size-built UI files (`skip_size_ui`), so a side bar made by an older Apply sizes
/// stays until the user applies sizes again. Only the live file counts: one renamed to `.merged`
/// is not used (the merge builds the side bar from vanilla and the luas), and a side bar is only
/// written with the filter buttons on, so a file without the markers is an old layout.
pub fn side_bar_outdated(root: &Path) -> bool {
    let bar = mods_dir(root)
        .join("SmallerInventorySlots")
        .join("UI")
        .join("COMPONENTS")
        .join("PAGESELECTBARSIDE.MBIN");
    let Ok(bytes) = fs::read(&bar) else {
        return false;
    };
    let has = |m: &[u8]| bytes.windows(m.len()).any(|w| w == m);
    !(SIDE_BAR_MARKERS.iter().all(|m| has(m))
        && crate::sizes::filter_bar::OWN_ICONS
            .iter()
            .all(|icon| has(icon.rsplit('/').next().unwrap_or(icon).as_bytes())))
}

fn with_side_bar_note(root: &Path, msg: String) -> String {
    if side_bar_outdated(root) {
        format!("{msg}\n{SIDE_BAR_NOTE}")
    } else {
        msg
    }
}

pub fn update_mod(root: &Path, name: &str, running: bool) -> Result<String, String> {
    busy(running)?;
    let src = stock_dir(root).join(name);
    if !src.is_dir() {
        return Err(format!("No replacement files for {name}."));
    }
    let live = mods_dir(root).join(name);
    let parked = parked_dir(root).join(name);
    let dest = if live.is_dir() {
        live.clone()
    } else if parked.is_dir() {
        parked.clone()
    } else {
        live.clone()
    };
    copy_stock_files(&src, &dest, name, true)?;
    if live.is_dir() && parked.is_dir() {
        let _ = fs::remove_dir_all(&parked);
    }
    let msg = format!("updated {name}");
    log_line(root, &msg);
    if name == "SmallerInventorySlots" {
        return Ok(with_side_bar_note(root, msg));
    }
    Ok(msg)
}

pub fn install_all(root: &Path, running: bool) -> Result<String, String> {
    busy(running)?;
    let mut done = Vec::new();
    for name in stock_names(root) {
        let live = mods_dir(root).join(&name);
        if live.is_dir() {
            continue;
        }
        install_mod(root, &name, false)?;
        done.push(name);
    }
    if done.is_empty() {
        Ok("nothing to install".into())
    } else {
        Ok(format!("installed {}", done.join(", ")))
    }
}

pub fn update_all(root: &Path, running: bool) -> Result<String, String> {
    busy(running)?;
    let mut done = Vec::new();
    for name in stock_names(root) {
        let live = mods_dir(root).join(&name);
        let parked = parked_dir(root).join(&name);
        let folder = if live.is_dir() {
            live
        } else if parked.is_dir() {
            parked
        } else {
            continue;
        };
        if is_outdated(root, &name, &folder) {
            update_mod(root, &name, false)?;
            done.push(name);
        }
    }
    let msg = if done.is_empty() { "nothing to update".to_string() } else { format!("updated {}", done.join(", ")) };
    Ok(with_side_bar_note(root, msg))
}

/// `.py`, `.py.off` and the `.pyc` bytecode Python left in `__pycache__`.
pub(crate) fn is_python_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    name.ends_with(".py") || name.ends_with(".py.off") || name.ends_with(".pyc")
}

/// Our mods' folder names (live and parked), without PulseTweaks (still Python).
pub(crate) fn our_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = OUR_MODS.iter().map(|s| (*s).to_string()).collect();
    names.extend(stock_names(root));
    names.sort();
    names.dedup();
    names.retain(|n| n != "PulseTweaks" && n != COMBINED_NAME);
    names
}

/// Files in our live or parked mod folders that differ from both the NMS.rs stock copy and the
/// old stock copy in `old_stock` (the 3.x `NMS.py\stock`): the user edited them. A file the old
/// stock does not have cannot be told apart from an old version, so it does not count.
pub(crate) fn edited_files(root: &Path, old_stock: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for name in our_names(root) {
        let new_src = stock_dir(root).join(&name);
        let old_src = old_stock.join(&name);
        if !new_src.is_dir() || !old_src.is_dir() {
            continue;
        }
        let wanted = fingerprint(&new_src, &name);
        for folder in [mods_dir(root).join(&name), parked_dir(root).join(&name)] {
            if !folder.is_dir() {
                continue;
            }
            for (rel, digest) in &wanted {
                let live = folder.join(rel);
                let old = old_src.join(rel);
                if !live.is_file() || !old.is_file() {
                    continue;
                }
                let have = hash_file(&live);
                if &have != digest && have != hash_file(&old) {
                    found.push(live);
                }
            }
        }
    }
    found.sort();
    found
}

pub(crate) fn python_leftovers(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = Vec::new();
    for name in our_names(root) {
        folders.push(mods_dir(root).join(&name));
        folders.push(parked_dir(root).join(&name));
    }
    folders.sort();
    folders.dedup();
    for folder in folders {
        if !folder.is_dir() {
            continue;
        }
        for path in walk_files(&folder) {
            if is_python_file(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

pub fn cleanup_python(root: &Path, running: bool) -> Result<String, String> {
    busy(running)?;
    let mut lines = Vec::new();
    for path in python_leftovers(root) {
        fs::remove_file(&path).map_err(|e| format!("delete {}: {e}", path.display()))?;
        let msg = format!("deleted {}", path.display());
        log_line(root, &msg);
        lines.push(msg);
        // An emptied `__pycache__` goes too (fails harmlessly while it still holds files).
        if let Some(cache) = path.parent().filter(|p| p.file_name().is_some_and(|n| n == "__pycache__")) {
            let _ = fs::remove_dir(cache);
        }
    }
    if lines.is_empty() {
        Ok("no python leftovers".into())
    } else {
        Ok(lines.join("\n"))
    }
}

pub fn strip_launch(running: bool) -> Result<String, String> {
    busy(running)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(steam::apply_for(&exe, false, false)?.join("\n"))
}

pub fn status(root: &Path) -> String {
    let mut lines = Vec::new();
    for name in stock_names(root) {
        let live = mods_dir(root).join(&name).is_dir();
        let parked = parked_dir(root).join(&name).is_dir();
        let state = if live {
            "live"
        } else if parked {
            "parked"
        } else {
            "missing"
        };
        lines.push(format!("{name} {state}"));
    }
    let leftover = python_leftovers(root);
    if leftover.is_empty() {
        lines.push("python leftover: no".into());
    } else {
        lines.push("python leftover: yes".into());
    }
    if side_bar_outdated(root) {
        lines.push(format!("side bar: old layout. {SIDE_BAR_NOTE}"));
    }
    lines.join("\n")
}

pub fn cli(root: &Path, args: &[&str]) -> Result<String, String> {
    let running = files::game_is_running();
    match args {
        ["park", name] => park_mod(root, name, running),
        ["unpark", name] => unpark_mod(root, name, running),
        ["park-all"] => park_all(root, running),
        ["install", name] => install_mod(root, name, running),
        ["update", name] => update_mod(root, name, running),
        ["install-all"] => install_all(root, running),
        ["update-all"] => update_all(root, running),
        ["cleanup-python"] => cleanup_python(root, running),
        ["strip-launch"] => strip_launch(running),
        ["status"] => Ok(status(root)),
        _ => Err(
            "usage: nms-rs mods park <name> | unpark <name> | park-all | install <name> | update <name> | install-all | update-all | cleanup-python | strip-launch | status"
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEMP_N: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn update_keeps_size_ui_but_copies_textures() {
        let sis = "SmallerInventorySlots";
        let ui = Path::new("UI").join("COMPONENTS").join("PAGESELECTBARSIDE.MBIN");
        let glyph = Path::new("TEXTURES")
            .join("UI")
            .join("FRONTEND")
            .join("ICONS")
            .join("CATEGORIES")
            .join("FILTEREGG.DDS");
        assert!(skip_update_file(sis, &ui, true), "size-built UI stays");
        assert!(!skip_update_file(sis, &ui, false), "install copies it");
        assert!(!skip_update_file(sis, &glyph, true), "glyphs are stock files");
        assert!(!skip_update_file(sis, Path::new("SmallerInventorySlots.lua"), true));

        // A live folder from before the egg glyph is outdated, and update brings the glyph.
        let root = temp_root("glyphs");
        let src = stock_dir(&root).join(sis);
        fs::create_dir_all(src.join(glyph.parent().unwrap())).unwrap();
        fs::write(src.join(&glyph), b"DDS new").unwrap();
        fs::write(src.join("SmallerInventorySlots.lua"), b"-- lua").unwrap();
        let live = mods_dir(&root).join(sis);
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("SmallerInventorySlots.lua"), b"-- lua").unwrap();
        assert!(is_outdated(&root, sis, &live));
        update_mod(&root, sis, false).unwrap();
        assert_eq!(fs::read(live.join(&glyph)).unwrap(), b"DDS new");
        assert!(!is_outdated(&root, sis, &live));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn old_side_bar_asks_for_apply_sizes() {
        let root = temp_root("sidebar");
        assert!(!side_bar_outdated(&root), "no side bar: nothing to say");
        let dir = mods_dir(&root).join("SmallerInventorySlots").join("UI").join("COMPONENTS");
        fs::create_dir_all(&dir).unwrap();
        let bar = dir.join("PAGESELECTBARSIDE.MBIN");
        fs::write(&bar, b"..FILTER17..").unwrap();
        assert!(side_bar_outdated(&root));
        assert!(status(&root).contains(SIDE_BAR_NOTE));
        // Layout 14: Foods and Custom, but the old atom and shield icons.
        fs::write(&bar, b"..FILTER18..FILTER19..FILTERFOODS.DDS..FILTERCUSTOM.DDS..DNA.ICON.DDS..").unwrap();
        assert!(side_bar_outdated(&root));
        assert!(status(&root).contains(SIDE_BAR_NOTE));
        fs::write(
            &bar,
            b"..FILTER18..FILTER19..FILTERTREASURE.DDS..FILTEREGG.DDS..FILTERBONE.DDS..FILTERFOODS.DDS..FILTERCUSTOM.DDS..",
        )
        .unwrap();
        assert!(!side_bar_outdated(&root));
        assert!(!status(&root).contains(SIDE_BAR_NOTE));
        // A merged (renamed) side bar is not used by the game: no note.
        fs::rename(&bar, dir.join("PAGESELECTBARSIDE.MBIN.merged")).unwrap();
        assert!(!side_bar_outdated(&root));
        let _ = fs::remove_dir_all(&root);
    }

    fn temp_root(tag: &str) -> PathBuf {
        let n = TEMP_N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nms-rs-t038-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("GAMEDATA").join("MODS")).unwrap();
        fs::create_dir_all(paths::loader_dir(&dir).join("stock")).unwrap();
        dir
    }

    fn write(path: &Path, body: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn auto_install_installs_once_and_respects_park() {
        let root = temp_root("auto");
        write(&stock_dir(&root).join("NMSrsMenu").join("NMSrsMenu.lua"), "-- a
");
        assert_eq!(auto_install(&root).len(), 1);
        let live = mods_dir(&root).join("NMSrsMenu");
        assert!(live.join("NMSrsMenu.lua").is_file());
        assert!(auto_install(&root).is_empty());
        park_mod(&root, "NMSrsMenu", false).unwrap();
        assert!(auto_install(&root).is_empty());
        assert!(!live.exists());
    }

    #[test]
    fn park_unpark_keeps_json() {
        let root = temp_root("park");
        let live = mods_dir(&root).join("CraftFromStorage");
        write(&live.join("craft_from_storage.py"), "x = 1\n");
        write(&live.join("craft_from_storage.json"), "{\"anywhere\": true}\n");
        park_mod(&root, "CraftFromStorage", false).unwrap();
        assert!(!live.exists());
        let parked = parked_dir(&root).join("CraftFromStorage").join("craft_from_storage.json");
        assert_eq!(fs::read_to_string(&parked).unwrap(), "{\"anywhere\": true}\n");
        unpark_mod(&root, "CraftFromStorage", false).unwrap();
        let json = live.join("craft_from_storage.json");
        assert_eq!(fs::read_to_string(&json).unwrap(), "{\"anywhere\": true}\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn running_game_refuses_park() {
        let root = temp_root("busy");
        let live = mods_dir(&root).join("CraftFromStorage");
        write(&live.join("a.py"), "x\n");
        let err = park_mod(&root, "CraftFromStorage", true).unwrap_err();
        assert_eq!(err, BUSY);
        assert!(live.is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_keeps_json_and_skips_size_mod_ui() {
        let root = temp_root("update");
        let stock = stock_dir(&root).join("BiggerItemGrids");
        write(&stock.join("bigger_item_grids.py"), "__version__ = \"3.0.0\"\n");
        write(&stock.join("README.txt"), "new readme\n");
        write(&stock.join("UI").join("POPUP.MBIN"), "new-ui\n");
        let live = mods_dir(&root).join("BiggerItemGrids");
        write(&live.join("bigger_item_grids.json"), "{\"write_log\": true}\n");
        write(&live.join("UI").join("POPUP.MBIN"), "old-ui\n");
        assert!(is_outdated(&root, "BiggerItemGrids", &live));
        update_mod(&root, "BiggerItemGrids", false).unwrap();
        assert_eq!(fs::read_to_string(live.join("README.txt")).unwrap(), "new readme\n");
        assert!(!live.join("bigger_item_grids.py").exists());
        assert!(!is_outdated(&root, "BiggerItemGrids", &live));
        assert_eq!(
            fs::read_to_string(live.join("bigger_item_grids.json")).unwrap(),
            "{\"write_log\": true}\n"
        );
        assert_eq!(fs::read_to_string(live.join("UI").join("POPUP.MBIN")).unwrap(), "old-ui\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_python_file_is_not_outdated_for_ported_mods() {
        let root = temp_root("pyskip");
        let stock = stock_dir(&root).join("QuickTransferAll");
        write(&stock.join("quick_transfer_all.py"), "__version__ = \"3.0.0\"\n");
        write(&stock.join("QuickDeposit.lua"), "lua\n");
        let live = mods_dir(&root).join("QuickTransferAll");
        write(&live.join("QuickDeposit.lua"), "lua\n");
        assert!(!is_outdated(&root, "QuickTransferAll", &live));
        let pstock = stock_dir(&root).join("PulseTweaks");
        write(&pstock.join("pulse_tweaks.py"), "__version__ = \"2.0.0\"\n");
        let plive = mods_dir(&root).join("PulseTweaks");
        write(&plive.join("other.txt"), "x\n");
        assert!(is_outdated(&root, "PulseTweaks", &plive));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_stock_files_adds_missing_script_but_keeps_ui_and_edits() {
        let root = temp_root("ensure");
        let stock = stock_dir(&root).join("SmallerInventorySlots");
        write(&stock.join("SmallerInventorySlots.lua"), "stock lua\n");
        write(&stock.join("smaller_inventory_slots.py"), "py\n");
        write(&stock.join("UI").join("PAGE.MBIN"), "stock-ui\n");
        let live = mods_dir(&root).join("SmallerInventorySlots");
        write(&live.join("UI").join("PAGE.MBIN"), "built-ui\n");
        ensure_stock_files(&root, "SmallerInventorySlots").unwrap();
        assert_eq!(fs::read_to_string(live.join("SmallerInventorySlots.lua")).unwrap(), "stock lua\n");
        assert!(!live.join("smaller_inventory_slots.py").exists());
        assert_eq!(fs::read_to_string(live.join("UI").join("PAGE.MBIN")).unwrap(), "built-ui\n");
        assert!(!is_outdated(&root, "SmallerInventorySlots", &live));
        write(&live.join("SmallerInventorySlots.lua"), "edited\n");
        ensure_stock_files(&root, "SmallerInventorySlots").unwrap();
        assert_eq!(fs::read_to_string(live.join("SmallerInventorySlots.lua")).unwrap(), "edited\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mod_listed_only_in_stock_json_installs_as_empty_folder() {
        let root = temp_root("manifest");
        write(&stock_dir(&root).join("stock.json"), "{\"QuickRecharge\": \"3.0.4\"}\n");
        assert_eq!(stock_names(&root), vec!["QuickRecharge".to_string()]);
        install_all(&root, false).unwrap();
        assert!(mods_dir(&root).join("QuickRecharge").is_dir());
        assert!(!is_outdated(&root, "QuickRecharge", &mods_dir(&root).join("QuickRecharge")));
        assert!(install_mod(&root, "NotListed", false).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cleanup_deletes_qta_leaves_pulse() {
        let root = temp_root("cleanup");
        let qta = mods_dir(&root).join("QuickTransferAll");
        write(&qta.join("quick_transfer_all.py"), "x\n");
        write(&qta.join("quick_transfer_all.py.off"), "off\n");
        write(&qta.join("__pycache__").join("quick_transfer_all.cpython-310.pyc"), "pyc\n");
        write(&qta.join("QuickTransferAll.lua"), "keep lua\n");
        write(&qta.join("quick_transfer_all.json"), "{}\n");
        write(&qta.join("notes.log"), "log\n");
        let pulse = mods_dir(&root).join("PulseTweaks");
        write(&pulse.join("pulse_tweaks.py"), "keep\n");
        let out = cleanup_python(&root, false).unwrap();
        assert!(out.contains("quick_transfer_all.py"), "{out}");
        assert!(!qta.join("quick_transfer_all.py").exists());
        assert!(!qta.join("quick_transfer_all.py.off").exists());
        assert!(!qta.join("__pycache__").exists(), "bytecode and its emptied folder go too");
        assert_eq!(fs::read_to_string(qta.join("QuickTransferAll.lua")).unwrap(), "keep lua\n");
        assert_eq!(fs::read_to_string(qta.join("quick_transfer_all.json")).unwrap(), "{}\n");
        assert_eq!(fs::read_to_string(qta.join("notes.log")).unwrap(), "log\n");
        assert_eq!(fs::read_to_string(pulse.join("pulse_tweaks.py")).unwrap(), "keep\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn status_lists_stock_and_python_leftover() {
        let root = temp_root("status");
        write(
            &stock_dir(&root).join("QuickTransferAll").join("quick_transfer_all.py"),
            "s\n",
        );
        write(&stock_dir(&root).join("PulseTweaks").join("pulse_tweaks.py"), "s\n");
        write(
            &mods_dir(&root).join("QuickTransferAll").join("quick_transfer_all.py"),
            "live\n",
        );
        let text = status(&root);
        assert!(text.contains("QuickTransferAll live"), "{text}");
        assert!(text.contains("PulseTweaks missing"), "{text}");
        assert!(text.contains("python leftover: yes"), "{text}");
        cleanup_python(&root, false).unwrap();
        let text = status(&root);
        assert!(text.contains("python leftover: no"), "{text}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn install_unparks_or_copies_stock() {
        let root = temp_root("install");
        write(
            &parked_dir(&root).join("CraftFromStorage").join("craft_from_storage.json"),
            "{\"anywhere\": true}\n",
        );
        install_mod(&root, "CraftFromStorage", false).unwrap();
        assert!(mods_dir(&root).join("CraftFromStorage").join("craft_from_storage.json").is_file());
        assert!(!parked_dir(&root).join("CraftFromStorage").exists());
        write(
            &stock_dir(&root).join("HideEmptyHover").join("README.txt"),
            "stock\n",
        );
        write(
            &stock_dir(&root).join("HideEmptyHover").join("hide_empty_hover.py"),
            "python history\n",
        );
        install_mod(&root, "HideEmptyHover", false).unwrap();
        assert_eq!(
            fs::read_to_string(mods_dir(&root).join("HideEmptyHover").join("README.txt")).unwrap(),
            "stock\n"
        );
        assert!(!mods_dir(&root).join("HideEmptyHover").join("hide_empty_hover.py").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn park_all_skips_combined() {
        let root = temp_root("parkall");
        write(&mods_dir(&root).join(COMBINED_NAME).join("combined.txt"), "x\n");
        write(&mods_dir(&root).join("QuickRecharge").join("quick_recharge.py"), "x\n");
        write(&mods_dir(&root).join("OtherPersonsMod").join("notes.txt"), "leave\n");
        park_all(&root, false).unwrap();
        assert!(mods_dir(&root).join(COMBINED_NAME).is_dir());
        assert!(mods_dir(&root).join("OtherPersonsMod").is_dir());
        assert!(parked_dir(&root).join("QuickRecharge").is_dir());
        let _ = fs::remove_dir_all(&root);
    }
}
