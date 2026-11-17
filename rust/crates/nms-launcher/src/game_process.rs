//! Process check used before the manager changes game files.

#[cfg(windows)]
use windows_sys::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W;

pub const BUSY: &str = "Close the game first. The manager cannot change files while NMS.exe is running.";
pub const UNKNOWN_KEY: &str = "game_status_unknown";

#[derive(Debug, PartialEq, Eq)]
pub enum GameStatus {
    Running,
    Closed,
    Unknown(String),
}

impl GameStatus {
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    pub fn require_closed(&self) -> Result<(), String> {
        match self {
            Self::Closed => Ok(()),
            Self::Running => Err(BUSY.into()),
            Self::Unknown(error) => Err(format!("{} ({error})", crate::i18n::I18n::new("en").t(UNKNOWN_KEY))),
        }
    }
}

pub fn status() -> GameStatus {
    query().unwrap_or_else(GameStatus::Unknown)
}

#[cfg(windows)]
fn query() -> Result<GameStatus, String> {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    // SAFETY: TH32CS_SNAPPROCESS requests a process-only snapshot. The PID is ignored.
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("CreateToolhelp32Snapshot: {}", io::Error::last_os_error()));
    }
    // SAFETY: a successful call returned a new CloseHandle-compatible handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(handle) };
    scan_snapshot(|first, entry| {
        // SAFETY: the snapshot is live and entry is writable with dwSize set.
        let ok = unsafe {
            if first {
                Process32FirstW(snapshot.as_raw_handle(), entry)
            } else {
                Process32NextW(snapshot.as_raw_handle(), entry)
            }
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
}

#[cfg(windows)]
fn scan_snapshot(
    mut read_entry: impl FnMut(bool, &mut PROCESSENTRY32W) -> std::io::Result<()>,
) -> Result<GameStatus, String> {
    use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;

    // SAFETY: this struct contains only integers and an inline UTF-16 buffer.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut first = true;
    loop {
        if let Err(error) = read_entry(first, &mut entry) {
            // First can also return NO_MORE_FILES if the snapshot has no process information.
            // A machine running this manager should have at least one process in the snapshot.
            if !first && error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(GameStatus::Closed);
            }
            let api = if first { "Process32FirstW" } else { "Process32NextW" };
            return Err(format!("{api}: {error}"));
        }
        let end = entry.szExeFile.iter().position(|&unit| unit == 0).ok_or("Unterminated process name")?;
        let name = &entry.szExeFile[..end];
        let expected = b"NMS.exe";
        if name.len() == expected.len()
            && name.iter().zip(expected).all(|(&unit, byte)| {
                u8::try_from(unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(byte))
            })
        {
            return Ok(GameStatus::Running);
        }
        first = false;
    }
}

#[cfg(not(windows))]
fn query() -> Result<GameStatus, String> {
    let status = nms_core::spawn::command("pgrep")
        .args(["-x", "NMS[.]exe"])
        .status()
        .map_err(|error| format!("pgrep: {error}"))?;
    match status.code() {
        Some(0) => Ok(GameStatus::Running),
        Some(1) => Ok(GameStatus::Closed),
        _ => Err(format!("pgrep: {status}")),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io;
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_NO_MORE_FILES};

    fn scan(entries: &[Result<&str, u32>]) -> GameStatus {
        let mut index = 0;
        scan_snapshot(|first, entry| {
            assert_eq!(first, index == 0);
            let item = entries[index];
            index += 1;
            let name = item.map_err(|code| io::Error::from_raw_os_error(code as i32))?;
            entry.szExeFile.fill(0);
            for (slot, unit) in entry.szExeFile.iter_mut().zip(name.encode_utf16()) {
                *slot = unit;
            }
            Ok(())
        })
        .unwrap_or_else(GameStatus::Unknown)
    }

    #[test]
    fn finds_case_insensitive_exact_name_in_first_or_later_entry() {
        assert_eq!(scan(&[Ok("nms.EXE")]), GameStatus::Running);
        assert_eq!(scan(&[Ok("other.exe"), Ok("NmS.Exe")]), GameStatus::Running);
    }

    #[test]
    fn end_of_complete_scan_allows_writes_but_partial_scan_does_not() {
        let other = [Ok("NMS.exe.old"), Ok("notNMS.exe"), Ok("ŃMS.exe")];
        assert_eq!(scan(&[other[0], other[1], other[2], Err(ERROR_NO_MORE_FILES)]), GameStatus::Closed);
        assert!(matches!(
            scan(&[other[0], Err(ERROR_ACCESS_DENIED)]),
            GameStatus::Unknown(ref error) if error.contains("Process32NextW")
        ));
    }

    #[test]
    fn failure_to_read_first_entry_is_unknown_even_with_no_more_files() {
        for code in [ERROR_ACCESS_DENIED, ERROR_NO_MORE_FILES] {
            assert!(matches!(
                scan(&[Err(code)]),
                GameStatus::Unknown(ref error) if error.contains("Process32FirstW")
            ));
        }
    }
}
