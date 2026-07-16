fn main() {
    let mut attributes = tauri_build::Attributes::new();

    // Tauri embeds the Windows application manifest (which declares the
    // Common-Controls v6 dependency) only into the main binary, via
    // `rustc-link-arg-bins`. Test executables link without it, so at startup the
    // ComCtl5->ComCtl6 stub reports STATUS_ENTRYPOINT_NOT_FOUND and the process
    // dies before any test runs. We embed the same manifest ourselves with
    // `rustc-link-arg` (no `-bins`), which covers tests too. See the T1 spec.
    #[cfg(windows)]
    {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        embed_app_manifest();
    }

    tauri_build::try_build(attributes).expect("failed to run the tauri build script");
}

#[cfg(windows)]
fn embed_app_manifest() {
    let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
