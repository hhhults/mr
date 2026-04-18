//! Visual preset management — authored JSON files for recalling shader param bundles.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::viz;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizPreset {
    /// Shader parameter overrides (only listed params are sent; others left as-is).
    pub params: HashMap<String, f64>,
    /// Corpus to load (skipped if None).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus: Option<String>,
    /// Mosaic transport speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_speed: Option<f64>,
    /// Transition duration in seconds (uses set_param_lerp when > 0).
    #[serde(default)]
    pub transition_secs: Option<f64>,
}

fn presets_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr").join("viz-presets")
}

fn preset_path(name: &str) -> PathBuf {
    presets_dir().join(format!("{}.json", name))
}

pub fn load(name: &str) -> Result<()> {
    let path = preset_path(name);
    if !path.exists() {
        return Err(Error::Other(format!(
            "viz preset \"{}\" not found\navailable: {}",
            name,
            list_names()?.join(", ")
        )));
    }

    let json = fs::read_to_string(&path)?;
    let preset: VizPreset =
        serde_json::from_str(&json).map_err(|e| Error::Other(format!("parse error: {e}")))?;

    // 1. Load corpus first (so it's available for transport commands).
    if let Some(ref corpus) = preset.corpus {
        viz::corpus_load(corpus)?;
    }

    // 2. Set shader params (lerp if transition_secs is set).
    let use_lerp = preset
        .transition_secs
        .map(|s| s > 0.0)
        .unwrap_or(false);
    let duration = preset.transition_secs.unwrap_or(0.0);

    let mut param_names: Vec<&String> = preset.params.keys().collect();
    param_names.sort();

    for name in param_names {
        let value = preset.params[name];
        if use_lerp {
            viz::set_lerp(name, value, duration)?;
        } else {
            viz::set(name, value)?;
        }
    }

    // 3. Set transport speed.
    if let Some(speed) = preset.transport_speed {
        viz::transport_speed(speed)?;
    }

    eprintln!("viz preset loaded \"{}\"", name);
    Ok(())
}

pub fn list() -> Result<()> {
    let names = list_names()?;
    if names.is_empty() {
        println!("no viz presets");
        return Ok(());
    }

    for name in &names {
        let path = preset_path(name);
        if let Ok(json) = fs::read_to_string(&path) {
            if let Ok(preset) = serde_json::from_str::<VizPreset>(&json) {
                let corpus = preset.corpus.as_deref().unwrap_or("-");
                let speed = preset
                    .transport_speed
                    .map(|s| format!("{s:.2}"))
                    .unwrap_or_else(|| "-".into());
                let n_params = preset.params.len();
                println!("  {name} — corpus: {corpus}, speed: {speed}, {n_params} params");
            }
        }
    }

    Ok(())
}

fn list_names() -> Result<Vec<String>> {
    let dir = presets_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    collect_presets(&dir, &dir)
}

/// Recursively collect preset names relative to the base dir.
fn collect_presets(base: &PathBuf, dir: &PathBuf) -> Result<Vec<String>> {
    let mut names = Vec::new();
    if !dir.exists() {
        return Ok(names);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            names.extend(collect_presets(base, &path)?);
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .with_extension("");
            names.push(rel.to_string_lossy().to_string());
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_path() {
        let path = preset_path("forge/drip");
        assert!(path.to_string_lossy().contains("viz-presets/forge/drip.json"));
    }

    #[test]
    fn test_preset_roundtrip() {
        let mut params = HashMap::new();
        params.insert("feedback_decay".into(), 0.98);
        params.insert("bg_energy".into(), 0.1);
        let preset = VizPreset {
            params,
            corpus: Some("03_biological_rhyme".into()),
            transport_speed: Some(0.03),
            transition_secs: Some(4.0),
        };
        let json = serde_json::to_string_pretty(&preset).unwrap();
        let parsed: VizPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.params.len(), 2);
        assert_eq!(parsed.corpus.as_deref(), Some("03_biological_rhyme"));
        assert_eq!(parsed.transport_speed, Some(0.03));
        assert_eq!(parsed.transition_secs, Some(4.0));
    }

    #[test]
    fn test_preset_minimal() {
        let json = r#"{"params":{"bg_energy":0.5}}"#;
        let preset: VizPreset = serde_json::from_str(json).unwrap();
        assert_eq!(preset.params.len(), 1);
        assert!(preset.corpus.is_none());
        assert!(preset.transport_speed.is_none());
        assert!(preset.transition_secs.is_none());
    }
}
