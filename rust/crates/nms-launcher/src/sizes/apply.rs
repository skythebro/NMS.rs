//! Clamp Python size ranges, write `NMS.rs\sizes.json`, build grid and slots UI.

use nms_core::paths;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use super::grid::{self, RowsSpec, AREA_W, CARD_W, GAP, MAX_ITEMS, MIN_CARD};
use super::inventory;
use super::layout;
use crate::files;
use crate::tools::{self, Tools};

pub const BUSY: &str = "Close the game first. The manager cannot change files while NMS.exe is running.";
const WINDOW_RANGE: (f64, f64) = (1.0, 1.4);
const SLOT_RANGE: (i64, i64) = (50, 100);
const DEFAULT_COLUMNS: i64 = 8;
const DEFAULT_PERCENT: i64 = 70;

pub fn clamp_window(value: f64) -> f64 {
    let (low, high) = WINDOW_RANGE;
    let mut n = if value.is_finite() { value } else { high };
    if n < low {
        n = low;
    }
    if n > high {
        n = high;
    }
    (n * 100.0).round() / 100.0
}

pub fn max_columns(window: f64) -> i64 {
    let window = clamp_window(window);
    let limit = ((AREA_W * window) / (MIN_CARD * CARD_W + GAP)) as i64;
    limit.max(1).min(MAX_ITEMS)
}

pub fn clamp_columns(value: i64, window: f64) -> i64 {
    let cap = max_columns(window);
    let mut n = value;
    if n < 1 {
        n = 1;
    }
    if n > cap {
        n = cap;
    }
    n
}

pub fn clamp_percent(value: f64) -> i64 {
    let (low, high) = SLOT_RANGE;
    let mut n = if value.is_finite() {
        value.round() as i64
    } else {
        DEFAULT_PERCENT
    };
    if n < low {
        n = low;
    }
    if n > high {
        n = high;
    }
    n
}

pub fn clamp_rows_text(text: &str, columns: i64) -> String {
    let raw = text.trim().to_ascii_lowercase();
    if raw.is_empty() || raw == "auto" || "auto".starts_with(&raw) {
        return if raw.is_empty() || raw == "auto" {
            "auto".into()
        } else {
            raw
        };
    }
    let Ok(mut n) = raw.parse::<i64>() else {
        return "auto".into();
    };
    let cap = (MAX_ITEMS / columns.max(1)).max(1);
    if n < 1 {
        n = 1;
    }
    if n > cap {
        n = cap;
    }
    n.to_string()
}

pub struct ApplySpec {
    pub columns: i64,
    pub rows: String,
    pub window: f64,
    pub slots: f64,
    pub filters: bool,
}

fn sizes_path(root: &Path) -> PathBuf {
    paths::loader_dir(root).join("sizes.json")
}

fn load_sizes(path: &Path) -> Value {
    if !path.is_file() {
        return json!({});
    }
    let Ok(text) = fs::read_to_string(path) else {
        return json!({});
    };
    serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
}

fn write_sizes(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, text + "\n").map_err(|e| e.to_string())
}

