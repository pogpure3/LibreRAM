// SPDX-License-Identifier: GPL-2.0-only
fn main() {
    let mut attributes = tauri_build::Attributes::new();
    // Release installs always run elevated; debug/test builds stay launchable
    // from a non-admin shell (cargo test hits os error 740 otherwise).
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        let manifest = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
    <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
  </requestedPrivileges></security></trustInfo>
</assembly>"#;
        attributes = attributes.windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(manifest));
    }
    tauri_build::try_build(attributes).expect("tauri build failed");
}
