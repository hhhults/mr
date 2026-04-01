use crate::connect::{self, parse_target};
use crate::error::Result;
use crate::json::{MrData, MrNote, write_stdout};

pub fn status() -> Result<()> {
    use ableton::Arg;

    let session = connect::connect()?;

    // Batch all status queries into one OSC cycle
    let queries = vec![
        ("/live/song/get/tempo".to_string(), vec![]),
        ("/live/song/get/is_playing".to_string(), vec![]),
        ("/live/song/get/signature_numerator".to_string(), vec![]),
        ("/live/song/get/signature_denominator".to_string(), vec![]),
        ("/live/song/get/num_tracks".to_string(), vec![]),
        ("/live/application/get/average_process_usage".to_string(), vec![]),
    ];
    let results = session.osc().batch_query(&queries)?;

    let tempo = results[0].first().and_then(|a| a.as_f32()).unwrap_or(120.0);
    let playing = results[1].first().and_then(|a| a.as_bool()).unwrap_or(false);
    let num = results[2].first().and_then(|a: &Arg| a.as_i32()).unwrap_or(4);
    let den = results[3].first().and_then(|a: &Arg| a.as_i32()).unwrap_or(4);
    let num_tracks = results[4].first().and_then(|a: &Arg| a.as_i32()).unwrap_or(0);
    let cpu = results[5].first().and_then(|a| a.as_f32()).unwrap_or(0.0);

    println!("tempo:     {:.1} BPM", tempo);
    println!("playing:   {}", if playing { "yes" } else { "no" });
    println!("time sig:  {}/{}", num, den);
    println!("tracks:    {}", num_tracks);
    println!("CPU:       {:.1}%", cpu);

    Ok(())
}

pub fn inspect_track(name: &str) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, name)?;
    let track = session.track(idx);

    let vol = track.get_volume()?;
    let pan = track.get_panning()?;

    println!("Track: {} [#{}]", name, idx);
    println!("  vol: {:.2}  pan: {:.2}", vol, pan);

    // Devices
    let device_names = track.device_names()?;
    if !device_names.is_empty() {
        println!("  devices:");
        for (i, dn) in device_names.iter().enumerate() {
            println!("    [{}] {}", i, dn);
        }
    }

    // Clips
    let clip_names = track.clip_names()?;
    if !clip_names.is_empty() {
        println!("  clips:");
        for (i, cn) in clip_names.iter().enumerate() {
            if !cn.is_empty() {
                let clip = track.clip(i as i32);
                let length = clip.get_length()?;
                println!("    [{}] \"{}\" ({:.1} beats)", i, cn, length);
            }
        }
    }

    Ok(())
}

pub fn inspect_clip(target: &str) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);
    let clip = track.clip(slot);

    let name = clip.get_name()?;
    let length = clip.get_length()?;
    let looping = clip.get_looping()?;
    let notes = clip.get_notes()?;

    println!("Clip: {}:{} \"{}\"", track_name, slot, name);
    println!("  length:  {:.1} beats", length);
    println!("  looping: {}", looping);
    println!("  notes:   {}", notes.len());

    if !notes.is_empty() {
        for n in &notes {
            println!(
                "    pitch:{:<3} start:{:.2} dur:{:.2} vel:{}",
                n.pitch, n.start, n.duration, n.velocity
            );
        }
    }

    Ok(())
}

pub fn params(track_name: &str, device_idx: Option<i32>) -> Result<()> {
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);
    let dev_idx = device_idx.unwrap_or(0);
    let device = track.device(dev_idx);

    let dev_name = device.name()?;
    let params = device.parameters()?;

    println!("Device: {} [{}]", dev_name, dev_idx);
    for p in &params {
        println!("  [{:<3}] {:<30} = {:.4}", p.index, p.name, p.value);
    }

    Ok(())
}

pub fn returns() -> Result<()> {
    let session = connect::connect()?;
    let names = session.return_track_names()?;

    if names.is_empty() {
        println!("no return tracks");
        return Ok(());
    }

    for (i, name) in names.iter().enumerate() {
        let letter = (b'A' + i as u8) as char;
        let rt = session.return_track(i as i32);
        let device_names = rt.device_names()?;
        let devices = if device_names.is_empty() {
            "(empty)".to_string()
        } else {
            device_names.join(", ")
        };
        println!("  {} ({}) — {}", letter, name, devices);
    }

    Ok(())
}

pub fn meters() -> Result<()> {
    let session = connect::connect()?;
    let names = session.track_names()?;

    if names.is_empty() {
        eprintln!("no tracks");
        return Ok(());
    }

    let max_name = names.iter().map(|n| n.len()).max().unwrap_or(8).max(8);
    let bar_width = 30;

    for (i, name) in names.iter().enumerate() {
        let track = session.track(i as i32);
        let level = track.get_output_meter()?;

        let filled = ((level * bar_width as f32).round() as usize).min(bar_width);
        let bar: String = "\u{2588}".repeat(filled);

        if level < 0.05 {
            println!(
                "  #{:<2} {:<width$}  SILENT",
                i,
                name,
                width = max_name,
            );
        } else {
            println!(
                "  #{:<2} {:<width$}  {:.3}  {}",
                i,
                name,
                level,
                bar,
                width = max_name,
            );
        }
    }

    Ok(())
}

/// Read clip notes as pattern JSON to stdout.
pub fn read(target: &str) -> Result<()> {
    let (track_name, slot) = parse_target(target)?;
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);
    let clip = track.clip(slot);

    let notes = clip.get_notes()?;

    let mr_notes: Vec<MrNote> = notes
        .iter()
        .map(|n| MrNote {
            pitch: n.pitch,
            start: n.start as f64,
            duration: n.duration as f64,
            velocity: n.velocity,
        })
        .collect();

    write_stdout(&MrData::Pattern { notes: mr_notes })?;
    Ok(())
}
