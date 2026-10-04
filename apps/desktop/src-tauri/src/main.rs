//! graphicgene on the desktop: the web app in a Tauri webview.
//!
//! The page is the build the web gets, core and all — the wasm runs in the
//! webview. Only storage differs: the page asks for files through the
//! commands in `files`, where a browser would use IndexedDB and downloads.

// No console window behind the app on Windows, except in debug builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod files;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            files::read_autosave,
            files::write_autosave,
            files::open_project,
            files::save_file,
        ])
        .run(tauri::generate_context!())
        .expect("graphicgene could not start");
}
