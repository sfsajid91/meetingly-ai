//! Plain-text transcript autosave and final export under Documents/Meetingly.

use crate::meetings::{Channel, Meeting, TranscriptSegment};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn segment_label(segment: &TranscriptSegment) -> String {
    if let Some(channel) = segment.channel {
        return match channel {
            Channel::You => "You".into(),
            Channel::Them => "Them".into(),
        };
    }
    match segment.speaker {
        Some(n) => format!("Speaker {}", n + 1),
        None => String::new(),
    }
}

pub fn format_transcript(segments: &[TranscriptSegment]) -> String {
    let labelled = segments.iter().any(|s| s.channel.is_some() || s.speaker.is_some());
    format_transcript_with_mode(segments, labelled)
}

fn format_transcript_with_mode(segments: &[TranscriptSegment], labelled: bool) -> String {
    if !labelled {
        return segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for s in segments {
        let label = {
            let l = segment_label(s);
            if l.is_empty() { "Unknown".into() } else { l }
        };
        if current.as_deref() == Some(label.as_str()) {
            if let Some(last) = lines.last_mut() {
                last.push(' ');
                last.push_str(&s.text);
            }
        } else {
            current = Some(label.clone());
            lines.push(format!("{label}: {}", s.text));
        }
    }
    lines.join("\n")
}

pub fn slice_tail_chars(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    let mut count = 0usize;
    for (idx, _) in s.char_indices().rev() {
        count += 1;
        if count == max_chars {
            return &s[idx..];
        }
    }
    s
}

pub fn format_transcript_tail(segments: &[TranscriptSegment], max_chars: usize) -> String {
    if segments.is_empty() || max_chars == 0 {
        return String::new();
    }
    let labelled = segments.iter().any(|s| s.channel.is_some() || s.speaker.is_some());
    let mut budget = 0usize;
    let mut start = segments.len();
    let mut prev_label: Option<String> = None;
    while start > 0 {
        start -= 1;
        let seg = &segments[start];
        budget += seg.text.chars().count() + 1;
        if labelled {
            let lbl = segment_label(seg);
            let lbl_str = if lbl.is_empty() { "Unknown" } else { lbl.as_str() };
            if prev_label.as_deref() != Some(lbl_str) {
                budget += lbl_str.chars().count() + 2;
                prev_label = Some(lbl_str.to_string());
            }
        }
        if budget >= max_chars {
            break;
        }
    }
    if labelled {
        let boundary = segment_label(&segments[start]);
        while start > 0 && segment_label(&segments[start - 1]) == boundary {
            start -= 1;
        }
    }
    let formatted = format_transcript_with_mode(&segments[start..], labelled);
    slice_tail_chars(&formatted, max_chars).to_string()
}

pub fn title_from_transcript(text: &str) -> String {
    let clean = text.trim();
    if clean.is_empty() {
        return "Untitled meeting".into();
    }
    let first = clean.split(['.', '!', '?']).next().unwrap_or("").trim();
    let first_chars = first.chars().count();
    if (6..=60).contains(&first_chars) {
        return first.to_string();
    }
    if clean.chars().count() > 60 {
        let truncated: String = clean.chars().take(57).collect();
        return format!("{}…", truncated.trim_end());
    }
    clean.to_string()
}

pub fn transcript_file_name(start_time_ms: f64, title: &str) -> String {
    let stamp = format_local_stamp(start_time_ms);
    let safe_title: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\u{0000}'..='\u{001f}' => ' ',
            _ => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', ' '])
        .chars()
        .take(60)
        .collect();
    if safe_title.is_empty() {
        format!("{stamp}.txt")
    } else {
        format!("{stamp} {safe_title}.txt")
    }
}

pub fn transcript_text(meeting: &Meeting) -> String {
    let minutes = ((meeting.duration / 60_000.0).round() as i64).max(1);
    let when = format_readable_time(meeting.start_time);
    let body = if meeting.transcript.is_empty() {
        meeting.full_transcript_text.clone()
    } else {
        format_transcript(&meeting.transcript)
    };
    let joined = format!("{}\n{when} · {minutes} min\n\n{body}\n", meeting.title);
    joined.replace("\r\n", "\n").replace('\n', "\r\n")
}

pub fn write_transcript_file(dir: &Path, meeting: &Meeting, previous: Option<&Path>) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let name = transcript_file_name(meeting.start_time, &meeting.title);
    let target = match previous {
        Some(prev) if is_name_or_copy(prev, &name) => prev.to_path_buf(),
        _ => unique_path(dir, &name),
    };
    fs::write(&target, transcript_text(meeting)).map_err(|e| format!("Could not write {}: {e}", target.display()))?;
    if let Some(prev) = previous
        && prev != target
    {
        let _ = fs::remove_file(prev);
    }
    Ok(target)
}

