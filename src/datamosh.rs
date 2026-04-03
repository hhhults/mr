//! `mr viz datamosh` — OSC bridge to FFlive via ffglitch-livecoding.
//!
//! Sends `/set "varname" <value>` messages to the ffglitch-livecoding
//! watchdog on port 5558. Static values are sent once; dynamic signals
//! (LFO, curve, ramp) spawn a 60 Hz update loop.

use std::net::UdpSocket;
use std::time::Instant;

use rosc::encoder;
use rosc::{OscMessage, OscPacket, OscType};

use crate::error::{Error, Result};

const WATCHDOG_ADDR: &str = "127.0.0.1:5558";
const UPDATE_HZ: f64 = 60.0;

// ─── Signal parsing (reuses conventions from visual/grain/voice.rs) ─────────

/// A lightweight signal type for the datamosh bridge.
/// We don't depend on metaritual directly — we parse the same spec strings.
#[derive(Debug, Clone)]
enum DmSignal {
    Const(f64),
    Sine { center: f64, depth: f64, rate: f64 },
    Triangle { center: f64, depth: f64, rate: f64 },
    Saw { center: f64, depth: f64, rate: f64 },
    Ramp { from: f64, to: f64, seconds: f64 },
}

impl DmSignal {
    fn is_const(&self) -> bool {
        matches!(self, DmSignal::Const(_))
    }

    fn sample(&self, t: f64) -> f64 {
        match self {
            DmSignal::Const(v) => *v,
            DmSignal::Sine { center, depth, rate } => {
                let phase = t * rate;
                center + depth * (phase * std::f64::consts::TAU).sin()
            }
            DmSignal::Triangle { center, depth, rate } => {
                let p = (t * rate).rem_euclid(1.0);
                let raw = if p < 0.25 {
                    p * 4.0
                } else if p < 0.75 {
                    2.0 - p * 4.0
                } else {
                    p * 4.0 - 4.0
                };
                center + depth * raw
            }
            DmSignal::Saw { center, depth, rate } => {
                let p = (t * rate).rem_euclid(1.0);
                center + depth * (2.0 * p - 1.0)
            }
            DmSignal::Ramp { from, to, seconds } => {
                if *seconds <= 0.0 {
                    return *to;
                }
                let frac = (t / seconds).clamp(0.0, 1.0);
                from + frac * (to - from)
            }
        }
    }
}

/// Parse a signal spec string.
///
/// Formats:
///   - bare number:          `0.5`
///   - `const:<val>`:        `const:0.5`
///   - `sine:<c>,<d>,<r>`:   `sine:0.5,0.3,0.25` (center, depth, rate in Hz)
///   - `tri:<c>,<d>,<r>`:    same but triangle
///   - `saw:<c>,<d>,<r>`:    same but sawtooth
///   - `ramp:<from>,<to>,<s>`: `ramp:0.0,1.0,8.0` (seconds)
///   - `lfo:<shape>,<c>,<d>,<r>`: `lfo:sine,0.5,0.3,0.25`
fn parse_signal(spec: &str) -> Result<DmSignal> {
    // Bare number
    if let Ok(v) = spec.parse::<f64>() {
        return Ok(DmSignal::Const(v));
    }

    let (kind, args_str) = spec
        .split_once(':')
        .ok_or_else(|| Error::Other(format!("bad signal spec: \"{spec}\"")))?;

    let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();

    match kind.trim() {
        "const" => {
            let v = parse_f64(args.first(), spec)?;
            Ok(DmSignal::Const(v))
        }
        "sine" | "sin" => {
            let (center, depth, rate) = parse_cdr(&args, spec)?;
            Ok(DmSignal::Sine { center, depth, rate })
        }
        "tri" | "triangle" => {
            let (center, depth, rate) = parse_cdr(&args, spec)?;
            Ok(DmSignal::Triangle { center, depth, rate })
        }
        "saw" | "sawtooth" => {
            let (center, depth, rate) = parse_cdr(&args, spec)?;
            Ok(DmSignal::Saw { center, depth, rate })
        }
        "ramp" => {
            if args.len() < 3 {
                return Err(Error::Other(format!(
                    "ramp needs 3 args (from,to,seconds): \"{spec}\""
                )));
            }
            let from = args[0].parse().map_err(|_| bad_spec(spec))?;
            let to = args[1].parse().map_err(|_| bad_spec(spec))?;
            let seconds = args[2].parse().map_err(|_| bad_spec(spec))?;
            Ok(DmSignal::Ramp { from, to, seconds })
        }
        "lfo" => {
            // lfo:<shape>,<center>,<depth>,<rate>
            if args.len() < 4 {
                return Err(Error::Other(format!(
                    "lfo needs 4 args (shape,center,depth,rate): \"{spec}\""
                )));
            }
            let center: f64 = args[1].parse().map_err(|_| bad_spec(spec))?;
            let depth: f64 = args[2].parse().map_err(|_| bad_spec(spec))?;
            let rate: f64 = args[3].parse().map_err(|_| bad_spec(spec))?;
            match args[0] {
                "sine" | "sin" => Ok(DmSignal::Sine { center, depth, rate }),
                "tri" | "triangle" => Ok(DmSignal::Triangle { center, depth, rate }),
                "saw" | "sawtooth" => Ok(DmSignal::Saw { center, depth, rate }),
                _ => Err(Error::Other(format!("unknown LFO shape: \"{}\"", args[0]))),
            }
        }
        _ => Err(Error::Other(format!("unknown signal kind: \"{kind}\""))),
    }
}

