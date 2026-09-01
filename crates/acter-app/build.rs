use tauri_build::{Attributes, try_build};

fn main() {
    try_build(attributes()).expect("failed to run the tauri build script");
}

/// The build attributes this platform needs.
///
/// **Two functions rather than one with a gated block inside it** (M1), which is
/// ARCHITECTURE's platform-divergence rule in its middle form. The gated block left
/// `attributes` declared `mut` and never mutated off Windows, so every non-Windows build
/// carried an `unused_mut` warning — and CI runs clippy with `-D warnings`.
///
/// Tauri embeds the Windows application manifest (which declares the Common-Controls v6
/// dependency) only into the main binary, via `rustc-link-arg-bins`. Test executables link
/// without it, so at startup the ComCtl5->ComCtl6 stub reports STATUS_ENTRYPOINT_NOT_FOUND
/// and the process dies before any test runs. We embed the same manifest ourselves with
/// `rustc-link-arg` (no `-bins`), which covers tests too. See the T1 spec.
#[cfg(windows)]
fn attributes() -> Attributes {
    use tauri_build::WindowsAttributes;

    embed_app_manifest();
    Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest())
}

/// **Nothing to add anywhere else.** There is no manifest to embed and no linker argument to
/// pass: the whole of the block above is one Windows loader problem, and a platform without
/// that loader builds with the attributes Tauri ships.
#[cfg(not(windows))]
fn attributes() -> Attributes {
    Attributes::new()
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
