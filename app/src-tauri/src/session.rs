//! Live recording session: coordinates native audio capture, Deepgram streaming,
//! per-channel utterance endpointing, reconnect backoff, and transcript autosave.

use crate::audio::{AudioLevels, LiveCapture};
use crate::deepgram::{self, DeepgramConfig, DeepgramHandle, SocketEvent, TranscriptEvent};
use crate::meetings::{Channel, Meeting, MeetingStatus, MeetingStore, TranscriptSegment};
use crate::settings::{AudioSource, ProviderKind, SettingsStore};
use crate::transcript_files::{TranscriptAutosave, format_transcript, format_transcript_tail, now_ms, title_from_transcript};
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const MAX_RECONNECTS: u32 = 5;
const UTTERANCE_MAX_MS: f64 = 20_000.0;
const DUPLICATE_FINAL_WINDOW_MS: f64 = 1_500.0;

fn spawn_bg(fut: impl Future<Output = ()> + Send + 'static) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(fut);
    } else {
        tauri::async_runtime::spawn(fut);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Idle,
    Connecting,
    Recording,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub status: SessionStatus,
    pub started_at: Option<f64>,
    pub notice: Option<String>,
    pub connected: bool,
    pub audio_source: AudioSource,
    pub microphone_id: String,
    pub monitor_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Utterance {
    pub text: String,
    pub speaker: Option<i64>,
    pub channel: Option<Channel>,
    pub timestamp: f64,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    State(SessionState),
    Transcript(TranscriptEvent),
    Utterance(Utterance),
    Levels(AudioLevels),
    Finished(Meeting),
    Discarded,
}

pub trait SocketSender: Send + Sync {
    fn send_audio(&self, pcm16: Vec<u8>) -> bool;
    fn close(&self);
}

impl SocketSender for DeepgramHandle {
    fn send_audio(&self, pcm16: Vec<u8>) -> bool {
        DeepgramHandle::send_audio(self, pcm16)
    }

    fn close(&self) {
        DeepgramHandle::close(self);
    }
}

pub type ConnectFuture = Pin<Box<dyn Future<Output = Result<(Arc<dyn SocketSender>, mpsc::UnboundedReceiver<SocketEvent>), String>> + Send>>;

pub trait TranscriptionConnector: Send + Sync {
    fn connect(&self, config: DeepgramConfig) -> ConnectFuture;
}

pub struct LiveTranscriptionConnector;

impl TranscriptionConnector for LiveTranscriptionConnector {
    fn connect(&self, config: DeepgramConfig) -> ConnectFuture {
        Box::pin(async move {
            let (handle, rx) = deepgram::connect(&config).await?;
            Ok((Arc::new(handle) as Arc<dyn SocketSender>, rx))
        })
    }
}

pub trait CaptureConnector: Send + Sync {
    fn start(
        &self,
        source: AudioSource,
        mic: &str,
        monitor: &str,
        denoise: bool,
        paused: Arc<AtomicBool>,
        on_pcm16: Box<dyn FnMut(Vec<u8>) + Send>,
        on_levels: Box<dyn FnMut(AudioLevels) + Send>,
        on_error: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn Send + Sync>, String>;
}

pub struct LiveCaptureConnector;

impl CaptureConnector for LiveCaptureConnector {
    fn start(
        &self,
        source: AudioSource,
        mic: &str,
        monitor: &str,
        denoise: bool,
        paused: Arc<AtomicBool>,
        mut on_pcm16: Box<dyn FnMut(Vec<u8>) + Send>,
        mut on_levels: Box<dyn FnMut(AudioLevels) + Send>,
        on_error: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn Send + Sync>, String> {
        let capture = LiveCapture::start(
            source,
            mic,
            monitor,
            denoise,
            paused,
            move |bytes| on_pcm16(bytes),
            move |levels| on_levels(levels),
            move |err| on_error(err),
        )?;
        Ok(Box::new(capture))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct EngineKey {
    transcriber_id: String,
    language: String,
}

#[derive(Clone, PartialEq, Eq)]
struct AudioKey {
    source: AudioSource,
    mic: String,
    monitor: String,
    denoise: bool,
}

struct Inner {
    status: SessionStatus,
    started_at: Option<f64>,
    notice: Option<String>,
    connected: bool,
    generation: u64,
    socket: Option<Arc<dyn SocketSender>>,
    capture: Option<Box<dyn Send + Sync>>,
    paused_flag: Arc<AtomicBool>,
    segments: Vec<TranscriptSegment>,
    transcript_chars: usize,
    pending: HashMap<Option<Channel>, Vec<TranscriptSegment>>,
    last_final: HashMap<Option<Channel>, (String, Option<i64>, f64)>,
    last_utterance: Option<Utterance>,
    last_them_utterance: Option<Utterance>,
    engine_key: Option<EngineKey>,
    audio_key: Option<AudioKey>,
    reconnects: u32,
    reconnecting: bool,
    opening: bool,
    autosave: TranscriptAutosave,
}

pub struct SessionManager {
    settings: Arc<SettingsStore>,
    meetings: Arc<MeetingStore>,
    connector: Arc<dyn TranscriptionConnector>,
    capture_connector: Arc<dyn CaptureConnector>,
    emit: Arc<dyn Fn(SessionEvent) + Send + Sync>,
    inner: Mutex<Inner>,
}

impl SessionManager {
    pub fn new(
        settings: Arc<SettingsStore>,
        meetings: Arc<MeetingStore>,
        transcript_dir: std::path::PathBuf,
        emit: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Self::with_connectors(
            settings,
            meetings,
            transcript_dir,
            Arc::new(LiveTranscriptionConnector),
            Arc::new(LiveCaptureConnector),
            emit,
        )
    }

    pub fn with_connectors(
        settings: Arc<SettingsStore>,
        meetings: Arc<MeetingStore>,
        transcript_dir: std::path::PathBuf,
        connector: Arc<dyn TranscriptionConnector>,
        capture_connector: Arc<dyn CaptureConnector>,
        emit: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            meetings,
            connector,
            capture_connector,
            emit: Arc::new(emit),
            inner: Mutex::new(Inner {
                status: SessionStatus::Idle,
                started_at: None,
                notice: None,
                connected: false,
                generation: 0,
                socket: None,
                capture: None,
                paused_flag: Arc::new(AtomicBool::new(false)),
                segments: Vec::new(),
                transcript_chars: 0,
                pending: HashMap::new(),
                last_final: HashMap::new(),
                last_utterance: None,
                last_them_utterance: None,
                engine_key: None,
                audio_key: None,
                reconnects: 0,
                reconnecting: false,
                opening: false,
                autosave: TranscriptAutosave::new(transcript_dir),
            }),
        })
    }

    pub fn state(&self) -> SessionState {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.build_state(&guard)
    }

    fn emit_state(&self) {
        (self.emit)(SessionEvent::State(self.state()));
    }

    fn build_state(&self, guard: &Inner) -> SessionState {
        let s = self.settings.get();
        SessionState {
            status: guard.status,
            started_at: guard.started_at,
            notice: guard.notice.clone(),
            connected: guard.connected,
            audio_source: s.audio_source,
            microphone_id: s.microphone_id,
            monitor_id: s.monitor_id,
        }
    }

    pub fn transcript_text(&self) -> String {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        format_transcript(&guard.segments)
    }

    pub fn recent_transcript_text(&self, max_chars: usize) -> String {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        format_transcript_tail(&guard.segments, max_chars)
    }

    pub fn transcript_char_estimate(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).transcript_chars
    }

    pub fn segments(&self) -> Vec<TranscriptSegment> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).segments.clone()
    }

    pub fn report_error(&self, err: String) {
        let state = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.notice = Some(err);
            self.build_state(&guard)
        };
        (self.emit)(SessionEvent::State(state));
    }

    pub fn last_utterance(&self) -> Option<Utterance> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if self.settings.get().audio_source != AudioSource::Microphone {
            guard.last_them_utterance.clone().or_else(|| guard.last_utterance.clone())
        } else {
            guard.last_utterance.clone()
        }
    }

    pub fn draft(&self) -> Option<Meeting> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.status == SessionStatus::Idle {
            return None;
        }
        self.to_meeting(&guard, now_ms())
    }

    pub fn save_autosave(&self) {
        let draft = self.draft();
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(err) = guard.autosave.save_draft(draft.as_ref()) {
            eprintln!("[session] autosave failed: {err}");
        }
    }

    pub fn transcript_dir(&self) -> std::path::PathBuf {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).autosave.dir().to_path_buf()
    }

    pub async fn start(self: &Arc<Self>) -> Result<SessionState, String> {
        let (config, engine_key, audio_key, session_gen) = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.status != SessionStatus::Idle {
                return Ok(self.build_state(&guard));
            }
            let (config, engine_key) = self.resolve_deepgram_config()?;
            let s = self.settings.get();
            let audio_key = AudioKey {
                source: s.audio_source,
                mic: s.microphone_id,
                monitor: s.monitor_id,
                denoise: s.denoise_mic,
            };
            validate_audio_selection(&audio_key)?;

            guard.generation += 1;
            let session_gen = guard.generation;
            guard.segments.clear();
            guard.transcript_chars = 0;
            guard.pending.clear();
            guard.last_final.clear();
            guard.last_utterance = None;
            guard.last_them_utterance = None;
            guard.reconnects = 0;
            guard.reconnecting = false;
            guard.started_at = Some(now_ms());
            guard.status = SessionStatus::Connecting;
            guard.notice = None;
            guard.connected = false;
            guard.paused_flag.store(false, Ordering::SeqCst);
            (config, engine_key, audio_key, session_gen)
        };
        self.emit_state();

        let capture = match self.start_capture(&audio_key, session_gen) {
            Ok(cap) => cap,
            Err(err) => {
                let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if guard.generation == session_gen {
                    guard.status = SessionStatus::Idle;
                    guard.started_at = None;
                    guard.notice = Some(err.clone());
                    let state = self.build_state(&guard);
                    drop(guard);
                    (self.emit)(SessionEvent::State(state));
                }
                return Err(err);
            }
        };

        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.generation != session_gen {
                drop(capture);
                return Ok(self.build_state(&guard));
            }
            guard.capture = Some(capture);
            guard.audio_key = Some(audio_key);
        }

        match self.open_socket_for(config, engine_key, session_gen).await {
            Ok(()) => {
                let state = {
                    let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    if guard.generation == session_gen && guard.status == SessionStatus::Connecting {
                        guard.status = SessionStatus::Recording;
                    }
                    self.build_state(&guard)
                };
                (self.emit)(SessionEvent::State(state.clone()));
                self.sync_engine();
                Ok(state)
            }
            Err(err) => {
                let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if guard.generation == session_gen {
                    guard.capture.take();
                    guard.socket.take();
                    guard.connected = false;
                    guard.status = SessionStatus::Idle;
                    guard.started_at = None;
                    guard.notice = Some(err.clone());
                    let state = self.build_state(&guard);
                    drop(guard);
                    (self.emit)(SessionEvent::State(state));
                }
                Err(err)
            }
        }
    }

    pub fn pause(&self) -> SessionState {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.status == SessionStatus::Recording {
            guard.status = SessionStatus::Paused;
            guard.paused_flag.store(true, Ordering::SeqCst);
            let state = self.build_state(&guard);
            drop(guard);
            (self.emit)(SessionEvent::State(state.clone()));
            return state;
        }
        self.build_state(&guard)
    }

    pub fn resume(self: &Arc<Self>) -> SessionState {
        let (need_reconnect, state) = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.status != SessionStatus::Paused {
                return self.build_state(&guard);
            }
            guard.paused_flag.store(false, Ordering::SeqCst);
            let need_reconnect = !guard.connected;
            if need_reconnect {
                guard.status = SessionStatus::Connecting;
                guard.reconnects = 0;
                guard.reconnecting = false;
            } else {
                guard.status = SessionStatus::Recording;
            }
            (need_reconnect, self.build_state(&guard))
        };
        (self.emit)(SessionEvent::State(state.clone()));
        if need_reconnect {
            let this = self.clone();
            spawn_bg(async move {
                let session_gen = this.inner.lock().unwrap_or_else(|e| e.into_inner()).generation;
                if let Ok((cfg, key)) = this.resolve_deepgram_config() {
                    match this.open_socket_for(cfg, key, session_gen).await {
                        Ok(()) => {
                            let mut guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                            if guard.generation == session_gen && guard.status == SessionStatus::Connecting {
                                guard.status = SessionStatus::Recording;
                            }
                            let st = this.build_state(&guard);
                            drop(guard);
                            (this.emit)(SessionEvent::State(st));
                            this.sync_engine();
                        }
                        Err(err) => {
                            let mut guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                            if guard.generation == session_gen {
                                guard.status = SessionStatus::Paused;
                                guard.paused_flag.store(true, Ordering::SeqCst);
                                guard.notice = Some(err);
                                let st = this.build_state(&guard);
                                drop(guard);
                                (this.emit)(SessionEvent::State(st));
                            }
                        }
                    }
                }
            });
        } else {
            self.sync_engine();
        }
        state
    }

    pub fn finish(&self) -> Result<Option<Meeting>, String> {
        let (flushed, meeting, state) = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.status == SessionStatus::Idle {
                return Ok(None);
            }
            guard.generation += 1;
            guard.capture.take();
            if let Some(sock) = guard.socket.take() {
                sock.close();
            }
            guard.connected = false;
            guard.paused_flag.store(false, Ordering::SeqCst);
            guard.reconnecting = false;
            guard.opening = false;
            let flushed = Self::drain_pending_utterances(&mut guard, now_ms());
            let meeting = self.to_meeting(&guard, now_ms());
            guard.segments.clear();
            guard.transcript_chars = 0;
            guard.last_final.clear();
            guard.last_utterance = None;
            guard.last_them_utterance = None;
            guard.status = SessionStatus::Idle;
            guard.started_at = None;
            guard.notice = None;
            guard.engine_key = None;
            guard.audio_key = None;
            let state = self.build_state(&guard);
            (flushed, meeting, state)
        };

        for u in flushed {
            (self.emit)(SessionEvent::Utterance(u));
        }
        (self.emit)(SessionEvent::State(state));

        let Some(meeting) = meeting else {
            self.inner.lock().unwrap_or_else(|e| e.into_inner()).autosave.discard();
            return Ok(None);
        };

        self.meetings.save(meeting.clone())?;
        if let Err(err) = self.inner.lock().unwrap_or_else(|e| e.into_inner()).autosave.finish(&meeting) {
            eprintln!("[session] transcript save failed: {err}");
        }
        (self.emit)(SessionEvent::Finished(meeting.clone()));
        Ok(Some(meeting))
    }

    pub fn cancel(&self) -> SessionState {
        let (was_active, state) = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let was_active = guard.status != SessionStatus::Idle;
            guard.generation += 1;
            guard.capture.take();
            if let Some(sock) = guard.socket.take() {
                sock.close();
            }
            guard.connected = false;
            guard.paused_flag.store(false, Ordering::SeqCst);
            guard.reconnecting = false;
            guard.opening = false;
            guard.segments.clear();
            guard.transcript_chars = 0;
            guard.pending.clear();
            guard.last_final.clear();
            guard.last_utterance = None;
            guard.last_them_utterance = None;
            guard.started_at = None;
            guard.notice = None;
            guard.engine_key = None;
            guard.audio_key = None;
            guard.status = SessionStatus::Idle;
            guard.autosave.discard();
            (was_active, self.build_state(&guard))
        };
        (self.emit)(SessionEvent::State(state.clone()));
        if was_active {
            (self.emit)(SessionEvent::Discarded);
        }
        state
    }

    pub fn sync_engine(self: &Arc<Self>) {
        let (flushed, need_socket, need_capture, session_gen, audio_key) = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !matches!(guard.status, SessionStatus::Recording | SessionStatus::Paused) {
                return;
            }
            let s = self.settings.get();
            let desired_audio = AudioKey {
                source: s.audio_source,
                mic: s.microphone_id,
                monitor: s.monitor_id,
                denoise: s.denoise_mic,
            };
            let need_capture = guard.audio_key.as_ref() != Some(&desired_audio);
            let desired_engine = s.transcriber_id.map(|id| EngineKey {
                transcriber_id: id,
                language: s.audio_language,
            });
            let need_socket = !guard.opening
                && !guard.reconnecting
                && desired_engine.is_some()
                && guard.engine_key != desired_engine;
            let flushed = if need_socket {
                let drained = Self::drain_pending_utterances(&mut guard, now_ms());
                guard.pending.clear();
                drained
            } else {
                Vec::new()
            };
            (flushed, need_socket, need_capture, guard.generation, desired_audio)
        };

        for u in flushed {
            (self.emit)(SessionEvent::Utterance(u));
        }

        if need_capture && validate_audio_selection(&audio_key).is_ok() {
            if let Ok(cap) = self.start_capture(&audio_key, session_gen) {
                let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if guard.generation == session_gen && matches!(guard.status, SessionStatus::Recording | SessionStatus::Paused) {
                    guard.capture = Some(cap);
                    guard.audio_key = Some(audio_key);
                }
            }
        }

        if need_socket {
            let this = self.clone();
            spawn_bg(async move {
                match this.resolve_deepgram_config() {
                    Ok((cfg, key)) => {
                        if let Err(err) = this.open_socket_for(cfg, key, session_gen).await {
                            eprintln!("[session] engine switch failed: {err}");
                            this.schedule_reconnect(session_gen);
                        } else {
                            this.sync_engine();
                        }
                    }
                    Err(err) => {
                        eprintln!("[session] engine switch missing config: {err}");
                    }
                }
            });
        }
    }

    fn resolve_deepgram_config(&self) -> Result<(DeepgramConfig, EngineKey), String> {
        let s = self.settings.get();
        let Some(id) = s.transcriber_id.clone() else {
            return Err("Configure and select a Deepgram transcription provider in Settings before listening.".into());
        };
        let Some(record) = s.provider(&id).cloned() else {
            return Err("The selected transcription provider no longer exists.".into());
        };
        if record.kind != ProviderKind::Deepgram {
            return Err("The selected transcriber is not a Deepgram provider.".into());
        }
        let Some(api_key) = self.settings.provider_key(&id)? else {
            return Err(format!("The API key for {} is missing. Enter it in Settings.", record.label));
        };
        let key = EngineKey {
            transcriber_id: id,
            language: s.audio_language.clone(),
        };
        let config = DeepgramConfig::multichannel(record.endpoint, api_key, record.model, s.audio_language);
        Ok((config, key))
    }

    fn start_capture(self: &Arc<Self>, audio_key: &AudioKey, session_gen: u64) -> Result<Box<dyn Send + Sync>, String> {
        let paused = self.inner.lock().unwrap_or_else(|e| e.into_inner()).paused_flag.clone();
        let this_pcm = Arc::downgrade(self);
        let this_lvl = Arc::downgrade(self);
        let this_err = Arc::downgrade(self);
        self.capture_connector.start(
            audio_key.source,
            &audio_key.mic,
            &audio_key.monitor,
            audio_key.denoise,
            paused,
            Box::new(move |bytes| {
                if let Some(session) = this_pcm.upgrade() {
                    session.forward_audio(session_gen, bytes);
                }
            }),
            Box::new(move |levels| {
                if let Some(session) = this_lvl.upgrade() {
                    let guard = session.inner.lock().unwrap_or_else(|e| e.into_inner());
                    if guard.generation == session_gen && guard.status == SessionStatus::Recording {
                        drop(guard);
                        (session.emit)(SessionEvent::Levels(levels));
                    }
                }
            }),
            Arc::new(move |err| {
                if let Some(session) = this_err.upgrade() {
                    session.on_capture_error(session_gen, err);
                }
            }),
        )
    }

    fn forward_audio(&self, session_gen: u64, bytes: Vec<u8>) {
        let sock = {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.generation != session_gen || guard.status != SessionStatus::Recording || !guard.connected {
                return;
            }
            guard.socket.clone()
        };
        if let Some(sock) = sock {
            let _ = sock.send_audio(bytes);
        }
    }

    fn on_capture_error(&self, session_gen: u64, err: String) {
        let state = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.generation != session_gen || guard.status == SessionStatus::Idle {
                return;
            }
            guard.capture.take();
            guard.audio_key = None;
            guard.status = SessionStatus::Paused;
            guard.paused_flag.store(true, Ordering::SeqCst);
            guard.notice = Some(err);
            self.build_state(&guard)
        };
        (self.emit)(SessionEvent::State(state));
    }

    async fn open_socket_for(self: &Arc<Self>, config: DeepgramConfig, key: EngineKey, session_gen: u64) -> Result<(), String> {
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.generation != session_gen {
                return Ok(());
            }
            if let Some(old) = guard.socket.take() {
                old.close();
            }
            guard.connected = false;
            guard.opening = true;
            guard.engine_key = Some(key);
        }

        let res = self.connector.connect(config).await;
        let (sock, mut rx) = match res {
            Ok(pair) => pair,
            Err(err) => {
                let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if guard.generation == session_gen {
                    guard.opening = false;
                }
                return Err(err);
            }
        };

        let state = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.generation != session_gen {
                sock.close();
                return Ok(());
            }
            guard.opening = false;
            guard.socket = Some(sock.clone());
            guard.connected = true;
            guard.reconnects = 0;
            guard.reconnecting = false;
            guard.notice = None;
            self.build_state(&guard)
        };
        (self.emit)(SessionEvent::State(state));

        let this = self.clone();
        spawn_bg(async move {
            while let Some(ev) = rx.recv().await {
                let still_active = {
                    let guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                    guard.generation == session_gen && guard.socket.as_ref().is_some_and(|s| Arc::ptr_eq(s, &sock))
                };
                if !still_active {
                    break;
                }
                match ev {
                    SocketEvent::Transcript(t) => this.on_transcript(session_gen, t),
                    SocketEvent::Error(msg) => {
                        eprintln!("[deepgram] {msg}");
                        let mut guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                        if guard.generation == session_gen {
                            guard.notice = Some(msg);
                            let st = this.build_state(&guard);
                            drop(guard);
                            (this.emit)(SessionEvent::State(st));
                        }
                    }
                    SocketEvent::Closed { by_us, reason } => {
                        if by_us {
                            break;
                        }
                        if !reason.is_empty() {
                            eprintln!("[deepgram] closed: {reason}");
                        }
                        {
                            let mut guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                            if guard.generation != session_gen || guard.status == SessionStatus::Idle {
                                break;
                            }
                            guard.connected = false;
                            let st = this.build_state(&guard);
                            drop(guard);
                            (this.emit)(SessionEvent::State(st));
                        }
                        this.schedule_reconnect(session_gen);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    fn schedule_reconnect(self: &Arc<Self>, session_gen: u64) {
        let delay_ms = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.generation != session_gen || guard.status == SessionStatus::Idle || guard.reconnecting {
                return;
            }
            if guard.reconnects >= MAX_RECONNECTS {
                guard.status = SessionStatus::Paused;
                guard.paused_flag.store(true, Ordering::SeqCst);
                guard.notice = Some("Transcription connection lost. Press resume to reconnect.".into());
                let st = self.build_state(&guard);
                drop(guard);
                (self.emit)(SessionEvent::State(st));
                return;
            }
            guard.reconnects += 1;
            guard.reconnecting = true;
            guard.notice = Some(format!(
                "Transcription connection lost, reconnecting ({}/{})…",
                guard.reconnects, MAX_RECONNECTS
            ));
            let delay = (1000u64 << (guard.reconnects - 1)).min(30_000);
            let st = self.build_state(&guard);
            drop(guard);
            (self.emit)(SessionEvent::State(st));
            delay
        };

        let this = self.clone();
        spawn_bg(async move {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            let should_try = {
                let mut guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                if guard.generation != session_gen || guard.status == SessionStatus::Idle {
                    guard.reconnecting = false;
                    false
                } else {
                    guard.reconnecting = false;
                    true
                }
            };
            if !should_try {
                return;
            }
            match this.resolve_deepgram_config() {
                Ok((cfg, key)) => {
                    if let Err(err) = this.open_socket_for(cfg, key, session_gen).await {
                        eprintln!("[session] reconnect failed: {err}");
                        this.schedule_reconnect(session_gen);
                    } else {
                        this.sync_engine();
                    }
                }
                Err(err) => {
                    let mut guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                    if guard.generation == session_gen {
                        guard.status = SessionStatus::Paused;
                        guard.paused_flag.store(true, Ordering::SeqCst);
                        guard.notice = Some(err);
                        let st = this.build_state(&guard);
                        drop(guard);
                        (this.emit)(SessionEvent::State(st));
                    }
                }
            }
        });
    }

    pub fn on_transcript(&self, session_gen: u64, event: TranscriptEvent) {
        let now = now_ms();
        let closed_utterance = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.generation != session_gen || guard.status == SessionStatus::Idle {
                return;
            }
            if event.is_final {
                if let Some((prev_text, prev_speaker, prev_ts)) = guard.last_final.get(&event.channel)
                    && prev_text == &event.text
                    && *prev_speaker == event.speaker
                    && now - *prev_ts < DUPLICATE_FINAL_WINDOW_MS
                {
                    return;
                }
                guard.last_final.insert(event.channel, (event.text.clone(), event.speaker, now));
            }

            let cloned = event.clone();
            if !event.is_final {
                drop(guard);
                (self.emit)(SessionEvent::Transcript(cloned));
                return;
            }

            let segment = TranscriptSegment {
                text: event.text,
                timestamp: now,
                confidence: event.confidence,
                speaker: event.speaker,
                channel: event.channel,
            };
            guard.transcript_chars += segment.text.chars().count() + 10;
            guard.segments.push(segment.clone());
            let pending = guard.pending.entry(event.channel).or_default();
            let first_ts = pending.first().map_or(now, |s| s.timestamp);
            pending.push(segment);

            let utterance = if event.speech_final || now - first_ts > UTTERANCE_MAX_MS {
                Self::close_channel_utterance(&mut guard, event.channel, now)
            } else {
                None
            };
            drop(guard);
            (self.emit)(SessionEvent::Transcript(cloned));
            utterance
        };

        if let Some(u) = closed_utterance {
            (self.emit)(SessionEvent::Utterance(u));
        }
    }

    fn close_channel_utterance(guard: &mut Inner, channel: Option<Channel>, now: f64) -> Option<Utterance> {
        let list = guard.pending.remove(&channel)?;
        if list.is_empty() {
            return None;
        }
        let text = list.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
        let last = list.last().unwrap();
        let utterance = Utterance {
            text,
            speaker: last.speaker,
            channel: last.channel,
            timestamp: now,
        };
        if utterance.channel == Some(Channel::Them) {
            guard.last_them_utterance = Some(utterance.clone());
        }
        guard.last_utterance = Some(utterance.clone());
        Some(utterance)
    }

    fn drain_pending_utterances(guard: &mut Inner, now: f64) -> Vec<Utterance> {
        let mut keys: Vec<(f64, Option<Channel>)> = guard
            .pending
            .iter()
            .filter_map(|(ch, segs)| segs.first().map(|s| (s.timestamp, *ch)))
            .collect();
        keys.sort_by(|a, b| a.0.total_cmp(&b.0));
        keys.into_iter()
            .filter_map(|(_, ch)| Self::close_channel_utterance(guard, ch, now))
            .collect()
    }

    fn to_meeting(&self, guard: &Inner, end_time: f64) -> Option<Meeting> {
        if guard.segments.is_empty() {
            return None;
        }
        let start_time = guard.started_at.unwrap_or(end_time);
        let full_transcript_text = format_transcript(&guard.segments);
        let combined_words: String = guard.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
        Some(Meeting {
            id: format!("meeting_{}", uuid::Uuid::new_v4().simple()),
            title: title_from_transcript(&combined_words),
            start_time,
            end_time,
            duration: (end_time - start_time).max(0.0),
            transcript: guard.segments.clone(),
            full_transcript_text,
            language: self.settings.get().audio_language,
            status: MeetingStatus::Processing,
            summary: None,
            created_at: end_time,
            updated_at: end_time,
        })
    }
}

fn validate_audio_selection(key: &AudioKey) -> Result<(), String> {
    if matches!(key.source, AudioSource::Both | AudioSource::Microphone) && key.mic.trim().is_empty() {
        return Err("Select a microphone in Settings before listening.".into());
    }
    if matches!(key.source, AudioSource::Both | AudioSource::System) && key.monitor.trim().is_empty() {
        return Err("Select a system monitor in Settings before listening.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;
    use crate::settings::{ProviderInput, SettingsPatch};
    use crate::storage::test_support::TempDir;

    struct FakeSocket {
        closed: AtomicBool,
        sent_chunks: Mutex<Vec<Vec<u8>>>,
    }

    impl FakeSocket {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                closed: AtomicBool::new(false),
                sent_chunks: Mutex::new(Vec::new()),
            })
        }
    }

    impl SocketSender for FakeSocket {
        fn send_audio(&self, pcm16: Vec<u8>) -> bool {
            self.sent_chunks.lock().unwrap().push(pcm16);
            true
        }

        fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    struct FakeConnector {
        sockets: Mutex<Vec<(Arc<FakeSocket>, mpsc::UnboundedSender<SocketEvent>, DeepgramConfig)>>,
        fail_next: AtomicBool,
    }

    impl FakeConnector {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                sockets: Mutex::new(Vec::new()),
                fail_next: AtomicBool::new(false),
            })
        }
    }

    impl TranscriptionConnector for FakeConnector {
        fn connect(&self, config: DeepgramConfig) -> ConnectFuture {
            let fail = self.fail_next.load(Ordering::SeqCst);
            let sock = FakeSocket::new();
            let (tx, rx) = mpsc::unbounded_channel();
            if !fail {
                self.sockets.lock().unwrap().push((sock.clone(), tx, config));
            }
            Box::pin(async move {
                if fail {
                    Err("Simulated connection refused".into())
                } else {
                    Ok((sock as Arc<dyn SocketSender>, rx))
                }
            })
        }
    }

    struct NoopCaptureConnector;

    impl CaptureConnector for NoopCaptureConnector {
        fn start(
            &self,
            _source: AudioSource,
            _mic: &str,
            _monitor: &str,
            _denoise: bool,
            _paused: Arc<AtomicBool>,
            _on_pcm16: Box<dyn FnMut(Vec<u8>) + Send>,
            _on_levels: Box<dyn FnMut(AudioLevels) + Send>,
            _on_error: Arc<dyn Fn(String) + Send + Sync>,
        ) -> Result<Box<dyn Send + Sync>, String> {
            Ok(Box::new(()))
        }
    }

    fn harness() -> (TempDir, Arc<SettingsStore>, Arc<MeetingStore>, Arc<FakeConnector>, Arc<SessionManager>, Arc<Mutex<Vec<SessionEvent>>>) {
        let dir = TempDir::new();
        let secrets = Arc::new(MemorySecrets::default());
        let settings = Arc::new(SettingsStore::open(dir.path(), secrets).unwrap());
        settings
            .save_provider(ProviderInput {
                id: None,
                kind: ProviderKind::Deepgram,
                label: "Deepgram".into(),
                endpoint: "wss://api.deepgram.com/v1/listen".into(),
                model: "nova-3".into(),
                key: Some("dg-key".into()),
                clear_key: false,
            })
            .unwrap();
        settings
            .update(SettingsPatch {
                microphone_id: Some("mic-1".into()),
                monitor_id: Some("sink.monitor".into()),
                ..Default::default()
            })
            .unwrap();
        let meetings = Arc::new(MeetingStore::open(dir.path()).unwrap());
        let connector = FakeConnector::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_cb = events.clone();
        let session = SessionManager::with_connectors(
            settings.clone(),
            meetings.clone(),
            dir.path().join("transcripts"),
            connector.clone(),
            Arc::new(NoopCaptureConnector),
            move |ev| events_cb.lock().unwrap().push(ev),
        );
        (dir, settings, meetings, connector, session, events)
    }

    #[tokio::test]
    async fn refuses_start_without_transcriber_or_selected_devices() {
        let dir = TempDir::new();
        let secrets = Arc::new(MemorySecrets::default());
        let settings = Arc::new(SettingsStore::open(dir.path(), secrets).unwrap());
        let meetings = Arc::new(MeetingStore::open(dir.path()).unwrap());
        let session = SessionManager::with_connectors(
            settings,
            meetings,
            dir.path().join("transcripts"),
            FakeConnector::new(),
            Arc::new(NoopCaptureConnector),
            |_| {},
        );
        let err = session.start().await.unwrap_err();
        assert!(err.contains("Deepgram"), "{err}");
        assert_eq!(session.state().status, SessionStatus::Idle);
    }

    #[tokio::test]
    async fn buffers_per_channel_deduplicates_finals_and_flushes_pending_on_finish() {
        let (_dir, _settings, meetings, connector, session, events) = harness();
        session.start().await.unwrap();
        let tx = connector.sockets.lock().unwrap()[0].1.clone();

        tx.send(SocketEvent::Transcript(TranscriptEvent {
            text: "First part".into(),
            is_final: true,
            speech_final: false,
            confidence: 0.9,
            speaker: Some(0),
            channel: Some(Channel::Them),
        }))
        .unwrap();
        // Duplicate final right after should be ignored.
        tx.send(SocketEvent::Transcript(TranscriptEvent {
            text: "First part".into(),
            is_final: true,
            speech_final: false,
            confidence: 0.9,
            speaker: Some(0),
            channel: Some(Channel::Them),
        }))
        .unwrap();
        tx.send(SocketEvent::Transcript(TranscriptEvent {
            text: "My reply".into(),
            is_final: true,
            speech_final: true,
            confidence: 0.95,
            speaker: None,
            channel: Some(Channel::You),
        }))
        .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        // Stop while `Them`'s "First part" is still pending (speech_final was false).
        let saved = session.finish().unwrap().unwrap();
        assert_eq!(saved.transcript.len(), 2);
        assert_eq!(saved.transcript[0].channel, Some(Channel::Them));
        assert_eq!(saved.transcript[1].channel, Some(Channel::You));
        assert_eq!(meetings.list().len(), 1);

        let utterances: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                SessionEvent::Utterance(u) => Some(u.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(utterances.len(), 2);
        assert_eq!(utterances[1].text, "First part");
        assert_eq!(utterances[1].channel, Some(Channel::Them));
    }

    #[tokio::test]
    async fn switches_language_mid_session_and_preserves_transcript() {
        let (_dir, settings, _meetings, connector, session, _) = harness();
        session.start().await.unwrap();
        let tx0 = connector.sockets.lock().unwrap()[0].1.clone();
        tx0.send(SocketEvent::Transcript(TranscriptEvent {
            text: "Hello everyone.".into(),
            is_final: true,
            speech_final: true,
            confidence: 1.0,
            speaker: None,
            channel: Some(Channel::Them),
        }))
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        settings.update(SettingsPatch { audio_language: Some("ru".into()), ..Default::default() }).unwrap();
        session.sync_engine();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        let langs: Vec<String> = connector.sockets.lock().unwrap().iter().map(|(_, _, c)| c.language.clone()).collect();
        assert_eq!(langs, vec!["en", "ru"]);
        assert!(connector.sockets.lock().unwrap()[0].0.closed.load(Ordering::SeqCst));

        let tx1 = connector.sockets.lock().unwrap()[1].1.clone();
        tx1.send(SocketEvent::Transcript(TranscriptEvent {
            text: "Привет.".into(),
            is_final: true,
            speech_final: true,
            confidence: 1.0,
            speaker: None,
            channel: Some(Channel::Them),
        }))
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let meeting = session.finish().unwrap().unwrap();
        assert_eq!(meeting.transcript.len(), 2);
        assert_eq!(meeting.language, "ru");
    }

    #[tokio::test]
    async fn reconnects_on_unexpected_close_without_losing_transcript() {
        let (_dir, _settings, _meetings, connector, session, _) = harness();
        session.start().await.unwrap();
        let tx0 = connector.sockets.lock().unwrap()[0].1.clone();
        tx0.send(SocketEvent::Transcript(TranscriptEvent {
            text: "Before drop.".into(),
            is_final: true,
            speech_final: true,
            confidence: 1.0,
            speaker: None,
            channel: Some(Channel::Them),
        }))
        .unwrap();
        tx0.send(SocketEvent::Closed { by_us: false, reason: "network drop".into() }).unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert_eq!(connector.sockets.lock().unwrap().len(), 2);
        assert!(session.state().connected);
        let meeting = session.finish().unwrap().unwrap();
        assert_eq!(meeting.full_transcript_text, "Them: Before drop.");
    }

    #[tokio::test]
    async fn prefers_interviewer_them_utterance_over_user_filler_in_both_mode() {
        let (_dir, settings, _meetings, connector, session, _) = harness();
        session.start().await.unwrap();
        let tx = connector.sockets.lock().unwrap()[0].1.clone();

        tx.send(SocketEvent::Transcript(TranscriptEvent {
            text: "How does consistent hashing work?".into(),
            is_final: true,
            speech_final: true,
            confidence: 0.99,
            speaker: None,
            channel: Some(Channel::Them),
        }))
        .unwrap();
        tx.send(SocketEvent::Transcript(TranscriptEvent {
            text: "Good question, let me think.".into(),
            is_final: true,
            speech_final: true,
            confidence: 0.99,
            speaker: None,
            channel: Some(Channel::You),
        }))
        .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(session.last_utterance().unwrap().text, "How does consistent hashing work?");

        settings.update(SettingsPatch { audio_source: Some(AudioSource::Microphone), ..Default::default() }).unwrap();
        assert_eq!(session.last_utterance().unwrap().text, "Good question, let me think.");
        let _ = session.finish();
    }
}
