use crate::connect;
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
