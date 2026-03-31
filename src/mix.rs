use crate::connect;
use crate::error::Result;

pub fn mix(
    track_name: &str,
    vol: Option<f32>,
    pan: Option<f32>,
    mute: bool,
    solo: bool,
) -> Result<()> {
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

pub fn send(track_name: &str, return_name: &str, level: f32) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let return_idx = connect::resolve_return(&session, return_name)?;
    let track = session.track(idx);

    track.set_send(return_idx, level)?;
    eprintln!("send {} → {} level:{:.2}", track_name, return_name, level);

    Ok(())
}