pub struct TranscriptAutosave {
    dir: PathBuf,
    file: Option<PathBuf>,
    lines: usize,
}

impl TranscriptAutosave {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, file: None, lines: 0 }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn save_draft(&mut self, meeting: Option<&Meeting>) -> Result<(), String> {
        let Some(meeting) = meeting else { return Ok(()) };
        if meeting.transcript.len() == self.lines {
            return Ok(());
        }
        let path = write_transcript_file(&self.dir, meeting, self.file.as_deref())?;
        self.file = Some(path);
        self.lines = meeting.transcript.len();
        Ok(())
    }

    pub fn finish(&mut self, meeting: &Meeting) -> Result<PathBuf, String> {
        let result = write_transcript_file(&self.dir, meeting, self.file.as_deref());
        self.file = None;
        self.lines = 0;
        result
    }

    pub fn discard(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = fs::remove_file(file);
        }
        self.lines = 0;
    }
}

pub fn default_transcript_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Meetingly")
}

fn is_name_or_copy(file: &Path, name: &str) -> bool {
    let Some(own) = file.file_name().and_then(|n| n.to_str()) else { return false };
    if own == name {
        return true;
    }
    let Some((base, ext)) = name.rsplit_once('.') else { return false };
    let ext = format!(".{ext}");
    let prefix = format!("{base} (");
    let suffix = format!("){ext}");
    if let Some(rest) = own.strip_prefix(&prefix).and_then(|s| s.strip_suffix(&suffix)) {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    false
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (base, ext) = name.rsplit_once('.').map(|(b, e)| (b, format!(".{e}"))).unwrap_or((name, String::new()));
    let mut index = 2usize;
    loop {
        let next = dir.join(format!("{base} ({index}){ext}"));
        if !next.exists() {
            return next;
        }
        index += 1;
    }
}

fn format_local_stamp(ms: f64) -> String {
    let (y, m, d, hh, mm) = split_ymd_hm(ms);
    format!("{y:04}-{m:02}-{d:02} {hh:02}-{mm:02}")
}

fn format_readable_time(ms: f64) -> String {
    let (y, m, d, hh, mm) = split_ymd_hm(ms);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

fn split_ymd_hm(ms: f64) -> (i32, u32, u32, u32, u32) {
    let secs = (ms / 1000.0).floor().max(0.0) as u64;
    let time = UNIX_EPOCH + Duration::from_secs(secs);
    let unix_secs = time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    // Civil calendar conversion in UTC when libc localtime isn't used; keeps file naming deterministic across platforms.
    #[cfg(unix)]
    {
        if let Some(tm) = localtime_unix(unix_secs) {
            return tm;
        }
    }
    civil_from_days(unix_secs.div_euclid(86_400), unix_secs.rem_euclid(86_400))
}

#[cfg(unix)]
fn localtime_unix(secs: i64) -> Option<(i32, u32, u32, u32, u32)> {
    #[repr(C)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        tm_gmtoff: isize,
        tm_zone: *const i8,
    }
    unsafe extern "C" {
        fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    }
    let mut out = std::mem::MaybeUninit::<Tm>::uninit();
    let ptr = unsafe { localtime_r(&secs, out.as_mut_ptr()) };
    if ptr.is_null() {
        return None;
    }
    let tm = unsafe { out.assume_init() };
    Some((
        tm.tm_year + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
    ))
}

fn civil_from_days(z: i64, sod: i64) -> (i32, u32, u32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    let hh = (sod / 3600) as u32;
    let mm = ((sod % 3600) / 60) as u32;
    (y as i32, m as u32, d as u32, hh, mm)
}

pub fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::MeetingStatus;
    use crate::storage::test_support::TempDir;

    fn sample_meeting(title: &str) -> Meeting {
        Meeting {
            id: "m1".into(),
            title: title.into(),
            start_time: 1_760_537_100_000.0,
            end_time: 1_760_537_225_000.0,
            duration: 125_000.0,
            transcript: vec![
                TranscriptSegment {
                    text: "What did we ship?".into(),
                    timestamp: 1_760_537_100_000.0,
                    confidence: 1.0,
                    speaker: None,
                    channel: Some(Channel::Them),
                },
                TranscriptSegment {
                    text: "The new onboarding.".into(),
                    timestamp: 1_760_537_105_000.0,
                    confidence: 1.0,
                    speaker: None,
                    channel: Some(Channel::You),
                },
                TranscriptSegment {
                    text: "And the docs.".into(),
                    timestamp: 1_760_537_107_000.0,
                    confidence: 1.0,
                    speaker: None,
                    channel: Some(Channel::You),
                },
            ],
            full_transcript_text: String::new(),
            language: "en".into(),
            status: MeetingStatus::Processing,
            summary: None,
            created_at: 1_760_537_100_000.0,
            updated_at: 1_760_537_100_000.0,
        }
    }

    #[test]
    fn formats_transcript_merging_consecutive_turns() {
        let m = sample_meeting("Weekly sync");
        assert_eq!(format_transcript(&m.transcript), "Them: What did we ship?\nYou: The new onboarding. And the docs.");
    }

    #[test]
    fn sanitizes_filename_and_uses_crlf_line_endings() {
        let name = transcript_file_name(1_760_537_100_000.0, "Weekly sync: Q4 plan?");
        assert!(name.ends_with(" Weekly sync Q4 plan.txt"), "{name}");
        let body = transcript_text(&sample_meeting("Weekly sync: Q4 plan?"));
        assert!(body.contains("Them: What did we ship?\r\nYou: The new onboarding. And the docs."));
        assert!(!body.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn autosave_creates_updates_renames_and_discards() {
        let dir = TempDir::new();
        let mut autosave = TranscriptAutosave::new(dir.path().to_path_buf());
        autosave.save_draft(None).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);

        let mut early = sample_meeting("What did we ship");
        early.transcript.truncate(1);
        autosave.save_draft(Some(&early)).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);

        let final_path = autosave.finish(&sample_meeting("Weekly sync")).unwrap();
        assert!(final_path.file_name().unwrap().to_string_lossy().ends_with("Weekly sync.txt"));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(fs::read_to_string(&final_path).unwrap().contains("And the docs."));

        let mut second = TranscriptAutosave::new(dir.path().to_path_buf());
        second.save_draft(Some(&sample_meeting("Discard me"))).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        second.discard();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn never_overwrites_an_earlier_file_with_the_same_name() {
        let dir = TempDir::new();
        let first = write_transcript_file(dir.path(), &sample_meeting("Sync"), None).unwrap();
        let second = write_transcript_file(dir.path(), &sample_meeting("Sync"), None).unwrap();
        assert_ne!(first, second);
        assert!(second.to_string_lossy().contains("(2).txt"));
        let kept = write_transcript_file(dir.path(), &sample_meeting("Sync"), Some(&second)).unwrap();
        assert_eq!(kept, second);
    }

    #[test]
    fn formats_transcript_tail_identically_to_full_suffix() {
        let mut segments = Vec::new();
        for i in 0..400 {
            segments.push(TranscriptSegment {
                text: format!("Segment {i} discussing distributed cache invalidation and latency budgets."),
                timestamp: i as f64 * 1000.0,
                confidence: 0.99,
                speaker: None,
                channel: Some(if i % 3 == 0 { Channel::Them } else { Channel::You }),
            });
        }
        let full = format_transcript(&segments);
        let expected = slice_tail_chars(&full, 4000);
        let actual = format_transcript_tail(&segments, 4000);
        assert_eq!(actual, expected);
        assert!(actual.chars().count() <= 4000);
    }
}
