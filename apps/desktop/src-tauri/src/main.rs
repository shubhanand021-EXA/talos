//! Thin binary wrapper: all logic lives in the library for testability.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    talos_desktop_lib::run();
}
