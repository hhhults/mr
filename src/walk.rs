use std::f64::consts::PI;
use std::thread;
use std::time::{Duration, Instant};

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::connect;
use crate::daemon::{self, DaemonRequest};
use crate::error::{Error, Result};

/// Stop active walks running in the daemon.
pub fn walk_stop(id: Option<&str>) -> Result<()> {
    if !daemon::is_running() {
        return Err(Error::Other("daemon not running — walk-stop requires the daemon".into()));
    }

    let (cmd, name) = match id {
        Some(id) => ("walk_stop", id.to_string()),
        None => ("walk_stop_all", String::new()),
    };

    let resp = daemon::send_request(&DaemonRequest {
        cmd: cmd.into(),
        name,
        ..Default::default()
    })?;

    if resp.ok {
        eprintln!("{}", resp.message.unwrap_or_default());
    } else {
        eprintln!("error: {}", resp.error.unwrap_or_default());
    }
    Ok(())
}

/// Smoothly change a device parameter in real-time.
///
/// If the daemon is running, the walk runs as a background task in the daemon
/// (non-blocking — the CLI returns immediately). Otherwise, runs locally.
pub fn walk(
    track_name: &str,
    param: &str,
    from: Option<f64>,
    to: Option<f64>,
    seconds: f64,
    device_idx: i32,
    drunk: bool,
    sine: bool,
    range: Option<Vec<f64>>,
    step: f64,
    cycle: f64,
    seed: u64,
) -> Result<()> {
    // Route through daemon if available (non-blocking)
    if daemon::is_running() {
        let mode = if drunk {
            "drunk"
        } else if sine {
            "sine"
        } else {
            "ramp"
        };
        let walk_range = range.as_ref().and_then(|r| {
            if r.len() >= 2 {
                Some([r[0], r[1]])
            } else {
                None
            }
        });

        let req = DaemonRequest {
            cmd: "walk".into(),
            track: track_name.to_string(),
            param: param.to_string(),
            device: device_idx,
            walk_mode: mode.to_string(),
            walk_from: from,
            walk_to: to,
            walk_range,
            walk_step: step,
            walk_cycle: cycle,
            walk_seconds: seconds,
            walk_seed: seed,
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

    // Direct fallback (blocking)
    let session = connect::connect()?;
    let idx = connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);
    let device = track.device(device_idx);

    let param_names = device.parameter_names()?;
    let param_idx = param_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(param))
        .or_else(|| {
            param_names
                .iter()
                .position(|n| n.to_lowercase().contains(&param.to_lowercase()))
        })
        .ok_or_else(|| Error::ParamNotFound(param.to_string()))? as i32;

    let resolved_name = &param_names[param_idx as usize];
    let current = device.get_param(param_idx)? as f64;

    let total_ticks = (seconds / 0.033).ceil() as usize;
    let start_time = Instant::now();

    let final_value;

    if drunk {
        let (vmin, vmax) = match &range {
            Some(r) if r.len() >= 2 => (r[0], r[1]),
            _ => (0.0, 1.0),
        };
        let mut rng = SmallRng::seed_from_u64(seed);
        let mut val = current.clamp(vmin, vmax);

        eprintln!(
            "walk {} \"{}\" drunk [{:.2}..{:.2}] step={:.3} for {:.1}s",
            track_name, resolved_name, vmin, vmax, step, seconds
        );

        for i in 0..total_ticks {
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed >= seconds {
                break;
            }
            device.set_param(param_idx, val as f32)?;
            val += rng.random::<f64>() * step * 2.0 - step;
            val = val.clamp(vmin, vmax);
            let target = Duration::from_secs_f64((i + 1) as f64 * 0.033);
            let now = start_time.elapsed();
            if target > now {
                thread::sleep(target - now);
            }
        }
        final_value = val;
    } else if sine {
        let (vmin, vmax) = match &range {
            Some(r) if r.len() >= 2 => (r[0], r[1]),
            _ => (0.0, 1.0),
        };

        eprintln!(
            "walk {} \"{}\" sine [{:.2}..{:.2}] cycle={:.1}s for {:.1}s",
            track_name, resolved_name, vmin, vmax, cycle, seconds
        );

        let mut last_val = 0.0;
        for i in 0..total_ticks {
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed >= seconds {
                break;
            }
            let t = elapsed / cycle;
            let n = 0.5 + 0.5 * (2.0 * PI * t).sin();
            let val = vmin + n * (vmax - vmin);
            last_val = val;
            device.set_param(param_idx, val as f32)?;
            let target = Duration::from_secs_f64((i + 1) as f64 * 0.033);
            let now = start_time.elapsed();
            if target > now {
                thread::sleep(target - now);
            }
        }
        final_value = last_val;
    } else {
        let v_from = from.unwrap_or(current);
        let v_to = to.unwrap_or(1.0);

        eprintln!(
            "walk {} \"{}\" ramp {:.3} → {:.3} over {:.1}s",
            track_name, resolved_name, v_from, v_to, seconds
        );

        for i in 0..total_ticks {
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed >= seconds {
                break;
            }
            let frac = (elapsed / seconds).clamp(0.0, 1.0);
            let val = v_from + frac * (v_to - v_from);
            device.set_param(param_idx, val as f32)?;
            let target = Duration::from_secs_f64((i + 1) as f64 * 0.033);
            let now = start_time.elapsed();
            if target > now {
                thread::sleep(target - now);
            }
        }
        device.set_param(param_idx, v_to as f32)?;
        final_value = v_to;
    }

    eprintln!("done — {} = {:.3}", resolved_name, final_value);
    Ok(())
}
