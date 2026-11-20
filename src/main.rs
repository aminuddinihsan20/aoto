#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    // Child-process modes for the webview windows (see login.rs / turnstile.rs).
    if aoto::run_child_window() {
        return Ok(());
    }
    aoto::run_with(|_| {})
}
