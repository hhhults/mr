use crate::error::Result;
use crate::json::{MrData, MrNote, read_stdin, write_stdout};
use crate::resolve::parse_scale;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

/// Read pattern from stdin, apply transform, write to stdout.
fn pipe(f: impl FnOnce(Vec<MrNote>) -> Vec<MrNote>) -> Result<()> {
    let data = read_stdin()?;
    let notes = data.into_pattern("transform")?;
    let result = f(notes);
    write_stdout(&MrData::Pattern { notes: result })
}

pub fn transpose(semitones: i32) -> Result<()> {
    pipe(|notes| {
        notes
            .into_iter()
            .map(|mut n| {
                n.pitch += semitones;
                n
            })
            .collect()
    })
}

pub fn rotate(k: i32) -> Result<()> {
    pipe(|mut notes| {
        if notes.is_empty() {
            return notes;
        }
        let len = notes.len() as i32;
        let k = ((k % len) + len) % len;
        // Rotate the pitch/velocity values while keeping start times
        let starts: Vec<f64> = notes.iter().map(|n| n.start).collect();
        let durations: Vec<f64> = notes.iter().map(|n| n.duration).collect();
        notes.rotate_right(k as usize);
        for (i, note) in notes.iter_mut().enumerate() {
            note.start = starts[i];
            note.duration = durations[i];
        }
        notes
    })
}

pub fn retrograde() -> Result<()> {
    pipe(|mut notes| {
        if notes.len() <= 1 {
            return notes;
        }
        // Reverse the pitch/velocity order, keep timing structure
        let starts: Vec<f64> = notes.iter().map(|n| n.start).collect();
        let durations: Vec<f64> = notes.iter().map(|n| n.duration).collect();
        notes.reverse();
        for (i, note) in notes.iter_mut().enumerate() {
            note.start = starts[i];
            note.duration = durations[i];
        }
        notes
    })
}

pub fn invert(axis: Option<i32>) -> Result<()> {
    pipe(|notes| {
        if notes.is_empty() {
            return notes;
        }
        let ax = axis.unwrap_or(notes[0].pitch);
        notes
            .into_iter()
            .map(|mut n| {
                n.pitch = ax - (n.pitch - ax);
                n
            })
            .collect()
    })
}

pub fn stretch(factor: f64) -> Result<()> {
    pipe(|notes| {
        notes
            .into_iter()
            .map(|mut n| {
                n.start *= factor;
                n.duration *= factor;
                n
            })
            .collect()
    })
}

pub fn degrade(probability: f64, seed: u64) -> Result<()> {
    pipe(|notes| {
        let mut rng = SmallRng::seed_from_u64(seed);
        notes
            .into_iter()
            .filter(|_| rng.random::<f64>() > probability)
            .collect()
    })
}

pub fn thin(keep_every: usize) -> Result<()> {
    pipe(|notes| {
        notes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % keep_every == 0)
            .map(|(_, n)| n)
            .collect()
    })
}

pub fn humanize(timing: f64, velocity: f64, seed: u64) -> Result<()> {
    pipe(|notes| {
        let mut rng = SmallRng::seed_from_u64(seed);
        notes
            .into_iter()
            .map(|mut n| {
                n.start += rng.random::<f64>() * timing * 2.0 - timing;
                if n.start < 0.0 {
                    n.start = 0.0;
                }
                let vel_offset = (rng.random::<f64>() * velocity * 2.0 - velocity) as i32;
                n.velocity = (n.velocity + vel_offset).clamp(1, 127);
                n
            })
            .collect()
    })
}

pub fn swing(amount: f64) -> Result<()> {
    pipe(|notes| {
        // Swing offbeat notes (odd-indexed by start time order)
        let mut sorted = notes;
        sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        for (i, note) in sorted.iter_mut().enumerate() {
            if i % 2 == 1 {
                note.start += amount;
            }
        }
        sorted
    })
}

