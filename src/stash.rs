use std::fs;
use std::path::PathBuf;

use crate::error::Result;
use crate::json::{read_stdin, write_stdout};

fn stash_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr").join("stash")
}

fn stash_path(name: &str) -> PathBuf {
    stash_dir().join(format!("{}.json", name))
}

/// Save stdin JSON to a named stash file.
pub fn save(name: &str) -> Result<()> {
    let data = read_stdin()?;
    let dir = stash_dir();
    fs::create_dir_all(&dir)?;

    let path = stash_path(name);
    let json = serde_json::to_string_pretty(&data)?;
    fs::write(&path, json)?;

    eprintln!("saved \"{}\" → {}", name, path.display());
    Ok(())
}

/// Load a named stash file to stdout JSON.
pub fn load(name: &str) -> Result<()> {
    let path = stash_path(name);
    if !path.exists() {
        // List available stashes
        let dir = stash_dir();
        let available = if dir.exists() {
            fs::read_dir(&dir)?
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    e.path()
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            "(none)".to_string()
        };
        return Err(crate::error::Error::Other(format!(
            "stash \"{}\" not found\navailable: {}",
            name, available
        )));
    }

    let json = fs::read_to_string(&path)?;
    let data: crate::json::MrData = serde_json::from_str(&json)?;
    write_stdout(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stash_path() {
        let path = stash_path("melody");
        assert!(path.to_string_lossy().contains("melody.json"));
        assert!(path.to_string_lossy().contains(".mr/stash"));
    }

    #[test]
    fn test_stash_dir_location() {
        let dir = stash_dir();
        assert!(dir.to_string_lossy().contains(".mr/stash"));
    }

    #[test]
    fn test_stash_file_roundtrip() {
        use crate::json::{MrData, MrNote};

        let dir = std::env::temp_dir().join("mr_test_stash");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_melody.json");

        let data = MrData::Pattern {
            notes: vec![
                MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 },
                MrNote { pitch: 64, start: 1.0, duration: 1.0, velocity: 80 },
            ],
        };

        let json = serde_json::to_string_pretty(&data).unwrap();
        std::fs::write(&path, &json).unwrap();

        let loaded_json = std::fs::read_to_string(&path).unwrap();
        let loaded: MrData = serde_json::from_str(&loaded_json).unwrap();
        assert_eq!(loaded.type_name(), "pattern");
        let notes = loaded.into_pattern("test").unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].pitch, 60);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_stash_curve_roundtrip() {
        use crate::json::MrData;

        let dir = std::env::temp_dir().join("mr_test_stash_curve");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_filter.json");

        let data = MrData::Curve {
            points: vec![[0.0, 0.2], [4.0, 0.8], [8.0, 0.5]],
        };

        let json = serde_json::to_string(&data).unwrap();
        std::fs::write(&path, &json).unwrap();

        let loaded: MrData = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let points = loaded.into_curve("test").unwrap();
        assert_eq!(points.len(), 3);
        assert!((points[1][1] - 0.8).abs() < 1e-10);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
