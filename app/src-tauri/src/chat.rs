//! Conversation history, manual chat, screenshot analysis, and question-gated auto-answering.

use crate::capture::CapturedScreen;
use crate::llm::{LlmClient, LlmMessage, LlmResult};
use crate::meetings::Channel;
use crate::session::{SessionManager, Utterance};
use crate::settings::{AnswerLength, AutoAnswer, Prompt, ProviderKind, SettingsStore, language_instruction};
use crate::transcript_files::now_ms;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub const SKIP_TOKEN: &str = "[no-answer]";
const MAX_HISTORY: usize = 40;
const HISTORY_IN_PROMPT: usize = 12;
const TRANSCRIPT_IN_PROMPT: usize = 4000;
const AUTO_ANSWER_MIN_LETTERS: usize = 4;

const ANSWER_FORMAT: &str = "Lay out every answer to be read at a glance while speaking: first the answer itself as one bold sentence, then 2 to 4 catch points as bullets the user can expand on out loud, each a fragment of 3 to 8 words, not a sentence. No headings.";

const SCREENSHOT_PROMPT: &str = "This is the user's current screen. Describe what matters and help with it: if it shows a question or a task, answer or solve it directly; if it shows code, explain or fix it; otherwise summarize what is on screen briefly.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    Text,
    Screenshot,
    Error,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub kind: ChatKind,
    pub text: String,
    pub timestamp: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChunk {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotResult {
    pub text: String,
    pub tokens: u64,
    pub dimensions: String,
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Message(ChatMessage),
    Chunk(ChatChunk),
    Busy(bool),
    Cleared,
    AutoAnswer(ChatMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Wait,
    Skip,
    Answer,
}

#[derive(Debug, Clone)]
pub struct BrainConfig {
    pub label: String,
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
}

pub type StreamFuture = Pin<Box<dyn Future<Output = Result<LlmResult, String>> + Send>>;

pub trait BrainBackend: Send + Sync {
    fn chat_stream(
        &self,
        config: BrainConfig,
        messages: Vec<LlmMessage>,
        on_delta: Box<dyn FnMut(&str) + Send>,
    ) -> StreamFuture;

    fn chat_once(&self, config: BrainConfig, messages: Vec<LlmMessage>) -> StreamFuture;

    fn vision(&self, config: BrainConfig, base64_jpeg: String, prompt: String, system: Option<String>) -> StreamFuture;
}

impl BrainBackend for LlmClient {
    fn chat_stream(
        &self,
        config: BrainConfig,
        messages: Vec<LlmMessage>,
        mut on_delta: Box<dyn FnMut(&str) + Send>,
    ) -> StreamFuture {
        let client = self.clone();
        Box::pin(async move {
            client
                .chat_stream(&config.endpoint, config.api_key.as_deref(), &config.model, &messages, move |d| on_delta(d))
                .await
                .map_err(|e| format!("{}: {e}", config.label))
        })
    }

    fn chat_once(&self, config: BrainConfig, messages: Vec<LlmMessage>) -> StreamFuture {
        let client = self.clone();
        Box::pin(async move {
            client
                .chat_once(&config.endpoint, config.api_key.as_deref(), &config.model, &messages, None)
                .await
                .map_err(|e| format!("{}: {e}", config.label))
        })
    }

    fn vision(&self, config: BrainConfig, base64_jpeg: String, prompt: String, system: Option<String>) -> StreamFuture {
        let client = self.clone();
        Box::pin(async move {
            client
                .vision(
                    &config.endpoint,
                    config.api_key.as_deref(),
                    &config.model,
                    &base64_jpeg,
                    &prompt,
                    system.as_deref(),
                )
                .await
                .map_err(|e| format!("{}: {e}", config.label))
        })
    }
}

pub type CaptureFuture = Pin<Box<dyn Future<Output = Result<CapturedScreen, String>> + Send>>;
pub type ScreenCapturer = Arc<dyn Fn() -> CaptureFuture + Send + Sync>;

struct ActiveProbe {
    superseded: AtomicBool,
}

struct Inner {
    history: Vec<ChatMessage>,
    busy: bool,
    active_task: Option<tokio::task::AbortHandle>,
    active_id: Option<String>,
    probe: Option<Arc<ActiveProbe>>,
    draining: bool,
    queued: Option<String>,
}

pub struct ChatService {
    settings: Arc<SettingsStore>,
    session: Arc<SessionManager>,
    backend: Arc<dyn BrainBackend>,
    capturer: ScreenCapturer,
    emit: Arc<dyn Fn(ChatEvent) + Send + Sync>,
    hide_for_capture: Arc<dyn Fn() -> Box<dyn FnOnce() + Send> + Send + Sync>,
    inner: Mutex<Inner>,
    drain_notify: Notify,
}

impl ChatService {
    pub fn new(
        settings: Arc<SettingsStore>,
        session: Arc<SessionManager>,
        backend: Arc<dyn BrainBackend>,
        capturer: ScreenCapturer,
        hide_for_capture: impl Fn() -> Box<dyn FnOnce() + Send> + Send + Sync + 'static,
        emit: impl Fn(ChatEvent) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            session,
            backend,
            capturer,
            emit: Arc::new(emit),
            hide_for_capture: Arc::new(hide_for_capture),
            inner: Mutex::new(Inner {
                history: Vec::new(),
                busy: false,
                active_task: None,
                active_id: None,
                probe: None,
                draining: false,
                queued: None,
            }),
            drain_notify: Notify::new(),
        })
    }

    pub fn is_busy(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).busy
    }

    pub fn history(&self) -> Vec<ChatMessage> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).history.clone()
    }

    pub fn clear(&self) {
        self.cancel();
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.history.clear();
        guard.queued = None;
        drop(guard);
        (self.emit)(ChatEvent::Cleared);
    }

    pub fn cancel(&self) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(probe) = guard.probe.take() {
            probe.superseded.store(true, Ordering::SeqCst);
        }
        let had_active = guard.active_task.take().is_some();
        if let Some(handle) = guard.active_task.take() {
            handle.abort();
        }
        let was_busy = guard.busy;
        guard.busy = false;
        guard.active_id = None;
        drop(guard);
        if was_busy {
            (self.emit)(ChatEvent::Busy(false));
        }
        had_active || was_busy
    }

    pub fn resolve_brain(&self) -> Result<BrainConfig, String> {
        let s = self.settings.get();
        let Some(id) = s.brain_id.clone() else {
            return Err("Configure and select a brain provider in Settings first.".into());
        };
        let Some(record) = s.provider(&id).cloned() else {
            return Err("The selected brain provider no longer exists.".into());
        };
        if record.kind != ProviderKind::OpenaiCompatible {
            return Err("The selected brain provider is not an OpenAI-compatible model.".into());
        }
        let api_key = self.settings.provider_key(&id)?;
        Ok(BrainConfig {
            label: record.label,
            endpoint: record.endpoint,
            model: record.model,
            api_key,
        })
    }

    pub async fn send(self: &Arc<Self>, text: String, prompt_override: Option<String>) -> Result<(), String> {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return Ok(());
        }
        {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.busy {
                return Err("Wait for the current answer to finish.".into());
            }
        }
        self.supersede_probe();
        self.push_message(None, ChatRole::User, ChatKind::Text, trimmed.clone());
        let user_content = prompt_override.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).unwrap_or(trimmed);
        let _ = self.reply(user_content, ChatKind::Text, None, false).await;
        self.trigger_drain();
        Ok(())
    }

    pub async fn answer_last(self: &Arc<Self>) -> Result<bool, String> {
        let Some(last) = self.session.last_utterance() else {
            return Ok(false);
        };
        if self.is_busy() {
            return Ok(false);
        }
        self.supersede_probe();
        let text = last.text.trim().to_string();
        if text.is_empty() {
            return Ok(false);
        }
        self.answer_utterance(text, false).await;
        self.trigger_drain();
        Ok(true)
    }

    pub async fn auto_answer(self: &Arc<Self>, utterance: Utterance) {
        if self.settings.get().auto_answer == AutoAnswer::Off {
            return;
        }
        if utterance.channel == Some(Channel::You) {
            return;
        }
        let text = utterance.text.trim();
        if letter_count(text) < AUTO_ANSWER_MIN_LETTERS {
            return;
        }
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.queued = Some(match guard.queued.take() {
                Some(prev) => format!("{prev} {text}"),
                None => text.to_string(),
            });
        }
        self.drain().await;
    }

    pub async fn screenshot(self: &Arc<Self>, prompt: Option<String>) -> Result<ScreenshotResult, String> {
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.busy {
                return Err("Wait for the current answer to finish.".into());
            }
            if let Some(probe) = guard.probe.take() {
                probe.superseded.store(true, Ordering::SeqCst);
            }
            guard.busy = true;
        }
        (self.emit)(ChatEvent::Busy(true));

        let restore = (self.hide_for_capture)();
        let shot_res = (self.capturer)().await;
        restore();

        let result = match shot_res {
            Ok(shot) => match self.resolve_brain() {
                Ok(brain) => {
                    let sys = self.system_prompt();
                    let ask = prompt.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).unwrap_or_else(|| SCREENSHOT_PROMPT.into());
                    let call = self.backend.vision(brain, shot.base64, ask, Some(sys));
                    let handle = tokio::spawn(call);
                    let abort = handle.abort_handle();
                    {
                        self.inner.lock().unwrap_or_else(|e| e.into_inner()).active_task = Some(abort);
                    }
                    match handle.await {
                        Ok(Ok(res)) => {
                            let display = if res.text.trim().is_empty() { "(no description)".into() } else { res.text.clone() };
                            self.push_message(None, ChatRole::Assistant, ChatKind::Screenshot, display);
                            Ok(ScreenshotResult {
                                text: res.text,
                                tokens: res.tokens,
                                dimensions: format!("{}x{}", shot.width, shot.height),
                            })
                        }
                        Ok(Err(err)) => {
                            self.push_message(None, ChatRole::Assistant, ChatKind::Error, err.clone());
                            Err(err)
                        }
                        Err(_) => Err("Screen analysis cancelled.".into()),
                    }
                }
                Err(err) => {
                    self.push_message(None, ChatRole::Assistant, ChatKind::Error, err.clone());
                    Err(err)
                }
            },
            Err(err) => {
                self.push_message(None, ChatRole::Assistant, ChatKind::Error, err.clone());
                Err(err)
            }
        };

        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.active_task = None;
            guard.busy = false;
        }
        (self.emit)(ChatEvent::Busy(false));
        self.trigger_drain();
        result
    }

    fn trigger_drain(self: &Arc<Self>) {
        let has_queued = self.inner.lock().unwrap_or_else(|e| e.into_inner()).queued.is_some();
        if has_queued {
            let this = self.clone();
            tokio::spawn(async move {
                this.drain().await;
            });
        }
    }

    async fn drain(self: &Arc<Self>) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.draining {
                return;
            }
            guard.draining = true;
        }

        loop {
            let next = {
                let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if guard.busy {
                    guard.draining = false;
                    return;
                }
                let mode = self.settings.get().auto_answer;
                if mode == AutoAnswer::Off {
                    guard.queued = None;
                    guard.draining = false;
                    return;
                }
                match guard.queued.take() {
                    Some(text) => (text, mode == AutoAnswer::Questions),
                    None => {
                        guard.draining = false;
                        return;
                    }
                }
            };
            self.answer_utterance(next.0, next.1).await;
            self.drain_notify.notify_waiters();
        }
    }

    fn supersede_probe(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(probe) = guard.probe.take() {
            probe.superseded.store(true, Ordering::SeqCst);
        }
    }

    async fn answer_utterance(self: &Arc<Self>, text: String, may_decline: bool) {
        let answer = "answer it directly: give the answer itself, ready to say out loud, never advice about how to answer";
        let ask = if may_decline {
            format!(
                "Heard in the meeting: \"{text}\"\n\nIf this asks the user something or calls for a reply from them (a question, a request, a task, in any language), {answer}. If it does not (small talk, a plain statement), reply with exactly {SKIP_TOKEN} and nothing else."
            )
        } else {
            format!("Heard in the meeting: \"{text}\"\n\nThe user wants an answer to this: {answer}.")
        };
        if let Some(message) = self.reply(ask, ChatKind::Auto, Some(text), may_decline).await {
            (self.emit)(ChatEvent::AutoAnswer(message));
        }
    }

    async fn reply(
        self: &Arc<Self>,
        user_content: String,
        kind: ChatKind,
        heard: Option<String>,
        gate: bool,
    ) -> Option<ChatMessage> {
        let brain = match self.resolve_brain() {
            Ok(b) => b,
            Err(err) => {
                if !gate {
                    self.push_message(None, ChatRole::Assistant, ChatKind::Error, err);
                } else {
                    eprintln!("[auto-answer] {err}");
                }
                return None;
            }
        };

        let messages = {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let history_slice = if heard.is_none() && !guard.history.is_empty() {
                &guard.history[..guard.history.len() - 1]
            } else {
                &guard.history[..]
            };
            let mut msgs = build_messages(&self.system_prompt(), history_slice, &self.session.transcript_text());
            msgs.push(LlmMessage::user(user_content));
            msgs
        };

        let id = new_id();
        let probe = gate.then(|| Arc::new(ActiveProbe { superseded: AtomicBool::new(false) }));

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum StreamGateState {
            Wait,
            Skip,
            Answer,
        }

        let shared_state = Arc::new(Mutex::new((
            if probe.is_some() { StreamGateState::Wait } else { StreamGateState::Answer },
            false, // committed
            String::new(), // buffered head
        )));

        if let Some(ref p) = probe {
            self.inner.lock().unwrap_or_else(|e| e.into_inner()).probe = Some(p.clone());
        } else {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.busy = true;
            guard.active_id = Some(id.clone());
            shared_state.lock().unwrap().1 = true;
            drop(guard);
            (self.emit)(ChatEvent::Busy(true));
        }

        let commit_visible = {
            let this = self.clone();
            let probe = probe.clone();
            let heard = heard.clone();
            let id = id.clone();
            Arc::new(move || -> bool {
                let mut guard = this.inner.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(ref p) = probe {
                    if p.superseded.load(Ordering::SeqCst) || guard.busy {
                        return false;
                    }
                    guard.probe = None;
                }
                guard.busy = true;
                guard.active_id = Some(id.clone());
                drop(guard);
                if let Some(ref h) = heard {
                    this.push_message(None, ChatRole::User, ChatKind::Auto, h.clone());
                }
                (this.emit)(ChatEvent::Busy(true));
                true
            })
        };

        let on_delta_state = shared_state.clone();
        let on_delta_commit = commit_visible.clone();
        let on_delta_emit = self.emit.clone();
        let on_delta_id = id.clone();

        let call = self.backend.chat_stream(
            brain,
            messages,
            Box::new(move |delta| {
                let mut st = on_delta_state.lock().unwrap();
                match st.0 {
                    StreamGateState::Skip => {}
                    StreamGateState::Wait => {
                        st.2.push_str(delta);
                        match gate_decision(&st.2) {
                            GateDecision::Wait => {}
                            GateDecision::Skip => {
                                st.0 = StreamGateState::Skip;
                            }
                            GateDecision::Answer => {
                                let head = std::mem::take(&mut st.2);
                                drop(st);
                                if on_delta_commit() {
                                    let mut st2 = on_delta_state.lock().unwrap();
                                    st2.0 = StreamGateState::Answer;
                                    st2.1 = true;
                                    drop(st2);
                                    (on_delta_emit)(ChatEvent::Chunk(ChatChunk { id: on_delta_id.clone(), text: head }));
                                } else {
                                    on_delta_state.lock().unwrap().0 = StreamGateState::Skip;
                                }
                            }
                        }
                    }
                    StreamGateState::Answer => {
                        drop(st);
                        (on_delta_emit)(ChatEvent::Chunk(ChatChunk {
                            id: on_delta_id.clone(),
                            text: delta.to_string(),
                        }));
                    }
                }
            }),
        );

        let handle = tokio::spawn(call);
        if probe.is_none() {
            self.inner.lock().unwrap_or_else(|e| e.into_inner()).active_task = Some(handle.abort_handle());
        }

        let outcome = handle.await;

        let (gate_state, committed) = {
            let st = shared_state.lock().unwrap();
            (st.0, st.1)
        };

        let result_msg = match outcome {
            Ok(Ok(res)) => {
                let final_committed = if gate_state != StreamGateState::Answer {
                    if gate_state == StreamGateState::Skip || gate_decision(&res.text) != GateDecision::Answer || !commit_visible() {
                        false
                    } else {
                        true
                    }
                } else {
                    committed
                };
                if final_committed {
                    let body = if res.text.trim().is_empty() { "(empty reply)".into() } else { res.text };
                    Some(self.push_message(Some(id.clone()), ChatRole::Assistant, kind, body))
                } else {
                    None
                }
            }
            Ok(Err(err)) => {
                // Never retry automatically after text appeared; surface the error clearly.
                if committed {
                    self.push_message(None, ChatRole::Assistant, ChatKind::Error, err);
                } else {
                    eprintln!("[auto-answer] failed: {err}");
                }
                None
            }
            Err(_) => None, // Aborted by user cancellation
        };

        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref p) = probe
                && guard.probe.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, p))
            {
                guard.probe = None;
            }
            if guard.active_id.as_deref() == Some(&id) {
                guard.active_task = None;
                guard.active_id = None;
                let was_busy = guard.busy;
                guard.busy = false;
                drop(guard);
                if was_busy {
                    (self.emit)(ChatEvent::Busy(false));
                }
            }
        }

        result_msg
    }

    fn system_prompt(&self) -> String {
        let s = self.settings.get();
        let prompt = s.active_prompt();
        compose_system_prompt(&prompt, s.answer_length, language_instruction(&s.output_language))
    }

    fn push_message(&self, id: Option<String>, role: ChatRole, kind: ChatKind, text: String) -> ChatMessage {
        let message = ChatMessage {
            id: id.unwrap_or_else(new_id),
            role,
            kind,
            text,
            timestamp: now_ms(),
        };
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.history.push(message.clone());
        if guard.history.len() > MAX_HISTORY {
            let drain = guard.history.len() - MAX_HISTORY;
            guard.history.drain(..drain);
        }
        drop(guard);
        (self.emit)(ChatEvent::Message(message.clone()));
        message
    }
}

