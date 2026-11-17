//! Local settings: preferences, prompts, shortcuts and provider records. Secrets are in the keychain (`secrets.rs`).

use crate::secrets::{SecretError, SecretStore};
use crate::storage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const SCHEMA_VERSION: u32 = 1;

pub const LANGUAGE_CODES: &[&str] = &[
    "en", "ar", "bg", "zh", "hr", "cs", "da", "nl", "et", "fi", "fr", "de", "hi", "hu", "it", "ja", "ko", "no", "pl",
    "pt", "ro", "ru", "sk", "es", "sv", "tr", "uk", "vi",
];

pub fn language_instruction(code: &str) -> &'static str {
    match code {
        "ar" => "Respond in Arabic (العربية).",
        "bg" => "Respond in Bulgarian (Български).",
        "zh" => "Respond in Chinese (中文).",
        "hr" => "Respond in Croatian (Hrvatski).",
        "cs" => "Respond in Czech (Čeština).",
        "da" => "Respond in Danish (Dansk).",
        "nl" => "Respond in Dutch (Nederlands).",
        "et" => "Respond in Estonian (Eesti).",
        "fi" => "Respond in Finnish (Suomi).",
        "fr" => "Respond in French (Français).",
        "de" => "Respond in German (Deutsch).",
        "hi" => "Respond in Hindi (हिन्दी).",
        "hu" => "Respond in Hungarian (Magyar).",
        "it" => "Respond in Italian (Italiano).",
        "ja" => "Respond in Japanese (日本語).",
        "ko" => "Respond in Korean (한국어).",
        "no" => "Respond in Norwegian Bokmål (Norsk).",
        "pl" => "Respond in Polish (Polski).",
        "pt" => "Respond in Portuguese (Português).",
        "ro" => "Respond in Romanian (Română).",
        "ru" => "Respond in Russian (Русский язык).",
        "sk" => "Respond in Slovak (Slovenčina).",
        "es" => "Respond in Spanish (Español).",
        "sv" => "Respond in Swedish (Svenska).",
        "tr" => "Respond in Turkish (Türkçe).",
        "uk" => "Respond in Ukrainian (Українська).",
        "vi" => "Respond in Vietnamese (Tiếng Việt).",
        _ => "Respond in English.",
    }
}

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a real-time assistant for meetings and interviews. Give the actual answer to what was asked, the way a knowledgeable person would say it out loud.

- Answer the question itself. Never coach or describe how to answer.
- If the question is about the user, answer in the first person as the user, from their background and what they said earlier. Put a short [placeholder] for a personal detail you don't know.
- No preamble, filler or closing question. Spoken language with contractions.
- For code, give the code with brief comments.
- If the question is ambiguous, answer the most likely meaning.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AutoAnswer {
    Off,
    /// Default for new configurations: answer when `them` asks a question.
    #[default]
    Questions,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioSource {
    #[default]
    Both,
    Microphone,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnswerLength {
    Short,
    Medium,
    #[default]
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub content: String,
    /// Background the assistant should know (CV, job description, product facts).
    #[serde(default)]
    pub notes: String,
}

impl Prompt {
    pub fn default_prompt() -> Self {
        Self { id: "default".into(), title: "Meeting assistant".into(), content: DEFAULT_SYSTEM_PROMPT.into(), notes: String::new() }
    }
}

/// Global shortcuts. The Electron build's web "dashboard" shortcut is gone with the dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Shortcuts {
    pub show_toolbar: String,
    pub toggle_all: String,
    pub listen: String,
    pub chat: String,
    pub screenshot: String,
    pub move_left: String,
    pub move_right: String,
    pub move_up: String,
    pub move_down: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            show_toolbar: "CommandOrControl+Space".into(),
            toggle_all: "CommandOrControl+B".into(),
            listen: "CommandOrControl+L".into(),
            chat: "CommandOrControl+Shift+C".into(),
            screenshot: "CommandOrControl+Enter".into(),
            move_left: "CommandOrControl+Left".into(),
            move_right: "CommandOrControl+Right".into(),
            move_up: "CommandOrControl+Up".into(),
            move_down: "CommandOrControl+Down".into(),
        }
    }
}

