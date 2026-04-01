use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use std::{fs, path::Path};

use crate::connect;
use crate::daemon::{self, DaemonRequest};
use crate::error::Result;

/// Stage a sample into Ableton's User Library so the browser can find it.
/// Returns the filename (not full path) to pass to load_sample.
pub(crate) fn stage_sample(file_path: &str) -> Result<String> {
    let src = PathBuf::from(shellexpand(file_path));
    if !src.exists() {
        return Err(crate::error::Error::Other(format!(
            "sample not found: {}",
            src.display()
        )));
    }

    let dest_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Music/Ableton/User Library/Samples/Imported");
    fs::create_dir_all(&dest_dir)?;

    let filename = src
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let dest = dest_dir.join(&filename);

    // Copy if missing or stale
    let should_copy = if dest.exists() {
        let src_mod = src.metadata()?.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let dst_mod = dest.metadata()?.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        dst_mod < src_mod
    } else {
        true
    };

    if should_copy {
        fs::copy(&src, &dest)?;
    }

    Ok(filename)
}

/// Expand ~ in paths
pub(crate) fn shellexpand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

pub fn create(
    name: &str,
    simpler: Option<&str>,
    instrument: Option<&str>,
    audio: bool,
) -> Result<()> {
    // Route through daemon if running
    if daemon::is_running() {
        let simpler_path = if let Some(s) = simpler {
            // Stage sample first (needs local filesystem access)
            let filename = stage_sample(s)?;
            let dest = dirs::home_dir()
                .unwrap_or_default()
                .join("Music/Ableton/User Library/Samples/Imported")
                .join(&filename);
            dest.to_string_lossy().to_string()
        } else {
            String::new()
        };

        let req = DaemonRequest {
            cmd: "track_create".into(),
            name: name.to_string(),
            instrument: instrument.unwrap_or("").to_string(),
            simpler: simpler_path,
            audio,
            ..Default::default()
        };
        let resp = daemon::send_request(&req)?;
        if resp.ok {
            eprintln!("{}", resp.message.unwrap_or_default());
        } else {
            eprintln!("error: {}", resp.error.unwrap_or_default());
        }
        return Ok(());
    }

    // Direct fallback
    let session = connect::connect()?;

    let track = if audio {
        session.create_audio_track(-1)?
    } else {
        session.create_midi_track(-1)?
    };
    thread::sleep(Duration::from_millis(150));

    track.set_name(name)?;

    if let Some(sample_path) = simpler {
        // Stage sample into User Library, then load by filename
        let filename = stage_sample(sample_path)?;
        thread::sleep(Duration::from_millis(300));

        // Try exact filename first
        let result = track.load_sample(&filename);
        if result.is_err() {
            // Try without extension
            let stem = Path::new(&filename)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            track.load_sample(&stem)?;
        }
        thread::sleep(Duration::from_millis(300));
        eprintln!("track \"{}\" [#{}] ← {}", name, track.track_idx, filename);
    } else if let Some(inst_name) = instrument {
        session.load_instrument(track.track_idx, inst_name)?;
        thread::sleep(Duration::from_millis(500));
        eprintln!("track \"{}\" [#{}] ← {}", name, track.track_idx, inst_name);
    } else {
        eprintln!("track \"{}\" [#{}]", name, track.track_idx);
    }

    Ok(())
}

pub fn effect(track_name: &str, effect_name: &str) -> Result<()> {
    // Route through daemon if running
    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "effect_load".into(),
            track: track_name.to_string(),
            name: effect_name.to_string(),
            ..Default::default()
        };
        let resp = daemon::send_request(&req)?;
        if resp.ok {
            eprintln!("{}", resp.message.unwrap_or_default());
        } else {
            eprintln!("error: {}", resp.error.unwrap_or_default());
        }
        return Ok(());
    }

    // Direct fallback
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    session.load_effect(idx, effect_name)?;
    thread::sleep(Duration::from_millis(300));
    eprintln!("effect \"{}\" → {}", effect_name, track_name);
    Ok(())
}

pub fn delete(name: &str) -> Result<()> {
    // Route through daemon if running
    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "track_delete".into(),
            track: name.to_string(),
            ..Default::default()
        };
        let resp = daemon::send_request(&req)?;
        if resp.ok {
            eprintln!("{}", resp.message.unwrap_or_default());
        } else {
            eprintln!("error: {}", resp.error.unwrap_or_default());
        }
        return Ok(());
    }

    // Direct fallback
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, name)?;
    session.delete_track(idx)?;
    eprintln!("deleted track \"{}\"", name);
    Ok(())
}

pub fn list() -> Result<()> {
    let session = connect::connect()?;
    let names = session.track_names()?;

    if names.is_empty() {
        eprintln!("no tracks");
        return Ok(());
    }

    // Batch-query all track info in one OSC cycle
    let info = session.batch_track_info(names.len() as i32)?;

    for (i, name) in names.iter().enumerate() {
        let (vol, pan, muted, _solo, armed) = info.get(i).copied().unwrap_or_default();

        let mut flags = Vec::new();
        if muted {
            flags.push("M");
        }
        if armed {
            flags.push("R");
        }
        let flag_str = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join(""))
        };

        let pan_str = if pan.abs() < 0.01 {
            "C".to_string()
        } else if pan < 0.0 {
            format!("L{:.0}", pan.abs() * 100.0)
        } else {
            format!("R{:.0}", pan * 100.0)
        };

        println!(
            "  #{:<2} {:<20} vol:{:.2}  pan:{}{}",
            i, name, vol, pan_str, flag_str
        );
    }

    Ok(())
}
