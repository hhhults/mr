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

pub fn stop_all() -> Result<()> {
    let session = connect::connect()?;
    session.stop_all_clips()?;
    eprintln!("stopped all clips");
    Ok(())
}
