//! The game's application object, found once from code: where `cGcApplication` lives, where its
//! data block pointer is stored, the state table, and the offsets of the main members.
//!
//! Every address comes from a unique pattern and is cross-checked against at least one other
//! pattern. Any mismatch leaves the context off (accessors return `None`), never a guess.
//! Research: `docs/research/RESEARCH-lifecycle-and-context.md` (private repo).
//!
//! Naming: `app_obj` is `cGcApplication` (static, in the exe's data); `data` is
//! `cGcApplication::Data`, the block it allocates at boot (what older mod code calls "app").

pub mod sigs;

#[cfg(windows)]
pub mod dispatch;
#[cfg(windows)]
pub mod live;

use crate::image::{Code, Lookup};
use crate::pattern::Pattern;

/// Getter functions in the block after `cGcApplication::Update`, in exe order. A build that adds
/// or removes one changes the count and the context stays off until the table is checked again.
/// 21 on build 180836 (`0x1402D7A30..0x1402D7B7B`; one of them returns the data block itself).
pub const GETTER_COUNT: usize = 21;
const GETTER_STRIDE: usize = 0x10;

/// Which getter (by position in the block) returns which member.
const SLOT_SIMULATION: usize = 0;
const SLOT_REALITY: usize = 1;
const SLOT_GAME_STATE: usize = 2;
const SLOT_HUD: usize = 4;
const SLOT_INPUT_CONTROLLER: usize = 6;
const SLOT_FRONTEND: usize = 16;

/// `cTkFSM` fields (read in `cTkFSM::StateChange` and `Construct`).
pub const FSM_CURRENT_STATE: usize = 0x10;
const STATE_ENTRY: usize = 0x18;
const STATE_ID_LEN: usize = 16;
const MAX_STATES: usize = 32;
const MIN_STATES: usize = 5;
/// Members larger than this cannot be inside `cGcApplication::Data` (allocated with 0x95F210 bytes).
const MAX_MEMBER: u32 = 0x0100_0000;

/// What one getter does with the data pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Member {
    /// An object embedded in the data block at this offset.
    Embedded(u32),
    /// A pointer stored in the data block at this offset.
    Pointer(u32),
}

impl Member {
    pub fn offset(self) -> u32 {
        match self {
            Member::Embedded(o) | Member::Pointer(o) => o,
        }
    }
}

/// Offsets inside `cGcApplication::Data`, read from the game's own getters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataMembers {
    pub simulation: u32,
    pub reality: u32,
    pub game_state: u32,
    pub hud: u32,
    pub frontend: u32,
    /// Offset of the pointer to the input controller.
    pub input_controller: u32,
}

/// Everything the context needs, as RVAs and offsets. Built from any [`Code`] (the running game or
/// `Binaries\NMS.exe` on disk), so the same checks run offline in tests and `sigcheck`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `cGcApplication::Update`, the per-frame hook site.
    pub update: usize,
    pub fsm_update: usize,
    pub fsm_construct: usize,
    /// The `cGcApplication` object.
    pub app_obj: usize,
    /// The global that holds the `cGcApplication::Data` pointer.
    pub data_slot: usize,
    /// Offset of that pointer inside `cGcApplication` (`data_slot == app_obj + data_member`).
    pub data_member: u32,
    /// State table: id and offset of each state object inside `cGcApplication`.
    pub states: Vec<(String, u32)>,
    pub members: DataMembers,
}

impl Layout {
    /// Index of the state whose object sits at `app_obj + offset`.
    pub fn state_at_offset(&self, offset: usize) -> Option<usize> {
        self.states.iter().position(|(_, o)| *o as usize == offset)
    }

    pub fn state_name(&self, index: usize) -> Option<&str> {
        self.states.get(index).map(|(n, _)| n.as_str())
    }

    pub fn state_index(&self, name: &str) -> Option<usize> {
        self.states.iter().position(|(n, _)| n == name)
    }

