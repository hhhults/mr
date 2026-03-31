//! Persistent daemon for incremental Ableton updates.
//!
//! The daemon holds a Session connection and caches track/clip state.
//! Write and automate commands route through it for fast incremental updates
//! (clear+rewrite notes instead of recreating clips).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::{fs, process, thread, time::Duration};

use ableton::Session;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::json::MrNote;

// ─── Socket path ─────────────────────────────────────────────────────────────

fn socket_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr")
}

pub fn socket_path() -> PathBuf {
    socket_dir().join("mr.sock")
}

fn pid_path() -> PathBuf {
    socket_dir().join("mr.pid")
}

// ─── Protocol ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub cmd: String,
    #[serde(default)]
    pub track: String,
    #[serde(default)]
    pub slot: i32,
    #[serde(default)]
    pub length: f64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub param: String,
    #[serde(default)]
    pub device: i32,
    #[serde(default)]
    pub resolution: f64,
    #[serde(default)]
    pub notes: Vec<MrNote>,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_kind: Option<String>,
}

impl DaemonResponse {
    fn ok(message: impl Into<String>) -> Self {
        DaemonResponse {
            ok: true,
            message: Some(message.into()),
            error: None,
            update_kind: None,
        }
    }

    fn ok_with_kind(message: impl Into<String>, kind: impl Into<String>) -> Self {
        DaemonResponse {
            ok: true,
            message: Some(message.into()),
            error: None,
            update_kind: Some(kind.into()),
        }
    }

    fn err(error: impl Into<String>) -> Self {
        DaemonResponse {
            ok: false,
            message: None,
            error: Some(error.into()),
            update_kind: None,
        }
    }
}

// ─── Clip state cache ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ClipState {
    length: f64,
    note_hash: u64,
}

