//! Filter-test cave, icon/skip stubs, tooltip table, and extra-button hide image.

use std::collections::HashMap;

use nms_core::mem;

use super::layout::{
    extra_filter_ids, BUTTON_TIPS, CUSTOM_INDEX, EXTRA_FILTER_NAMES, FOODS_INDEX, IS_HIDDEN, PAGE_ELEMENT,
    PRODUCT_CATEGORY, SKIP_BYTES, TIP_SIZE, VANILLA_FILTERS,
};

/// Distance from a button's id to its `on` byte in the hide cave data.
pub const HIDE_ON_FROM_IDS: usize = 16;
use crate::common::custom_filter::{
    COOKING_INGREDIENT, COOKING_VALUE, CONSUMABLE, CUSTOM_ID, ELEMENT_KIND, ELEMENT_X, ELEMENT_Y, ENTRY_SIZE, FOODS_ID,
    MAX_MARKS,
};
/// Bytes `install_custom_patch` allocates for the Custom cave (code, data and table).
pub const CUSTOM_CAVE_SIZE: usize = 0x2000;

pub fn rel8(source: usize, target: usize) -> Result<u8, String> {
    let rel = target as i64 - (source as i64 + 1);
    if !(-128..=127).contains(&rel) {
        return Err(format!("short jump {source:#x} -> {target:#x} does not fit"));
    }
    Ok(rel as u8)
}

pub fn rel32_bytes(source: usize, target: usize) -> Result<[u8; 4], String> {
    let rel = mem::rel32(source.wrapping_add(4), target)
        .ok_or_else(|| format!("jump {source:#x} -> {target:#x} does not fit"))?;
    Ok(rel.to_le_bytes())
}

/// Tiny assembler for the hand-written caves: code bytes plus short, near and RIP-relative fixups
/// resolved once every label is known.
struct Asm {
    base: usize,
    code: Vec<u8>,
    labels: HashMap<&'static str, usize>,
    /// (operand offset, label, kind): kind 1 = rel8, 4 = rel32 from the operand end, or a RIP
    /// operand followed by `tail` immediate bytes.
    fixes: Vec<(usize, &'static str, u8, usize)>,
    far: Vec<(usize, usize)>,
}

impl Asm {
    fn new(base: usize) -> Self {
        Self {
            base,
            code: Vec::new(),
            labels: HashMap::new(),
            fixes: Vec::new(),
            far: Vec::new(),
        }
    }

    fn emit(&mut self, raw: &[u8]) -> usize {
        let at = self.code.len();
        self.code.extend_from_slice(raw);
        at
    }

    fn label(&mut self, name: &'static str) {
        self.labels.insert(name, self.code.len());
    }

    /// Short jump `op rel8` to a label.
    fn short(&mut self, op: u8, target: &'static str) {
        self.emit(&[op, 0]);
        self.fixes.push((self.code.len() - 1, target, 1, 0));
    }

    /// Instruction ending in a rel32 to an address outside the cave.
    fn far(&mut self, prefix: &[u8], target: usize) {
        self.emit(prefix);
        self.far.push((self.code.len(), target));
        self.emit(&[0, 0, 0, 0]);
    }

    /// Instruction with a RIP-relative operand to a label, followed by `tail` immediate bytes.
    fn rip(&mut self, prefix: &[u8], target: &'static str, tail: &[u8]) {
        self.emit(prefix);
        let at = self.code.len();
        self.emit(&[0, 0, 0, 0]);
        self.emit(tail);
        self.fixes.push((at, target, 4, tail.len()));
    }

