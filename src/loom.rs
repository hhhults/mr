use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use metaritual::weave::{
    Crossing, CrossingKind, Envelope, Movement, Strand, Trajectory, Weave,
};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ── State file ──────────────────────────────────────────────────────────────

fn loom_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr").join("loom")
}

fn active_path() -> PathBuf {
    loom_dir().join("active.json")
}

/// Persistent loom state — the weave plan plus timing
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoomState {
    weave: Weave,
    /// Unix timestamp when the weave was started (0 = not started)
    started_at: u64,
    /// Total duration in seconds
    duration_seconds: f64,
}

impl LoomState {
    fn position(&self) -> f64 {
        if self.started_at == 0 || self.duration_seconds <= 0.0 {
            return 0.0;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let elapsed = now.saturating_sub(self.started_at) as f64;
        (elapsed / self.duration_seconds).min(1.0)
    }

    fn elapsed_minutes(&self) -> f64 {
        if self.started_at == 0 {
            return 0.0;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let elapsed = now.saturating_sub(self.started_at) as f64;
        elapsed / 60.0
    }
}

fn load_state() -> Result<LoomState> {
    let path = active_path();
    if !path.exists() {
        return Err(Error::Other(
            "no active loom — create one with: mr loom new --duration <bars>".into(),
        ));
    }
    let json = fs::read_to_string(&path)?;
    let state: LoomState = serde_json::from_str(&json)?;
    Ok(state)
}

fn save_state(state: &LoomState) -> Result<()> {
    let dir = loom_dir();
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(state)?;
    fs::write(active_path(), json)?;
    Ok(())
}

// ── Commands ────────────────────────────────────────────────────────────────

/// Create a new weave plan
pub fn new(duration_bars: u32, duration_minutes: Option<f64>) -> Result<()> {
    let duration_seconds = duration_minutes.unwrap_or(20.0) * 60.0;

    let state = LoomState {
        weave: Weave {
            duration_bars,
            arc: Vec::new(),
            strands: Vec::new(),
            crossings: Vec::new(),
        },
        started_at: 0,
        duration_seconds,
    };

    save_state(&state)?;
    eprintln!(
        "loom created: {} bars, {:.0}m duration",
        duration_bars,
        duration_seconds / 60.0
    );
    Ok(())
}

/// Add or update a strand
pub fn strand(
    name: &str,
    enter: f64,
    attack: Option<f64>,
    sustain: Option<f64>,
    release: Option<f64>,
) -> Result<()> {
    let mut state = load_state()?;

    let envelope = Envelope {
        enter,
        attack: attack.unwrap_or(0.1),
        sustain: sustain.unwrap_or(0.7),
        release: release.unwrap_or(0.1),
    };

    // Update existing or add new
    if let Some(existing) = state.weave.strands.iter_mut().find(|s| s.name == name) {
        existing.envelope = envelope;
        eprintln!("updated strand \"{}\"", name);
    } else {
        state.weave.strands.push(Strand {
            name: name.to_string(),
            envelope,
            trajectories: std::collections::HashMap::new(),
        });
        eprintln!(
            "added strand \"{}\" (enter: {:.0}%, exit: {:.0}%)",
            name,
            enter * 100.0,
            (enter + attack.unwrap_or(0.1) + sustain.unwrap_or(0.7) + release.unwrap_or(0.1))
                * 100.0
        );
    }

    save_state(&state)
}

/// Add a trajectory to a strand
pub fn trajectory(strand_name: &str, param: &str, spec: &str) -> Result<()> {
    let mut state = load_state()?;

    let s = state
        .weave
        .strands
        .iter_mut()
        .find(|s| s.name == strand_name)
        .ok_or_else(|| Error::Other(format!("strand \"{}\" not found", strand_name)))?;

    let traj = parse_trajectory(spec)?;
    s.trajectories.insert(param.to_string(), traj);
    eprintln!("set trajectory {} on \"{}\"", param, strand_name);

    save_state(&state)
}

fn parse_trajectory(spec: &str) -> Result<Trajectory> {
    // Formats:
    //   "constant:0.5"
    //   "ramp:0.2,0.8"
    //   "drunk:0.2,0.8,0.04"   (range_lo, range_hi, step)
    //   "sine:0.2,0.8,2"       (range_lo, range_hi, cycles)
    //   "follow:pad,filter_freq,0.1"  (strand, param, lag)
    let parts: Vec<&str> = spec.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(Error::Other(format!(
            "invalid trajectory: \"{}\"\nformats: constant:0.5, ramp:0.2,0.8, drunk:0.2,0.8,0.04, sine:0.2,0.8,2, follow:strand,param,lag",
            spec
        )));
    }

    let kind = parts[0];
    let args: Vec<&str> = parts[1].split(',').collect();

    match kind {
        "constant" => {
            let value = args[0].parse::<f64>().map_err(|_| Error::Other("bad value".into()))?;
            Ok(Trajectory::Constant { value })
        }
        "ramp" => {
            if args.len() < 2 {
                return Err(Error::Other("ramp needs: from,to".into()));
            }
            let from = args[0].parse::<f64>().map_err(|_| Error::Other("bad from".into()))?;
            let to = args[1].parse::<f64>().map_err(|_| Error::Other("bad to".into()))?;
            Ok(Trajectory::Ramp { from, to })
        }
        "drunk" => {
            if args.len() < 3 {
                return Err(Error::Other("drunk needs: lo,hi,step".into()));
            }
            let lo = args[0].parse::<f64>().map_err(|_| Error::Other("bad lo".into()))?;
            let hi = args[1].parse::<f64>().map_err(|_| Error::Other("bad hi".into()))?;
            let step = args[2].parse::<f64>().map_err(|_| Error::Other("bad step".into()))?;
            Ok(Trajectory::Drunk {
                range: (lo, hi),
                step,
                seed: 42,
            })
        }
        "sine" => {
            if args.len() < 3 {
                return Err(Error::Other("sine needs: lo,hi,cycles".into()));
            }
            let lo = args[0].parse::<f64>().map_err(|_| Error::Other("bad lo".into()))?;
            let hi = args[1].parse::<f64>().map_err(|_| Error::Other("bad hi".into()))?;
            let cycles = args[2].parse::<f64>().map_err(|_| Error::Other("bad cycles".into()))?;
            Ok(Trajectory::Sine {
                range: (lo, hi),
                cycles,
            })
        }
        "follow" => {
            if args.len() < 3 {
                return Err(Error::Other("follow needs: strand,param,lag".into()));
            }
            let lag = args[2].parse::<f64>().map_err(|_| Error::Other("bad lag".into()))?;
            Ok(Trajectory::Follow {
                strand: args[0].to_string(),
                param: args[1].to_string(),
                lag,
            })
        }
        _ => Err(Error::Other(format!(
            "unknown trajectory kind: \"{}\"\navailable: constant, ramp, drunk, sine, follow",
            kind
        ))),
    }
}

/// Add a crossing between strands
pub fn crossing(
    kind: &str,
    strand_names: &[String],
    at: f64,
    duration: Option<f64>,
) -> Result<()> {
    let mut state = load_state()?;

    let crossing_kind = match kind {
        "handoff" => CrossingKind::Handoff,
        "crossfade" => CrossingKind::Crossfade,
        "entrain" => CrossingKind::Entrain,
        "diverge" => CrossingKind::Diverge,
        "converge" => CrossingKind::Converge,
        "echo" => CrossingKind::Echo,
        _ => {
            return Err(Error::Other(format!(
                "unknown crossing kind: \"{}\"\navailable: handoff, crossfade, entrain, diverge, converge, echo",
                kind
            )));
        }
    };

    let dur = duration.unwrap_or(0.05);
    state.weave.crossings.push(Crossing {
        kind: crossing_kind,
        strands: strand_names.to_vec(),
        window: (at, (at + dur).min(1.0)),
    });

    eprintln!(
        "added {} crossing: {} at {:.0}%-{:.0}%",
        kind,
        strand_names.join(" × "),
        at * 100.0,
        (at + dur) * 100.0
    );

    save_state(&state)
}

/// Add an arc movement
pub fn arc(name: &str, from: f64, to: f64, energy: Option<&str>) -> Result<()> {
    let mut state = load_state()?;

    let energy_traj = if let Some(spec) = energy {
        parse_trajectory(spec)?
    } else {
        // Default: ramp from position-proportional low to high
        Trajectory::Ramp {
            from: from,
            to: to,
        }
    };

    state.weave.arc.push(Movement {
        name: name.to_string(),
        span: (from, to),
        energy: energy_traj,
    });

    eprintln!("added arc \"{}\" ({:.0}%-{:.0}%)", name, from * 100.0, to * 100.0);

    save_state(&state)
}

/// Start the weave (begin tracking time)
pub fn start() -> Result<()> {
    let mut state = load_state()?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    state.started_at = now;

    save_state(&state)?;

    eprintln!(
        "loom started — {:.0}m duration, {} strands, {} crossings",
        state.duration_seconds / 60.0,
        state.weave.strands.len(),
        state.weave.crossings.len()
    );

    Ok(())
}

/// Show current weave status as JSON
pub fn status() -> Result<()> {
    let state = load_state()?;
    let pos = state.position();

    let active = state.weave.active_strands(pos);
    let movement = state.weave.current_movement(pos);
    let crossings = state.weave.active_crossings(pos);
    let upcoming = state.weave.upcoming(pos, 0.15);

    // Build status object
    let active_info: Vec<serde_json::Value> = active
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "phase": s.envelope.phase(pos),
                "level": (s.envelope.level(pos) * 100.0).round() / 100.0,
                "trajectories": s.trajectories.iter().map(|(k, v)| {
                    // Evaluate trajectory at strand-local time
                    let strand_t = if s.envelope.exit() > s.envelope.enter {
                        ((pos - s.envelope.enter) / (s.envelope.exit() - s.envelope.enter)).clamp(0.0, 1.0)
                    } else { 0.0 };
                    (k.clone(), serde_json::json!({ "value": (v.eval(strand_t) * 1000.0).round() / 1000.0 }))
                }).collect::<serde_json::Map<String, serde_json::Value>>(),
            })
        })
        .collect();

    let crossing_info: Vec<serde_json::Value> = crossings
        .iter()
        .map(|c| {
            serde_json::json!({
                "kind": format!("{:?}", c.kind).to_lowercase(),
                "strands": c.strands,
                "progress": if c.window.1 > c.window.0 {
                    ((pos - c.window.0) / (c.window.1 - c.window.0)).clamp(0.0, 1.0)
                } else { 1.0 },
            })
        })
        .collect();

    let upcoming_info: Vec<serde_json::Value> = upcoming
        .iter()
        .map(|e| {
            serde_json::json!({
                "t": (e.t * 1000.0).round() / 1000.0,
                "action": e.action,
            })
        })
        .collect();

    let status = serde_json::json!({
        "position": (pos * 1000.0).round() / 1000.0,
        "elapsed_minutes": (state.elapsed_minutes() * 10.0).round() / 10.0,
        "remaining_minutes": ((state.duration_seconds / 60.0 - state.elapsed_minutes()) * 10.0).round() / 10.0,
        "started": state.started_at > 0,
        "movement": movement.map(|m| serde_json::json!({
            "name": m.name,
            "energy": (m.energy.eval(
                if m.span.1 > m.span.0 { (pos - m.span.0) / (m.span.1 - m.span.0) } else { 0.0 }
            ) * 100.0).round() / 100.0,
        })),
        "active_strands": active_info,
        "active_crossings": crossing_info,
        "upcoming": upcoming_info,
        "total_strands": state.weave.strands.len(),
        "total_crossings": state.weave.crossings.len(),
    });

    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

