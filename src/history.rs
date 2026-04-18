//! Session history — version control for Ableton state.
//!
//! Captures before/after state for every mutation, stores in an append-only log.
//! Supports browsing, restoring, checkpointing, and labeling.
//!
//! Storage layout:
//!   ~/.mr/sessions/<session-id>/
//!   ├── meta.json          # session metadata + AI session tracking
//!   ├── log.jsonl           # append-only change log
//!   └── checkpoints/
//!       ├── 0000.json       # full state at init
//!       └── 0050.json       # periodic checkpoint

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::json::MrNote;
use crate::state::SessionState;

// ─── Paths ──────────────────────────────────────────────────────────────────

fn sessions_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr").join("sessions")
}

fn active_session_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr").join("active_session")
}

/// Read the active session ID from disk.
pub fn active_session_id() -> Option<String> {
    fs::read_to_string(active_session_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ─── Timestamps ─────────────────────────────────────────────────────────────

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn format_ts(ts: f64) -> String {
    let secs = ts as u64;
    let days = secs / 86400;
    let tod = secs % 86400;
    let (y, mo, d) = epoch_days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        mo,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

fn format_ts_short(ts: f64) -> String {
    let secs = ts as u64;
    let tod = secs % 86400;
    format!("{:02}:{:02}:{:02}", tod / 3600, (tod % 3600) / 60, tod % 60)
}

fn format_ts_date(ts: f64) -> String {
    let secs = ts as u64;
    let days = secs / 86400;
    let (y, mo, d) = epoch_days_to_date(days);
    format!("{:04}-{:02}-{:02}", y, mo, d)
}

fn epoch_days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut y = 1970u64;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if days < diy {
            break;
        }
        days -= diy;
        y += 1;
    }
    const DIM: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1u64;
    for m in 0..12 {
        let d = if m == 1 && is_leap(y) { 29 } else { DIM[m] };
        if days < d {
            mo = m as u64 + 1;
            break;
        }
        days -= d;
    }
    (y, mo, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn format_ago(ts: f64) -> String {
    let ago = now_epoch() - ts;
    if ago < 60.0 {
        format!("{}s ago", ago as u64)
    } else if ago < 3600.0 {
        format!("{}m ago", (ago / 60.0) as u64)
    } else if ago < 86400.0 {
        format!("{}h ago", (ago / 3600.0) as u64)
    } else {
        format!("{}d ago", (ago / 86400.0) as u64)
    }
}

// ─── Data types ─────────────────────────────────────────────────────────────

/// AI session tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSession {
    pub provider: String,
    pub session_id: String,
    pub joined: String,
}

/// Session metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub created: String,
    pub name: String,
    pub ai_sessions: Vec<AiSession>,
}

/// A captured slice of state (before or after a mutation).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum StateCapture {
    #[serde(rename = "clip")]
    Clip { notes: Vec<MrNote>, length: f64 },

    #[serde(rename = "mix")]
    Mix {
        volume: f64,
        pan: f64,
        mute: bool,
        solo: bool,
    },

    #[serde(rename = "send")]
    Send { return_idx: i32, level: f64 },

    #[serde(rename = "param")]
    Param {
        device: i32,
        param: String,
        value: f64,
    },

    #[serde(rename = "tempo")]
    Tempo { value: f64 },

    #[serde(rename = "automation")]
    Automation {
        device: i32,
        param: String,
        points: Vec<[f64; 2]>,
    },

    /// No state to capture (new clip, new track, effect load, etc.)
    #[serde(rename = "empty")]
    Empty,
}

/// Identifies what was affected by a mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_idx: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<i32>,
}

/// A single entry in the history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub seq: u64,
    pub ts: f64,
    pub cmd: String,
    pub args: String,
    pub target: HistoryTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_session: Option<String>,
    pub before: StateCapture,
    pub after: StateCapture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Saved checkpoint (full session state at a point in time).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub seq: u64,
    pub ts: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub state: SessionState,
}

// ─── Ref parsing ────────────────────────────────────────────────────────────

/// A reference to a point in history.
#[derive(Debug)]
pub enum HistoryRef {
    /// Absolute sequence number: @47
    Seq(u64),
    /// Relative to latest: @-1 (last change), @-2 (second to last)
    Relative(i32),
    /// Time ago in seconds: @5m → 300.0, @1h → 3600.0
    TimeAgo(f64),
    /// Label search: @"groovy"
    Label(String),
    /// Current live state
    Current,
}

