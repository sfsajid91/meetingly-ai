//! Live conversation suggestions rail.

use crate::chat::{BrainBackend, ChatKind, ChatRole, ChatService};
use crate::llm::LlmMessage;
use crate::session::SessionManager;
use crate::settings::{SettingsStore, language_instruction};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_ITEMS: usize = 4;
const MAX_CHARS: usize = 140;
const TRANSCRIPT_IN_PROMPT: usize = 3000;
const MAX_CONTEXT_CHARS: usize = 500;
const RECENT_CHARS: usize = 600;
const DEBOUNCE: Duration = Duration::from_secs(2);
const MIN_INTERVAL: Duration = Duration::from_secs(20);
const FIRST_RUN_CHARS: usize = 80;
const GROWTH_CHARS: usize = 120;
const BACKOFF: Duration = Duration::from_secs(60);

const SUGGEST_PROMPT: &str = "You help someone in a live conversation (a job interview, a sales call, a meeting) by suggesting questions for their AI assistant to answer right now. \"Them\" is the other side, \"You\" is the user. A click on a suggestion shows the AI's answer, so every suggestion is a question with a real answer. Do not suggest questions for the user to ask Them.

Suggest up to 4, most useful first, each at most 14 words and clear on its own:
1. If Them just asked the user something (not small talk), the first suggestion is that question, restated as a direct question with \"it\" or \"that\" spelled out.
2. A term, technology, name or number that just came up: \"What is ...?\", \"How does ... work?\".
3. The question Them is most likely to ask next, so its answer is ready.

Ask the question itself. Never coach: no \"How should I explain ...\", \"What should I mention ...\", \"How can I describe ...\".
Work from the NEW part of the transcript. Repeat an earlier suggestion only while its topic is still being discussed, and never suggest something already answered on screen.

Also return:
- \"topic\": what is being discussed right now, at most 6 words.
- \"context\": at most 60 words of lasting facts: who Them is, their company and product, the role or deal, what the user said about themselves, topics already covered. Update the previous context; keep every fact that still matters.

