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
mod shortcuts;
mod storage;
mod suggestions;
mod transcript_files;
mod tray;
mod windows;

use audio::AudioDevices;
use chat::{ChatEvent, ChatMessage, ChatService, ScreenshotResult};
use llm::{LlmClient, ProviderTestResult};
use meetings::{Meeting, MeetingStore, TranscriptSegment};
use reports::ReportGenerator;
use serde::Deserialize;
use session::{SessionEvent, SessionManager, SessionState, SessionStatus, Utterance};
use settings::{ProviderInput, ProviderKind, ProviderRole, SettingsPatch, SettingsStore, SettingsView};
use shortcuts::ShortcutManager;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use suggestions::{SuggestionService, SuggestionsState};
use transcript_files::{default_transcript_dir, write_transcript_file};
use windows::{PanelTab, WindowKind, WindowManager, WindowsState};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};

type Settings = Arc<SettingsStore>;

struct AppPaths {
    data_dir: PathBuf,
    last_transcript: Mutex<Option<PathBuf>>,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FitSize {
    width: f64,
    height: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DragStep {
    phase: String,
    dx: f64,
    dy: f64,
}

fn changed(app: &AppHandle, store: &SettingsStore) -> SettingsView {
    let view = store.view();
    let _ = app.emit("settings:changed", &view);
    view
}

fn sync_after_settings(app: &AppHandle) {
    let store = app.state::<Settings>();
    let windows = app.state::<Arc<WindowManager>>();
    let session = app.state::<Arc<SessionManager>>();
    let shortcuts = app.state::<Arc<ShortcutManager>>();

    let s = store.get();
    windows.set_stealth(app, s.stealth);
    windows.set_suggestions_rail(app, s.suggestions && session.state().status != SessionStatus::Idle);
    shortcuts.register_all(app);
    session.sync_engine();

    let view = store.view();
    let _ = app.emit("settings:changed", &view);
    let _ = app.emit("session:state", session.state());
}

/// Keychain access can block (D-Bus, unlock prompts), so it runs off the main thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("Background task failed: {e}"))?
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
fn audio_devices() -> Result<AudioDevices, String> {
    audio::devices()
}

#[tauri::command]
fn meetings_list(meetings: State<'_, Arc<MeetingStore>>) -> Vec<Meeting> {
    meetings.list()
}

#[tauri::command]
fn meetings_get(meetings: State<'_, Arc<MeetingStore>>, id: String) -> Option<Meeting> {
    meetings.get(&id)
}

#[tauri::command]
fn meetings_delete(app: AppHandle, meetings: State<'_, Arc<MeetingStore>>, id: String) -> Result<bool, String> {
    let deleted = meetings.delete(&id)?;
    if deleted {
        let _ = app.emit("meetings:changed", ());
    }
    Ok(deleted)
}

#[tauri::command]
fn meetings_export(
    meetings: State<'_, Arc<MeetingStore>>,
    session: State<'_, Arc<SessionManager>>,
    id: String,
    reveal: Option<bool>,
) -> Result<String, String> {
    let meeting = meetings.get(&id).ok_or_else(|| "Meeting not found.".to_string())?;
    let dir = session.transcript_dir();
    let path = write_transcript_file(&dir, &meeting, None)?;
    if reveal.unwrap_or(true) {
        tray::open_in_file_manager(&path);
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn reports_generate(reports: State<'_, Arc<ReportGenerator>>, id: String) -> Result<Option<Meeting>, String> {
    reports.generate(&id).await
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
    sync_after_settings(&app);
    Ok(store.view())
}

#[tauri::command]
async fn provider_save(app: AppHandle, store: State<'_, Settings>, input: ProviderInput) -> Result<SettingsView, String> {
    let store_arc = store.inner().clone();
    let worker = store_arc.clone();
    blocking(move || worker.save_provider(input)).await?;
    sync_after_settings(&app);
    Ok(store_arc.view())
}

#[tauri::command]
async fn provider_remove(app: AppHandle, store: State<'_, Settings>, id: String) -> Result<SettingsView, String> {
    let store_arc = store.inner().clone();
    let worker = store_arc.clone();
    let result = blocking(move || worker.remove_provider(&id)).await;
    sync_after_settings(&app);
    result.map(|()| store_arc.view())
}

#[tauri::command]
fn provider_select(
    app: AppHandle,
    store: State<'_, Settings>,
    role: ProviderRole,
    id: Option<String>,
) -> Result<SettingsView, String> {
    store.select_provider(role, id)?;
    sync_after_settings(&app);
    Ok(store.view())
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

#[tauri::command]
fn session_state(session: State<'_, Arc<SessionManager>>) -> SessionState {
    session.state()
}

#[tauri::command]
async fn session_start(session: State<'_, Arc<SessionManager>>) -> Result<SessionState, String> {
    session.start().await
}

#[tauri::command]
fn session_pause(session: State<'_, Arc<SessionManager>>) -> SessionState {
    session.pause()
}

#[tauri::command]
fn session_resume(session: State<'_, Arc<SessionManager>>) -> SessionState {
    session.resume()
}

#[tauri::command]
fn session_finish(
    session: State<'_, Arc<SessionManager>>,
    reports: State<'_, Arc<ReportGenerator>>,
    paths: State<'_, AppPaths>,
) -> Result<Option<Meeting>, String> {
    let finished = session.finish()?;
    if let Some(ref meeting) = finished {
        let file = transcript_files::transcript_file_name(meeting.start_time, &meeting.title);
        *paths.last_transcript.lock().unwrap_or_else(|e| e.into_inner()) = Some(session.transcript_dir().join(file));
        let reports = reports.inner().clone();
        let id = meeting.id.clone();
        tauri::async_runtime::spawn(async move {
            let _ = reports.generate(&id).await;
        });
    }
    Ok(finished)
}

#[tauri::command]
fn session_cancel(session: State<'_, Arc<SessionManager>>) -> SessionState {
    session.cancel()
}

#[tauri::command]
fn session_transcript(session: State<'_, Arc<SessionManager>>) -> String {
    session.transcript_text()
}

#[tauri::command]
fn session_segments(session: State<'_, Arc<SessionManager>>) -> Vec<TranscriptSegment> {
    session.segments()
}

#[tauri::command]
fn session_last_utterance(session: State<'_, Arc<SessionManager>>) -> Option<Utterance> {
    session.last_utterance()
}

#[tauri::command]
fn chat_history(chat: State<'_, Arc<ChatService>>) -> Vec<ChatMessage> {
    chat.history()
}

#[tauri::command]
fn chat_send(chat: State<'_, Arc<ChatService>>, text: String) {
    let chat = chat.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = chat.send(text, None).await;
    });
}

#[tauri::command]
async fn chat_answer_last(chat: State<'_, Arc<ChatService>>) -> Result<bool, String> {
    chat.answer_last().await
}

#[tauri::command]
fn chat_clear(chat: State<'_, Arc<ChatService>>) {
    chat.clear();
}

#[tauri::command]
fn chat_cancel(chat: State<'_, Arc<ChatService>>) -> bool {
    chat.cancel()
}

#[tauri::command]
async fn chat_screenshot(chat: State<'_, Arc<ChatService>>, prompt: Option<String>) -> Result<ScreenshotResult, String> {
    chat.screenshot(prompt).await
}

#[tauri::command]
fn suggestions_state(suggestions: State<'_, Arc<SuggestionService>>) -> SuggestionsState {
    suggestions.state()
}

#[tauri::command]
fn suggestions_refresh(suggestions: State<'_, Arc<SuggestionService>>) {
    let suggestions = suggestions.inner().clone();
    tauri::async_runtime::spawn(async move {
        suggestions.refresh().await;
    });
}

#[tauri::command]
fn suggestions_use(app: AppHandle, windows: State<'_, Arc<WindowManager>>, suggestions: State<'_, Arc<SuggestionService>>, id: String) {
    if suggestions.use_item(&id) {
        windows.show_panel(&app, PanelTab::Answers, false);
    }
}

#[tauri::command]
fn windows_state(app: AppHandle, windows: State<'_, Arc<WindowManager>>) -> WindowsState {
    windows.state(&app)
}

#[tauri::command]
fn windows_show(app: AppHandle, windows: State<'_, Arc<WindowManager>>, kind: WindowKind) {
    windows.show(&app, kind, false);
}

#[tauri::command]
fn windows_hide(app: AppHandle, windows: State<'_, Arc<WindowManager>>, kind: WindowKind) {
    windows.hide(&app, kind);
}

#[tauri::command]
fn windows_toggle(app: AppHandle, windows: State<'_, Arc<WindowManager>>, kind: WindowKind) -> bool {
    windows.toggle(&app, kind)
}

#[tauri::command]
fn windows_toggle_all(app: AppHandle, windows: State<'_, Arc<WindowManager>>) {
    windows.toggle_all(&app);
}

#[tauri::command]
fn panel_show(app: AppHandle, windows: State<'_, Arc<WindowManager>>, tab: PanelTab) {
    windows.show_panel(&app, tab, false);
}

#[tauri::command]
fn panel_toggle(app: AppHandle, windows: State<'_, Arc<WindowManager>>, tab: PanelTab) {
    windows.toggle_panel(&app, tab);
}

#[tauri::command]
fn panel_set_tab(app: AppHandle, windows: State<'_, Arc<WindowManager>>, tab: PanelTab) {
    windows.set_panel_tab(&app, tab);
}

#[tauri::command]
fn windows_fit(app: AppHandle, window: WebviewWindow, windows: State<'_, Arc<WindowManager>>, size: FitSize) {
    windows.fit(&app, window.label(), size.width, size.height);
}

#[tauri::command]
fn windows_drag(app: AppHandle, windows: State<'_, Arc<WindowManager>>, step: DragStep) {
    if step.dx.is_finite() && step.dy.is_finite() && step.dx.abs() < 20_000.0 && step.dy.abs() < 20_000.0 {
        windows.drag_main(&app, &step.phase, step.dx, step.dy);
    }
}

#[tauri::command]
fn transcripts_reveal(session: State<'_, Arc<SessionManager>>, paths: State<'_, AppPaths>) {
    let dir = session.transcript_dir();
    let _ = std::fs::create_dir_all(&dir);
    let last = paths.last_transcript.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(file) = last.filter(|p| p.exists()) {
        tray::open_in_file_manager(file.parent().unwrap_or(&dir));
    } else {
        tray::open_in_file_manager(&dir);
    }
}

#[tauri::command]
fn app_open_data_dir(paths: State<'_, AppPaths>) {
    tray::open_in_file_manager(&paths.data_dir);
}

#[tauri::command]
fn app_quit(app: AppHandle, windows: State<'_, Arc<WindowManager>>, session: State<'_, Arc<SessionManager>>) {
    windows.mark_quitting();
    let _ = session.finish();
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(LlmClient::new())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if let Some(mgr) = app.try_state::<Arc<ShortcutManager>>() {
                        mgr.handle_event(app, shortcut, event.state);
                    }
                })
                .build(),
        )
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && let Some(windows) = window.app_handle().try_state::<Arc<WindowManager>>()
                && !windows.is_quitting()
            {
                api.prevent_close();
                windows.hide_all(window.app_handle());
            }
        })
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

            let windows = Arc::new(WindowManager::new(store.get().stealth));
            let transcript_dir = std::env::var_os("MEETINGLY_TRANSCRIPT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(default_transcript_dir);

            let app_handle = app.handle().clone();
            let windows_session = windows.clone();
            let settings_session = store.clone();
            let session = SessionManager::new(store.clone(), meetings.clone(), transcript_dir.clone(), {
                let app_handle = app_handle.clone();
                move |ev| match ev {
                    SessionEvent::State(state) => {
                        windows_session.set_suggestions_rail(
                            &app_handle,
                            settings_session.get().suggestions && state.status != SessionStatus::Idle,
                        );
                        if state.status != SessionStatus::Idle {
                            windows_session.ensure_chat(&app_handle);
                        } else if let Some(s) = app_handle.try_state::<Arc<SuggestionService>>() {
                            s.reset();
                        }
                        let _ = app_handle.emit("session:state", &state);
                    }
                    SessionEvent::Transcript(t) => {
                        let _ = app_handle.emit("session:transcript", &t);
                    }
                    SessionEvent::Utterance(u) => {
                        let _ = app_handle.emit("session:utterance", &u);
                        if let Some(chat) = app_handle.try_state::<Arc<ChatService>>() {
                            let chat = chat.inner().clone();
                            let u_clone = u.clone();
                            tauri::async_runtime::spawn(async move {
                                chat.auto_answer(u_clone).await;
                            });
                        }
                        if let Some(s) = app_handle.try_state::<Arc<SuggestionService>>() {
                            s.inner().clone().on_utterance();
                        }
                    }
                    SessionEvent::Levels(levels) => {
                        let _ = app_handle.emit("audio-levels", &levels);
                    }
                    SessionEvent::Finished(meeting) => {
                        let _ = &meeting.id;
                        let _ = app_handle.emit("meetings:changed", ());
                    }
                    SessionEvent::Discarded => {}
                }
            });

            let llm = app.state::<LlmClient>().inner().clone();
            let backend: Arc<dyn chat::BrainBackend> = Arc::new(llm);

            let chat = ChatService::new(
                store.clone(),
                session.clone(),
                backend.clone(),
                Arc::new(|| Box::pin(capture::capture_primary_display())),
                {
                    let app_handle = app.handle().clone();
                    let windows = windows.clone();
                    move || windows.hide_for_capture(&app_handle)
                },
                {
                    let app_handle = app.handle().clone();
                    let windows = windows.clone();
                    move |ev| match ev {
                        ChatEvent::Message(msg) => {
                            let _ = app_handle.emit("chat:message", &msg);
                        }
                        ChatEvent::Chunk(chunk) => {
                            let _ = app_handle.emit("chat:chunk", &chunk);
                        }
                        ChatEvent::Busy(busy) => {
                            let _ = app_handle.emit("chat:busy", busy);
                        }
                        ChatEvent::Cleared => {
                            let _ = app_handle.emit("chat:cleared", ());
                        }
                        ChatEvent::AutoAnswer(msg) => {
                            let _ = &msg.id;
                            windows.show_panel(&app_handle, PanelTab::Answers, true);
                        }
                    }
                },
            );

            let suggestions = SuggestionService::new(store.clone(), session.clone(), chat.clone(), backend.clone(), {
                let app_handle = app.handle().clone();
                move |state| {
                    let _ = app_handle.emit("suggestions:state", &state);
                }
            });

            let reports = ReportGenerator::new(store.clone(), meetings.clone(), chat.clone(), backend, {
                let app_handle = app.handle().clone();
                move || {
                    let _ = app_handle.emit("meetings:changed", ());
                }
            });

            let shortcuts = ShortcutManager::new(store.clone(), windows.clone(), session.clone(), chat.clone());
            shortcuts.register_all(app.handle());
            windows.init_main(app.handle());
            let _ = tray::setup_tray(app.handle(), windows.clone(), transcript_dir, dir.clone());

            let reports_resume = reports.clone();
            tauri::async_runtime::spawn(async move {
                reports_resume.resume_unfinished().await;
            });

            let session_autosave = session.clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                interval.tick().await;
                loop {
                    interval.tick().await;
                    session_autosave.save_autosave();
                }
            });

            app.manage(store.clone());
            app.manage(meetings);
            app.manage(outcome);
            app.manage(windows);
            app.manage(session);
            app.manage(chat);
            app.manage(suggestions);
            app.manage(reports);
            app.manage(shortcuts);
            app.manage(AppPaths {
                data_dir: dir,
                last_transcript: Mutex::new(None),
            });
            changed(app.handle(), &store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            audio_devices,
            meetings_list,
            meetings_get,
            meetings_delete,
            meetings_export,
            reports_generate,
            migration_status,
            settings_get,
            settings_update,
            provider_save,
            provider_remove,
            provider_select,
            provider_models,
            provider_test,
            session_state,
            session_start,
            session_pause,
            session_resume,
            session_finish,
            session_cancel,
            session_transcript,
            session_segments,
            session_last_utterance,
            chat_history,
            chat_send,
            chat_answer_last,
            chat_clear,
            chat_cancel,
            chat_screenshot,
            suggestions_state,
            suggestions_refresh,
            suggestions_use,
            windows_state,
            windows_show,
            windows_hide,
            windows_toggle,
            windows_toggle_all,
            panel_show,
            panel_toggle,
            panel_set_tab,
            windows_fit,
            windows_drag,
            transcripts_reveal,
            app_open_data_dir,
            app_quit
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