impl HistoryRef {
    /// Parse a ref string. The leading `@` is optional.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.strip_prefix('@').unwrap_or(s);
        if s == "now" || s == "current" {
            return Ok(HistoryRef::Current);
        }
        // @-N (negative relative)
        if let Some(rest) = s.strip_prefix('-') {
            if let Ok(n) = rest.parse::<i32>() {
                return Ok(HistoryRef::Relative(-(n.abs())));
            }
        }
        // @Nm (minutes) or @Nh (hours)
        if s.ends_with('m') {
            if let Ok(n) = s[..s.len() - 1].parse::<f64>() {
                return Ok(HistoryRef::TimeAgo(n * 60.0));
            }
        }
        if s.ends_with('h') {
            if let Ok(n) = s[..s.len() - 1].parse::<f64>() {
                return Ok(HistoryRef::TimeAgo(n * 3600.0));
            }
        }
        // @N (positive sequence number)
        if let Ok(n) = s.parse::<u64>() {
            return Ok(HistoryRef::Seq(n));
        }
        // Label (strip surrounding quotes)
        let label = s.trim_matches('"').trim_matches('\'').to_string();
        Ok(HistoryRef::Label(label))
    }
}

// ─── History manager ────────────────────────────────────────────────────────

/// Manages the history log for a single session.
pub struct History {
    dir: PathBuf,
    meta: SessionMeta,
    seq: u64,
    log_file: File,
    ai_session_id: Option<String>,
}

impl History {
    /// Initialize a new session. Creates directory, meta.json, empty log.
    pub fn init(name: &str, ai_session: Option<AiSession>) -> Result<Self> {
        let id = generate_session_id(name);
        let dir = sessions_dir().join(&id);
        fs::create_dir_all(&dir)?;
        fs::create_dir_all(dir.join("checkpoints"))?;

        let now = format_ts(now_epoch());
        let mut ai_sessions = Vec::new();
        let ai_session_id = ai_session.as_ref().map(|a| a.session_id.clone());
        if let Some(ai) = ai_session {
            ai_sessions.push(ai);
        }

        let meta = SessionMeta {
            id: id.clone(),
            created: now,
            name: name.to_string(),
            ai_sessions,
        };

        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(dir.join("meta.json"), meta_json)?;

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("log.jsonl"))?;

        // Mark as active session
        fs::write(active_session_path(), &id)?;

