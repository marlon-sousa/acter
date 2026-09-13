use tauri_build::{Attributes, try_build};
use vergen_gitcl::{Emitter, Gitcl};

fn main() {
    stamp_the_version();
    try_build(attributes()).expect("failed to run the tauri build script");
}

/// What this build is, stamped into the binary so the About dialog can say it (spec 26,
/// decision 3).
///
/// **Two facts and no more**: the describe string and the short commit. A pure function in
/// `container.rs` turns them into the identifier a bug report carries and the sentence
/// About speaks, because those strings are read aloud and a build script's output cannot be
/// unit tested.
///
/// **Two passes rather than one**, and this is the reason: `git describe` fails outright in
/// a repository with no tags, and one `add_instructions` that asked for both would lose the
/// commit with it — so today's Acter, which has no tags, would report the Cargo version
/// instead of the development build it is. Describe goes first, because a failing pass
/// clears the rerun instructions collected so far and the pass after it puts them back.
///
/// **Nothing is emitted when git cannot answer**, which is deliberate: neither
/// `idempotent` nor `default_on_error` is enabled, so a directory that is not a repository
/// stamps no variables at all and `option_env!` answers `None` — which is the third rule of
/// decision 3, a source tarball saying what Cargo says.
///
/// **An environment variable of the same name wins**, which is what the release workflow
/// sets from `GITHUB_REF_NAME` (decision 21). With a tag per platform, `windows-v1.0.0` and
/// `macos-v1.0.0` sit on the same commit, and `git describe` would pick one of them by its
/// own rules rather than the one being built.
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
        // Not a failure worth stopping a build over: a tree with no git still builds, and
        // what it costs is the About dialog saying the Cargo version instead.
        println!("cargo:warning=Acter could not stamp its version from git: {why}");
    }
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