fn is_user_keep(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    if name.ends_with(".log") {
        return true;
    }
    name.ends_with(".json") && !name.ends_with("_preset.json")
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

fn stash_user_files(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut kept = Vec::new();
    if !folder.is_dir() {
        return kept;
    }
    for path in walk_files(folder) {
        if !is_user_keep(&path) {
            continue;
        }
        if let (Ok(rel), Ok(bytes)) = (path.strip_prefix(folder), fs::read(&path)) {
            kept.push((rel.to_path_buf(), bytes));
        }
    }
    kept
}

fn restore_user_files(folder: &Path, kept: &[(PathBuf, Vec<u8>)]) -> Result<(), String> {
    if kept.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    for (rel, bytes) in kept {
        let out = folder.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&out, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn rows_spec(text: &str) -> RowsSpec {
    if text == "auto" {
        RowsSpec::Auto
    } else if let Ok(n) = text.parse::<i64>() {
        RowsSpec::Count(n)
    } else {
        RowsSpec::Auto
    }
}

pub fn apply(root: &Path, spec: ApplySpec, running: bool) -> Result<String, String> {
    if running {
        return Err(BUSY.into());
    }
    let window = clamp_window(spec.window);
    let columns = clamp_columns(spec.columns, window);
    let rows_text = clamp_rows_text(&spec.rows, columns);
    let percent = clamp_percent(spec.slots);
    let filters = spec.filters;
    let grid = grid::grid_from(Some(columns), Some(rows_spec(&rows_text)), Some(window))?;
    let scale = inventory::scale_from(percent as f64 / 100.0)?;

    let loader = paths::loader_dir(root);
    let mut say = |_s: &str| {};
    let tools: Tools = tools::ensure_tools(&loader, root, &mut say)?;
    let pcbanks = root.join("GAMEDATA").join("PCBANKS");
    let mods = root.join("GAMEDATA").join("MODS");
    fs::create_dir_all(&mods).map_err(|e| e.to_string())?;

    let grid_dir = mods.join("BiggerItemGrids");
    let slot_dir = mods.join("SmallerInventorySlots");
    let kept_grid = stash_user_files(&grid_dir);
    let kept_slots = stash_user_files(&slot_dir);
    let work = loader.join("cache").join("sizes");

    let gmsg = layout::build(&grid, &grid_dir, &pcbanks, &tools, &work.join("grid"))?;
    restore_user_files(&grid_dir, &kept_grid)?;
    crate::mods::ensure_stock_files(root, "BiggerItemGrids")?;
    if grid_dir.is_dir() {
        let preset = json!({
            "columns": grid.columns,
            "rows": grid.rows,
            "window_scale": grid.window_scale,
        });
        let text = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
        fs::write(grid_dir.join("bigger_item_grids_preset.json"), text + "\n").map_err(|e| e.to_string())?;
    }

    let smsg = inventory::build_slots(scale, filters, &slot_dir, &pcbanks, &tools, &work.join("slots"))?;
    restore_user_files(&slot_dir, &kept_slots)?;
    crate::mods::ensure_stock_files(root, "SmallerInventorySlots")?;
    if slot_dir.is_dir() {
        let preset = json!({"scale": scale, "filters": filters});
        fs::write(
            slot_dir.join("smaller_inventory_slots_preset.json"),
            serde_json::to_string(&preset).map_err(|e| e.to_string())? + "\n",
        )
        .map_err(|e| e.to_string())?;
    }

    let file = sizes_path(root);
    let mut remembered = load_sizes(&file);
    if !remembered.is_object() {
        remembered = json!({});
    }
    if let Some(map) = remembered.as_object_mut() {
        map.insert(
            "BiggerItemGrids".into(),
            json!({
                "columns": grid.columns,
                "rows": grid.rows,
                "window_scale": grid.window_scale,
            }),
        );
        map.insert(
            "SmallerInventorySlots".into(),
            json!({
                "scale": scale,
                "filters": filters,
            }),
        );
    }
    write_sizes(&file, &remembered)?;
    Ok(format!("{gmsg}\n{smsg}"))
}

fn parse_apply_args(args: &[&str]) -> Result<ApplySpec, String> {
    let usage = "usage: nms-rs sizes apply --columns N --rows auto|N --window F --slots P [--no-filters]";
    if args.first() != Some(&"apply") {
        return Err(usage.into());
    }
    let mut columns: Option<i64> = None;
    let mut rows: Option<String> = None;
    let mut window: Option<f64> = None;
    let mut slots: Option<f64> = None;
    let mut filters = true;
    let rest = &args[1..];
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--no-filters" => {
                filters = false;
                i += 1;
            }
            "--columns" => {
                let v = rest.get(i + 1).ok_or(usage)?;
                columns = Some(v.parse().map_err(|_| "columns must be a whole number")?);
                i += 2;
            }
            "--rows" => {
                let v = rest.get(i + 1).ok_or(usage)?;
                rows = Some((*v).to_string());
                i += 2;
            }
            "--window" => {
                let v = rest.get(i + 1).ok_or(usage)?;
                window = Some(v.parse().map_err(|_| "window must be a number")?);
                i += 2;
            }
            "--slots" => {
                let v = rest.get(i + 1).ok_or(usage)?;
                slots = Some(v.parse().map_err(|_| "slots must be a number")?);
                i += 2;
            }
            _ => return Err(usage.into()),
        }
    }
    Ok(ApplySpec {
        columns: columns.unwrap_or(DEFAULT_COLUMNS),
        rows: rows.unwrap_or_else(|| "auto".into()),
        window: window.unwrap_or(1.4),
        slots: slots.unwrap_or(DEFAULT_PERCENT as f64),
        filters,
    })
}

pub fn cli(root: &Path, args: &[&str]) -> Result<String, String> {
    let spec = parse_apply_args(args)?;
    apply(root, spec, files::game_is_running())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_match_python() {
        assert_eq!(clamp_window(2.05), 1.4);
        assert_eq!(clamp_window(0.5), 1.0);
        assert_eq!(clamp_percent(110.0), 100);
        assert_eq!(clamp_percent(40.0), 50);
        assert_eq!(max_columns(1.4), 9);
        assert_eq!(clamp_columns(0, 1.4), 1);
        assert_eq!(clamp_columns(99, 1.4), 9);
        assert_eq!(clamp_columns(8, 1.4), 8);
        assert_eq!(clamp_rows_text("20", 8), "12");
        assert_eq!(clamp_rows_text("auto", 8), "auto");
        assert_eq!(clamp_rows_text("", 8), "auto");
    }

    #[test]
    fn apply_refuses_when_game_running() {
        let spec = ApplySpec {
            columns: 8,
            rows: "auto".into(),
            window: 1.4,
            slots: 70.0,
            filters: true,
        };
        let err = apply(Path::new("."), spec, true).unwrap_err();
        assert_eq!(err, BUSY);
    }

    #[test]
    fn remember_roundtrip_on_temp_tree() {
        let dir = std::env::temp_dir().join(format!("nms-rs-t038-sizes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(paths::loader_dir(&dir)).unwrap();
        let file = sizes_path(&dir);
        let mut remembered = json!({});
        remembered["BiggerItemGrids"] = json!({"columns": 8, "rows": 3, "window_scale": 1.4});
        remembered["SmallerInventorySlots"] = json!({"scale": 0.7, "filters": true});
        write_sizes(&file, &remembered).unwrap();
        let loaded = load_sizes(&file);
        assert_eq!(loaded["BiggerItemGrids"]["columns"], 8);
        assert_eq!(loaded["SmallerInventorySlots"]["scale"], 0.7);
        let _ = fs::remove_dir_all(&dir);
    }
}