        Ok(History {
            dir,
            meta,
            seq: 0,
            log_file,
            ai_session_id,
        })
    }

    /// Load an existing session from disk.
    pub fn load(session_id: &str) -> Result<Self> {
        let dir = sessions_dir().join(session_id);
        if !dir.exists() {
            return Err(Error::Other(format!(
                "session \"{}\" not found",
                session_id
            )));
        }

        let meta_json = fs::read_to_string(dir.join("meta.json"))?;
        let meta: SessionMeta = serde_json::from_str(&meta_json)?;

        // Find the highest seq number from the log
        let mut seq = 0u64;
        let log_path = dir.join("log.jsonl");
        if log_path.exists() {
            let file = File::open(&log_path)?;
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
                    seq = seq.max(entry.seq);
                }
            }
        }

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        // Detect AI session for this process
        let ai_session_id = detect_ai_session();

        Ok(History {
            dir,
            meta,
            seq,
            log_file,
            ai_session_id,
        })
    }

    /// Append a history entry to the log.
    pub fn append(
        &mut self,
        cmd: &str,
        args: &str,
        target: HistoryTarget,
        before: StateCapture,
        after: StateCapture,
    ) -> Result<u64> {
        self.seq += 1;
        let entry = HistoryEntry {
            seq: self.seq,
            ts: now_epoch(),
            cmd: cmd.to_string(),
            args: args.to_string(),
            target,
            ai_session: self.ai_session_id.clone(),
            before,
            after,
            label: None,
        };

        let json = serde_json::to_string(&entry)?;
        self.log_file.write_all(json.as_bytes())?;
        self.log_file.write_all(b"\n")?;
        self.log_file.flush()?;

        Ok(self.seq)
    }

    /// Save a full-state checkpoint.
    pub fn checkpoint(&self, state: &SessionState, label: Option<&str>) -> Result<()> {
        let filename = format!("{:04}.json", self.seq);
        let path = self.dir.join("checkpoints").join(&filename);

        let cp = Checkpoint {
            seq: self.seq,
            ts: now_epoch(),
            label: label.map(String::from),
            state: state.clone(),
        };
        let json = serde_json::to_string_pretty(&cp)?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Add a label entry to the log.
    pub fn label_current(&mut self, label: &str) -> Result<u64> {
        self.seq += 1;
        let entry = HistoryEntry {
            seq: self.seq,
            ts: now_epoch(),
            cmd: "label".to_string(),
            args: label.to_string(),
            target: HistoryTarget {
                track: None,
                track_idx: None,
                slot: None,
            },
            ai_session: self.ai_session_id.clone(),
            before: StateCapture::Empty,
            after: StateCapture::Empty,
            label: Some(label.to_string()),
        };
        let json = serde_json::to_string(&entry)?;
        self.log_file.write_all(json.as_bytes())?;
        self.log_file.write_all(b"\n")?;
        self.log_file.flush()?;
        Ok(self.seq)
    }

    /// Register an AI session that is touching this music session.
    pub fn register_ai_session(&mut self, ai: AiSession) -> Result<()> {
        // Don't add duplicates
        if self
            .meta
            .ai_sessions
            .iter()
            .any(|a| a.session_id == ai.session_id)
        {
            return Ok(());
        }
        self.ai_session_id = Some(ai.session_id.clone());
        self.meta.ai_sessions.push(ai);
        let meta_json = serde_json::to_string_pretty(&self.meta)?;
        fs::write(self.dir.join("meta.json"), meta_json)?;
        Ok(())
    }

    /// Read all entries from the log.
    pub fn read_log(&self) -> Result<Vec<HistoryEntry>> {
        let log_path = self.dir.join("log.jsonl");
        if !log_path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&log_path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Query entries, optionally filtered by track and/or slot.
    pub fn query(
        &self,
        track: Option<&str>,
        slot: Option<i32>,
    ) -> Result<Vec<HistoryEntry>> {
        let entries = self.read_log()?;
        Ok(entries
            .into_iter()
            .filter(|e| {
                // Include label entries only in unfiltered queries
                if e.cmd == "label" {
                    return track.is_none() && slot.is_none();
                }
                if let Some(t) = track {
                    match &e.target.track {
                        Some(et) if et.eq_ignore_ascii_case(t) => {}
                        _ => return false,
                    }
                }
                if let Some(s) = slot {
                    match e.target.slot {
                        Some(es) if es == s => {}
                        _ => return false,
                    }
                }
                true
            })
            .collect())
    }

    /// Resolve a ref to a specific entry in the context of a target filter.
    pub fn resolve(
        &self,
        r: &HistoryRef,
        track: Option<&str>,
        slot: Option<i32>,
    ) -> Result<HistoryEntry> {
        let entries = self.query(track, slot)?;
        if entries.is_empty() {
            return Err(Error::Other("no history for this target".to_string()));
        }

        match r {
            HistoryRef::Current => {
                Err(Error::Other("@current: nothing to restore".to_string()))
            }
            HistoryRef::Seq(seq) => {
                // Find the most recent entry at or before this seq
                entries
                    .iter()
                    .rev()
                    .find(|e| e.seq <= *seq)
                    .cloned()
                    .ok_or_else(|| Error::Other(format!("no entry at or before @{}", seq)))
            }
            HistoryRef::Relative(n) => {
                // -1 = most recent, -2 = second most recent, etc.
                let idx = entries.len() as i32 + n;
                if idx < 0 || idx >= entries.len() as i32 {
                    return Err(Error::Other(format!(
                        "ref @{} out of range ({} entries for this target)",
                        n,
                        entries.len()
                    )));
                }
                Ok(entries[idx as usize].clone())
            }
            HistoryRef::TimeAgo(secs) => {
                let cutoff = now_epoch() - secs;
                entries
                    .iter()
                    .rev()
                    .find(|e| e.ts <= cutoff)
                    .cloned()
                    .ok_or_else(|| {
                        Error::Other(format!("no entry from {}+ seconds ago", secs))
                    })
            }
            HistoryRef::Label(label) => {
                // Labels are session-level, so search the FULL log to find
                // the label's seq, then find the target's state at that point.
                let all_entries = self.read_log()?;
                let lower = label.to_lowercase();
                let label_entry = all_entries
                    .iter()
                    .rev()
                    .find(|e| {
                        e.label
                            .as_ref()
                            .map(|l| l.to_lowercase().contains(&lower))
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| {
                        Error::Other(format!("no entry labeled \"{}\"", label))
                    })?;
                let label_seq = label_entry.seq;
                // Now find the most recent target entry at or before that seq
                entries
                    .iter()
                    .rev()
                    .find(|e| e.seq <= label_seq)
                    .cloned()
                    .ok_or_else(|| {
                        Error::Other(format!(
                            "no entry for this target at or before @{} [{}]",
                            label_seq, label
                        ))
                    })
            }
        }
    }

    pub fn session_id(&self) -> &str {
        &self.meta.id
    }

    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    pub fn current_seq(&self) -> u64 {
        self.seq
    }
}

// ─── Session ID generation ──────────────────────────────────────────────────

fn generate_session_id(name: &str) -> String {
    let now = now_epoch() as u64;
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        format!("session-{}", now)
    } else {
        format!("{}-{}", slug, now)
    }
}

// ─── AI session detection ─────────────────────────────────────────────��─────

/// Detect the current Claude Code session by finding the most recently modified
/// .jsonl file in ~/.claude/projects/*/
pub fn detect_ai_session() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let claude_projects = PathBuf::from(&home).join(".claude").join("projects");
    if !claude_projects.exists() {
        return None;
    }

    let mut newest: Option<(String, std::time::SystemTime)> = None;

    for project_entry in fs::read_dir(&claude_projects).ok()?.flatten() {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        for file_entry in fs::read_dir(&project_path).ok()?.flatten() {
            let fpath = file_entry.path();
            if fpath.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(meta) = file_entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if newest.as_ref().map_or(true, |(_, t)| modified > *t) {
                        if let Some(stem) = fpath.file_stem() {
                            newest =
                                Some((stem.to_string_lossy().to_string(), modified));
                        }
                    }
                }
            }
        }
    }

    newest.map(|(id, _)| id)
}