/// Show the full weave plan
pub fn show() -> Result<()> {
    let state = load_state()?;
    let events = state.weave.compile();

    eprintln!(
        "loom: {} bars, {:.0}m, {} strands, {} crossings, {} arc segments",
        state.weave.duration_bars,
        state.duration_seconds / 60.0,
        state.weave.strands.len(),
        state.weave.crossings.len(),
        state.weave.arc.len(),
    );

    // Print arc
    if !state.weave.arc.is_empty() {
        eprintln!("\narc:");
        for m in &state.weave.arc {
            eprintln!(
                "  {} ({:.0}%-{:.0}%)",
                m.name,
                m.span.0 * 100.0,
                m.span.1 * 100.0
            );
        }
    }

    // Print strands
    eprintln!("\nstrands:");
    for s in &state.weave.strands {
        eprintln!(
            "  {} (enter {:.0}%, full {:.0}%, release {:.0}%, exit {:.0}%)",
            s.name,
            s.envelope.enter * 100.0,
            (s.envelope.enter + s.envelope.attack) * 100.0,
            (s.envelope.enter + s.envelope.attack + s.envelope.sustain) * 100.0,
            s.envelope.exit() * 100.0,
        );
        for (param, traj) in &s.trajectories {
            eprintln!("    {} → {:?}", param, traj);
        }
    }

    // Print crossings
    if !state.weave.crossings.is_empty() {
        eprintln!("\ncrossings:");
        for c in &state.weave.crossings {
            eprintln!(
                "  {:?} {} ({:.0}%-{:.0}%)",
                c.kind,
                c.strands.join(" × "),
                c.window.0 * 100.0,
                c.window.1 * 100.0,
            );
        }
    }

    // Print event timeline
    eprintln!("\ntimeline ({} events):", events.len());
    for e in &events {
        let minutes = e.t * state.duration_seconds / 60.0;
        eprintln!("  {:.0}% ({:.1}m) — {:?}", e.t * 100.0, minutes, e.action);
    }

    Ok(())
}