    /// One line for `core.log`.
    pub fn describe(&self) -> String {
        let m = &self.members;
        let names: Vec<&str> = self.states.iter().map(|(n, _)| n.as_str()).collect();
        format!(
            "game context: cGcApplication RVA {:#x}, data slot RVA {:#x} (+{:#x}), Update RVA {:#x}, {} states ({}), \
             members: simulation {:#x}, reality {:#x}, game state {:#x}, HUD {:#x}, frontend {:#x}, input controller [{:#x}]",
            self.app_obj,
            self.data_slot,
            self.data_member,
            self.update,
            self.states.len(),
            names.join(" "),
            m.simulation,
            m.reality,
            m.game_state,
            m.hud,
            m.frontend,
            m.input_controller
        )
    }
}

fn unique(code: &dyn Code, name: &str, pattern: &str) -> Result<usize, String> {
    let p = Pattern::parse(pattern).map_err(|e| format!("{name}: {e}"))?;
    match code.find_unique(&p) {
        Lookup::Unique(rva) => Ok(rva),
        Lookup::Missing => Err(format!("{name} not found")),
        Lookup::Multiple(_) => Err(format!("{name} matches more than once")),
    }
}

fn bytes(code: &dyn Code, rva: usize, len: usize) -> Result<Vec<u8>, String> {
    code.read(rva, len).ok_or_else(|| format!("cannot read {len} bytes at RVA {rva:#x}"))
}

/// Decode one getter: `mov rax,[rcx+member]` or `mov rax,[rip+global]`, then `ret` or one of
/// `add rax,imm32` / `add rax,imm8` / `mov rax,[rax+imm32]` followed by `ret`.
fn getter(code: &dyn Code, rva: usize, data_member: u8, data_slot: usize) -> Option<Member> {
    let b = code.read(rva, GETTER_STRIDE)?;
    let rest = match b.as_slice() {
        [0x48, 0x8B, 0x41, m, rest @ ..] if *m == data_member => rest,
        [0x48, 0x8B, 0x05, ..] => {
            if code.rel32_target(rva + 3)? != data_slot {
                return None;
            }
            &b[7..]
        }
        _ => return None,
    };
    let member = match rest {
        [0xC3, ..] => Member::Embedded(0),
        [0x48, 0x05, a, b1, c, d, 0xC3, ..] => Member::Embedded(u32::from_le_bytes([*a, *b1, *c, *d])),
        [0x48, 0x83, 0xC0, imm, 0xC3, ..] if *imm < 0x80 => Member::Embedded(*imm as u32),
        [0x48, 0x8B, 0x80, a, b1, c, d, 0xC3, ..] => Member::Pointer(u32::from_le_bytes([*a, *b1, *c, *d])),
        _ => return None,
    };
    (member.offset() < MAX_MEMBER).then_some(member)
}

fn embedded(slots: &[Member], slot: usize, what: &str) -> Result<u32, String> {
    match slots.get(slot) {
        Some(Member::Embedded(o)) => Ok(*o),
        _ => Err(format!("getter {slot} ({what}) is not an embedded member")),
    }
}

/// Read the state table: 0x18-byte entries (16-byte id, 8-byte offset), ended by an all-zero id.
fn states(code: &dyn Code, table: usize) -> Result<Vec<(String, u32)>, String> {
    let mut out = Vec::new();
    for i in 0..=MAX_STATES {
        let e = bytes(code, table + i * STATE_ENTRY, STATE_ENTRY)?;
        let id = &e[..STATE_ID_LEN];
        if id.iter().all(|&c| c == 0) {
            break;
        }
        if i == MAX_STATES {
            return Err("state table has no end".into());
        }
        let len = id.iter().position(|&c| c == 0).unwrap_or(STATE_ID_LEN);
        let name = &id[..len];
        if name.is_empty() || !name.iter().all(|c| c.is_ascii_graphic()) {
            return Err(format!("state {i} has an unreadable id"));
        }
        let offset = u64::from_le_bytes([e[16], e[17], e[18], e[19], e[20], e[21], e[22], e[23]]);
        let offset = u32::try_from(offset).map_err(|_| format!("state {i} offset out of range"))?;
        out.push((String::from_utf8_lossy(name).into_owned(), offset));
    }
    if out.len() < MIN_STATES {
        return Err(format!("only {} states in the table", out.len()));
    }
    Ok(out)
}