pub fn compose_system_prompt(prompt: &Prompt, length: AnswerLength, lang_instruction: &str) -> String {
    let mut parts = vec![prompt.content.trim().to_string()];
    let notes = prompt.notes.trim();
    if !notes.is_empty() {
        parts.push(format!("Background about the user, use it whenever relevant:\n{notes}"));
    }
    parts.push(ANSWER_FORMAT.to_string());
    let len_instruction = match length {
        AnswerLength::Short => "Stop after the catch points.",
        AnswerLength::Medium => "After the catch points, add one short paragraph with the key details.",
        AnswerLength::Auto => "Usually stop after the catch points; add code or a few steps after them only when the question needs it.",
    };
    parts.push(len_instruction.to_string());
    parts.push(lang_instruction.to_string());
    parts.join("\n\n")
}

pub fn build_messages(system: &str, history: &[ChatMessage], transcript: &str) -> Vec<LlmMessage> {
    let mut out = vec![LlmMessage::system(system)];
    let clean_transcript = transcript.trim();
    if !clean_transcript.is_empty() {
        let tail: String = if clean_transcript.chars().count() > TRANSCRIPT_IN_PROMPT {
            let skip = clean_transcript.chars().count() - TRANSCRIPT_IN_PROMPT;
            clean_transcript.chars().skip(skip).collect()
        } else {
            clean_transcript.to_string()
        };
        out.push(LlmMessage::system(format!("Live meeting transcript (most recent):\n{tail}")));
    }
    let filtered: Vec<&ChatMessage> = history.iter().filter(|m| m.kind != ChatKind::Error).collect();
    let start = filtered.len().saturating_sub(HISTORY_IN_PROMPT);
    for msg in &filtered[start..] {
        let content = match msg.kind {
            ChatKind::Screenshot => format!("[Screenshot analysis]\n{}", msg.text),
            ChatKind::Auto if msg.role == ChatRole::User => format!("[Heard in the meeting] {}", msg.text),
            _ => msg.text.clone(),
        };
        out.push(match msg.role {
            ChatRole::User => LlmMessage::user(content),
            ChatRole::Assistant => LlmMessage::assistant(content),
        });
    }
    out
}

