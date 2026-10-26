// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn is_native_host_mode(args: &[String]) -> bool {
    if args.iter().any(|a| a == "--native-host") {
        return true;
    }
    if args.len() >= 2 {
        let first = &args[1];
        // Chromium browsers pass origin: 'chrome-extension://<id>/'
        if first.starts_with("chrome-extension://") || first.starts_with("moz-extension://") {
            return true;
        }
        // Firefox passes path to manifest or extension id
        if first.ends_with(".json") && args.len() >= 3 && args[2].contains('@') {
            return true;
        }
    }
    false
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if is_native_host_mode(&args) {
        impressive_download_manager_lib::native_host::run_native_host();
        return;
    }

    impressive_download_manager_lib::run()
}

