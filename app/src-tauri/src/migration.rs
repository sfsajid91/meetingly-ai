//! One-time import of prompts, preferences, shortcuts and meeting history from the Electron build.
//!
//! The Electron data directory is only read, never written, so rollback is reinstalling the Electron build. Legacy
//! credentials (the linked account token and the encrypted own key) are deliberately not imported.

use crate::meetings::{Meeting, MeetingStore};
use crate::settings::{AnswerLength, AudioSource, AutoAnswer, Prompt, Settings, SettingsStore, Shortcuts};
use crate::storage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MARKER_FILE: &str = "migration.json";
pub const BACKUP_DIR: &str = "pre-migration-backup";
const MARKER_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub version: u32,
    pub source: String,
    pub completed_at_ms: u64,
    pub settings_imported: bool,
    pub meetings_imported: usize,
    /// Legacy meetings that could not be read; they remain in the Electron directory.
    pub meetings_skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Outcome {
    NoLegacyData,
    AlreadyDone { marker: Marker },
    Imported { marker: Marker },
    /// Nothing was marked complete; the next start retries.
    Failed { error: String },
}

/// The Electron build's userData directory: `<config dir>/Meetingly` on every platform.
/// `MEETINGLY_LEGACY_DIR` points at a copy instead, for testing against real data safely.
pub fn legacy_dir(config_dir: &Path) -> PathBuf {
    std::env::var_os("MEETINGLY_LEGACY_DIR").map_or_else(|| config_dir.join("Meetingly"), PathBuf::from)
}

pub fn run(legacy: &Path, target: &Path, settings: &SettingsStore, meetings: &MeetingStore) -> Outcome {
    match import(legacy, target, settings, meetings) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::Failed { error },
    }
}

fn import(legacy: &Path, target: &Path, settings: &SettingsStore, meetings: &MeetingStore) -> Result<Outcome, String> {
    let marker_path = target.join(MARKER_FILE);
    if let Some(marker) = storage::read_json::<Marker>(&marker_path)? {
        return Ok(Outcome::AlreadyDone { marker });
    }
    let legacy_settings = storage::read_json::<Value>(&legacy.join("settings.json"))?;
    let legacy_meetings = storage::read_json::<Vec<Value>>(&legacy.join("meetings.json"))?;
    if legacy_settings.is_none() && legacy_meetings.is_none() {
        return Ok(Outcome::NoLegacyData);
    }

    back_up_target(target)?;

    let parsed: Vec<Result<Meeting, _>> =
        legacy_meetings.unwrap_or_default().into_iter().map(serde_json::from_value::<Meeting>).collect();
    let meetings_skipped = parsed.iter().filter(|m| m.is_err()).count();
    let imported: Vec<Meeting> = parsed.into_iter().filter_map(Result::ok).collect();
    let meetings_imported = imported.len();
    if !imported.is_empty() {
        meetings.upsert(imported)?;
    }

    let settings_imported = legacy_settings.is_some();
    if let Some(raw) = legacy_settings {
        settings.replace(merge(&raw, settings.get()))?;
    }

    let marker = Marker {
        version: MARKER_VERSION,
        source: legacy.display().to_string(),
        completed_at_ms: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
        settings_imported,
        meetings_imported,
        meetings_skipped,
    };
    storage::write_json(&marker_path, &marker)?;
    Ok(Outcome::Imported { marker })
}

