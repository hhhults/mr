use crate::error::{Error, Result};
use crate::json::{MrData, MrNote, write_stdout};

// GM drum mapping
const KICK: i32 = 36;
const RIM: i32 = 37;
const SNARE: i32 = 38;
const CLAP: i32 = 39;
const CH: i32 = 42;  // closed hat
const OH: i32 = 46;  // open hat
const TOM_LO: i32 = 45;
const TOM_HI: i32 = 50;
const RIDE: i32 = 51;
const COWBELL: i32 = 56;
const SHAKER: i32 = 69;

struct Genre {
    name: &'static str,
    aliases: &'static [&'static str],
    bpm_sweet: f32,
    bpm_range: (f32, f32),
    swing: f32,
    info: &'static str,
    build: fn() -> Vec<MrNote>,
}

fn step(n: usize) -> f64 {
    (n - 1) as f64 * 0.25
}

fn hit(pitch: i32, s: usize, vel: i32) -> MrNote {
    MrNote { pitch, start: step(s), duration: 0.25, velocity: vel ,
                ..Default::default()}
}

fn hits(pitch: i32, steps: &[usize], vel: i32) -> Vec<MrNote> {
    steps.iter().map(|&s| hit(pitch, s, vel)).collect()
}

fn shaker_16(vel_on: i32, vel_off: i32) -> Vec<MrNote> {
    (1..=16).map(|s| hit(SHAKER, s, if s % 2 == 1 { vel_on } else { vel_off })).collect()
}

// ── Genre drum pattern builders ──────────────────────────

fn chicago_house() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 100));
    n.extend(hits(CLAP, &[5,13], 100));
    n.extend(hits(CH, &[1,3,5,7,9,11,13,15], 80));
    n.extend(hits(OH, &[7,15], 90));
    n.extend(hits(SHAKER, &[4,8,12,16], 70));
    n
}

fn acid_house() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 100));
    n.extend(hits(CLAP, &[5,13], 90));
    n.extend(hits(CH, &[1,3,5,7,9,11,13,15], 70));
    n.extend(hits(OH, &[7,15], 80));
    n
}

fn deep_house() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 95));
    n.extend(hits(CLAP, &[5,13], 80));
    n.extend(hits(RIDE, &[1,3,5,7,9,11,13,15], 75));
    n.extend(shaker_16(55, 35));
    n
}

fn detroit_techno() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 100));
    n.extend(hits(SNARE, &[5,13], 100));
    n.extend(hits(CH, &[1,3,5,7,9,11,13,15], 70));
    n.extend(hits(RIDE, &(1..=16).collect::<Vec<_>>(), 60));
    n.extend(hits(CLAP, &[5,13], 90));
    n.extend(hits(CLAP, &[8,16], 40)); // ghost claps
    n
}

fn minimal_techno() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 100));
    n.extend(hits(CH, &[3,7,11,15], 65));
    n.extend(hits(CLAP, &[5,13], 60));
    n
}

fn electro() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,7,11], 100)); // broken kick
    n.extend(hits(SNARE, &[5,13], 100));
    n.extend(hits(COWBELL, &[1,3,5,7,9,11,13,15], 80));
    n.extend(hits(CH, &[1,3,5,7,9,11,13,15], 65));
    n.extend(hits(TOM_HI, &[13,16], 90));
    n
}

fn twostep_garage() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,11], 100)); // skippy kick!
    n.extend(hits(CLAP, &[5,15], 100));
    n.extend(hits(CH, &[3,7,11,15], 80));
    n.extend(shaker_16(70, 45));
    n
}

fn dubstep() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.push(hit(KICK, 1, 100));
    n.push(hit(SNARE, 9, 100)); // snare on beat 3 = half-time
    n.extend(hits(CH, &[3,7,11,15], 70));
    n
}

fn jungle() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,7,9,15], 100));
    n.extend(hits(SNARE, &[5,13], 110));
    n.extend(hits(SNARE, &[2,4,8,11,14,16], 45)); // ghosts
    n.extend(hits(CH, &(1..=16).collect::<Vec<_>>(), 50));
    n
}

fn dembow() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 100));
    n.extend(hits(SNARE, &[4,7,12,15], 100)); // tresillo!
    n.extend(hits(CH, &[1,3,5,7,9,11,13,15], 70));
    n
}

