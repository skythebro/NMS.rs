//! Start the core once the DLL is loaded: log, kill switch, UI merge, mods, settings pipe.

use std::io;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;

use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{paths, Registry};

const PREPARE_LOG_PREFIX: &str = "prepare:";

pub fn run() {
    let exe = std::env::current_exe().unwrap_or_default();
    let is_game = exe.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("NMS.exe"));
    let Some(root) = paths::game_root() else { return };
    if !is_game {
        return;
    }
    let dir = paths::loader_dir(&root);
    let log = Arc::new(Log::open(&dir.join("logs").join("core.log"), true));
    log.info(&format!("nms-rs {} loaded into {}", nms_core::ipc::VERSION, exe.display()));
    if dir.join("disabled").exists() {
        log.info("NMS.rs\\disabled exists: staying vanilla");
        return;
    }
    merge_ui(&log, &dir, spawn_prepare);

    let image = match ProcessImage::current() {
        Ok(i) => i,
        Err(e) => {
            log.info(&format!("cannot read the game image, staying vanilla: {e}"));
            return;
        }
    };
    log.info(&format!("image base {:#x}", image.base()));
    let mut registry = Registry::default();
    // Dev-only: the self-test enables and disables a hook live, so it never ships.
    #[cfg(feature = "selftest")]
    registry.add(Box::new(crate::selftest::SelfTest::new(&dir, image.clone(), log.clone())));
    nms_mods::register(&mut registry, &nms_mods::Ctx { root, image: image.clone(), log: log.clone() });
    log.info(&format!("mods: {}", registry.names().join(", ")));

    let registry = Arc::new(registry);
    nms_mods::install_options_menu(&image, registry.clone(), log.clone());

    let pipe_log = log.clone();
    nms_core::ipc::serve(registry, move |m| pipe_log.info(m));
    log.info("settings pipe ready");
}

fn spawn_prepare(mut cmd: Command) -> io::Result<Output> {
    cmd.output()
}

/// `nms-rs.exe prepare` with cwd `<game>\NMS.rs`. Each path is its own `Command` field, not a shell line.
/// No console window (`nms_core::spawn`); its MBINCompiler and hgpaktool children share that
/// windowless console.
fn prepare_command(exe: &Path, cwd: &Path) -> Command {
    let mut cmd = nms_core::spawn::command(exe);
    cmd.arg("prepare");
    cmd.current_dir(cwd);
    cmd
}

/// First work on the boot thread after DllMain. Never panics. Failure still lets hooks register.
fn merge_ui(log: &Log, loader: &Path, run_child: impl FnOnce(Command) -> io::Result<Output>) {
    let exe = loader.join("nms-rs.exe");
    if !exe.is_file() {
        log.info("UI merge skipped: NMS.rs\\nms-rs.exe is not installed.");
        return;
    }
    let cmd = prepare_command(&exe, loader);
    match run_child(cmd) {
        Ok(out) => {
            log_prepare_stream(log, &out.stdout);
            log_prepare_stream(log, &out.stderr);
            if out.status.success() {
                log.info("UI merge finished.");
            } else {
                match out.status.code() {
                    Some(code) => log.info(&format!("UI merge failed: exit {code}.")),
                    None => log.info("UI merge failed: child exited without a status."),
                }
            }
        }
        Err(e) => {
            log.info(&format!("UI merge failed to start: {e}"));
        }
    }
}

fn log_prepare_stream(log: &Log, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    for line in text.lines() {
        if !line.is_empty() {
            log.info(&format!("{PREPARE_LOG_PREFIX} {line}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::windows::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    fn temp_loader(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nms-proxy-boot-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp loader");
        dir
    }

    fn open_log(loader: &Path) -> (Log, PathBuf) {
        let path = loader.join("core-test.log");
        (Log::open(&path, true), path)
    }

    fn fake_output(code: u32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output { status: ExitStatus::from_raw(code), stdout: stdout.to_vec(), stderr: stderr.to_vec() }
    }

    #[test]
    fn prepare_command_is_nms_rs_prepare_in_loader_dir() {
        let loader = Path::new(r"C:\Games\No Man's Sky\NMS.rs");
        let exe = loader.join("nms-rs.exe");
        let cmd = prepare_command(&exe, loader);
        assert_eq!(Path::new(cmd.get_program()), exe.as_path());
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["prepare"]);
        assert_eq!(cmd.get_current_dir(), Some(loader));
    }

    #[test]
    fn missing_exe_skips_without_calling_runner() {
        let loader = temp_loader("missing");
        let (log, log_path) = open_log(&loader);
        let mut ran = false;
        merge_ui(&log, &loader, |_| {
            ran = true;
            Ok(fake_output(0, b"", b""))
        });
        assert!(!ran, "missing nms-rs.exe must not spawn");
        let text = fs::read_to_string(&log_path).unwrap();
        assert!(
            text.contains("UI merge skipped: NMS.rs\\nms-rs.exe is not installed."),
            "{text}"
        );
        assert!(!text.contains("UI merge failed"), "{text}");
        let _ = fs::remove_dir_all(&loader);
    }

    #[test]
    fn child_exit_1_is_logged_and_run_continues() {
        let loader = temp_loader("exit1");
        fs::write(loader.join("nms-rs.exe"), b"").expect("dummy nms-rs.exe");
        let (log, log_path) = open_log(&loader);
        merge_ui(&log, &loader, |cmd| {
            assert_eq!(Path::new(cmd.get_program()).file_name().and_then(|n| n.to_str()), Some("nms-rs.exe"));
            let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
            assert_eq!(args, ["prepare"]);
            assert_eq!(cmd.get_current_dir(), Some(loader.as_path()));
            Ok(fake_output(1, b"stdout line\n", b"stderr line\n"))
        });
        let text = fs::read_to_string(&log_path).unwrap();
        assert!(text.contains("prepare: stdout line"), "{text}");
        assert!(text.contains("prepare: stderr line"), "{text}");
        assert!(text.contains("UI merge failed: exit 1."), "{text}");
        assert!(!text.contains("UI merge finished."), "{text}");
        let _ = fs::remove_dir_all(&loader);
    }

    #[test]
    fn spawn_error_is_logged_and_run_continues() {
        let loader = temp_loader("spawn");
        fs::write(loader.join("nms-rs.exe"), b"").expect("dummy nms-rs.exe");
        let (log, log_path) = open_log(&loader);
        merge_ui(&log, &loader, |_| Err(io::Error::new(io::ErrorKind::Other, "injected spawn failure")));
        let text = fs::read_to_string(&log_path).unwrap();
        assert!(text.contains("UI merge failed to start: injected spawn failure"), "{text}");
        let _ = fs::remove_dir_all(&loader);
    }

    #[test]
    fn zero_exit_logs_finished() {
        let loader = temp_loader("ok");
        fs::write(loader.join("nms-rs.exe"), b"").expect("dummy nms-rs.exe");
        let (log, log_path) = open_log(&loader);
        merge_ui(&log, &loader, |_| Ok(fake_output(0, b"Checking UI merge.\n", b"")));
        let text = fs::read_to_string(&log_path).unwrap();
        assert!(text.contains("prepare: Checking UI merge."), "{text}");
        assert!(text.contains("UI merge finished."), "{text}");
        let _ = fs::remove_dir_all(&loader);
    }
}
