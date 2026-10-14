//! Find or download MBINCompiler and HGPAKtool, then run them with no extra window.

use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
use zip::ZipArchive;

const USER_AGENT: &str = "NMS.rs UI merge";
const MBINCOMPILER_REPO: &str = "monkeyman192/MBINCompiler";
const HGPAKTOOL_REPO: &str = "monkeyman192/HGPAKtool";
// Downloads are pinned to the versions the grid and slot builders were checked against (byte for
// byte against the Python builds) and verified by SHA-256 before anything is written. To move to
// a newer tool after a game update: download the new release, run the parity tests
// (`cargo test --release -p nms-launcher parity -- --ignored`), then change the tag and hashes here.
const MBINCOMPILER_TAG: &str = "v7.04.1-pre3";
const MBINCOMPILER_FILES: &[(&str, &str)] = &[
    ("MBINCompiler.exe", "4179dddb665f7cddbe9dddddf6e529172abdd98b0097f65fdd224467d5bb3ea4"),
    ("libMBIN.dll", "cc7c35d1bf111a7e01ec933b13b6f879eb95e621c0af2f229dbf492743652184"),
];
const HGPAKTOOL_TAG: &str = "1.1.3";
const HGPAKTOOL_ASSET: &str = "hgpaktool-x86_64-pc-windows.zip";
const HGPAKTOOL_ZIP_SHA256: &str = "1c3753e227ba79c7dab9b93777855c15532bc7eb3a2e9dc53daa6e8f6ce0c6fb";

pub struct Tools {
    pub mbincompiler: PathBuf,
    pub hgpaktool: PathBuf,
}

pub fn ensure_tools(loader: &Path, game_root: &Path, say: &mut dyn FnMut(&str)) -> Result<Tools, String> {
    let mut compiler = existing_tool(loader, game_root, "MBINCompiler.exe", "MBINCompiler");
    let mut paktool = existing_tool(loader, game_root, "hgpaktool.exe", "HGPAKtool");
    if compiler.is_none() {
        say("Downloading MBINCompiler for the UI merge...");
        compiler = Some(download_mbincompiler(loader)?);
    }
    if paktool.is_none() {
        say("Downloading HGPAKtool for the UI merge...");
        paktool = Some(download_hgpaktool(loader)?);
    }
    Ok(Tools {
        mbincompiler: compiler.expect("MBINCompiler"),
        hgpaktool: paktool.expect("HGPAKtool"),
    })
}

pub fn existing_tool(loader: &Path, game_root: &Path, name: &str, folder: &str) -> Option<PathBuf> {
    let nms_py = game_root.join("NMS.py").join("tools").join(folder).join(name);
    let dev = game_root.join("Modding").join("Tools").join(folder).join(name);
    for path in [
        loader.join("tools").join(folder).join(name),
        nms_py,
        dev,
    ] {
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn run<S: AsRef<OsStr>>(args: &[S]) -> Result<(), String> {
    let Some(first) = args.first() else {
        return Err("tool failed: empty command".into());
    };
    let tool_name = Path::new(first).file_name().and_then(OsStr::to_str).unwrap_or("tool");
    let mut cmd = nms_core::spawn::command(first);
    if args.len() > 1 {
        cmd.args(&args[1..]);
    }
    let result = cmd.output().map_err(|e| format!("{tool_name} failed: {e}"))?;
    if result.status.success() {
        return Ok(());
    }
    let stdout = tail_chars(&String::from_utf8_lossy(&result.stdout), 400);
    let stderr = tail_chars(&String::from_utf8_lossy(&result.stderr), 400);
    Err(format!("{tool_name} failed: {stdout} {stderr}"))
}

fn tail_chars(text: &str, n: usize) -> String {
    let count = text.chars().count();
    if count <= n {
        text.to_string()
    } else {
        text.chars().skip(count - n).collect()
    }
}

fn download(url: &str, timeout_secs: u64) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(timeout_secs))
        .call()
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Lowercase hex SHA-256 of `data`.
fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// A download is used only if it is the exact file we tested with.
fn verified(name: &str, data: Vec<u8>, want: &str) -> Result<Vec<u8>, String> {
    let got = sha256_hex(&data);
    if got == want {
        Ok(data)
    } else {
        Err(format!(
            "{name} does not match the version NMS.rs was tested with (sha256 {got}, expected {want}). Nothing was installed."
        ))
    }
}

fn release_url(repo: &str, tag: &str, asset: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{asset}")
}

fn download_mbincompiler(loader: &Path) -> Result<PathBuf, String> {
    let dest = loader.join("tools").join("MBINCompiler");
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    // Fetch and check both files before writing either.
    let mut files = Vec::new();
    for (name, want) in MBINCOMPILER_FILES {
        let url = release_url(MBINCOMPILER_REPO, MBINCOMPILER_TAG, name);
        files.push((*name, verified(name, download(&url, 300)?, want)?));
    }
    for (name, data) in files {
        fs::write(dest.join(name), data).map_err(|e| e.to_string())?;
    }
    Ok(dest.join("MBINCompiler.exe"))
}

fn download_hgpaktool(loader: &Path) -> Result<PathBuf, String> {
    let dest = loader.join("tools").join("HGPAKtool");
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let url = release_url(HGPAKTOOL_REPO, HGPAKTOOL_TAG, HGPAKTOOL_ASSET);
    let blob = verified(HGPAKTOOL_ASSET, download(&url, 300)?, HGPAKTOOL_ZIP_SHA256)?;
    let mut archive = ZipArchive::new(Cursor::new(blob)).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().map_err(|e| e.to_string())?.to_ascii_lowercase();
        if name.ends_with("hgpaktool.exe") {
            let mut out = Vec::new();
            file.read_to_end(&mut out).map_err(|e| e.to_string())?;
            let exe = dest.join("hgpaktool.exe");
            fs::write(&exe, out).map_err(|e| e.to_string())?;
            return Ok(exe);
        }
    }
    Err("hgpaktool.exe is not in the zip".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_known_value() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_wrong_download_is_refused() {
        let err = verified("x.exe", b"abc".to_vec(), &"0".repeat(64)).unwrap_err();
        assert!(err.contains("Nothing was installed"), "{err}");
        assert!(verified("x.exe", b"abc".to_vec(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_ok());
    }

    /// Network: downloads both pinned tools into a temp folder and checks them.
    /// `cargo test --release -p nms-launcher pinned_downloads -- --ignored`
    #[test]
    #[ignore]
    fn pinned_downloads_verify() {
        let loader = std::env::temp_dir().join(format!("nms-rs-pins-{}", std::process::id()));
        let _ = fs::remove_dir_all(&loader);
        let compiler = download_mbincompiler(&loader).expect("MBINCompiler download");
        let paktool = download_hgpaktool(&loader).expect("HGPAKtool download");
        assert!(compiler.is_file() && paktool.is_file());
        assert!(loader.join("tools").join("MBINCompiler").join("libMBIN.dll").is_file());
        let _ = fs::remove_dir_all(&loader);
    }

    #[test]
    fn pins_are_well_formed() {
        for (_, sha) in MBINCOMPILER_FILES {
            assert_eq!(sha.len(), 64);
            assert!(sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        }
        assert_eq!(HGPAKTOOL_ZIP_SHA256.len(), 64);
        assert_eq!(
            release_url("a/b", "v1", "f.zip"),
            "https://github.com/a/b/releases/download/v1/f.zip"
        );
    }
}
