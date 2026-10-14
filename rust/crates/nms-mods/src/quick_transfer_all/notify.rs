//! Sort-walk notify cave: fire-byte toasts for Deposit and lock, duration 1.5 in the cave.

use std::collections::HashMap;

use nms_core::mem::{alloc_exec_near, read_bytes, read_i32, read_u8};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::quick_transfer_all as sigs;

use super::caves::{
    find_sig, rel32_bytes, rel32_va, write_code, write_exec, ButtonNotify, NOTIFY_PATCH_BYTES,
};
use super::text;

const NOTIFY_TOAST_GLOBAL_AT: usize = 3;
const NOTIFY_TOAST_TIMER_AT: usize = 20;
const NOTIFY_TOAST_R9_AT: usize = 40;
const NOTIFY_TOAST_OBJECT_AT: usize = 52;
const NOTIFY_TOAST_ADD_AT: usize = 84;
const NOTIFY_TOAST_CLEAN_AT: usize = 94;
const NOTIFY_DURATION: [u8; 4] = [0x00, 0x00, 0xC0, 0x3F];
const DEPOSIT_HASH_AT: usize = 0x5C;
const DEPOSIT_CHILD_AT: usize = 0x6A;

fn emit(code: &mut Vec<u8>, raw: &[u8]) -> usize {
    let at = code.len();
    code.extend_from_slice(raw);
    at
}

fn put_rel32(code: &mut [u8], at: usize, cave: usize, target: usize) -> Result<(), String> {
    let bytes = rel32_bytes(cave.wrapping_add(at), target)?;
    if at.wrapping_add(4) > code.len() {
        return Err("rel32 site missing".into());
    }
    code[at..at + 4].copy_from_slice(&bytes);
    Ok(())
}

