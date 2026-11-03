//! Embeds version details and a manifest in the Windows executable. Explorer shows the details,
//! and the manifest states that Cairn needs no admin rights.

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
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "Cairn")
        .set("FileDescription", "Cairn drop shelf")
        .set("CompanyName", "Mahmoud Alnakeeb")
        .set("LegalCopyright", "Copyright (C) 2026 Mahmoud Alnakeeb")
        .set("InternalName", "cairn")
        .set("OriginalFilename", "cairn.exe")
        .set_manifest(MANIFEST);
    if let Err(e) = res.compile() {
        // MSVC builds (CI, releases) must have the details. The GNU toolchain may lack windres,
        // so it just warns.
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            panic!("embedding the version details failed: {e}");
        }
        println!("cargo:warning=version details not embedded: {e}");
    }
}
