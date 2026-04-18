use crate::error::{Error, Result};
use metaritual::construct;

/// Parse a note name like "C4", "D#3", "Bb5", or a raw MIDI number like "60".
pub fn parse_pitch(s: &str) -> Result<i32> {
    // Try raw number first
    if let Ok(n) = s.parse::<i32>() {
        return Ok(n);
    }

    let s = s.trim();
    if s.is_empty() {
        return Err(Error::BadNote(s.to_string()));
    }

    let bytes = s.as_bytes();
    let mut pos = 0;

    // Note letter
    let base = match bytes[pos].to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return Err(Error::BadNote(s.to_string())),
    };
    pos += 1;

    // Accidentals
    let mut offset = 0i32;
    while pos < bytes.len() {
        match bytes[pos] {
            b'#' => {
                offset += 1;
                pos += 1;
            }
            b'b' => {
                offset -= 1;
                pos += 1;
            }
            _ => break,
        }
    }

    // Octave number
    let octave_str = &s[pos..];
    let octave: i32 = octave_str
        .parse()
        .map_err(|_| Error::BadNote(s.to_string()))?;

    // MIDI: C4 = 60, so C(-1) = 0
    let midi = (octave + 1) * 12 + base + offset;
    if !(0..=127).contains(&midi) {
        return Err(Error::BadNote(format!("{} (MIDI {} out of range)", s, midi)));
    }
    Ok(midi)
}

/// Parse a scale mode name to its interval set.
pub fn parse_scale(mode: &str) -> Result<&'static [u8]> {
    match mode.to_lowercase().as_str() {
        "major" | "maj" | "ionian" => Ok(construct::MAJOR),
        "minor" | "min" | "aeolian" => Ok(construct::MINOR),
        "dorian" | "dor" => Ok(construct::DORIAN),
        "phrygian" | "phryg" => Ok(construct::PHRYGIAN),
        "lydian" | "lyd" => Ok(construct::LYDIAN),
        "mixolydian" | "mixo" => Ok(construct::MIXOLYDIAN),
        "pentatonic" | "pent" => Ok(construct::PENTATONIC),
        "minor_pentatonic" | "min_pent" => Ok(construct::MINOR_PENTATONIC),
        "blues" => Ok(construct::BLUES),
        "whole_tone" | "wholetone" => Ok(construct::WHOLE_TONE),
        "chromatic" | "chrom" => Ok(construct::CHROMATIC),
        _ => Err(Error::BadScale(mode.to_string())),
    }
}

/// Parse a chord quality name to its interval set.
pub fn parse_chord_quality(q: &str) -> Result<&'static [u8]> {
    match q.to_lowercase().as_str() {
        // Triads
        "maj" | "major" => Ok(construct::MAJOR_TRIAD),
        "min" | "minor" | "m" => Ok(construct::MINOR_TRIAD),
        "dim" | "diminished" => Ok(construct::DIM),
        "aug" | "augmented" => Ok(construct::AUG),
        "sus2" => Ok(construct::SUS2),
        "sus4" => Ok(construct::SUS4),
        "5" => Ok(construct::POWER5),
        // Seventh chords
        "maj7" | "major7" => Ok(construct::MAJOR_7),
        "m7" | "min7" | "minor7" => Ok(construct::MINOR_7),
        "dom7" | "7" => Ok(construct::DOM_7),
        // Sixth chords
        "6" | "maj6" | "major6" => Ok(construct::MAJ6),
        "m6" | "min6" | "minor6" => Ok(construct::MIN6),
        // Ninth chords
        "maj9" | "major9" => Ok(construct::MAJ9),
        "m9" | "min9" | "minor9" => Ok(construct::MIN9),
        "dom9" | "9" => Ok(construct::DOM9),
        // Eleventh chords
        "maj11" | "major11" => Ok(construct::MAJ11),
        "m11" | "min11" | "minor11" => Ok(construct::MIN11),
        "dom11" | "11" => Ok(construct::DOM11),
        // Thirteenth chords
        "maj13" | "major13" => Ok(construct::MAJ13),
        "m13" | "min13" | "minor13" => Ok(construct::MIN13),
        "dom13" | "13" => Ok(construct::DOM13),
        // Add chords
        "add9" => Ok(construct::ADD9),
        "add11" => Ok(construct::ADD11),
        // Altered dominants
        "7sharp9" | "7#9" => Ok(construct::DOM7_SHARP9),
        "7flat9" | "7b9" => Ok(construct::DOM7_FLAT9),
        "7sharp11" | "7#11" => Ok(construct::DOM7_SHARP11),
        _ => Err(Error::BadChord(q.to_string())),
    }
}

