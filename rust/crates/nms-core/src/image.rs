//! A view of NMS.exe laid out by RVA, either the running game or the file on disk.
//!
//! Both give the same offsets, so a signature checked offline against `Binaries\NMS.exe`
//! resolves to the same RVA in the game.

use std::ops::Range;
use std::path::Path;

use crate::pattern::Pattern;

const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub rva: usize,
    pub virtual_size: usize,
    pub raw_offset: usize,
    pub raw_size: usize,
    pub executable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headers {
    pub size_of_image: usize,
    pub size_of_headers: usize,
    pub sections: Vec<Section>,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| format!("PE header cut short at {at:#x}"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("PE header cut short at {at:#x}"))
}

/// Parse the DOS, NT and section headers from the first bytes of a PE32+ image.
pub fn parse_headers(b: &[u8]) -> Result<Headers, String> {
    if u16_at(b, 0)? != 0x5A4D {
        return Err("not an MZ file".into());
    }
    let nt = u32_at(b, 0x3C)? as usize;
    if u32_at(b, nt)? != 0x0000_4550 {
        return Err("no PE signature".into());
    }
    let file_header = nt + 4;
    let sections_count = u16_at(b, file_header + 2)? as usize;
    let optional_size = u16_at(b, file_header + 16)? as usize;
    let optional = file_header + 20;
    if u16_at(b, optional)? != 0x20B {
        return Err("not a 64-bit image".into());
    }
    let size_of_image = u32_at(b, optional + 56)? as usize;
    let size_of_headers = u32_at(b, optional + 60)? as usize;
    let table = optional + optional_size;
    let mut sections = Vec::with_capacity(sections_count);
    for i in 0..sections_count {
        let s = table + i * 40;
        let raw_name = b.get(s..s + 8).ok_or("section table cut short")?;
        let name = String::from_utf8_lossy(raw_name).trim_end_matches('\0').to_string();
        sections.push(Section {
            name,
            virtual_size: u32_at(b, s + 8)? as usize,
            rva: u32_at(b, s + 12)? as usize,
            raw_size: u32_at(b, s + 16)? as usize,
            raw_offset: u32_at(b, s + 20)? as usize,
            executable: u32_at(b, s + 36)? & IMAGE_SCN_MEM_EXECUTE != 0,
        });
    }
    Ok(Headers { size_of_image, size_of_headers, sections })
}

/// Result of looking a pattern up where exactly one hit is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Unique(usize),
    Missing,
    Multiple(usize),
}

impl Lookup {
    pub fn unique(self) -> Option<usize> {
        match self {
            Lookup::Unique(rva) => Some(rva),
            _ => None,
        }
    }
}

/// Code laid out by RVA that can be scanned, read and patched.
pub trait Code {
    /// RVA ranges of the executable sections.
    fn exec_ranges(&self) -> Vec<Range<usize>>;
    /// Bytes at an RVA, or `None` if the range is outside the image.
    fn read(&self, rva: usize, len: usize) -> Option<Vec<u8>>;
    /// Overwrite code at an RVA.
    fn write(&mut self, rva: usize, bytes: &[u8]) -> Result<(), String>;
    /// Run `f` over the bytes of one RVA range.
    fn with_range(&self, range: Range<usize>, f: &mut dyn FnMut(&[u8]));

    /// Up to `limit` RVAs where the pattern matches inside executable sections.
    fn find_all(&self, pattern: &Pattern, limit: usize) -> Vec<usize> {
        let mut hits = Vec::new();
        for range in self.exec_ranges() {
            let start = range.start;
            self.with_range(range, &mut |bytes| {
                for off in pattern.find_all(bytes, limit - hits.len()) {
                    hits.push(start + off);
                }
            });
            if hits.len() >= limit {
                break;
            }
        }
        hits
    }

    /// Target RVA of a rel32 operand at `disp_rva`, e.g. the 4 bytes after an `E8` call opcode
    /// (not the opcode itself). The displacement counts from the end of those 4 bytes.
    fn rel32_target(&self, disp_rva: usize) -> Option<usize> {
        let b = self.read(disp_rva, 4)?;
        let disp = i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64;
        usize::try_from(disp_rva as i64 + 4 + disp).ok()
    }

    fn find_unique(&self, pattern: &Pattern) -> Lookup {
        match self.find_all(pattern, 2).as_slice() {
            [] => Lookup::Missing,
            [one] => Lookup::Unique(*one),
            many => Lookup::Multiple(many.len()),
        }
    }
}

/// NMS.exe read from disk and laid out by RVA, for offline signature checks and tests.
#[derive(Clone)]
pub struct FileImage {
    bytes: Vec<u8>,
    exec: Vec<Range<usize>>,
}

