//! Controller: desktop entry point; delegates immediately to the composition root.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use acter_app::run;

fn main() {
    run();
}
