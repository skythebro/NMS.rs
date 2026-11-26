//! Our loader in `Binaries\`. The main zip ships it as xinput9_1_0.dll; players whose
//! xinput9_1_0.dll belongs to another mod use the same loader built as winmm.dll or version.dll
//! (Nexus "DLL only" files), or as an ASI plugin `nms_rs.asi` when that other file is an ASI
//! loader (Ultimate ASI Loader). A file counts as ours by the marker bytes inside it, never by its
//! name. Design: `docs\asi-plugin.md`.

use std::path::{Path, PathBuf};

/// Every DLL name NMS.rs ships under, the main one first.
pub const NAMES: [&str; 3] = ["xinput9_1_0.dll", "winmm.dll", "version.dll"];
pub const MAIN: &str = NAMES[0];
/// File name of the ASI plugin build (`--features as-asi`).
pub const ASI: &str = "nms_rs.asi";
/// `NMS_RS_PROXY_MARKER` in nms-proxy, present in every build.
const MARKER: &[u8] = b"NMS.rs proxy marker";
/// `NMS_RS_ASI_MARKER`, only in the ASI build (it has no proxy exports).
const ASI_MARKER: &[u8] = b"NMS.rs asi marker";
/// Names Ultimate ASI Loader x64 is shipped as.
pub const ASI_LOADER_NAMES: [&str; 19] = [
    "xinput9_1_0.dll", "winmm.dll", "version.dll", "dinput8.dll", "dxgi.dll", "d3d9.dll", "d3d10.dll",
    "d3d11.dll", "d3d12.dll", "dsound.dll", "wininet.dll", "winhttp.dll", "binkw64.dll", "bink2w64.dll",
    "xinput1_1.dll", "xinput1_2.dll", "xinput1_3.dll", "xinput1_4.dll", "xinputuap.dll",
];
/// The ones the game is known to load at startup: NMS.exe imports xinput9_1_0, winmm and
/// winhttp; version comes in through the PlayFab, DLSS and Streamline DLLs. An ASI loader under any
/// other name may never load, so it does not count for installing our .asi.
pub const ASI_LOADER_EFFECTIVE: [&str; 4] = ["xinput9_1_0.dll", "winmm.dll", "winhttp.dll", "version.dll"];
/// Export only Ultimate ASI Loader has.
const ASI_LOADER_EXPORT: &str = "IsUltimateASILoader";
/// Folders under `Binaries\` that Ultimate ASI Loader loads `*.asi` from after its own folder, each
/// with one level of subfolders (`LoadRecursively`, on by default).
const ASI_DIRS: [&str; 3] = ["scripts", "plugins", "update"];

pub fn path(root: &Path, name: &str) -> PathBuf {
    root.join("Binaries").join(name)
}

fn has(path: &Path, marker: &[u8]) -> bool {
    std::fs::read(path).map(|b| b.windows(marker.len()).any(|w| w == marker)).unwrap_or(false)
}

pub fn is_ours(path: &Path) -> bool {
    has(path, MARKER)
}

/// The ASI build: fine as `*.asi`, breaks the game under a DLL name NMS.exe imports.
pub fn is_asi_build(path: &Path) -> bool {
    has(path, ASI_MARKER)
}

/// The DLL names under which an NMS.rs loader DLL sits in `Binaries\`, in `NAMES` order.
pub fn installed(root: &Path) -> Vec<&'static str> {
    NAMES.iter().copied().filter(|n| is_ours(&path(root, n))).collect()
}

/// `*.asi` files of ours where an ASI loader looks, as paths relative to `Binaries\`
/// (`nms_rs.asi`, `scripts\nms_rs.asi`), in Ultimate ASI Loader's load order.
pub fn installed_asi(root: &Path) -> Vec<String> {
    let bin = root.join("Binaries");
    let mut found = Vec::new();
    let mut scan = |dir: &Path| {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut names: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("asi")))
            .collect();
        names.sort();
        for p in names.into_iter().filter(|p| is_ours(p)) {
            if let Ok(rel) = p.strip_prefix(&bin) {
                found.push(rel.display().to_string());
            }
        }
    };
    scan(&bin);
    for dir in ASI_DIRS {
        let top = bin.join(dir);
        let mut subs: Vec<PathBuf> =
            std::fs::read_dir(&top).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        for sub in subs {
            scan(&sub);
        }
        scan(&top);
    }
    found
}