/// Confirm walk stays in the image but `visual` starts 0, matching Python.
#[allow(clippy::too_many_arguments)]
fn notify_image(
    cave: usize,
    back: usize,
    timer_fn: usize,
    add_fn: usize,
    clean_fn: usize,
    global_addr: usize,
    title: usize,
    object_off: i32,
    hash_fn: usize,
    child_fn: usize,
    confirm_fn: usize,
    lang: &str,
) -> Result<(Vec<u8>, HashMap<&'static str, usize>), String> {
    let mut code = Vec::new();
    let mut fix: Vec<(usize, usize, &'static str)> = Vec::new();
    emit(&mut code, &[0x53, 0x56, 0x41, 0x56, 0x41, 0x57]);
    emit(&mut code, &[0x48, 0x83, 0xEC, 0x60]);
    let _ = (hash_fn, child_fn, confirm_fn);
    let cmp_dep = emit(&mut code, &[0x80, 0x3D]);
    let dep_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((dep_d, cmp_dep + 7, "dep_fire"));
    emit(&mut code, &[0x00]);
    let jz_dep = emit(&mut code, &[0x0F, 0x84, 0, 0, 0, 0]);
    let dep_off = emit(&mut code, &[0xC6, 0x05]);
    let dep_off_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((dep_off_d, dep_off + 7, "dep_fire"));
    emit(&mut code, &[0x00]);
    let lea_dosd = emit(&mut code, &[0x4C, 0x8D, 0x3D]);
    let dosd_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((dosd_d, lea_dosd + 7, "dep_osd"));
    let toast1 = emit_toast(&mut code, &mut fix, object_off);
    let lock_walk = code.len();
    let cmp_fire = emit(&mut code, &[0x80, 0x3D]);
    let fire_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((fire_d, cmp_fire + 7, "lock_fire"));
    emit(&mut code, &[0x00]);
    let jz_lock = emit(&mut code, &[0x0F, 0x84, 0, 0, 0, 0]);
    let fire_off = emit(&mut code, &[0xC6, 0x05]);
    let fire_off_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((fire_off_d, fire_off + 7, "lock_fire"));
    emit(&mut code, &[0x00]);
    let cmp_mode = emit(&mut code, &[0x80, 0x3D]);
    let mode_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((mode_d, cmp_mode + 7, "lock_mode"));
    emit(&mut code, &[0x00]);
    let lea_loff = emit(&mut code, &[0x4C, 0x8D, 0x3D]);
    let loff_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((loff_d, lea_loff + 7, "lock_off"));
    let jz_mode = emit(&mut code, &[0x74, 0]);
    let lea_lon = emit(&mut code, &[0x4C, 0x8D, 0x3D]);
    let lon_d = emit(&mut code, &[0, 0, 0, 0]);
    fix.push((lon_d, lea_lon + 7, "lock_on"));
    let toast2_at = code.len();
    let toast2 = emit_toast(&mut code, &mut fix, object_off);
    let done = code.len();
    emit(&mut code, &[0x48, 0x83, 0xC4, 0x60, 0x41, 0x5F, 0x41, 0x5E, 0x5E, 0x5B]);
    emit(&mut code, NOTIFY_PATCH_BYTES);
    let jmp_back = emit(&mut code, &[0xE9, 0, 0, 0, 0]);
    let rel = toast2_at as i32 - (jz_mode as i32 + 2);
    if !(0..=127).contains(&rel) {
        return Err("lock-mode toast jump does not fit".into());
    }
    code[jz_mode + 1] = rel as u8;
    put_rel32(&mut code, jz_dep + 2, cave, cave + lock_walk)?;
    put_rel32(&mut code, jz_lock + 2, cave, cave + done)?;
    put_rel32(&mut code, jmp_back + 1, cave, back)?;
    for block in [toast1, toast2] {
        let (call_t0, call_add, call_cl, call_t1, call_t2, r9_at, glob_at) = block;
        put_rel32(&mut code, call_t0 + 1, cave, timer_fn)?;
        put_rel32(&mut code, call_add + 1, cave, add_fn)?;
        put_rel32(&mut code, call_cl + 1, cave, clean_fn)?;
        put_rel32(&mut code, call_t1 + 1, cave, timer_fn)?;
        put_rel32(&mut code, call_t2 + 1, cave, timer_fn)?;
        let r9 = rel32_bytes(cave.wrapping_add(r9_at), title)?;
        code[r9_at..r9_at + 4].copy_from_slice(&r9);
        let g = rel32_bytes(cave.wrapping_add(glob_at), global_addr)?;
        code[glob_at..glob_at + 4].copy_from_slice(&g);
    }
    let body_len = code.len();
    let pad = (16 - (body_len % 16)) % 16;
    code.extend(std::iter::repeat_n(0u8, pad));
    let mut labels = HashMap::new();
    let mut add = |label: &'static str, raw: &[u8], code: &mut Vec<u8>| {
        labels.insert(label, code.len());
        code.extend_from_slice(raw);
    };
    // Toast texts (`TOAST_LEN` each): AddTimedMessage copies a C string of up to 0x200 bytes.
    // `dep_osd` is rewritten before each Deposit toast (`set_deposit_osd`).
    add("dep_osd", &text::toast(text::get(lang, "nothing")), &mut code);
    add("lock_on", &text::toast(text::get(lang, "lock_on")), &mut code);
    add("lock_off", &text::toast(text::get(lang, "lock_off")), &mut code);
    add("timer", &[0u8; 16], &mut code);
    add("duration", &NOTIFY_DURATION, &mut code);
    add("lock_mode", &[0], &mut code);
    add("lock_fire", &[0], &mut code);
    add("dep_fire", &[0], &mut code);
    if code.len() > 0xC00 {
        return Err("the sort-notice pass does not fit the cave".into());
    }
    for (at, end, label) in fix {
        let dest = *labels.get(label).ok_or("cave label missing")?;
        let bytes = rel32_bytes(cave.wrapping_add(end).wrapping_sub(4), cave.wrapping_add(dest))?;
        code[at..at + 4].copy_from_slice(&bytes);
    }
    let abs = labels.into_iter().map(|(k, v)| (k, cave + v)).collect();
    Ok((code, abs))
}

