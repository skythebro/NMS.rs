//! Check every signature and patch in `nms_sdk` against one NMS.exe.

use std::path::{Path, PathBuf};

use nms_core::{Code, Lookup, Pattern};
use nms_sdk::{ModSigs, SigUse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub owner: &'static str,
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    /// Where the signature (or patch) was found. `None` when it is missing or ambiguous.
    pub rva: Option<usize>,
}

/// Failures that were reviewed and accepted for now: (owner, name, reason). Printed as KNOWN,
/// never hidden. Remove an entry as soon as its card fixes it.
pub const KNOWN: &[(&str, &str, &str)] = &[
    (
        "QuickTransferAll",
        "CONFIRM_SLOW_LOAD",
        "4 hits by design, all read the same global via the same RIP; the Rust port must check they agree",
    ),
    ("PulseTweaks", "OBS", "withdrawn patch, never applied; pattern hard-codes a call rel32"),
    ("PulseTweaks", "Pulse in atmosphere (path obstruct)", "withdrawn patch, never applied; uses OBS"),
];

pub fn known_reason(line: &Line) -> Option<&'static str> {
    KNOWN.iter().find(|(o, n, _)| *o == line.owner && *n == line.name).map(|(_, _, r)| *r)
}

fn describe(lookup: Lookup) -> (bool, String, Option<usize>) {
    match lookup {
        Lookup::Unique(rva) => (true, format!("unique at RVA {rva:#x}"), Some(rva)),
        Lookup::Missing => (false, "MISSING".into(), None),
        Lookup::Multiple(_) => (false, "MATCHES MORE THAN ONCE".into(), None),
    }
}

pub fn check(code: &dyn Code, tables: &[ModSigs]) -> Vec<Line> {
    let mut lines = Vec::new();
    for m in tables {
        for s in m.sigs {
            let (ok, detail, rva) = match Pattern::parse(s.pattern) {
                Ok(p) => describe(code.find_unique(&p)),
                Err(e) => (false, e.to_string(), None),
            };
            lines.push(Line { owner: m.owner, name: s.name, ok, detail, rva });
        }
        for p in m.patches {
            let (ok, detail, rva) = match p.locate(code) {
                Ok(rva) => {
                    let site = rva + p.at;
                    let state = match code.read(site, p.old.len()) {
                        Some(b) if b == p.old => "vanilla bytes",
                        Some(b) if b == p.new => "already patched",
                        _ => "UNEXPECTED BYTES",
                    };
                    (state != "UNEXPECTED BYTES", format!("unique at RVA {rva:#x}, {state}"), Some(rva))
                }
                Err(outcome) => (false, format!("{outcome:?}"), None),
            };
            lines.push(Line { owner: m.owner, name: p.label, ok, detail, rva });
        }
    }
    lines
}

/// How one signature changed between two builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delta {
    /// Found in both builds, same place, same result (or failing in both).
    Same,
    /// Found in both builds, but at a different RVA. Usually harmless: the hook still lands.
    Moved,
    /// Found in the old build, not unique (or missing) in the new one. The mod that owns it is broken.
    Broke,
    /// Failing in the old build, found in the new one.
    Fixed,
    /// Only in the new tables (a mod added a signature).
    New,
    /// Only in the old tables (a signature was removed).
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub owner: &'static str,
    pub name: &'static str,
    pub delta: Delta,
    pub old: Option<Line>,
    pub new: Option<Line>,
}

/// Match two `check` results by (owner, name) and say what changed. Order follows `new`, then
/// signatures that only existed in `old`.
pub fn compare(old: &[Line], new: &[Line]) -> Vec<Change> {
    let mut out = Vec::new();
    for n in new {
        let found = old.iter().find(|o| o.owner == n.owner && o.name == n.name);
        let (delta, old_line) = match found {
            None => (Delta::New, None),
            Some(o) => {
                let delta = match (o.ok, n.ok) {
                    (true, false) => Delta::Broke,
                    (false, true) => Delta::Fixed,
                    (true, true) if o.rva != n.rva => Delta::Moved,
                    _ => Delta::Same,
                };
                (delta, Some(o.clone()))
            }
        };
        out.push(Change { owner: n.owner, name: n.name, delta, old: old_line, new: Some(n.clone()) });
    }
    for o in old {
        if !new.iter().any(|n| n.owner == o.owner && n.name == o.name) {
            out.push(Change { owner: o.owner, name: o.name, delta: Delta::Gone, old: Some(o.clone()), new: None });
        }
    }
    out
}

/// Bytes at a function's entry a detour may overwrite.
const HOOK_SPAN: usize = 16;

/// Hooks that subscribe through `nms-mods/src/common/hooks.rs`: one detour serves every mod listed.
pub const SHARED: &[(&str, &str)] = &[
    ("SmallerInventorySlots", "NMSPY_DoInventorySlots"),
    ("QuickTransferAll", "NMSPY_DoInventorySlots"),
    ("SmallerInventorySlots", "SORT"),
    ("QuickTransferAll", "SORT"),
    ("CraftFromStorage", "QUICK_PICK"),
    ("QuickTransferAll", "TRANSFER"),
];

