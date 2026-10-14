//! Every signature and code patch the mods use, so `sigcheck` can prove each one is unique in a
//! given NMS.exe before anything ships.
//!
//! One module per mod under `sigs/`. Values are copied verbatim from the Python mod (same names
//! without the `_SIG` suffix, same hex), so a reviewer can diff them line by line.

pub use nms_core::CodePatch;

pub mod sigs;

/// What a signature is used for. Each must match exactly once in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigUse {
    /// Start of a function that gets a hook.
    Hook,
    /// Code read to find an address, a call target or a cave site.
    Locate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sig {
    pub name: &'static str,
    pub pattern: &'static str,
    pub kind: SigUse,
}

/// All signatures of one mod.
#[derive(Debug, Clone, Copy)]
pub struct ModSigs {
    /// Folder name under `GAMEDATA\MODS`.
    pub owner: &'static str,
    pub sigs: &'static [Sig],
    pub patches: &'static [CodePatch],
}

/// Every mod's table, in port order.
pub fn all() -> &'static [ModSigs] {
    sigs::ALL
}

#[cfg(test)]
mod tests {
    use super::*;
    use nms_core::Pattern;
    use std::collections::HashSet;

    #[test]
    fn every_pattern_parses_and_patches_fit() {
        for m in all() {
            for s in m.sigs {
                Pattern::parse(s.pattern).unwrap_or_else(|e| panic!("{}::{}: {e}", m.owner, s.name));
            }
            for p in m.patches {
                assert_eq!(p.old.len(), p.new.len(), "{}::{}", m.owner, p.label);
                let pat = Pattern::parse(p.sig).unwrap_or_else(|e| panic!("{}::{}: {e}", m.owner, p.label));
                pat.with_bytes_at(p.at, p.new).unwrap_or_else(|e| panic!("{}::{}: {e}", m.owner, p.label));
            }
        }
    }

    #[test]
    fn names_are_unique_per_mod() {
        for m in all() {
            let mut seen = HashSet::new();
            for s in m.sigs {
                assert!(seen.insert(s.name), "{}: duplicate sig {}", m.owner, s.name);
            }
            for p in m.patches {
                assert!(seen.insert(p.label), "{}: duplicate patch {}", m.owner, p.label);
            }
        }
    }
}