fn emit_toast(
    code: &mut Vec<u8>,
    fix: &mut Vec<(usize, usize, &'static str)>,
    object_off: i32,
) -> (usize, usize, usize, usize, usize, usize, usize) {
    emit(code, &[0xC6, 0x44, 0x24, 0x50, 0x00]);
    let lea_tm = emit(code, &[0x48, 0x8D, 0x05]);
    let tm = emit(code, &[0, 0, 0, 0]);
    fix.push((tm, lea_tm + 7, "timer"));
    emit(code, &[0xC7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x48, 0x8B, 0xC8]);
    let call_t0 = emit(code, &[0xE8, 0, 0, 0, 0]);
    emit(code, &[0xC6, 0x44, 0x24, 0x50, 0x00]);
    let lea_ax = emit(code, &[0x48, 0x8D, 0x05]);
    let ax = emit(code, &[0, 0, 0, 0]);
    fix.push((ax, lea_ax + 7, "timer"));
    emit(code, &[0xC6, 0x44, 0x24, 0x48, 0x00]);
    let r9_at = emit(code, &[0x4C, 0x8D, 0x0D, 0, 0, 0, 0]);
    emit(code, &[0xC6, 0x44, 0x24, 0x40, 0x00]);
    let glob_at = emit(code, &[0x48, 0x8B, 0x1D, 0, 0, 0, 0]);
    let mut lea_obj = vec![0x48, 0x8D, 0x8B];
    lea_obj.extend_from_slice(&object_off.to_le_bytes());
    emit(code, &lea_obj);
    emit(code, &[0x0F, 0x57, 0xC0, 0xF3, 0x0F, 0x11, 0x44, 0x24, 0x38]);
    let load_d2 = emit(code, &[0xF3, 0x0F, 0x10, 0x15]);
    let dur = emit(code, &[0, 0, 0, 0]);
    fix.push((dur, load_d2 + 8, "duration"));
    emit(code, &[0x48, 0x89, 0x44, 0x24, 0x28, 0x49, 0x8B, 0xD7]);
    let mut audio = vec![0xC7, 0x44, 0x24, 0x20];
    audio.extend_from_slice(&0x18AECCF6u32.to_le_bytes());
    emit(code, &audio);
    let call_add = emit(code, &[0xE8, 0, 0, 0, 0]);
    let lea_cl = emit(code, &[0x48, 0x8D, 0x0D]);
    let cl = emit(code, &[0, 0, 0, 0]);
    fix.push((cl, lea_cl + 7, "timer"));
    let call_cl = emit(code, &[0xE8, 0, 0, 0, 0]);
    emit(code, &[0xB2, 0x01]);
    let lea_t1 = emit(code, &[0x48, 0x8D, 0x0D]);
    let t1 = emit(code, &[0, 0, 0, 0]);
    fix.push((t1, lea_t1 + 7, "timer"));
    let call_t1 = emit(code, &[0xE8, 0, 0, 0, 0]);
    emit(code, &[0x33, 0xDB]);
    let lea_t2 = emit(code, &[0x48, 0x8D, 0x0D]);
    let t2 = emit(code, &[0, 0, 0, 0]);
    fix.push((t2, lea_t2 + 7, "timer"));
    emit(code, &[0x89, 0x19]);
    let call_t2 = emit(code, &[0xE8, 0, 0, 0, 0]);
    (call_t0, call_add, call_cl, call_t1, call_t2, r9_at + 3, glob_at + 3)
}

pub fn install_button_notify(image: &ProcessImage, lang: &str) -> Result<ButtonNotify, String> {
    let patch_rva = find_sig(image, sigs::NOTIFY_PATCH)
        .ok_or_else(|| "the sort click notice was not found (game update?)".to_string())?;
    let toast_rva = find_sig(image, sigs::NOTIFY_TOAST)
        .ok_or_else(|| "the sort click notice was not found (game update?)".to_string())?;
    let site = image.va(patch_rva);
    let toast = image.va(toast_rva);
    let mut hash_fn = 0usize;
    let mut child_fn = 0usize;
    let mut confirm_fn = 0usize;
    if let (Some(lookup_rva), Some(confirm_rva)) = (find_sig(image, sigs::DEPOSIT_LOOKUP), find_sig(image, sigs::CONFIRM))
    {
        let lookup_va = image.va(lookup_rva);
        if read_u8(lookup_va.wrapping_add(DEPOSIT_HASH_AT)) == Some(0xE8)
            && read_u8(lookup_va.wrapping_add(DEPOSIT_CHILD_AT)) == Some(0xE8)
        {
            hash_fn = rel32_va(lookup_va.wrapping_add(DEPOSIT_HASH_AT).wrapping_add(1)).unwrap_or(0);
            child_fn = rel32_va(lookup_va.wrapping_add(DEPOSIT_CHILD_AT).wrapping_add(1)).unwrap_or(0);
            confirm_fn = image.va(confirm_rva);
        }
    }
    if read_u8(toast.wrapping_add(NOTIFY_TOAST_TIMER_AT)) != Some(0xE8) {
        return Err("the sort notice timer call is not where it was".into());
    }
    if read_u8(toast.wrapping_add(NOTIFY_TOAST_ADD_AT)) != Some(0xE8) {
        return Err("the sort notice AddTimedMessage call is not where it was".into());
    }
    if read_u8(toast.wrapping_add(NOTIFY_TOAST_CLEAN_AT)) != Some(0xE8) {
        return Err("the sort notice cleanup call is not where it was".into());
    }
    if read_u8(toast.wrapping_add(NOTIFY_TOAST_R9_AT)) != Some(0x4C) {
        return Err("the sort notice title lea is not where it was".into());
    }
    if read_bytes(toast.wrapping_add(NOTIFY_TOAST_OBJECT_AT), 3).as_deref() != Some(&[0x48, 0x8D, 0x8B]) {
        return Err("the sort notice object lea is not where it was".into());
    }
    let timer_fn = rel32_va(toast.wrapping_add(NOTIFY_TOAST_TIMER_AT).wrapping_add(1)).unwrap_or(0);
    let add_fn = rel32_va(toast.wrapping_add(NOTIFY_TOAST_ADD_AT).wrapping_add(1)).unwrap_or(0);
    let clean_fn = rel32_va(toast.wrapping_add(NOTIFY_TOAST_CLEAN_AT).wrapping_add(1)).unwrap_or(0);
    let holder_disp = read_i32(toast.wrapping_add(NOTIFY_TOAST_GLOBAL_AT)).unwrap_or(0);
    let global_addr = toast.wrapping_add(7).wrapping_add(holder_disp as usize);
    let title_disp = read_i32(toast.wrapping_add(NOTIFY_TOAST_R9_AT).wrapping_add(3)).unwrap_or(0);
    let title = toast
        .wrapping_add(NOTIFY_TOAST_R9_AT)
        .wrapping_add(7)
        .wrapping_add(title_disp as usize);
    let object_off = read_i32(toast.wrapping_add(NOTIFY_TOAST_OBJECT_AT).wrapping_add(3)).unwrap_or(0);
    let head = read_bytes(site, NOTIFY_PATCH_BYTES.len()).unwrap_or_default();
    let cave = if head == NOTIFY_PATCH_BYTES {
        alloc_exec_near(site, 0x1000).ok_or_else(|| "no memory free near the game code for the deposit button".to_string())?
    } else if head.first() == Some(&0xE9) {
        rel32_va(site.wrapping_add(1)).ok_or_else(|| "the sort click restore is not where it was".to_string())?
    } else {
        return Err(format!(
            "the sort click restore is not where it was (found {})",
            head.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
        ));
    };
    let (image_bytes, labels) = notify_image(
        cave,
        site + NOTIFY_PATCH_BYTES.len(),
        timer_fn,
        add_fn,
        clean_fn,
        global_addr,
        title,
        object_off,
        hash_fn,
        child_fn,
        confirm_fn,
        lang,
    )?;
    write_exec(cave, &image_bytes);
    if head == NOTIFY_PATCH_BYTES {
        let rel = rel32_bytes(site.wrapping_add(1), cave)?;
        let mut jmp = vec![0xE9];
        jmp.extend_from_slice(&rel);
        write_code(site, &jmp)?;
    }
    let toast = Some((add_fn, clean_fn, global_addr, title, object_off));
    Ok(ButtonNotify::new(
        *labels.get("lock_mode").unwrap_or(&0),
        *labels.get("lock_fire").unwrap_or(&0),
        *labels.get("dep_fire").unwrap_or(&0),
        *labels.get("dep_osd").unwrap_or(&0),
        toast,
        *labels.get("visual").unwrap_or(&0),
        *labels.get("dep_click").unwrap_or(&0),
        *labels.get("lock_click").unwrap_or(&0),
    ))
}

pub fn lock_click_ok(image: &ProcessImage) -> bool {
    let Some(confirm) = Pattern::parse(sigs::CONFIRM)
        .ok()
        .and_then(|p| image.find_unique(&p).unique())
    else {
        return false;
    };
    let Some(block) = Pattern::parse(sigs::FILTER)
        .ok()
        .and_then(|p| image.find_unique(&p).unique())
    else {
        return false;
    };
    let Some(lookup) = find_sig(image, sigs::DEPOSIT_LOOKUP) else {
        return false;
    };
    let _ = confirm;
    let hover = image.va(block).wrapping_add(super::caves::HOVER_FLAG_AT);
    read_bytes(hover, super::caves::HOVER_FLAG_BYTES.len()).as_deref() == Some(super::caves::HOVER_FLAG_BYTES)
        && read_u8(image.va(lookup).wrapping_add(0x84)) == Some(0xE8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rip_targets(code: &[u8], cave: usize, opcode: &[u8]) -> Vec<usize> {
        code.windows(7)
            .enumerate()
            .filter(|(_, w)| w.starts_with(opcode))
            .map(|(at, w)| {
                let disp = i32::from_le_bytes([w[3], w[4], w[5], w[6]]);
                cave.wrapping_add(at + 7).wrapping_add(disp as isize as usize)
            })
            .collect()
    }

    #[test]
    fn toast_title_and_global_resolve() {
        let cave = 0x1_5000_0000usize;
        let (global, title) = (0x1_4500_0000usize, 0x1_4600_0000usize);
        let (code, _) = notify_image(cave, 0x1_4000_1000, 0x1_4000_2000, 0x1_4000_3000, 0x1_4000_4000, global, title, 0x120, 0, 0, 0, "en")
            .expect("cave builds");
        assert_eq!(rip_targets(&code, cave, &[0x48, 0x8B, 0x1D]), vec![global, global]);
        assert_eq!(rip_targets(&code, cave, &[0x4C, 0x8D, 0x0D]), vec![title, title]);
    }

    #[test]
    fn toast_slots_hold_the_language_and_point_right() {
        let cave = 0x1_5000_0000usize;
        let (code, labels) =
            notify_image(cave, 0x1_4000_1000, 0x1_4000_2000, 0x1_4000_3000, 0x1_4000_4000, 0x1_4500_0000, 0x1_4600_0000, 0x120, 0, 0, 0, "ko")
                .expect("cave builds");
        assert!(code.len() <= 0xC00);
        for (label, id) in [("dep_osd", "nothing"), ("lock_on", "lock_on"), ("lock_off", "lock_off")] {
            let at = labels[label] - cave;
            let want = text::get("ko", id).as_bytes();
            assert_eq!(&code[at..at + want.len()], want, "{label}");
            assert_eq!(code[at + want.len()], 0, "{label}");
        }
        assert_eq!(labels["lock_on"] - labels["dep_osd"], text::TOAST_LEN);
        assert_eq!(labels["lock_off"] - labels["lock_on"], text::TOAST_LEN);
        assert_eq!(labels["timer"] - labels["lock_off"], text::TOAST_LEN);
        // `lea r15, [rip+x]` targets: the Deposit text, then lock off and lock on.
        let lea = rip_targets(&code, cave, &[0x4C, 0x8D, 0x3D]);
        assert_eq!(lea, vec![labels["dep_osd"], labels["lock_off"], labels["lock_on"]]);
    }
}
