//! Live code patches that apply and undo with the same call, like `_patch_code` in the Python mods.
//!
//! The signature is written with the original bytes. Once patched, it is looked up again with the
//! new bytes in place, so the same patch can be found and taken back. Anything unexpected leaves
//! the game vanilla and says why.

use crate::image::{Code, Lookup};
use crate::pattern::Pattern;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodePatch {
    pub label: &'static str,
    pub sig: &'static str,
    pub at: usize,
    pub old: &'static [u8],
    pub new: &'static [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    TurnedOff,
    AlreadyApplied,
    AlreadyOff,
    NotFound,
    FoundTwice,
    Unexpected,
    WriteFailed,
    BadPatch,
}

impl Outcome {
    /// True when the code now is what was asked for.
    pub fn ok(self) -> bool {
        matches!(self, Outcome::Applied | Outcome::TurnedOff | Outcome::AlreadyApplied | Outcome::AlreadyOff)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchResult {
    pub outcome: Outcome,
    pub message: String,
}

impl CodePatch {
    fn patterns(&self) -> Result<(Pattern, Pattern), String> {
        if self.old.len() != self.new.len() {
            return Err("old and new are different lengths".into());
        }
        let original = Pattern::parse(self.sig).map_err(|e| e.to_string())?;
        let patched = original.with_bytes_at(self.at, self.new).map_err(|e| e.to_string())?;
        Ok((original, patched))
    }

    /// RVA of the signature start, found in its original or its patched form.
    pub fn locate(&self, code: &dyn Code) -> Result<usize, Outcome> {
        let (original, patched) = self.patterns().map_err(|_| Outcome::BadPatch)?;
        match code.find_unique(&original) {
            Lookup::Unique(rva) => Ok(rva),
            Lookup::Multiple(_) => Err(Outcome::FoundTwice),
            Lookup::Missing => match code.find_unique(&patched) {
                Lookup::Unique(rva) => Ok(rva),
                Lookup::Multiple(_) => Err(Outcome::FoundTwice),
                Lookup::Missing => Err(Outcome::NotFound),
            },
        }
    }

    /// Apply (`true`) or undo (`false`) the patch.
    pub fn set(&self, code: &mut dyn Code, apply: bool) -> PatchResult {
        let label = self.label;
        let result = |outcome: Outcome, text: &str| PatchResult { outcome, message: format!("{label} {text}") };
        let site = match self.locate(code) {
            Ok(rva) => rva + self.at,
            Err(Outcome::NotFound) => return result(Outcome::NotFound, "not changed: game code not found (game update?)"),
            Err(Outcome::FoundTwice) => return result(Outcome::FoundTwice, "not changed: game code found more than once (game update?)"),
            Err(_) => return result(Outcome::BadPatch, "not changed: the patch itself is malformed"),
        };
        let (from, to) = if apply { (self.old, self.new) } else { (self.new, self.old) };
        let current = match code.read(site, from.len()) {
            Some(bytes) => bytes,
            None => return result(Outcome::Unexpected, "not changed: unexpected game code (game update?)"),
        };
        if current == to {
            return if apply {
                result(Outcome::AlreadyApplied, "already applied")
            } else {
                result(Outcome::AlreadyOff, "already turned off")
            };
        }
        if current != from {
            return result(Outcome::Unexpected, "not changed: unexpected game code (game update?)");
        }
        if let Err(e) = code.write(site, to) {
            return result(Outcome::WriteFailed, &format!("not changed: could not write the code ({e})"));
        }
        if apply {
            result(Outcome::Applied, "applied")
        } else {
            result(Outcome::TurnedOff, "turned off")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::FileImage;

    const P: CodePatch = CodePatch {
        label: "Refiner: storage tabs",
        sig: "8B 90 88 44 01 00 83 FA 35 74 ? 8D 42 C6 83 F8 02 76 ?",
        at: 0x0B,
        old: &[0x8D, 0x42, 0xC6, 0x83, 0xF8, 0x02],
        new: &[0x8D, 0x42, 0xC5, 0x83, 0xF8, 0x01],
    };

    fn code() -> Vec<u8> {
        let mut c = vec![0xCC; 16];
        c.extend_from_slice(&[0x8B, 0x90, 0x88, 0x44, 0x01, 0x00, 0x83, 0xFA, 0x35, 0x74, 0x07]);
        c.extend_from_slice(&[0x8D, 0x42, 0xC6, 0x83, 0xF8, 0x02, 0x76, 0x10]);
        c.extend_from_slice(&[0xCC; 16]);
        c
    }

    #[test]
    fn applies_and_undoes() {
        let mut img = FileImage::from_code(&code());
        let r = P.set(&mut img, true);
        assert_eq!(r.outcome, Outcome::Applied, "{}", r.message);
        assert_eq!(img.read(16 + 0x0B, 6).unwrap(), P.new);
        assert_eq!(P.set(&mut img, true).outcome, Outcome::AlreadyApplied);
        assert_eq!(P.set(&mut img, false).outcome, Outcome::TurnedOff);
        assert_eq!(img.read(16 + 0x0B, 6).unwrap(), P.old);
        assert_eq!(P.set(&mut img, false).outcome, Outcome::AlreadyOff);
    }

    #[test]
    fn missing_code_stays_vanilla() {
        let mut img = FileImage::from_code(&[0xCC; 64]);
        let r = P.set(&mut img, true);
        assert_eq!(r.outcome, Outcome::NotFound);
        assert_eq!(r.message, "Refiner: storage tabs not changed: game code not found (game update?)");
        assert_eq!(img.read(0, 64).unwrap(), vec![0xCC; 64]);
    }

    #[test]
    fn duplicate_code_stays_vanilla() {
        let mut twice = code();
        twice.extend(code());
        let mut img = FileImage::from_code(&twice);
        assert_eq!(P.set(&mut img, true).outcome, Outcome::FoundTwice);
        assert_eq!(img.read(0, twice.len()).unwrap(), twice);
    }

    #[test]
    fn rejects_mismatched_lengths() {
        let bad = CodePatch { new: &[0x90], ..P };
        let mut img = FileImage::from_code(&code());
        assert_eq!(bad.set(&mut img, true).outcome, Outcome::BadPatch);
    }
}
