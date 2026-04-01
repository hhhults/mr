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