fn parse_f64(arg: Option<&&str>, spec: &str) -> Result<f64> {
    arg.ok_or_else(|| bad_spec(spec))?
        .parse()
        .map_err(|_| bad_spec(spec))
}

fn parse_cdr(args: &[&str], spec: &str) -> Result<(f64, f64, f64)> {
    if args.len() < 3 {
        return Err(Error::Other(format!(
            "need 3 args (center,depth,rate): \"{spec}\""
        )));
    }
    let center: f64 = args[0].parse().map_err(|_| bad_spec(spec))?;
    let depth: f64 = args[1].parse().map_err(|_| bad_spec(spec))?;
    let rate: f64 = args[2].parse().map_err(|_| bad_spec(spec))?;
    Ok((center, depth, rate))
}

fn bad_spec(spec: &str) -> Error {
    Error::Other(format!("bad signal spec: \"{spec}\""))
}

// ─── OSC sender ─────────────────────────────────────────────────────────────

/// Send a single `/set "varname" value` OSC message.
fn osc_set(socket: &UdpSocket, name: &str, value: f64) -> Result<()> {
    let msg = OscPacket::Message(OscMessage {
        addr: "/set".to_string(),
        args: vec![OscType::String(name.to_string()), OscType::Float(value as f32)],
    });
    let buf = encoder::encode(&msg)
        .map_err(|e| Error::Other(format!("osc encode: {e}")))?;
    socket
        .send_to(&buf, WATCHDOG_ADDR)
        .map_err(|e| Error::Other(format!("osc send to {WATCHDOG_ADDR}: {e}")))?;
    Ok(())
}

/// Send `/loop "path"` OSC message.
fn osc_loop(socket: &UdpSocket, path: &str) -> Result<()> {
    let msg = OscPacket::Message(OscMessage {
        addr: "/loop".to_string(),
        args: vec![OscType::String(path.to_string())],
    });
    let buf = encoder::encode(&msg)
        .map_err(|e| Error::Other(format!("osc encode: {e}")))?;
    socket
        .send_to(&buf, WATCHDOG_ADDR)
        .map_err(|e| Error::Other(format!("osc send: {e}")))?;
    Ok(())
}

/// Send `/watch "script_path"` OSC message.
fn osc_watch(socket: &UdpSocket, script_path: &str) -> Result<()> {
    let msg = OscPacket::Message(OscMessage {
        addr: "/watch".to_string(),
        args: vec![OscType::String(script_path.to_string())],
    });
    let buf = encoder::encode(&msg)
        .map_err(|e| Error::Other(format!("osc encode: {e}")))?;
    socket
        .send_to(&buf, WATCHDOG_ADDR)
        .map_err(|e| Error::Other(format!("osc send: {e}")))?;
    Ok(())
}

