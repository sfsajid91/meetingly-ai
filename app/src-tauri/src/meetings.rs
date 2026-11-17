//! Meeting history: one JSON array, newest first.

use crate::storage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    You,
    Them,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub text: String,
    pub timestamp: f64,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub speaker: Option<i64>,
    #[serde(default)]
    pub channel: Option<Channel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeetingStatus {
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub start_time: f64,
    pub end_time: f64,
    pub duration: f64,
    #[serde(default)]
    pub transcript: Vec<TranscriptSegment>,
    #[serde(default)]
    pub full_transcript_text: String,
    pub language: String,
    pub status: MeetingStatus,
    /// Generated report, kept as written so older and newer report shapes both survive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<serde_json::Value>,
    pub created_at: f64,
    pub updated_at: f64,
}

pub struct MeetingStore {
    file: PathBuf,
    meetings: Mutex<Vec<Meeting>>,
}

impl MeetingStore {
    pub const FILE_NAME: &'static str = "meetings.json";

    pub fn open(dir: &Path) -> Result<Self, String> {
        let file = dir.join(Self::FILE_NAME);
        let meetings = match storage::read_json::<Vec<Meeting>>(&file) {
            Ok(found) => found.unwrap_or_default(),
            Err(error) => {
                let moved = storage::quarantine(&file)?;
                eprintln!("[meetings] unreadable history moved to {}: {error}", moved.display());
                Vec::new()
            }
        };
        Ok(Self { file, meetings: Mutex::new(meetings) })
    }

    pub fn list(&self) -> Vec<Meeting> {
        self.meetings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn get(&self, id: &str) -> Option<Meeting> {
        self.meetings.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|m| m.id == id).cloned()
    }

    pub fn save(&self, meeting: Meeting) -> Result<(), String> {
        self.upsert(vec![meeting])
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let mut guard = self.meetings.lock().unwrap_or_else(|e| e.into_inner());
        let before = guard.len();
        let mut next = guard.clone();
        next.retain(|m| m.id != id);
        if next.len() == before {
            return Ok(false);
        }
        storage::write_json(&self.file, &next)?;
        *guard = next;
        Ok(true)
    }

    /// Inserts or replaces meetings by id and persists once.
    pub fn upsert(&self, incoming: Vec<Meeting>) -> Result<(), String> {
        let mut guard = self.meetings.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = guard.clone();
        for meeting in incoming {
            match next.iter_mut().find(|m| m.id == meeting.id) {
                Some(slot) => *slot = meeting,
                None => next.push(meeting),
            }
        }
        next.sort_by(|a, b| b.start_time.total_cmp(&a.start_time));
        storage::write_json(&self.file, &next)?;
        *guard = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::TempDir;

    fn sample(id: &str, start_time: f64) -> Meeting {
        Meeting {
            id: id.into(),
            title: format!("Meeting {id}"),
            start_time,
            end_time: start_time + 1000.0,
            duration: 1000.0,
            transcript: vec![],
            full_transcript_text: "hello".into(),
            language: "en".into(),
            status: MeetingStatus::Completed,
            summary: None,
            created_at: start_time,
            updated_at: start_time,
        }
    }

    #[test]
    fn save_get_delete_and_sort_newest_first() {
        let dir = TempDir::new();
        let store = MeetingStore::open(dir.path()).unwrap();
        store.save(sample("m1", 100.0)).unwrap();
        store.save(sample("m2", 200.0)).unwrap();
        assert_eq!(store.list().iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["m2", "m1"]);
        assert_eq!(store.get("m1").unwrap().title, "Meeting m1");
        assert!(store.delete("m1").unwrap());
        assert!(!store.delete("m1").unwrap());
        assert!(store.get("m1").is_none());
        let reopened = MeetingStore::open(dir.path()).unwrap();
        assert_eq!(reopened.list().len(), 1);
        assert_eq!(reopened.list()[0].id, "m2");
    }
}
