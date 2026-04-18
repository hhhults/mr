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

    let ableton_notes: Vec<ableton::Note> = notes.iter().map(|n| n.to_ableton()).collect();

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

/// Set clip loop points and looping state.
pub fn clip_loop(
    target: &str,
    start: Option<f64>,
    end: Option<f64>,
    on: bool,
    off: bool,
) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);
    let clip = track.clip(slot);

    let mut changes = Vec::new();

    if let Some(s) = start {
        clip.set_loop_start(s as f32)?;
        changes.push(format!("start:{:.1}", s));
    }
    if let Some(e) = end {
        clip.set_loop_end(e as f32)?;
        changes.push(format!("end:{:.1}", e));
    }
    if on {
        clip.set_looping(true)?;
        changes.push("looping:on".to_string());
    }
    if off {
        clip.set_looping(false)?;
        changes.push("looping:off".to_string());
    }

    if !changes.is_empty() {
        eprintln!("clip-loop {}:{} → {}", track_name, slot, changes.join(" "));
    }

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

fn resolve_param_idx(session: &ableton::Session, track_idx: i32, device_idx: i32, param_name: &str) -> Result<i32> {
    let device = session.track(track_idx).device(device_idx);
    let names = device.parameter_names()?;
    names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(param_name))
        .or_else(|| {
            names
                .iter()
                .position(|n| n.to_lowercase().contains(&param_name.to_lowercase()))
        })
        .map(|i| i as i32)
        .ok_or_else(|| Error::ParamNotFound(param_name.to_string()))
}

/// Insert a single automation step on a parameter.
pub fn envelope_step(
    target: &str,
    device_idx: i32,
    param_name: &str,
    time: f64,
    duration: f64,
    value: f64,
) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let param_idx = resolve_param_idx(&session, idx, device_idx, param_name)?;
    let clip = session.track(idx).clip(slot);
    clip.insert_automation_step(device_idx, param_idx, time as f32, duration as f32, value as f32)?;
    eprintln!(
        "{}:{} {} @ {:.3}..{:.3} = {:.3}",
        track_name,
        slot,
        param_name,
        time,
        time + duration,
        value
    );
    Ok(())
}

/// Read the current automation value at a time.
pub fn envelope_value(
    target: &str,
    device_idx: i32,
    param_name: &str,
    time: f64,
) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let param_idx = resolve_param_idx(&session, idx, device_idx, param_name)?;
    let clip = session.track(idx).clip(slot);
    let v = clip.automation_value_at(device_idx, param_idx, time as f32)?;
    println!("{}", v);
    Ok(())
}

/// Sample the envelope at regular intervals and write as a curve to stdout.
pub fn envelope_read(
    target: &str,
    device_idx: i32,
    param_name: &str,
    length: f64,
    resolution: f64,
) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let param_idx = resolve_param_idx(&session, idx, device_idx, param_name)?;
    let clip = session.track(idx).clip(slot);

    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut t = 0.0;
    while t <= length + 1e-6 {
        let v = clip.automation_value_at(device_idx, param_idx, t as f32)? as f64;
        points.push([t, v]);
        t += resolution;
    }
    crate::json::write_stdout(&crate::json::MrData::Curve { points })
}