/// Send `/clean` OSC message.
fn osc_clean(socket: &UdpSocket) -> Result<()> {
    let msg = OscPacket::Message(OscMessage {
        addr: "/clean".to_string(),
        args: vec![],
    });
    let buf = encoder::encode(&msg)
        .map_err(|e| Error::Other(format!("osc encode: {e}")))?;
    socket
        .send_to(&buf, WATCHDOG_ADDR)
        .map_err(|e| Error::Other(format!("osc send: {e}")))?;
    Ok(())
}

fn bind_socket() -> Result<UdpSocket> {
    UdpSocket::bind("0.0.0.0:0").map_err(|e| Error::Other(format!("bind UDP: {e}")))
}

// ─── Named parameter with signal ───────────────────────────────────────────

struct ParamUpdate {
    name: String,
    signal: DmSignal,
    last_sent: f64,
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Set datamosh parameters. Static values send once; dynamic signals run a
/// 60 Hz update loop (blocking, Ctrl-C to stop).
pub fn datamosh(
    intensity: Option<&str>,
    mv_scale: Option<&str>,
    mv_rotate: Option<&str>,
    mv_noise: Option<&str>,
    iframe_rate: Option<&str>,
    dct_noise: Option<&str>,
    dct_quantize: Option<&str>,
    block_shift: Option<&str>,
    mode: Option<&str>,
    clean: bool,
    script: Option<&str>,
    video: Option<&str>,
) -> Result<()> {
    let socket = bind_socket()?;

    // Handle clean
    if clean {
        osc_clean(&socket)?;
        eprintln!("datamosh: sent /clean");
        return Ok(());
    }

    // Handle script loading
    if let Some(script_path) = script {
        osc_watch(&socket, script_path)?;
        eprintln!("datamosh: watching script {script_path}");
    }

    // Handle video loading
    if let Some(video_path) = video {
        osc_loop(&socket, video_path)?;
        eprintln!("datamosh: looping video {video_path}");
    }

    // Handle mode (maps name → integer)
    if let Some(mode_str) = mode {
        let mode_val = match mode_str {
            "smear" | "0" => 0.0,
            "iframe-drop" | "iframe" | "1" => 1.0,
            "pixel-sort" | "2" => 2.0,
            "block-shift" | "block" | "3" => 3.0,
            "dct-crush" | "dct" | "4" => 4.0,
            _ => {
                return Err(Error::Other(format!(
                    "unknown mode: \"{mode_str}\"\navailable: smear, iframe-drop, block-shift, dct-crush"
                )));
            }
        };
        osc_set(&socket, "mode", mode_val)?;
        eprintln!("datamosh: mode = {mode_str} ({mode_val})");
    }

    // Build parameter list
    let mut params = Vec::new();
    let specs: &[(&str, Option<&str>)] = &[
        ("intensity", intensity),
        ("mv_scale", mv_scale),
        ("mv_rotate", mv_rotate),
        ("mv_noise", mv_noise),
        ("iframe_rate", iframe_rate),
        ("dct_noise", dct_noise),
        ("dct_quantize", dct_quantize),
        ("block_shift", block_shift),
    ];

    for (name, spec_opt) in specs {
        if let Some(spec) = spec_opt {
            let signal = parse_signal(spec)?;
            params.push(ParamUpdate {
                name: name.to_string(),
                signal,
                last_sent: f64::NAN,
            });
        }
    }

    if params.is_empty() {
        // Nothing to send (might have only set mode/script/clean)
        return Ok(());
    }

    // Check if all signals are static
    let all_static = params.iter().all(|p| p.signal.is_const());

    if all_static {
        // One-shot send
        for p in &params {
            let value = p.signal.sample(0.0);
            osc_set(&socket, &p.name, value)?;
            eprintln!("datamosh: {} = {:.4}", p.name, value);
        }
        return Ok(());
    }

    // Dynamic mode: 60 Hz update loop
    eprintln!(
        "datamosh: running {} param(s) at {UPDATE_HZ} Hz (Ctrl-C to stop)",
        params.len()
    );

    let start = Instant::now();
    let interval = std::time::Duration::from_secs_f64(1.0 / UPDATE_HZ);

    // Install Ctrl-C handler via signal
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc_flag(&r);

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        let t = start.elapsed().as_secs_f64();

        for p in &mut params {
            let value = p.signal.sample(t);
            // Only send if changed beyond threshold
            if (value - p.last_sent).abs() > 0.0005 {
                let _ = osc_set(&socket, &p.name, value);
                p.last_sent = value;
            }
        }

        std::thread::sleep(interval);
    }

