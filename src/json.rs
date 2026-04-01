use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A MIDI note in the pipe format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrNote {
    pub pitch: i32,
    pub start: f64,
    pub duration: f64,
    pub velocity: i32,
}

/// A chord in a progression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrChord {
    pub root: String,
    pub quality: String,
    pub root_midi: i32,
}

/// A grain reference in a slice manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrGrain {
    pub path: String,
    pub index: usize,
    pub start: f64,
    pub duration: f64,
    pub source: String,
}

/// The four data types that flow through pipes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MrData {
    #[serde(rename = "pattern")]
    Pattern { notes: Vec<MrNote> },

    #[serde(rename = "progression")]
    Progression { chords: Vec<MrChord> },

    #[serde(rename = "curve")]
    Curve { points: Vec<[f64; 2]> },

    #[serde(rename = "pitches")]
    Pitches { values: Vec<i32> },

    #[serde(rename = "grains")]
    Grains {
        source: String,
        sample_rate: u32,
        grains: Vec<MrGrain>,
    },
}

impl MrData {
    pub fn type_name(&self) -> &'static str {
        match self {
            MrData::Pattern { .. } => "pattern",
            MrData::Progression { .. } => "progression",
            MrData::Curve { .. } => "curve",
            MrData::Pitches { .. } => "pitches",
            MrData::Grains { .. } => "grains",
        }
    }

    /// Unwrap as pattern, or return a type mismatch error.
    pub fn into_pattern(self, command: &str) -> Result<Vec<MrNote>> {
        match self {
            MrData::Pattern { notes } => Ok(notes),
            other => Err(Error::TypeMismatch {
                command: command.to_string(),
                expected: "pattern".to_string(),
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Unwrap as curve, or return a type mismatch error.
    pub fn into_curve(self, command: &str) -> Result<Vec<[f64; 2]>> {
        match self {
            MrData::Curve { points } => Ok(points),
            other => Err(Error::TypeMismatch {
                command: command.to_string(),
                expected: "curve".to_string(),
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Unwrap as progression, or return a type mismatch error.
    pub fn into_progression(self, command: &str) -> Result<Vec<MrChord>> {
        match self {
            MrData::Progression { chords } => Ok(chords),
            other => Err(Error::TypeMismatch {
                command: command.to_string(),
                expected: "progression".to_string(),
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Unwrap as pitches, or return a type mismatch error.
    pub fn into_pitches(self, command: &str) -> Result<Vec<i32>> {
        match self {
            MrData::Pitches { values } => Ok(values),
            other => Err(Error::TypeMismatch {
                command: command.to_string(),
                expected: "pitches".to_string(),
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Unwrap as grains, or return a type mismatch error.
    pub fn into_grains(self, command: &str) -> Result<(String, u32, Vec<MrGrain>)> {
        match self {
            MrData::Grains {
                source,
                sample_rate,
                grains,
            } => Ok((source, sample_rate, grains)),
            other => Err(Error::TypeMismatch {
                command: command.to_string(),
                expected: "grains".to_string(),
                got: other.type_name().to_string(),
            }),
        }
    }
}

/// Read MrData from stdin. Returns None if stdin is a TTY (no pipe).
pub fn read_stdin() -> Result<MrData> {
    if atty_stdin() {
        return Err(Error::NoInput("write <target>".to_string()));
    }
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    let data: MrData = serde_json::from_str(&buf)?;
    Ok(data)
}

/// Write MrData to stdout as JSON.
pub fn write_stdout(data: &MrData) -> Result<()> {
    let json = serde_json::to_string(data)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(json.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

/// Check if stdin is a TTY (not piped).
pub(crate) fn atty_stdin() -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc::isatty(io::stdin().as_raw_fd()) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_roundtrip() {
        let data = MrData::Pattern {
            notes: vec![
                MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 },
                MrNote { pitch: 64, start: 1.0, duration: 1.0, velocity: 80 },
            ],
        };
        let json = serde_json::to_string(&data).unwrap();
        let parsed: MrData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.type_name(), "pattern");
    }

    #[test]
    fn test_curve_roundtrip() {
        let data = MrData::Curve {
            points: vec![[0.0, 0.5], [1.0, 0.8]],
        };
        let json = serde_json::to_string(&data).unwrap();
        let parsed: MrData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.type_name(), "curve");
    }

    #[test]
    fn test_type_mismatch() {
        let data = MrData::Curve { points: vec![] };
        let err = data.into_pattern("transpose").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("transpose"));
        assert!(msg.contains("pattern"));
        assert!(msg.contains("curve"));
    }
}
