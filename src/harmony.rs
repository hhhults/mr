use metaritual::harmony::{self, VoiceLeadingGraph};

use crate::error::{Error, Result};
use crate::json::{MrChord, MrData, write_stdout};

fn parse_chord_arg(spec: &str) -> Result<(u8, &'static str)> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(Error::BadChord(spec.to_string()));
    }

    let bytes = spec.as_bytes();
    let mut pos = 0;

    // Note letter
    let base: u8 = match bytes[pos].to_ascii_uppercase() {
        b'C' => 0, b'D' => 2, b'E' => 4, b'F' => 5,
        b'G' => 7, b'A' => 9, b'B' => 11,
        _ => return Err(Error::BadChord(spec.to_string())),
    };
    pos += 1;

    let mut offset = 0i8;
    while pos < bytes.len() {
        match bytes[pos] {
            b'#' => { offset += 1; pos += 1; }
            b'b' => { offset -= 1; pos += 1; }
            _ => break,
        }
    }
    let root = ((base as i8 + offset).rem_euclid(12)) as u8;
    let quality_str = &spec[pos..];

    // Map quality to graph type names
    let type_name: &'static str = match quality_str.to_lowercase().as_str() {
        "" | "maj" | "major" => "major",
        "m" | "min" | "minor" => "minor",
        "aug" | "augmented" => "augmented",
        "dim" | "diminished" => "diminished",
        "sus2" => "sus2",
        "sus4" => "sus4",
        "5" => "power5",
        "maj7" | "major7" => "maj7",
        "m7" | "min7" | "minor7" => "min7",
        "7" | "dom7" => "dom7",
        "mm7" | "minmaj7" => "minmaj7",
        "hdim7" | "ø7" => "hdim7",
        "dim7" | "°7" => "dim7",
        "augmaj7" | "augm7" => "augmaj7",
        "aug7" | "augdom7" => "augdom7",
        "6" | "maj6" | "major6" => "maj6",
        "m6" | "min6" | "minor6" => "min6",
        "maj9" | "major9" => "maj9",
        "m9" | "min9" | "minor9" => "min9",
        "9" | "dom9" => "dom9",
        "maj11" | "major11" => "maj11",
        "m11" | "min11" | "minor11" => "min11",
        "11" | "dom11" => "dom11",
        "maj13" | "major13" => "maj13",
        "m13" | "min13" | "minor13" => "min13",
        "13" | "dom13" => "dom13",
        "add9" => "add9",
        "add11" => "add11",
        "7sharp9" | "7#9" => "7#9",
        "7flat9" | "7b9" => "7b9",
        "7sharp11" | "7#11" => "7#11",
        _ => return Err(Error::BadChord(spec.to_string())),
    };

    Ok((root, type_name))
}

fn is_triad_type(type_name: &str) -> bool {
    matches!(type_name, "major" | "minor" | "augmented" | "diminished" | "sus2" | "sus4")
}

fn chord_to_mr(c: &harmony::Chord) -> MrChord {
    MrChord {
        root: c.root_name().to_string(),
        quality: c.type_name.to_string(),
        root_midi: 60 + c.root as i32,
    }
}

fn emit_progression(chords: &[harmony::Chord]) -> Result<()> {
    let mr_chords: Vec<MrChord> = chords.iter().map(chord_to_mr).collect();
    // Also print human-readable to stderr
    let labels: Vec<String> = chords.iter().map(|c| c.label()).collect();
    eprintln!("{}", labels.join(" → "));
    write_stdout(&MrData::Progression { chords: mr_chords })
}

pub fn walk(chord_spec: &str, steps: usize, dist: u8, avoid_revisit: bool, seed: u64) -> Result<()> {
    let (root, type_name) = parse_chord_arg(chord_spec)?;
    let triads = is_triad_type(type_name);
    let g = VoiceLeadingGraph::new(triads, 2);
    let start = g.find(root, type_name)
        .ok_or_else(|| Error::BadChord(chord_spec.to_string()))?;
    let path = g.random_walk(start, steps, dist, avoid_revisit, seed);
    emit_progression(&path)
}

pub fn path(start_spec: &str, end_spec: &str, smoothest: bool) -> Result<()> {
    let (sr, st) = parse_chord_arg(start_spec)?;
    let (er, et) = parse_chord_arg(end_spec)?;

    // Both must be same cardinality
    let triads_s = is_triad_type(st);
    let triads_e = is_triad_type(et);
    if triads_s != triads_e {
        return Err(Error::Other(
            "start and end chords must be same cardinality (both triads or both sevenths)".into(),
        ));
    }

    let g = VoiceLeadingGraph::new(triads_s, 2);
    let start = g.find(sr, st).ok_or_else(|| Error::BadChord(start_spec.to_string()))?;
    let end = g.find(er, et).ok_or_else(|| Error::BadChord(end_spec.to_string()))?;

    let result = if smoothest {
        g.smoothest_path(start, end)
    } else {
        g.shortest_path(start, end)
    };

    match result {
        Some(path) => emit_progression(&path),
        None => Err(Error::Other(format!("no path from {} to {}", start_spec, end_spec))),
    }
}

