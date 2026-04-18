use crate::connect;
use crate::daemon::{self, DaemonRequest};
use crate::error::Result;

pub fn scene(idx: i32, name: Option<&str>) -> Result<()> {
    let session = connect::connect()?;

    // Create scene if it doesn't exist yet
    let num = session.num_scenes()?;
    if idx >= num {
        for _ in num..=idx {
            session.create_scene(-1)?;
        }
    }

    if let Some(n) = name {
        session.scene(idx).set_name(n)?;
        eprintln!("scene {} → \"{}\"", idx, n);
    } else {
        eprintln!("scene {} ready", idx);
    }

    Ok(())
}

pub fn fire(idx: Option<i32>) -> Result<()> {
    // Route through daemon if running
    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "scene_fire".into(),
            scene_idx: idx,
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

    if let Some(i) = idx {
        session.fire_scene(i)?;
        eprintln!("fired scene {}", i);
    } else {
        session.play()?;
        eprintln!("▶ playing");
    }

    Ok(())
}

/// Parse a launch-quantization string to Live's int enum value.
/// `no-q` = 0, `8-bars` = 1, `4-bars` = 2, `2-bars` = 3, `bar` = 4, `1/2` = 5,
/// `1/2t` = 6, `1/4` = 7, `1/4t` = 8, `1/8` = 9, `1/8t` = 10, `1/16` = 11,
/// `1/16t` = 12, `1/32` = 13.
fn parse_quantization(s: &str) -> Result<i32> {
    match s {
        "no-q" | "off" | "immediate" => Ok(0),
        "8-bars" => Ok(1),
        "4-bars" => Ok(2),
        "2-bars" => Ok(3),
        "bar" | "1-bar" => Ok(4),
        "1/2" | "half" => Ok(5),
        "1/2t" => Ok(6),
        "1/4" | "quarter" => Ok(7),
        "1/4t" => Ok(8),
        "1/8" | "eighth" => Ok(9),
        "1/8t" => Ok(10),
        "1/16" | "sixteenth" => Ok(11),
        "1/16t" => Ok(12),
        "1/32" => Ok(13),
        _ => Err(crate::error::Error::Other(format!(
            "unknown quantization '{}' — try: no-q, bar, 1/2, 1/4, 1/8, 1/16",
            s
        ))),
    }
}

pub fn fire_clip_ext(target: &str, legato: bool, q: Option<&str>) -> Result<()> {
    let parts: Vec<&str> = target.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(crate::error::Error::BadTarget(target.to_string()));
    }
    let slot: i32 = parts[0]
        .parse()
        .map_err(|_| crate::error::Error::BadTarget(target.to_string()))?;
    let track_name = parts[1];

    let q_val = q.map(parse_quantization).transpose()?;

    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);
    track.fire_slot_ext(slot, legato, q_val)?;
    eprintln!(
        "fired {}:{}{}{}",
        track_name,
        slot,
        if legato { " [legato]" } else { "" },
        q.map(|s| format!(" [q={}]", s)).unwrap_or_default()
    );
    Ok(())
}

pub fn capture_scene() -> Result<()> {
    let session = connect::connect()?;
    session.capture_and_insert_scene()?;
    eprintln!("captured playing clips into new scene");
    Ok(())
}

pub fn move_device(from_track: &str, src_device: i32, to_track: &str, dest_pos: i32) -> Result<()> {
    let session = connect::connect()?;
    let src_idx = connect::resolve_track(&session, from_track)?;
    let dest_idx = connect::resolve_track(&session, to_track)?;
    session.move_device(src_idx, src_device, dest_idx, dest_pos)?;
    eprintln!(
        "moved device {} from \"{}\" to \"{}\" @ {}",
        src_device, from_track, to_track, dest_pos
    );
    Ok(())
}

pub fn fire_clip(target: &str) -> Result<()> {
    let parts: Vec<&str> = target.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(crate::error::Error::BadTarget(target.to_string()));
    }
    let slot: i32 = parts[0].parse().map_err(|_| {
        crate::error::Error::BadTarget(target.to_string())
    })?;
    let track_name = parts[1];

    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "clip_fire".into(),
            track: track_name.to_string(),
            slot,
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

    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    session.fire_clip(idx, slot)?;
    eprintln!("fired {}:{}", track_name, slot);
    Ok(())
}

pub fn stop_all() -> Result<()> {
    let session = connect::connect()?;
    session.stop_all_clips()?;
    eprintln!("stopped all clips");
    Ok(())
}