impl Shortcuts {
    fn normalized(mut self) -> Self {
        let defaults = Shortcuts::default();
        for (value, fallback) in [
            (&mut self.show_toolbar, defaults.show_toolbar),
            (&mut self.toggle_all, defaults.toggle_all),
            (&mut self.listen, defaults.listen),
            (&mut self.chat, defaults.chat),
            (&mut self.screenshot, defaults.screenshot),
            (&mut self.move_left, defaults.move_left),
            (&mut self.move_right, defaults.move_right),
            (&mut self.move_up, defaults.move_up),
            (&mut self.move_down, defaults.move_down),
        ] {
            let trimmed = value.trim();
            *value = if trimmed.is_empty() { fallback } else { trimmed.to_string() };
        }
        self
    }
}

/// What a provider does in a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderRole {
    Transcription,
    Brain,
}

/// A supported provider API. Each kind has exactly one role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// Deepgram live streaming (WebSocket).
    Deepgram,
    /// Any OpenAI-compatible chat completions endpoint (OpenAI, Gemini AI Studio, OpenRouter, Ollama, ...).
    OpenaiCompatible,
}

impl ProviderKind {
    pub fn role(self) -> ProviderRole {
        match self {
            ProviderKind::Deepgram => ProviderRole::Transcription,
            ProviderKind::OpenaiCompatible => ProviderRole::Brain,
        }
    }

    fn validate_endpoint(self, endpoint: &str) -> Result<(), String> {
        let lower = endpoint.to_ascii_lowercase();
        let local = |rest: &str| {
            let host = rest.split(['/', '?']).next().unwrap_or("");
            let host = host.rsplit_once(':').map_or(host, |(h, port)| if port.chars().all(|c| c.is_ascii_digit()) { h } else { host });
            matches!(host, "localhost" | "127.0.0.1" | "[::1]")
        };
        let ok = match self {
            ProviderKind::Deepgram => lower.strip_prefix("wss://").is_some_and(|rest| !rest.is_empty()),
            ProviderKind::OpenaiCompatible => {
                lower.strip_prefix("https://").is_some_and(|rest| !rest.is_empty())
                    || lower.strip_prefix("http://").is_some_and(local)
            }
        };
        if ok {
            Ok(())
        } else {
            Err(match self {
                ProviderKind::Deepgram => "A Deepgram endpoint must be a wss:// URL.".into(),
                ProviderKind::OpenaiCompatible => {
                    "An OpenAI-compatible endpoint must use https://, or http:// only for localhost.".into()
                }
            })
        }
    }
}

/// A configured provider as stored on disk. The key itself is in the keychain under `key_ref`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecord {
    pub id: String,
    pub kind: ProviderKind,
    pub label: String,
    pub endpoint: String,
    pub model: String,
    /// Keychain account holding the API key; `None` for keyless endpoints such as a local Ollama.
    #[serde(default)]
    pub key_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub output_language: String,
    pub audio_language: String,
    pub audio_source: AudioSource,
    /// CPAL input device name; empty until chosen.
    pub microphone_id: String,
    /// PulseAudio/PipeWire monitor source name; empty until chosen.
    pub monitor_id: String,
    pub denoise_mic: bool,
    pub auto_answer: AutoAnswer,
    pub answer_length: AnswerLength,
    pub suggestions: bool,
    pub stealth: bool,
    pub auto_launch: bool,
    pub prompts: Vec<Prompt>,
    pub active_prompt_id: Option<String>,
    pub shortcuts: Shortcuts,
    pub providers: Vec<ProviderRecord>,
    pub transcriber_id: Option<String>,
    pub brain_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let prompt = Prompt::default_prompt();
        Self {
            schema_version: SCHEMA_VERSION,
            output_language: "en".into(),
            audio_language: "en".into(),
            audio_source: AudioSource::Both,
            microphone_id: String::new(),
            monitor_id: String::new(),
            denoise_mic: true,
            auto_answer: AutoAnswer::Questions,
            answer_length: AnswerLength::Auto,
            suggestions: false,
            stealth: true,
            auto_launch: false,
            active_prompt_id: Some(prompt.id.clone()),
            prompts: vec![prompt],
            shortcuts: Shortcuts::default(),
            providers: Vec::new(),
            transcriber_id: None,
            brain_id: None,
        }
    }
}

