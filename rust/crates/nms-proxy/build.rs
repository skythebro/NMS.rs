//! Windows version information, so the file shows what it is in Explorer
//! and to virus scanners. `NMS_RS_VERSION` (for example 4.0.0) overrides the crate version.

fn main() {
    println!("cargo:rerun-if-env-changed=NMS_RS_VERSION");
    #[cfg(windows)]
    {
        let version = std::env::var("NMS_RS_VERSION")
            .unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()));
        let mut parts = version.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
        let (major, minor, patch) = (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        );
        let packed = (major << 48) | (minor << 32) | (patch << 16);
        let mut res = winresource::WindowsResource::new();
        res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
        res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        res.set("ProductName", "NMS.rs");
        res.set("FileDescription", "NMS.rs mod loader (XInput proxy) for the NMS game");
        res.set("InternalName", "xinput9_1_0");
        res.set("OriginalFilename", "xinput9_1_0.dll");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        if let Err(err) = res.compile() {
            println!("cargo:warning=version information not embedded: {err}");
        }
    }
}
