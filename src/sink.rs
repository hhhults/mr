use std::thread;
use std::time::Duration;

use crate::connect::{self, parse_target};
use crate::daemon::{self, DaemonRequest};
use crate::error::{Error, Result};
use crate::json::read_stdin;

/// Write a pattern to an Ableton clip.
/// Routes through daemon if running (incremental updates), falls back to direct.
pub fn write(target: &str, length: Option<f64>, name: Option<&str>) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let data = read_stdin()?;
    let notes = data.into_pattern("write")?;

    let clip_length = length.unwrap_or_else(|| {
        notes
            .iter()
            .map(|n| n.start + n.duration)
            .fold(0.0f64, f64::max)
            .max(1.0)
    });

    // Try daemon first
    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "write".into(),
            track: track_name.to_string(),
            slot,
            length: clip_length,
            name: name.unwrap_or("").to_string(),
            notes: notes.clone(),
            ..Default::default()
        };
        let resp = daemon::send_request(&req)?;
        if resp.ok {
            let kind = resp.update_kind.as_deref().unwrap_or("?");
            eprintln!("{} [{}]", resp.message.unwrap_or_default(), kind);
        } else {
            eprintln!("error: {}", resp.error.unwrap_or_default());
        }
        return Ok(());
    }

    // Direct fallback
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);

    if track.has_clip(slot)? {
        track.delete_clip(slot)?;
        thread::sleep(Duration::from_millis(50));
    }

    let clip = track.create_clip(slot, clip_length as f32)?;
    thread::sleep(Duration::from_millis(100));

    if let Some(n) = name {
        clip.set_name(n)?;
    }
    clip.set_looping(true)?;

    let ableton_notes: Vec<ableton::Note> = notes
        .iter()
        .map(|n| ableton::Note::new(n.pitch, n.start as f32, n.duration as f32, n.velocity))
        .collect();

    for chunk in ableton_notes.chunks(80) {
        clip.add_notes(chunk)?;
        thread::sleep(Duration::from_millis(10));
    }

    thread::sleep(Duration::from_millis(50));
    clip.fire()?;

    eprintln!(
        "wrote {} notes to {}:{} ({:.1} beats)",
        ableton_notes.len(),
        track_name,
        slot,
        clip_length
    );

    Ok(())
}

/// Write a curve as automation on a clip parameter.
/// Routes through daemon if running, falls back to direct.
pub fn automate(target: &str, param: &str, device_idx: i32, resolution: f64) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let data = read_stdin()?;
    let points = data.into_curve("automate")?;

    // Try daemon first
    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "automate".into(),
            track: track_name.to_string(),
            slot,
            param: param.to_string(),
            device: device_idx,
            resolution,
            points: points.clone(),
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
    let track = session.track(idx);
    let clip = track.clip(slot);

    let device = track.device(device_idx);
    let param_names = device.parameter_names()?;
    let param_idx = param_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(param))
        .or_else(|| {
            param_names
                .iter()
                .position(|n| n.to_lowercase().contains(&param.to_lowercase()))
        })
        .ok_or_else(|| Error::ParamNotFound(param.to_string()))? as i32;

    let smooth_points: Vec<(f32, f32)> = points
        .iter()
        .map(|p| (p[0] as f32, p[1] as f32))
        .collect();

    clip.automate_smooth(device_idx, param_idx, &smooth_points, resolution as f32)?;

    eprintln!(
        "automated {} on {}:{} ({} points)",
        param,
        track_name,
        slot,
        points.len()
    );

    Ok(())
}