/// Every loader file of ours, DLL names first, then .asi paths (relative to `Binaries\`).
pub fn installed_all(root: &Path) -> Vec<String> {
    installed(root).into_iter().map(str::to_string).chain(installed_asi(root)).collect()
}

/// Names of a PE file's exports. Empty for anything that is not a 64-bit PE with an export table.
pub fn export_names(bytes: &[u8]) -> Vec<String> {
    fn exports(b: &[u8]) -> Option<Vec<String>> {
        let u32_at = |at: usize| b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        let headers = nms_core::image::parse_headers(b).ok()?;
        let nt = u32_at(0x3C)? as usize;
        let optional = nt + 24;
        let export_rva = u32_at(optional + 112)? as usize;
        if export_rva == 0 {
            return Some(Vec::new());
        }
        let off = |rva: usize| {
            headers
                .sections
                .iter()
                .find(|s| s.rva <= rva && rva < s.rva + s.virtual_size.max(s.raw_size))
                .map(|s| rva - s.rva + s.raw_offset)
        };
        let dir = off(export_rva)?;
        let count = u32_at(dir + 24)? as usize;
        let names = off(u32_at(dir + 32)? as usize)?;
        let mut out = Vec::with_capacity(count.min(4096));
        for i in 0..count.min(4096) {
            let start = off(u32_at(names + 4 * i)? as usize)?;
            let end = start + b.get(start..)?.iter().position(|&c| c == 0)?;
            out.push(String::from_utf8_lossy(&b[start..end]).into_owned());
        }
        Some(out)
    }
    exports(bytes).unwrap_or_default()
}

/// Another mod's file that is Ultimate ASI Loader (it exports `IsUltimateASILoader`).
pub fn is_asi_loader(path: &Path) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => {
            !bytes.windows(MARKER.len()).any(|w| w == MARKER)
                && export_names(&bytes).iter().any(|n| n == ASI_LOADER_EXPORT)
        }
        Err(_) => false,
    }
}

/// The file name of an ASI loader in `Binaries\` under a name the game loads, if there is one.
pub fn asi_loader(root: &Path) -> Option<&'static str> {
    ASI_LOADER_EFFECTIVE.iter().copied().find(|n| is_asi_loader(&path(root, n)))
}

/// ASI loaders under names the game is not known to load (`dinput8.dll`, say).
pub fn idle_asi_loaders(root: &Path) -> Vec<&'static str> {
    ASI_LOADER_NAMES
        .iter()
        .copied()
        .filter(|n| !ASI_LOADER_EFFECTIVE.contains(n) && is_asi_loader(&path(root, n)))
        .collect()
}

/// `LoadFromScriptsOnly=1` in the loader's ini files (later files win, as in the loader):
/// `*.asi` directly in `Binaries\` are then skipped.
pub fn scripts_only(root: &Path, loader: &str) -> bool {
    let bin = root.join("Binaries");
    let stem = loader.trim_end_matches(".dll");
    let files = [
        bin.join(format!("{stem}.ini")),
        bin.join("global.ini"),
        bin.join("scripts").join("global.ini"),
        bin.join("plugins").join("global.ini"),
        bin.join("update").join("global.ini"),
    ];
    let mut value = false;
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let mut section = String::new();
        for line in text.lines() {
            let line = line.split([';', '#']).next().unwrap_or("").trim();
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_ascii_lowercase();
            } else if let Some((key, v)) = line.split_once('=') {
                if section == "globalsets" && key.trim().eq_ignore_ascii_case("LoadFromScriptsOnly") {
                    value = v.trim().parse::<i64>().map(|n| n != 0).unwrap_or(false);
                }
            }
        }
    }
    value
}

/// Where `install --as asi` puts the plugin, relative to `Binaries\`: an existing one of ours stays
/// where it is; otherwise the loader's own folder, which it loads before `scripts\` and `plugins\`
/// (so our code snapshot comes before other plugins' hooks), or `scripts\` with LoadFromScriptsOnly.
pub fn asi_target(root: &Path, loader: Option<&str>) -> String {
    if let Some(existing) = installed_asi(root).into_iter().next() {
        return existing;
    }
    match loader {
        Some(l) if scripts_only(root, l) => format!("scripts\\{ASI}"),
        _ => ASI.to_string(),
    }
}

