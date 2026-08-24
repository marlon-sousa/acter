//! Router: which operating system this build is running on.
//!
//! **The frontend needs it to decide where the menu bar lives** (spec A7). On Windows the
//! menu bar is in the document, because a native one freezes NVDA for tens of seconds
//! every time it opens; on macOS a menu belongs in the system bar and not in the window at
//! all, and Linux is likely to want its own answer. That is a platform decision, and the
//! platform is a fact the compiler already knows.

#[tauri::command]
pub(crate) fn platform() -> &'static str {
    std::env::consts::OS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compiled in, so this asserts the build rather than the machine: what matters is
    /// that the frontend's Windows branch is reachable from a Windows build.
    #[test]
    fn the_platform_is_the_one_this_was_built_for() {
        assert_eq!(platform(), std::env::consts::OS);
        #[cfg(windows)]
        assert_eq!(platform(), "windows");
    }
}