/// Detect which AI provider is driving this process.
pub fn detect_ai_provider() -> &'static str {
    if std::env::var("CLAUDECODE").is_ok() {
        "claude-code"
    } else {
        "unknown"
    }
}

// ─── CLI functions ──────────────────────────────────────────────────────────

/// `mr session init <name>`
pub fn session_init(
    name: &str,
    ai_session_id: Option<&str>,
    ai_provider: Option<&str>,
) -> Result<()> {
    let ai_session = if let Some(id) = ai_session_id {
        let provider = ai_provider.unwrap_or_else(|| detect_ai_provider());
        Some(AiSession {
            provider: provider.to_string(),
            session_id: id.to_string(),
            joined: format_ts(now_epoch()),
        })
    } else {
        detect_ai_session().map(|id| AiSession {
            provider: detect_ai_provider().to_string(),
            session_id: id,
            joined: format_ts(now_epoch()),
        })
    };

    let history = History::init(name, ai_session)?;
    eprintln!("session \"{}\" started ({})", name, history.session_id());
    if let Some(ai) = history.meta.ai_sessions.first() {
        eprintln!("  ai: {} {}", ai.provider, ai.session_id);
    }
    Ok(())
}

/// `mr session list`
pub fn session_list() -> Result<()> {
    let dir = sessions_dir();
    if !dir.exists() {
        println!("no sessions");
        return Ok(());
    }

    let active = active_session_id();

    let mut sessions: Vec<(String, SessionMeta, usize)> = Vec::new();
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        if let Ok(json) = fs::read_to_string(&meta_path) {
            if let Ok(meta) = serde_json::from_str::<SessionMeta>(&json) {
                // Count log entries
                let log_path = path.join("log.jsonl");
                let count = if log_path.exists() {
                    BufReader::new(File::open(&log_path).unwrap_or_else(|_| {
                        File::open("/dev/null").unwrap()
                    }))
                    .lines()
                    .filter_map(|l| l.ok())
                    .filter(|l| !l.trim().is_empty())
                    .count()
                } else {
                    0
                };
                let dir_name = entry.file_name().to_string_lossy().to_string();
                sessions.push((dir_name, meta, count));
            }
        }
    }

    sessions.sort_by(|a, b| b.1.created.cmp(&a.1.created));

    if sessions.is_empty() {
        println!("no sessions");
        return Ok(());
    }

    for (dir_name, meta, count) in &sessions {
        let marker = if active.as_deref() == Some(dir_name.as_str()) {
            " *"
        } else {
            ""
        };
        println!(
            "  {}{} — {} — {} changes",
            meta.name,
            marker,
            &meta.created[..10.min(meta.created.len())],
            count
        );
        for ai in &meta.ai_sessions {
            let short_id = if ai.session_id.len() > 8 {
                &ai.session_id[..8]
            } else {
                &ai.session_id
            };
            println!("    {}: {}...", ai.provider, short_id);
        }
    }
    Ok(())
}

/// `mr session resume <name>`
pub fn session_resume(name: &str) -> Result<()> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Err(Error::Other("no sessions".to_string()));
    }

    let lower = name.to_lowercase();
    let mut found: Option<(String, SessionMeta)> = None;

    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        if let Ok(json) = fs::read_to_string(&meta_path) {
            if let Ok(meta) = serde_json::from_str::<SessionMeta>(&json) {
                if meta.name.to_lowercase().contains(&lower)
                    || meta.id.contains(name)
                {
                    found = Some((
                        entry.file_name().to_string_lossy().to_string(),
                        meta,
                    ));
                }
            }
        }
    }

    match found {
        Some((dir_name, meta)) => {
            // Also register current AI session
            let mut history = History::load(&dir_name)?;
            if let Some(ai_id) = detect_ai_session() {
                let _ = history.register_ai_session(AiSession {
                    provider: detect_ai_provider().to_string(),
                    session_id: ai_id,
                    joined: format_ts(now_epoch()),
                });
            }

            // Set as active
            fs::write(active_session_path(), &dir_name)?;
            eprintln!("session \"{}\" resumed", meta.name);
            if !meta.ai_sessions.is_empty() {
                eprintln!("AI sessions:");
                for ai in &meta.ai_sessions {
                    eprintln!("  claude --resume {}", ai.session_id);
                }
            }
            Ok(())
        }
        None => Err(Error::Other(format!("session \"{}\" not found", name))),
    }
}