    fn finish(mut self) -> Result<(Vec<u8>, HashMap<&'static str, usize>), String> {
        for (at, target) in self.far.clone() {
            let bytes = rel32_bytes(self.base.wrapping_add(at), target)?;
            self.code[at..at + 4].copy_from_slice(&bytes);
        }
        for (at, name, kind, tail) in self.fixes.clone() {
            let target = *self.labels.get(name).ok_or_else(|| format!("cave label {name} missing"))?;
            if kind == 1 {
                let rel = target as i64 - (at as i64 + 1);
                if !(-128..=127).contains(&rel) {
                    return Err(format!("short jump to {name} does not fit"));
                }
                self.code[at] = rel as u8;
            } else {
                let end = at + 4 + tail;
                let rel = i32::try_from(target as i64 - end as i64).map_err(|_| "cave too large".to_string())?;
                self.code[at..at + 4].copy_from_slice(&rel.to_le_bytes());
            }
        }
        Ok((self.code, self.labels))
    }
}

/// Product dim cave at `FILTER + DIM_AT`. Ids below 5 run the vanilla `filter == 4` test; Foods (when
/// its `foods_on` byte is set) lights `CookingValue > 0` or a Consumable cooking ingredient
/// (`custom_filter::foods_lit`); any other id lights `category == id - 5`.
/// Returns the image and the offset of the `foods_on` byte.
pub fn cave_bytes(cave: usize, join: usize, deploy: usize) -> Result<(Vec<u8>, usize), String> {
    let mut a = Asm::new(cave);
    a.emit(&[0x51]); // push rcx
    a.emit(&[0x41, 0x83, 0xFE, VANILLA_FILTERS]); // cmp r14d, 5
    a.short(0x7C, "vanilla"); // jl
    a.emit(&[0x48, 0x85, 0xC0]); // test rax, rax
    a.short(0x74, "lit"); // jz: no product data, lit (as before)
    a.emit(&[0x41, 0x83, 0xFE, FOODS_ID as u8]); // cmp r14d, FOODS_ID
    a.short(0x75, "category");
    a.rip(&[0x80, 0x3D], "foods_on", &[0x00]); // cmp byte [rip+foods_on], 0
    a.short(0x74, "category");
    let mut cooking = vec![0x83, 0xB8];
    cooking.extend_from_slice(&(COOKING_VALUE as u32).to_le_bytes());
    cooking.push(0x00);
    a.emit(&cooking); // cmp dword [rax+0x1A0], 0
    a.short(0x7F, "lit"); // jg
    let mut ingredient = vec![0x80, 0xB8];
    ingredient.extend_from_slice(&(COOKING_INGREDIENT as u32).to_le_bytes());
    ingredient.push(0x00);
    a.emit(&ingredient); // cmp byte [rax+0x2F2], 0
    a.short(0x74, "dim"); // je: not a cooking ingredient
    let mut consumable = vec![0x83, 0xB8];
    consumable.extend_from_slice(&PRODUCT_CATEGORY.to_le_bytes());
    consumable.push(CONSUMABLE as u8);
    a.emit(&consumable); // cmp dword [rax+0x1E8], Consumable
    a.short(0x74, "lit"); // je: bait and creature pellets
    a.short(0xEB, "dim");
    a.label("category");
    a.emit(&[0x44, 0x89, 0xF1]); // mov ecx, r14d
    a.emit(&[0x83, 0xE9, VANILLA_FILTERS]); // sub ecx, 5
    let mut cmp = vec![0x39, 0x88];
    cmp.extend_from_slice(&PRODUCT_CATEGORY.to_le_bytes());
    a.emit(&cmp); // cmp [rax+0x1E8], ecx
    a.short(0x74, "lit");
    a.label("dim");
    a.emit(&[0xB3, 0x01]); // mov bl, 1
    a.short(0xEB, "done");
    a.label("lit");
    a.emit(&[0x32, 0xDB]); // xor bl, bl
    a.short(0xEB, "done");
    a.label("vanilla");
    a.emit(&[0x59]); // pop rcx
    a.emit(&[0x41, 0x83, 0xFE, 0x04]); // cmp r14d, 4
    a.far(&[0x0F, 0x85], join); // jne join
    a.far(&[0xE9], deploy); // jmp back
    a.label("done");
    a.emit(&[0x59]); // pop rcx
    a.far(&[0xE9], join);
    while a.code.len() % 16 != 0 {
        a.emit(&[0xCC]);
    }
    a.label("foods_on");
    a.emit(&[0x00]);
    let (code, labels) = a.finish()?;
    let foods = *labels.get("foods_on").ok_or_else(|| "cave label foods_on missing".to_string())?;
    Ok((code, foods))
}

/// Offsets of the Custom cave's live data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomLayout {
    pub on: usize,
    pub count: usize,
    /// The inventory store `DoInventorySlots` is drawing, written by the SmallerInventorySlots hook
    /// before each call and cleared after it (per-slot marks compare it).
    pub store: usize,
    pub table: usize,
}