Reply with JSON only: {\"topic\": \"...\", \"context\": \"...\", \"questions\": [\"...\", \"...\"]}";

pub const QUICK_ACTIONS: &[(&str, &str, &str)] = &[
    (
        "say",
        "What should I say?",
        "Based on the conversation so far, what should I say next? Give the exact words to say, ready to say out loud, not advice about what to say.",
    ),
    (
        "ask",
        "Questions to ask",
        "Suggest three sharp questions I could ask the other side right now, specific to this conversation.",
    ),
    (
        "recap",
        "Recap so far",
        "Recap the conversation so far: what was discussed, what was decided, what is still open.",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionsState {
    pub topic: String,
    pub items: Vec<Suggestion>,
    pub updating: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSuggestions {
    pub topic: String,
    pub context: String,
    pub questions: Vec<String>,
}

enum BgTask {
    Tokio(tokio::task::JoinHandle<()>),
    Tauri(tauri::async_runtime::JoinHandle<()>),
}

impl BgTask {
    fn abort(&self) {
        match self {
            Self::Tokio(h) => h.abort(),
            Self::Tauri(h) => h.abort(),
        }
    }
}

fn spawn_bg(fut: impl std::future::Future<Output = ()> + Send + 'static) -> BgTask {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        BgTask::Tokio(handle.spawn(fut))
    } else {
        BgTask::Tauri(tauri::async_runtime::spawn(fut))
    }
}

struct Inner {
    topic: String,
    context: String,
    items: Vec<Suggestion>,
    used: HashSet<String>,
    updating: bool,
    again: bool,
    timer: Option<BgTask>,
    last_run: Option<Instant>,
    seen_chars: usize,
    failed_at: Option<Instant>,
    generation: u64,
}

pub struct SuggestionService {
    settings: Arc<SettingsStore>,
    session: Arc<SessionManager>,
    chat: Arc<ChatService>,
    backend: Arc<dyn BrainBackend>,
    emit: Arc<dyn Fn(SuggestionsState) + Send + Sync>,
    inner: Mutex<Inner>,
}

impl SuggestionService {
    pub fn new(
        settings: Arc<SettingsStore>,
        session: Arc<SessionManager>,
        chat: Arc<ChatService>,
        backend: Arc<dyn BrainBackend>,
        emit: impl Fn(SuggestionsState) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            session,
            chat,
            backend,
            emit: Arc::new(emit),
            inner: Mutex::new(Inner {
                topic: String::new(),
                context: String::new(),
                items: Vec::new(),
                used: HashSet::new(),
                updating: false,
                again: false,
                timer: None,
                last_run: None,
                seen_chars: 0,
                failed_at: None,
                generation: 0,
            }),
        })
    }

    pub fn state(&self) -> SuggestionsState {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        SuggestionsState {
            topic: g.topic.clone(),
            items: g.items.clone(),
            updating: g.updating,
        }
    }

    pub fn on_utterance(self: &Arc<Self>) {
        if !self.settings.get().suggestions {
            return;
        }
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.updating {
            g.again = true;
        } else {
            drop(g);
            self.schedule(DEBOUNCE);
        }
    }

    pub async fn refresh(self: &Arc<Self>) {
        self.cancel_timer();
        self.run(true).await;
    }

    pub fn reset(&self) {
        self.cancel_timer();
        let emit_needed = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.topic.is_empty() && g.items.is_empty() && !g.updating && g.seen_chars == 0 {
                false
            } else {
                g.generation += 1;
                g.topic.clear();
                g.context.clear();
                g.items.clear();
                g.used.clear();
                g.updating = false;
                g.again = false;
                g.last_run = None;
                g.seen_chars = 0;
                g.failed_at = None;
                true
            }
        };
        if emit_needed {
            (self.emit)(self.state());
        }
    }

    pub fn use_item(self: &Arc<Self>, id: &str) -> bool {
        if self.chat.is_busy() {
            return false;
        }
        if let Some((_, label, prompt)) = QUICK_ACTIONS.iter().find(|(qid, _, _)| *qid == id) {
            let chat = self.chat.clone();
            let label = (*label).to_string();
            let prompt = (*prompt).to_string();
            spawn_bg(async move {
                let _ = chat.send(label, Some(prompt)).await;
            });
            return true;
        }
        let chosen = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let Some(pos) = g.items.iter().position(|s| s.id == id) else {
                return false;
            };
            let item = g.items.remove(pos);
            g.used.insert(norm_key(&item.text));
            item.text
        };
        (self.emit)(self.state());
        let chat = self.chat.clone();
        spawn_bg(async move {
            let _ = chat.send(chosen, None).await;
        });
        true
    }

    fn cancel_timer(&self) {
        if let Some(handle) = self.inner.lock().unwrap_or_else(|e| e.into_inner()).timer.take() {
            handle.abort();
        }
    }

    fn schedule(self: &Arc<Self>, delay: Duration) {
        self.cancel_timer();
        let this = self.clone();
        let handle = spawn_bg(async move {
            tokio::time::sleep(delay).await;
            this.inner.lock().unwrap_or_else(|e| e.into_inner()).timer = None;
            this.run(false).await;
        });
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).timer = Some(handle);
    }

    async fn run(self: &Arc<Self>, force: bool) {
        if !self.settings.get().suggestions {
            return;
        }
        let total_chars = self.session.transcript_char_estimate();
        if total_chars == 0 {
            return;
        }
        if !force {
            let wait = {
                let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if g.updating {
                    g.again = true;
                    return;
                }
                let grown = total_chars.saturating_sub(g.seen_chars);
                let min_growth = if g.last_run.is_none() { FIRST_RUN_CHARS } else { GROWTH_CHARS };
                if grown < min_growth {
                    return;
                }
                let now = Instant::now();
                let wait_interval = g.last_run.map(|t| (t + MIN_INTERVAL).saturating_duration_since(now)).unwrap_or_default();
                let wait_backoff = g.failed_at.map(|t| (t + BACKOFF).saturating_duration_since(now)).unwrap_or_default();
                wait_interval.max(wait_backoff)
            };
            if !wait.is_zero() {
                self.schedule(wait);
                return;
            }
        }
        let window = self.session.recent_transcript_text(TRANSCRIPT_IN_PROMPT);
        if window.trim().is_empty() {
            return;
        }
        let (generation, messages) = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.updating {
                g.again = true;
                return;
            }
            g.updating = true;
            g.again = false;
            g.last_run = Some(Instant::now());
            let since = g.seen_chars;
            g.seen_chars = total_chars;
            let msgs = self.build_prompt_messages(&g, &window, total_chars, since);
            (g.generation, msgs)
        };
        (self.emit)(self.state());

        let outcome = match self.chat.resolve_brain() {
            Ok(brain) => self.backend.chat_once(brain, messages).await,
            Err(err) => Err(err),
        };

        let rerun = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.generation != generation {
                return;
            }
            match outcome {
                Ok(res) => {
                    if let Some(parsed) = parse_suggestions(&res.text) {
                        if !parsed.topic.is_empty() {
                            g.topic = parsed.topic;
                        }
                        if !parsed.context.is_empty() {
                            g.context = parsed.context;
                        }
                        let filtered: Vec<String> = parsed.questions.into_iter().filter(|q| !g.used.contains(&norm_key(q))).collect();
                        g.items = merge_suggestions(&g.items, filtered);
                    }
                    g.failed_at = None;
                }
                Err(err) => {
                    g.failed_at = Some(Instant::now());
                    eprintln!("[suggestions] failed: {err}");
                }
            }
            g.updating = false;
            let again = g.again;
            g.again = false;
            again
        };
        (self.emit)(self.state());
        if rerun {
            self.schedule(DEBOUNCE);
        }
    }

    fn build_prompt_messages(&self, g: &Inner, window: &str, total_chars: usize, since: usize) -> Vec<LlmMessage> {
        let s = self.settings.get();
        let prompt = s.active_prompt();
        let mut system = vec![SUGGEST_PROMPT.to_string()];
        if !prompt.notes.trim().is_empty() {
            system.push(format!("Background about the user:\n{}", prompt.notes.trim()));
        }
        system.push(format!(
            "Write the topic, the context and the questions in the language this names: {}",
            language_instruction(&s.output_language)
        ));

        let win_chars = window.chars().count();
        let fresh_chars = if since > 0 && since < total_chars {
            (total_chars - since).min(win_chars)
        } else {
            RECENT_CHARS.min(win_chars)
        };
        let split_chars = win_chars.saturating_sub(fresh_chars);
        let split_byte = window.char_indices().nth(split_chars).map_or(window.len(), |(idx, _)| idx);
        let earlier = &window[..split_byte];
        let fresh = &window[split_byte..];

        let answered: Vec<String> = self
            .chat
            .history()
            .into_iter()
            .filter(|m| m.role == ChatRole::User && m.kind != ChatKind::Error)
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|m| format!("- {}", m.text.chars().take(160).collect::<String>()))
            .collect();

        let earlier_items = if g.items.is_empty() {
            "(none)".to_string()
        } else {
            g.items.iter().map(|s| format!("- {}", s.text)).collect::<Vec<_>>().join("\n")
        };
        let mut parts = vec![
            format!("What you know so far: {}", if g.context.is_empty() { "(nothing yet)" } else { &g.context }),
            format!("Your earlier suggestions:\n{earlier_items}"),
        ];
        if !answered.is_empty() {
            parts.push(format!("Already answered on screen:\n{}", answered.join("\n")));
        }
        if !earlier.trim().is_empty() {
            parts.push(format!("Transcript, earlier part:\n{}", earlier.trim()));
        }
        parts.push(format!("Transcript, NEW since your last suggestions:\n{}", fresh.trim()));

        vec![LlmMessage::system(system.join("\n\n")), LlmMessage::user(parts.join("\n\n"))]
    }
}

