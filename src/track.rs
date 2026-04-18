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

pub fn rename(track_name: &str, new_name: &str) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);
    track.set_name(new_name)?;
    eprintln!("renamed \"{}\" → \"{}\"", track_name, new_name);
    Ok(())
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

pub fn pad_sample(track_name: &str, pad_note: i32, sample_path: &str) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;

    // Stage sample into User Library
    let filename = stage_sample(sample_path)?;
    thread::sleep(Duration::from_millis(300));

    // Try exact filename first, then stem
    let result = session.load_sample_pad(idx, 0, pad_note, &filename);
    if result.is_err() {
        let stem = Path::new(&filename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        session.load_sample_pad(idx, 0, pad_note, &stem)?;
    }
    thread::sleep(Duration::from_millis(300));
    eprintln!("pad {} ← {} on \"{}\"", pad_note, filename, track_name);
    Ok(())
}

// ---------- Simpler slice helpers ----------

pub fn slices(track_name: &str, pad: Option<i32>) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let length = session.simpler_sample_length(idx, 0, pad).unwrap_or(0.0);
    let positions = session.simpler_slices(idx, 0, pad)?;
    eprintln!(
        "simpler {}{} — sample {} samples — {} slices",
        track_name,
        pad.map(|p| format!(":pad{}", p)).unwrap_or_default(),
        length as i64,
        positions.len()
    );
    for (i, p) in positions.iter().enumerate() {
        eprintln!("  [{}] {:.0} samples", i, p);
    }
    Ok(())
}

pub fn slice_add(track_name: &str, pad: Option<i32>, time: f32) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    session.simpler_insert_slice(idx, 0, pad, time)?;
    eprintln!(
        "slice added at {:.0} on {}{}",
        time,
        track_name,
        pad.map(|p| format!(":pad{}", p)).unwrap_or_default()
    );
    Ok(())
}

pub fn slice_clear(track_name: &str, pad: Option<i32>) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    session.simpler_clear_slices(idx, 0, pad)?;
    eprintln!(
        "cleared slices on {}{}",
        track_name,
        pad.map(|p| format!(":pad{}", p)).unwrap_or_default()
    );
    Ok(())
}

pub fn simpler_mode(track_name: &str, pad: Option<i32>, mode: &str) -> Result<()> {
    let mode_num = match mode.to_lowercase().as_str() {
        "classic" | "0" => 0,
        "one-shot" | "oneshot" | "1" => 1,
        "slicing" | "slice" | "2" => 2,
        other => {
            return Err(crate::error::Error::Other(format!(
                "unknown mode '{}' — use classic, one-shot, or slicing",
                other
            )))
        }
    };
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    session.simpler_set_playback_mode(idx, 0, pad, mode_num)?;
    eprintln!(
        "simpler mode = {} on {}{}",
        mode,
        track_name,
        pad.map(|p| format!(":pad{}", p)).unwrap_or_default()
    );
    Ok(())
}

pub fn slice_reset(track_name: &str, pad: Option<i32>) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    session.simpler_reset_slices(idx, 0, pad)?;
    eprintln!(
        "reset slices on {}{}",
        track_name,
        pad.map(|p| format!(":pad{}", p)).unwrap_or_default()
    );
    Ok(())
}

pub fn swap(track_name: &str, device_idx: i32, preset_name: &str) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let name = session.hotswap_device(idx, device_idx, preset_name)?;
    thread::sleep(Duration::from_millis(300));
    eprintln!("swap dev[{}] on \"{}\" <- {}", device_idx, track_name, name);
    Ok(())
}

pub fn drum_rack(track_name: &str) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let name = session.load_drum_rack(idx)?;
    thread::sleep(Duration::from_millis(300));
    eprintln!("drum rack → \"{}\" ({})", track_name, name);
    Ok(())
}

pub fn pad_load(track_name: &str, pad_note: i32, device_name: &str) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let name = session.load_device_pad(idx, 0, pad_note, device_name)?;
    thread::sleep(Duration::from_millis(400));
    eprintln!("pad {} ← {} on \"{}\"", pad_note, name, track_name);
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

pub fn route(
    track_name: &str,
    input: Option<&str>,
    input_channel: Option<&str>,
    output: Option<&str>,
    output_channel: Option<&str>,
) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);

    // If no flags, show current routing
    if input.is_none() && input_channel.is_none() && output.is_none() && output_channel.is_none() {
        let in_type = track.get_input_routing_type()?;
        let in_ch = track.get_input_routing_channel()?;
        let out_type = track.get_output_routing_type()?;
        let out_ch = track.get_output_routing_channel()?;

        println!("Routing: {} [#{}]", track_name, idx);
        println!("  input:  {} / {}", in_type, in_ch);
        println!("  output: {} / {}", out_type, out_ch);

        let in_types = track.input_routing_types()?;
        let out_types = track.output_routing_types()?;
        println!("  available inputs:  {}", in_types.join(", "));
        println!("  available outputs: {}", out_types.join(", "));
        return Ok(());
    }

    if let Some(in_type) = input {
        track.set_input_routing_type(in_type)?;
        eprintln!("input → {}", in_type);
    }
    if let Some(in_ch) = input_channel {
        track.set_input_routing_channel(in_ch)?;
        eprintln!("input channel → {}", in_ch);
    }
    if let Some(out_type) = output {
        track.set_output_routing_type(out_type)?;
        eprintln!("output → {}", out_type);
    }
    if let Some(out_ch) = output_channel {
        track.set_output_routing_channel(out_ch)?;
        eprintln!("output channel → {}", out_ch);
    }

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