pub fn overlap(amount: f64) -> Result<()> {
    pipe(|mut notes| {
        notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        for i in 0..notes.len().saturating_sub(1) {
            let gap = notes[i + 1].start - notes[i].start;
            if gap > 0.0 {
                notes[i].duration = gap + amount;
            }
        }
        // Last note: just extend by amount
        if let Some(last) = notes.last_mut() {
            last.duration += amount;
        }
        notes
    })
}

pub fn crescendo(start_vel: i32, end_vel: i32) -> Result<()> {
    pipe(|mut notes| {
        let n = notes.len();
        if n <= 1 {
            if let Some(note) = notes.first_mut() {
                note.velocity = start_vel;
            }
            return notes;
        }
        // Sort by start time for consistent crescendo
        notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        for (i, note) in notes.iter_mut().enumerate() {
            let frac = i as f64 / (n - 1) as f64;
            note.velocity =
                (start_vel as f64 + frac * (end_vel - start_vel) as f64).round() as i32;
            note.velocity = note.velocity.clamp(1, 127);
        }
        notes
    })
}

pub fn quantize(root: &str, mode: &str) -> Result<()> {
    let root_midi = crate::resolve::parse_pitch(root)?;
    let intervals = parse_scale(mode)?;

    // Build full MIDI pitch set for the scale
    let root_pc = root_midi % 12;
    let mut scale_pitches: Vec<i32> = Vec::new();
    for octave in 0..=10 {
        for &interval in intervals {
            let pitch = root_pc + octave * 12 + interval as i32;
            if (0..=127).contains(&pitch) {
                scale_pitches.push(pitch);
            }
        }
    }
    scale_pitches.sort();

    pipe(|notes| {
        notes
            .into_iter()
            .map(|mut n| {
                // Snap to nearest scale pitch
                n.pitch = *scale_pitches
                    .iter()
                    .min_by_key(|&&p| (p - n.pitch).abs())
                    .unwrap_or(&n.pitch);
                n
            })
            .collect()
    })
}

pub fn repeat(times: usize) -> Result<()> {
    pipe(|notes| {
        if notes.is_empty() || times == 0 {
            return vec![];
        }
        let total_length = notes
            .iter()
            .map(|n| n.start + n.duration)
            .fold(0.0f64, f64::max);

        let mut result = Vec::new();
        for rep in 0..times {
            let offset = rep as f64 * total_length;
            for n in &notes {
                result.push(MrNote {
                    pitch: n.pitch,
                    start: n.start + offset,
                    duration: n.duration,
                    velocity: n.velocity,
                });
            }
        }
        result
    })
}

pub fn velocities(vel_pattern: &str) -> Result<()> {
    let vels: Vec<i32> = vel_pattern
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<i32>()
                .map_err(|_| crate::error::Error::Other(format!("bad velocity: {}", s)))
        })
        .collect::<Result<Vec<i32>>>()?;

    pipe(|notes| {
        if vels.is_empty() {
            return notes;
        }
        notes
            .into_iter()
            .enumerate()
            .map(|(i, mut n)| {
                n.velocity = vels[i % vels.len()].clamp(1, 127);
                n
            })
            .collect()
    })
}

pub fn concat(second_json: &str) -> Result<()> {
    let data = read_stdin()?;
    let mut notes = data.into_pattern("concat")?;

    let second_data: MrData = serde_json::from_str(second_json)
        .map_err(|e| crate::error::Error::Other(format!("bad JSON for second pattern: {}", e)))?;
    let second_notes = second_data.into_pattern("concat")?;

    let offset = notes
        .iter()
        .map(|n| n.start + n.duration)
        .fold(0.0f64, f64::max);

    for mut n in second_notes {
        n.start += offset;
        notes.push(n);
    }

    write_stdout(&MrData::Pattern { notes })
}

