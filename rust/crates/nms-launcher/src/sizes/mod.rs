//! Sizes builders: extract vanilla MBIN, edit MXML, compile. Grid layout is this card.
//!
//! Not every builder is reached from `main` yet, hence the `dead_code` allow.
#![allow(dead_code)]

use nms_amumss::exml::Mxml;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::{self, Tools};

pub mod apply;
pub mod filter_bar;
pub mod grid;
pub mod inventory;
pub mod layout;

pub struct Job {
    pub source: String,
    pub target: String,
    pub apply: Box<dyn FnOnce(&mut Mxml) -> Result<(), String>>,
}

pub fn transform(
    work: &Path,
    jobs: Vec<Job>,
    pak: &Path,
    tools: &Tools,
) -> Result<HashMap<String, PathBuf>, String> {
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let vanilla_root = work.join("vanilla");
    fs::create_dir_all(&vanilla_root).map_err(|e| e.to_string())?;
    let mut args: Vec<&OsStr> = vec![
        tools.hgpaktool.as_os_str(),
        OsStr::new("-U"),
        OsStr::new("-q"),
        OsStr::new("-O"),
        vanilla_root.as_os_str(),
    ];
    let lowered: Vec<String> = jobs.iter().map(|j| j.source.to_lowercase()).collect();
    for src in &lowered {
        args.push(OsStr::new("-f"));
        args.push(OsStr::new(src));
    }
    args.push(pak.as_os_str());
    tools::run(&args)?;

    let mut found: HashMap<String, PathBuf> = HashMap::new();
    for path in walk_mbin(&vanilla_root) {
        if let Ok(rel) = path.strip_prefix(&vanilla_root) {
            found.insert(rel.to_string_lossy().replace('\\', "/").to_lowercase(), path);
        }
    }

    let mut built: HashMap<String, PathBuf> = HashMap::new();
    for (index, job) in jobs.into_iter().enumerate() {
        let vanilla = found.get(&job.source.to_lowercase()).ok_or_else(|| {
            format!(
                "{} is not in {}",
                job.source,
                pak.file_name()
                    .and_then(OsStr::to_str)
                    .unwrap_or("pak")
            )
        })?;
        tools::run(&[
            tools.mbincompiler.as_os_str(),
            OsStr::new("-y"),
            OsStr::new("-q"),
            vanilla.as_os_str(),
        ])?;
        let mut xml = vanilla.with_extension("MXML");
        if !xml.is_file() {
            xml = vanilla.with_extension("mxml");
        }
        if !xml.is_file() {
            return Err(format!("MBINCompiler did not write {}", xml.display()));
        }
        let mut tree = Mxml::read(&xml)?;
        (job.apply)(&mut tree)?;
        let out_mxml = work.join(format!("{index}.MXML"));
        tree.write(&out_mxml)?;
        tools::run(&[
            tools.mbincompiler.as_os_str(),
            OsStr::new("-y"),
            OsStr::new("-q"),
            out_mxml.as_os_str(),
        ])?;
        let want = format!("{index}.mbin");
        let compiled = fs::read_dir(work)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(OsStr::to_str)
                    .map(|n| n.to_ascii_lowercase() == want)
                    .unwrap_or(false)
            })
            .ok_or_else(|| format!("MBINCompiler did not write {want}"))?;
        built.insert(job.target, compiled);
    }
    Ok(built)
}