impl Settings {
    /// Repairs anything a hand edit or an older build could leave inconsistent.
    pub fn normalized(mut self) -> Self {
        self.schema_version = SCHEMA_VERSION;
        for language in [&mut self.output_language, &mut self.audio_language] {
            if !LANGUAGE_CODES.contains(&language.as_str()) {
                *language = "en".into();
            }
        }
        let mut seen = std::collections::HashSet::new();
        self.prompts.retain(|p| !p.id.trim().is_empty() && seen.insert(p.id.clone()));
        if self.prompts.is_empty() {
            self.prompts.push(Prompt::default_prompt());
        }
        if !self.active_prompt_id.as_ref().is_some_and(|id| self.prompts.iter().any(|p| &p.id == id)) {
            self.active_prompt_id = Some(self.prompts[0].id.clone());
        }
        self.shortcuts = self.shortcuts.normalized();
        let mut seen = std::collections::HashSet::new();
        self.providers.retain(|p| !p.id.is_empty() && seen.insert(p.id.clone()) && p.kind.validate_endpoint(&p.endpoint).is_ok());
        let selected = |id: &Option<String>, role: ProviderRole, providers: &[ProviderRecord]| {
            id.clone().filter(|id| providers.iter().any(|p| &p.id == id && p.kind.role() == role))
        };
        self.transcriber_id = selected(&self.transcriber_id, ProviderRole::Transcription, &self.providers);
        self.brain_id = selected(&self.brain_id, ProviderRole::Brain, &self.providers);
        self
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderRecord> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn active_prompt(&self) -> Prompt {
        self.active_prompt_id
            .as_ref()
            .and_then(|id| self.prompts.iter().find(|p| &p.id == id))
            .or_else(|| self.prompts.first())
            .cloned()
            .unwrap_or_else(Prompt::default_prompt)
    }
}

/// A provider as the webview sees it: whether a key exists, never the key or its keychain reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub kind: ProviderKind,
    pub role: ProviderRole,
    pub label: String,
    pub endpoint: String,
    pub model: String,
    pub has_key: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub output_language: String,
    pub audio_language: String,
    pub audio_source: AudioSource,
    pub microphone_id: String,
    pub monitor_id: String,
    pub denoise_mic: bool,
    pub auto_answer: AutoAnswer,
    pub answer_length: AnswerLength,
    pub suggestions: bool,
    pub stealth: bool,
    pub auto_launch: bool,
    pub prompts: Vec<Prompt>,
    pub active_prompt_id: Option<String>,
    pub shortcuts: Shortcuts,
    pub providers: Vec<ProviderView>,
    pub transcriber_id: Option<String>,
    pub brain_id: Option<String>,
    /// Set when the settings file is from a newer build and changes cannot be saved.
    pub read_only_reason: Option<String>,
}

/// Preference changes from the webview. Providers are changed only through the provider operations.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub output_language: Option<String>,
    pub audio_language: Option<String>,
    pub audio_source: Option<AudioSource>,
    pub microphone_id: Option<String>,
    pub monitor_id: Option<String>,
    pub denoise_mic: Option<bool>,
    pub auto_answer: Option<AutoAnswer>,
    pub answer_length: Option<AnswerLength>,
    pub suggestions: Option<bool>,
    pub stealth: Option<bool>,
    pub auto_launch: Option<bool>,
    pub prompts: Option<Vec<Prompt>>,
    pub active_prompt_id: Option<String>,
    pub shortcuts: Option<Shortcuts>,
}