/// Steam launch option for Proton, which otherwise prefers Wine's builtin DLL of that name.
pub fn wine_launch_option(name: &str) -> String {
    format!("WINEDLLOVERRIDES=\"{}=n,b\" %command%", name.trim_end_matches(".dll"))
}

/// The DLL that needs the Proton override for these loader files: ours, or the ASI loader's when
/// ours is an .asi (loaded by full path, it needs none).
pub fn wine_dll(root: &Path, loaders: &[String]) -> String {
    match loaders {
        [one] if one.to_ascii_lowercase().ends_with(".asi") => asi_loader(root).unwrap_or(MAIN).to_string(),
        [one] => one.clone(),
        _ => MAIN.to_string(),
    }
}

/// "Two NMS.rs loader files ..." when more than one is installed; the first to load runs the mods.
pub fn twice_warning<S: AsRef<str>>(names: &[S]) -> Option<String> {
    (names.len() > 1).then(|| {
        let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
        format!(
            "NMS.rs loader files: {} are all in Binaries. Only one runs the mods; delete the ones you do not need while the game is closed.",
            names.join(", ")
        )
    })
}

/// Problems with an .asi setup: plugin without a loader, plugin the loader skips, ASI build under
/// a DLL name.
pub fn asi_warnings(root: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    for name in installed(root) {
        if is_asi_build(&path(root, name)) {
            lines.push(format!(
                "Binaries\\{name} is the NMS.rs ASI plugin build, which has no {name} functions: the game will not start with it. Rename it to {ASI} or reinstall NMS.rs."
            ));
        }
    }
    let asi = installed_asi(root);
    if asi.is_empty() {
        return lines;
    }
    match asi_loader(root) {
        None => lines.push(format!(
            "Binaries\\{} is NMS.rs as an ASI plugin, but no ASI loader was found in Binaries, so it will not load.",
            asi[0]
        )),
        Some(loader) => {
            if scripts_only(root, loader) {
                for rel in asi.iter().filter(|r| !r.contains('\\')) {
                    lines.push(format!(
                        "Binaries\\{rel} will not load: the ASI loader's ini sets LoadFromScriptsOnly=1. Move it to Binaries\\scripts."
                    ));
                }
            }
        }
    }
    lines
}

