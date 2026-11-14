//! Every signature and code patch the mods use, so `sigcheck` can prove each one is unique in a
//! given NMS.exe before anything ships.
//!
//! One module per mod under `sigs/`. Values are copied verbatim from the Python mod (same names
//! without the `_SIG` suffix, same hex), so a reviewer can diff them line by line.

pub use nms_core::CodePatch;
use nms_core::pattern::PatternError;
use nms_core::{Code, Lookup, Pattern};

pub mod sigs;

/// What a signature is used for. Each must match exactly once in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigUse {
    /// Start of a function that gets a hook.
    Hook,
    /// A direct call whose target gets a hook. The offset points to the E8 opcode.
    HookCall(usize),
    /// Code read to find an address, a call target or a cave site.
    Locate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sig {
    pub name: &'static str,
    pub pattern: &'static str,
    pub kind: SigUse,
}

impl Sig {
    /// Resolve the location used by the mod. Call signatures resolve to the callee.
    pub fn locate(&self, code: &dyn Code) -> Result<Lookup, PatternError> {
        let hit = code.find_unique(&Pattern::parse(self.pattern)?);
        let (SigUse::HookCall(call_at), Lookup::Unique(rva)) = (self.kind, hit) else {
            return Ok(hit);
        };
        let target = rva.checked_add(call_at).and_then(|call| {
            if code.read(call, 1)?.as_slice() != [0xE8] {
                return None;
            }
            let target = code.rel32_target(call.checked_add(1)?)?;
            code.exec_ranges().iter().any(|range| range.contains(&target)).then_some(target)
        });
        Ok(target.map_or(Lookup::Missing, Lookup::Unique))
    }
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
    use nms_core::FileImage;
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

    #[test]
    fn direct_call_hook_requires_a_unique_call_to_executable_code() {
        let sig = Sig { name: "call", pattern: "90 E8 ? ? ? ?", kind: SigUse::HookCall(1) };
        let bytes = [0x90, 0xE8, 1, 0, 0, 0, 0x90, 0xC3];
        assert_eq!(sig.locate(&FileImage::from_code(&bytes)).unwrap(), Lookup::Unique(7));
        let outside = [0x90, 0xE8, 2, 0, 0, 0, 0x90, 0xC3];
        assert_eq!(sig.locate(&FileImage::from_code(&outside)).unwrap(), Lookup::Missing);
        let duplicate = [bytes, bytes].concat();
        assert!(matches!(sig.locate(&FileImage::from_code(&duplicate)).unwrap(), Lookup::Multiple(_)));
    }
}