/// Adds a provider (no `id`) or edits one. `key`: `None` keeps the saved key, `Some("")` with `clear_key` removes it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderInput {
    pub id: Option<String>,
    pub kind: ProviderKind,
    pub label: String,
    pub endpoint: String,
    pub model: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub clear_key: bool,
}

pub struct SettingsStore {
    file: PathBuf,
    secrets: Arc<dyn SecretStore>,
    state: Mutex<State>,
}

struct State {
    settings: Settings,
    read_only_reason: Option<String>,
}

fn key_account(provider_id: &str) -> String {
    format!("provider:{provider_id}")
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

impl SettingsStore {
    pub const FILE_NAME: &'static str = "settings.json";

    /// Loads `dir/settings.json`. A corrupt file is moved aside and replaced by defaults; a file from a newer build is
    /// loaded read-only so it is never downgraded.
    pub fn open(dir: &Path, secrets: Arc<dyn SecretStore>) -> Result<Self, String> {
        let file = dir.join(Self::FILE_NAME);
        let (settings, read_only_reason) = match storage::read_json::<serde_json::Value>(&file) {
            Ok(None) => (Settings::default(), None),
            Ok(Some(value)) => {
                let version = value.get("schemaVersion").and_then(|v| v.as_u64()).unwrap_or(0);
                match serde_json::from_value::<Settings>(value) {
                    Ok(settings) if version > SCHEMA_VERSION as u64 => (
                        settings.normalized(),
                        Some(format!("Settings were saved by a newer Meetingly (schema {version}); changes cannot be saved.")),
                    ),
                    Ok(settings) => (settings.normalized(), None),
                    Err(error) => {
                        let moved = storage::quarantine(&file)?;
                        eprintln!("[settings] unreadable settings moved to {}: {error}", moved.display());
                        (Settings::default(), None)
                    }
                }
            }
            Err(error) => {
                let moved = storage::quarantine(&file)?;
                eprintln!("[settings] unreadable settings moved to {}: {error}", moved.display());
                (Settings::default(), None)
            }
        };
        Ok(Self { file, secrets, state: Mutex::new(State { settings, read_only_reason }) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self) -> Settings {
        self.lock().settings.clone()
    }

    pub fn view(&self) -> SettingsView {
        let state = self.lock();
        let s = &state.settings;
        SettingsView {
            output_language: s.output_language.clone(),
            audio_language: s.audio_language.clone(),
            audio_source: s.audio_source,
            microphone_id: s.microphone_id.clone(),
            monitor_id: s.monitor_id.clone(),
            denoise_mic: s.denoise_mic,
            auto_answer: s.auto_answer,
            answer_length: s.answer_length,
            suggestions: s.suggestions,
            stealth: s.stealth,
            auto_launch: s.auto_launch,
            prompts: s.prompts.clone(),
            active_prompt_id: s.active_prompt_id.clone(),
            shortcuts: s.shortcuts.clone(),
            providers: s
                .providers
                .iter()
                .map(|p| ProviderView {
                    id: p.id.clone(),
                    kind: p.kind,
                    role: p.kind.role(),
                    label: p.label.clone(),
                    endpoint: p.endpoint.clone(),
                    model: p.model.clone(),
                    has_key: p.key_ref.is_some(),
                })
                .collect(),
            transcriber_id: s.transcriber_id.clone(),
            brain_id: s.brain_id.clone(),
            read_only_reason: state.read_only_reason.clone(),
        }
    }

    /// Applies `change` to a copy and persists it; memory changes only after the write succeeds.
    fn commit<T>(&self, change: impl FnOnce(&mut Settings) -> Result<T, String>) -> Result<T, String> {
        let mut state = self.lock();
        if let Some(reason) = &state.read_only_reason {
            return Err(reason.clone());
        }
        let mut next = state.settings.clone();
        let result = change(&mut next)?;
        let next = next.normalized();
        storage::write_json(&self.file, &next)?;
        state.settings = next;
        Ok(result)
    }

    /// Replaces the whole document. Used by migration; not exposed to the webview.
    pub fn replace(&self, settings: Settings) -> Result<(), String> {
        self.commit(|current| {
            *current = settings;
            Ok(())
        })
    }

    pub fn update(&self, patch: SettingsPatch) -> Result<(), String> {
        self.commit(|s| {
            if let Some(v) = patch.output_language {
                s.output_language = v;
            }
            if let Some(v) = patch.audio_language {
                s.audio_language = v;
            }
            if let Some(v) = patch.audio_source {
                s.audio_source = v;
            }
            if let Some(v) = patch.microphone_id {
                s.microphone_id = v;
            }
            if let Some(v) = patch.monitor_id {
                s.monitor_id = v;
            }
            if let Some(v) = patch.denoise_mic {
                s.denoise_mic = v;
            }
            if let Some(v) = patch.auto_answer {
                s.auto_answer = v;
            }
            if let Some(v) = patch.answer_length {
                s.answer_length = v;
            }
            if let Some(v) = patch.suggestions {
                s.suggestions = v;
            }
            if let Some(v) = patch.stealth {
                s.stealth = v;
            }
            if let Some(v) = patch.auto_launch {
                s.auto_launch = v;
            }
            if let Some(v) = patch.prompts {
                s.prompts = v;
            }
            if let Some(v) = patch.active_prompt_id {
                s.active_prompt_id = Some(v);
            }
            if let Some(v) = patch.shortcuts {
                s.shortcuts = v;
            }
            Ok(())
        })
    }

    /// Adds or edits a provider. A new key is written to the keychain before the record; if the keychain fails,
    /// nothing changes and no key is stored anywhere else. Returns the provider id.
    pub fn save_provider(&self, input: ProviderInput) -> Result<String, String> {
        let label = input.label.trim().to_string();
        let endpoint = input.endpoint.trim().trim_end_matches('/').to_string();
        let model = input.model.trim().to_string();
        if label.is_empty() || label.chars().count() > 80 {
            return Err("Give the provider a name of up to 80 characters.".into());
        }
        if model.is_empty() {
            return Err("Enter the model to use.".into());
        }
        input.kind.validate_endpoint(&endpoint)?;
        let key = input.key.as_deref().map(str::trim).filter(|k| !k.is_empty());
        if key.is_some() && input.clear_key {
            return Err("Either enter a new key or remove the saved one, not both.".into());
        }
        if let Some(reason) = self.lock().read_only_reason.clone() {
            return Err(reason);
        }

        let existing = match &input.id {
            Some(id) => Some(self.get().provider(id).cloned().ok_or_else(|| "That provider no longer exists.".to_string())?),
            None => None,
        };
        if let Some(existing) = &existing
            && existing.kind != input.kind
        {
            return Err("A provider's type cannot be changed; add a new provider instead.".into());
        }
        if input.kind == ProviderKind::Deepgram && key.is_none() && existing.as_ref().is_none_or(|p| p.key_ref.is_none() || input.clear_key) {
            return Err("Deepgram needs an API key.".into());
        }
        let id = existing.as_ref().map_or_else(new_id, |p| p.id.clone());
        let account = key_account(&id);
        if let Some(key) = key {
            self.secrets.set(&account, key).map_err(|e| e.to_string())?;
        }
        let key_ref = if key.is_some() {
            Some(account.clone())
        } else if input.clear_key {
            None
        } else {
            existing.as_ref().and_then(|p| p.key_ref.clone())
        };
        let record = ProviderRecord { id: id.clone(), kind: input.kind, label, endpoint, model, key_ref };
        let saved = self.commit(|s| {
            match s.providers.iter_mut().find(|p| p.id == record.id) {
                Some(slot) => *slot = record.clone(),
                None => s.providers.push(record.clone()),
            }
            // The first provider of a role becomes the selected one.
            match record.kind.role() {
                ProviderRole::Transcription if s.transcriber_id.is_none() => s.transcriber_id = Some(record.id.clone()),
                ProviderRole::Brain if s.brain_id.is_none() => s.brain_id = Some(record.id.clone()),
                _ => {}
            }
            Ok(())
        });
        match saved {
            Ok(()) => {
                if input.clear_key && existing.as_ref().is_some_and(|p| p.key_ref.is_some()) {
                    // The record no longer points at the key; a failed delete only leaves an orphan.
                    if let Err(error) = self.secrets.delete(&account) {
                        eprintln!("[settings] removed key reference but could not delete it from the keychain: {error}");
                    }
                }
                Ok(id)
            }
            Err(error) => {
                // Roll back a key written for a provider that was never saved.
                if key.is_some() && existing.is_none() {
                    let _ = self.secrets.delete(&account);
                }
                Err(error)
            }
        }
    }

    /// Removes a provider and its keychain entry. The record goes first so a keychain failure cannot leave a provider
    /// that points at a missing key.
    pub fn remove_provider(&self, id: &str) -> Result<(), String> {
        let removed = self.commit(|s| {
            let before = s.providers.len();
            let record = s.providers.iter().find(|p| p.id == id).cloned();
            s.providers.retain(|p| p.id != id);
            if s.providers.len() == before {
                return Err("That provider no longer exists.".to_string());
            }
            Ok(record)
        })?;
        if let Some(account) = removed.and_then(|p| p.key_ref) {
            self.secrets.delete(&account).map_err(|e| format!("The provider was removed, but its key is still in the keychain: {e}"))?;
        }
        Ok(())
    }

    pub fn select_provider(&self, role: ProviderRole, id: Option<String>) -> Result<(), String> {
        self.commit(|s| {
            if let Some(id) = &id {
                let provider = s.provider(id).ok_or_else(|| "That provider no longer exists.".to_string())?;
                if provider.kind.role() != role {
                    return Err("That provider cannot be used for this role.".into());
                }
            }
            match role {
                ProviderRole::Transcription => s.transcriber_id = id,
                ProviderRole::Brain => s.brain_id = id,
            }
            Ok(())
        })
    }

    /// The API key for a provider, for Rust-side networking only. `Ok(None)` for keyless providers.
    // Its caller is the provider networking step, which waits on the audio gate.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn provider_key(&self, id: &str) -> Result<Option<String>, String> {
        let record = self.get().provider(id).cloned().ok_or_else(|| "That provider no longer exists.".to_string())?;
        let Some(account) = record.key_ref else { return Ok(None) };
        match self.secrets.get(&account) {
            Ok(Some(key)) => Ok(Some(key)),
            Ok(None) => Err(format!("The API key for {} is missing from the keychain. Enter it again.", record.label)),
            Err(SecretError::Unavailable(detail)) => Err(format!(
                "The system keychain is unavailable, so the API key for {} cannot be read. ({detail})",
                record.label
            )),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;
    use crate::storage::test_support::TempDir;
    use std::sync::atomic::Ordering;

    fn store(dir: &TempDir) -> (SettingsStore, Arc<MemorySecrets>) {
        let secrets = Arc::new(MemorySecrets::default());
        (SettingsStore::open(dir.path(), secrets.clone()).unwrap(), secrets)
    }

    fn brain(key: Option<&str>) -> ProviderInput {
        ProviderInput {
            id: None,
            kind: ProviderKind::OpenaiCompatible,
            label: "Gemini".into(),
            endpoint: "https://generativelanguage.googleapis.com/v1beta/openai/".into(),
            model: "gemini-flash-latest".into(),
            key: key.map(Into::into),
            clear_key: false,
        }
    }

    fn deepgram(key: Option<&str>) -> ProviderInput {
        ProviderInput {
            id: None,
            kind: ProviderKind::Deepgram,
            label: "Deepgram".into(),
            endpoint: "wss://api.deepgram.com/v1/listen".into(),
            model: "nova-3".into(),
            key: key.map(Into::into),
            clear_key: false,
        }
    }

    #[test]
    fn new_install_defaults_to_question_auto_answer() {
        let dir = TempDir::new();
        let (store, _) = store(&dir);
        let view = store.view();
        assert_eq!(view.auto_answer, AutoAnswer::Questions);
        assert!(view.providers.is_empty());
        assert_eq!(view.active_prompt_id.as_deref(), Some("default"));
    }

    #[test]
    fn key_goes_to_keychain_and_never_to_disk_or_view() {
        let dir = TempDir::new();
        let (store, secrets) = store(&dir);
        let id = store.save_provider(brain(Some("sk-secret-123"))).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert!(!on_disk.contains("sk-secret-123"));
        let view = serde_json::to_string(&store.view()).unwrap();
        assert!(!view.contains("sk-secret-123"));
        assert!(!view.contains("keyRef"));
        assert!(store.view().providers[0].has_key);
        assert_eq!(secrets.secrets.lock().unwrap().get(&format!("provider:{id}")).unwrap(), "sk-secret-123");
        assert_eq!(store.provider_key(&id).unwrap().as_deref(), Some("sk-secret-123"));
    }

    #[test]
    fn providers_and_selection_persist_across_restart() {
        let dir = TempDir::new();
        let secrets = Arc::new(MemorySecrets::default());
        let first = SettingsStore::open(dir.path(), secrets.clone()).unwrap();
        let brain_id = first.save_provider(brain(Some("k1"))).unwrap();
        let dg_id = first.save_provider(deepgram(Some("k2"))).unwrap();
        drop(first);
        let second = SettingsStore::open(dir.path(), secrets).unwrap();
        let view = second.view();
        assert_eq!(view.brain_id.as_deref(), Some(brain_id.as_str()));
        assert_eq!(view.transcriber_id.as_deref(), Some(dg_id.as_str()));
        assert_eq!(second.provider_key(&dg_id).unwrap().as_deref(), Some("k2"));
    }

    #[test]
    fn unavailable_keychain_rejects_the_save_and_keeps_prior_settings() {
        let dir = TempDir::new();
        let (store, secrets) = store(&dir);
        let existing = store.save_provider(brain(Some("old"))).unwrap();
        let before = std::fs::read(dir.path().join("settings.json")).unwrap();
        secrets.unavailable.store(true, Ordering::SeqCst);
        let error = store.save_provider(deepgram(Some("new-key"))).unwrap_err();
        assert!(error.contains("keychain is unavailable"), "{error}");
        assert_eq!(std::fs::read(dir.path().join("settings.json")).unwrap(), before);
        assert_eq!(store.view().providers.len(), 1);
        assert!(!std::fs::read_to_string(dir.path().join("settings.json")).unwrap().contains("new-key"));
        let read = store.provider_key(&existing).unwrap_err();
        assert!(read.contains("unavailable"), "{read}");
    }

    #[test]
    fn editing_without_a_key_keeps_the_saved_key() {
        let dir = TempDir::new();
        let (store, _) = store(&dir);
        let id = store.save_provider(brain(Some("k"))).unwrap();
        let mut edit = brain(None);
        edit.id = Some(id.clone());
        edit.model = "gemini-pro-latest".into();
        store.save_provider(edit).unwrap();
        assert_eq!(store.get().provider(&id).unwrap().model, "gemini-pro-latest");
        assert_eq!(store.provider_key(&id).unwrap().as_deref(), Some("k"));
    }

    #[test]
    fn clearing_a_key_deletes_it_from_the_keychain() {
        let dir = TempDir::new();
        let (store, secrets) = store(&dir);
        let id = store.save_provider(brain(Some("k"))).unwrap();
        let mut edit = brain(None);
        edit.id = Some(id.clone());
        edit.clear_key = true;
        store.save_provider(edit).unwrap();
        assert!(!store.view().providers[0].has_key);
        assert!(secrets.secrets.lock().unwrap().is_empty());
        assert_eq!(store.provider_key(&id).unwrap(), None);
    }

    #[test]
    fn deepgram_requires_a_key_and_a_wss_endpoint() {
        let dir = TempDir::new();
        let (store, _) = store(&dir);
        assert!(store.save_provider(deepgram(None)).unwrap_err().contains("API key"));
        let mut plain = deepgram(Some("k"));
        plain.endpoint = "https://api.deepgram.com/v1/listen".into();
        assert!(store.save_provider(plain).unwrap_err().contains("wss://"));
    }

    #[test]
    fn plain_http_is_only_allowed_for_localhost() {
        let dir = TempDir::new();
        let (store, _) = store(&dir);
        let mut ollama = brain(None);
        ollama.endpoint = "http://localhost:11434/v1".into();
        let id = store.save_provider(ollama).unwrap();
        assert_eq!(store.provider_key(&id).unwrap(), None);
        let mut remote = brain(Some("k"));
        remote.endpoint = "http://example.com/v1".into();
        assert!(store.save_provider(remote).is_err());
        let mut sneaky = brain(Some("k"));
        sneaky.endpoint = "http://localhost.example.com/v1".into();
        assert!(store.save_provider(sneaky).is_err());
    }

    #[test]
    fn multiple_providers_per_role_with_explicit_selection() {
        let dir = TempDir::new();
        let (store, _) = store(&dir);
        let first = store.save_provider(brain(Some("a"))).unwrap();
        let second = store.save_provider(brain(Some("b"))).unwrap();
        assert_eq!(store.view().brain_id.as_deref(), Some(first.as_str()));
        store.select_provider(ProviderRole::Brain, Some(second.clone())).unwrap();
        assert_eq!(store.view().brain_id.as_deref(), Some(second.as_str()));
        assert!(store.select_provider(ProviderRole::Transcription, Some(second.clone())).is_err());
    }

    #[test]
    fn removing_a_provider_clears_selection_and_key() {
        let dir = TempDir::new();
        let (store, secrets) = store(&dir);
        let id = store.save_provider(brain(Some("k"))).unwrap();
        store.remove_provider(&id).unwrap();
        let view = store.view();
        assert!(view.providers.is_empty());
        assert_eq!(view.brain_id, None);
        assert!(secrets.secrets.lock().unwrap().is_empty());
    }

    #[test]
    fn corrupt_settings_are_moved_aside_not_overwritten() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("settings.json"), b"{ broken").unwrap();
        let (store, _) = store(&dir);
        assert_eq!(store.view().auto_answer, AutoAnswer::Questions);
        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("settings.json.corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn settings_from_a_newer_build_are_read_only() {
        let dir = TempDir::new();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, br#"{ "schemaVersion": 99, "outputLanguage": "de" }"#).unwrap();
        let (store, _) = store(&dir);
        let view = store.view();
        assert_eq!(view.output_language, "de");
        assert!(view.read_only_reason.is_some());
        assert!(store.update(SettingsPatch { suggestions: Some(true), ..Default::default() }).is_err());
        assert!(std::fs::read_to_string(&file).unwrap().contains("99"));
    }

    #[test]
    fn normalize_repairs_invalid_values() {
        let s = Settings {
            output_language: "xx".into(),
            prompts: vec![],
            active_prompt_id: Some("missing".into()),
            shortcuts: Shortcuts { listen: "  ".into(), ..Shortcuts::default() },
            brain_id: Some("ghost".into()),
            ..Settings::default()
        }
        .normalized();
        assert_eq!(s.output_language, "en");
        assert_eq!(s.prompts.len(), 1);
        assert_eq!(s.active_prompt_id.as_deref(), Some("default"));
        assert_eq!(s.shortcuts.listen, "CommandOrControl+L");
        assert_eq!(s.brain_id, None);
    }

    #[test]
    fn patch_rejects_unknown_fields() {
        let error = serde_json::from_str::<SettingsPatch>(r#"{ "providers": [] }"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }
}