/// Find and cross-check everything. `Err` names the first check that failed.
pub fn resolve(code: &dyn Code) -> Result<Layout, String> {
    let update = unique(code, "UPDATE", sigs::UPDATE)?;
    let tail = unique(code, "UPDATE_TAIL", sigs::UPDATE_TAIL)?;
    if tail != update + sigs::UPDATE_TAIL_AT {
        return Err(format!("UPDATE_TAIL is at Update+{:#x}, expected +{:#x}", tail.wrapping_sub(update), sigs::UPDATE_TAIL_AT));
    }
    let app_obj = code.rel32_target(tail + sigs::TAIL_APP_DISP).ok_or("UPDATE_TAIL: application lea")?;
    let fsm_update = unique(code, "FSM_UPDATE", sigs::FSM_UPDATE)?;
    if code.rel32_target(tail + sigs::TAIL_FSM_CALL_DISP) != Some(fsm_update) {
        return Err("Update does not call cTkFSM::Update where expected".into());
    }

    let construct_call = unique(code, "FSM_CONSTRUCT_CALL", sigs::FSM_CONSTRUCT_CALL)?;
    let fsm_construct = unique(code, "FSM_CONSTRUCT", sigs::FSM_CONSTRUCT)?;
    if code.rel32_target(construct_call + sigs::CONSTRUCT_APP_DISP) != Some(app_obj) {
        return Err("cTkFSM::Construct is not called on the same application object".into());
    }
    if code.rel32_target(construct_call + sigs::CONSTRUCT_CALL_DISP) != Some(fsm_construct) {
        return Err("FSM_CONSTRUCT_CALL does not call cTkFSM::Construct".into());
    }
    let table = code.rel32_target(construct_call + sigs::CONSTRUCT_TABLE_DISP).ok_or("state table lea")?;
    let states = states(code, table)?;

    let getters = unique(code, "APP_GETTERS", sigs::APP_GETTERS)?;
    let head = bytes(code, getters, sigs::GETTERS_SLOT_DISP + 4)?;
    let data_member = head[3];
    let data_slot = code.rel32_target(getters + sigs::GETTERS_SLOT_DISP).ok_or("data slot")?;
    if data_slot != app_obj + data_member as usize {
        return Err(format!(
            "data slot RVA {data_slot:#x} is not cGcApplication+{data_member:#x} (RVA {app_obj:#x})"
        ));
    }
    let mut slots = Vec::new();
    while let Some(m) = getter(code, getters + slots.len() * GETTER_STRIDE, data_member, data_slot) {
        slots.push(m);
        if slots.len() > GETTER_COUNT {
            break;
        }
    }
    if slots.len() != GETTER_COUNT {
        return Err(format!("{} getters after Update, expected {GETTER_COUNT}", slots.len()));
    }
    let input_controller = match slots[SLOT_INPUT_CONTROLLER] {
        Member::Pointer(o) => o,
        Member::Embedded(_) => return Err("getter 6 (input controller) is not a pointer member".into()),
    };
    let members = DataMembers {
        simulation: embedded(&slots, SLOT_SIMULATION, "simulation")?,
        reality: embedded(&slots, SLOT_REALITY, "reality")?,
        game_state: embedded(&slots, SLOT_GAME_STATE, "game state")?,
        hud: embedded(&slots, SLOT_HUD, "HUD")?,
        frontend: embedded(&slots, SLOT_FRONTEND, "frontend")?,
        input_controller,
    };
    Ok(Layout { update, fsm_update, fsm_construct, app_obj, data_slot, data_member: data_member as u32, states, members })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::FileImage;
    use std::path::PathBuf;

    fn game_exe() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("NMS_EXE") {
            return Some(PathBuf::from(p));
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|d| d.join("Binaries").join("NMS.exe"))
            .find(|p| p.is_file())
    }

    /// Runs against the real `Binaries\NMS.exe` (or `NMS_EXE`). Ignored by default, like the parity tests.
    #[test]
    #[ignore]
    fn resolves_on_the_installed_exe() {
        let exe = game_exe().expect("no NMS.exe found; set NMS_EXE");
        let img = FileImage::load(&exe).unwrap();
        let layout = resolve(&img).unwrap_or_else(|e| panic!("{e}"));
        println!("{}", layout.describe());
        assert_eq!(layout.data_member, 0x38);
        assert_eq!(layout.state_name(0), Some("APPBOOT"));
        assert!(layout.state_index("MODESELECTOR").is_some());
        assert!(layout.state_index("APPVIEW").is_some());
        assert_eq!(layout.states.len(), 13, "state count changed: check the lifecycle docs");
    }

    #[test]
    fn decodes_each_getter_shape() {
        // add imm32, add imm8, deref imm32, rip global + add imm32.
        let mut code = vec![0xCC; 0x60];
        code[0x00..0x0B].copy_from_slice(&[0x48, 0x8B, 0x41, 0x38, 0x48, 0x05, 0x90, 0xCF, 0x4C, 0x00, 0xC3]);
        code[0x10..0x19].copy_from_slice(&[0x48, 0x8B, 0x41, 0x38, 0x48, 0x83, 0xC0, 0x60, 0xC3]);
        code[0x20..0x2C].copy_from_slice(&[0x48, 0x8B, 0x41, 0x38, 0x48, 0x8B, 0x80, 0x78, 0x3B, 0x93, 0x00, 0xC3]);
        // mov rax,[rip+disp] at 0x30, disp to 0x50: 0x50 - (0x33 + 4) = 0x19.
        code[0x30..0x3E].copy_from_slice(&[0x48, 0x8B, 0x05, 0x19, 0, 0, 0, 0x48, 0x05, 0x70, 0x0E, 0, 0, 0xC3]);
        let img = FileImage::from_code(&code);
        assert_eq!(getter(&img, 0x00, 0x38, 0x50), Some(Member::Embedded(0x4CCF90)));
        assert_eq!(getter(&img, 0x10, 0x38, 0x50), Some(Member::Embedded(0x60)));
        assert_eq!(getter(&img, 0x20, 0x38, 0x50), Some(Member::Pointer(0x933B78)));
        assert_eq!(getter(&img, 0x30, 0x38, 0x50), Some(Member::Embedded(0xE70)));
        // Wrong member byte or wrong global: refused.
        assert_eq!(getter(&img, 0x00, 0x40, 0x50), None);
        assert_eq!(getter(&img, 0x30, 0x38, 0x58), None);
        // Padding is not a getter.
        assert_eq!(getter(&img, 0x40, 0x38, 0x50), None);
    }

    #[test]
    fn reads_a_state_table_and_stops_at_the_zero_id() {
        let mut data = Vec::new();
        for (name, off) in [("APPBOOT", 0xE0u64), ("B", 0x10), ("C", 0x20), ("D", 0x30), ("MODESELECTOR", 0x59D0)] {
            let mut id = [0u8; 16];
            id[..name.len()].copy_from_slice(name.as_bytes());
            data.extend_from_slice(&id);
            data.extend_from_slice(&off.to_le_bytes());
        }
        data.extend_from_slice(&[0u8; 24]);
        let img = FileImage::from_code(&data);
        let s = states(&img, 0).unwrap();
        assert_eq!(s.len(), 5);
        assert_eq!(s[4], ("MODESELECTOR".to_string(), 0x59D0));
    }

    #[test]
    fn refuses_a_short_or_garbled_state_table() {
        let mut data = vec![0u8; 24 * 3];
        data[..2].copy_from_slice(b"AB");
        assert!(states(&FileImage::from_code(&data), 0).is_err(), "one state is too few");
        let mut bad = vec![0u8; 24 * 2];
        bad[0] = 0x01;
        assert!(states(&FileImage::from_code(&bad), 0).is_err(), "control bytes are not an id");
    }
}