/// `mr session end`
pub fn session_end() -> Result<()> {
    let path = active_session_path();
    if path.exists() {
        let id = fs::read_to_string(&path)?;
        fs::remove_file(&path)?;
        eprintln!("session ended ({})", id.trim());
    } else {
        eprintln!("no active session");
    }
    Ok(())
}

/// `mr history [target] [--since Nm] [--labeled]`
pub fn show_history(
    target: Option<&str>,
    since: Option<&str>,
    labeled_only: bool,
) -> Result<()> {
    let session_id = active_session_id().ok_or_else(|| {
        Error::Other("no active session — run `mr session init`".to_string())
    })?;
    let history = History::load(&session_id)?;

    let (track, slot) = parse_target_filter(target);
    let entries = history.query(track.as_deref(), slot)?;

    if entries.is_empty() {
        println!(
            "no history{}",
            target.map(|t| format!(" for {}", t)).unwrap_or_default()
        );
        return Ok(());
    }

    // Apply time filter
    let cutoff = if let Some(since) = since {
        let r = HistoryRef::parse(since)?;
        match r {
            HistoryRef::TimeAgo(secs) => Some(now_epoch() - secs),
            _ => None,
        }
    } else {
        None
    };

    for entry in &entries {
        if let Some(cutoff) = cutoff {
            if entry.ts < cutoff {
                continue;
            }
        }
        if labeled_only && entry.label.is_none() {
            continue;
        }

        let time = format_ts_short(entry.ts);
        let ago = format_ago(entry.ts);
        let target_str = format_target(&entry.target);
        let label_str = entry
            .label
            .as_ref()
            .map(|l| format!(" [{}]", l))
            .unwrap_or_default();
        let detail = format_capture_brief(&entry.after);
        println!(
            "  @{:<4} {} ({:>6}) {} {}{} — {}",
            entry.seq, time, ago, entry.cmd, target_str, label_str, detail
        );
    }

    Ok(())
}

