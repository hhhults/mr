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

/// List occupied drum pads on a Drum Rack via raw OSC.
pub fn pads(track_name: &str, device_idx: Option<i32>) -> Result<()> {
    use ableton::Arg;

    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let osc = session.osc();
    let dev_idx = device_idx.unwrap_or(0);
    let prefix = [Arg::Int(idx), Arg::Int(dev_idx)];

    // Get occupied pad notes and names
    let notes_resp = osc.query("/live/device/get/drum_pads/note", &prefix)?;
    let names_resp = osc.query("/live/device/get/drum_pads/name", &prefix)?;

    let pad_notes: Vec<i32> = notes_resp[2..].iter().filter_map(|a| a.as_i32()).collect();
    let pad_names: Vec<String> = names_resp[2..].iter().filter_map(|a| a.as_str().map(String::from)).collect();

    if pad_notes.is_empty() {
        println!("no drum pads on {} device {}", track_name, dev_idx);
        return Ok(());
    }

    println!("Drum Rack pads on {}:", track_name);
    for (note, name) in pad_notes.iter().zip(pad_names.iter()) {
        let note_name = connect::midi_note_name(*note);

        // Get devices on this pad's chain
        let pad_prefix = [Arg::Int(idx), Arg::Int(dev_idx), Arg::Int(*note)];
        let dev_resp = osc.query("/live/drum_pad/chain/get/devices/name", &pad_prefix).unwrap_or_default();
        let dev_names: Vec<String> = dev_resp[3..].iter().filter_map(|a| a.as_str().map(String::from)).collect();
        let devices = if dev_names.is_empty() {
            "(empty)".to_string()
        } else {
            dev_names.join(", ")
        };

        println!(
            "  {:<4} ({:<3}) {:<12} — {}",
            note_name, note, name, devices
        );
    }

    Ok(())
}

/// List chains on a rack device via raw OSC.
pub fn chains(track_name: &str, device_idx: Option<i32>) -> Result<()> {
    use ableton::Arg;

    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let osc = session.osc();
    let dev_idx = device_idx.unwrap_or(0);
    let prefix = [Arg::Int(idx), Arg::Int(dev_idx)];

    // Get number of chains
    let num_resp = osc.query("/live/device/get/num_chains", &prefix)?;
    let num = num_resp.get(2).and_then(|a| a.as_i32()).unwrap_or(0);

    if num == 0 {
        println!("no chains on {} device {}", track_name, dev_idx);
        return Ok(());
    }

    // Get chain names
    let names_resp = osc.query("/live/device/get/chains/name", &prefix)?;
    let chain_names: Vec<String> = names_resp[2..].iter().filter_map(|a| a.as_str().map(String::from)).collect();

    println!("Chains on {} device {}:", track_name, dev_idx);
    for i in 0..num {
        let name = chain_names.get(i as usize).cloned().unwrap_or_default();

        // Get chain volume
        let chain_prefix = [Arg::Int(idx), Arg::Int(dev_idx), Arg::Int(i)];
        let vol = osc.query("/live/chain/get/volume", &chain_prefix)
            .ok()
            .and_then(|r| r.get(3).and_then(|a| a.as_f32()))
            .unwrap_or(0.0);

        // Get chain device names
        let dev_resp = osc.query("/live/chain/get/devices/name", &chain_prefix).unwrap_or_default();
        let dev_names: Vec<String> = dev_resp[3..].iter().filter_map(|a| a.as_str().map(String::from)).collect();
        let devices = if dev_names.is_empty() {
            "(empty)".to_string()
        } else {
            dev_names.join(", ")
        };

        println!(
            "  [{}] \"{}\"  vol:{:.2}  — {}",
            i, name, vol, devices
        );
    }

    Ok(())
}

/// Show parameters for a return track device via raw OSC.
pub fn return_params(return_name: &str, device_idx: Option<i32>) -> Result<()> {
    use ableton::Arg;

    let session = connect::connect()?;
    let idx = connect::resolve_return(&session, return_name)?;
    let osc = session.osc();
    let dev_idx = device_idx.unwrap_or(0);
    let prefix = [Arg::Int(idx), Arg::Int(dev_idx)];

    // Get device name from the return track's device list
    let rt_prefix = [Arg::Int(idx)];
    let dev_names_resp = osc.query("/live/return_track/get/devices/name", &rt_prefix)?;
    let dev_names: Vec<String> = dev_names_resp.iter().filter_map(|a| a.as_str().map(String::from)).collect();
    let dev_name = dev_names.get(dev_idx as usize).cloned().unwrap_or_else(|| "Unknown".to_string());

    // Get parameter names and values
    let names_resp = osc.query("/live/return_track/device/get/parameters/name", &prefix)?;
    let values_resp = osc.query("/live/return_track/device/get/parameters/value", &prefix)?;

    let param_names: Vec<String> = names_resp[2..].iter().filter_map(|a| a.as_str().map(String::from)).collect();
    let param_values: Vec<f32> = values_resp[2..].iter().filter_map(|a| a.as_f32()).collect();

    println!("Device: {} [{}]", dev_name, dev_idx);
    for (i, (name, value)) in param_names.iter().zip(param_values.iter()).enumerate() {
        println!("  [{:<3}] {:<30} = {:.4}", i, name, value);
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
