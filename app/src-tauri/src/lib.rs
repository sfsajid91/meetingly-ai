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
use shortcuts::{ActionId, ShortcutService, ShortcutStatusView};
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

fn changed(app: &AppHandle, store: &SettingsStore) -> SettingsView {
    let view = store.view();
    let _ = app.emit("settings:changed", &view);
    view
}

fn sync_after_settings(app: &AppHandle, shortcuts_changed: bool) {
    let store = app.state::<Settings>();
    let windows = app.state::<Arc<WindowManager>>();
    let session = app.state::<Arc<SessionManager>>();
    let shortcuts = app.state::<Arc<ShortcutService>>();

    let s = store.get();
    windows.set_stealth(app, s.stealth);
    if shortcuts_changed {
        shortcuts.sync_from_settings(app);
    }
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
    let shortcuts_changed = patch.shortcuts.is_some();
    store.update(patch)?;
    sync_after_settings(&app, shortcuts_changed);
    Ok(store.view())
}

#[tauri::command]
async fn provider_save(app: AppHandle, store: State<'_, Settings>, input: ProviderInput) -> Result<SettingsView, String> {
    let store_arc = store.inner().clone();
    let worker = store_arc.clone();
    blocking(move || worker.save_provider(input)).await?;
    sync_after_settings(&app, false);
    Ok(store_arc.view())
}

