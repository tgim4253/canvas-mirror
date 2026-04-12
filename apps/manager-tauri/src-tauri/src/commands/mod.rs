use tauri::State;

use canvas_mirror_model::ServerStatusDto;

use crate::state::{
    AppState, CreateRoomInput, ManagedRoomDto, ServerSettingsDto, UpdateRoomInput,
    UpdateServerSettingsInput,
};

#[tauri::command]
pub fn list_rooms(state: State<'_, AppState>) -> Result<Vec<ManagedRoomDto>, String> {
    state
        .runtime()
        .list_rooms()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_room(
    state: State<'_, AppState>,
    input: CreateRoomInput,
) -> Result<ManagedRoomDto, String> {
    state
        .runtime()
        .create_room(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_room(
    state: State<'_, AppState>,
    room_id: String,
    input: UpdateRoomInput,
) -> Result<ManagedRoomDto, String> {
    state
        .runtime()
        .update_room(&room_id, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_room(state: State<'_, AppState>, room_id: String) -> Result<ManagedRoomDto, String> {
    state
        .runtime()
        .delete_room(&room_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_room_running(
    state: State<'_, AppState>,
    room_id: String,
    running: bool,
) -> Result<ManagedRoomDto, String> {
    state
        .runtime()
        .set_room_running(&room_id, running)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_server_status(state: State<'_, AppState>) -> Result<ServerStatusDto, String> {
    Ok(state.runtime().server_status())
}

#[tauri::command]
pub fn get_server_settings(state: State<'_, AppState>) -> Result<ServerSettingsDto, String> {
    Ok(state.runtime().server_settings())
}

#[tauri::command]
pub fn update_server_settings(
    state: State<'_, AppState>,
    input: UpdateServerSettingsInput,
) -> Result<ServerSettingsDto, String> {
    state
        .runtime()
        .update_server_settings(input)
        .map_err(|error| error.to_string())
}
