//! Structured meeting report generation and unfinished report recovery.

use crate::chat::{BrainBackend, ChatService};
use crate::llm::LlmMessage;
use crate::meetings::{Meeting, MeetingStatus, MeetingStore};
use crate::settings::{SettingsStore, language_instruction};
use crate::transcript_files::now_ms;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const HEADERS: [&str; 8] = [
    "TITLE",
    "OVERVIEW",
    "KEY POINTS",
    "DECISIONS",
    "ACTION ITEMS",
    "NEXT STEPS",
    "FOLLOW-UP QUESTIONS",
    "TOPICS",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummary {
    pub title: String,
    pub overview: String,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub action_items: Vec<String>,
    pub next_steps: Vec<String>,
    pub follow_up_questions: Vec<String>,
    pub topics: Vec<String>,
    pub generated_at: f64,
    pub language: String,
}

pub struct ReportGenerator {
    settings: Arc<SettingsStore>,
    meetings: Arc<MeetingStore>,
    chat: Arc<ChatService>,
    backend: Arc<dyn BrainBackend>,
    on_changed: Arc<dyn Fn() + Send + Sync>,
}

impl ReportGenerator {
    pub fn new(
        settings: Arc<SettingsStore>,
        meetings: Arc<MeetingStore>,
        chat: Arc<ChatService>,
        backend: Arc<dyn BrainBackend>,
        on_changed: impl Fn() + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            meetings,
            chat,
            backend,
            on_changed: Arc::new(on_changed),
        })
    }

    pub async fn resume_unfinished(self: &Arc<Self>) {
        let stuck: Vec<String> = self
            .meetings
            .list()
            .into_iter()
            .filter(|m| m.status == MeetingStatus::Processing)
            .map(|m| m.id)
            .collect();
        for id in stuck {
            let _ = self.generate(&id).await;
        }
    }

    pub async fn generate(&self, meeting_id: &str) -> Result<Option<Meeting>, String> {
        let Some(mut meeting) = self.meetings.get(meeting_id) else {
            return Ok(None);
        };
        if meeting.full_transcript_text.trim().is_empty() {
            meeting.status = MeetingStatus::Failed;
            meeting.updated_at = now_ms();
            self.meetings.save(meeting.clone())?;
            (self.on_changed)();
            return Ok(Some(meeting));
        }

        let brain = match self.chat.resolve_brain() {
            Ok(b) => b,
            Err(err) => {
                meeting.status = MeetingStatus::Failed;
                meeting.updated_at = now_ms();
                self.meetings.save(meeting.clone())?;
                (self.on_changed)();
                return Err(err);
            }
        };

        meeting.status = MeetingStatus::Processing;
        meeting.summary = None;
        meeting.updated_at = now_ms();
        self.meetings.save(meeting.clone())?;
        (self.on_changed)();

        let language = self.settings.get().output_language;
        let messages = vec![
            LlmMessage::system(format!("You are an expert meeting analyst. {}", language_instruction(&language))),
            LlmMessage::user(build_report_prompt(&meeting)),
        ];

        match self.backend.chat_once(brain, messages).await {
            Ok(res) => {
                let summary = parse_report(&res.text, &language);
                if !summary.title.is_empty() {
                    meeting.title = summary.title.clone();
                }
                meeting.status = MeetingStatus::Completed;
                meeting.summary = serde_json::to_value(&summary).ok();
                meeting.updated_at = now_ms();
                self.meetings.save(meeting.clone())?;
                (self.on_changed)();
                Ok(Some(meeting))
            }
            Err(err) => {
                meeting.status = MeetingStatus::Failed;
                meeting.updated_at = now_ms();
                self.meetings.save(meeting.clone())?;
                (self.on_changed)();
                Err(err)
            }
        }
    }
}