pub fn morph(second_json: &str, amount: f64, seed: u64) -> Result<()> {
    let data = read_stdin()?;
    let notes_a = data.into_pattern("morph")?;

    let second_data: MrData = serde_json::from_str(second_json)
        .map_err(|e| crate::error::Error::Other(format!("bad JSON for second pattern: {}", e)))?;
    let notes_b = second_data.into_pattern("morph")?;

    let amount = amount.clamp(0.0, 1.0);
    let grid = 0.25; // 16th note quantization

    // Quantize notes onto the grid, keyed by (grid_position, pitch)
    use std::collections::HashMap;

    // Key: (grid slot as integer, pitch)
    // Value: (velocity, duration) — last write wins if duplicates
    let mut map_a: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
    for n in &notes_a {
        let slot = (n.start / grid).round() as i64;
        map_a.insert((slot, n.pitch), (n.velocity, n.duration));
    }

    let mut map_b: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
    for n in &notes_b {
        let slot = (n.start / grid).round() as i64;
        map_b.insert((slot, n.pitch), (n.velocity, n.duration));
    }

    // Collect all grid positions
    let mut all_keys: Vec<(i64, i32)> = map_a.keys().chain(map_b.keys()).cloned().collect();
    all_keys.sort();
    all_keys.dedup();

    let mut rng = SmallRng::seed_from_u64(seed);
    let mut result = Vec::new();

    for key in &all_keys {
        let in_a = map_a.get(key);
        let in_b = map_b.get(key);

        let keep = match (in_a, in_b) {
            // In both: always survives
            (Some(_), Some(_)) => true,
            // Only in A: survives with probability 1.0 - amount
            (Some(_), None) => rng.random::<f64>() < (1.0 - amount),
            // Only in B: survives with probability amount
            (None, Some(_)) => rng.random::<f64>() < amount,
            // In neither: impossible since we built keys from both maps
            (None, None) => false,
        };

        if keep {
            let (vel_a, dur_a) = in_a.copied().unwrap_or((100, grid));
            let (vel_b, dur_b) = in_b.copied().unwrap_or((100, grid));
            let vel = ((vel_a as f64) * (1.0 - amount) + (vel_b as f64) * amount).round() as i32;
            let dur = dur_a * (1.0 - amount) + dur_b * amount;

            result.push(MrNote {
                pitch: key.1,
                start: key.0 as f64 * grid,
                duration: dur,
                velocity: vel.clamp(1, 127),
            });
        }
    }

    result.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    write_stdout(&MrData::Pattern { notes: result })
}

pub fn weave(voice: &str, balance: f64, seed: u64, transpose: i32) -> Result<()> {
    let data = read_stdin()?;
    let notes = data.into_pattern("weave")?;
    let balance = balance.clamp(0.0, 1.0);
    let is_b = match voice {
        "b" | "B" => true,
        "a" | "A" => false,
        _ => {
            return Err(crate::error::Error::Other(
                "voice must be 'a' or 'b'".to_string(),
            ))
        }
    };

    let mut rng = SmallRng::seed_from_u64(seed);
    let result: Vec<MrNote> = notes
        .into_iter()
        .filter(|_| {
            let val = rng.random::<f64>();
            // val < balance => voice B; val >= balance => voice A
            if is_b { val < balance } else { val >= balance }
        })
        .map(|mut n| {
            n.pitch += transpose;
            n
        })
        .collect();

    write_stdout(&MrData::Pattern { notes: result })
}