impl FileImage {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_file_bytes(&file)
    }

    pub fn from_file_bytes(file: &[u8]) -> Result<Self, String> {
        let headers = parse_headers(file)?;
        let mut bytes = vec![0u8; headers.size_of_image];
        let head = headers.size_of_headers.min(file.len()).min(bytes.len());
        bytes[..head].copy_from_slice(&file[..head]);
        let mut exec = Vec::new();
        for s in &headers.sections {
            let len = s.raw_size;
            let src = file.get(s.raw_offset..s.raw_offset + len).ok_or_else(|| format!("section {} past end of file", s.name))?;
            let end = (s.rva + len).min(bytes.len());
            bytes[s.rva..end].copy_from_slice(&src[..end - s.rva]);
            if s.executable {
                exec.push(s.rva..s.rva + s.virtual_size.max(s.raw_size).min(bytes.len() - s.rva));
            }
        }
        Ok(Self { bytes, exec })
    }

    /// The whole image laid out by RVA (headers, code and data), for scans outside code such as string anchors.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A bare executable buffer at RVA 0, for unit tests.
    #[allow(clippy::single_range_in_vec_init)]
    pub fn from_code(code: &[u8]) -> Self {
        Self { bytes: code.to_vec(), exec: vec![0..code.len()] }
    }
}

impl Code for FileImage {
    fn exec_ranges(&self) -> Vec<Range<usize>> {
        self.exec.clone()
    }

    fn read(&self, rva: usize, len: usize) -> Option<Vec<u8>> {
        self.bytes.get(rva..rva + len).map(<[u8]>::to_vec)
    }

    fn write(&mut self, rva: usize, bytes: &[u8]) -> Result<(), String> {
        let dst = self.bytes.get_mut(rva..rva + bytes.len()).ok_or("write outside the image")?;
        dst.copy_from_slice(bytes);
        Ok(())
    }

    fn with_range(&self, range: Range<usize>, f: &mut dyn FnMut(&[u8])) {
        if let Some(slice) = self.bytes.get(range) {
            f(slice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE32+ file: headers in the first 0x200 bytes, one executable section at RVA 0x1000.
    pub(crate) fn tiny_pe(code: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 0x400];
        f[0] = b'M';
        f[1] = b'Z';
        f[0x3C] = 0x80;
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        let fh = 0x84;
        f[fh + 2] = 1; // one section
        f[fh + 16] = 0xF0; // size of optional header
        let opt = fh + 20;
        f[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        f[opt + 56..opt + 60].copy_from_slice(&0x2000u32.to_le_bytes());
        f[opt + 60..opt + 64].copy_from_slice(&0x200u32.to_le_bytes());
        let s = opt + 0xF0;
        f[s..s + 5].copy_from_slice(b".text");
        f[s + 8..s + 12].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        f[s + 16..s + 20].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 20..s + 24].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 36..s + 40].copy_from_slice(&0x6000_0020u32.to_le_bytes());
        f[0x200..0x200 + code.len()].copy_from_slice(code);
        f
    }

    #[test]
    fn lays_out_sections_by_rva() {
        let file = tiny_pe(&[0x90, 0xAA, 0xBB, 0xCC]);
        let img = FileImage::from_file_bytes(&file).unwrap();
        assert_eq!(img.exec_ranges(), vec![0x1000..0x1200]);
        let p = Pattern::parse("AA BB CC").unwrap();
        assert_eq!(img.find_unique(&p), Lookup::Unique(0x1001));
        assert_eq!(img.read(0x1001, 2), Some(vec![0xAA, 0xBB]));
    }

    #[test]
    fn reports_missing_and_multiple() {
        let img = FileImage::from_code(&[1, 2, 1, 2]);
        assert_eq!(img.find_unique(&Pattern::parse("01 02").unwrap()), Lookup::Multiple(2));
        assert_eq!(img.find_unique(&Pattern::parse("03").unwrap()), Lookup::Missing);
    }

    #[test]
    fn resolves_call_targets_from_the_byte_after_e8() {
        // call +0x10 at RVA 2: target = 2 + 1 + 4 + 0x10.
        let img = FileImage::from_code(&[0x90, 0x90, 0xE8, 0x10, 0, 0, 0, 0x90]);
        assert_eq!(img.rel32_target(3), Some(0x17));
        let back = FileImage::from_code(&[0xE8, 0xFB, 0xFF, 0xFF, 0xFF]);
        assert_eq!(back.rel32_target(1), Some(0));
    }

    #[test]
    fn rejects_non_pe() {
        assert!(FileImage::from_file_bytes(&[0u8; 64]).is_err());
    }
}
