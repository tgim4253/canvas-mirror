#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod commands;
pub mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = state::AppState::load(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_rooms,
            commands::create_room,
            commands::update_room,
            commands::delete_room,
            commands::set_room_running,
            commands::get_server_status,
            commands::get_server_settings,
            commands::update_server_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