/// Copies whatever the Tauri directory held before the first import attempt. A retry keeps the first copy.
fn back_up_target(target: &Path) -> Result<(), String> {
    let backup = target.join(BACKUP_DIR);
    if backup.exists() {
        return Ok(());
    }
    let staging = target.join(format!(".{BACKUP_DIR}.partial"));
    let _ = std::fs::remove_dir_all(&staging);
    storage::ensure_private_dir(&staging)?;
    for name in [SettingsStore::FILE_NAME, MeetingStore::FILE_NAME] {
        let source = target.join(name);
        if source.exists() {
            let bytes = std::fs::read(&source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
            storage::atomic_write(&staging.join(name), &bytes)?;
        }
    }
    std::fs::rename(&staging, &backup).map_err(|e| format!("Could not finish the pre-migration backup: {e}"))
}

/// Legacy preferences on top of the current Tauri settings. Provider configuration and audio sources are Tauri-only
/// and kept; each legacy field is taken only when it parses.
fn merge(raw: &Value, current: Settings) -> Settings {
    fn field<T: serde::de::DeserializeOwned>(raw: &Value, key: &str) -> Option<T> {
        raw.get(key).cloned().and_then(|v| serde_json::from_value(v).ok())
    }
    let mut next = current;
    if let Some(v) = field::<String>(raw, "outputLanguage") {
        next.output_language = v;
    }
    if let Some(v) = field::<String>(raw, "audioLanguage") {
        next.audio_language = v;
    }
    if let Some(v) = field::<AudioSource>(raw, "audioSource") {
        next.audio_source = v;
    }
    if let Some(v) = field::<AutoAnswer>(raw, "autoAnswer") {
        next.auto_answer = v;
    }
    if let Some(v) = field::<AnswerLength>(raw, "answerLength") {
        next.answer_length = v;
    }
    for (key, slot) in [("suggestions", &mut next.suggestions), ("stealth", &mut next.stealth), ("autoLaunch", &mut next.auto_launch)] {
        if let Some(v) = field::<bool>(raw, key) {
            *slot = v;
        }
    }
    if let Some(prompts) = raw.get("prompts").and_then(Value::as_array) {
        let prompts: Vec<Prompt> = prompts.iter().filter_map(|p| serde_json::from_value(p.clone()).ok()).collect();
        if !prompts.is_empty() {
            next.prompts = prompts;
            next.active_prompt_id = field::<String>(raw, "activePromptId");
        }
    }
    if let Some(v) = field::<Shortcuts>(raw, "shortcuts") {
        next.shortcuts = v;
    }
    next.normalized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;
    use crate::settings::{ProviderInput, ProviderKind};
    use crate::storage::test_support::TempDir;
    use std::fs;
    use std::sync::Arc;

    const LEGACY_SETTINGS: &str = r#"{
      "deviceId": "abcdef0123456789",
      "outputLanguage": "de",
      "audioLanguage": "ru",
      "audioSource": "both",
      "transcriptionEngine": "local",
      "audioDeviceId": "",
      "autoAnswer": "always",
      "answerLength": "short",
      "suggestions": true,
      "keepMeetingsLocal": true,
      "stealth": false,
      "autoLaunch": false,
      "prompts": [
        { "id": "p1", "title": "Interview", "content": "Be brief.", "notes": "CV: Rust engineer" },
        { "id": "p2", "title": "Sales", "content": "Sell." }
      ],
      "activePromptId": "p2",
      "shortcuts": { "listen": "CommandOrControl+Shift+L", "dashboard": "CommandOrControl+D" },
      "cloud": { "token": "CLOUD-TOKEN-SECRET", "email": "me@example.com" },
      "ownKey": { "baseUrl": "https://api.openai.com/v1", "model": "gpt", "key": "ENCRYPTED-OWN-KEY" },
      "__v": 3
    }"#;

    const LEGACY_MEETINGS: &str = r#"[
      { "id": "m1", "remoteId": "r1", "title": "Standup", "startTime": 1000, "endTime": 2000, "duration": 1000,
        "transcript": [ { "text": "hello", "timestamp": 1001, "confidence": 0.9, "speaker": null, "channel": "them" } ],
        "fullTranscriptText": "hello", "language": "en", "status": "completed",
        "summary": { "title": "Standup", "keyPoints": ["a"] }, "localOnly": true, "createdAt": 1000, "updatedAt": 2000 },
      { "id": "m2", "title": "Later", "startTime": 5000, "endTime": 6000, "duration": 1000, "transcript": [],
        "fullTranscriptText": "", "language": "en", "status": "failed", "createdAt": 5000, "updatedAt": 6000 },
      { "id": "broken" }
    ]"#;

    struct Fixture {
        legacy: TempDir,
        target: TempDir,
        secrets: Arc<MemorySecrets>,
    }

    impl Fixture {
        fn new() -> Self {
            let legacy = TempDir::new();
            fs::write(legacy.path().join("settings.json"), LEGACY_SETTINGS).unwrap();
            fs::write(legacy.path().join("meetings.json"), LEGACY_MEETINGS).unwrap();
            Self { legacy, target: TempDir::new(), secrets: Arc::new(MemorySecrets::default()) }
        }

        fn stores(&self) -> (SettingsStore, MeetingStore) {
            (SettingsStore::open(self.target.path(), self.secrets.clone()).unwrap(), MeetingStore::open(self.target.path()).unwrap())
        }

        fn migrate(&self) -> Outcome {
            let (settings, meetings) = self.stores();
            run(self.legacy.path(), self.target.path(), &settings, &meetings)
        }

        fn legacy_bytes(&self) -> (Vec<u8>, Vec<u8>) {
            (fs::read(self.legacy.path().join("settings.json")).unwrap(), fs::read(self.legacy.path().join("meetings.json")).unwrap())
        }

        fn target_text(&self) -> String {
            let mut all = String::new();
            for name in [SettingsStore::FILE_NAME, MeetingStore::FILE_NAME, MARKER_FILE] {
                all += &fs::read_to_string(self.target.path().join(name)).unwrap_or_default();
            }
            all
        }
    }

    #[test]
    fn imports_preferences_prompts_shortcuts_and_meetings() {
        let f = Fixture::new();
        let Outcome::Imported { marker } = f.migrate() else { panic!("expected import") };
        assert_eq!((marker.meetings_imported, marker.meetings_skipped), (2, 1));
        let (settings, meetings) = f.stores();
        let s = settings.get();
        assert_eq!((s.output_language.as_str(), s.audio_language.as_str()), ("de", "ru"));
        assert_eq!(s.audio_source, AudioSource::Both);
        assert_eq!(s.auto_answer, AutoAnswer::Always);
        assert_eq!(s.answer_length, AnswerLength::Short);
        assert!(s.suggestions && !s.stealth);
        assert_eq!(s.prompts.len(), 2);
        assert_eq!(s.prompts[0].notes, "CV: Rust engineer");
        assert_eq!(s.active_prompt_id.as_deref(), Some("p2"));
        assert_eq!(s.shortcuts.listen, "CommandOrControl+Shift+L");
        assert_eq!(s.shortcuts.chat, Shortcuts::default().chat);
        let list = meetings.list();
        assert_eq!(list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["m2", "m1"]);
        assert_eq!(list[1].transcript[0].text, "hello");
        assert!(list[1].summary.is_some());
    }

    #[test]
    fn legacy_credentials_and_cloud_fields_are_not_imported() {
        let f = Fixture::new();
        f.migrate();
        let text = f.target_text();
        for leaked in ["CLOUD-TOKEN-SECRET", "ENCRYPTED-OWN-KEY", "me@example.com", "abcdef0123456789", "remoteId", "localOnly", "keepMeetingsLocal", "dashboard"] {
            assert!(!text.contains(leaked), "{leaked} leaked into the Tauri data");
        }
        assert!(f.secrets.secrets.lock().unwrap().is_empty());
    }

    #[test]
    fn the_electron_directory_is_never_modified() {
        let f = Fixture::new();
        let before = f.legacy_bytes();
        let names_before: Vec<_> = fs::read_dir(f.legacy.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        f.migrate();
        f.migrate();
        assert_eq!(f.legacy_bytes(), before);
        let names_after: Vec<_> = fs::read_dir(f.legacy.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names_before, names_after);
    }

    #[test]
    fn a_completed_migration_does_not_run_again() {
        let f = Fixture::new();
        f.migrate();
        let (settings, _) = f.stores();
        settings.update(crate::settings::SettingsPatch { output_language: Some("fr".into()), ..Default::default() }).unwrap();
        assert!(matches!(f.migrate(), Outcome::AlreadyDone { .. }));
        assert_eq!(f.stores().0.get().output_language, "fr");
    }

    #[test]
    fn an_interrupted_migration_retries_to_the_same_result() {
        let clean = Fixture::new();
        clean.migrate();
        let expected = (clean.stores().0.get(), clean.stores().1.list());

        let f = Fixture::new();
        {
            // Interrupted after the meetings were written and before settings and the marker.
            let (_, meetings) = f.stores();
            let partial: Vec<Meeting> = serde_json::from_str::<Vec<Value>>(LEGACY_MEETINGS)
                .unwrap()
                .into_iter()
                .filter_map(|m| serde_json::from_value(m).ok())
                .take(1)
                .collect();
            meetings.upsert(partial).unwrap();
        }
        assert!(!f.target.path().join(MARKER_FILE).exists());
        assert!(matches!(f.migrate(), Outcome::Imported { .. }));
        assert_eq!((f.stores().0.get(), f.stores().1.list()), expected);
    }

    #[test]
    fn providers_configured_before_a_retry_are_kept_and_the_first_backup_survives() {
        let f = Fixture::new();
        let (settings, _) = f.stores();
        settings
            .save_provider(ProviderInput {
                id: None,
                kind: ProviderKind::OpenaiCompatible,
                label: "Local".into(),
                endpoint: "http://localhost:11434/v1".into(),
                model: "llama".into(),
                key: None,
                clear_key: false,
            })
            .unwrap();
        let original = fs::read(f.target.path().join("settings.json")).unwrap();
        f.migrate();
        let s = f.stores().0.get();
        assert_eq!(s.providers.len(), 1);
        assert_eq!(s.brain_id.as_deref(), Some(s.providers[0].id.as_str()));
        assert_eq!(fs::read(f.target.path().join(BACKUP_DIR).join("settings.json")).unwrap(), original);
    }

    #[test]
    fn unreadable_legacy_meetings_fail_without_marking_complete() {
        let f = Fixture::new();
        fs::write(f.legacy.path().join("meetings.json"), b"[ truncated").unwrap();
        assert!(matches!(f.migrate(), Outcome::Failed { .. }));
        assert!(!f.target.path().join(MARKER_FILE).exists());
        fs::write(f.legacy.path().join("meetings.json"), LEGACY_MEETINGS).unwrap();
        assert!(matches!(f.migrate(), Outcome::Imported { .. }));
    }

    #[test]
    fn no_legacy_directory_means_nothing_to_do() {
        let f = Fixture::new();
        let empty = TempDir::new();
        let (settings, meetings) = f.stores();
        assert_eq!(run(&empty.path().join("missing"), f.target.path(), &settings, &meetings), Outcome::NoLegacyData);
        assert!(!f.target.path().join(MARKER_FILE).exists());
    }
}
