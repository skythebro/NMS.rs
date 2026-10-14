//! Windows version information and a manifest that says it never needs administrator rights, so the file shows what it is in Explorer
//! and to virus scanners. `NMS_RS_VERSION` (for example 4.0.0) overrides the crate version.

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-env-changed=NMS_RS_VERSION");
    println!("cargo:rerun-if-changed=assets/nms-rs.ico");
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
        res.set("FileDescription", "NMS.rs mod manager and tools for the NMS game");
        res.set("InternalName", "nms-rs");
        res.set("OriginalFilename", "nms-rs.exe");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        res.set_manifest(MANIFEST);
        // Explorer, the taskbar and Alt+Tab show this badge (made by dev/make_app_icon.py).
        res.set_icon("assets/nms-rs.ico");
        if let Err(err) = res.compile() {
            println!("cargo:warning=version information not embedded: {err}");
        }
    }
}