/// Custom cave at `FILTER + CUSTOM_AT`. With the page filter on Custom (and `custom_on` set) it lights
/// every element that matches a table entry, of any kind, and dims the rest; otherwise it runs
/// the stolen `test eax, eax; jne kind_test; cmp r14d, 1` and returns to the vanilla `jbe`.
/// An entry matches on (id, type); an entry with a store (per-slot mark) also needs that store to
/// be the one being drawn and the element's slot x, y (`+0x10`, `+0x14`) to be the entry's.
/// `r12` is the inventory element (the vanilla code reads its type at `+0x24` and passes it to the
/// product lookup), `bl` is the dim result, `join` is where every branch of the vanilla test meets.
/// `rax` is free on the Custom path: every branch reloads it after `join`.
pub fn custom_cave_bytes(cave: usize, join: usize, back: usize, kind_test: usize) -> Result<(Vec<u8>, CustomLayout), String> {
    let mut a = Asm::new(cave);
    a.emit(&[0x41, 0x83, 0xFE, CUSTOM_ID as u8]); // cmp r14d, CUSTOM_ID
    a.short(0x75, "orig");
    a.rip(&[0x80, 0x3D], "on", &[0x00]); // cmp byte [rip+on], 0
    a.short(0x74, "orig");
    a.emit(&[0x51, 0x52]); // push rcx; push rdx
    a.rip(&[0x8B, 0x0D], "count", &[]); // mov ecx, [rip+count]
    a.rip(&[0x48, 0x8D, 0x15], "table", &[]); // lea rdx, [rip+table]
    a.label("loop");
    a.emit(&[0x85, 0xC9]); // test ecx, ecx
    a.short(0x74, "dim");
    a.emit(&[0x49, 0x8B, 0x04, 0x24]); // mov rax, [r12]
    a.emit(&[0x48, 0x3B, 0x02]); // cmp rax, [rdx]
    a.short(0x75, "next");
    a.emit(&[0x49, 0x8B, 0x44, 0x24, 0x08]); // mov rax, [r12+8]
    a.emit(&[0x48, 0x3B, 0x42, 0x08]); // cmp rax, [rdx+8]
    a.short(0x75, "next");
    a.emit(&[0x41, 0x8B, 0x44, 0x24, ELEMENT_KIND as u8]); // mov eax, [r12+0x24]
    a.emit(&[0x3B, 0x42, 0x10]); // cmp eax, [rdx+0x10]
    a.short(0x75, "next");
    a.emit(&[0x48, 0x8B, 0x42, 0x20]); // mov rax, [rdx+0x20]  (entry store)
    a.emit(&[0x48, 0x85, 0xC0]); // test rax, rax
    a.short(0x74, "lit"); // per-item mark
    a.rip(&[0x48, 0x3B, 0x05], "store", &[]); // cmp rax, [rip+store]
    a.short(0x75, "next");
    a.emit(&[0x41, 0x8B, 0x44, 0x24, ELEMENT_X as u8]); // mov eax, [r12+0x10]
    a.emit(&[0x3B, 0x42, 0x14]); // cmp eax, [rdx+0x14]
    a.short(0x75, "next");
    a.emit(&[0x41, 0x8B, 0x44, 0x24, ELEMENT_Y as u8]); // mov eax, [r12+0x14]
    a.emit(&[0x3B, 0x42, 0x18]); // cmp eax, [rdx+0x18]
    a.short(0x74, "lit");
    a.label("next");
    a.emit(&[0x48, 0x83, 0xC2, ENTRY_SIZE as u8]); // add rdx, 0x30
    a.emit(&[0xFF, 0xC9]); // dec ecx
    a.short(0xEB, "loop");
    a.label("dim");
    a.emit(&[0xB3, 0x01]); // mov bl, 1
    a.short(0xEB, "out");
    a.label("lit");
    a.emit(&[0x32, 0xDB]); // xor bl, bl
    a.label("out");
    a.emit(&[0x5A, 0x59]); // pop rdx; pop rcx
    a.far(&[0xE9], join);
    a.label("orig");
    a.emit(&[0x85, 0xC0]); // test eax, eax
    a.far(&[0x0F, 0x85], kind_test); // jne kind_test
    a.emit(&[0x41, 0x83, 0xFE, 0x01]); // cmp r14d, 1
    a.far(&[0xE9], back); // jmp back to the jbe, flags kept
    while a.code.len() % 16 != 0 {
        a.emit(&[0xCC]);
    }
    a.label("on");
    a.emit(&[0, 0, 0, 0]);
    a.label("count");
    a.emit(&[0, 0, 0, 0]);
    a.label("store");
    a.emit(&[0; 8]);
    while a.code.len() % 16 != 0 {
        a.emit(&[0]);
    }
    a.label("table");
    a.emit(&vec![0u8; MAX_MARKS * ENTRY_SIZE]);
    let (code, labels) = a.finish()?;
    let get = |name: &str| labels.get(name).copied().ok_or_else(|| format!("cave label {name} missing"));
    Ok((
        code,
        CustomLayout {
            on: get("on")?,
            count: get("count")?,
            store: get("store")?,
            table: get("table")?,
        },
    ))
}

