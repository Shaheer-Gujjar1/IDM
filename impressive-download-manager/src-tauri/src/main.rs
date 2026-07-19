// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Force X11/XWayland backend on Linux to bypass WebKitGTK/Wayland window decoration bugs.
        // This resolves the issue where titlebar buttons (close, minimize, maximize) are frozen
        // on startup under Wayland sessions.
        std::env::set_var("GDK_BACKEND", "x11");
    }
    impressive_download_manager_lib::run()
}
