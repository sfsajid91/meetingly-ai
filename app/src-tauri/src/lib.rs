mod audio;
mod secrets;
mod settings;
mod storage;

use audio::{AudioDevices, AudioManager, AudioPaths};
use settings::{ProviderInput, ProviderRole, SettingsPatch, SettingsStore, SettingsView};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

type Settings = Arc<SettingsStore>;

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

fn changed(app: &AppHandle, store: &SettingsStore) -> SettingsView {
  let view = store.view();
  let _ = app.emit("settings-changed", &view);
  view
}

/// Keychain access can block (D-Bus, unlock prompts), so it runs off the main thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
  tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("Background task failed: {e}"))?
}

#[tauri::command]
fn settings_get(store: State<'_, Settings>) -> SettingsView {
  store.view()
}

#[tauri::command]
fn settings_update(app: AppHandle, store: State<'_, Settings>, patch: SettingsPatch) -> Result<SettingsView, String> {
  store.update(patch)?;
  Ok(changed(&app, &store))
}

#[tauri::command]
async fn provider_save(app: AppHandle, store: State<'_, Settings>, input: ProviderInput) -> Result<SettingsView, String> {
  let store = store.inner().clone();
  let worker = store.clone();
  blocking(move || worker.save_provider(input)).await?;
  Ok(changed(&app, &store))
}

#[tauri::command]
async fn provider_remove(app: AppHandle, store: State<'_, Settings>, id: String) -> Result<SettingsView, String> {
  let store = store.inner().clone();
  let worker = store.clone();
  let result = blocking(move || worker.remove_provider(&id)).await;
  // The record may be gone even when the keychain delete failed; refresh the UI either way.
  let view = changed(&app, &store);
  result.map(|()| view)
}

#[tauri::command]
fn provider_select(
  app: AppHandle,
  store: State<'_, Settings>,
  role: ProviderRole,
  id: Option<String>,
) -> Result<SettingsView, String> {
  store.select_provider(role, id)?;
  Ok(changed(&app, &store))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .manage(AudioManager::new())
    .setup(|app| {
      let dir = app.path().app_data_dir().map_err(|e| format!("Could not locate app data directory: {e}"))?;
      storage::ensure_private_dir(&dir)?;
      let store: Settings = Arc::new(SettingsStore::open(&dir, Arc::new(secrets::OsKeychain))?);
      app.manage(store);
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      audio_devices,
      audio_start,
      audio_stop,
      settings_get,
      settings_update,
      provider_save,
      provider_remove,
      provider_select
    ])
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