/// `mr restore <target> <ref>`
pub fn restore_state(target: &str, ref_str: &str) -> Result<()> {
    let session_id = active_session_id()
        .ok_or_else(|| Error::Other("no active session".to_string()))?;
    let history = History::load(&session_id)?;

    let (track, slot) = parse_target_filter(Some(target));
    let track = track.ok_or_else(|| {
        Error::Other("restore requires a target (e.g. kick:0)".to_string())
    })?;

    let r = HistoryRef::parse(ref_str)?;
    let entry = history.resolve(&r, Some(&track), slot)?;

    // For @-N (undo), restore the `before` state.
    // For everything else, restore the `after` state (state at that point).
    let state_to_restore = match &r {
        HistoryRef::Relative(_) => &entry.before,
        _ => &entry.after,
    };

    match state_to_restore {
        StateCapture::Clip { notes, length } => {
            let slot = slot.unwrap_or(0);
            eprintln!(
                "restoring {}:{} to @{} ({} notes, {:.1} beats)",
                track,
                slot,
                entry.seq,
                notes.len(),
                length
            );
            if crate::daemon::is_running() {
                let resp =
                    crate::daemon::send_request(&crate::daemon::DaemonRequest {
                        cmd: "write".into(),
                        track: track.clone(),
                        slot,
                        length: *length,
                        name: format!("restored @{}", entry.seq),
                        notes: notes.clone(),
                        ..Default::default()
                    })?;
                if !resp.ok {
                    return Err(Error::Other(
                        resp.error.unwrap_or("restore failed".into()),
                    ));
                }
            } else {
                let session = crate::connect::connect()?;
                let track_idx = crate::connect::resolve_track(&session, &track)?;
                let track_obj = session.track(track_idx);
                if track_obj.has_clip(slot).unwrap_or(false) {
                    let _ = track_obj.delete_clip(slot);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                let clip = track_obj.create_clip(slot, *length as f32)?;
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = clip.set_name(&format!("restored @{}", entry.seq));
                let _ = clip.set_looping(true);
                let ableton_notes: Vec<ableton::Note> =
                    notes.iter().map(|n| n.to_ableton()).collect();
                for chunk in ableton_notes.chunks(80) {
                    clip.add_notes(chunk)?;
                }
                let _ = clip.fire();
            }
        }
        StateCapture::Mix {
            volume,
            pan,
            mute,
            solo,
        } => {
            eprintln!(
                "restoring {} mix to @{} (vol:{:.2} pan:{:.2})",
                track, entry.seq, volume, pan
            );
            if crate::daemon::is_running() {
                let resp =
                    crate::daemon::send_request(&crate::daemon::DaemonRequest {
                        cmd: "mix".into(),
                        track: track.clone(),
                        volume: Some(*volume),
                        pan: Some(*pan),
                        mute: Some(*mute),
                        solo: Some(*solo),
                        ..Default::default()
                    })?;
                if !resp.ok {
                    return Err(Error::Other(
                        resp.error.unwrap_or("restore failed".into()),
                    ));
                }
            } else {
                let session = crate::connect::connect()?;
                let track_idx =
                    crate::connect::resolve_track(&session, &track)?;
                let t = session.track(track_idx);
                t.set_volume(*volume as f32)?;
                t.set_panning(*pan as f32)?;
                t.set_mute(*mute)?;
                t.set_solo(*solo)?;
            }
        }
        StateCapture::Tempo { value } => {
            eprintln!("restoring tempo to {:.1} BPM", value);
            let session = crate::connect::connect()?;
            session.set_tempo(*value as f32)?;
        }
        StateCapture::Send { return_idx, level } => {
            eprintln!(
                "restoring {} send {} to {:.2}",
                track, return_idx, level
            );
            let session = crate::connect::connect()?;
            let track_idx = crate::connect::resolve_track(&session, &track)?;
            let t = session.track(track_idx);
            t.set_send(*return_idx, *level as f32)?;
        }
        StateCapture::Param {
            device,
            param,
            value,
        } => {
            eprintln!(
                "restoring {} {}:{} to {:.3}",
                track, device, param, value
            );
            if crate::daemon::is_running() {
                let resp =
                    crate::daemon::send_request(&crate::daemon::DaemonRequest {
                        cmd: "device_param".into(),
                        track: track.clone(),
                        device: *device,
                        params_kv: vec![[param.clone(), format!("{}", value)]],
                        ..Default::default()
                    })?;
                if !resp.ok {
                    return Err(Error::Other(
                        resp.error.unwrap_or("restore failed".into()),
                    ));
                }
            } else {
                let session = crate::connect::connect()?;
                let track_idx =
                    crate::connect::resolve_track(&session, &track)?;
                let t = session.track(track_idx);
                let dev = t.device(*device);
                let names = dev.parameter_names()?;
                if let Some(idx) = names.iter().position(|n| n == param) {
                    dev.set_param(idx as i32, *value as f32)?;
                }
            }
        }
        StateCapture::Empty => {
            return Err(Error::Other(
                "nothing to restore (empty state)".to_string(),
            ));
        }
        StateCapture::Automation { .. } => {
            return Err(Error::Other(
                "automation restore not yet supported".to_string(),
            ));
        }
    }

    Ok(())
}

/// `mr checkpoint [label]`
pub fn add_checkpoint(label: Option<&str>) -> Result<()> {
    let _session_id = active_session_id()
        .ok_or_else(|| Error::Other("no active session".to_string()))?;

    if crate::daemon::is_running() {
        let resp = crate::daemon::send_request(&crate::daemon::DaemonRequest {
            cmd: "history_checkpoint".into(),
            name: label.unwrap_or("").to_string(),
            ..Default::default()
        })?;
        if resp.ok {
            eprintln!("{}", resp.message.unwrap_or_default());
        } else {
            return Err(Error::Other(
                resp.error.unwrap_or("checkpoint failed".into()),
            ));
        }
    } else {
        let session_id = active_session_id().unwrap();
        let history = History::load(&session_id)?;
        let session = crate::connect::connect()?;
        let state = SessionState::from_session(&session);
        history.checkpoint(&state, label)?;
        eprintln!(
            "checkpoint @{} saved{}",
            history.current_seq(),
            label.map(|l| format!(" [{}]", l)).unwrap_or_default()
        );
    }
    Ok(())
}

/// `mr label <text>`
pub fn add_label(text: &str) -> Result<()> {
    let _session_id = active_session_id()
        .ok_or_else(|| Error::Other("no active session".to_string()))?;

    if crate::daemon::is_running() {
        let resp = crate::daemon::send_request(&crate::daemon::DaemonRequest {
            cmd: "history_label".into(),
            name: text.to_string(),
            ..Default::default()
        })?;
        if resp.ok {
            eprintln!("{}", resp.message.unwrap_or_default());
        } else {
            return Err(Error::Other(
                resp.error.unwrap_or("label failed".into()),
            ));
        }
    } else {
        let session_id = active_session_id().unwrap();
        let mut history = History::load(&session_id)?;
        history.label_current(text)?;
        eprintln!("labeled @{} \"{}\"", history.current_seq(), text);
    }
    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn parse_target_filter(target: Option<&str>) -> (Option<String>, Option<i32>) {
    match target {
        None => (None, None),
        Some(t) => {
            if let Some((track, slot_str)) = t.split_once(':') {
                (Some(track.to_string()), slot_str.parse().ok())
            } else {
                (Some(t.to_string()), None)
            }
        }
    }
}

fn format_target(target: &HistoryTarget) -> String {
    match (&target.track, target.slot) {
        (Some(t), Some(s)) => format!("{}:{}", t, s),
        (Some(t), None) => t.clone(),
        _ => "session".to_string(),
    }
}

fn format_capture_brief(capture: &StateCapture) -> String {
    match capture {
        StateCapture::Clip { notes, length } => {
            format!("{} notes, {:.1} beats", notes.len(), length)
        }
        StateCapture::Mix {
            volume, pan, mute, ..
        } => {
            format!("vol:{:.2} pan:{:.2} mute:{}", volume, pan, mute)
        }
        StateCapture::Send { return_idx, level } => {
            format!("return {} level:{:.2}", return_idx, level)
        }
        StateCapture::Param { param, value, .. } => {
            format!("{}: {:.3}", param, value)
        }
        StateCapture::Tempo { value } => format!("{:.1} BPM", value),
        StateCapture::Automation { param, points, .. } => {
            format!("{}: {} points", param, points.len())
        }
        StateCapture::Empty => "new".to_string(),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ref_parse_seq() {
        match HistoryRef::parse("@47").unwrap() {
            HistoryRef::Seq(n) => assert_eq!(n, 47),
            _ => panic!("expected Seq"),
        }
    }

    #[test]
    fn test_ref_parse_relative() {
        match HistoryRef::parse("@-1").unwrap() {
            HistoryRef::Relative(n) => assert_eq!(n, -1),
            _ => panic!("expected Relative"),
        }
        match HistoryRef::parse("@-3").unwrap() {
            HistoryRef::Relative(n) => assert_eq!(n, -3),
            _ => panic!("expected Relative"),
        }
    }

    #[test]
    fn test_ref_parse_time_ago() {
        match HistoryRef::parse("@5m").unwrap() {
            HistoryRef::TimeAgo(s) => assert!((s - 300.0).abs() < 0.1),
            _ => panic!("expected TimeAgo"),
        }
        match HistoryRef::parse("@2h").unwrap() {
            HistoryRef::TimeAgo(s) => assert!((s - 7200.0).abs() < 0.1),
            _ => panic!("expected TimeAgo"),
        }
    }

    #[test]
    fn test_ref_parse_current() {
        match HistoryRef::parse("@now").unwrap() {
            HistoryRef::Current => {}
            _ => panic!("expected Current"),
        }
        match HistoryRef::parse("@current").unwrap() {
            HistoryRef::Current => {}
            _ => panic!("expected Current"),
        }
    }

    #[test]
    fn test_ref_parse_label() {
        match HistoryRef::parse("@\"groovy\"").unwrap() {
            HistoryRef::Label(l) => assert_eq!(l, "groovy"),
            _ => panic!("expected Label"),
        }
        match HistoryRef::parse("@the-drop").unwrap() {
            HistoryRef::Label(l) => assert_eq!(l, "the-drop"),
            _ => panic!("expected Label"),
        }
    }

    #[test]
    fn test_generate_session_id() {
        let id = generate_session_id("acid jungle");
        assert!(id.starts_with("acid-jungle-"));
    }

    #[test]
    fn test_format_ts() {
        // 2024-01-01T00:00:00Z = 1704067200
        let ts = 1704067200.0;
        let s = format_ts(ts);
        assert!(s.starts_with("2024-01-01"));
    }

    #[test]
    fn test_state_capture_roundtrip() {
        let cap = StateCapture::Clip {
            notes: vec![MrNote {
                pitch: 60,
                start: 0.0,
                duration: 1.0,
                velocity: 100,
            }],
            length: 16.0,
        };
        let json = serde_json::to_string(&cap).unwrap();
        let parsed: StateCapture = serde_json::from_str(&json).unwrap();
        match parsed {
            StateCapture::Clip { notes, length } => {
                assert_eq!(notes.len(), 1);
                assert_eq!(length, 16.0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_history_entry_roundtrip() {
        let entry = HistoryEntry {
            seq: 1,
            ts: 1704067200.0,
            cmd: "write".to_string(),
            args: "kick:0".to_string(),
            target: HistoryTarget {
                track: Some("kick".to_string()),
                track_idx: Some(0),
                slot: Some(0),
            },
            ai_session: Some("abc123".to_string()),
            before: StateCapture::Empty,
            after: StateCapture::Clip {
                notes: vec![MrNote {
                    pitch: 36,
                    start: 0.0,
                    duration: 0.5,
                    velocity: 127,
                }],
                length: 16.0,
            },
            label: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: HistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, 1);
        assert_eq!(parsed.cmd, "write");
    }

    #[test]
    fn test_parse_target_filter() {
        let (t, s) = parse_target_filter(Some("kick:0"));
        assert_eq!(t.unwrap(), "kick");
        assert_eq!(s.unwrap(), 0);

        let (t, s) = parse_target_filter(Some("pad"));
        assert_eq!(t.unwrap(), "pad");
        assert!(s.is_none());

        let (t, s) = parse_target_filter(None);
        assert!(t.is_none());
        assert!(s.is_none());
    }

    #[test]
    fn test_format_capture_brief() {
        assert_eq!(
            format_capture_brief(&StateCapture::Tempo { value: 120.0 }),
            "120.0 BPM"
        );
        assert_eq!(
            format_capture_brief(&StateCapture::Empty),
            "new"
        );
    }

    #[test]
    fn test_session_meta_roundtrip() {
        let meta = SessionMeta {
            id: "test-123".to_string(),
            created: "2024-01-01T00:00:00Z".to_string(),
            name: "test session".to_string(),
            ai_sessions: vec![AiSession {
                provider: "claude-code".to_string(),
                session_id: "abc-123".to_string(),
                joined: "2024-01-01T00:00:00Z".to_string(),
            }],
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        let parsed: SessionMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-123");
        assert_eq!(parsed.ai_sessions.len(), 1);
    }

    #[test]
    fn test_history_file_roundtrip() {
        let dir = std::env::temp_dir().join("mr_test_history");
        let _ = fs::remove_dir_all(&dir);

        // Temporarily override sessions dir via the History struct directly
        let mut history = History {
            dir: dir.clone(),
            meta: SessionMeta {
                id: "test".to_string(),
                created: format_ts(now_epoch()),
                name: "test".to_string(),
                ai_sessions: vec![],
            },
            seq: 0,
            log_file: {
                fs::create_dir_all(&dir).unwrap();
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("log.jsonl"))
                    .unwrap()
            },
            ai_session_id: None,
        };

        // Append some entries
        history
            .append(
                "write",
                "kick:0",
                HistoryTarget {
                    track: Some("kick".into()),
                    track_idx: Some(0),
                    slot: Some(0),
                },
                StateCapture::Empty,
                StateCapture::Clip {
                    notes: vec![MrNote {
                        pitch: 36,
                        start: 0.0,
                        duration: 0.5,
                        velocity: 127,
                    }],
                    length: 16.0,
                },
            )
            .unwrap();

        history
            .append(
                "write",
                "pad:0",
                HistoryTarget {
                    track: Some("pad".into()),
                    track_idx: Some(1),
                    slot: Some(0),
                },
                StateCapture::Empty,
                StateCapture::Clip {
                    notes: vec![],
                    length: 8.0,
                },
            )
            .unwrap();

        history
            .append(
                "write",
                "kick:0",
                HistoryTarget {
                    track: Some("kick".into()),
                    track_idx: Some(0),
                    slot: Some(0),
                },
                StateCapture::Clip {
                    notes: vec![MrNote {
                        pitch: 36,
                        start: 0.0,
                        duration: 0.5,
                        velocity: 127,
                    }],
                    length: 16.0,
                },
                StateCapture::Clip {
                    notes: vec![
                        MrNote {
                            pitch: 36,
                            start: 0.0,
                            duration: 0.5,
                            velocity: 127,
                        },
                        MrNote {
                            pitch: 36,
                            start: 4.0,
                            duration: 0.5,
                            velocity: 100,
                        },
                    ],
                    length: 16.0,
                },
            )
            .unwrap();

        assert_eq!(history.current_seq(), 3);

        // Query all
        let all = history.read_log().unwrap();
        assert_eq!(all.len(), 3);

        // Query filtered
        let kick = history.query(Some("kick"), Some(0)).unwrap();
        assert_eq!(kick.len(), 2);

        let pad = history.query(Some("pad"), None).unwrap();
        assert_eq!(pad.len(), 1);

        // Resolve
        let entry = history
            .resolve(&HistoryRef::Seq(1), Some("kick"), Some(0))
            .unwrap();
        assert_eq!(entry.seq, 1);

        let entry = history
            .resolve(&HistoryRef::Relative(-1), Some("kick"), Some(0))
            .unwrap();
        assert_eq!(entry.seq, 3);

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }
}
