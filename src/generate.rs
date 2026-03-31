use crate::error::Result;
use crate::json::{MrData, MrNote, write_stdout};
use crate::resolve::{parse_chord_spec, parse_pitch, parse_scale};

/// Generate a scale pattern.
pub fn scale(root: &str, mode: &str, octaves: usize, duration: f64, velocity: i32) -> Result<()> {
    let root_midi = parse_pitch(root)?;
    let intervals = parse_scale(mode)?;

    let mut notes = Vec::new();
    let mut t = 0.0;
    for oct in 0..octaves {
        for &interval in intervals {
            let pitch = root_midi + (oct as i32 * 12) + interval as i32;
            if (0..=127).contains(&pitch) {
                notes.push(MrNote {
                    pitch,
                    start: t,
                    duration,
                    velocity,
                });
                t += duration;
            }
        }
    }

    write_stdout(&MrData::Pattern { notes })
}

/// Generate a euclidean rhythm pattern.
pub fn euclidean(
    hits: usize,
    steps: usize,
    pitch: i32,
    duration: f64,
    rotation: usize,
    velocity: i32,
) -> Result<()> {
    let bools = bjorklund(hits.min(steps), steps);

    // Apply rotation
    let mut rotated = bools;
    if steps > 0 && rotation > 0 {
        let rot = rotation % steps;
        rotated.rotate_left(rot);
    }

    let mut notes = Vec::new();
    for (i, &hit) in rotated.iter().enumerate() {
        if hit {
            notes.push(MrNote {
                pitch,
                start: i as f64 * duration,
                duration,
                velocity,
            });
        }
    }

    write_stdout(&MrData::Pattern { notes })
}

/// Generate a chord (simultaneous notes).
pub fn chord(spec: &str, duration: f64, velocity: i32) -> Result<()> {
    let (root_midi, intervals) = parse_chord_spec(spec)?;

    let notes: Vec<MrNote> = intervals
        .iter()
        .map(|&i| MrNote {
            pitch: root_midi + i as i32,
            start: 0.0,
            duration,
            velocity,
        })
        .collect();

    write_stdout(&MrData::Pattern { notes })
}

/// Parse inline note notation: "C4:2 E4:1 G4:1:100 B4:4"
/// Format: pitch:duration[:velocity]
pub fn notes(notation: &str) -> Result<()> {
    let mut result = Vec::new();
    let mut t = 0.0;

    for token in notation.split_whitespace() {
        let parts: Vec<&str> = token.split(':').collect();
        if parts.is_empty() {
            continue;
        }

        let pitch = parse_pitch(parts[0])?;
        let dur = if parts.len() > 1 {
            parts[1]
                .parse::<f64>()
                .map_err(|_| crate::error::Error::BadNote(token.to_string()))?
        } else {
            1.0
        };
        let vel = if parts.len() > 2 {
            parts[2]
                .parse::<i32>()
                .map_err(|_| crate::error::Error::BadNote(token.to_string()))?
        } else {
            100
        };

        result.push(MrNote {
            pitch,
            start: t,
            duration: dur,
            velocity: vel,
        });
        t += dur;
    }

    write_stdout(&MrData::Pattern { notes: result })
}

// Bjorklund algorithm (same as in metaritual but standalone for CLI)
fn bjorklund(hits: usize, steps: usize) -> Vec<bool> {
    if steps == 0 {
        return vec![];
    }
    if hits >= steps {
        return vec![true; steps];
    }
    if hits == 0 {
        return vec![false; steps];
    }

    let mut groups: Vec<Vec<bool>> = (0..hits)
        .map(|_| vec![true])
        .chain((0..(steps - hits)).map(|_| vec![false]))
        .collect();

    let mut count = hits;
    loop {
        let remainder = groups.len() - count;
        if remainder <= 1 {
            break;
        }
        let take = count.min(remainder);
        let mut new_groups = Vec::new();
        for i in 0..take {
            let mut g = groups[i].clone();
            g.extend_from_slice(&groups[count + i]);
            new_groups.push(g);
        }
        for i in take..count {
            new_groups.push(groups[i].clone());
        }
        for i in (count + take)..groups.len() {
            new_groups.push(groups[i].clone());
        }
        count = take + (count - take).max(0);
        groups = new_groups;
    }

    groups.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bjorklund() {
        assert_eq!(
            bjorklund(3, 8),
            vec![true, false, false, true, false, false, true, false]
        );
        assert_eq!(
            bjorklund(5, 8),
            vec![true, false, true, false, true, false, true, true]
        );
        assert_eq!(bjorklund(0, 4), vec![false, false, false, false]);
        assert_eq!(bjorklund(4, 4), vec![true, true, true, true]);
        assert_eq!(bjorklund(1, 1), vec![true]);
    }

    #[test]
    fn test_parse_notes_notation() {
        // Test inline parsing logic directly
        let notation = "C4:2 E4:1 G4:1:80";
        let mut result = Vec::new();
        let mut t = 0.0;
        for token in notation.split_whitespace() {
            let parts: Vec<&str> = token.split(':').collect();
            let pitch = parse_pitch(parts[0]).unwrap();
            let dur: f64 = parts[1].parse().unwrap();
            let vel: i32 = if parts.len() > 2 {
                parts[2].parse().unwrap()
            } else {
                100
            };
            result.push((pitch, t, dur, vel));
            t += dur;
        }
        assert_eq!(result, vec![(60, 0.0, 2.0, 100), (64, 2.0, 1.0, 100), (67, 3.0, 1.0, 80)]);
    }

    #[test]
    fn test_scale_note_count() {
        // dorian has 7 notes per octave
        let root = 60i32;
        let intervals = metaritual::construct::DORIAN;
        let octaves = 2;
        let mut count = 0;
        for oct in 0..octaves {
            for &interval in intervals {
                let pitch = root + (oct as i32 * 12) + interval as i32;
                if (0..=127).contains(&pitch) {
                    count += 1;
                }
            }
        }
        assert_eq!(count, 14); // 7 * 2
    }

    #[test]
    fn test_euclidean_hit_count() {
        let bools = bjorklund(5, 16);
        assert_eq!(bools.iter().filter(|&&b| b).count(), 5);
        assert_eq!(bools.len(), 16);
    }
}