#[tauri::command]
async fn provider_remove(app: AppHandle, store: State<'_, Settings>, id: String) -> Result<SettingsView, String> {
    let store_arc = store.inner().clone();
    let worker = store_arc.clone();
    let result = blocking(move || worker.remove_provider(&id)).await;
    sync_after_settings(&app, false);
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
    sync_after_settings(&app, false);
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
fn shortcuts_status(shortcuts: State<'_, Arc<ShortcutService>>) -> ShortcutStatusView {
    shortcuts.status()
}

#[tauri::command]
fn shortcuts_trigger(app: AppHandle, shortcuts: State<'_, Arc<ShortcutService>>, action: ActionId) {
    shortcuts.trigger_in_app(&app, action);
}

#[tauri::command]
fn shortcuts_configure_system(shortcuts: State<'_, Arc<ShortcutService>>) -> Result<(), String> {
    shortcuts.configure_system()
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
    let mut builder = tauri::Builder::default().manage(LlmClient::new());
    if ShortcutService::uses_legacy_plugin() {
        builder = builder.plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if let Some(svc) = app.try_state::<Arc<ShortcutService>>() {
                        svc.handle_legacy_event(app, shortcut, event.state);
                    }
                })
                .build(),
        );
    }
    builder
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. }
                if window.label() == "main" =>
            {
                if let Some(windows) = window.app_handle().try_state::<Arc<WindowManager>>()
                    && !windows.is_quitting()
                {
                    api.prevent_close();
                    windows.hide_all(window.app_handle());
                }
            }
            WindowEvent::Moved(_) if window.label() == "main" => {
                if let Some(windows) = window.app_handle().try_state::<Arc<WindowManager>>() {
                    windows.layout_chat(window.app_handle());
                }
            }
            _ => {}
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
            let session = SessionManager::new(store.clone(), meetings.clone(), transcript_dir.clone(), {
                let app_handle = app_handle.clone();
                move |ev| match ev {
                    SessionEvent::State(state) => {
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

            let shortcuts = ShortcutService::new(store.clone(), windows.clone(), session.clone(), chat.clone());
            shortcuts.start(app.handle());
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
            shortcuts_status,
            shortcuts_trigger,
            shortcuts_configure_system,
            transcripts_reveal,
            app_open_data_dir,
            app_quit
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

#[cfg(test)]
mod perf_bench {
    use super::*;
    use crate::audio::{FirDecimator3, LinearResampler};
    use crate::meetings::Channel;
    use crate::transcript_files::{format_transcript, format_transcript_tail, slice_tail_chars};
    use std::f32::consts::TAU;
    use std::hint::black_box;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn median_ms(samples: &[f64]) -> f64 {
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        sorted[sorted.len() / 2]
    }

    #[test]
    #[ignore]
    fn perf_benchmarks() {
        let mut segments = Vec::with_capacity(1200);
        for i in 0..1200 {
            segments.push(TranscriptSegment {
                text: format!("Turn {i} covers consistent hashing, quorum writes, and tail latency SLAs."),
                timestamp: i as f64 * 2000.0,
                confidence: 0.98,
                speaker: None,
                channel: Some(if i % 3 == 0 { Channel::Them } else { Channel::You }),
            });
        }

        let runs = 7usize;
        let prompt_iters = 200usize;
        let mut prompt_before = Vec::with_capacity(runs);
        let mut prompt_after = Vec::with_capacity(runs);
        let mut out_a_len = 0usize;
        let mut out_b_len = 0usize;

        for r in 0..runs {
            let run_old = || -> (f64, usize) {
                let mut xor_acc = 0usize;
                let t0 = Instant::now();
                for _ in 0..prompt_iters {
                    let full = format_transcript(black_box(&segments));
                    let clean = full.trim();
                    let count = clean.chars().count();
                    let tail: String = if count > 4000 {
                        clean.chars().skip(count - 4000).collect()
                    } else {
                        clean.to_string()
                    };
                    xor_acc ^= black_box(&tail).len();
                }
                (t0.elapsed().as_secs_f64() * 1000.0, xor_acc)
            };
            let run_new = || -> (f64, usize) {
                let mut xor_acc = 0usize;
                let t0 = Instant::now();
                for _ in 0..prompt_iters {
                    let tail = format_transcript_tail(black_box(&segments), 4000);
                    xor_acc ^= black_box(&tail).len();
                }
                (t0.elapsed().as_secs_f64() * 1000.0, xor_acc)
            };
            if r % 2 == 0 {
                let (ms_a, a) = run_old();
                let (ms_b, b) = run_new();
                prompt_before.push(ms_a);
                prompt_after.push(ms_b);
                out_a_len ^= a;
                out_b_len ^= b;
            } else {
                let (ms_b, b) = run_new();
                let (ms_a, a) = run_old();
                prompt_after.push(ms_b);
                prompt_before.push(ms_a);
                out_b_len ^= b;
                out_a_len ^= a;
            }
        }
        assert_eq!(
            format_transcript_tail(&segments, 4000),
            slice_tail_chars(format_transcript(&segments).trim(), 4000)
        );

        let blocks: Vec<Vec<f32>> = (0..2812)
            .map(|b| (0..1024).map(|i| (TAU * 440.0 * (b * 1024 + i) as f32 / 44_100.0).sin() * 0.5).collect())
            .collect();
        let mut dsp_before = Vec::with_capacity(runs);
        let mut dsp_after = Vec::with_capacity(runs);
        let mut sum_before = 0.0f64;
        let mut sum_after = 0.0f64;

        for r in 0..runs {
            let run_alloc = || -> (f64, f64) {
                let mut resampler = LinearResampler::new(44_100, 48_000);
                let mut dec = FirDecimator3::new();
                let mut acc = 0.0f64;
                let t0 = Instant::now();
                for block in &blocks {
                    let up = resampler.push(black_box(block));
                    let out = dec.push(&up);
                    for &s in &out {
                        acc += s as f64;
                    }
                }
                (t0.elapsed().as_secs_f64() * 1000.0, acc)
            };
            let run_scratch = || -> (f64, f64) {
                let mut resampler = LinearResampler::new(44_100, 48_000);
                let mut dec = FirDecimator3::new();
                let mut buf48 = Vec::with_capacity(2048);
                let mut buf16 = Vec::with_capacity(1024);
                let mut acc = 0.0f64;
                let t0 = Instant::now();
                for block in &blocks {
                    resampler.push_into(black_box(block), &mut buf48);
                    dec.push_into(&buf48, &mut buf16);
                    for &s in &buf16 {
                        acc += s as f64;
                    }
                }
                (t0.elapsed().as_secs_f64() * 1000.0, acc)
            };
            if r % 2 == 0 {
                let (ms_a, a) = run_alloc();
                let (ms_b, b) = run_scratch();
                dsp_before.push(ms_a);
                dsp_after.push(ms_b);
                sum_before = a;
                sum_after = b;
            } else {
                let (ms_b, b) = run_scratch();
                let (ms_a, a) = run_alloc();
                dsp_after.push(ms_b);
                dsp_before.push(ms_a);
                sum_after = b;
                sum_before = a;
            }
        }
        assert!((sum_before - sum_after).abs() < 1e-4, "DSP checksum mismatch: {sum_before} vs {sum_after}");

        async fn bench_uncached(req_count: &AtomicUsize, base: &str, msg: &[crate::llm::LlmMessage]) -> (f64, usize) {
            let before = req_count.load(Ordering::SeqCst);
            let t0 = Instant::now();
            for _ in 0..5 {
                let client = LlmClient::new();
                let res = client.chat_stream(base, None, "compat-model", msg, |_| {}).await.unwrap();
                assert_eq!(res.text, "Consistent hashing works.");
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            (ms, req_count.load(Ordering::SeqCst) - before)
        }

        async fn bench_cached(req_count: &AtomicUsize, base: &str, msg: &[crate::llm::LlmMessage]) -> (f64, usize) {
            let before = req_count.load(Ordering::SeqCst);
            let client = LlmClient::new();
            let t0 = Instant::now();
            for _ in 0..5 {
                let res = client.chat_stream(base, None, "compat-model", msg, |_| {}).await.unwrap();
                assert_eq!(res.text, "Consistent hashing works.");
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            (ms, req_count.load(Ordering::SeqCst) - before)
        }

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (llm_before, llm_after, reqs_before, reqs_after) = rt.block_on(async {
            let req_count = Arc::new(AtomicUsize::new(0));
            let req_srv = req_count.clone();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                while let Ok((mut sock, _)) = listener.accept().await {
                    let req_srv = req_srv.clone();
                    tokio::spawn(async move {
                        let mut buf = Vec::new();
                        let mut tmp = [0u8; 4096];
                        let header_end;
                        loop {
                            let n = sock.read(&mut tmp).await.unwrap_or(0);
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&tmp[..n]);
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                header_end = pos + 4;
                                break;
                            }
                        }
                        let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                        let content_len = headers
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok()).flatten()
                            })
                            .unwrap_or(0);
                        while buf.len() < header_end + content_len {
                            let n = sock.read(&mut tmp).await.unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&tmp[..n]);
                        }
                        req_srv.fetch_add(1, Ordering::SeqCst);
                        let body = String::from_utf8_lossy(&buf[header_end..header_end + content_len]).to_string();
                        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
                        if parsed.get("stream_options").is_some() {
                            let msg = br#"{"error":{"message":"unknown parameter stream_options"}}"#;
                            let head = format!(
                                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                msg.len()
                            );
                            let _ = sock.write_all(head.as_bytes()).await;
                            let _ = sock.write_all(msg).await;
                        } else {
                            let sse = b"data: {\"choices\":[{\"delta\":{\"content\":\"Consistent hashing works.\"}}]}\n\ndata: [DONE]\n\n";
                            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                            let _ = sock.write_all(head.as_bytes()).await;
                            let _ = sock.write_all(sse).await;
                        }
                        let _ = sock.flush().await;
                    });
                }
            });
            let base = format!("http://{addr}/v1");
            let msg = vec![crate::llm::LlmMessage::user("Explain consistent hashing")];

            let mut llm_before = Vec::with_capacity(runs);
            let mut llm_after = Vec::with_capacity(runs);
            let mut reqs_before = 0usize;
            let mut reqs_after = 0usize;

            for r in 0..runs {
                if r % 2 == 0 {
                    let (ms_b, rb) = bench_uncached(&req_count, &base, &msg).await;
                    let (ms_a, ra) = bench_cached(&req_count, &base, &msg).await;
                    llm_before.push(ms_b);
                    llm_after.push(ms_a);
                    reqs_before = rb;
                    reqs_after = ra;
                } else {
                    let (ms_a, ra) = bench_cached(&req_count, &base, &msg).await;
                    let (ms_b, rb) = bench_uncached(&req_count, &base, &msg).await;
                    llm_after.push(ms_a);
                    llm_before.push(ms_b);
                    reqs_after = ra;
                    reqs_before = rb;
                }
            }
            (llm_before, llm_after, reqs_before, reqs_after)
        });

        let summary = serde_json::json!({
            "prompt_1200_segments_200x": {
                "runs": runs,
                "before_ms": { "median": median_ms(&prompt_before), "min": prompt_before.iter().copied().fold(f64::INFINITY, f64::min), "max": prompt_before.iter().copied().fold(0.0_f64, f64::max), "samples": prompt_before },
                "after_ms": { "median": median_ms(&prompt_after), "min": prompt_after.iter().copied().fold(f64::INFINITY, f64::min), "max": prompt_after.iter().copied().fold(0.0_f64, f64::max), "samples": prompt_after },
                "checksum_match": out_a_len == out_b_len
            },
            "audio_dsp_60s_2812_blocks": {
                "runs": runs,
                "before_ms": { "median": median_ms(&dsp_before), "min": dsp_before.iter().copied().fold(f64::INFINITY, f64::min), "max": dsp_before.iter().copied().fold(0.0_f64, f64::max), "samples": dsp_before },
                "after_ms": { "median": median_ms(&dsp_after), "min": dsp_after.iter().copied().fold(f64::INFINITY, f64::min), "max": dsp_after.iter().copied().fold(0.0_f64, f64::max), "samples": dsp_after },
                "checksum_delta": (sum_before - sum_after).abs()
            },
            "llm_stream_5_questions_no_stream_options": {
                "runs": runs,
                "before_ms": { "median": median_ms(&llm_before), "min": llm_before.iter().copied().fold(f64::INFINITY, f64::min), "max": llm_before.iter().copied().fold(0.0_f64, f64::max), "samples": llm_before },
                "after_ms": { "median": median_ms(&llm_after), "min": llm_after.iter().copied().fold(f64::INFINITY, f64::min), "max": llm_after.iter().copied().fold(0.0_f64, f64::max), "samples": llm_after },
                "http_requests_per_5_questions_before": reqs_before,
                "http_requests_per_5_questions_after": reqs_after
            }
        });
        println!("PERF_BENCH_JSON={}", serde_json::to_string(&summary).unwrap());
    }
}
