use crate::error::Result;
use crate::json::{MrData, MrNote, read_stdin, write_stdout};

/// Extract a feature from a pattern as a curve.
///
/// Features: "pitch", "velocity", "density"
pub fn to_curve(feature: &str, bars: f64, resolution: f64) -> Result<()> {
    let data = read_stdin()?;
    let notes = data.into_pattern("to-curve")?;

    let points = match feature {
        "pitch" => pitch_curve(&notes, bars, resolution),
        "velocity" => velocity_curve(&notes, bars, resolution),
        "density" => density_curve(&notes, bars, resolution),
        _ => {
            return Err(crate::error::Error::Other(format!(
                "unknown feature: \"{}\"\navailable: pitch, velocity, density",
                feature
            )))
        }
    };

    write_stdout(&MrData::Curve { points })
}

/// Convert a curve to a pattern (curve values become pitches).
pub fn to_pattern(duration: f64, velocity: i32) -> Result<()> {
    let data = read_stdin()?;
    let points = data.into_curve("to-pattern")?;

    let notes: Vec<MrNote> = points
        .iter()
        .map(|[t, v]| MrNote {
            pitch: v.round() as i32,
            start: *t,
            duration,
            velocity,
        })
        .filter(|n| (0..=127).contains(&n.pitch))
        .collect();

    write_stdout(&MrData::Pattern { notes })
}

/// Extract pitch contour as a normalized curve.
fn pitch_curve(notes: &[MrNote], bars: f64, resolution: f64) -> Vec<[f64; 2]> {
    if notes.is_empty() {
        return vec![];
    }

    let min_p = notes.iter().map(|n| n.pitch).min().unwrap() as f64;
    let max_p = notes.iter().map(|n| n.pitch).max().unwrap() as f64;
    let range = (max_p - min_p).max(1.0);

    // Sort notes by start time
    let mut sorted = notes.to_vec();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    // Sample pitch at each time step (use nearest note)
    let mut points = Vec::new();
    let mut t = 0.0;
    while t <= bars {
        let nearest = sorted
            .iter()
            .filter(|n| n.start <= t && t < n.start + n.duration)
            .last()
            .or_else(|| {
                sorted
                    .iter()
                    .min_by(|a, b| {
                        (a.start - t)
                            .abs()
                            .partial_cmp(&(b.start - t).abs())
                            .unwrap()
                    })
            });
        let v = match nearest {
            Some(n) => (n.pitch as f64 - min_p) / range,
            None => 0.5,
        };
        points.push([t, v]);
        t += resolution;
    }
    points
}

/// Extract velocity contour as a normalized curve.
fn velocity_curve(notes: &[MrNote], bars: f64, resolution: f64) -> Vec<[f64; 2]> {
    if notes.is_empty() {
        return vec![];
    }

    let mut sorted = notes.to_vec();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    let mut points = Vec::new();
    let mut t = 0.0;
    while t <= bars {
        let active: Vec<&MrNote> = sorted
            .iter()
            .filter(|n| n.start <= t && t < n.start + n.duration)
            .collect();
        let v = if active.is_empty() {
            // Find nearest note
            sorted
                .iter()
                .min_by(|a, b| {
                    (a.start - t)
                        .abs()
                        .partial_cmp(&(b.start - t).abs())
                        .unwrap()
                })
                .map(|n| n.velocity as f64 / 127.0)
                .unwrap_or(0.0)
        } else {
            // Average velocity of active notes
            let sum: f64 = active.iter().map(|n| n.velocity as f64).sum();
            sum / active.len() as f64 / 127.0
        };
        points.push([t, v]);
        t += resolution;
    }
    points
}

/// Extract note density as a curve (how many notes are active at each time).
fn density_curve(notes: &[MrNote], bars: f64, resolution: f64) -> Vec<[f64; 2]> {
    if notes.is_empty() {
        return vec![];
    }

    // Find max possible simultaneous notes for normalization
    let mut points = Vec::new();
    let mut t = 0.0;
    let mut max_density = 0usize;

    // First pass: find max density
    while t <= bars {
        let count = notes
            .iter()
            .filter(|n| n.start <= t && t < n.start + n.duration)
            .count();
        max_density = max_density.max(count);
        t += resolution;
    }

    let max_d = max_density.max(1) as f64;

    // Second pass: normalize
    t = 0.0;
    while t <= bars {
        let count = notes
            .iter()
            .filter(|n| n.start <= t && t < n.start + n.duration)
            .count();
        points.push([t, count as f64 / max_d]);
        t += resolution;
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_notes() -> Vec<MrNote> {
        vec![
            MrNote { pitch: 60, start: 0.0, duration: 2.0, velocity: 100 },
            MrNote { pitch: 72, start: 2.0, duration: 2.0, velocity: 50 },
            MrNote { pitch: 60, start: 4.0, duration: 2.0, velocity: 80 },
        ]
    }

    #[test]
    fn test_pitch_curve_range() {
        let notes = make_notes();
        let curve = pitch_curve(&notes, 6.0, 0.5);
        for p in &curve {
            assert!(p[1] >= 0.0 && p[1] <= 1.0, "pitch curve value out of range: {}", p[1]);
        }
        // At t=0 pitch is 60 (min) → 0.0
        assert!((curve[0][1] - 0.0).abs() < 0.001);
        // At t=2 pitch is 72 (max) → 1.0
        let at_2 = curve.iter().find(|p| (p[0] - 2.0).abs() < 0.01).unwrap();
        assert!((at_2[1] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_velocity_curve_range() {
        let notes = make_notes();
        let curve = velocity_curve(&notes, 6.0, 0.5);
        for p in &curve {
            assert!(p[1] >= 0.0 && p[1] <= 1.0);
        }
    }

    #[test]
    fn test_density_curve() {
        let notes = vec![
            MrNote { pitch: 60, start: 0.0, duration: 4.0, velocity: 100 },
            MrNote { pitch: 64, start: 1.0, duration: 2.0, velocity: 100 },
            MrNote { pitch: 67, start: 1.0, duration: 2.0, velocity: 100 },
        ];
        let curve = density_curve(&notes, 4.0, 0.5);
        // At t=0: 1 note active
        // At t=1: 3 notes active (max)
        let at_0 = curve.iter().find(|p| (p[0] - 0.0).abs() < 0.01).unwrap();
        let at_1 = curve.iter().find(|p| (p[0] - 1.0).abs() < 0.01).unwrap();
        assert!(at_0[1] < at_1[1]); // density increases
        assert!((at_1[1] - 1.0).abs() < 0.001); // max density = 1.0
    }

    #[test]
    fn test_to_pattern_logic() {
        let points = vec![[0.0, 60.0], [1.0, 64.0], [2.0, 67.0]];
        let notes: Vec<MrNote> = points
            .iter()
            .map(|p| MrNote {
                pitch: (p[1] as f64).round() as i32,
                start: p[0],
                duration: 0.5,
                velocity: 100,
            })
            .collect();
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0].pitch, 60);
        assert_eq!(notes[1].pitch, 64);
        assert_eq!(notes[2].pitch, 67);
    }
}