pub fn icon_stub(stub: usize, original: &[u8], back: usize) -> Result<Vec<u8>, String> {
    let mut code = Vec::new();
    code.extend_from_slice(&[0x41, 0x83, 0xFD, VANILLA_FILTERS]);
    code.extend_from_slice(&[0x7C, 0x07]);
    code.extend_from_slice(&[0x31, 0xC9]);
    code.push(0xE9);
    let jmp1 = rel32_bytes(stub.wrapping_add(code.len()), back)?;
    code.extend_from_slice(&jmp1);
    code.extend_from_slice(original);
    code.push(0xE9);
    let jmp2 = rel32_bytes(stub.wrapping_add(code.len()), back)?;
    code.extend_from_slice(&jmp2);
    Ok(code)
}

pub fn skip_stub(stub: usize, back: usize, miss: usize) -> Result<Vec<u8>, String> {
    let mut code = Vec::new();
    code.extend_from_slice(&[0x41, 0x83, 0xFD, VANILLA_FILTERS]);
    let jl_at = code.len();
    code.extend_from_slice(&[0x7C, 0x00]);
    code.push(0xE9);
    let skip_jmp = rel32_bytes(stub.wrapping_add(code.len()), back)?;
    code.extend_from_slice(&skip_jmp);
    let vanilla = code.len();
    let jl = rel8(stub.wrapping_add(jl_at).wrapping_add(1), stub.wrapping_add(vanilla))?;
    if let Some(slot) = code.get_mut(jl_at.wrapping_add(1)) {
        *slot = jl;
    }
    if let Some(head) = SKIP_BYTES.get(..8) {
        code.extend_from_slice(head);
    }
    let je1 = code.len();
    code.extend_from_slice(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00]);
    if let Some(mid) = SKIP_BYTES.get(14..22) {
        code.extend_from_slice(mid);
    }
    let je2 = code.len();
    code.extend_from_slice(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00]);
    code.push(0xE9);
    let back_jmp = rel32_bytes(stub.wrapping_add(code.len()), back)?;
    code.extend_from_slice(&back_jmp);
    let put = |code: &mut Vec<u8>, at: usize, target: usize| -> Result<(), String> {
        let bytes = rel32_bytes(stub.wrapping_add(at).wrapping_add(2), target)?;
        if at.wrapping_add(6) > code.len() {
            return Err("skip stub je site missing".into());
        }
        code[at + 2..at + 6].copy_from_slice(&bytes);
        Ok(())
    };
    put(&mut code, je1, miss)?;
    put(&mut code, je2, miss)?;
    Ok(code)
}

pub fn tip_table(vanilla: &[u8]) -> Result<Vec<u8>, String> {
    let mut table = vanilla.to_vec();
    for key in BUTTON_TIPS {
        let raw = key.as_bytes();
        if raw.len() >= TIP_SIZE {
            return Err(format!("tooltip key {key} is too long"));
        }
        table.extend_from_slice(raw);
        table.resize(table.len() + (TIP_SIZE - raw.len()), 0);
    }
    Ok(table)
}

