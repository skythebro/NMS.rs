//! Child processes that never open a console window.
//!
//! Every `std::process::Command` in the workspace is made here (`command`) or passed through
//! `hidden`. On Windows both set CREATE_NO_WINDOW: a console child (nms-rs.exe prepare,
//! MBINCompiler, hgpaktool, tasklist, PowerShell) gets a console that has no window, and its own
//! console children share it. That last part matters: hgpaktool is a PyInstaller one-file exe
//! that starts a second copy of itself. The old DETACHED_PROCESS flag left the first copy with no
//! console at all, so Windows gave the second copy a new, visible console: the flashing windows
//! during the UI merge and Apply sizes. CREATE_NO_WINDOW is ignored for GUI programs (NMS.exe,
//! steam.exe, explorer.exe), so it is safe on them too.
//!
//! Output still works: `output()` pipes stdout and stderr as usual. Started from a terminal,
//! the child gets its own hidden console instead of sharing the terminal, so anything that must
//! print straight into the user's terminal (setup re-running itself) is on the allow-list in the
//! test below and keeps a plain `Command`.
//!
//! The test `no_raw_command_new_outside_spawn` fails on any new raw `Command::new(` in the crates.

use std::ffi::OsStr;
use std::process::{Command, Stdio};

/// CREATE_NO_WINDOW from the Win32 process creation flags.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Make `cmd` start without a console window (Windows; no-op elsewhere).
pub fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `Command::new(program)` that starts without a console window.
pub fn command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    hidden(&mut cmd);
    cmd
}

/// `command(program)` for a GUI program the window starts and forgets (Explorer, NMS.exe).
///
/// Stdin, stdout and stderr are NUL. A plain `Command` hands the parent's standard handles to the
/// child, and the manager has none that work: it frees its console at start (`detach_if_alone`),
/// so the handles are stale or of a kind CreateProcess refuses, and `spawn` fails with
/// "The handle is invalid (os error 6)" or "The request is not supported (os error 50)".
pub fn gui<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = command(program);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Raw `Command::new(` that may stay, with the reason. (file under `crates/`, text on the line).
    const ALLOWED: &[(&str, &str, &str)] = &[
        (
            "nms-launcher/src/setup.rs",
            "Command::new(&child)",
            "setup re-runs itself from the game folder and must share the user's console (progress, --yes prompt)",
        ),
        (
            "nms-launcher/src/play.rs",
            "Command::new(program)",
            "the game command from Steam (%command%): a GUI exe, and the play console stays its parent",
        ),
        (
            "nms-launcher/src/play.rs",
            "Command::new(\"explorer.exe\")",
            "steam:// URL through Explorer, a GUI program: no console",
        ),
    ];

    fn crates_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates dir").to_path_buf()
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn no_raw_command_new_outside_spawn() {
        let root = crates_dir();
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        assert!(files.len() > 20, "scan found too few sources under {}", root.display());
        let needle = ["Command", "::new("].concat();
        let mut used = vec![false; ALLOWED.len()];
        let mut bad = Vec::new();
        for path in &files {
            let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if rel == "nms-core/src/spawn.rs" {
                continue;
            }
            let text = std::fs::read_to_string(path).unwrap_or_default();
            for (n, line) in text.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") || !line.contains(&needle) {
                    continue;
                }
                match ALLOWED.iter().position(|(file, snippet, _)| *file == rel && line.contains(snippet)) {
                    Some(i) => used[i] = true,
                    None => bad.push(format!("{rel}:{}: {}", n + 1, code)),
                }
            }
        }
        assert!(
            bad.is_empty(),
            "use nms_core::spawn::command (no console window) or add a justified ALLOWED entry:\n{}",
            bad.join("\n")
        );
        let stale: Vec<_> = ALLOWED.iter().zip(&used).filter(|(_, u)| !**u).map(|(a, _)| a.0).collect();
        assert!(stale.is_empty(), "ALLOWED entries that no longer match: {stale:?}");
    }

    #[cfg(windows)]
    #[test]
    fn hidden_child_runs_and_output_is_captured() {
        let out = command("cmd").args(["/c", "echo ok"]).output().expect("cmd /c echo ok");
        assert!(out.status.success(), "{out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// Regression: the manager frees its console, so its standard handles are dead. A plain
    /// `Command` then fails to spawn (os error 6 here, os error 50 on a test PC); `gui` must not.
    #[cfg(windows)]
    #[test]
    fn gui_child_starts_with_dead_standard_handles() {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetStdHandle(n: u32) -> *mut c_void;
            fn SetStdHandle(n: u32, h: *mut c_void) -> i32;
        }
        const STD_INPUT_HANDLE: u32 = -10i32 as u32;
        // SAFETY: plain Win32 calls; the handle is put back before the asserts run.
        let (raw, gui_result) = unsafe {
            let old = GetStdHandle(STD_INPUT_HANDLE);
            SetStdHandle(STD_INPUT_HANDLE, 0x7777_0004usize as *mut c_void);
            let raw = Command::new("cmd.exe").args(["/c", "exit"]).status().is_ok();
            let gui_result = gui("cmd.exe").args(["/c", "exit"]).status();
            SetStdHandle(STD_INPUT_HANDLE, old);
            (raw, gui_result)
        };
        assert!(gui_result.is_ok_and(|s| s.success()), "gui() must spawn with a dead stdin");
        assert!(!raw, "a plain Command is expected to fail with a dead stdin (that is why gui exists)");
    }

    #[cfg(windows)]
    #[test]
    fn hidden_child_has_a_console_of_its_own() {
        // CREATE_NO_WINDOW gives the child a (windowless) console, so its console children inherit
        // it instead of opening a new window. `mode con` opens CON directly, so it only works with
        // a console attached. (The DETACHED_PROCESS case is not run here: there mode.com, a console
        // grandchild, would get a new visible window, which is the bug this module fixes.)
        let out = command("cmd").args(["/c", "mode con"]).output().expect("mode con");
        assert!(out.status.success(), "{out:?}");
        assert!(!out.stdout.is_empty(), "{out:?}");
    }
}
