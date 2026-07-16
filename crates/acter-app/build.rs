use tauri_build::{Attributes, try_build};

fn main() {
    let mut attributes = Attributes::new();

    // Tauri embeds the Windows application manifest (which declares the
    // Common-Controls v6 dependency) only into the main binary, via
    // `rustc-link-arg-bins`. Test executables link without it, so at startup the
    // ComCtl5->ComCtl6 stub reports STATUS_ENTRYPOINT_NOT_FOUND and the process
    // dies before any test runs. We embed the same manifest ourselves with
    // `rustc-link-arg` (no `-bins`), which covers tests too. See the T1 spec.
    #[cfg(windows)]
    {
        use tauri_build::WindowsAttributes;

        attributes = attributes.windows_attributes(WindowsAttributes::new_without_app_manifest());
        embed_app_manifest();
    }

    try_build(attributes).expect("failed to run the tauri build script");
}

#[cfg(windows)]
fn embed_app_manifest() {
    use std::env::var;
    use std::path::Path;

    let manifest = Path::new(&var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
