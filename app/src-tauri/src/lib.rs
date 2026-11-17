mod audio;
mod capture;
mod chat;
mod deepgram;
mod llm;
mod meetings;
mod migration;
mod reports;
mod secrets;
mod session;
mod settings;
mod storage;
mod suggestions;
mod transcript_files;

use audio::{AudioDevices, AudioManager, AudioPaths};
use llm::{LlmClient, ProviderTestResult};
use meetings::{Meeting, MeetingStore};
use serde::Deserialize;
use settings::{ProviderInput, ProviderKind, ProviderRole, SettingsPatch, SettingsStore, SettingsView};
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
fn meetings_list(meetings: State<'_, Arc<MeetingStore>>) -> Vec<Meeting> {
  meetings.list()
}

#[tauri::command]
fn migration_status(outcome: State<'_, migration::Outcome>) -> migration::Outcome {
  outcome.inner().clone()
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelsInput {
  id: Option<String>,
  endpoint: String,
  key: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderTestInput {
  id: Option<String>,
  kind: ProviderKind,
  endpoint: String,
  model: String,
  key: Option<String>,
  language: Option<String>,
}

async fn resolve_provider_key(store: Settings, id: Option<String>, typed_key: Option<String>) -> Result<Option<String>, String> {
  if let Some(k) = typed_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
    return Ok(Some(k));
  }
  if let Some(id) = id {
    return blocking(move || store.provider_key(&id)).await;
  }
  Ok(None)
}

#[tauri::command]
async fn provider_models(
  store: State<'_, Settings>,
  llm: State<'_, LlmClient>,
  input: ProviderModelsInput,
) -> Result<Vec<String>, String> {
  let key = resolve_provider_key(store.inner().clone(), input.id, input.key).await?;
  llm.list_models(&input.endpoint, key.as_deref()).await
}

#[tauri::command]
async fn provider_test(
  store: State<'_, Settings>,
  llm: State<'_, LlmClient>,
  input: ProviderTestInput,
) -> Result<ProviderTestResult, String> {
  let key = resolve_provider_key(store.inner().clone(), input.id, input.key).await?;
  match input.kind {
    ProviderKind::Deepgram => {
      let Some(api_key) = key else {
        return Err("Deepgram needs an API key.".into());
      };
      let lang = input.language.unwrap_or_else(|| store.get().audio_language);
      Ok(deepgram::test_connection(&input.endpoint, &api_key, &input.model, &lang).await)
    }
    ProviderKind::OpenaiCompatible => Ok(llm.test_provider(&input.endpoint, key.as_deref(), &input.model).await),
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .manage(AudioManager::new())
    .manage(LlmClient::new())
    .setup(|app| {
      let dir = app.path().app_data_dir().map_err(|e| format!("Could not locate app data directory: {e}"))?;
      storage::ensure_private_dir(&dir)?;
      let store: Settings = Arc::new(SettingsStore::open(&dir, Arc::new(secrets::OsKeychain))?);
      let meetings = Arc::new(MeetingStore::open(&dir)?);
      let config = app.path().config_dir().map_err(|e| format!("Could not locate config directory: {e}"))?;
      let outcome = migration::run(&migration::legacy_dir(&config), &dir, &store, &meetings);
      if let migration::Outcome::Failed { error } = &outcome {
        eprintln!("[migration] {error}");
      }
      app.manage(store);
      app.manage(meetings);
      app.manage(outcome);
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      audio_devices,
      audio_start,
      audio_stop,
      meetings_list,
      migration_status,
      settings_get,
      settings_update,
      provider_save,
      provider_remove,
      provider_select,
      provider_models,
      provider_test
    ])
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
