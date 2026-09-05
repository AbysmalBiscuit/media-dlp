pub mod args;
pub mod binaries;
pub mod job;
pub mod settings;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                if let Ok(dir) = tauri::Manager::path(&handle).app_config_dir() {
                    let channel = settings::load(&dir).update_channel;
                    let _ = binaries::check_for_updates(handle, channel);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::save_settings,
            settings::config_path,
            binaries::browser_support,
            binaries::ytdlp_version,
            binaries::check_for_updates,
            job::probe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
