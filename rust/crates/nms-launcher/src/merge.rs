//! Combine AMUMSS Lua scripts from GAMEDATA\\MODS into z_Combined Mods.

use nms_amumss::exml::{apply_change_table, ExmlChangeError, Mxml};
use nms_amumss::lua::{
    change_tables, mbin_sources, parse_mod_script_file, script_applyable, script_targets,
};
use nms_core::paths;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::files;
use crate::tools::{self, Tools};

pub const MERGE_LOGIC: i64 = 3;
pub const COMBINED_NAME: &str = "z_Combined Mods";
const MERGED_SUFFIX: &str = ".merged";
pub const OUR_ORDER: &[&str] = &[
    "SmallerInventorySlots",
    "BiggerItemGrids",
    "CompactTooltips",
    "CompactQuickTransfer",
    "QuickTransferAll",
    "QuickRecharge",
    "TradeUITweaks",
    "NMSrsMenu",
    "GalaxyMapFilters",
];

#[derive(Clone, Debug)]
pub struct Script {
    pub folder: PathBuf,
    pub name: String,
    pub path: PathBuf,
    pub container: Value,
    pub apply: bool,
}

pub struct Merge {
    pub mods: PathBuf,
    pub pcbanks: PathBuf,
    pub loader: PathBuf,
    pub game_root: PathBuf,
}

impl Merge {
    pub fn for_game(root: &Path) -> Self {
        Self {
            mods: root.join("GAMEDATA").join("MODS"),
            pcbanks: root.join("GAMEDATA").join("PCBANKS"),
            loader: paths::loader_dir(root),
            game_root: root.to_path_buf(),
        }
    }

    fn combined(&self) -> PathBuf {
        self.mods.join(COMBINED_NAME)
    }

    fn stamp_path(&self) -> PathBuf {
        self.loader.join("cache").join("ui_merge.json")
    }

    fn work_path(&self) -> PathBuf {
        self.loader.join("cache").join("ui_merge_work")
    }

    fn merge_order_path(&self) -> PathBuf {
        paths::loader_dir(&self.game_root).join("merge_order.txt")
    }

    pub fn merge_installed_mods(
        &self,
        say: &mut dyn FnMut(&str),
        problem: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        say("Checking UI merge.");
        let mut jobs: Vec<(String, Vec<String>)> = Vec::new();
        let result = self.merge_body(say, &mut jobs);
        if let Err(error) = &result {
            problem(&format!(
                "UI scripts could not be merged ({error}). Mods that replace the same file will still clash."
            ));
        }
        let map = jobs_to_map(&jobs);
        files::write_conflict_report(&self.game_root, Some(say), &map, false);
        result
    }

    fn merge_body(
        &self,
        say: &mut dyn FnMut(&str),
        jobs: &mut Vec<(String, Vec<String>)>,
    ) -> Result<(), String> {
        let our_files = self.our_ui_targets(None);
        let scripts = self.discover_scripts(say, Some(&our_files));
        *jobs = if scripts.is_empty() {
            Vec::new()
        } else {
            self.merge_jobs(&scripts)
        };
        if scripts.is_empty() {
            self.restore_all(say);
            if self.combined().is_dir() {
                fs::remove_dir_all(self.combined()).map_err(|e| e.to_string())?;
                say(&format!("Removed {}: no UI scripts left.", self.combined().display()));
            }
            self.clear_stamp();
            return Ok(());
        }
        if jobs.is_empty() {
            self.restore_all(say);
            if self.combined().is_dir() {
                fs::remove_dir_all(self.combined()).map_err(|e| e.to_string())?;
                say(&format!(
                    "Removed {}: each UI file has only one owner.",
                    self.combined().display()
                ));
            }
            self.clear_stamp();
            return Ok(());
        }
        let stamp = self.stamp(&scripts, jobs)?;
        if self.stamp_path().is_file() {
            let text = fs::read_to_string(self.stamp_path()).map_err(|e| e.to_string())?;
            let saved: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if saved == stamp && self.combined_ok(jobs) {
                say(&format!(
                    "UI merge is up to date ({} file(s) in {COMBINED_NAME}).",
                    jobs.len()
                ));
                return Ok(());
            }
        }
        say(&format!(
            "Merging {} UI file(s) from {} script(s).",
            jobs.len(),
            scripts.len()
        ));
        let toolset = tools::ensure_tools(&self.loader, &self.game_root, say)?;
        self.build(jobs, &scripts, &toolset, say)?;
        self.write_combined_txt(&scripts, jobs)?;
        fs::create_dir_all(self.loader.join("cache")).map_err(|e| e.to_string())?;
        let body = serde_json::to_string_pretty(&stamp).map_err(|e| e.to_string())? + "\n";
        fs::write(self.stamp_path(), body).map_err(|e| e.to_string())?;
        let mut names: BTreeSet<String> = BTreeSet::new();
        for (_, list) in jobs.iter() {
            names.extend(list.iter().cloned());
        }
        let joined: Vec<String> = names.into_iter().collect();
        say(&format!(
            "Wrote {} ({}).",
            self.combined().display(),
            joined.join(", ")
        ));
        Ok(())
    }

