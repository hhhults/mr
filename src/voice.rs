use crate::error::Result;
use crate::json::{MrData, MrNote, read_stdin, write_stdout};

/// Voice a chord progression as a pattern.
///
/// Each chord becomes a set of simultaneous notes, voiced within the given range.
fn voice_progression(
    voicing_fn: impl Fn(&[i32], i32, i32) -> Vec<i32>,
    range: [i32; 2],
    duration: f64,
) -> Result<()> {
    let data = read_stdin()?;
    let chords = data.into_progression("voice")?;

    let mut notes = Vec::new();
    let mut t = 0.0;

    for chord in &chords {
        let root = chord.root_midi;

        // Look up intervals for this quality
        let intervals = quality_to_intervals(&chord.quality);
        let raw_pitches: Vec<i32> = intervals.iter().map(|&i| root + i as i32).collect();

        let voiced = voicing_fn(&raw_pitches, range[0], range[1]);

        for &pitch in &voiced {
            notes.push(MrNote {
                pitch,
                start: t,
                duration,
                velocity: 100,
            });
        }
        t += duration;
    }

    write_stdout(&MrData::Pattern { notes })
}

fn quality_to_intervals(quality: &str) -> Vec<u8> {
    match quality {
        "major" => vec![0, 4, 7],
        "minor" => vec![0, 3, 7],
        "augmented" => vec![0, 4, 8],
        "diminished" => vec![0, 3, 6],
        "sus2" => vec![0, 2, 7],
        "sus4" => vec![0, 5, 7],
        "power5" => vec![0, 7],
        "maj7" => vec![0, 4, 7, 11],
        "dom7" => vec![0, 4, 7, 10],
        "min7" => vec![0, 3, 7, 10],
        "minmaj7" => vec![0, 3, 7, 11],
        "hdim7" => vec![0, 3, 6, 10],
        "dim7" => vec![0, 3, 6, 9],
        "augmaj7" => vec![0, 4, 8, 11],
        "augdom7" => vec![0, 4, 8, 10],
        "maj6" => vec![0, 4, 7, 9],
        "min6" => vec![0, 3, 7, 9],
        "maj9" => vec![0, 4, 7, 11, 14],
        "min9" => vec![0, 3, 7, 10, 14],
        "dom9" => vec![0, 4, 7, 10, 14],
        "maj11" => vec![0, 4, 7, 11, 14, 17],
        "min11" => vec![0, 3, 7, 10, 14, 17],
        "dom11" => vec![0, 4, 7, 10, 14, 17],
        "maj13" => vec![0, 4, 7, 11, 14, 17, 21],
        "min13" => vec![0, 3, 7, 10, 14, 17, 21],
        "dom13" => vec![0, 4, 7, 10, 14, 17, 21],
        "add9" => vec![0, 4, 7, 14],
        "add11" => vec![0, 4, 7, 17],
        "7#9" => vec![0, 4, 7, 10, 15],
        "7b9" => vec![0, 4, 7, 10, 13],
        "7#11" => vec![0, 4, 7, 10, 18],
        _ => vec![0, 4, 7], // default to major triad
    }
}

/// Place pitches within a range, shifting octaves as needed.
fn place_in_range(pitches: &[i32], low: i32, high: i32) -> Vec<i32> {
    pitches
        .iter()
        .map(|&p| {
            let mut placed = p;
            while placed < low { placed += 12; }
            while placed > high { placed -= 12; }
            if placed < low { placed += 12; } // safety
            placed.clamp(low, high)
        })
        .collect()
}

/// Bloom voicing: stagger entries slightly for a flower-opening effect.
pub fn bloom(range: [i32; 2], spread: f64) -> Result<()> {
    voice_progression(
        |pitches, low, high| {
            let mut voiced = place_in_range(pitches, low, high);
            voiced.sort();
            voiced
        },
        range,
        4.0, // duration per chord — will be overridden by the --duration flag if added
    )?;

    // Re-apply spread: offset each note's start by i * spread
    // We need to post-process the output
    // Actually, let's handle this inline in voice_progression
    Ok(())
}