fn hash_notes(notes: &[MrNote]) -> u64 {
    // Simple hash: combine all note data
    let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
    for n in notes {
        h ^= n.pitch as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= (n.start * 1000.0) as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= (n.duration * 1000.0) as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= n.velocity as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ─── Daemon server ───────────────────────────────────────────────────────────

struct DaemonState {
    session: Session,
    track_cache: HashMap<String, i32>,
    clip_cache: HashMap<(i32, i32), ClipState>, // (track_idx, slot) → state
}

impl DaemonState {
    fn new(session: Session) -> Self {
        DaemonState {
            session,
            track_cache: HashMap::new(),
            clip_cache: HashMap::new(),
        }
    }

    fn refresh_track_cache(&mut self) -> Result<()> {
        let names = self.session.track_names()?;
        self.track_cache.clear();
        for (i, name) in names.iter().enumerate() {
            self.track_cache.insert(name.to_lowercase(), i as i32);
        }
        Ok(())
    }

    fn resolve_track(&mut self, name: &str) -> Result<i32> {
        let key = name.to_lowercase();
        if let Some(&idx) = self.track_cache.get(&key) {
            return Ok(idx);
        }
        // Cache miss — refresh
        self.refresh_track_cache()?;
        self.track_cache
            .get(&key)
            .copied()
            .ok_or_else(|| Error::TrackNotFound(name.to_string()))
    }

    fn handle_write(&mut self, req: &DaemonRequest) -> DaemonResponse {
        let track_idx = match self.resolve_track(&req.track) {
            Ok(i) => i,
            Err(e) => return DaemonResponse::err(e.to_string()),
        };

        let track = self.session.track(track_idx);
        let new_hash = hash_notes(&req.notes);
        let cache_key = (track_idx, req.slot);

        // Check clip state
        let update_kind = if let Some(cached) = self.clip_cache.get(&cache_key) {
            if cached.note_hash == new_hash && (cached.length - req.length).abs() < 0.01 {
                // Nothing changed
                return DaemonResponse::ok_with_kind(
                    format!("{}:{} unchanged", req.track, req.slot),
                    "no-op",
                );
            } else if (cached.length - req.length).abs() < 0.01 {
                "notes-only"
            } else {
                "recreate"
            }
        } else {
            "create"
        };

        let result: std::result::Result<&str, String> = match update_kind {
            "notes-only" => {
                // Fast path: just clear and rewrite notes
                let clip = track.clip(req.slot);
                if let Err(e) = clip.clear_notes() {
                    return DaemonResponse::err(format!("clear_notes: {}", e));
                }
                let ableton_notes: Vec<ableton::Note> = req.notes.iter()
                    .map(|n| ableton::Note::new(n.pitch, n.start as f32, n.duration as f32, n.velocity))
                    .collect();
                for chunk in ableton_notes.chunks(80) {
                    if let Err(e) = clip.add_notes(chunk) {
                        return DaemonResponse::err(format!("add_notes: {}", e));
                    }
                }
                Ok("notes-only")
            }
            _ => {
                // Full path: delete if exists, create, write
                if track.has_clip(req.slot).unwrap_or(false) {
                    let _ = track.delete_clip(req.slot);
                    thread::sleep(Duration::from_millis(50));
                }
                let clip = match track.create_clip(req.slot, req.length as f32) {
                    Ok(c) => c,
                    Err(e) => return DaemonResponse::err(format!("create_clip: {}", e)),
                };
                thread::sleep(Duration::from_millis(100));
                if !req.name.is_empty() {
                    let _ = clip.set_name(&req.name);
                }
                let _ = clip.set_looping(true);
                let ableton_notes: Vec<ableton::Note> = req.notes.iter()
                    .map(|n| ableton::Note::new(n.pitch, n.start as f32, n.duration as f32, n.velocity))
                    .collect();
                for chunk in ableton_notes.chunks(80) {
                    if let Err(e) = clip.add_notes(chunk) {
                        return DaemonResponse::err(format!("add_notes: {}", e));
                    }
                }
                let _ = clip.fire();
                Ok(update_kind)
            }
        };

        match result {
            Ok(kind) => {
                self.clip_cache.insert(cache_key, ClipState {
                    length: req.length,
                    note_hash: new_hash,
                });
                DaemonResponse::ok_with_kind(
                    format!("wrote {} notes to {}:{} ({:.1} beats)", req.notes.len(), req.track, req.slot, req.length),
                    kind,
                )
            }
            Err(e) => DaemonResponse::err(e.to_string()),
        }
    }

    fn handle_automate(&mut self, req: &DaemonRequest) -> DaemonResponse {
        let track_idx = match self.resolve_track(&req.track) {
            Ok(i) => i,
            Err(e) => return DaemonResponse::err(e.to_string()),
        };

        let track = self.session.track(track_idx);
        let device = track.device(req.device);

        // Find param index
        let param_names = match device.parameter_names() {
            Ok(n) => n,
            Err(e) => return DaemonResponse::err(format!("parameter_names: {}", e)),
        };
        let param_idx = param_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&req.param))
            .or_else(|| {
                param_names
                    .iter()
                    .position(|n| n.to_lowercase().contains(&req.param.to_lowercase()))
            });
        let param_idx = match param_idx {
            Some(i) => i as i32,
            None => return DaemonResponse::err(format!("parameter \"{}\" not found", req.param)),
        };

        let clip = track.clip(req.slot);
        let smooth_points: Vec<(f32, f32)> = req.points.iter()
            .map(|p| (p[0] as f32, p[1] as f32))
            .collect();
        let res = if req.resolution > 0.0 { req.resolution as f32 } else { 0.25 };

        if let Err(e) = clip.automate_smooth(req.device, param_idx, &smooth_points, res) {
            return DaemonResponse::err(format!("automate_smooth: {}", e));
        }

        DaemonResponse::ok_with_kind(
            format!("automated {} on {}:{} ({} points)", req.param, req.track, req.slot, req.points.len()),
            "automation",
        )
    }

    fn handle_request(&mut self, req: &DaemonRequest) -> DaemonResponse {
        match req.cmd.as_str() {
            "write" => self.handle_write(req),
            "automate" => self.handle_automate(req),
            "status" => {
                let tracks = self.track_cache.len();
                let clips = self.clip_cache.len();
                DaemonResponse::ok(format!("daemon running — {} tracks cached, {} clips tracked", tracks, clips))
            }
            "refresh" => {
                match self.refresh_track_cache() {
                    Ok(()) => DaemonResponse::ok(format!("refreshed — {} tracks", self.track_cache.len())),
                    Err(e) => DaemonResponse::err(e.to_string()),
                }
            }
            "shutdown" => DaemonResponse::ok("shutting down"),
            _ => DaemonResponse::err(format!("unknown command: {}", req.cmd)),
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Start the daemon. Blocks the current process (call from forked child).
pub fn run_daemon() -> Result<()> {
    let session = Session::connect().map_err(|e| Error::Connection(e.to_string()))?;
    let mut state = DaemonState::new(session);

    // Warm the cache
    state.refresh_track_cache()?;

    let sock = socket_path();
    let dir = socket_dir();
    fs::create_dir_all(&dir)?;

    // Remove stale socket
    if sock.exists() {
        fs::remove_file(&sock)?;
    }

    let listener = UnixListener::bind(&sock)?;
    eprintln!("mr daemon listening on {}", sock.display());

    // Write PID file
    fs::write(pid_path(), process::id().to_string())?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                match handle_connection(&mut state, stream) {
                    ControlFlow::Continue => {}
                    ControlFlow::Shutdown => break,
                }
            }
            Err(e) => {
                eprintln!("daemon: connection error: {}", e);
            }
        }
    }

    // Cleanup
    let _ = fs::remove_file(&sock);
    let _ = fs::remove_file(pid_path());
    eprintln!("mr daemon stopped");
    Ok(())
}

enum ControlFlow {
    Continue,
    Shutdown,
}

