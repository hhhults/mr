use crate::connect;
use crate::daemon::{self, DaemonRequest};
use crate::error::{Error, Result};

/// Set device parameters using semantic key:value pairs.
///
/// Looks up parameter names case-insensitively on the given device.
pub fn set_params(track_name: &str, device_idx: i32, params: &[String]) -> Result<()> {
    // Route through daemon if running
    if daemon::is_running() {
        let params_kv: Vec<[String; 2]> = params
            .iter()
            .map(|pair| {
                let parts: Vec<&str> = pair.splitn(2, ':').collect();
                if parts.len() != 2 {
                    [pair.to_string(), String::new()]
                } else {
                    [parts[0].to_string(), parts[1].to_string()]
                }
            })
            .collect();
        let req = DaemonRequest {
            cmd: "device_param".into(),
            track: track_name.to_string(),
            device: device_idx,
            params_kv,
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
    let device = track.device(device_idx);

    let param_names = device.parameter_names()?;

    let mut changes = Vec::new();

    for pair in params {
        let parts: Vec<&str> = pair.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(Error::BadKeyValue(pair.to_string()));
        }
        let key = parts[0];
        let value: f32 = parts[1]
            .parse()
            .map_err(|_| Error::BadKeyValue(pair.to_string()))?;

        // Find param index by case-insensitive name match
        let param_idx = param_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(key))
            .or_else(|| {
                // Also try partial match (e.g., "attack" matches "Attack Time")
                param_names
                    .iter()
                    .position(|n| n.to_lowercase().contains(&key.to_lowercase()))
            })
            .ok_or_else(|| Error::ParamNotFound(key.to_string()))?;

        device.set_param(param_idx as i32, value)?;
        changes.push(format!("{}:{:.3}", param_names[param_idx], value));
    }

    if !changes.is_empty() {
        eprintln!("device {} → {}", track_name, changes.join(" "));
    }

    Ok(())
}

/// Set parameters on a drum pad's device via raw OSC.
///
/// OSC addresses:
///   /live/drum_pad/chain/device/get/parameters/name  [track, device(0), pad_note, chain_device_idx]
///   /live/drum_pad/chain/device/set/parameter/value   [track, device(0), pad_note, chain_device_idx, param_idx, value]
pub fn set_pad_params(track_name: &str, pad_note: i32, device_idx: i32, params: &[String]) -> Result<()> {
    use ableton::Arg;

    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let osc = session.osc();

    // Drum Rack is always device 0 on the track
    let rack_idx = 0;
    let prefix = [Arg::Int(idx), Arg::Int(rack_idx), Arg::Int(pad_note), Arg::Int(device_idx)];

    // Get parameter names
    let resp = osc.query("/live/drum_pad/chain/device/get/parameters/name", &prefix)?;
    let param_names: Vec<String> = resp[4..].iter().filter_map(|a| a.as_str().map(String::from)).collect();

    let mut changes = Vec::new();

    for pair in params {
        let parts: Vec<&str> = pair.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(Error::BadKeyValue(pair.to_string()));
        }
        let key = parts[0];
        let value: f32 = parts[1]
            .parse()
            .map_err(|_| Error::BadKeyValue(pair.to_string()))?;

        let param_idx = find_param(&param_names, key)?;

        osc.send(
            "/live/drum_pad/chain/device/set/parameter/value",
            &[Arg::Int(idx), Arg::Int(rack_idx), Arg::Int(pad_note), Arg::Int(device_idx),
              Arg::Int(param_idx as i32), Arg::Float(value)],
        )?;
        changes.push(format!("{}:{:.3}", param_names[param_idx], value));
    }

    if !changes.is_empty() {
        let note_name = connect::midi_note_name(pad_note);
        eprintln!("pad {} ({}) {} → {}", note_name, pad_note, track_name, changes.join(" "));
    }

    Ok(())
}

/// Set parameters on a chain's device within a rack via raw OSC.
///
/// OSC addresses:
///   /live/chain/device/get/parameters/name  [track, rack_device, chain, chain_device]
///   /live/chain/device/set/parameter/value   [track, rack_device, chain, chain_device, param_idx, value]
pub fn set_chain_params(
    track_name: &str,
    rack_device_idx: i32,
    chain_idx: i32,
    device_idx: i32,
    params: &[String],
) -> Result<()> {
    use ableton::Arg;

    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let osc = session.osc();

    let prefix = [Arg::Int(idx), Arg::Int(rack_device_idx), Arg::Int(chain_idx), Arg::Int(device_idx)];

    // Get parameter names
    let resp = osc.query("/live/chain/device/get/parameters/name", &prefix)?;
    let param_names: Vec<String> = resp[4..].iter().filter_map(|a| a.as_str().map(String::from)).collect();

    let mut changes = Vec::new();

    for pair in params {
        let parts: Vec<&str> = pair.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(Error::BadKeyValue(pair.to_string()));
        }
        let key = parts[0];
        let value: f32 = parts[1]
            .parse()
            .map_err(|_| Error::BadKeyValue(pair.to_string()))?;

        let param_idx = find_param(&param_names, key)?;

        osc.send(
            "/live/chain/device/set/parameter/value",
            &[Arg::Int(idx), Arg::Int(rack_device_idx), Arg::Int(chain_idx), Arg::Int(device_idx),
              Arg::Int(param_idx as i32), Arg::Float(value)],
        )?;
        changes.push(format!("{}:{:.3}", param_names[param_idx], value));
    }

    if !changes.is_empty() {
        eprintln!("chain [{}] {} → {}", chain_idx, track_name, changes.join(" "));
    }

    Ok(())
}

/// Set parameters on a return track's device via raw OSC.
///
/// OSC addresses:
///   /live/return_track/device/get/parameters/name  [return_idx, device_idx]
///   /live/return_track/device/set/parameter/value   [return_idx, device_idx, param_idx, value]
pub fn set_return_params(return_name: &str, device_idx: i32, params: &[String]) -> Result<()> {
    use ableton::Arg;

    let session = connect::connect()?;
    let idx = connect::resolve_return(&session, return_name)?;
    let osc = session.osc();

    let prefix = [Arg::Int(idx), Arg::Int(device_idx)];

    // Get parameter names
    let resp = osc.query("/live/return_track/device/get/parameters/name", &prefix)?;
    let param_names: Vec<String> = resp[2..].iter().filter_map(|a| a.as_str().map(String::from)).collect();

    let mut changes = Vec::new();

    for pair in params {
        let parts: Vec<&str> = pair.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(Error::BadKeyValue(pair.to_string()));
        }
        let key = parts[0];
        let value: f32 = parts[1]
            .parse()
            .map_err(|_| Error::BadKeyValue(pair.to_string()))?;

        let param_idx = find_param(&param_names, key)?;

        osc.send(
            "/live/return_track/device/set/parameter/value",
            &[Arg::Int(idx), Arg::Int(device_idx), Arg::Int(param_idx as i32), Arg::Float(value)],
        )?;
        changes.push(format!("{}:{:.3}", param_names[param_idx], value));
    }

    if !changes.is_empty() {
        let letter = (b'A' + idx as u8) as char;
        eprintln!("return {} device {} → {}", letter, device_idx, changes.join(" "));
    }

    Ok(())
}

/// Find a parameter index by case-insensitive name match (exact or partial).
fn find_param(param_names: &[String], key: &str) -> Result<usize> {
    param_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(key))
        .or_else(|| {
            param_names
                .iter()
                .position(|n| n.to_lowercase().contains(&key.to_lowercase()))
        })
        .ok_or_else(|| Error::ParamNotFound(key.to_string()))
}
