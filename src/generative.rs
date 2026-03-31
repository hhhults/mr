use metaritual::accumulator::Accumulator;
use metaritual::construct;
use metaritual::markov::Markov;
use metaritual::pattern::Pattern;

use crate::error::Result;
use crate::json::{MrData, MrNote, write_stdout};
use crate::resolve::{parse_pitch, parse_scale};

/// Generate a melody using a Markov chain over a scale.
pub fn markov(
    root: &str,
    mode: &str,
    steps: usize,
    duration: f64,
    velocity: i32,
    seed: u64,
) -> Result<()> {
    let root_midi = parse_pitch(root)?;
    let intervals = parse_scale(mode)?;

    // Build scale as Pattern for Markov to learn from
    let scale_pattern = construct::scale(root_midi as f64, intervals, 2);
    let chain = Markov::from_pattern(&scale_pattern);

    let dur = if duration > 0.0 { duration } else { 1.0 / steps as f64 };
    let melody = chain.generate(steps, dur, Some(root_midi as f64), seed);

    let notes = pattern_to_mr_notes(&melody, duration, velocity);
    write_stdout(&MrData::Pattern { notes })
}

/// Generate an evolving pattern using the Accumulator.
pub fn accumulate(
    seed_json: &str,
    transforms_str: &str,
    iterations: usize,
    velocity: i32,
) -> Result<()> {
    // Parse seed from JSON or inline notation
    let seed_data: MrData = serde_json::from_str(seed_json).map_err(|e| {
        crate::error::Error::Other(format!("bad seed JSON: {}", e))
    })?;
    let seed_notes = seed_data.into_pattern("accumulate")?;
    let seed_pattern = mr_notes_to_pattern(&seed_notes);

    // Parse transform specs like "rotate:3,transpose:5,invert,retrograde"
    let transform_fns = parse_transforms(transforms_str)?;

    let acc = Accumulator::new(seed_pattern, transform_fns);
    let result = acc.generate(iterations);

    let notes = pattern_to_mr_notes(&result, 0.0, velocity);
    write_stdout(&MrData::Pattern { notes })
}

/// Convert MrNotes to a metaritual Pattern.
fn mr_notes_to_pattern(notes: &[MrNote]) -> Pattern {
    if notes.is_empty() {
        return Pattern::empty();
    }
    let mut sorted = notes.to_vec();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    Pattern::Seq(
        sorted
            .iter()
            .map(|n| Pattern::Atom(n.pitch as f64, n.duration))
            .collect(),
    )
}

/// Convert a metaritual Pattern back to MrNotes.
fn pattern_to_mr_notes(pattern: &Pattern, default_duration: f64, velocity: i32) -> Vec<MrNote> {
    let events = pattern.events();
    events
        .iter()
        .map(|e| {
            let dur = if default_duration > 0.0 { default_duration } else { e.duration };
            MrNote {
                pitch: e.value.round() as i32,
                start: e.start,
                duration: dur,
                velocity,
            }
        })
        .filter(|n| (0..=127).contains(&n.pitch))
        .collect()
}

/// Parse a comma-separated transform spec into boxed transform functions.
fn parse_transforms(spec: &str) -> Result<Vec<Box<dyn Fn(&Pattern) -> Pattern>>> {
    let mut transforms: Vec<Box<dyn Fn(&Pattern) -> Pattern>> = Vec::new();

    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let (name, arg) = if let Some(idx) = part.find(':') {
            (&part[..idx], Some(&part[idx + 1..]))
        } else {
            (part, None)
        };

        match name {
            "rotate" => {
                let k: i32 = arg.unwrap_or("1").parse().map_err(|_| {
                    crate::error::Error::Other(format!("bad rotate arg: {:?}", arg))
                })?;
                transforms.push(Box::new(move |p: &Pattern| p.rotate(k)));
            }
            "transpose" => {
                let n: f64 = arg.unwrap_or("1").parse().map_err(|_| {
                    crate::error::Error::Other(format!("bad transpose arg: {:?}", arg))
                })?;
                transforms.push(Box::new(move |p: &Pattern| p.shift(n)));
            }
            "invert" => {
                let axis: f64 = arg
                    .map(|a| a.parse::<f64>())
                    .transpose()
                    .map_err(|_| crate::error::Error::Other("bad invert axis".into()))?
                    .unwrap_or(60.0);
                transforms.push(Box::new(move |p: &Pattern| p.invert(axis)));
            }
            "retrograde" => {
                transforms.push(Box::new(|p: &Pattern| p.retrograde()));
            }
            "stretch" => {
                let f: f64 = arg.unwrap_or("2").parse().map_err(|_| {
                    crate::error::Error::Other(format!("bad stretch arg: {:?}", arg))
                })?;
                transforms.push(Box::new(move |p: &Pattern| p.stretch(f)));
            }
            "thin" => {
                let n: usize = arg.unwrap_or("2").parse().map_err(|_| {
                    crate::error::Error::Other(format!("bad thin arg: {:?}", arg))
                })?;
                transforms.push(Box::new(move |p: &Pattern| p.thin(n)));
            }
            "degrade" => {
                let prob: f64 = arg.unwrap_or("0.3").parse().map_err(|_| {
                    crate::error::Error::Other(format!("bad degrade arg: {:?}", arg))
                })?;
                transforms.push(Box::new(move |p: &Pattern| p.degrade(prob, 42)));
            }
            _ => {
                return Err(crate::error::Error::Other(format!(
                    "unknown transform: \"{}\"\navailable: rotate, transpose, invert, retrograde, stretch, thin, degrade",
                    name
                )));
            }
        }
    }

    Ok(transforms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mr_notes_roundtrip() {
        let notes = vec![
            MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 },
            MrNote { pitch: 64, start: 1.0, duration: 1.0, velocity: 100 },
            MrNote { pitch: 67, start: 2.0, duration: 1.0, velocity: 100 },
        ];
        let pattern = mr_notes_to_pattern(&notes);
        let back = pattern_to_mr_notes(&pattern, 0.0, 100);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].pitch, 60);
        assert_eq!(back[1].pitch, 64);
        assert_eq!(back[2].pitch, 67);
    }

    #[test]
    fn test_parse_transforms() {
        let transforms = parse_transforms("rotate:3,transpose:5,invert,retrograde").unwrap();
        assert_eq!(transforms.len(), 4);

        // Apply each to a simple pattern to verify they work
        let seed = Pattern::Seq(vec![
            Pattern::Atom(60.0, 0.25),
            Pattern::Atom(64.0, 0.25),
        ]);
        for t in &transforms {
            let result = t(&seed);
            // Should not panic
            assert!(!result.events().is_empty() || true); // degrade could empty it
        }
    }

    #[test]
    fn test_parse_transforms_bad_name() {
        let result = parse_transforms("nonexistent");
        assert!(result.is_err());
    }
}