fn jersey_club() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,2,4,5,7,8,10,11,13,14,16], 100)); // machine gun
    n.extend(hits(CLAP, &[5,13], 100));
    n
}

fn amapiano() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 90));
    n.extend(hits(RIM, &[5,13], 85));
    n.extend(hits(TOM_LO, &[1,4,7,11,14], 100)); // log drum
    n.extend(shaker_16(60, 35));
    n
}

fn trance() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 100));
    n.extend(hits(CLAP, &[5,13], 100));
    n.extend(hits(CH, &[3,7,11,15], 80)); // offbeat hats
    n.extend(hits(OH, &[7,15], 85));
    n
}

fn psytrance() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 100));
    n.extend(hits(CH, &(1..=16).collect::<Vec<_>>(), 55));
    n
}

fn footwork() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,4,7,10,12,14], 100)); // irregular
    n.extend(hits(SNARE, &[5,9,13], 90));
    n.extend(hits(CH, &[1,3,5,7,9,11,13,15], 60));
    n
}

fn uk_funky() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 95));
    n.extend(hits(CLAP, &[5,13], 90));
    n.extend(hits(CH, &[3,7,11,15], 75));
    n.extend(hits(TOM_HI, &[3,6,8,11,14,16], 70));
    n.extend(hits(TOM_LO, &[1,4,7,10,13,16], 65));
    n.extend(shaker_16(55, 35));
    n
}

fn ghetto_house() -> Vec<MrNote> {
    let mut n = Vec::new();
    n.extend(hits(KICK, &[1,5,9,13], 110));
    n.extend(hits(CLAP, &[5,13], 100));
    n.extend(hits(CH, &(1..=16).collect::<Vec<_>>(), 65));
    n.extend(hits(TOM_HI, &[3,7,11,15], 95));
    n.extend(hits(TOM_LO, &[4,8,12,16], 90));
    n
}

fn grime() -> Vec<MrNote> {
    let mut n = Vec::new();
    // Full-time, NOT half-time like dubstep
    n.extend(hits(KICK, &[1,4,9,11], 100));
    n.extend(hits(SNARE, &[5,13], 100));
    n.extend(hits(CH, &[1,3,5,7,9,11,13,15], 70));
    n.extend(hits(CLAP, &[5,13], 85));
    n
}

// ── Genre registry ───────────────────────────────────────