pub fn orbit(chord_spec: &str, ops_str: &str, max_iter: usize) -> Result<()> {
    let (root, type_name) = parse_chord_arg(chord_spec)?;
    if !is_triad_type(type_name) {
        return Err(Error::Other("orbit with PLR operations only works for triads".into()));
    }
    let g = VoiceLeadingGraph::new(true, 2);
    let start = g.find(root, type_name)
        .ok_or_else(|| Error::BadChord(chord_spec.to_string()))?;

    let ops: Vec<char> = ops_str
        .split(',')
        .filter_map(|s| s.trim().chars().next())
        .collect();

    let path = g.orbit(start, &ops, max_iter);
    emit_progression(&path)
}

pub fn neighbors(chord_spec: &str, dist: u8) -> Result<()> {
    let (root, type_name) = parse_chord_arg(chord_spec)?;
    let triads = is_triad_type(type_name);
    let g = VoiceLeadingGraph::new(triads, 2);
    let chord = g.find(root, type_name)
        .ok_or_else(|| Error::BadChord(chord_spec.to_string()))?;

    let nbrs = g.neighbors(chord, Some(dist));
    println!("Neighbors of {} (dist ≤ {}):", chord.label(), dist);
    for (c, d) in &nbrs {
        println!("  {} (dist={})", c.label(), d);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chord_arg() {
        let (root, ty) = parse_chord_arg("Cmaj7").unwrap();
        assert_eq!(root, 0);
        assert_eq!(ty, "maj7");

        let (root, ty) = parse_chord_arg("F#m7").unwrap();
        assert_eq!(root, 6);
        assert_eq!(ty, "min7");

        let (root, ty) = parse_chord_arg("Bbdim7").unwrap();
        assert_eq!(root, 10);
        assert_eq!(ty, "dim7");

        let (root, ty) = parse_chord_arg("Cmajor").unwrap();
        assert_eq!(root, 0);
        assert_eq!(ty, "major");
    }

    #[test]
    fn test_is_triad() {
        assert!(is_triad_type("major"));
        assert!(is_triad_type("minor"));
        assert!(!is_triad_type("maj7"));
        assert!(!is_triad_type("dom7"));
    }

    #[test]
    fn test_parse_chord_arg_all_roots() {
        for (name, expected) in &[
            ("C", 0), ("D", 2), ("E", 4), ("F", 5), ("G", 7), ("A", 9), ("B", 11),
        ] {
            let (root, _) = parse_chord_arg(name).unwrap();
            assert_eq!(root, *expected, "failed for {}", name);
        }
    }

    #[test]
    fn test_parse_chord_arg_accidentals() {
        let (root, _) = parse_chord_arg("C#").unwrap();
        assert_eq!(root, 1);
        let (root, _) = parse_chord_arg("Db").unwrap();
        assert_eq!(root, 1);
        let (root, _) = parse_chord_arg("F##").unwrap(); // double sharp
        assert_eq!(root, 7); // F## = G
    }

    #[test]
    fn test_parse_chord_arg_bad() {
        assert!(parse_chord_arg("").is_err());
        assert!(parse_chord_arg("X").is_err());
        assert!(parse_chord_arg("Cxyz").is_err());
    }

    #[test]
    fn test_voice_leading_graph_neighbors() {
        let g = VoiceLeadingGraph::new(true, 2);
        let c_major = g.find(0, "major").expect("C major should exist");
        let nbrs = g.neighbors(c_major, Some(2));
        assert!(!nbrs.is_empty(), "C major should have neighbors");
        // All neighbors should be within distance 2
        for (_, d) in &nbrs {
            assert!(*d <= 2);
        }
    }

    #[test]
    fn test_voice_leading_graph_path() {
        let g = VoiceLeadingGraph::new(true, 2);
        let c_major = g.find(0, "major").unwrap();
        let a_minor = g.find(9, "minor").unwrap();
        let path = g.shortest_path(c_major, a_minor);
        assert!(path.is_some(), "should find path from C to Am");
        let path = path.unwrap();
        assert!(path.len() >= 2, "path should have at least start and end");
        assert_eq!(path.first().unwrap().root, 0);
        assert_eq!(path.last().unwrap().root, 9);
    }

    #[test]
    fn test_voice_leading_graph_walk() {
        let g = VoiceLeadingGraph::new(true, 2);
        let c_major = g.find(0, "major").unwrap();
        let walked = g.random_walk(c_major, 5, 2, false, 42);
        // random_walk returns steps + start chord
        assert!(walked.len() >= 5, "walk should return at least 5 chords, got {}", walked.len());
    }

    #[test]
    fn test_voice_leading_graph_orbit() {
        let g = VoiceLeadingGraph::new(true, 2);
        let c_major = g.find(0, "major").unwrap();
        let orbit = g.orbit(c_major, &['P', 'L', 'R'], 20);
        assert!(!orbit.is_empty());
        // Orbit should eventually return to start (or hit max_iter)
        assert!(orbit.len() <= 20);
    }

    #[test]
    fn test_chord_to_mr_format() {
        let g = VoiceLeadingGraph::new(true, 2);
        let c_major = g.find(0, "major").unwrap();
        let mr = chord_to_mr(c_major);
        assert_eq!(mr.quality, "major");
        assert_eq!(mr.root_midi, 60);
    }
}
