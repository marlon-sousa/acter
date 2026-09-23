//! Router: which operating system this build is running on.

#[tauri::command]
pub(crate) fn platform() -> &'static str {
    std::env::consts::OS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_platform_is_the_one_this_was_built_for() {
        assert_eq!(platform(), std::env::consts::OS);
        #[cfg(windows)]
        assert_eq!(platform(), "windows");
    }
}