const GENRES: &[Genre] = &[
    Genre {
        name: "chicago-house",
        aliases: &["house", "chicago"],
        bpm_sweet: 121.0, bpm_range: (118.0, 125.0), swing: 0.52,
        info: "Four-on-the-floor, clap on 2/4, offbeat OH. Keys: Cm, Am, Fm. Juno pads, octave-jumping bass.",
        build: chicago_house,
    },
    Genre {
        name: "acid-house",
        aliases: &["acid"],
        bpm_sweet: 122.0, bpm_range: (118.0, 125.0), swing: 0.52,
        info: "Sparse drums to leave space for 303 filter sweeps. Saw osc, 18dB LP, reso 70-85%, env mod 50-80%.",
        build: acid_house,
    },
    Genre {
        name: "deep-house",
        aliases: &["deep"],
        bpm_sweet: 120.0, bpm_range: (110.0, 125.0), swing: 0.56,
        info: "Jazz harmony (7ths, 9ths, 11ths). Rhodes/Wurlitzer via FM. Prominent ride, shakers. Gradual layering.",
        build: deep_house,
    },
    Genre {
        name: "detroit-techno",
        aliases: &["techno", "detroit"],
        bpm_sweet: 130.0, bpm_range: (125.0, 135.0), swing: 0.50,
        info: "16th-note ride creates mechanical urgency. Parallel chords (fixed minor 7th shape transposed). DX7 bells.",
        build: detroit_techno,
    },
    Genre {
        name: "minimal-techno",
        aliases: &["minimal"],
        bpm_sweet: 125.0, bpm_range: (120.0, 130.0), swing: 0.50,
        info: "Extreme repetition, micro-variations. Nudge notes ±5-25ms. Sidechain everything to kick. One change per 8-32 bars.",
        build: minimal_techno,
    },
    Genre {
        name: "electro",
        aliases: &[],
        bpm_sweet: 120.0, bpm_range: (110.0, 130.0), swing: 0.50,
        info: "Broken syncopated kick (NOT four-on-floor). 808 cowbell on 8ths. No swing. Vocoder textures.",
        build: electro,
    },
    Genre {
        name: "2step-garage",
        aliases: &["2step", "garage", "ukg"],
        bpm_sweet: 130.0, bpm_range: (128.0, 132.0), swing: 0.58,
        info: "Kick SKIPS beats 2/4 — the skippy syncopation. Heavy 16th swing (54-62%). Sub bass with portamento.",
        build: twostep_garage,
    },
    Genre {
        name: "dubstep",
        aliases: &[],
        bpm_sweet: 140.0, bpm_range: (132.0, 142.0), swing: 0.50,
        info: "Half-time: kick on 1, snare on 3. Feels like 70 BPM. Sub sine 30-60Hz mono. Wobble = LFO→filter cutoff.",
        build: dubstep,
    },
    Genre {
        name: "jungle",
        aliases: &["dnb"],
        bpm_sweet: 170.0, bpm_range: (160.0, 175.0), swing: 0.50,
        info: "Double-time breaks over half-time sub (~85 feel). Amen chops, ghost snares. Reese bass: detuned saws ±7-15 cents.",
        build: jungle,
    },
    Genre {
        name: "dembow",
        aliases: &["reggaeton"],
        bpm_sweet: 92.0, bpm_range: (85.0, 100.0), swing: 0.50,
        info: "Tresillo (3+3+2): snares at steps 4,7,12,15. 808 sub with glide slides. The most infectious rhythm on earth.",
        build: dembow,
    },
    Genre {
        name: "jersey-club",
        aliases: &["jersey"],
        bpm_sweet: 135.0, bpm_range: (130.0, 140.0), swing: 0.50,
        info: "Machine-gun kicks: rapid doubles/triples/stutters. Bed squeak sample. Chopped R&B vocals.",
        build: jersey_club,
    },
    Genre {
        name: "amapiano",
        aliases: &[],
        bpm_sweet: 112.0, bpm_range: (108.0, 115.0), swing: 0.54,
        info: "Log drum = bass + rhythm accent. Jazzy piano chords. 16th shakers loud/soft. From South Africa.",
        build: amapiano,
    },
    Genre {
        name: "trance",
        aliases: &["uplifting-trance"],
        bpm_sweet: 138.0, bpm_range: (136.0, 142.0), swing: 0.50,
        info: "Offbeat hats (not every 8th). Supersaw: 7 saws, 15-30 cents detune. 32-bar buildups. Minor keys, Am-F-C-G.",
        build: trance,
    },
    Genre {
        name: "psytrance",
        aliases: &["psy"],
        bpm_sweet: 145.0, bpm_range: (135.0, 155.0), swing: 0.50,
        info: "Offbeat 16th bass: K-b-b-b pattern. Phrygian/Phrygian Dominant. Ping-pong delay (dotted 1/8).",
        build: psytrance,
    },
    Genre {
        name: "footwork",
        aliases: &[],
        bpm_sweet: 160.0, bpm_range: (155.0, 165.0), swing: 0.50,
        info: "Intentionally off-grid, tumbling feel. Triplets, half/double-time drops. Vocal sample repetition.",
        build: footwork,
    },
    Genre {
        name: "uk-funky",
        aliases: &["funky"],
        bpm_sweet: 130.0, bpm_range: (128.0, 132.0), swing: 0.56,
        info: "Syncopated with African/Caribbean percussion. Congas, djembes, talking drums. Dembow influence.",
        build: uk_funky,
    },
    Genre {
        name: "ghetto-house",
        aliases: &["ghetto"],
        bpm_sweet: 135.0, bpm_range: (130.0, 140.0), swing: 0.50,
        info: "Ultra-stripped: minimal drums, simple bass, heavy toms. 130-140+ BPM. Raw and direct.",
        build: ghetto_house,
    },
    Genre {
        name: "grime",
        aliases: &[],
        bpm_sweet: 140.0, bpm_range: (138.0, 142.0), swing: 0.50,
        info: "Full-time patterns (NOT half-time). Eski: detuned square waves with portamento. Sparse, icy, dry.",
        build: grime,
    },
];

fn find_genre(name: &str) -> Result<&'static Genre> {
    let lower = name.to_lowercase();
    for g in GENRES {
        if g.name == lower || g.aliases.iter().any(|&a| a == lower) {
            return Ok(g);
        }
    }
    Err(Error::Other(format!(
        "unknown genre: \"{}\"\navailable: {}",
        name,
        GENRES.iter().map(|g| g.name).collect::<Vec<_>>().join(", ")
    )))
}

