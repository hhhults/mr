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
}