    eprintln!("\ndatamosh: stopped");
    Ok(())
}

/// Set up a Ctrl-C handler that sets the flag to false.
fn ctrlc_flag(flag: &std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let f = flag.clone();
    // Use libc signal handler — minimal, no extra deps
    unsafe {
        libc::signal(libc::SIGINT, sigint_handler as *const () as libc::sighandler_t);
    }
    RUNNING_FLAG.store(f.as_ref() as *const _ as usize, std::sync::atomic::Ordering::SeqCst);
}

static RUNNING_FLAG: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

extern "C" fn sigint_handler(_sig: libc::c_int) {
    let ptr = RUNNING_FLAG.load(std::sync::atomic::Ordering::SeqCst);
    if ptr != 0 {
        let flag = unsafe { &*(ptr as *const std::sync::atomic::AtomicBool) };
        flag.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_number() {
        let s = parse_signal("0.5").unwrap();
        assert!(matches!(s, DmSignal::Const(v) if (v - 0.5).abs() < 1e-9));
    }

    #[test]
    fn parse_const() {
        let s = parse_signal("const:0.75").unwrap();
        assert!(matches!(s, DmSignal::Const(v) if (v - 0.75).abs() < 1e-9));
    }

    #[test]
    fn parse_sine() {
        let s = parse_signal("sine:0.5,0.3,0.25").unwrap();
        if let DmSignal::Sine { center, depth, rate } = s {
            assert!((center - 0.5).abs() < 1e-9);
            assert!((depth - 0.3).abs() < 1e-9);
            assert!((rate - 0.25).abs() < 1e-9);
        } else {
            panic!("expected Sine");
        }
    }

    #[test]
    fn parse_ramp() {
        let s = parse_signal("ramp:0.0,1.0,8.0").unwrap();
        if let DmSignal::Ramp { from, to, seconds } = s {
            assert!((from - 0.0).abs() < 1e-9);
            assert!((to - 1.0).abs() < 1e-9);
            assert!((seconds - 8.0).abs() < 1e-9);
        } else {
            panic!("expected Ramp");
        }
    }

    #[test]
    fn parse_lfo_spec() {
        let s = parse_signal("lfo:sine,0.5,0.3,1.0").unwrap();
        assert!(matches!(s, DmSignal::Sine { .. }));
    }

    #[test]
    fn parse_bad_spec() {
        assert!(parse_signal("garbage").is_err());
        assert!(parse_signal("sine:1.0").is_err()); // not enough args
    }

    #[test]
    fn signal_const_sample() {
        let s = DmSignal::Const(0.7);
        assert!((s.sample(0.0) - 0.7).abs() < 1e-9);
        assert!((s.sample(100.0) - 0.7).abs() < 1e-9);
    }

    #[test]
    fn signal_sine_at_zero() {
        let s = DmSignal::Sine {
            center: 0.5,
            depth: 0.3,
            rate: 1.0,
        };
        // At t=0, sin(0) = 0, so output = center = 0.5
        assert!((s.sample(0.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn signal_sine_at_quarter() {
        let s = DmSignal::Sine {
            center: 0.5,
            depth: 0.3,
            rate: 1.0,
        };
        // At t=0.25, sin(π/2) = 1.0, so output = 0.5 + 0.3 = 0.8
        assert!((s.sample(0.25) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn signal_ramp_interpolates() {
        let s = DmSignal::Ramp {
            from: 0.0,
            to: 1.0,
            seconds: 10.0,
        };
        assert!((s.sample(0.0) - 0.0).abs() < 1e-9);
        assert!((s.sample(5.0) - 0.5).abs() < 1e-9);
        assert!((s.sample(10.0) - 1.0).abs() < 1e-9);
        // Clamps beyond duration
        assert!((s.sample(20.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn signal_triangle() {
        let s = DmSignal::Triangle {
            center: 0.0,
            depth: 1.0,
            rate: 1.0,
        };
        // Peak at t=0.25
        assert!((s.sample(0.25) - 1.0).abs() < 1e-6);
        // Trough at t=0.75
        assert!((s.sample(0.75) - (-1.0)).abs() < 1e-6);
    }
}
