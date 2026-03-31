use crate::connect;
use crate::error::Result;

pub fn tempo(bpm: f32) -> Result<()> {
    let session = connect::connect()?;
    session.set_tempo(bpm)?;
    eprintln!("tempo → {}", bpm);
    Ok(())
}

pub fn play() -> Result<()> {
    let session = connect::connect()?;
    session.play()?;
    eprintln!("▶ playing");
    Ok(())
}

pub fn stop() -> Result<()> {
    let session = connect::connect()?;
    session.stop()?;
    eprintln!("■ stopped");
    Ok(())
}

pub fn undo() -> Result<()> {
    let session = connect::connect()?;
    session.undo()?;
    eprintln!("↩ undo");
    Ok(())
}

pub fn redo() -> Result<()> {
    let session = connect::connect()?;
    session.redo()?;
    eprintln!("↪ redo");
    Ok(())
}