/// Status lines for `prepare` and Pack logs.
pub fn status_lines(root: &Path) -> Vec<String> {
    let names = installed_all(root);
    let mut lines = vec![match names.as_slice() {
        [] => "NMS.rs loader file: none in Binaries".to_string(),
        many => format!("NMS.rs loader file: Binaries\\{}", many.join(", Binaries\\")),
    }];
    lines.extend(twice_warning(&names));
    let foreign: Vec<&str> = NAMES.iter().copied().filter(|n| path(root, n).is_file() && !names.iter().any(|o| o == n)).collect();
    if !foreign.is_empty() {
        lines.push(format!("Other mods' files with an NMS.rs name: Binaries\\{}", foreign.join(", Binaries\\")));
    }
    if let Some(loader) = asi_loader(root) {
        lines.push(format!("ASI loader: Binaries\\{loader} (Ultimate ASI Loader)"));
    }
    for idle in idle_asi_loaders(root) {
        lines.push(format!("ASI loader under a name the game is not known to load: Binaries\\{idle}"));
    }
    lines.extend(asi_warnings(root));
    lines
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nms-proxy-names-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Binaries")).unwrap();
        dir
    }

    /// A minimal PE32+ file with one section and an export table naming `names`.
    pub(crate) fn fake_pe(names: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let fh = 0x84;
        b[fh + 2..fh + 4].copy_from_slice(&1u16.to_le_bytes());
        b[fh + 16..fh + 18].copy_from_slice(&0xF0u16.to_le_bytes());
        let opt = fh + 20;
        b[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        b[opt + 56..opt + 60].copy_from_slice(&0x2000u32.to_le_bytes());
        b[opt + 60..opt + 64].copy_from_slice(&0x200u32.to_le_bytes());
        // Export directory at RVA 0x1000 (file offset 0x200).
        b[opt + 112..opt + 116].copy_from_slice(&0x1000u32.to_le_bytes());
        let sec = opt + 0xF0;
        b[sec..sec + 5].copy_from_slice(b".edat");
        b[sec + 8..sec + 12].copy_from_slice(&0x200u32.to_le_bytes());
        b[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sec + 16..sec + 20].copy_from_slice(&0x200u32.to_le_bytes());
        b[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes());
        let dir = 0x200;
        b[dir + 24..dir + 28].copy_from_slice(&(names.len() as u32).to_le_bytes());
        let table_rva = 0x1000 + 0x40;
        b[dir + 32..dir + 36].copy_from_slice(&(table_rva as u32).to_le_bytes());
        let mut text_rva = 0x1000 + 0x80;
        for (i, name) in names.iter().enumerate() {
            let at = 0x200 + 0x40 + 4 * i;
            b[at..at + 4].copy_from_slice(&(text_rva as u32).to_le_bytes());
            let off = text_rva - 0x1000 + 0x200;
            b[off..off + name.len()].copy_from_slice(name.as_bytes());
            text_rva += name.len() + 1;
        }
        b
    }

    #[test]
    fn ours_by_marker_not_by_name() {
        let root = temp_root("marker");
        std::fs::write(path(&root, "winmm.dll"), b"MZ...NMS.rs proxy marker\0...").unwrap();
        std::fs::write(path(&root, "xinput9_1_0.dll"), b"MZ snap angles").unwrap();
        assert_eq!(installed(&root), ["winmm.dll"]);
        assert_eq!(wine_launch_option("winmm.dll"), "WINEDLLOVERRIDES=\"winmm=n,b\" %command%");
        let lines = status_lines(&root);
        assert_eq!(lines[0], "NMS.rs loader file: Binaries\\winmm.dll");
        assert!(lines.iter().any(|l| l.contains("Other mods' files") && l.contains("xinput9_1_0.dll")), "{lines:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn two_of_ours_warn() {
        let root = temp_root("twice");
        for name in ["xinput9_1_0.dll", "version.dll"] {
            std::fs::write(path(&root, name), b"NMS.rs proxy marker").unwrap();
        }
        assert_eq!(installed(&root), ["xinput9_1_0.dll", "version.dll"]);
        assert!(twice_warning(&installed(&root)).unwrap().contains("xinput9_1_0.dll, version.dll"));
        assert!(twice_warning(&["winmm.dll"]).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn export_names_reads_the_table() {
        assert_eq!(export_names(&fake_pe(&["IsUltimateASILoader", "XInputGetState"])), ["IsUltimateASILoader", "XInputGetState"]);
        assert!(export_names(b"MZ not a pe").is_empty());
    }

    /// The real files when they are around: our built loader has no such export, System32's
    /// xinput neither.
    #[test]
    fn system_dll_is_no_asi_loader() {
        let sys = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("System32");
        let real = sys.join("xinput9_1_0.dll");
        if real.is_file() {
            assert!(!is_asi_loader(&real));
            assert!(export_names(&std::fs::read(&real).unwrap()).iter().any(|n| n == "XInputGetState"));
        }
    }

    /// The real Ultimate ASI Loader file (read, never loaded) when `NMS_RS_UAL_DLL` points at one.
    #[test]
    fn real_ultimate_asi_loader_is_detected() {
        let Some(file) = std::env::var_os("NMS_RS_UAL_DLL") else { return };
        let root = temp_root("real-ual");
        std::fs::copy(&file, path(&root, MAIN)).unwrap();
        assert_eq!(asi_loader(&root), Some(MAIN));
        assert!(installed(&root).is_empty(), "not ours");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn asi_found_in_every_folder_ual_searches() {
        let root = temp_root("asi");
        let bin = root.join("Binaries");
        for dir in ["scripts\\sub", "plugins", "update", "other"] {
            std::fs::create_dir_all(bin.join(dir)).unwrap();
        }
        std::fs::write(bin.join("nms_rs.asi"), b"NMS.rs proxy marker").unwrap();
        std::fs::write(bin.join("scripts\\sub\\renamed.ASI"), b"NMS.rs proxy marker").unwrap();
        std::fs::write(bin.join("plugins\\ImperialGalacticMap.asi"), b"someone else").unwrap();
        std::fs::write(bin.join("update\\nms_rs.asi"), b"NMS.rs proxy marker").unwrap();
        std::fs::write(bin.join("other\\nms_rs.asi"), b"NMS.rs proxy marker").unwrap();
        assert_eq!(installed_asi(&root), ["nms_rs.asi", "scripts\\sub\\renamed.ASI", "update\\nms_rs.asi"]);
        assert!(twice_warning(&installed_all(&root)).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn asi_loader_detection_and_warnings() {
        let root = temp_root("loader");
        let bin = root.join("Binaries");
        std::fs::write(bin.join("nms_rs.asi"), b"NMS.rs proxy marker NMS.rs asi marker").unwrap();
        // No loader yet: the plugin would never load.
        assert!(asi_warnings(&root).iter().any(|l| l.contains("no ASI loader")), "{:?}", asi_warnings(&root));
        assert_eq!(wine_dll(&root, &installed_all(&root)), MAIN);
        std::fs::write(bin.join("xinput9_1_0.dll"), fake_pe(&["IsUltimateASILoader", "XInputGetState"])).unwrap();
        assert_eq!(asi_loader(&root), Some("xinput9_1_0.dll"));
        assert!(asi_warnings(&root).is_empty());
        assert_eq!(asi_target(&root, Some("xinput9_1_0.dll")), "nms_rs.asi");
        // LoadFromScriptsOnly in the ini: the root .asi is skipped.
        std::fs::write(bin.join("xinput9_1_0.ini"), "[GlobalSets]\nLoadFromScriptsOnly=1 ; only scripts\n").unwrap();
        assert!(scripts_only(&root, "xinput9_1_0.dll"));
        assert!(asi_warnings(&root).iter().any(|l| l.contains("LoadFromScriptsOnly")));
        // A later ini wins.
        std::fs::create_dir_all(bin.join("scripts")).unwrap();
        std::fs::write(bin.join("scripts\\global.ini"), "[GlobalSets]\nLoadFromScriptsOnly = 0\n").unwrap();
        assert!(!scripts_only(&root, "xinput9_1_0.dll"));
        std::fs::write(bin.join("scripts\\global.ini"), "[Other]\nLoadFromScriptsOnly = 0\n").unwrap();
        assert!(scripts_only(&root, "xinput9_1_0.dll"));
        std::fs::remove_file(bin.join("nms_rs.asi")).unwrap();
        assert_eq!(asi_target(&root, Some("xinput9_1_0.dll")), "scripts\\nms_rs.asi");
        let lines = status_lines(&root);
        assert!(lines.iter().any(|l| l == "ASI loader: Binaries\\xinput9_1_0.dll (Ultimate ASI Loader)"), "{lines:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn asi_build_under_a_dll_name_is_flagged() {
        let root = temp_root("asidll");
        std::fs::write(path(&root, "winmm.dll"), b"NMS.rs proxy marker NMS.rs asi marker").unwrap();
        let w = asi_warnings(&root);
        assert!(w.iter().any(|l| l.contains("winmm.dll is the NMS.rs ASI plugin build")), "{w:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn proton_option_names_the_asi_loader() {
        let root = temp_root("wine");
        std::fs::write(path(&root, "winmm.dll"), fake_pe(&["IsUltimateASILoader"])).unwrap();
        std::fs::write(path(&root, "nms_rs.asi"), b"NMS.rs proxy marker").unwrap();
        assert_eq!(wine_dll(&root, &installed_all(&root)), "winmm.dll");
        assert_eq!(wine_dll(&root, &["version.dll".to_string()]), "version.dll");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ultimate ASI Loader as dinput8.dll: NMS is not known to load that name, so it does not count.
    #[test]
    fn asi_loader_under_an_idle_name_does_not_count() {
        let root = temp_root("idle");
        std::fs::write(path(&root, "dinput8.dll"), fake_pe(&["IsUltimateASILoader"])).unwrap();
        assert_eq!(asi_loader(&root), None);
        assert_eq!(idle_asi_loaders(&root), ["dinput8.dll"]);
        assert!(status_lines(&root).iter().any(|l| l.contains("not known to load: Binaries\\dinput8.dll")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