fn handle_connection(state: &mut DaemonState, stream: UnixStream) -> ControlFlow {
    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => return ControlFlow::Continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: DaemonRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = DaemonResponse::err(format!("bad request: {}", e));
                let _ = write_response(&stream, &resp);
                return ControlFlow::Continue;
            }
        };

        let is_shutdown = req.cmd == "shutdown";
        let resp = state.handle_request(&req);
        let _ = write_response(&stream, &resp);

        if is_shutdown {
            return ControlFlow::Shutdown;
        }
    }
    ControlFlow::Continue
}

fn write_response(mut stream: &UnixStream, resp: &DaemonResponse) -> Result<()> {
    let json = serde_json::to_string(resp)?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

// ─── Client functions ────────────────────────────────────────────────────────

/// Check if the daemon is running.
pub fn is_running() -> bool {
    socket_path().exists() && UnixStream::connect(socket_path()).is_ok()
}

/// Send a request to the daemon and get the response.
pub fn send_request(req: &DaemonRequest) -> Result<DaemonResponse> {
    let stream = UnixStream::connect(socket_path())
        .map_err(|e| Error::Other(format!("cannot connect to daemon: {}", e)))?;

    let json = serde_json::to_string(req)?;
    let mut writer = &stream;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    // Shutdown the write half so daemon knows we're done
    stream.shutdown(std::net::Shutdown::Write)?;

    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let resp: DaemonResponse = serde_json::from_str(&line)?;
        return Ok(resp);
    }

    Err(Error::Other("no response from daemon".into()))
}

/// Start the daemon in the background.
pub fn start() -> Result<()> {
    if is_running() {
        eprintln!("daemon already running");
        return Ok(());
    }

    // Fork to background
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            return Err(Error::Other("fork failed".into()));
        }
        if pid > 0 {
            // Parent: wait a moment for child to start
            thread::sleep(Duration::from_millis(500));
            if is_running() {
                eprintln!("daemon started (pid {})", pid);
            } else {
                eprintln!("daemon failed to start");
            }
            return Ok(());
        }
        // Child: become session leader
        libc::setsid();
    }

    // Child process: run the daemon
    if let Err(e) = run_daemon() {
        eprintln!("daemon error: {}", e);
        process::exit(1);
    }
    process::exit(0);
}

/// Stop the daemon.
pub fn stop() -> Result<()> {
    if !is_running() {
        eprintln!("daemon not running");
        return Ok(());
    }

    let resp = send_request(&DaemonRequest {
        cmd: "shutdown".into(),
        ..Default::default()
    })?;

    if resp.ok {
        eprintln!("daemon stopped");
        // Clean up socket if still there
        thread::sleep(Duration::from_millis(100));
        let _ = fs::remove_file(socket_path());
    } else {
        eprintln!("shutdown failed: {}", resp.error.unwrap_or_default());
    }
    Ok(())
}

/// Query daemon status.
pub fn status() -> Result<()> {
    if !is_running() {
        println!("daemon: not running");
        return Ok(());
    }

    let resp = send_request(&DaemonRequest {
        cmd: "status".into(),
        ..Default::default()
    })?;

    println!("daemon: {}", resp.message.unwrap_or_else(|| "running".into()));
    Ok(())
}

// Default impl for DaemonRequest (needed for shutdown/status requests)
impl Default for DaemonRequest {
    fn default() -> Self {
        DaemonRequest {
            cmd: String::new(),
            track: String::new(),
            slot: 0,
            length: 0.0,
            name: String::new(),
            param: String::new(),
            device: 0,
            resolution: 0.25,
            notes: Vec::new(),
            points: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_notes_deterministic() {
        let notes = vec![
            MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 },
            MrNote { pitch: 64, start: 1.0, duration: 1.0, velocity: 80 },
        ];
        let h1 = hash_notes(&notes);
        let h2 = hash_notes(&notes);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_notes_different() {
        let notes1 = vec![
            MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 },
        ];
        let notes2 = vec![
            MrNote { pitch: 61, start: 0.0, duration: 1.0, velocity: 100 },
        ];
        assert_ne!(hash_notes(&notes1), hash_notes(&notes2));
    }

    #[test]
    fn test_socket_path() {
        let path = socket_path();
        assert!(path.to_string_lossy().contains("mr.sock"));
    }

    #[test]
    fn test_daemon_request_roundtrip() {
        let req = DaemonRequest {
            cmd: "write".into(),
            track: "pad".into(),
            slot: 0,
            length: 16.0,
            name: "Melody".into(),
            notes: vec![
                MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cmd, "write");
        assert_eq!(parsed.track, "pad");
        assert_eq!(parsed.notes.len(), 1);
    }

    #[test]
    fn test_daemon_response_serialization() {
        let resp = DaemonResponse::ok_with_kind("wrote 5 notes", "notes-only");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("notes-only"));
        assert!(json.contains("wrote 5 notes"));

        let resp = DaemonResponse::err("track not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("track not found"));
        assert!(json.contains("\"ok\":false"));
    }
}