/// Output the canonical drum pattern for a genre.
pub fn drums(name: &str, set_tempo: bool, show_info: bool) -> Result<()> {
    let genre = find_genre(name)?;

    if show_info || set_tempo {
        eprintln!("genre: {} ({:.0}–{:.0} BPM, sweet spot {:.0})",
            genre.name, genre.bpm_range.0, genre.bpm_range.1, genre.bpm_sweet);
        if genre.swing > 0.50 {
            eprintln!("swing: {:.0}%", genre.swing * 100.0);
        }
        eprintln!("{}", genre.info);
    }

    if set_tempo {
        crate::session::tempo(genre.bpm_sweet)?;
        eprintln!("tempo → {:.0}", genre.bpm_sweet);
    }

    let mut notes = (genre.build)();
    notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap().then(a.pitch.cmp(&b.pitch)));
    write_stdout(&MrData::Pattern { notes })
}

/// List all available genres.
pub fn list() -> Result<()> {
    for g in GENRES {
        let aliases = if g.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", g.aliases.join(", "))
        };
        eprintln!(
            "  {:18}{:6.0} BPM   {}",
            format!("{}{}", g.name, aliases),
            g.bpm_sweet,
            genre_family(g.name),
        );
    }
    Ok(())
}

fn genre_family(name: &str) -> &'static str {
    match name {
        "chicago-house" | "acid-house" | "deep-house" | "ghetto-house" => "four-on-the-floor",
        "detroit-techno" | "minimal-techno" => "four-on-the-floor",
        "electro" | "grime" => "broken kick",
        "2step-garage" => "skipped kick",
        "dubstep" => "half-time",
        "jungle" => "double-time breaks",
        "dembow" => "tresillo (3+3+2)",
        "jersey-club" => "stutter kicks",
        "amapiano" | "uk-funky" => "four-on-the-floor + percussion",
        "trance" | "psytrance" => "four-on-the-floor + offbeat",
        "footwork" => "off-grid",
        _ => "",
    }
}

// ── Named rhythm figures (single-voice, composable) ──────

