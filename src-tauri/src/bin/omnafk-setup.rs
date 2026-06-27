#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    omnafk::setup::handoff_uninstaller_if_needed();
    omnafk::setup::run();
}