pub fn build_report_prompt(meeting: &Meeting) -> String {
    let minutes = ((meeting.duration / 60_000.0).round() as i64).max(1);
    format!(
        "Analyze this meeting transcript ({minutes} min).\n\nTRANSCRIPT:\n\"\"\"\n{}\n\"\"\"\n\nReply using exactly these section headers, each on its own line, in this order. Use \"- \" bullets inside list sections. Leave a section empty if nothing applies.\n\n**TITLE:**\n(one line, a descriptive title)\n\n**OVERVIEW:**\n(2-3 sentences)\n\n**KEY POINTS:**\n**DECISIONS:**\n**ACTION ITEMS:**\n**NEXT STEPS:**\n**FOLLOW-UP QUESTIONS:**\n**TOPICS:**",
        meeting.full_transcript_text
    )
}

pub fn parse_report(text: &str, language: &str) -> MeetingSummary {
    let mut sections: std::collections::HashMap<&str, Vec<String>> = std::collections::HashMap::new();
    let mut current: Option<&str> = None;

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if let Some((header, inline_rest)) = match_header(trimmed) {
            current = Some(header);
            let slot = sections.entry(header).or_default();
            if !inline_rest.is_empty() {
                slot.push(inline_rest.to_string());
            }
            continue;
        }
        if let Some(h) = current {
            sections.entry(h).or_default().push(trimmed.to_string());
        }
    }

    let body = |name: &str| -> String {
        sections
            .get(name)
            .map(|lines| lines.join("\n").trim().to_string())
            .unwrap_or_default()
    };
    let bullets = |name: &str| -> Vec<String> {
        body(name)
            .lines()
            .map(|l| l.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '•' | '-' | '*')))
            .map(|l| strip_leading_number(l).trim().to_string())
            .filter(|l| !l.is_empty() && !(l.starts_with('(') && l.ends_with(')')))
            .take(12)
            .collect()
    };

    let title = body("TITLE")
        .lines()
        .next()
        .unwrap_or("")
        .trim_matches(['"', '\'', ' '])
        .to_string();
    let overview_raw = body("OVERVIEW");
    let overview = if overview_raw.starts_with('(') && overview_raw.ends_with(')') {
        String::new()
    } else {
        overview_raw
    };

    MeetingSummary {
        title,
        overview,
        key_points: bullets("KEY POINTS"),
        decisions: bullets("DECISIONS"),
        action_items: bullets("ACTION ITEMS"),
        next_steps: bullets("NEXT STEPS"),
        follow_up_questions: bullets("FOLLOW-UP QUESTIONS"),
        topics: bullets("TOPICS"),
        generated_at: now_ms(),
        language: language.to_string(),
    }
}

fn match_header(line: &str) -> Option<(&'static str, &str)> {
    let clean = line.trim_start_matches(['*', '#', ' ']);
    for header in HEADERS {
        if let Some(after) = clean.strip_prefix(header) {
            let after_markers = after.trim_start_matches(['*', ':', ' ']);
            if after.starts_with(':') || after.starts_with("**") || after.trim().is_empty() {
                return Some((header, after_markers.trim()));
            }
        }
    }
    None
}

fn strip_leading_number(line: &str) -> &str {
    let trimmed = line.trim_start();
    let digits = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(after) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            return after.trim_start();
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bold_and_plain_report_sections() {
        let sample = "**TITLE:**\nQ3 roadmap sync\n\n**OVERVIEW:**\nAligned on billing.\n\n**KEY POINTS:**\n- Billing first\n* Mobile slips\n\n**DECISIONS:**\n- Ship billing\n\n**ACTION ITEMS:**\n1. Anna drafts spec\n2) Ben books sync\n\n**NEXT STEPS:**\n\n**FOLLOW-UP QUESTIONS:**\n- Who owns runbook?\n\n**TOPICS:**\n- billing";
        let s = parse_report(sample, "en");
        assert_eq!(s.title, "Q3 roadmap sync");
        assert_eq!(s.overview, "Aligned on billing.");
        assert_eq!(s.key_points, vec!["Billing first", "Mobile slips"]);
        assert_eq!(s.decisions, vec!["Ship billing"]);
        assert_eq!(s.action_items, vec!["Anna drafts spec", "Ben books sync"]);
        assert!(s.next_steps.is_empty());
        assert_eq!(s.follow_up_questions, vec!["Who owns runbook?"]);
        assert_eq!(s.topics, vec!["billing"]);
    }
}
