mod audio;

use audio::{AudioDevices, AudioManager, AudioPaths};
use tauri::{AppHandle, Manager, State};

#[tauri::command]
fn audio_devices() -> Result<AudioDevices, String> {
  audio::devices()
}

#[tauri::command]
fn audio_start(
  app: AppHandle,
  state: State<'_, AudioManager>,
  microphone: String,
  monitor: String,
) -> Result<(), String> {
  audio::start(app, &state, microphone, monitor)
}

#[tauri::command]
fn audio_stop(app: AppHandle, state: State<'_, AudioManager>) -> Result<AudioPaths, String> {
  let app_data = app.path().app_data_dir().map_err(|e| format!("Could not locate app data directory: {e}"));
  audio::stop(&state, app_data)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .manage(AudioManager::new())
    .invoke_handler(tauri::generate_handler![audio_devices, audio_start, audio_stop])
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