pub fn hide_image(
    cave: usize,
    back: usize,
    find: usize,
    name_hash: usize,
    child: usize,
    cursor: usize,
) -> Result<(Vec<u8>, HashMap<&'static str, usize>), String> {
    let mut code = Vec::new();
    let mut fix: Vec<(usize, usize, &'static str)> = Vec::new();
    let emit = |raw: &[u8], code: &mut Vec<u8>| {
        let at = code.len();
        code.extend_from_slice(raw);
        at
    };
    let lea_label = |reg: u8, label: &'static str, code: &mut Vec<u8>, fix: &mut Vec<(usize, usize, &'static str)>| {
        let at = {
            let start = code.len();
            code.extend_from_slice(&[0x48, 0x8D, reg]);
            start
        };
        fix.push((at + 3, at + 7, label));
        code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    };
    let rip_disp = |prefix: &[u8], label: &'static str, code: &mut Vec<u8>, fix: &mut Vec<(usize, usize, &'static str)>| {
        let at = {
            let start = code.len();
            code.extend_from_slice(prefix);
            start
        };
        fix.push((at + prefix.len(), at + prefix.len() + 4, label));
        code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        at
    };

    emit(&[0x57, 0x53, 0x41, 0x57], &mut code);
    emit(&[0x48, 0x83, 0xEC, 0x38], &mut code);
    rip_disp(&[0xFF, 0x05], "ticks", &mut code, &mut fix);
    let clear_hover = emit(&[0xC6, 0x05], &mut code);
    fix.push((clear_hover + 2, clear_hover + 7, "hover"));
    emit(&[0x00, 0x00, 0x00, 0x00, 0xFF], &mut code);
    let mut page_load = vec![0x49, 0x8B, 0x8E];
    page_load.extend_from_slice(&(PAGE_ELEMENT as u32).to_le_bytes());
    emit(&page_load, &mut code);
    emit(&[0x48, 0x85, 0xC9], &mut code);
    let jz_page = emit(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00], &mut code);
    lea_label(0x15, "page", &mut code, &mut fix);
    emit(
        &[0x0F, 0x10, 0x02, 0x66, 0x0F, 0x7F, 0x45, 0xB0, 0x48, 0x8D, 0x55, 0xB0, 0x45, 0x33, 0xC0],
        &mut code,
    );
    let call1 = emit(&[0xE8, 0x00, 0x00, 0x00, 0x00], &mut code);
    emit(&[0x48, 0x85, 0xC0], &mut code);
    let jz_pc = emit(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00], &mut code);
    emit(&[0x48, 0x8B, 0xC8], &mut code);
    lea_label(0x15, "filters", &mut code, &mut fix);
    emit(
        &[0x0F, 0x10, 0x02, 0x66, 0x0F, 0x7F, 0x45, 0xB0, 0x48, 0x8D, 0x55, 0xB0, 0x45, 0x33, 0xC0],
        &mut code,
    );
    let call2 = emit(&[0xE8, 0x00, 0x00, 0x00, 0x00], &mut code);
    emit(&[0x48, 0x85, 0xC0], &mut code);
    let jz_filters = emit(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00], &mut code);
    emit(&[0x48, 0x8B, 0xF8], &mut code);
    lea_label(0x1D, "names", &mut code, &mut fix);
    lea_label(0x05, "ids", &mut code, &mut fix);
    emit(&[0x48, 0x89, 0x44, 0x24, 0x30], &mut code);
    let mut count_imm = vec![0x41, 0xBF];
    count_imm.extend_from_slice(&(EXTRA_FILTER_NAMES.len() as u32).to_le_bytes());
    emit(&count_imm, &mut code);
    let loop_at = code.len();
    emit(&[0x48, 0x8B, 0xD3, 0x48, 0x8D, 0x4D, 0xB0, 0x45, 0x33, 0xC0], &mut code);
    let call_hash = emit(&[0xE8, 0x00, 0x00, 0x00, 0x00], &mut code);
    emit(&[0x48, 0x8B, 0xD0, 0x48, 0x8B, 0xCF, 0x45, 0x33, 0xC0], &mut code);
    let call_child = emit(&[0xE8, 0x00, 0x00, 0x00, 0x00], &mut code);
    emit(&[0x48, 0x85, 0xC0], &mut code);
    let jz_child = emit(&[0x74, 0x00], &mut code);
    emit(&[0x48, 0x8B, 0x48, 0x48], &mut code);
    // Per-button switch: the `on` byte sits 16 bytes after the button's id ([rsp+0x30] walks ids).
    emit(&[0x48, 0x8B, 0x54, 0x24, 0x30, 0x80, 0x7A, HIDE_ON_FROM_IDS as u8, 0x00], &mut code);
    let je_off = emit(&[0x74, 0x00], &mut code);
    emit(&[0x40, 0x84, 0xF6], &mut code);
    let je_hide = emit(&[0x74, 0x00], &mut code);
    emit(&[0xC6, 0x41, IS_HIDDEN as u8, 0x00], &mut code);
    emit(&[0x48, 0x8B, 0xC8], &mut code);
    let call_cursor = emit(&[0xE8, 0x00, 0x00, 0x00, 0x00], &mut code);
    emit(&[0x84, 0xC0], &mut code);
    let jz_cursor = emit(&[0x74, 0x00], &mut code);
    emit(&[0x48, 0x8B, 0x54, 0x24, 0x30, 0x8A, 0x02], &mut code);
    let _mov_hover = rip_disp(&[0x88, 0x05], "hover", &mut code, &mut fix);
    let jmp_next = emit(&[0xEB, 0x00], &mut code);
    let hide = emit(&[0xC6, 0x41, IS_HIDDEN as u8, 0x01], &mut code);
    let nxt = emit(&[0x48, 0x83, 0xC3, 0x10, 0x48, 0xFF, 0x44, 0x24, 0x30, 0x41, 0xFF, 0xCF, 0x75, 0x00], &mut code);
    let jne_rel = nxt + 13;
    let done = code.len();
    emit(&[0x48, 0x83, 0xC4, 0x38, 0x41, 0x5F, 0x5B, 0x5F], &mut code);
    emit(&[0x0F, 0x57, 0xC0], &mut code);
    emit(&[0x0F, 0x11, 0x44, 0x24, 0x70], &mut code);
    let jmp_back = emit(&[0xE9, 0x00, 0x00, 0x00, 0x00], &mut code);

    let set8 = |code: &mut Vec<u8>, at: usize, target: usize| -> Result<(), String> {
        let rel = target as i64 - (at as i64 + 1);
        if !(-128..=127).contains(&rel) {
            return Err(format!("short jump in the extra filter hide does not fit ({rel})"));
        }
        if let Some(slot) = code.get_mut(at) {
            *slot = rel as u8;
            Ok(())
        } else {
            Err("hide short jump site missing".into())
        }
    };
    set8(&mut code, jz_child + 1, nxt)?;
    set8(&mut code, je_off + 1, hide)?;
    set8(&mut code, je_hide + 1, hide)?;
    set8(&mut code, jz_cursor + 1, nxt)?;
    set8(&mut code, jmp_next + 1, nxt)?;
    set8(&mut code, jne_rel, loop_at)?;

    let put32 = |code: &mut Vec<u8>, at: usize, target: usize| -> Result<(), String> {
        let bytes = rel32_bytes(cave.wrapping_add(at), target)?;
        if at.wrapping_add(4) > code.len() {
            return Err("hide rel32 site missing".into());
        }
        code[at..at + 4].copy_from_slice(&bytes);
        Ok(())
    };
    put32(&mut code, jz_page + 2, cave.wrapping_add(done))?;
    put32(&mut code, jz_pc + 2, cave.wrapping_add(done))?;
    put32(&mut code, jz_filters + 2, cave.wrapping_add(done))?;
    put32(&mut code, jmp_back + 1, back)?;
    put32(&mut code, call1 + 1, find)?;
    put32(&mut code, call2 + 1, find)?;
    put32(&mut code, call_hash + 1, name_hash)?;
    put32(&mut code, call_child + 1, child)?;
    put32(&mut code, call_cursor + 1, cursor)?;

    let body = code;
    let pad = (16 - (body.len() % 16)) % 16;
    let mut data = vec![0u8; pad];
    let mut labels: HashMap<&'static str, usize> = HashMap::new();
    let add = |label: &'static str, raw: &[u8], data: &mut Vec<u8>, labels: &mut HashMap<&'static str, usize>| {
        labels.insert(label, body.len() + data.len());
        data.extend_from_slice(raw);
    };
    let pad16 = |s: &[u8]| {
        let mut buf = s.to_vec();
        buf.resize(16, 0);
        buf
    };
    add("page", &pad16(b"PAGECONTENT"), &mut data, &mut labels);
    add("filters", &pad16(b"FILTERS"), &mut data, &mut labels);
    let mut names = Vec::new();
    for name in EXTRA_FILTER_NAMES {
        names.extend_from_slice(&pad16(name));
    }
    if names.len() != 16 * EXTRA_FILTER_NAMES.len() {
        return Err("an extra filter name does not fit the 16-byte lookup".into());
    }
    add("names", &names, &mut data, &mut labels);
    let mut ids = extra_filter_ids().to_vec();
    ids.resize(HIDE_ON_FROM_IDS, 0);
    add("ids", &ids, &mut data, &mut labels);
    // Category buttons start shown; Foods and Custom start hidden until their switch is on.
    let on: Vec<u8> = (0..EXTRA_FILTER_NAMES.len())
        .map(|i| u8::from(i != FOODS_INDEX && i != CUSTOM_INDEX))
        .collect();
    add("on", &on, &mut data, &mut labels);
    add("ticks", &[0, 0, 0, 0], &mut data, &mut labels);
    add("hover", &[0xFF], &mut data, &mut labels);

    let mut image = body;
    image.extend_from_slice(&data);
    for (at, end, label) in fix {
        let off = match labels.get(label) {
            Some(o) => *o,
            None => return Err(format!("hide label {label} missing")),
        };
        let bytes = rel32_bytes(cave.wrapping_add(end).wrapping_sub(4), cave.wrapping_add(off))?;
        if at.wrapping_add(4) > image.len() {
            return Err("hide rip site missing".into());
        }
        image[at..at + 4].copy_from_slice(&bytes);
    }
    let vas: HashMap<&'static str, usize> = labels
        .into_iter()
        .map(|(k, off)| (k, cave.wrapping_add(off)))
        .collect();
    Ok((image, vas))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_cave_head_and_length() {
        let cave = 0x0000_7FF6_1000_0000usize;
        let join = cave.wrapping_add(0x200);
        let deploy = cave.wrapping_add(0x300);
        let (body, foods) = cave_bytes(cave, join, deploy).unwrap();
        assert_eq!(&body[..5], super::super::layout::CAVE_HEAD);
        assert_eq!(body[0], 0x51);
        assert_eq!(foods % 16, 0);
        assert_eq!(foods, body.len() - 1);
        // The Foods compare reads CookingValue at +0x1A0 and jumps on "greater" (signed).
        let cooking = [0x83, 0xB8, 0xA0, 0x01, 0x00, 0x00, 0x00, 0x7F];
        assert!(body.windows(cooking.len()).any(|w| w == cooking));
        // Second arm: CookingIngredient byte at +0x2F2, then category == Consumable (1).
        let ingredient = [0x80, 0xB8, 0xF2, 0x02, 0x00, 0x00, 0x00, 0x74];
        assert!(body.windows(ingredient.len()).any(|w| w == ingredient));
        let consumable = [0x83, 0xB8, 0xE8, 0x01, 0x00, 0x00, 0x01, 0x74];
        assert!(body.windows(consumable.len()).any(|w| w == consumable));
        let mut category = vec![0x39, 0x88];
        category.extend_from_slice(&PRODUCT_CATEGORY.to_le_bytes());
        assert!(body.windows(category.len()).any(|w| w == category.as_slice()));
        // The RIP operand of `cmp byte [rip+foods_on], 0` points at the data byte.
        let at = body.windows(2).position(|w| w == [0x80, 0x3D]).unwrap();
        let disp = i32::from_le_bytes(body[at + 2..at + 6].try_into().unwrap());
        assert_eq!((at as i64 + 7 + disp as i64) as usize, foods);
        // Same input gives the same image (the reload check compares whole images).
        assert_eq!(cave_bytes(cave, join, deploy).unwrap().0, body);
    }

    #[test]
    fn custom_cave_layout() {
        let cave = 0x0000_7FF6_1000_0000usize;
        let (body, layout) = custom_cave_bytes(cave, cave + 0x2000, cave + 0x2100, cave + 0x2200).unwrap();
        assert_eq!(&body[..4], super::super::layout::CUSTOM_HEAD);
        assert_eq!(layout.on % 16, 0);
        assert_eq!(layout.count, layout.on + 4);
        assert_eq!(layout.store, layout.on + 8);
        assert_eq!(layout.table % 16, 0);
        assert_eq!(body.len(), layout.table + MAX_MARKS * ENTRY_SIZE);
        assert!(body.len() <= CUSTOM_CAVE_SIZE);
        // Per-slot test: `cmp rax, [rip+store]` reads the drawn store, then slot x and y.
        let at = body.windows(3).position(|w| w == [0x48, 0x3B, 0x05]).unwrap();
        let disp = i32::from_le_bytes(body[at + 3..at + 7].try_into().unwrap());
        assert_eq!((at as i64 + 7 + disp as i64) as usize, layout.store);
        for want in [[0x41, 0x8B, 0x44, 0x24, 0x10, 0x3B, 0x42, 0x14], [0x41, 0x8B, 0x44, 0x24, 0x14, 0x3B, 0x42, 0x18]] {
            assert!(body.windows(want.len()).any(|w| w == want));
        }
        assert!(body.windows(4).any(|w| w == [0x48, 0x83, 0xC2, 0x30]));
        let at = body.windows(2).position(|w| w == [0x80, 0x3D]).unwrap();
        let disp = i32::from_le_bytes(body[at + 2..at + 6].try_into().unwrap());
        assert_eq!((at as i64 + 7 + disp as i64) as usize, layout.on);
        let at = body.windows(2).position(|w| w == [0x8B, 0x0D]).unwrap();
        let disp = i32::from_le_bytes(body[at + 2..at + 6].try_into().unwrap());
        assert_eq!((at as i64 + 6 + disp as i64) as usize, layout.count);
        let at = body.windows(3).position(|w| w == [0x48, 0x8D, 0x15]).unwrap();
        let disp = i32::from_le_bytes(body[at + 3..at + 7].try_into().unwrap());
        assert_eq!((at as i64 + 7 + disp as i64) as usize, layout.table);
        // The stolen bytes are re-run on the vanilla path, then a jmp back.
        let orig = [0x85, 0xC0, 0x0F, 0x85];
        assert!(body.windows(orig.len()).any(|w| w == orig));
    }

    #[test]
    #[ignore]
    fn dump_caves() {
        let cave = 0x0000_0001_4100_0000usize;
        let (dim, _) = cave_bytes(cave, 0x1406B716F, 0x1406B714D).unwrap();
        let (custom, layout) = custom_cave_bytes(cave, 0x1406B716F, 0x1406B7086, 0x1406B7093).unwrap();
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        println!("DIM {}", hex(&dim));
        println!("CUSTOM {}", hex(&custom[..layout.on]));
    }

    #[test]
    fn icon_skip_and_tips() {
        let stub = 0x0000_7FF6_2000_0000usize;
        let back = stub.wrapping_add(0x40);
        let orig = [0x8Bu8, 0x8C, 0x39, 0x11, 0x22, 0x33, 0x44];
        let icon = icon_stub(stub, &orig, back).unwrap();
        assert_eq!(&icon[..4], &[0x41, 0x83, 0xFD, 5]);
        assert!(icon.windows(orig.len()).any(|w| w == orig));
        let skip = skip_stub(stub, back, stub.wrapping_add(0x80)).unwrap();
        assert_eq!(&skip[..4], &[0x41, 0x83, 0xFD, 5]);
        let vanilla = b"UI_INV_FILTER_ALL_TIP\0".to_vec();
        let mut pad = vanilla.clone();
        pad.resize(5 * TIP_SIZE, 0);
        let table = tip_table(&pad).unwrap();
        assert_eq!(table.len(), 5 * TIP_SIZE + BUTTON_TIPS.len() * TIP_SIZE);
        assert!(table.starts_with(b"UI_INV_FILTER_ALL_TIP\0"));
    }

    #[test]
    fn hide_image_labels_and_head() {
        let cave = 0x0000_7FF6_3000_0000usize;
        let (image, labels) = hide_image(cave, cave + 0x1000, cave + 0x2000, cave + 0x3000, cave + 0x4000, cave + 0x5000)
            .unwrap();
        assert_eq!(&image[..4], &[0x57, 0x53, 0x41, 0x57]);
        assert!(labels.contains_key("hover"));
        assert!(labels.contains_key("ticks"));
        assert!(labels.contains_key("names"));
        let ids = labels["ids"];
        assert!(ids >= cave);
        assert_eq!(labels["on"], ids + HIDE_ON_FROM_IDS);
        let on = labels["on"] - cave;
        assert_eq!(image[on + FOODS_INDEX], 0);
        assert_eq!(image[on + CUSTOM_INDEX], 0);
        assert_eq!(image[on], 1);
        let id_bytes = labels["ids"] - cave;
        assert_eq!(&image[id_bytes + FOODS_INDEX..id_bytes + CUSTOM_INDEX + 1], &[17, 18]);
        // The per-button check reads [rsp+0x30] + 16.
        let check = [0x48, 0x8B, 0x54, 0x24, 0x30, 0x80, 0x7A, 0x10, 0x00, 0x74];
        assert!(image.windows(check.len()).any(|w| w == check));
    }
}