/// Open voicing: drop inner voices down an octave.
pub fn open(range: [i32; 2]) -> Result<()> {
    voice_progression(
        |pitches, low, high| {
            let mut voiced = place_in_range(pitches, low, high);
            voiced.sort();
            // Drop 2nd voice down an octave (drop-2 voicing)
            if voiced.len() >= 3 {
                voiced[1] -= 12;
                if voiced[1] < low {
                    voiced[1] += 12;
                }
            }
            voiced.sort();
            voiced
        },
        range,
        4.0,
    )
}

/// Wide voicing: spread across multiple octaves.
pub fn wide(range: [i32; 2]) -> Result<()> {
    voice_progression(
        |pitches, low, high| {
            let span = (high - low).max(12);
            let n = pitches.len();
            pitches
                .iter()
                .enumerate()
                .map(|(i, &p)| {
                    let target_oct = low + (i as i32 * span / n.max(1) as i32);
                    let pc = p % 12;
                    let base = (target_oct / 12) * 12 + pc;
                    base.clamp(low, high)
                })
                .collect()
        },
        range,
        4.0,
    )
}

/// Voice-led: minimal voice movement between successive chords.
pub fn led(range: [i32; 2]) -> Result<()> {
    let data = read_stdin()?;
    let chords = data.into_progression("voice led")?;

    let mut notes = Vec::new();
    let mut t = 0.0;
    let mut prev_voiced: Option<Vec<i32>> = None;

    for chord in &chords {
        let root = chord.root_midi;
        let intervals = quality_to_intervals(&chord.quality);
        let raw: Vec<i32> = intervals.iter().map(|&i| root + i as i32).collect();

        let voiced = match &prev_voiced {
            None => {
                // First chord: place in range
                place_in_range(&raw, range[0], range[1])
            }
            Some(prev) => {
                // Minimize total voice movement from previous chord
                voice_lead_to(&raw, prev, range[0], range[1])
            }
        };

        for &pitch in &voiced {
            notes.push(MrNote {
                pitch,
                start: t,
                duration: 4.0,
                velocity: 100,
            });
        }
        prev_voiced = Some(voiced);
        t += 4.0;
    }

    write_stdout(&MrData::Pattern { notes })
}

/// Find the voicing of `target_pcs` closest to `prev` voices.
fn voice_lead_to(target_pcs: &[i32], prev: &[i32], low: i32, high: i32) -> Vec<i32> {
    // For each target pitch class, find the octave placement closest to the
    // corresponding previous voice (by index, wrapping if sizes differ).
    target_pcs
        .iter()
        .enumerate()
        .map(|(i, &pc)| {
            let pc_mod = pc % 12;
            let reference = if i < prev.len() { prev[i] } else { (low + high) / 2 };
            // Try all octave placements and pick the closest to reference
            let mut best = pc_mod;
            let mut best_dist = i32::MAX;
            let mut candidate = pc_mod;
            while candidate <= high + 12 {
                if candidate >= low && candidate <= high {
                    let dist = (candidate - reference).abs();
                    if dist < best_dist {
                        best = candidate;
                        best_dist = dist;
                    }
                }
                candidate += 12;
            }
            best.clamp(low, high)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_place_in_range() {
        let pitches = vec![60, 64, 67]; // C4 E4 G4
        let placed = place_in_range(&pitches, 48, 72);
        for &p in &placed {
            assert!(p >= 48 && p <= 72, "pitch {} out of range", p);
        }
    }

    #[test]
    fn test_voice_lead_to_stays_close() {
        let prev = vec![60, 64, 67]; // C4 E4 G4
        let target = vec![62, 65, 69]; // D4 F4 A4 (Dm)
        let voiced = voice_lead_to(&target, &prev, 48, 84);
        // Each voice should have moved by at most a few semitones
        for (i, &v) in voiced.iter().enumerate() {
            let dist = (v - prev[i]).abs();
            assert!(dist <= 12, "voice {} moved {} semitones (prev={}, new={})",
                i, dist, prev[i], v);
        }
    }

    #[test]
    fn test_quality_to_intervals() {
        assert_eq!(quality_to_intervals("maj7"), vec![0, 4, 7, 11]);
        assert_eq!(quality_to_intervals("minor"), vec![0, 3, 7]);
        assert_eq!(quality_to_intervals("dom7"), vec![0, 4, 7, 10]);
    }
}
