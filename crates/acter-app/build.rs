use tauri_build::{Attributes, try_build};
use vergen_gitcl::{Emitter, Gitcl};

fn main() {
    stamp_the_version();
    try_build(attributes()).expect("failed to run the tauri build script");
}

/// Describe and commit are separate passes, describe first, because `git describe` fails in a
/// repository with no tags and a failed pass must not take the commit with it.
///
/// A `VERGEN_GIT_DESCRIBE` in the environment wins over git; the release workflow sets it to the
/// platform tag being built, since two platform tags sit on one commit.
fn stamp_the_version() {
    println!("cargo:rerun-if-env-changed=VERGEN_GIT_DESCRIBE");
    println!("cargo:rerun-if-env-changed=VERGEN_GIT_SHA");
    let describe = Gitcl::builder().describe(true, false, None).build();
    let commit = Gitcl::builder().sha(true).build();
    let mut emitter = Emitter::default();
    let stamped = (|| {
        emitter.add_instructions(&describe)?;
        emitter.add_instructions(&commit)?;
        emitter.emit()
    })();
    if let Err(why) = stamped {
        println!("cargo:warning=Acter could not stamp its version from git: {why}");
    }
}

/// Tauri embeds the Windows app manifest (Common-Controls v6) only into the main binary, and a
/// test executable linked without it dies at startup with STATUS_ENTRYPOINT_NOT_FOUND, so the
/// manifest is embedded with `rustc-link-arg`, which covers tests too.
#[cfg(windows)]
fn attributes() -> Attributes {
    use tauri_build::WindowsAttributes;

    embed_app_manifest();
    Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest())
}

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