fn walk_mbin(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rec(&path, out);
            } else if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("mbin"))
                .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }
    rec(root, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::grid::{grid_from, RowsSpec};
    use super::layout;
    use crate::tools::Tools;
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::{Path, PathBuf};

    struct Case {
        name: &'static str,
        columns: i64,
        rows: RowsSpec,
        window: f64,
    }

    const CASES: &[Case] = &[
        Case {
            name: "8x_auto_x1.4",
            columns: 8,
            rows: RowsSpec::Auto,
            window: 1.4,
        },
        Case {
            name: "6x3_x1.0",
            columns: 6,
            rows: RowsSpec::Count(3),
            window: 1.0,
        },
        Case {
            name: "10x_auto_x1.2",
            columns: 10,
            rows: RowsSpec::Auto,
            window: 1.2,
        },
        Case {
            name: "5x4_x1.4",
            columns: 5,
            rows: RowsSpec::Count(4),
            window: 1.4,
        },
    ];

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repo root")
    }

    /// `NMS_GAME_DIR` when set, else the game folder above a checkout in `<game>\Modding\<repo>`.
    fn game_root() -> PathBuf {
        std::env::var_os("NMS_GAME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../.."))
            .canonicalize()
            .expect("game root")
    }

    fn ui_files(dir: &Path) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        fn rec(dir: &Path, root: &Path, names: &mut BTreeSet<String>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    rec(&path, root, names);
                } else if let Ok(rel) = path.strip_prefix(root) {
                    names.insert(rel.to_string_lossy().replace('\\', "/").to_lowercase());
                }
            }
        }
        rec(dir, dir, &mut names);
        names
    }

    fn first_diff_lines(left: &str, right: &str) -> String {
        for (i, (a, b)) in left.lines().zip(right.lines()).enumerate() {
            if a != b {
                return format!("line {}: rust {a:?} python {b:?}", i + 1);
            }
        }
        format!(
            "length rust {} python {}",
            left.lines().count(),
            right.lines().count()
        )
    }

    fn decompile(compiler: &Path, mbin: &Path) -> String {
        crate::tools::run(&[
            compiler.as_os_str(),
            OsStr::new("-y"),
            OsStr::new("-q"),
            mbin.as_os_str(),
        ])
        .unwrap_or_else(|e| panic!("decompile {}: {e}", mbin.display()));
        let mut xml = mbin.with_extension("MXML");
        if !xml.is_file() {
            xml = mbin.with_extension("mxml");
        }
        fs::read_to_string(&xml).unwrap_or_default()
    }

    /// Decompile a copy of `mbin` inside `scratch`, so a failing run never writes an MXML next to
    /// the golden files (that broke the next run's file-list check).
    fn decompile_copy(compiler: &Path, mbin: &Path, scratch: &Path, tag: &str) -> String {
        let dir = scratch.join(format!("diff-{tag}"));
        let _ = fs::create_dir_all(&dir);
        let copy = dir.join(mbin.file_name().unwrap_or_else(|| OsStr::new("file.MBIN")));
        fs::copy(mbin, &copy).unwrap_or_else(|e| panic!("copy {}: {e}", mbin.display()));
        decompile(compiler, &copy)
    }

    #[test]
    #[ignore]
    fn slots_parity() {
        let repo = repo_root();
        let game = game_root();
        let pcbanks = game.join("GAMEDATA").join("PCBANKS");
        let tools = Tools {
            mbincompiler: game
                .join("Modding")
                .join("Tools")
                .join("MBINCompiler")
                .join("MBINCompiler.exe"),
            hgpaktool: game
                .join("Modding")
                .join("Tools")
                .join("HGPAKtool")
                .join("hgpaktool.exe"),
        };
        assert!(
            tools.mbincompiler.is_file(),
            "MBINCompiler missing at {}",
            tools.mbincompiler.display()
        );
        assert!(
            tools.hgpaktool.is_file(),
            "HGPAKtool missing at {}",
            tools.hgpaktool.display()
        );
        assert!(
            pcbanks.join("NMSARC.Precache.pak").is_file(),
            "pak missing"
        );
        assert!(
            pcbanks.join("NMSARC.UI.pak").is_file(),
            "UI pak missing"
        );

        struct SlotsCase {
            name: &'static str,
            scale: f64,
            filters: bool,
        }
        const SLOTS_CASES: &[SlotsCase] = &[
            SlotsCase {
                name: "70_filters",
                scale: 0.7,
                filters: true,
            },
            SlotsCase {
                name: "70_no_filters",
                scale: 0.7,
                filters: false,
            },
            SlotsCase {
                name: "50_filters",
                scale: 0.5,
                filters: true,
            },
            SlotsCase {
                name: "100_filters",
                scale: 1.0,
                filters: true,
            },
        ];

        for case in SLOTS_CASES {
            let golden = repo
                .join("build")
                .join("golden_slots")
                .join(case.name)
                .join("SmallerInventorySlots")
                .join("UI");
            assert!(
                golden.is_dir(),
                "missing Python golden {}; run py -3 dev/golden_slots.py",
                golden.display()
            );
            let tmp = std::env::temp_dir().join(format!(
                "nms-slots-parity-{}-{}",
                case.name,
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&tmp);
            fs::create_dir_all(&tmp).expect("temp");
            let mod_dir = tmp.join("SmallerInventorySlots");
            let work = tmp.join("work");
            super::inventory::build_slots(
                case.scale,
                case.filters,
                &mod_dir,
                &pcbanks,
                &tools,
                &work,
            )
            .unwrap_or_else(|e| panic!("{} build: {e}", case.name));
            let rust_ui = mod_dir.join("UI");
            let gold_names = ui_files(&golden);
            let rust_names = ui_files(&rust_ui);
            assert_eq!(
                rust_names, gold_names,
                "{} UI file list rust {rust_names:?} python {gold_names:?}",
                case.name
            );
            for name in &gold_names {
                let left = rust_ui.join(name);
                let right = golden.join(name);
                let lb = fs::read(&left).unwrap_or_else(|e| panic!("read {}: {e}", left.display()));
                let rb = fs::read(&right).unwrap_or_else(|e| panic!("read {}: {e}", right.display()));
                if lb == rb {
                    continue;
                }
                let lx = decompile_copy(&tools.mbincompiler, &left, &tmp, "rust");
                let rx = decompile_copy(&tools.mbincompiler, &right, &tmp, "golden");
                panic!(
                    "{} {} bytes differ ({}/{}). {}",
                    case.name,
                    name,
                    lb.len(),
                    rb.len(),
                    first_diff_lines(&lx, &rx)
                );
            }
            let _ = fs::remove_dir_all(&tmp);
        }
    }

    #[test]
    #[ignore]
    fn grid_parity() {
        let repo = repo_root();
        let game = game_root();
        let pcbanks = game.join("GAMEDATA").join("PCBANKS");
        let tools = Tools {
            mbincompiler: game
                .join("Modding")
                .join("Tools")
                .join("MBINCompiler")
                .join("MBINCompiler.exe"),
            hgpaktool: game
                .join("Modding")
                .join("Tools")
                .join("HGPAKtool")
                .join("hgpaktool.exe"),
        };
        assert!(
            tools.mbincompiler.is_file(),
            "MBINCompiler missing at {}",
            tools.mbincompiler.display()
        );
        assert!(
            tools.hgpaktool.is_file(),
            "HGPAKtool missing at {}",
            tools.hgpaktool.display()
        );
        assert!(
            pcbanks.join("NMSARC.Precache.pak").is_file(),
            "pak missing"
        );

        for case in CASES {
            let golden = repo
                .join("build")
                .join("golden_grid")
                .join(case.name)
                .join("BiggerItemGrids")
                .join("UI");
            assert!(
                golden.is_dir(),
                "missing Python golden {}; run py -3 dev/golden_grid.py",
                golden.display()
            );
            let grid = grid_from(Some(case.columns), Some(case.rows), Some(case.window))
                .unwrap_or_else(|e| panic!("{}: {e}", case.name));
            let tmp = std::env::temp_dir().join(format!(
                "nms-grid-parity-{}-{}",
                case.name,
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&tmp);
            fs::create_dir_all(&tmp).expect("temp");
            let mod_dir = tmp.join("BiggerItemGrids");
            let work = tmp.join("work");
            layout::build(&grid, &mod_dir, &pcbanks, &tools, &work)
                .unwrap_or_else(|e| panic!("{} build: {e}", case.name));
            let rust_ui = mod_dir.join("UI");
            let gold_names = ui_files(&golden);
            let rust_names = ui_files(&rust_ui);
            assert_eq!(
                rust_names, gold_names,
                "{} UI file list rust {rust_names:?} python {gold_names:?}",
                case.name
            );
            for name in &gold_names {
                let left = rust_ui.join(name);
                let right = golden.join(name);
                let lb = fs::read(&left).unwrap_or_else(|e| panic!("read {}: {e}", left.display()));
                let rb = fs::read(&right).unwrap_or_else(|e| panic!("read {}: {e}", right.display()));
                if lb == rb {
                    continue;
                }
                let lx = decompile_copy(&tools.mbincompiler, &left, &tmp, "rust");
                let rx = decompile_copy(&tools.mbincompiler, &right, &tmp, "golden");
                panic!(
                    "{} {} bytes differ ({}/{}). {}",
                    case.name,
                    name,
                    lb.len(),
                    rb.len(),
                    first_diff_lines(&lx, &rx)
                );
            }
            let _ = fs::remove_dir_all(&tmp);
        }
    }
}