pub fn stack(second_json: &str) -> Result<()> {
    let data = read_stdin()?;
    let mut notes = data.into_pattern("stack")?;

    let second_data: MrData = serde_json::from_str(second_json)
        .map_err(|e| crate::error::Error::Other(format!("bad JSON for second pattern: {}", e)))?;
    let second_notes = second_data.into_pattern("stack")?;

    notes.extend(second_notes);

    write_stdout(&MrData::Pattern { notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_notes(pitches: &[(i32, f64, f64)]) -> Vec<MrNote> {
        pitches
            .iter()
            .map(|&(pitch, start, dur)| MrNote {
                pitch,
                start,
                duration: dur,
                velocity: 100,
            })
            .collect()
    }

    #[test]
    fn test_transpose_notes() {
        let notes = make_notes(&[(60, 0.0, 1.0), (64, 1.0, 1.0)]);
        let result: Vec<MrNote> = notes
            .into_iter()
            .map(|mut n| {
                n.pitch += 5;
                n
            })
            .collect();
        assert_eq!(result[0].pitch, 65);
        assert_eq!(result[1].pitch, 69);
    }

    #[test]
    fn test_retrograde_logic() {
        let notes = make_notes(&[(60, 0.0, 1.0), (64, 1.0, 1.0), (67, 2.0, 1.0)]);
        let starts: Vec<f64> = notes.iter().map(|n| n.start).collect();
        let durations: Vec<f64> = notes.iter().map(|n| n.duration).collect();
        let mut reversed = notes;
        reversed.reverse();
        for (i, note) in reversed.iter_mut().enumerate() {
            note.start = starts[i];
            note.duration = durations[i];
        }
        // Pitches reversed: 67, 64, 60
        assert_eq!(reversed[0].pitch, 67);
        assert_eq!(reversed[1].pitch, 64);
        assert_eq!(reversed[2].pitch, 60);
        // Times preserved
        assert_eq!(reversed[0].start, 0.0);
        assert_eq!(reversed[1].start, 1.0);
        assert_eq!(reversed[2].start, 2.0);
    }

    #[test]
    fn test_invert_logic() {
        let notes = make_notes(&[(60, 0.0, 1.0), (64, 1.0, 1.0), (67, 2.0, 1.0)]);
        let axis = 60;
        let result: Vec<i32> = notes.iter().map(|n| axis - (n.pitch - axis)).collect();
        assert_eq!(result, vec![60, 56, 53]); // Mirror around C4
    }

    #[test]
    fn test_stretch_logic() {
        let notes = make_notes(&[(60, 0.0, 1.0), (64, 2.0, 1.0)]);
        let factor = 2.0;
        let result: Vec<(f64, f64)> = notes
            .iter()
            .map(|n| (n.start * factor, n.duration * factor))
            .collect();
        assert_eq!(result, vec![(0.0, 2.0), (4.0, 2.0)]);
    }

    #[test]
    fn test_degrade_reduces_notes() {
        let notes = make_notes(&[
            (60, 0.0, 1.0),
            (62, 1.0, 1.0),
            (64, 2.0, 1.0),
            (65, 3.0, 1.0),
            (67, 4.0, 1.0),
            (69, 5.0, 1.0),
            (71, 6.0, 1.0),
            (72, 7.0, 1.0),
        ]);
        let mut rng = SmallRng::seed_from_u64(42);
        let result: Vec<MrNote> = notes
            .into_iter()
            .filter(|_| rng.random::<f64>() > 0.5)
            .collect();
        // With 50% probability, roughly half should survive
        assert!(result.len() < 8);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_thin_every_2() {
        let notes = make_notes(&[
            (60, 0.0, 1.0),
            (62, 1.0, 1.0),
            (64, 2.0, 1.0),
            (65, 3.0, 1.0),
        ]);
        let result: Vec<MrNote> = notes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .map(|(_, n)| n)
            .collect();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].pitch, 60);
        assert_eq!(result[1].pitch, 64);
    }

    #[test]
    fn test_crescendo_logic() {
        let mut notes = make_notes(&[
            (60, 0.0, 1.0),
            (62, 1.0, 1.0),
            (64, 2.0, 1.0),
            (65, 3.0, 1.0),
        ]);
        let n = notes.len();
        for (i, note) in notes.iter_mut().enumerate() {
            let frac = i as f64 / (n - 1) as f64;
            note.velocity = (30.0 + frac * (127.0 - 30.0)).round() as i32;
        }
        assert_eq!(notes[0].velocity, 30);
        assert_eq!(notes[3].velocity, 127);
        // Middle values should be interpolated
        assert!(notes[1].velocity > 30 && notes[1].velocity < 127);
    }

    #[test]
    fn test_repeat_timing() {
        let notes = make_notes(&[(60, 0.0, 1.0), (64, 1.0, 1.0)]);
        let total_length = 2.0; // max(0+1, 1+1)
        let mut result = Vec::new();
        for rep in 0..3 {
            let offset = rep as f64 * total_length;
            for n in &notes {
                result.push(MrNote {
                    pitch: n.pitch,
                    start: n.start + offset,
                    duration: n.duration,
                    velocity: n.velocity,
                });
            }
        }
        assert_eq!(result.len(), 6);
        assert_eq!(result[0].start, 0.0);
        assert_eq!(result[2].start, 2.0); // second repetition
        assert_eq!(result[4].start, 4.0); // third repetition
    }

    #[test]
    fn test_overlap_logic() {
        let mut notes = make_notes(&[(60, 0.0, 0.5), (64, 1.0, 0.5), (67, 2.0, 0.5)]);
        let amount = 0.3;
        for i in 0..notes.len().saturating_sub(1) {
            let gap = notes[i + 1].start - notes[i].start;
            if gap > 0.0 {
                notes[i].duration = gap + amount;
            }
        }
        notes.last_mut().unwrap().duration += amount;
        assert!((notes[0].duration - 1.3).abs() < 0.001); // 1.0 gap + 0.3
        assert!((notes[1].duration - 1.3).abs() < 0.001);
        assert!((notes[2].duration - 0.8).abs() < 0.001); // 0.5 + 0.3
    }

    #[test]
    fn test_quantize_snaps_to_scale() {
        let root_pc = 0; // C
        let intervals = metaritual::construct::MAJOR; // C D E F G A B
        let mut scale_pitches: Vec<i32> = Vec::new();
        for octave in 0..=10 {
            for &interval in intervals {
                let pitch = root_pc + octave * 12 + interval as i32;
                if (0..=127).contains(&pitch) {
                    scale_pitches.push(pitch);
                }
            }
        }
        scale_pitches.sort();

        // C#4 (61) should snap to C4 (60) or D4 (62)
        let snapped = *scale_pitches
            .iter()
            .min_by_key(|&&p| (p - 61).abs())
            .unwrap();
        assert!(snapped == 60 || snapped == 62);

        // F#4 (66) should snap to F4 (65) or G4 (67)
        let snapped = *scale_pitches
            .iter()
            .min_by_key(|&&p| (p - 66).abs())
            .unwrap();
        assert!(snapped == 65 || snapped == 67);
    }

    #[test]
    fn test_rotate_preserves_timing() {
        let notes = make_notes(&[(60, 0.0, 1.0), (64, 1.0, 1.0), (67, 2.0, 1.0)]);
        let starts: Vec<f64> = notes.iter().map(|n| n.start).collect();
        let k = 1usize;
        let mut rotated = notes;
        rotated.rotate_right(k);
        for (i, note) in rotated.iter_mut().enumerate() {
            note.start = starts[i];
        }
        // After rotating pitches right by 1: [67, 60, 64]
        assert_eq!(rotated[0].pitch, 67);
        assert_eq!(rotated[1].pitch, 60);
        assert_eq!(rotated[2].pitch, 64);
        // Times unchanged
        assert_eq!(rotated[0].start, 0.0);
        assert_eq!(rotated[1].start, 1.0);
        assert_eq!(rotated[2].start, 2.0);
    }

    #[test]
    fn test_morph_amount_zero_keeps_a() {
        // amount=0 should keep all of A, none of B-only
        let a = make_notes(&[(60, 0.0, 0.25), (60, 0.5, 0.25)]);
        let b = make_notes(&[(60, 0.25, 0.25), (60, 0.75, 0.25)]);
        let amount = 0.0;
        let grid = 0.25;

        use std::collections::HashMap;
        let mut map_a: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
        for n in &a { map_a.insert(((n.start / grid).round() as i64, n.pitch), (n.velocity, n.duration)); }
        let mut map_b: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
        for n in &b { map_b.insert(((n.start / grid).round() as i64, n.pitch), (n.velocity, n.duration)); }

        let mut all_keys: Vec<(i64, i32)> = map_a.keys().chain(map_b.keys()).cloned().collect();
        all_keys.sort();
        all_keys.dedup();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut result = Vec::new();
        for key in &all_keys {
            let in_a = map_a.get(key);
            let in_b = map_b.get(key);
            let keep = match (in_a, in_b) {
                (Some(_), Some(_)) => true,
                (Some(_), None) => rng.random::<f64>() < (1.0 - amount),
                (None, Some(_)) => rng.random::<f64>() < amount,
                (None, None) => false,
            };
            if keep { result.push(key); }
        }
        // A has slots 0, 2; B has slots 1, 3
        // amount=0: keep all A (slots 0,2), none of B-only (slots 1,3)
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, 0);
        assert_eq!(result[1].0, 2);
    }

    #[test]
    fn test_morph_amount_one_keeps_b() {
        let a = make_notes(&[(60, 0.0, 0.25), (60, 0.5, 0.25)]);
        let b = make_notes(&[(60, 0.25, 0.25), (60, 0.75, 0.25)]);
        let amount = 1.0;
        let grid = 0.25;

        use std::collections::HashMap;
        let mut map_a: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
        for n in &a { map_a.insert(((n.start / grid).round() as i64, n.pitch), (n.velocity, n.duration)); }
        let mut map_b: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
        for n in &b { map_b.insert(((n.start / grid).round() as i64, n.pitch), (n.velocity, n.duration)); }

        let mut all_keys: Vec<(i64, i32)> = map_a.keys().chain(map_b.keys()).cloned().collect();
        all_keys.sort();
        all_keys.dedup();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut result = Vec::new();
        for key in &all_keys {
            let in_a = map_a.get(key);
            let in_b = map_b.get(key);
            let keep = match (in_a, in_b) {
                (Some(_), Some(_)) => true,
                (Some(_), None) => rng.random::<f64>() < (1.0 - amount),
                (None, Some(_)) => rng.random::<f64>() < amount,
                (None, None) => false,
            };
            if keep { result.push(key); }
        }
        // amount=1: keep none of A-only (slots 0,2), all of B (slots 1,3)
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, 1);
        assert_eq!(result[1].0, 3);
    }

    #[test]
    fn test_morph_shared_notes_always_survive() {
        // Same note at same grid position in both => always survives at any amount
        let a = make_notes(&[(60, 0.0, 0.25)]);
        let b = make_notes(&[(60, 0.0, 0.25)]);
        let grid = 0.25;

        use std::collections::HashMap;
        let mut map_a: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
        for n in &a { map_a.insert(((n.start / grid).round() as i64, n.pitch), (n.velocity, n.duration)); }
        let mut map_b: HashMap<(i64, i32), (i32, f64)> = HashMap::new();
        for n in &b { map_b.insert(((n.start / grid).round() as i64, n.pitch), (n.velocity, n.duration)); }

        // Shared notes survive at any amount
        for &amount in &[0.0, 0.3, 0.5, 0.7, 1.0] {
            let key = (0i64, 60i32);
            let in_a = map_a.get(&key);
            let in_b = map_b.get(&key);
            let keep = match (in_a, in_b) {
                (Some(_), Some(_)) => true,
                _ => false,
            };
            assert!(keep, "shared note should survive at amount={}", amount);
        }
    }

    #[test]
    fn test_morph_velocity_interpolation() {
        // Velocity should blend between A and B
        let vel_a = 100i32;
        let vel_b = 60i32;
        let amount = 0.5;
        let blended = ((vel_a as f64) * (1.0 - amount) + (vel_b as f64) * amount).round() as i32;
        assert_eq!(blended, 80);

        let amount = 0.25;
        let blended = ((vel_a as f64) * (1.0 - amount) + (vel_b as f64) * amount).round() as i32;
        assert_eq!(blended, 90);
    }

    #[test]
    fn test_weave_complementarity() {
        // With the same seed, voice A + voice B should contain all original notes
        // with no duplicates and no missing notes.
        let notes = make_notes(&[
            (60, 0.0, 1.0),
            (62, 1.0, 1.0),
            (64, 2.0, 1.0),
            (65, 3.0, 1.0),
            (67, 4.0, 1.0),
            (69, 5.0, 1.0),
            (71, 6.0, 1.0),
            (72, 7.0, 1.0),
        ]);

        let seed = 77u64;
        let balance = 0.4;

        // Simulate weave for voice A
        let mut rng_a = SmallRng::seed_from_u64(seed);
        let voice_a: Vec<MrNote> = notes
            .iter()
            .filter(|_| rng_a.random::<f64>() >= balance)
            .cloned()
            .collect();

        // Simulate weave for voice B (same seed, same RNG sequence)
        let mut rng_b = SmallRng::seed_from_u64(seed);
        let voice_b: Vec<MrNote> = notes
            .iter()
            .filter(|_| rng_b.random::<f64>() < balance)
            .cloned()
            .collect();

        // Together they should have all original notes
        assert_eq!(
            voice_a.len() + voice_b.len(),
            notes.len(),
            "A ({}) + B ({}) should equal original ({})",
            voice_a.len(),
            voice_b.len(),
            notes.len()
        );

        // Collect all pitches from both voices and verify they match the original
        let mut combined_pitches: Vec<i32> =
            voice_a.iter().chain(voice_b.iter()).map(|n| n.pitch).collect();
        combined_pitches.sort();
        let mut original_pitches: Vec<i32> = notes.iter().map(|n| n.pitch).collect();
        original_pitches.sort();
        assert_eq!(combined_pitches, original_pitches);
    }

    #[test]
    fn test_weave_balance_zero_all_in_a() {
        let notes = make_notes(&[
            (60, 0.0, 1.0),
            (62, 1.0, 1.0),
            (64, 2.0, 1.0),
            (65, 3.0, 1.0),
        ]);

        let seed = 42u64;
        let balance = 0.0;

        // balance=0 means val < 0 is never true => nothing in B, everything in A
        let mut rng = SmallRng::seed_from_u64(seed);
        let voice_a: Vec<MrNote> = notes
            .iter()
            .filter(|_| rng.random::<f64>() >= balance)
            .cloned()
            .collect();
        assert_eq!(voice_a.len(), notes.len());

        let mut rng = SmallRng::seed_from_u64(seed);
        let voice_b: Vec<MrNote> = notes
            .iter()
            .filter(|_| rng.random::<f64>() < balance)
            .cloned()
            .collect();
        assert_eq!(voice_b.len(), 0);
    }

    #[test]
    fn test_weave_balance_one_all_in_b() {
        let notes = make_notes(&[
            (60, 0.0, 1.0),
            (62, 1.0, 1.0),
            (64, 2.0, 1.0),
            (65, 3.0, 1.0),
        ]);

        let seed = 42u64;
        let balance = 1.0;

        // balance=1 means val < 1.0 is always true => everything in B, nothing in A
        let mut rng = SmallRng::seed_from_u64(seed);
        let voice_a: Vec<MrNote> = notes
            .iter()
            .filter(|_| rng.random::<f64>() >= balance)
            .cloned()
            .collect();
        assert_eq!(voice_a.len(), 0);

        let mut rng = SmallRng::seed_from_u64(seed);
        let voice_b: Vec<MrNote> = notes
            .iter()
            .filter(|_| rng.random::<f64>() < balance)
            .cloned()
            .collect();
        assert_eq!(voice_b.len(), notes.len());
    }

    #[test]
    fn test_weave_transpose() {
        // Verify transpose shifts pitches correctly
        let notes = make_notes(&[(60, 0.0, 1.0), (64, 1.0, 1.0)]);
        let transpose = 12;

        let result: Vec<MrNote> = notes
            .into_iter()
            .map(|mut n| {
                n.pitch += transpose;
                n
            })
            .collect();
        assert_eq!(result[0].pitch, 72);
        assert_eq!(result[1].pitch, 76);
    }
}