/// End the active weave
pub fn end() -> Result<()> {
    let path = active_path();
    if path.exists() {
        // Archive it
        let state = load_state()?;
        let archive_name = format!(
            "weave_{}.json",
            state.started_at
        );
        let archive_path = loom_dir().join(archive_name);
        fs::rename(&path, &archive_path)?;
        eprintln!("loom ended, archived to {}", archive_path.display());
    } else {
        eprintln!("no active loom");
    }
    Ok(())
}

/// Output the raw weave JSON (for piping or inspection)
pub fn export() -> Result<()> {
    let state = load_state()?;
    println!("{}", serde_json::to_string_pretty(&state.weave)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loom_state_roundtrip() {
        let state = LoomState {
            weave: Weave {
                duration_bars: 64,
                arc: vec![],
                strands: vec![],
                crossings: vec![],
            },
            started_at: 0,
            duration_seconds: 1200.0,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let parsed: LoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.weave.duration_bars, 64);
        assert_eq!(parsed.duration_seconds, 1200.0);
    }

    #[test]
    fn test_loom_state_position_not_started() {
        let state = LoomState {
            weave: Weave { duration_bars: 64, arc: vec![], strands: vec![], crossings: vec![] },
            started_at: 0,
            duration_seconds: 600.0,
        };
        assert_eq!(state.position(), 0.0);
    }

    #[test]
    fn test_loom_state_position_zero_duration() {
        let state = LoomState {
            weave: Weave { duration_bars: 64, arc: vec![], strands: vec![], crossings: vec![] },
            started_at: 1000,
            duration_seconds: 0.0,
        };
        assert_eq!(state.position(), 0.0);
    }

    #[test]
    fn test_parse_trajectory_constant() {
        let t = parse_trajectory("constant:0.5").unwrap();
        match t {
            Trajectory::Constant { value } => assert!((value - 0.5).abs() < 1e-10),
            _ => panic!("expected Constant"),
        }
    }

    #[test]
    fn test_parse_trajectory_ramp() {
        let t = parse_trajectory("ramp:0.2,0.8").unwrap();
        match t {
            Trajectory::Ramp { from, to } => {
                assert!((from - 0.2).abs() < 1e-10);
                assert!((to - 0.8).abs() < 1e-10);
            }
            _ => panic!("expected Ramp"),
        }
    }

    #[test]
    fn test_parse_trajectory_drunk() {
        let t = parse_trajectory("drunk:0.3,0.7,0.05").unwrap();
        match t {
            Trajectory::Drunk { range, step, .. } => {
                assert!((range.0 - 0.3).abs() < 1e-10);
                assert!((range.1 - 0.7).abs() < 1e-10);
                assert!((step - 0.05).abs() < 1e-10);
            }
            _ => panic!("expected Drunk"),
        }
    }

    #[test]
    fn test_parse_trajectory_sine() {
        let t = parse_trajectory("sine:0.1,0.9,3").unwrap();
        match t {
            Trajectory::Sine { range, cycles } => {
                assert!((range.0 - 0.1).abs() < 1e-10);
                assert!((cycles - 3.0).abs() < 1e-10);
            }
            _ => panic!("expected Sine"),
        }
    }

    #[test]
    fn test_parse_trajectory_follow() {
        let t = parse_trajectory("follow:pad,filter_freq,0.1").unwrap();
        match t {
            Trajectory::Follow { strand, param, lag } => {
                assert_eq!(strand, "pad");
                assert_eq!(param, "filter_freq");
                assert!((lag - 0.1).abs() < 1e-10);
            }
            _ => panic!("expected Follow"),
        }
    }

    #[test]
    fn test_parse_trajectory_bad_kind() {
        assert!(parse_trajectory("nonexistent:1.0").is_err());
    }

    #[test]
    fn test_parse_trajectory_bad_format() {
        assert!(parse_trajectory("just_a_word").is_err());
    }

    #[test]
    fn test_parse_trajectory_missing_args() {
        assert!(parse_trajectory("ramp:0.2").is_err());
        assert!(parse_trajectory("drunk:0.2,0.5").is_err());
        assert!(parse_trajectory("sine:0.2,0.8").is_err());
        assert!(parse_trajectory("follow:pad,freq").is_err());
    }

    #[test]
    fn test_loom_state_with_strands() {
        let state = LoomState {
            weave: Weave {
                duration_bars: 32,
                arc: vec![Movement {
                    name: "rise".into(),
                    span: (0.0, 0.5),
                    energy: Trajectory::Ramp { from: 0.2, to: 0.8 },
                }],
                strands: vec![Strand {
                    name: "pad".into(),
                    envelope: Envelope { enter: 0.0, attack: 0.1, sustain: 0.6, release: 0.2 },
                    trajectories: std::collections::HashMap::new(),
                }],
                crossings: vec![],
            },
            started_at: 0,
            duration_seconds: 600.0,
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: LoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.weave.strands.len(), 1);
        assert_eq!(parsed.weave.strands[0].name, "pad");
        assert_eq!(parsed.weave.arc.len(), 1);
    }
}