/// Parse a chord spec like "Cmaj7", "F#m7", "Bbdim" into (root_midi, intervals).
pub fn parse_chord_spec(s: &str) -> Result<(i32, &'static [u8])> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::BadChord(s.to_string()));
    }

    // Find where the note name ends and quality begins.
    // Note name: letter + optional accidentals, NO octave number for chord specs.
    let bytes = s.as_bytes();
    let mut pos = 0;

    // Note letter
    let base = match bytes[pos].to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return Err(Error::BadChord(s.to_string())),
    };
    pos += 1;

    let mut offset = 0i32;
    while pos < bytes.len() {
        match bytes[pos] {
            b'#' => {
                offset += 1;
                pos += 1;
            }
            b'b' => {
                offset -= 1;
                pos += 1;
            }
            _ => break,
        }
    }

    let root_pc = ((base + offset) % 12 + 12) % 12;
    // Default octave 4 for chord roots → MIDI = (4+1)*12 + root_pc
    let root_midi = 60 + root_pc;

    let quality_str = &s[pos..];
    let intervals = if quality_str.is_empty() {
        construct::MAJOR_TRIAD
    } else {
        parse_chord_quality(quality_str)?
    };

    Ok((root_midi, intervals))
}

/// Format a MIDI pitch as a note name.
pub fn pitch_name(midi: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
    ];
    let octave = (midi / 12) - 1;
    let pc = (midi % 12) as usize;
    format!("{}{}", NAMES[pc], octave)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pitch() {
        assert_eq!(parse_pitch("C4").unwrap(), 60);
        assert_eq!(parse_pitch("A4").unwrap(), 69);
        assert_eq!(parse_pitch("C#4").unwrap(), 61);
        assert_eq!(parse_pitch("Db4").unwrap(), 61);
        assert_eq!(parse_pitch("C5").unwrap(), 72);
        assert_eq!(parse_pitch("C3").unwrap(), 48);
        assert_eq!(parse_pitch("60").unwrap(), 60);
        assert_eq!(parse_pitch("B3").unwrap(), 59);
    }

    #[test]
    fn test_parse_scale() {
        assert_eq!(parse_scale("dorian").unwrap(), construct::DORIAN);
        assert_eq!(parse_scale("pent").unwrap(), construct::PENTATONIC);
        assert!(parse_scale("nonexistent").is_err());
    }

    #[test]
    fn test_parse_chord_spec() {
        let (root, intervals) = parse_chord_spec("Cmaj7").unwrap();
        assert_eq!(root, 60);
        assert_eq!(intervals, construct::MAJOR_7);

        let (root, intervals) = parse_chord_spec("F#m7").unwrap();
        assert_eq!(root, 66);
        assert_eq!(intervals, construct::MINOR_7);
    }

    #[test]
    fn test_pitch_name() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(69), "A4");
        assert_eq!(pitch_name(61), "C#4");
    }

    #[test]
    fn test_parse_chord_quality_triads() {
        assert_eq!(parse_chord_quality("maj").unwrap(), construct::MAJOR_TRIAD);
        assert_eq!(parse_chord_quality("major").unwrap(), construct::MAJOR_TRIAD);
        assert_eq!(parse_chord_quality("min").unwrap(), construct::MINOR_TRIAD);
        assert_eq!(parse_chord_quality("minor").unwrap(), construct::MINOR_TRIAD);
        assert_eq!(parse_chord_quality("m").unwrap(), construct::MINOR_TRIAD);
        assert_eq!(parse_chord_quality("dim").unwrap(), construct::DIM);
        assert_eq!(parse_chord_quality("aug").unwrap(), construct::AUG);
        assert_eq!(parse_chord_quality("sus2").unwrap(), construct::SUS2);
        assert_eq!(parse_chord_quality("sus4").unwrap(), construct::SUS4);
        assert_eq!(parse_chord_quality("5").unwrap(), construct::POWER5);
    }

    #[test]
    fn test_parse_chord_quality_sevenths() {
        assert_eq!(parse_chord_quality("maj7").unwrap(), construct::MAJOR_7);
        assert_eq!(parse_chord_quality("m7").unwrap(), construct::MINOR_7);
        assert_eq!(parse_chord_quality("dom7").unwrap(), construct::DOM_7);
        assert_eq!(parse_chord_quality("7").unwrap(), construct::DOM_7);
    }

    #[test]
    fn test_parse_chord_quality_extended() {
        assert_eq!(parse_chord_quality("maj9").unwrap(), construct::MAJ9);
        assert_eq!(parse_chord_quality("min9").unwrap(), construct::MIN9);
        assert_eq!(parse_chord_quality("dom9").unwrap(), construct::DOM9);
        assert_eq!(parse_chord_quality("9").unwrap(), construct::DOM9);
        assert_eq!(parse_chord_quality("maj11").unwrap(), construct::MAJ11);
        assert_eq!(parse_chord_quality("11").unwrap(), construct::DOM11);
        assert_eq!(parse_chord_quality("maj13").unwrap(), construct::MAJ13);
        assert_eq!(parse_chord_quality("13").unwrap(), construct::DOM13);
    }

    #[test]
    fn test_parse_chord_quality_altered() {
        assert_eq!(parse_chord_quality("7#9").unwrap(), construct::DOM7_SHARP9);
        assert_eq!(parse_chord_quality("7b9").unwrap(), construct::DOM7_FLAT9);
        assert_eq!(parse_chord_quality("7#11").unwrap(), construct::DOM7_SHARP11);
    }

    #[test]
    fn test_parse_chord_quality_invalid() {
        assert!(parse_chord_quality("xyz").is_err());
        assert!(parse_chord_quality("").is_err());
    }

    #[test]
    fn test_parse_chord_quality_case_insensitive() {
        assert_eq!(parse_chord_quality("MAJ").unwrap(), construct::MAJOR_TRIAD);
        assert_eq!(parse_chord_quality("Min7").unwrap(), construct::MINOR_7);
        assert_eq!(parse_chord_quality("DOM7").unwrap(), construct::DOM_7);
    }

    #[test]
    fn test_parse_pitch_edge_cases() {
        assert!(parse_pitch("").is_err());
        assert!(parse_pitch("X4").is_err());
        // Very high/low should error
        assert!(parse_pitch("C11").is_err()); // MIDI > 127
    }

    #[test]
    fn test_parse_chord_spec_no_quality() {
        // No quality = major triad
        let (root, intervals) = parse_chord_spec("C").unwrap();
        assert_eq!(root, 60);
        assert_eq!(intervals, construct::MAJOR_TRIAD);
    }

    #[test]
    fn test_pitch_name_roundtrip() {
        for midi in 24..=96 {
            let name = pitch_name(midi);
            let parsed = parse_pitch(&name).unwrap();
            assert_eq!(parsed, midi, "roundtrip failed for MIDI {}: name={}", midi, name);
        }
    }
}