fn shared(owner: &str, name: &str) -> bool {
    SHARED.iter().any(|(o, n)| *o == owner && *n == name)
}

/// Two mods hooking the same function entry. Two detours on one prologue is not allowed: each
/// pair must go through a shared hook. (Locating a hooked function is fine: patterns are searched in
/// the load-time snapshot.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlap {
    pub hook: (&'static str, &'static str),
    pub other: (&'static str, &'static str),
    pub rva: usize,
    pub shared: bool,
}

pub fn overlaps(code: &dyn Code, tables: &[ModSigs]) -> Vec<Overlap> {
    let mut found = Vec::new();
    for m in tables {
        for s in m.sigs {
            let Ok(p) = Pattern::parse(s.pattern) else { continue };
            if let Lookup::Unique(rva) = code.find_unique(&p) {
                found.push((m.owner, s.name, s.kind == SigUse::Hook, rva, s.pattern.split_whitespace().count()));
            }
        }
    }
    let mut out = Vec::new();
    for &(h_owner, h_name, is_hook, h_rva, _) in &found {
        if !is_hook {
            continue;
        }
        for &(o_owner, o_name, o_hook, o_rva, o_len) in &found {
            if o_owner == h_owner || !o_hook {
                continue;
            }
            if o_rva < h_rva + HOOK_SPAN && h_rva < o_rva + o_len {
                out.push(Overlap {
                    hook: (h_owner, h_name),
                    other: (o_owner, o_name),
                    rva: h_rva,
                    shared: shared(h_owner, h_name) && shared(o_owner, o_name),
                });
            }
        }
    }
    out
}

/// `<game>\Binaries\NMS.exe` found by walking up from `start`.
pub fn find_exe(start: &Path) -> Option<PathBuf> {
    start.ancestors().map(|d| d.join("Binaries").join("NMS.exe")).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nms_core::FileImage;
    use nms_sdk::{CodePatch, Sig};

    const PATCH: CodePatch = CodePatch { label: "p", sig: "AA BB CC DD", at: 1, old: &[0xBB], new: &[0x90] };
    const TABLE: ModSigs = ModSigs {
        owner: "T",
        sigs: &[
            Sig { name: "once", pattern: "11 22", kind: SigUse::Hook },
            Sig { name: "twice", pattern: "33 44", kind: SigUse::Locate },
            Sig { name: "never", pattern: "55 66", kind: SigUse::Locate },
        ],
        patches: &[PATCH],
    };

    #[test]
    fn reports_each_case() {
        let img = FileImage::from_code(&[0x11, 0x22, 0x33, 0x44, 0x33, 0x44, 0xAA, 0xBB, 0xCC, 0xDD]);
        let lines = check(&img, &[TABLE]);
        let oks: Vec<bool> = lines.iter().map(|l| l.ok).collect();
        assert_eq!(oks, vec![true, false, false, true]);
        assert!(lines[3].detail.contains("vanilla bytes"));
    }

    #[test]
    fn compare_reports_broke_fixed_moved_new_gone() {
        let line = |name, ok, rva| Line { owner: "T", name, ok, detail: String::new(), rva };
        let old = [line("broke", true, Some(1)), line("fixed", false, None), line("moved", true, Some(2)), line("gone", true, Some(3))];
        let new = [line("broke", false, None), line("fixed", true, Some(4)), line("moved", true, Some(5)), line("added", true, Some(6))];
        let got: Vec<(&str, Delta)> = compare(&old, &new).iter().map(|c| (c.name, c.delta)).collect();
        assert_eq!(
            got,
            vec![
                ("broke", Delta::Broke),
                ("fixed", Delta::Fixed),
                ("moved", Delta::Moved),
                ("added", Delta::New),
                ("gone", Delta::Gone),
            ]
        );
    }

    #[test]
    fn finds_two_mods_hooking_one_function() {
        const A: ModSigs = ModSigs {
            owner: "A",
            sigs: &[Sig { name: "fn", pattern: "11 22 33 44", kind: SigUse::Hook }],
            patches: &[],
        };
        const B: ModSigs = ModSigs {
            owner: "B",
            sigs: &[
                Sig { name: "same", pattern: "11 22 33", kind: SigUse::Hook },
                Sig { name: "located", pattern: "22 33 44", kind: SigUse::Locate },
                Sig { name: "far", pattern: "AA BB", kind: SigUse::Hook },
            ],
            patches: &[],
        };
        let mut code = vec![0x11, 0x22, 0x33, 0x44];
        code.extend([0u8; 32]);
        code.extend([0xAA, 0xBB]);
        let img = FileImage::from_code(&code);
        let found = overlaps(&img, &[A, B]);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].hook, found[0].other), (("A", "fn"), ("B", "same")));
        assert_eq!((found[1].hook, found[1].other), (("B", "same"), ("A", "fn")));
        assert!(!found[0].shared);
    }
}