    pub fn print_plan(&self) {
        let mut say = |m: &str| println!("{m}");
        let our_files = self.our_ui_targets(None);
        let scripts = self.discover_scripts(&mut say, Some(&our_files));
        println!("Scripts found:");
        for script in &scripts {
            let file = script.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let folder = script.folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| script.name.clone());
            println!(
                "  {folder} {file} applyable={}",
                script.apply
            );
        }
        let jobs = self.merge_jobs(&scripts);
        println!("Jobs:");
        for (target, names) in &jobs {
            println!("  {target}: {}", names.join(", "));
        }
    }

    fn script_rows(&self) -> Vec<(PathBuf, String)> {
        let mut rows = Vec::new();
        if !self.mods.is_dir() {
            return rows;
        }
        let mut folders: BTreeMap<String, String> = BTreeMap::new();
        let mut dir_names: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.mods) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == COMBINED_NAME {
                    continue;
                }
                folders.insert(name.to_ascii_lowercase(), name.clone());
                dir_names.push(name);
            }
        }
        dir_names.sort();
        for name in &dir_names {
            let folder = self.mods.join(name);
            let mut luas: Vec<PathBuf> = glob_lua(&folder);
            luas.sort();
            for path in luas {
                rows.push((path, name.clone()));
            }
        }
        let mut loose: Vec<PathBuf> = glob_lua(&self.mods);
        loose.sort();
        for path in loose {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let name = folders.get(&stem.to_ascii_lowercase()).cloned().unwrap_or(stem);
            rows.push((path, name));
        }
        rows
    }

    fn text_mentions(&self, path: &Path, ours: &BTreeSet<String>) -> bool {
        let Ok(bytes) = fs::read(path) else {
            return false;
        };
        let text = String::from_utf8_lossy(strip_bom(&bytes)).to_uppercase();
        for target in ours {
            if text.contains(&target.to_uppercase()) {
                return true;
            }
            let name = target.rsplit('/').next().unwrap_or(target);
            if text.contains(&name.to_uppercase()) {
                return true;
            }
        }
        false
    }

    pub fn discover_scripts(
        &self,
        say: &mut dyn FnMut(&str),
        our_files: Option<&BTreeSet<String>>,
    ) -> Vec<Script> {
        let mut found = Vec::new();
        let ours: BTreeSet<String> = our_files
            .map(|set| set.iter().map(|p| p.to_uppercase()).collect())
            .unwrap_or_default();
        for (path, name) in self.script_rows() {
            if !OUR_ORDER.contains(&name.as_str()) && !ours.is_empty() && !self.text_mentions(&path, &ours)
            {
                continue;
            }
            let container = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                parse_mod_script_file(&path)
            })) {
                Ok(value) => value,
                Err(_) => {
                    if OUR_ORDER.contains(&name.as_str()) {
                        say(&format!(
                            "{} in {name} was skipped: panic while parsing",
                            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                        ));
                    }
                    continue;
                }
            };
            let Some(container) = container else {
                continue;
            };
            let targets = script_targets(&container);
            let touches_ours = OUR_ORDER.contains(&name.as_str()) || !targets.is_disjoint(&ours);
            if !touches_ours {
                continue;
            }
            let apply = script_applyable(&container);
            if !apply {
                let named = if targets.is_empty() {
                    "no files".to_string()
                } else {
                    targets.iter().cloned().collect::<Vec<_>>().join(", ")
                };
                let reason = container
                    .get("_skip_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("it builds its table in Lua");
                say(&format!(
                    "{} in {name} names {named}; not applied ({reason}). If AMUMSS already wrote that MBIN in this folder, it is still combined when another script edits the same file.",
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                ));
            }
            let folder = if self.mods.join(&name).is_dir() {
                self.mods.join(&name)
            } else {
                path.parent().unwrap_or(&self.mods).to_path_buf()
            };
            found.push(Script {
                folder,
                name,
                path,
                container,
                apply,
            });
        }
        found.sort_by(|a, b| {
            order_tuple(self.order(&a.name), &a.name)
                .cmp(&order_tuple(self.order(&b.name), &b.name))
                .then_with(|| {
                    let an = a.path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    let bn = b.path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    an.cmp(&bn)
                })
        });
        found
    }

    pub fn order(&self, name: &str) -> (u8, OrderSecond) {
        let extra = files::merge_order_names(&self.game_root);
        if let Some(index) = extra.iter().position(|n| n == name) {
            return (2, OrderSecond::Index(index));
        }
        if let Some(index) = OUR_ORDER.iter().position(|n| *n == name) {
            return (0, OrderSecond::Index(index));
        }
        (1, OrderSecond::Name(name.to_lowercase()))
    }

    pub fn our_ui_targets(&self, scripts: Option<&[Script]>) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        if let Some(scripts) = scripts {
            for script in scripts {
                if OUR_ORDER.contains(&script.name.as_str()) {
                    found.extend(script_targets(&script.container));
                }
            }
        } else if self.mods.is_dir() {
            for name in OUR_ORDER {
                let folder = self.mods.join(name);
                if !folder.is_dir() {
                    continue;
                }
                for path in glob_lua(&folder) {
                    if let Some(container) = parse_mod_script_file(&path) {
                        found.extend(script_targets(&container));
                    }
                }
            }
        }
        if self.mods.is_dir() {
            for name in OUR_ORDER {
                let folder = self.mods.join(name);
                if !folder.is_dir() {
                    continue;
                }
                for path in walk_files(&folder) {
                    let Ok(rel) = path.strip_prefix(&folder) else {
                        continue;
                    };
                    let mut rel = rel.to_string_lossy().replace('\\', "/").to_uppercase();
                    if rel.ends_with(".MERGED") {
                        rel.truncate(rel.len() - ".MERGED".len());
                    }
                    if rel.ends_with(".MBIN") {
                        found.insert(rel);
                    }
                }
            }
        }
        found
    }

    pub fn merge_jobs(&self, scripts: &[Script]) -> Vec<(String, Vec<String>)> {
        let ours_files = self.our_ui_targets(Some(scripts));
        let mut claims: Vec<(String, BTreeSet<String>)> = Vec::new();
        let mut applyable: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for script in scripts {
            for target in script_targets(&script.container) {
                if !ours_files.contains(&target) {
                    continue;
                }
                if let Some((_, names)) = claims.iter_mut().find(|(t, _)| t == &target) {
                    names.insert(script.name.clone());
                } else {
                    let mut names = BTreeSet::new();
                    names.insert(script.name.clone());
                    claims.push((target.clone(), names));
                }
                if script.apply {
                    applyable.entry(target).or_default().insert(script.name.clone());
                }
            }
        }
        let mut jobs = Vec::new();
        for (target, names) in &claims {
            let providers = self.providers(target, Some(names));
            let others: BTreeSet<String> = providers.difference(names).cloned().collect();
            let ours: BTreeSet<String> = applyable
                .get(target)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|n| OUR_ORDER.contains(&n.as_str()))
                .collect();
            if names.len() >= 2 || !others.is_empty() || (ours.len() == 1 && providers.is_empty()) {
                let mut combined = names.clone();
                combined.extend(others);
                jobs.push((target.clone(), self.ordered_names(&combined)));
            }
        }
        jobs
    }

    pub fn ordered_names(&self, names: &BTreeSet<String>) -> Vec<String> {
        let mut list: Vec<String> = names.iter().cloned().collect();
        list.sort_by(|a, b| order_tuple(self.order(a), a).cmp(&order_tuple(self.order(b), b)));
        list
    }

    fn providers(&self, target: &str, claimants: Option<&BTreeSet<String>>) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        if !self.mods.is_dir() {
            return found;
        }
        let relative = path_from_target(target);
        let mut scan: BTreeSet<String> = OUR_ORDER.iter().map(|s| (*s).to_string()).collect();
        if let Some(claimants) = claimants {
            scan.extend(claimants.iter().cloned());
        }
        let Ok(entries) = fs::read_dir(&self.mods) else {
            return found;
        };
        for entry in entries.flatten() {
            let folder = entry.path();
            if !folder.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == COMBINED_NAME {
                continue;
            }
            let path = folder.join(&relative);
            if path.is_file() || with_merged_suffix(&path).is_file() {
                found.insert(name);
            } else if scan.contains(&name) && !self.leftover_bins(&folder, target).is_empty() {
                found.insert(name);
            }
        }
        found
    }

    fn leftover_bins(&self, folder: &Path, target: &str) -> Vec<PathBuf> {
        let target_path = Path::new(target);
        let stem = target_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        let want = target_path
            .file_name()
            .map(|s| s.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        let mut hits = Vec::new();
        if !folder.is_dir() {
            return hits;
        }
        for path in walk_files(folder) {
            if path.components().any(|c| c.as_os_str() == COMBINED_NAME) {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_uppercase())
                .unwrap_or_default();
            if name == want
                || name == format!("{want}.MERGED")
                || name == format!("{stem}.MBIN")
                || name == format!("{stem}.MBIN.MERGED")
            {
                hits.push(path);
            }
        }
        hits
    }

    pub fn stamp(&self, scripts: &[Script], jobs: &[(String, Vec<String>)]) -> Result<Value, String> {
        let mut rows = Vec::new();
        for script in scripts {
            let blob = fs::read(&script.path).map_err(|e| e.to_string())?;
            rows.push(json!({
                "mod": script.name,
                "file": script.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                "sha256": sha256_hex(&blob),
            }));
        }
        let mut leftovers = Vec::new();
        for (target, names) in jobs {
            for name in names {
                let folder = self.mods.join(name);
                if !folder.is_dir() {
                    continue;
                }
                for path in self.leftover_bins(&folder, target) {
                    let meta = path.metadata().map_err(|e| e.to_string())?;
                    let rel = path
                        .strip_prefix(&folder)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    leftovers.push(json!({
                        "mod": name,
                        "file": rel,
                        "size": meta.len() as i64,
                        "mtime": mtime_secs(&path),
                    }));
                }
            }
        }
        leftovers.sort_by(|a, b| {
            let am = a.get("mod").and_then(Value::as_str).unwrap_or("");
            let bm = b.get("mod").and_then(Value::as_str).unwrap_or("");
            am.cmp(bm).then_with(|| {
                let af = a.get("file").and_then(Value::as_str).unwrap_or("");
                let bf = b.get("file").and_then(Value::as_str).unwrap_or("");
                af.cmp(bf)
            })
        });
        let order_blob = if self.merge_order_path().is_file() {
            fs::read(self.merge_order_path()).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut paks = Vec::new();
        for pak in self.paks() {
            let meta = pak.metadata().map_err(|e| e.to_string())?;
            paks.push(json!({
                "name": pak.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                "size": meta.len() as i64,
                "mtime": mtime_secs(&pak),
            }));
        }
        let mut job_map = Map::new();
        let mut sorted_jobs = jobs.to_vec();
        sorted_jobs.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, names) in sorted_jobs {
            job_map.insert(path, json!(names));
        }
        Ok(json!({
            "scripts": rows,
            "jobs": job_map,
            "leftovers": leftovers,
            "merge_order": sha256_hex(&order_blob),
            "paks": paks,
            "merge_logic": MERGE_LOGIC,
        }))
    }

    fn combined_ok(&self, jobs: &[(String, Vec<String>)]) -> bool {
        if !self.combined().join("combined.txt").is_file() {
            return false;
        }
        jobs.iter().all(|(path, _)| self.combined().join(path_from_target(path)).is_file())
    }

    fn build(
        &self,
        jobs: &[(String, Vec<String>)],
        scripts: &[Script],
        toolset: &Tools,
        say: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        let work = self.work_path();
        if work.exists() {
            fs::remove_dir_all(&work).map_err(|e| e.to_string())?;
        }
        fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let mut built: Vec<(String, PathBuf)> = Vec::new();
        let result = (|| {
            for (target, names) in jobs {
                say(&format!("  {target}: {}", names.join(", ")));
                let path = self.build_one(target, names, scripts, toolset, &work)?;
                built.push((target.clone(), path));
            }
            self.restore_all(say);
            if self.combined().exists() {
                fs::remove_dir_all(self.combined()).map_err(|e| e.to_string())?;
            }
            for (target, path) in &built {
                let out = self.combined().join(path_from_target(target));
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                fs::copy(path, &out).map_err(|e| e.to_string())?;
            }
            self.disable_sources(jobs, say)?;
            Ok(())
        })();
        let _ = fs::remove_dir_all(&work);
        result
    }

    fn build_one(
        &self,
        target: &str,
        names: &[String],
        scripts: &[Script],
        toolset: &Tools,
        work: &Path,
    ) -> Result<PathBuf, String> {
        let relevant: Vec<&Script> = scripts.iter().filter(|s| names.contains(&s.name)).collect();
        let source_path = extract_source(target, &relevant);
        let base = self.base_mxml(target, names, &source_path, toolset, work)?;
        let mut tree = Mxml::read(&base).map_err(|e| e.to_string())?;
        for script in relevant {
            if !script.apply || claim_only(script, target) {
                continue;
            }
            for row in change_tables(&script.container) {
                let pairs = mbin_sources(&row);
                if !pairs.iter().any(|(extract, write)| write == target || extract == target) {
                    continue;
                }
                let changes = row.get("EXML_CHANGE_TABLE").or_else(|| row.get("MXML_CHANGE_TABLE"));
                let list = as_list(changes.unwrap_or(&Value::Null));
                if list.is_empty() {
                    continue;
                }
                apply_change_table(&mut tree, &list).map_err(|e: ExmlChangeError| e.to_string())?;
            }
        }
        let out_mxml = work.join(format!("{}.MXML", target.replace('/', "_")));
        tree.write(&out_mxml).map_err(|e| e.to_string())?;
        tools::run(&[
            toolset.mbincompiler.as_os_str(),
            OsStr::new("-y"),
            OsStr::new("-q"),
            out_mxml.as_os_str(),
        ])?;
        let mut compiled = out_mxml.with_extension("MBIN");
        if !compiled.is_file() {
            compiled = out_mxml.with_extension("mbin");
        }
        if !compiled.is_file() {
            return Err(format!("MBINCompiler did not write {}", compiled.display()));
        }
        Ok(compiled)
    }

    fn base_mxml(
        &self,
        target: &str,
        names: &[String],
        source: &str,
        toolset: &Tools,
        work: &Path,
    ) -> Result<PathBuf, String> {
        let claim_names: Vec<&String> = names.iter().filter(|name| self.folder_claim_only(name, target)).collect();
        if claim_names.len() == 1 {
            let shipped = self.mods.join(claim_names[0]).join(path_from_target(target));
            let disabled = with_merged_suffix(&shipped);
            let pick = if shipped.is_file() {
                Some(shipped)
            } else if disabled.is_file() {
                Some(disabled)
            } else {
                None
            };
            if let Some(pick) = pick {
                let dest = work.join(format!("{}_shipped.MBIN", target.replace('/', "_")));
                fs::copy(&pick, &dest).map_err(|e| e.to_string())?;
                tools::run(&[
                    toolset.mbincompiler.as_os_str(),
                    OsStr::new("-y"),
                    OsStr::new("-q"),
                    dest.as_os_str(),
                ])?;
                return Ok(dest.with_extension("MXML"));
            }
        }
        if let Some(leftover) = self.best_leftover_bin(names, target) {
            let dest = work.join(format!("{}_leftover.MBIN", target.replace('/', "_")));
            fs::copy(&leftover, &dest).map_err(|e| e.to_string())?;
            tools::run(&[
                toolset.mbincompiler.as_os_str(),
                OsStr::new("-y"),
                OsStr::new("-q"),
                dest.as_os_str(),
            ])?;
            let xml = dest.with_extension("MXML");
            if xml.is_file() {
                return Ok(xml);
            }
        }
        self.extract_vanilla(source, toolset, work)
    }

    fn folder_applies_edits(&self, name: &str, target: &str) -> bool {
        let folder = self.mods.join(name);
        if !folder.is_dir() {
            return false;
        }
        for path in glob_lua(&folder) {
            let Some(container) = parse_mod_script_file(&path) else {
                continue;
            };
            if !script_applyable(&container) {
                continue;
            }
            let fake = Script {
                folder: folder.clone(),
                name: name.to_string(),
                path: path.clone(),
                container: container.clone(),
                apply: true,
            };
            if script_targets(&container).contains(target) && !claim_only(&fake, target) {
                return true;
            }
        }
        false
    }

    fn best_leftover_bin(&self, names: &[String], target: &str) -> Option<PathBuf> {
        let mut hits = Vec::new();
        for name in names {
            if self.folder_applies_edits(name, target) {
                continue;
            }
            hits.extend(self.leftover_bins(&self.mods.join(name), target));
        }
        let live: Vec<PathBuf> = hits
            .iter()
            .filter(|path| {
                !path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_uppercase().ends_with(".MERGED"))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let pool = if live.is_empty() { hits } else { live };
        pool.into_iter().next()
    }

    fn folder_claim_only(&self, name: &str, target: &str) -> bool {
        let folder = self.mods.join(name);
        for path in glob_lua(&folder) {
            let Some(container) = parse_mod_script_file(&path) else {
                continue;
            };
            let fake = Script {
                folder: folder.clone(),
                name: name.to_string(),
                path: path.clone(),
                container,
                apply: true,
            };
            if script_targets(&fake.container).contains(target) {
                return claim_only(&fake, target);
            }
        }
        false
    }

    fn extract_vanilla(&self, source: &str, toolset: &Tools, work: &Path) -> Result<PathBuf, String> {
        let vanilla_root = work.join("vanilla");
        fs::create_dir_all(&vanilla_root).map_err(|e| e.to_string())?;
        let needle = source.to_lowercase();
        let mut extracted = None;
        for pak in self.paks() {
            tools::run(&[
                toolset.hgpaktool.as_os_str(),
                OsStr::new("-U"),
                OsStr::new("-q"),
                OsStr::new("-O"),
                vanilla_root.as_os_str(),
                OsStr::new("-f"),
                OsStr::new(&needle),
                pak.as_os_str(),
            ])?;
            let hits: Vec<PathBuf> = walk_files(&vanilla_root)
                .into_iter()
                .filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("mbin")).unwrap_or(false))
                .collect();
            let match_path = hits.into_iter().find(|p| {
                p.strip_prefix(&vanilla_root)
                    .map(|rel| rel.to_string_lossy().replace('\\', "/").to_lowercase() == needle)
                    .unwrap_or(false)
            });
            if let Some(path) = match_path {
                extracted = Some(path);
                break;
            }
        }
        let extracted = extracted.ok_or_else(|| format!("{source} is not in the game paks"))?;
        tools::run(&[
            toolset.mbincompiler.as_os_str(),
            OsStr::new("-y"),
            OsStr::new("-q"),
            extracted.as_os_str(),
        ])?;
        let mut xml = extracted.with_extension("MXML");
        if !xml.is_file() {
            xml = extracted.with_extension("mxml");
        }
        if !xml.is_file() {
            return Err(format!("MBINCompiler did not write {}", xml.display()));
        }
        Ok(xml)
    }

    pub fn disable_sources(
        &self,
        jobs: &[(String, Vec<String>)],
        say: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        for (target, _) in jobs {
            let relative = path_from_target(target);
            let Ok(entries) = fs::read_dir(&self.mods) else {
                continue;
            };
            for entry in entries.flatten() {
                let folder = entry.path();
                if !folder.is_dir() {
                    continue;
                }
                let folder_name = entry.file_name().to_string_lossy().into_owned();
                if folder_name == COMBINED_NAME {
                    continue;
                }
                let path = folder.join(&relative);
                let mut extras = Vec::new();
                if path.is_file() {
                    extras.push(path.clone());
                }
                for item in self.leftover_bins(&folder, target) {
                    if !extras.contains(&item) {
                        extras.push(item);
                    }
                }
                for item in extras {
                    if !item.is_file() {
                        continue;
                    }
                    let name_upper = item
                        .file_name()
                        .map(|n| n.to_string_lossy().to_uppercase())
                        .unwrap_or_default();
                    if name_upper.ends_with(".MERGED") {
                        continue;
                    }
                    let disabled = with_merged_suffix(&item);
                    if disabled.exists() {
                        fs::remove_file(&disabled).map_err(|e| e.to_string())?;
                    }
                    fs::rename(&item, &disabled).map_err(|e| e.to_string())?;
                    let rel = item
                        .strip_prefix(&folder)
                        .unwrap_or(&item)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let disabled_name = disabled
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    say(&format!("  parked {folder_name}/{rel} as {disabled_name}"));
                }
            }
        }
        Ok(())
    }

    pub fn restore_all(&self, say: &mut dyn FnMut(&str)) {
        if !self.mods.is_dir() {
            return;
        }
        for path in walk_files(&self.mods) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !name.ends_with(MERGED_SUFFIX) {
                continue;
            }
            if path.components().any(|c| c.as_os_str() == COMBINED_NAME) {
                continue;
            }
            let restored = strip_merged_suffix(&path);
            if restored.exists() {
                continue;
            }
            if let Some(parent) = restored.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if fs::rename(&path, &restored).is_ok() {
                let rel = restored
                    .strip_prefix(&self.mods)
                    .unwrap_or(&restored)
                    .to_string_lossy()
                    .replace('\\', "/");
                say(&format!("  restored {rel}"));
            }
        }
    }

    fn write_combined_txt(&self, scripts: &[Script], jobs: &[(String, Vec<String>)]) -> Result<(), String> {
        fs::create_dir_all(self.combined()).map_err(|e| e.to_string())?;
        let mut lines = vec![
            "Combined by the NMS.rs loader from AMUMSS Lua scripts.".to_string(),
            "The game loads these files instead of the copies in the listed mods.".to_string(),
            String::new(),
        ];
        let mut seen: Vec<String> = Vec::new();
        for (_, names) in jobs {
            for name in names {
                if !seen.contains(name) {
                    seen.push(name.clone());
                }
            }
        }
        lines.push("Mods combined:".into());
        for name in &seen {
            let files: Vec<String> = scripts
                .iter()
                .filter(|s| &s.name == name)
                .map(|s| s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                .collect();
            lines.push(format!("  {name} ({})", files.join(", ")));
        }
        lines.push(String::new());
        lines.push("Files:".into());
        let mut sorted_jobs = jobs.to_vec();
        sorted_jobs.sort_by(|a, b| a.0.cmp(&b.0));
        for (target, names) in sorted_jobs {
            lines.push(format!("  {target}"));
            for name in names {
                lines.push(format!("    {name}"));
            }
        }
        lines.push(String::new());
        fs::write(self.combined().join("combined.txt"), lines.join("\n") + "\n").map_err(|e| e.to_string())
    }

    fn paks(&self) -> Vec<PathBuf> {
        ["NMSARC.Precache.pak", "NMSARC.UI.pak"]
            .into_iter()
            .map(|name| self.pcbanks.join(name))
            .filter(|p| p.is_file())
            .collect()
    }

    fn clear_stamp(&self) {
        let path = self.stamp_path();
        if path.is_file() {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderSecond {
    Index(usize),
    Name(String),
}

fn order_tuple(order: (u8, OrderSecond), name: &str) -> (u8, OrderSecond, String) {
    (order.0, order.1, name.to_lowercase())
}

fn extract_source(target: &str, scripts: &[&Script]) -> String {
    for script in scripts {
        for row in change_tables(&script.container) {
            for (extract, write) in mbin_sources(&row) {
                if write == target {
                    return extract;
                }
            }
        }
    }
    target.to_string()
}

fn claim_only(script: &Script, target: &str) -> bool {
    for row in change_tables(&script.container) {
        if !mbin_sources(&row)
            .iter()
            .any(|(extract, write)| write == target || extract == target)
        {
            continue;
        }
        let changes = row.get("EXML_CHANGE_TABLE").or_else(|| row.get("MXML_CHANGE_TABLE"));
        if py_truthy(changes.unwrap_or(&Value::Null)) {
            return false;
        }
    }
    true
}

pub fn as_list(value: &Value) -> Vec<Value> {
    if let Some(list) = value.as_array() {
        return list.clone();
    }
    if let Some(map) = value.as_object() {
        let mut keyed: Vec<(i64, Value)> = Vec::new();
        let mut ok = true;
        for (key, item) in map {
            match key.parse::<i64>() {
                Ok(n) => keyed.push((n, item.clone())),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            keyed.sort_by_key(|(n, _)| *n);
            return keyed.into_iter().map(|(_, item)| item).collect();
        }
        return map.values().cloned().collect();
    }
    Vec::new()
}

fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn jobs_to_map(jobs: &[(String, Vec<String>)]) -> BTreeMap<String, Vec<String>> {
    jobs.iter().cloned().collect()
}

fn path_from_target(target: &str) -> PathBuf {
    target.split('/').collect()
}

fn with_merged_suffix(path: &Path) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(MERGED_SUFFIX);
    PathBuf::from(raw)
}

fn strip_merged_suffix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    PathBuf::from(&s[..s.len().saturating_sub(MERGED_SUFFIX.len())])
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn mtime_secs(path: &Path) -> i64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn glob_lua(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map(|e| e.eq_ignore_ascii_case("lua")).unwrap_or(false))
        .collect()
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut kids: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        kids.sort();
        for path in kids {
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lua mods of the Rust flow that the old Python loader's OUR_ORDER never listed.
    const RUST_ONLY_LUA_MODS: &[&str] = &["NMSrsMenu", "GalaxyMapFilters"];

    fn temp_merge(tag: &str) -> Merge {
        let dir = std::env::temp_dir().join(format!("nms-launcher-merge-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("GAMEDATA").join("MODS")).unwrap();
        fs::create_dir_all(paths::loader_dir(&dir)).unwrap();
        Merge::for_game(&dir)
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn apply_lua(target: &str) -> String {
        format!(
            "NMS_MOD_DEFINITION_CONTAINER = {{\n\
               MODIFICATIONS = {{{{\n\
                 MBIN_CHANGE_TABLE = {{{{\n\
                   MBIN_FILE_SOURCE = \"{target}\",\n\
                   EXML_CHANGE_TABLE = {{{{\n\
                     SPECIAL_KEY_WORDS = {{\"ID\", \"X\"}},\n\
                     VALUE_CHANGE_TABLE = {{{{\"Y\", \"1\"}}}},\n\
                   }}}},\n\
                 }}}},\n\
               }}}},\n\
             }}\n"
        )
    }

    fn claim_lua(target: &str) -> String {
        format!(
            "NMS_MOD_DEFINITION_CONTAINER = {{\n\
               MODIFICATIONS = {{{{\n\
                 MBIN_CHANGE_TABLE = {{{{\n\
                   MBIN_FILE_SOURCE = \"{target}\",\n\
                   EXML_CHANGE_TABLE = {{}},\n\
                 }}}},\n\
               }}}},\n\
             }}\n"
        )
    }

    fn cleanup(m: &Merge) {
        let _ = fs::remove_dir_all(&m.game_root);
    }

    #[test]
    fn two_scripts_on_one_file_make_a_job() {
        let m = temp_merge("two");
        let target = "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN";
        write(&m.mods.join("SmallerInventorySlots").join("a.lua"), &apply_lua(target));
        write(&m.mods.join("QuickTransferAll").join("b.lua"), &apply_lua(target));
        write(&m.mods.join("SmallerInventorySlots").join("UI").join("COMPONENTS").join("PAGESELECTBARSIDE.MBIN"), "x");
        let mut notes = Vec::new();
        let mut say = |s: &str| notes.push(s.to_string());
        let our = m.our_ui_targets(None);
        let scripts = m.discover_scripts(&mut say, Some(&our));
        let jobs = m.merge_jobs(&scripts);
        assert_eq!(jobs.len(), 1, "{jobs:?} notes={notes:?}");
        assert_eq!(jobs[0].0, target);
        assert_eq!(jobs[0].1, vec!["SmallerInventorySlots".to_string(), "QuickTransferAll".to_string()]);
        cleanup(&m);
    }

    #[test]
    fn applyable_without_mbin_is_quick_deposit_job() {
        let m = temp_merge("qd");
        let target = "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN";
        write(&m.mods.join("QuickTransferAll").join("QuickDeposit.lua"), &apply_lua(target));
        let mut say = |_: &str| {};
        let our = m.our_ui_targets(None);
        let scripts = m.discover_scripts(&mut say, Some(&our));
        let jobs = m.merge_jobs(&scripts);
        assert_eq!(jobs, vec![(target.to_string(), vec!["QuickTransferAll".to_string()])], "{jobs:?}");
        cleanup(&m);
    }

    #[test]
    fn one_owner_with_mbin_makes_no_job() {
        let m = temp_merge("one");
        let target = "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN";
        write(&m.mods.join("SmallerInventorySlots").join("a.lua"), &claim_lua(target));
        write(&m.mods.join("SmallerInventorySlots").join("UI").join("COMPONENTS").join("PAGESELECTBARSIDE.MBIN"), "x");
        let mut say = |_: &str| {};
        let our = m.our_ui_targets(None);
        let scripts = m.discover_scripts(&mut say, Some(&our));
        let jobs = m.merge_jobs(&scripts);
        assert!(jobs.is_empty(), "{jobs:?}");
        cleanup(&m);
    }

    #[test]
    fn merge_order_txt_moves_a_folder_last() {
        let m = temp_merge("order");
        write(&paths::loader_dir(&m.game_root).join("merge_order.txt"), "SomeoneElse\n");
        let names: BTreeSet<String> = ["SomeoneElse", "AlsoThem", "SmallerInventorySlots"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(
            m.ordered_names(&names),
            vec!["SmallerInventorySlots", "AlsoThem", "SomeoneElse"]
        );
        cleanup(&m);
    }

    #[test]
    fn restore_and_disable_renames() {
        let m = temp_merge("rename");
        let rel = PathBuf::from("UI").join("COMPONENTS").join("PAGESELECTBARSIDE.MBIN");
        let live = m.mods.join("SmallerInventorySlots").join(&rel);
        write(&live, "x");
        let jobs = vec![(
            "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN".to_string(),
            vec!["SmallerInventorySlots".to_string()],
        )];
        let mut notes = Vec::new();
        let mut say = |s: &str| notes.push(s.to_string());
        m.disable_sources(&jobs, &mut say).unwrap();
        drop(say);
        assert!(!live.is_file(), "{notes:?}");
        assert!(with_merged_suffix(&live).is_file());
        notes.clear();
        let mut say = |s: &str| notes.push(s.to_string());
        m.restore_all(&mut say);
        drop(say);
        assert!(live.is_file(), "{notes:?}");
        assert!(!with_merged_suffix(&live).is_file());
        cleanup(&m);
    }

    #[test]
    fn as_list_orders_numeric_keys() {
        let mut map = Map::new();
        map.insert("2".into(), json!({"v": 2}));
        map.insert("1".into(), json!({"v": 1}));
        let list = as_list(&Value::Object(map));
        assert_eq!(list, vec![json!({"v": 1}), json!({"v": 2})]);
        assert_eq!(as_list(&json!([1, 2])), vec![json!(1), json!(2)]);
        assert!(as_list(&Value::Null).is_empty());
    }

    #[test]
    fn stamp_stable_for_the_same_tree() {
        let m = temp_merge("stamp");
        let target = "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN";
        write(&m.mods.join("QuickTransferAll").join("QuickDeposit.lua"), &apply_lua(target));
        let mut say = |_: &str| {};
        let our = m.our_ui_targets(None);
        let scripts = m.discover_scripts(&mut say, Some(&our));
        let jobs = m.merge_jobs(&scripts);
        let a = m.stamp(&scripts, &jobs).unwrap();
        let b = m.stamp(&scripts, &jobs).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["merge_logic"], json!(MERGE_LOGIC));
        cleanup(&m);
    }

    fn game_root_from_manifest() -> Option<PathBuf> {
        let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        paths::find_game_root(&start)
    }

    fn copy_file(src: &Path, dst: &Path) {
        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(src, dst);
    }

    fn copy_tree(src: &Path, dst: &Path) {
        if !src.is_dir() {
            return;
        }
        for path in walk_files(src) {
            let rel = path.strip_prefix(src).unwrap();
            copy_file(&path, &dst.join(rel));
        }
    }

    fn restore_merged_in(mods: &Path) {
        for path in walk_files(mods) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !name.ends_with(MERGED_SUFFIX) {
                continue;
            }
            if path.components().any(|c| c.as_os_str() == COMBINED_NAME) {
                continue;
            }
            let restored = strip_merged_suffix(&path);
            if restored.exists() {
                continue;
            }
            let _ = fs::rename(&path, &restored);
        }
    }

    fn python_jobs(real_root: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
        let lib = real_root.join("NMS.py").join("lib");
        let code = format!(
            "import json, sys\n\
sys.path.insert(0, r'{}')\n\
import amumss_merge as m\n\
scripts = m._discover_scripts(lambda s: None, m._our_ui_targets())\n\
jobs = m._merge_jobs(scripts)\n\
print(json.dumps(jobs, sort_keys=True))\n",
            lib.display()
        );
        let script = std::env::temp_dir().join(format!("nms-merge-jobs-{}.py", std::process::id()));
        fs::write(&script, code).map_err(|e| e.to_string())?;
        let out = nms_core::spawn::command("py")
            .args(["-3", &script.to_string_lossy()])
            .output()
            .map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&script);
        if !out.status.success() {
            return Err(format!(
                "python jobs failed: {} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let parsed: BTreeMap<String, Vec<String>> =
            serde_json::from_str(text.trim()).map_err(|e| format!("{e}: {text}"))?;
        Ok(parsed)
    }

    fn first_diff_lines(a: &str, b: &str) -> String {
        for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
            if la != lb {
                return format!("line {}: rust {la:?} live {lb:?}", i + 1);
            }
        }
        format!("length rust {} live {}", a.lines().count(), b.lines().count())
    }

    #[test]
    #[ignore]
    fn merge_parity() {
        let real = game_root_from_manifest().expect("game folder above this crate");
        let real_mods = real.join("GAMEDATA").join("MODS");
        assert!(real_mods.is_dir(), "{}", real_mods.display());
        let tmp = std::env::temp_dir().join(format!("nms-launcher-parity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mods = tmp.join("GAMEDATA").join("MODS");
        fs::create_dir_all(&mods).unwrap();
        fs::create_dir_all(paths::loader_dir(&tmp)).unwrap();
        if real.join("NMS.rs").join("merge_order.txt").is_file() {
            copy_file(
                &real.join("NMS.rs").join("merge_order.txt"),
                &paths::loader_dir(&tmp).join("merge_order.txt"),
            );
        }
        for src in [
            real.join("Modding").join("Tools").join("MBINCompiler").join("MBINCompiler.exe"),
            real.join("Modding").join("Tools").join("MBINCompiler").join("libMBIN.dll"),
            real.join("Modding").join("Tools").join("HGPAKtool").join("hgpaktool.exe"),
        ] {
            if src.is_file() {
                let folder = src.parent().unwrap().file_name().unwrap();
                copy_file(&src, &paths::loader_dir(&tmp).join("tools").join(folder).join(src.file_name().unwrap()));
            }
        }
        if let Ok(entries) = fs::read_dir(&real_mods) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == COMBINED_NAME {
                    continue;
                }
                if path.is_dir() {
                    let dest = mods.join(&name);
                    fs::create_dir_all(&dest).unwrap();
                    for lua in glob_lua(&path) {
                        copy_file(&lua, &dest.join(lua.file_name().unwrap()));
                    }
                    copy_tree(&path.join("UI"), &dest.join("UI"));
                    for file in walk_files(&path) {
                        let fname = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if fname.to_ascii_lowercase().ends_with(".merged") {
                            let rel = file.strip_prefix(&path).unwrap();
                            copy_file(&file, &dest.join(rel));
                        }
                    }
                } else if path.extension().map(|e| e.eq_ignore_ascii_case("lua")).unwrap_or(false) {
                    copy_file(&path, &mods.join(&name));
                }
            }
        }
        restore_merged_in(&mods);
        let merge = Merge {
            mods: mods.clone(),
            pcbanks: real.join("GAMEDATA").join("PCBANKS"),
            loader: paths::loader_dir(&tmp),
            game_root: tmp.clone(),
        };
        let mut said = Vec::new();
        let mut problems = Vec::new();
        let mut say = |s: &str| said.push(s.to_string());
        let mut problem = |s: &str| problems.push(s.to_string());
        merge.merge_installed_mods(&mut say, &mut problem).unwrap_or_else(|e| {
            panic!("rust merge failed: {e}\nsaid={said:?}\nproblems={problems:?}");
        });
        assert!(problems.is_empty(), "{problems:?} said={said:?}");
        let py_jobs = python_jobs(&real).expect("python job list");
        let mut say2 = |_: &str| {};
        let our = merge.our_ui_targets(None);
        let scripts = merge.discover_scripts(&mut say2, Some(&our));
        let rust_jobs: BTreeMap<String, Vec<String>> = merge.merge_jobs(&scripts).into_iter().collect();
        // The Python loader (history, not extended) never had the Rust-era lua mods, so its
        // OUR_ORDER lacks them and it builds no job for a file only they own. Compare the rest;
        // the byte check below still covers every Rust job.
        let py_known = |job: &(&String, &Vec<String>)| !job.1.iter().all(|n| RUST_ONLY_LUA_MODS.contains(&n.as_str()));
        let shared_jobs: BTreeMap<String, Vec<String>> =
            rust_jobs.iter().filter(py_known).map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(shared_jobs, py_jobs, "job list rust vs python (Rust-only lua mods left out)");
        let live_combined = real_mods.join(COMBINED_NAME);
        let built_combined = merge.combined();
        for (target, _) in &rust_jobs {
            let built = built_combined.join(path_from_target(target));
            let live = live_combined.join(path_from_target(target));
            assert!(built.is_file(), "missing built {}", built.display());
            assert!(live.is_file(), "missing live {}", live.display());
            let a = fs::read(&built).unwrap();
            let b = fs::read(&live).unwrap();
            if a != b {
                let compiler = real
                    .join("Modding")
                    .join("Tools")
                    .join("MBINCompiler")
                    .join("MBINCompiler.exe");
                let dump = tmp.join("diff");
                fs::create_dir_all(&dump).unwrap();
                let left = dump.join("rust.MBIN");
                let right = dump.join("live.MBIN");
                fs::write(&left, &a).unwrap();
                fs::write(&right, &b).unwrap();
                let _ = tools::run(&[compiler.as_os_str(), OsStr::new("-y"), OsStr::new("-q"), left.as_os_str()]);
                let _ = tools::run(&[compiler.as_os_str(), OsStr::new("-y"), OsStr::new("-q"), right.as_os_str()]);
                let lx = fs::read_to_string(left.with_extension("MXML")).unwrap_or_default();
                let rx = fs::read_to_string(right.with_extension("MXML")).unwrap_or_default();
                panic!(
                    "MBIN differs for {target} ({} vs {} bytes): {}",
                    a.len(),
                    b.len(),
                    first_diff_lines(&lx, &rx)
                );
            }
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