pub fn gate_decision(head: &str) -> GateDecision {
    let stripped = head.trim().trim_start_matches(['*', '_', '`']);
    if stripped.is_empty() {
        return GateDecision::Wait;
    }
    let lower = stripped.to_ascii_lowercase();
    if SKIP_TOKEN.starts_with(&lower) && lower.len() < SKIP_TOKEN.len() {
        return GateDecision::Wait;
    }
    if lower.starts_with(SKIP_TOKEN) {
        let rest = stripped[SKIP_TOKEN.len()..].trim_matches(['*', '_', '`', ' ', '\t', '\r', '\n']);
        if rest.is_empty() {
            return GateDecision::Skip;
        }
    }
    GateDecision::Answer
}

fn letter_count(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphanumeric()).count()
}

fn new_id() -> String {
    format!("m{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::MeetingStore;
    use crate::secrets::MemorySecrets;
    use crate::session::{CaptureConnector, ConnectFuture, TranscriptionConnector};
    use crate::settings::ProviderInput;
    use crate::storage::test_support::TempDir;

    struct DummyConnector;
    impl TranscriptionConnector for DummyConnector {
        fn connect(&self, _: crate::deepgram::DeepgramConfig) -> ConnectFuture {
            Box::pin(async { Err("unused".into()) })
        }
    }
    struct DummyCapture;
    impl CaptureConnector for DummyCapture {
        fn start(
            &self,
            _: crate::settings::AudioSource,
            _: &str,
            _: &str,
            _: bool,
            _: Arc<AtomicBool>,
            _: Box<dyn FnMut(Vec<u8>) + Send>,
            _: Box<dyn FnMut(crate::audio::AudioLevels) + Send>,
            _: Arc<dyn Fn(String) + Send + Sync>,
        ) -> Result<Box<dyn Send + Sync>, String> {
            Ok(Box::new(()))
        }
    }

    struct MockBackend {
        calls: Mutex<usize>,
        reply: Mutex<Box<dyn FnMut(Vec<LlmMessage>, Box<dyn FnMut(&str) + Send>) -> Result<LlmResult, String> + Send>>,
    }

    impl MockBackend {
        fn new(f: impl FnMut(Vec<LlmMessage>, Box<dyn FnMut(&str) + Send>) -> Result<LlmResult, String> + Send + 'static) -> Arc<Self> {
            Arc::new(Self { calls: Mutex::new(0), reply: Mutex::new(Box::new(f)) })
        }
    }

    impl BrainBackend for MockBackend {
        fn chat_stream(&self, _: BrainConfig, messages: Vec<LlmMessage>, on_delta: Box<dyn FnMut(&str) + Send>) -> StreamFuture {
            *self.calls.lock().unwrap() += 1;
            let res = (self.reply.lock().unwrap())(messages, on_delta);
            Box::pin(async move { res })
        }

        fn chat_once(&self, _: BrainConfig, _: Vec<LlmMessage>) -> StreamFuture {
            Box::pin(async { Ok(LlmResult { text: "OK".into(), tokens: 1 }) })
        }

        fn vision(&self, _: BrainConfig, _: String, _: String, _: Option<String>) -> StreamFuture {
            Box::pin(async { Ok(LlmResult { text: "A code editor.".into(), tokens: 10 }) })
        }
    }

    fn setup_chat(backend: Arc<dyn BrainBackend>) -> (TempDir, Arc<SettingsStore>, Arc<ChatService>, Arc<Mutex<Vec<ChatEvent>>>) {
        let dir = TempDir::new();
        let secrets = Arc::new(MemorySecrets::default());
        let settings = Arc::new(SettingsStore::open(dir.path(), secrets).unwrap());
        settings
            .save_provider(ProviderInput {
                id: None,
                kind: ProviderKind::OpenaiCompatible,
                label: "TestBrain".into(),
                endpoint: "https://api.example.com/v1".into(),
                model: "test-model".into(),
                key: Some("sk-1".into()),
                clear_key: false,
            })
            .unwrap();
        let meetings = Arc::new(MeetingStore::open(dir.path()).unwrap());
        let session = SessionManager::with_connectors(
            settings.clone(),
            meetings,
            dir.path().join("transcripts"),
            Arc::new(DummyConnector),
            Arc::new(DummyCapture),
            |_| {},
        );
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev_cb = events.clone();
        let chat = ChatService::new(
            settings.clone(),
            session,
            backend,
            Arc::new(|| Box::pin(async { Ok(CapturedScreen { base64: "abc".into(), width: 100, height: 100, bytes: 3 }) })),
            || Box::new(|| {}),
            move |ev| ev_cb.lock().unwrap().push(ev),
        );
        (dir, settings, chat, events)
    }

    #[test]
    fn gate_decision_waits_for_prefix_and_skips_marker() {
        assert_eq!(gate_decision(""), GateDecision::Wait);
        assert_eq!(gate_decision("**[no-"), GateDecision::Wait);
        assert_eq!(gate_decision("[no-answer]"), GateDecision::Skip);
        assert_eq!(gate_decision("**[no-answer]**"), GateDecision::Skip);
        assert_eq!(gate_decision("**Kafka is a log.**"), GateDecision::Answer);
    }

    #[tokio::test]
    async fn never_auto_answers_you_channel_and_skips_non_questions() {
        let backend = MockBackend::new(|_, mut on_delta| {
            on_delta("[no-answer]");
            Ok(LlmResult { text: "[no-answer]".into(), tokens: 2 })
        });
        let (_dir, _settings, chat, events) = setup_chat(backend.clone());
        chat.auto_answer(Utterance {
            text: "What is Kafka in my opinion?".into(),
            speaker: None,
            channel: Some(Channel::You),
            timestamp: 0.0,
        })
        .await;
        assert_eq!(*backend.calls.lock().unwrap(), 0);

        chat.auto_answer(Utterance {
            text: "We moved the standup to Thursday.".into(),
            speaker: None,
            channel: Some(Channel::Them),
            timestamp: 1.0,
        })
        .await;
        assert_eq!(*backend.calls.lock().unwrap(), 1);
        assert!(chat.history().is_empty());
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn partial_stream_failure_surfaces_provider_name_and_never_retries() {
        let backend = MockBackend::new(|_, mut on_delta| {
            on_delta("First partial ");
            Err("TestBrain: stream interrupted".into())
        });
        let (_dir, _settings, chat, _events) = setup_chat(backend.clone());
        chat.send("Explain Raft".into(), None).await.unwrap();
        assert_eq!(*backend.calls.lock().unwrap(), 1);
        let hist = chat.history();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[1].kind, ChatKind::Error);
        assert!(hist[1].text.contains("TestBrain"));
    }

    #[tokio::test]
    async fn streams_auto_answer_for_them_question() {
        let backend = MockBackend::new(|_, mut on_delta| {
            on_delta("**Raft uses ");
            on_delta("leader election.**");
            Ok(LlmResult { text: "**Raft uses leader election.**".into(), tokens: 12 })
        });
        let (_dir, _settings, chat, events) = setup_chat(backend);
        chat.auto_answer(Utterance {
            text: "How does Raft elect a leader?".into(),
            speaker: None,
            channel: Some(Channel::Them),
            timestamp: 1.0,
        })
        .await;
        let hist = chat.history();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].kind, ChatKind::Auto);
        assert_eq!(hist[1].role, ChatRole::Assistant);
        assert!(events.lock().unwrap().iter().any(|e| matches!(e, ChatEvent::AutoAnswer(_))));
    }
}
