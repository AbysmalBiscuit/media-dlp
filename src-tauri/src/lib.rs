pub mod args;
pub mod binaries;
pub mod settings;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::save_settings,
            settings::config_path,
            binaries::browser_support,
            binaries::ytdlp_version,
            binaries::check_for_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
