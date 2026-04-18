use crate::connect;
use crate::daemon::{self, DaemonRequest};
use crate::error::Result;

pub fn mix(
    track_name: &str,
    vol: Option<f32>,
    pan: Option<f32>,
    mute: bool,
    solo: bool,
) -> Result<()> {
    // Route through daemon if running
    if daemon::is_running() {
        // For mute/solo toggle, query current state first
        let (mute_val, solo_val) = if mute || solo {
            let session = connect::connect()?;
            let idx = connect::resolve_track(&session, track_name)?;
            let track = session.track(idx);
            let cur_mute = track.get_mute().unwrap_or(false);
            let cur_solo = track.get_solo().unwrap_or(false);
            (
                if mute { Some(!cur_mute) } else { None },
                if solo { Some(!cur_solo) } else { None },
            )
        } else {
            (None, None)
        };

        let req = DaemonRequest {
            cmd: "mix".into(),
            track: track_name.to_string(),
            volume: vol.map(|v| v as f64),
            pan: pan.map(|p| p as f64),
            mute: mute_val,
            solo: solo_val,
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

    let mut changes = Vec::new();

    if let Some(v) = vol {
        track.set_volume(v)?;
        changes.push(format!("vol:{:.2}", v));
    }
    if let Some(p) = pan {
        track.set_panning(p)?;
        changes.push(format!("pan:{:.2}", p));
    }
    if mute {
        let current = track.get_mute()?;
        track.set_mute(!current)?;
        changes.push(format!("mute:{}", !current));
    }
    if solo {
        let current = track.get_solo()?;
        track.set_solo(!current)?;
        changes.push(format!("solo:{}", !current));
    }

    if !changes.is_empty() {
        eprintln!("mix {} → {}", track_name, changes.join(" "));
    }

    Ok(())
}

/// Explicitly set mute state (not toggle)
pub fn set_mute(track_name: &str, muted: bool) -> Result<()> {
    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "mix".into(),
            track: track_name.to_string(),
            mute: Some(muted),
            ..Default::default()
        };
        let resp = daemon::send_request(&req)?;
        if !resp.ok {
            eprintln!("error: {}", resp.error.unwrap_or_default());
            return Ok(());
        }
    } else {
        let session = connect::connect()?;
        let idx = connect::resolve_track(&session, track_name)?;
        let track = session.track(idx);
        track.set_mute(muted)?;
    }

    if muted {
        eprintln!("muted {track_name}");
    } else {
        eprintln!("unmuted {track_name}");
    }

    Ok(())
}

pub fn send(track_name: &str, return_name: &str, level: f32) -> Result<()> {
    // Route through daemon if running
    if daemon::is_running() {
        let req = DaemonRequest {
            cmd: "send_level".into(),
            track: track_name.to_string(),
            return_name: return_name.to_string(),
            level: level as f64,
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
    let return_idx = connect::resolve_return(&session, return_name)?;
    let track = session.track(idx);

    track.set_send(return_idx, level)?;
    eprintln!("send {} → {} level:{:.2}", track_name, return_name, level);

    Ok(())
}