/// Generate a named rhythmic figure on a single pitch.
pub fn rhythm(name: &str, pitch: i32, velocity: i32, duration: f64) -> Result<()> {
    let steps: Vec<usize> = match name {
        "four-on-the-floor" | "4otf" | "4x4" => vec![1, 5, 9, 13],
        "backbeat" => vec![5, 13],  // clap/snare on 2 and 4
        "offbeat-8th" | "offbeat" => vec![3, 7, 11, 15],  // offbeat 8ths
        "8th-notes" | "8ths" => vec![1, 3, 5, 7, 9, 11, 13, 15],
        "16th-notes" | "16ths" => (1..=16).collect(),
        "dembow" | "tresillo" => vec![4, 7, 12, 15],  // 3+3+2 snare
        "2step" | "skippy" => vec![1, 11],  // garage skippy kick
        "halftime" => vec![1, 9],  // kick-1 snare-3
        "breakbeat" | "break" => vec![1, 7, 9, 15],  // jungle-style broken kick
        "stutter" | "jersey" => vec![1, 2, 4, 5, 7, 8, 10, 11, 13, 14, 16],  // machine gun
        "clave-32" | "son-clave" => vec![1, 4, 7, 11, 13],  // 3-2 son clave
        "clave-23" => vec![1, 3, 7, 9, 13],  // 2-3 rumba clave
        _ => {
            return Err(Error::Other(format!(
                "unknown rhythm: \"{}\"\navailable: four-on-the-floor, backbeat, offbeat, 8ths, 16ths, \
                dembow, 2step, halftime, breakbeat, stutter, clave-32, clave-23",
                name
            )));
        }
    };

    let notes: Vec<MrNote> = steps
        .iter()
        .map(|&s| MrNote {
            pitch,
            start: (s - 1) as f64 * duration,
            duration,
            velocity,
            ..Default::default()
        })
        .collect();

    write_stdout(&MrData::Pattern { notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_genre_by_name() {
        assert!(find_genre("chicago-house").is_ok());
        assert!(find_genre("detroit-techno").is_ok());
        assert!(find_genre("minimal-techno").is_ok());
    }

    #[test]
    fn test_find_genre_by_alias() {
        assert!(find_genre("house").is_ok());
        assert!(find_genre("techno").is_ok());
    }

    #[test]
    fn test_find_genre_unknown() {
        assert!(find_genre("nonexistent_genre").is_err());
    }

    #[test]
    fn test_all_genres_produce_notes() {
        for g in GENRES {
            let notes = (g.build)();
            assert!(!notes.is_empty(), "genre {} produced no notes", g.name);
            for n in &notes {
                assert!(n.pitch >= 0 && n.pitch <= 127, "bad pitch {} in {}", n.pitch, g.name);
                assert!(n.start >= 0.0, "negative start in {}", g.name);
                assert!(n.velocity >= 1 && n.velocity <= 127, "bad velocity in {}", g.name);
            }
        }
    }

    #[test]
    fn test_genre_bpm_ranges_valid() {
        for g in GENRES {
            assert!(g.bpm_range.0 <= g.bpm_sweet, "{} min > sweet", g.name);
            assert!(g.bpm_sweet <= g.bpm_range.1, "{} sweet > max", g.name);
            assert!(g.bpm_range.0 > 0.0, "{} bpm_min should be > 0", g.name);
        }
    }

    #[test]
    fn test_genre_swing_range() {
        for g in GENRES {
            assert!(g.swing >= 0.0 && g.swing <= 1.0,
                "genre {} swing {} out of [0,1]", g.name, g.swing);
        }
    }

    #[test]
    fn test_rhythm_known_names_produce_steps() {
        let known = vec![
            ("four-on-the-floor", 4), ("backbeat", 2), ("offbeat", 4),
            ("8ths", 8), ("16ths", 16), ("dembow", 4), ("2step", 2),
            ("halftime", 2), ("breakbeat", 4), ("stutter", 11),
            ("clave-32", 5), ("clave-23", 5),
        ];
        for (name, expected_len) in &known {
            let steps: Vec<usize> = match *name {
                "four-on-the-floor" | "4otf" | "4x4" => vec![1, 5, 9, 13],
                "backbeat" => vec![5, 13],
                "offbeat-8th" | "offbeat" => vec![3, 7, 11, 15],
                "8th-notes" | "8ths" => vec![1, 3, 5, 7, 9, 11, 13, 15],
                "16th-notes" | "16ths" => (1..=16).collect(),
                "dembow" | "tresillo" => vec![4, 7, 12, 15],
                "2step" | "skippy" => vec![1, 11],
                "halftime" => vec![1, 9],
                "breakbeat" | "break" => vec![1, 7, 9, 15],
                "stutter" | "jersey" => vec![1, 2, 4, 5, 7, 8, 10, 11, 13, 14, 16],
                "clave-32" | "son-clave" => vec![1, 4, 7, 11, 13],
                "clave-23" => vec![1, 3, 7, 9, 13],
                _ => panic!("unmatched: {}", name),
            };
            assert_eq!(steps.len(), *expected_len, "wrong count for {}", name);
        }
    }

    #[test]
    fn test_genre_family() {
        assert_eq!(genre_family("chicago-house"), "four-on-the-floor");
        assert_eq!(genre_family("electro"), "broken kick");
        assert_eq!(genre_family("jungle"), "double-time breaks");
        assert_eq!(genre_family("dubstep"), "half-time");
        assert_eq!(genre_family("nonexistent"), "");
    }

    #[test]
    fn test_step_and_hit_helpers() {
        assert_eq!(step(1), 0.0);
        assert_eq!(step(5), 1.0);
        assert_eq!(step(16), 3.75);

        let n = hit(KICK, 1, 100);
        assert_eq!(n.pitch, KICK);
        assert!((n.start - 0.0).abs() < 1e-10);
        assert_eq!(n.velocity, 100);
    }

    #[test]
    fn test_hits_helper() {
        let notes = hits(SNARE, &[5, 13], 90);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].pitch, SNARE);
        assert!((notes[0].start - 1.0).abs() < 1e-10); // step(5) = 1.0
        assert!((notes[1].start - 3.0).abs() < 1e-10); // step(13) = 3.0
    }

    #[test]
    fn test_shaker_16_alternating_velocity() {
        let notes = shaker_16(80, 50);
        assert_eq!(notes.len(), 16);
        assert_eq!(notes[0].velocity, 80); // step 1 = odd
        assert_eq!(notes[1].velocity, 50); // step 2 = even
    }
}
