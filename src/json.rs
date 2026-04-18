use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

fn default_probability() -> f64 { 1.0 }
fn is_one(v: &f64) -> bool { (*v - 1.0).abs() < 1e-9 }
fn default_release() -> f64 { 64.0 }
fn is_default_release(v: &f64) -> bool { (*v - 64.0).abs() < 1e-9 }
fn is_zero_f(v: &f64) -> bool { v.abs() < 1e-9 }

/// A MIDI note in the pipe format.
///
/// Expressive fields (probability, velocity_deviation, release_velocity) are
/// optional and default to Live's "no-op" values. They are skipped in JSON
/// output when at default, keeping the pipe format backwards-compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrNote {
    pub pitch: i32,
    pub start: f64,
    pub duration: f64,
    pub velocity: i32,
    #[serde(default = "default_probability", skip_serializing_if = "is_one")]
    pub probability: f64,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub velocity_deviation: f64,
    #[serde(default = "default_release", skip_serializing_if = "is_default_release")]
    pub release_velocity: f64,
}

impl Default for MrNote {
    fn default() -> Self {
        Self {
            pitch: 60,
            start: 0.0,
            duration: 1.0,
            velocity: 100,
            probability: 1.0,
            velocity_deviation: 0.0,
            release_velocity: 64.0,
        }
    }
}

impl MrNote {
    /// Convert to the ableton-rs Note, carrying all expressive fields.
    pub fn to_ableton(&self) -> ableton::Note {
        ableton::Note {
            pitch: self.pitch,
            start: self.start as f32,
            duration: self.duration as f32,
            velocity: self.velocity,
            mute: false,
            probability: self.probability as f32,
            velocity_deviation: self.velocity_deviation as f32,
            release_velocity: self.release_velocity as f32,
        }
    }
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
                MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100, ..Default::default() },
                MrNote { pitch: 64, start: 1.0, duration: 1.0, velocity: 80, ..Default::default() },
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

    #[test]
    fn test_into_curve_success() {
        let data = MrData::Curve { points: vec![[0.0, 0.5], [1.0, 0.8]] };
        let points = data.into_curve("test").unwrap();
        assert_eq!(points.len(), 2);
    }

    #[test]
    fn test_into_curve_wrong_type() {
        let data = MrData::Pattern { notes: vec![] };
        let err = data.into_curve("test").unwrap_err();
        assert!(err.to_string().contains("curve"));
        assert!(err.to_string().contains("pattern"));
    }

    #[test]
    fn test_into_progression_success() {
        let data = MrData::Progression {
            chords: vec![MrChord {
                root: "C".into(),
                quality: "major".into(),
                root_midi: 60,
            }],
        };
        let chords = data.into_progression("test").unwrap();
        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].root, "C");
    }

    #[test]
    fn test_into_progression_wrong_type() {
        let data = MrData::Curve { points: vec![] };
        let err = data.into_progression("test").unwrap_err();
        assert!(err.to_string().contains("progression"));
    }

    #[test]
    fn test_into_pitches_success() {
        let data = MrData::Pitches { values: vec![60, 64, 67] };
        let values = data.into_pitches("test").unwrap();
        assert_eq!(values, vec![60, 64, 67]);
    }

    #[test]
    fn test_into_pitches_wrong_type() {
        let data = MrData::Pattern { notes: vec![] };
        assert!(data.into_pitches("test").is_err());
    }

    #[test]
    fn test_into_grains_success() {
        let data = MrData::Grains {
            source: "test.wav".into(),
            sample_rate: 44100,
            grains: vec![MrGrain {
                path: "grain_0.wav".into(),
                index: 0,
                start: 0.0,
                duration: 0.1,
                source: "test.wav".into(),
            }],
        };
        let (source, sr, grains) = data.into_grains("test").unwrap();
        assert_eq!(source, "test.wav");
        assert_eq!(sr, 44100);
        assert_eq!(grains.len(), 1);
    }

    #[test]
    fn test_into_grains_wrong_type() {
        let data = MrData::Curve { points: vec![] };
        assert!(data.into_grains("test").is_err());
    }

    #[test]
    fn test_type_name_all_variants() {
        assert_eq!(MrData::Pattern { notes: vec![] }.type_name(), "pattern");
        assert_eq!(MrData::Curve { points: vec![] }.type_name(), "curve");
        assert_eq!(MrData::Progression { chords: vec![] }.type_name(), "progression");
        assert_eq!(MrData::Pitches { values: vec![] }.type_name(), "pitches");
        assert_eq!(MrData::Grains { source: String::new(), sample_rate: 0, grains: vec![] }.type_name(), "grains");
    }

    #[test]
    fn test_progression_roundtrip() {
        let data = MrData::Progression {
            chords: vec![
                MrChord { root: "C".into(), quality: "major".into(), root_midi: 60 },
                MrChord { root: "G".into(), quality: "dom7".into(), root_midi: 67 },
            ],
        };
        let json = serde_json::to_string(&data).unwrap();
        let parsed: MrData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.type_name(), "progression");
        let chords = parsed.into_progression("test").unwrap();
        assert_eq!(chords.len(), 2);
    }

    #[test]
    fn test_grains_roundtrip() {
        let data = MrData::Grains {
            source: "test.wav".into(),
            sample_rate: 48000,
            grains: vec![MrGrain {
                path: "g.wav".into(),
                index: 0,
                start: 0.5,
                duration: 0.2,
                source: "test.wav".into(),
            }],
        };
        let json = serde_json::to_string(&data).unwrap();
        let parsed: MrData = serde_json::from_str(&json).unwrap();
        let (src, sr, gs) = parsed.into_grains("test").unwrap();
        assert_eq!(src, "test.wav");
        assert_eq!(sr, 48000);
        assert_eq!(gs.len(), 1);
        assert!((gs[0].start - 0.5).abs() < 1e-10);
    }
}
