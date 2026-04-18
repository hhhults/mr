use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::connect;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub tempo: f32,
    pub tracks: Vec<TrackState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackState {
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub sends: Vec<f32>,
}

fn snapshots_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr").join("snapshots")
}

fn snapshot_path(name: &str) -> PathBuf {
    snapshots_dir().join(format!("{}.json", name))
}

pub fn save(name: &str) -> Result<()> {
    let session = connect::connect()?;
    let tempo = session.get_tempo()?;
    let track_names = session.track_names()?;
    let return_names = session.return_track_names()?;
    let num_returns = return_names.len() as i32;

    let mut tracks = Vec::new();
    for (i, tname) in track_names.iter().enumerate() {
        let track = session.track(i as i32);
        let volume = track.get_volume()?;
        let pan = track.get_panning()?;
        let mute = track.get_mute()?;

        let mut sends = Vec::new();
        for s in 0..num_returns {
            let level = track.get_send(s)?;
            sends.push(level);
        }

        tracks.push(TrackState {
            name: tname.clone(),
            volume,
            pan,
            mute,
            sends,
        });
    }

    let num_tracks = tracks.len();
    let snapshot = Snapshot { tempo, tracks };

    let dir = snapshots_dir();
    fs::create_dir_all(&dir)?;

    let path = snapshot_path(name);
    let json = serde_json::to_string_pretty(&snapshot)?;
    fs::write(&path, json)?;

    eprintln!(
        "snapshot saved \"{}\" ({} tracks, {:.1} BPM)",
        name,
        num_tracks,
        tempo
    );
    Ok(())
}

pub fn restore(name: &str) -> Result<()> {
    let path = snapshot_path(name);
    if !path.exists() {
        return Err(Error::Other(format!(
            "snapshot \"{}\" not found\navailable: {}",
            name,
            list_names()?.join(", ")
        )));
    }

    let json = fs::read_to_string(&path)?;
    let snapshot: Snapshot = serde_json::from_str(&json)?;

    let session = connect::connect()?;
    session.set_tempo(snapshot.tempo)?;

    let track_names = session.track_names()?;
    let mut matched = 0;
    let mut skipped = Vec::new();

    for saved in &snapshot.tracks {
        // Match by name, case-insensitive
        let idx = track_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&saved.name));

        match idx {
            Some(i) => {
                let track = session.track(i as i32);
                track.set_volume(saved.volume)?;
                track.set_panning(saved.pan)?;
                track.set_mute(saved.mute)?;

                for (s, &level) in saved.sends.iter().enumerate() {
                    // Silently skip if session has fewer returns than snapshot
                    let _ = track.set_send(s as i32, level);
                }
                matched += 1;
            }
            None => {
                skipped.push(saved.name.clone());
            }
        }
    }

    eprintln!(
        "snapshot restored \"{}\" ({} tracks, {:.1} BPM)",
        name, matched, snapshot.tempo
    );
    if !skipped.is_empty() {
        eprintln!("  skipped (not in session): {}", skipped.join(", "));
    }

    Ok(())
}

pub fn list() -> Result<()> {
    let names = list_names()?;
    if names.is_empty() {
        println!("no snapshots");
        return Ok(());
    }

    for name in &names {
        let path = snapshot_path(name);
        let json = fs::read_to_string(&path)?;
        let snapshot: Snapshot = serde_json::from_str(&json)?;
        let tracks: Vec<&str> = snapshot.tracks.iter().map(|t| t.name.as_str()).collect();
        println!(
            "  {} — {:.1} BPM, {} tracks ({})",
            name,
            snapshot.tempo,
            snapshot.tracks.len(),
            tracks.join(", ")
        );
    }

    Ok(())
}

pub fn delete(name: &str) -> Result<()> {
    let path = snapshot_path(name);
    if !path.exists() {
        return Err(Error::Other(format!(
            "snapshot \"{}\" not found\navailable: {}",
            name,
            list_names()?.join(", ")
        )));
    }

    fs::remove_file(&path)?;
    eprintln!("snapshot deleted \"{}\"", name);
    Ok(())
}

fn list_names() -> Result<Vec<String>> {
    let dir = snapshots_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                path.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_path() {
        let path = snapshot_path("garden_peak");
        assert!(path.to_string_lossy().contains("garden_peak.json"));
        assert!(path.to_string_lossy().contains(".mr/snapshots"));
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let snapshot = Snapshot {
            tempo: 120.0,
            tracks: vec![
                TrackState {
                    name: "bass".to_string(),
                    volume: 0.55,
                    pan: 0.0,
                    mute: false,
                    sends: vec![0.1, 0.15],
                },
                TrackState {
                    name: "pad".to_string(),
                    volume: 0.7,
                    pan: -0.3,
                    mute: false,
                    sends: vec![0.4, 0.0],
                },
            ],
        };
        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        let parsed: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tempo, 120.0);
        assert_eq!(parsed.tracks.len(), 2);
        assert_eq!(parsed.tracks[0].name, "bass");
        assert_eq!(parsed.tracks[0].volume, 0.55);
        assert_eq!(parsed.tracks[1].pan, -0.3);
        assert_eq!(parsed.tracks[1].sends, vec![0.4, 0.0]);
    }

    #[test]
    fn test_snapshot_file_roundtrip() {
        let dir = std::env::temp_dir().join("mr_test_snapshots");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_snap.json");

        let snapshot = Snapshot {
            tempo: 135.0,
            tracks: vec![TrackState {
                name: "kick".into(),
                volume: 0.85,
                pan: 0.0,
                mute: false,
                sends: vec![0.2],
            }],
        };
        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        fs::write(&path, &json).unwrap();

        let loaded_json = fs::read_to_string(&path).unwrap();
        let loaded: Snapshot = serde_json::from_str(&loaded_json).unwrap();
        assert_eq!(loaded.tempo, 135.0);
        assert_eq!(loaded.tracks[0].name, "kick");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_snapshot_empty_tracks() {
        let snapshot = Snapshot {
            tempo: 100.0,
            tracks: vec![],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tracks.len(), 0);
    }

    #[test]
    fn test_snapshot_mute_field() {
        let snapshot = Snapshot {
            tempo: 120.0,
            tracks: vec![TrackState {
                name: "test".into(),
                volume: 0.5,
                pan: 0.0,
                mute: true,
                sends: vec![],
            }],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: Snapshot = serde_json::from_str(&json).unwrap();
        assert!(parsed.tracks[0].mute);
    }

    #[test]
    fn test_list_names_empty_dir() {
        // list_names with non-existent dir should return empty
        let orig = std::env::var("HOME").unwrap_or_default();
        // Don't actually modify HOME; just test the path logic
        let dir = snapshots_dir();
        // If dir doesn't exist, list_names returns empty vec
        if !dir.exists() {
            let names = list_names().unwrap();
            assert!(names.is_empty());
        }
    }
}