#[derive(Deserialize)]
struct RawSuggestions {
    topic: Option<serde_json::Value>,
    context: Option<serde_json::Value>,
    questions: Option<serde_json::Value>,
}

pub fn parse_suggestions(text: &str) -> Option<ParsedSuggestions> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    let raw: RawSuggestions = serde_json::from_str(&text[start..=end]).ok()?;
    let list = raw.questions?.as_array()?.clone();
    let mut seen = HashSet::new();
    let mut questions = Vec::new();
    for item in list {
        let Some(s) = item.as_str() else { continue };
        let clean = squash(s);
        if clean.is_empty() || !seen.insert(norm_key(&clean)) {
            continue;
        }
        let capped = if clean.chars().count() > MAX_CHARS {
            let prefix: String = clean.chars().take(MAX_CHARS - 1).collect();
            format!("{prefix}…")
        } else {
            clean
        };
        questions.push(capped);
        if questions.len() == MAX_ITEMS {
            break;
        }
    }
    let topic = raw
        .topic
        .and_then(|v| v.as_str().map(squash))
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect();
    let context = raw
        .context
        .and_then(|v| v.as_str().map(squash))
        .unwrap_or_default()
        .chars()
        .take(MAX_CONTEXT_CHARS)
        .collect();
    Some(ParsedSuggestions { topic, context, questions })
}

pub fn merge_suggestions(previous: &[Suggestion], questions: Vec<String>) -> Vec<Suggestion> {
    let ids: HashMap<String, String> = previous.iter().map(|s| (norm_key(&s.text), s.id.clone())).collect();
    questions
        .into_iter()
        .map(|text| {
            let id = ids.get(&norm_key(&text)).cloned().unwrap_or_else(|| format!("q{}", uuid::Uuid::new_v4().simple()));
            Suggestion { id, text }
        })
        .collect()
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn norm_key(text: &str) -> String {
    squash(text).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json_and_deduplicates_and_truncates() {
        let json = r#"Here: ```json {"topic":"  Kafka   internals ","questions":["What is ISR?","what is isr?","",7,"A?","B?","C?","D?"]} ```"#;
        let parsed = parse_suggestions(json).unwrap();
        assert_eq!(parsed.topic, "Kafka internals");
        assert_eq!(parsed.questions, vec!["What is ISR?", "A?", "B?", "C?"]);

        let long = format!(r#"{{"topic":"x","questions":["{}"]}}"#, "q".repeat(200));
        let parsed_long = parse_suggestions(&long).unwrap();
        assert_eq!(parsed_long.questions[0].chars().count(), MAX_CHARS);
        assert!(parsed_long.questions[0].ends_with('…'));
    }

    #[test]
    fn merges_suggestions_preserving_ids() {
        let prev = vec![Suggestion { id: "id-1".into(), text: "What is Kafka?".into() }];
        let next = merge_suggestions(&prev, vec!["what is kafka?".into(), "What is Flink?".into()]);
        assert_eq!(next[0].id, "id-1");
        assert_ne!(next[1].id, "id-1");
    }
}
