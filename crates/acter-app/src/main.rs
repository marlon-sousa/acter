//! Controller: desktop entry point; delegates immediately to the composition root.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    acter_app::run();
}
