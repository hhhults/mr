use ableton::Session;

use crate::daemon;
use crate::daemon_transport::DaemonTransport;
use crate::error::{Error, Result};

/// Connect to Ableton Live.
/// If the daemon is running, routes through it (shared connection, cached state).
/// Otherwise falls back to direct UDP.
pub fn connect() -> Result<Session> {
    if daemon::is_running() {
        let transport = DaemonTransport::connect()
            .map_err(|e| Error::Connection(format!("daemon: {}", e)))?;
        Ok(Session::with_transport(transport))
    } else {
        Session::connect().map_err(|e| Error::Connection(e.to_string()))
    }
}

/// Resolve a track name to its index by querying Ableton.
pub fn resolve_track(session: &Session, name: &str) -> Result<i32> {
    if let Some(idx_str) = name.strip_prefix('#') {
        return idx_str
            .parse::<i32>()
            .map_err(|_| Error::TrackNotFound(name.to_string()));
    }

    let names = session.track_names()?;
    names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|i| i as i32)
        .ok_or_else(|| {
            let available = names.join(", ");
            Error::Other(format!(
                "track \"{}\" not found\navailable: {}",
                name, available
            ))
        })
}

/// Resolve a return track identifier (A, B, C or index) to its index.
pub fn resolve_return(_session: &Session, name: &str) -> Result<i32> {
    if name.len() == 1 {
        let ch = name.chars().next().unwrap().to_ascii_uppercase();
        if ch.is_ascii_uppercase() {
            return Ok((ch as i32) - ('A' as i32));
        }
    }
    if let Some(idx_str) = name.strip_prefix('#') {
        return idx_str
            .parse::<i32>()
            .map_err(|_| Error::ReturnTrackNotFound(name.to_string()));
    }
    name.parse::<i32>()
        .map_err(|_| Error::ReturnTrackNotFound(name.to_string()))
}

/// Convert a MIDI note number to its name (e.g., 36 → "C1", 60 → "C3").
pub fn midi_note_name(note: i32) -> String {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (note / 12) - 2; // MIDI note 0 = C-2, 60 = C3
    let name_idx = (note % 12) as usize;
    format!("{}{}", names[name_idx], octave)
}

/// Parse a "track:slot" target string.
pub fn parse_target(target: &str) -> Result<(&str, i32)> {
    let parts: Vec<&str> = target.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(Error::BadTarget(target.to_string()));
    }
    let slot = parts[1]
        .parse::<i32>()
        .map_err(|_| Error::BadTarget(target.to_string()))?;
    Ok((parts[0], slot))
}
