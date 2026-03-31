mod bridge;
mod connect;
mod curve;
mod curve_transform;
mod daemon;
mod device;
mod error;
mod generate;
mod generative;
mod genre;
mod harmony;
mod inspect;
mod json;
mod loom;
mod mix;
mod resolve;
mod scene;
mod session;
mod sink;
mod snapshot;
mod spectrum;
mod stash;
mod track;
mod transform;
mod voice;
mod walk;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mr", about = "Live-coding CLI for Ableton Live")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    // ── Session ──────────────────────────────────────────────
    /// Set tempo in BPM
    Tempo { bpm: f32 },
    /// Start playback
    Play,
    /// Stop playback
    Stop,
    /// Undo last action
    Undo,
    /// Redo last undone action
    Redo,

    // ── Tracks ───────────────────────────────────────────────
    /// Create a new track
    Track {
        name: String,
        #[arg(long)]
        simpler: Option<String>,
        #[arg(long)]
        instrument: Option<String>,
        #[arg(long)]
        audio: bool,
    },
    /// Delete a track by name
    DeleteTrack { name: String },
    /// Load an audio effect onto a track
    Effect { track: String, name: String },
    /// List all tracks
    Tracks,

    // ── Mixing ───────────────────────────────────────────────
    /// Set track mix parameters
    Mix {
        track: String,
        #[arg(long)]
        vol: Option<f32>,
        #[arg(long)]
        pan: Option<f32>,
        #[arg(long)]
        mute: bool,
        #[arg(long)]
        solo: bool,
    },
    /// Set send level to a return track
    Send {
        track: String,
        #[arg(name = "return")]
        return_track: String,
        level: f32,
    },

    // ── Device ───────────────────────────────────────────────
    /// Set device parameters (key:value pairs)
    Device {
        track: String,
        #[arg(long, default_value_t = 0)]
        device: i32,
        params: Vec<String>,
    },

    // ── Inspection ───────────────────────────────────────────
    /// Show session status
    Status,
    /// Inspect a track or clip
    Inspect { target: String },
    /// Show device parameters
    Params {
        track: String,
        device_idx: Option<i32>,
    },
    /// List return tracks
    Returns,
    /// Read clip notes as pattern JSON (stdout)
    Read { target: String },
    /// Show output meter levels for all tracks
    Meters,
    /// Show real-time frequency spectrum (requires BlackHole)
    Spectrum {
        /// Output raw JSON instead of visual bars
        #[arg(long)]
        json: bool,
        /// Continuously update the display
        #[arg(long)]
        continuous: bool,
        /// Audio input device name (default: BlackHole)
        #[arg(long)]
        device: Option<String>,
    },

    // ── Pattern Generators (stdout) ─────────────────────────
    /// Generate a scale pattern
    Scale {
        /// Root note (e.g. "C4", "D#3")
        root: String,
        /// Scale mode (major, dorian, pentatonic, etc.)
        mode: String,
        #[arg(long, default_value_t = 1)]
        octaves: usize,
        #[arg(long, default_value_t = 1.0)]
        duration: f64,
        #[arg(long, default_value_t = 100)]
        velocity: i32,
    },
    /// Generate a euclidean rhythm pattern
    Euclidean {
        hits: usize,
        steps: usize,
        #[arg(long, default_value_t = 60)]
        pitch: i32,
        #[arg(long, default_value_t = 0.25)]
        duration: f64,
        #[arg(long, default_value_t = 0)]
        rotation: usize,
        #[arg(long, default_value_t = 100)]
        velocity: i32,
    },
    /// Generate a chord (simultaneous notes)
    Chord {
        /// Chord spec (e.g. "Cmaj7", "F#m7", "Bbdim")
        spec: String,
        #[arg(long, default_value_t = 4.0)]
        duration: f64,
        #[arg(long, default_value_t = 100)]
        velocity: i32,
    },
    /// Generate notes from inline notation ("C4:2 E4:1 G4:1:80")
    Notes {
        /// pitch:duration[:velocity] separated by spaces
        notation: String,
    },

    // ── Pattern Transforms (stdin → stdout) ─────────────────
    /// Transpose all pitches by semitones
    Transpose { semitones: i32 },
    /// Rotate note pitches/velocities (keep timing)
    Rotate { n: i32 },
    /// Reverse note order (keep timing)
    Retrograde,
    /// Invert pitches around an axis
    Invert {
        #[arg(long)]
        axis: Option<i32>,
    },
    /// Scale timing by a factor
    Stretch { factor: f64 },
    /// Randomly remove notes
    Degrade {
        probability: f64,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Keep every Nth note
    Thin { n: usize },
    /// Add random timing/velocity variation
    Humanize {
        #[arg(long, default_value_t = 0.02)]
        timing: f64,
        #[arg(long, default_value_t = 5.0)]
        velocity: f64,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Push offbeat notes later
    Swing { amount: f64 },
    /// Extend note durations to overlap
    Overlap { amount: f64 },
    /// Apply velocity gradient from start to end
    Crescendo {
        start_vel: i32,
        end_vel: i32,
    },
    /// Snap pitches to a scale
    Quantize {
        root: String,
        mode: String,
    },
    /// Repeat pattern N times end-to-end
    Repeat { times: usize },
    /// Set velocity pattern (comma-separated, cycles)
    Velocities { pattern: String },
    /// Concatenate a second pattern after this one
    Concat {
        /// JSON string of second pattern
        second: String,
    },
    /// Stack a second pattern on top (same time)
    Stack {
        /// JSON string of second pattern
        second: String,
    },
    /// Interpolate between two patterns on a note grid
    Morph {
        /// JSON string of second pattern
        second: String,
        #[arg(long, default_value_t = 0.5)]
        amount: f64,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Split pattern into two complementary voices
    Weave {
        /// Which voice to output: "a" or "b"
        #[arg(long)]
        voice: String,
        /// Fraction of notes sent to voice B (0.0–1.0)
        #[arg(long, default_value_t = 0.5)]
        balance: f64,
        /// RNG seed (same seed = complementary split)
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Transpose output by N semitones
        #[arg(long, default_value_t = 0)]
        transpose: i32,
    },

    // ── Curve Generators (stdout) ───────────────────────────
    /// Generate automation curves
    Curve {
        #[command(subcommand)]
        cmd: CurveCmd,
    },

    // ── Curve Transforms (stdin → stdout) ───────────────────
    /// Invert curve values (1.0 - value)
    CurveInvert,
    /// Reverse curve values in time
    CurveReverse,
    /// Remap curve to a new value range
    CurveScale {
        min: f64,
        max: f64,
    },
    /// Shift all curve values by an offset
    CurveOffset { amount: f64 },
    /// Crossfade with a second curve
    CurveBlend {
        /// JSON string of second curve
        second: String,
        #[arg(long, default_value_t = 0.5)]
        mix: f64,
    },

    // ── Cross-Domain Bridges ────────────────────────────────
    /// Extract a pattern feature as a curve (pitch, velocity, density)
    ToCurve {
        /// Feature to extract: pitch, velocity, density
        feature: String,
        #[arg(long, default_value_t = 16.0)]
        bars: f64,
        #[arg(long, default_value_t = 0.5)]
        resolution: f64,
    },
    /// Convert curve values to pattern pitches
    ToPattern {
        #[arg(long, default_value_t = 0.5)]
        duration: f64,
        #[arg(long, default_value_t = 100)]
        velocity: i32,
    },

    // ── Save / Load ─────────────────────────────────────────
    /// Save stdin data to a named stash
    Save { name: String },
    /// Load a named stash to stdout
    Load { name: String },

    // ── Harmony (stdout progression) ──────────────────────
    /// Generate chord progressions via voice-leading graph
    Harmony {
        #[command(subcommand)]
        cmd: HarmonyCmd,
    },

    // ── Voicing (stdin progression → stdout pattern) ────
    /// Voice a chord progression
    Voice {
        #[command(subcommand)]
        cmd: VoiceCmd,
    },

    // ── Generative ──────────────────────────────────────
    /// Generate melody via Markov chain over a scale
    Markov {
        /// Root note (e.g. "C4")
        root: String,
        /// Scale mode
        mode: String,
        #[arg(long, default_value_t = 16)]
        steps: usize,
        #[arg(long, default_value_t = 0.5)]
        duration: f64,
        #[arg(long, default_value_t = 100)]
        velocity: i32,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Generate evolving pattern via iterative accumulation
    Accumulate {
        /// Seed pattern as JSON
        #[arg(long)]
        seed: String,
        /// Comma-separated transforms (rotate:3,transpose:5,invert,retrograde)
        #[arg(long)]
        transforms: String,
        #[arg(long, default_value_t = 8)]
        iterations: usize,
        #[arg(long, default_value_t = 100)]
        velocity: i32,
    },

    // ── Genre & Rhythm ────────────────────────────────────────
    /// Generate canonical drum pattern for a genre
    Genre {
        #[command(subcommand)]
        cmd: GenreCmd,
    },
    /// Generate a named rhythmic figure (single-voice, composable)
    Rhythm {
        /// Figure name (four-on-the-floor, dembow, 2step, halftime, breakbeat, stutter, etc.)
        name: String,
        #[arg(long, default_value_t = 36)]
        pitch: i32,
        #[arg(long, default_value_t = 100)]
        velocity: i32,
        #[arg(long, default_value_t = 0.25)]
        duration: f64,
    },

    // ── Real-time parameter control ─────────────────────────
    /// Smoothly change a device parameter over time in real-time
    Walk {
        track: String,
        param: String,
        #[arg(long)]
        from: Option<f64>,
        #[arg(long)]
        to: Option<f64>,
        #[arg(long, default_value_t = 8.0)]
        seconds: f64,
        #[arg(long, default_value_t = 0)]
        device: i32,
        /// Drunk walk mode
        #[arg(long)]
        drunk: bool,
        /// Sine oscillation mode
        #[arg(long)]
        sine: bool,
        /// Range for drunk/sine modes
        #[arg(long, num_args = 2)]
        range: Option<Vec<f64>>,
        /// Step size for drunk walk
        #[arg(long, default_value_t = 0.05)]
        step: f64,
        /// Cycle length for sine mode (seconds)
        #[arg(long, default_value_t = 4.0)]
        cycle: f64,
        /// Random seed for drunk walk
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },

    // ── Sinks (stdin → Ableton) ─────────────────────────────
    /// Write pattern to a clip
    Write {
        /// "track:slot" (e.g. "pad:0")
        target: String,
        #[arg(long)]
        length: Option<f64>,
        #[arg(long)]
        name: Option<String>,
    },
    /// Write curve as clip automation
    Automate {
        /// "track:slot" (e.g. "pad:0")
        target: String,
        /// Parameter name
        param: String,
        #[arg(long, default_value_t = 0)]
        device: i32,
        #[arg(long, default_value_t = 0.25)]
        resolution: f64,
    },

    // ── Daemon ───────────────────────────────────────────────
    /// Manage the persistent daemon for incremental updates
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
    },

    // ── Scenes ───────────────────────────────────────────────
    /// Create/configure a scene
    Scene {
        idx: i32,
        #[arg(long)]
        name: Option<String>,
    },
    /// Fire a scene (or just play)
    Fire { idx: Option<i32> },
    /// Stop all clips
    StopAll,

    // ── Snapshots ────────────────────────────────────────────
    /// Save/restore session mix state
    Snapshot {
        #[command(subcommand)]
        cmd: SnapshotCmd,
    },

    // ── Loom (global weave) ─────────────────────────────────
    /// Manage global compositional weave — strands, crossings, arcs
    Loom {
        #[command(subcommand)]
        cmd: LoomCmd,
    },
}

#[derive(Subcommand)]
enum CurveCmd {
    /// Sine wave
    Sine {
        #[arg(long)]
        cycle: f64,
        #[arg(long, num_args = 2)]
        range: Vec<f64>,
        #[arg(long)]
        bars: f64,
        #[arg(long, default_value_t = 0.0)]
        phase: f64,
        #[arg(long, default_value_t = 0.5)]
        resolution: f64,
    },
    /// Drunk walk (Brownian motion)
    Drunk {
        #[arg(long, num_args = 2)]
        range: Vec<f64>,
        #[arg(long)]
        bars: f64,
        #[arg(long, default_value_t = 0.05)]
        step: f64,
        #[arg(long, default_value_t = 0.0)]
        gravity: f64,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 0.5)]
        resolution: f64,
    },
    /// Attack-sustain-release envelope
    Envelope {
        #[arg(long, default_value_t = 0.1)]
        attack: f64,
        #[arg(long, default_value_t = 0.8)]
        sustain: f64,
        #[arg(long, default_value_t = 0.3)]
        release: f64,
        #[arg(long)]
        bars: f64,
        #[arg(long, num_args = 2, default_values_t = vec![0.0, 1.0])]
        range: Vec<f64>,
        #[arg(long, default_value_t = 0.5)]
        resolution: f64,
    },
    /// Linear ramp
    Ramp {
        start: f64,
        end: f64,
        #[arg(long)]
        bars: f64,
        #[arg(long, default_value_t = 0.5)]
        resolution: f64,
    },
    /// Random values with smoothing
    Stochastic {
        #[arg(long, num_args = 2)]
        range: Vec<f64>,
        #[arg(long)]
        bars: f64,
        #[arg(long, default_value_t = 3)]
        density: usize,
        #[arg(long, default_value_t = 3)]
        smoothing: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 0.5)]
        resolution: f64,
    },
}

#[derive(Subcommand)]
enum HarmonyCmd {
    /// Random walk through chord space
    Walk {
        /// Starting chord (e.g. "Cmaj7", "F#m7")
        chord: String,
        #[arg(long, default_value_t = 4)]
        steps: usize,
        #[arg(long, default_value_t = 1)]
        dist: u8,
        #[arg(long)]
        avoid_revisit: bool,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Shortest/smoothest path between two chords
    Path {
        start: String,
        end: String,
        #[arg(long)]
        smoothest: bool,
    },
    /// PLR orbit (triads only)
    Orbit {
        /// Starting chord (e.g. "Cmajor")
        chord: String,
        /// Comma-separated operations (P,L,R)
        ops: String,
        #[arg(long, default_value_t = 24)]
        max_iter: usize,
    },
    /// List neighbors of a chord
    Neighbors {
        chord: String,
        #[arg(long, default_value_t = 1)]
        dist: u8,
    },
}

#[derive(Subcommand)]
enum VoiceCmd {
    /// Bloom voicing (staggered entries)
    Bloom {
        #[arg(long, num_args = 2, default_values_t = vec![48, 72])]
        range: Vec<i32>,
    },
    /// Open voicing (drop-2)
    Open {
        #[arg(long, num_args = 2, default_values_t = vec![48, 72])]
        range: Vec<i32>,
    },
    /// Wide voicing (spread across octaves)
    Wide {
        #[arg(long, num_args = 2, default_values_t = vec![36, 84])]
        range: Vec<i32>,
    },
    /// Voice-led (minimal movement between chords)
    Led {
        #[arg(long, num_args = 2, default_values_t = vec![48, 72])]
        range: Vec<i32>,
    },
}

#[derive(Subcommand)]
enum GenreCmd {
    /// Output the drum pattern for a genre (pipe to mr write)
    Drums {
        /// Genre name (house, techno, 2step, dubstep, jungle, dembow, jersey, etc.)
        name: String,
        /// Also set Ableton tempo to the genre's sweet spot
        #[arg(long)]
        tempo: bool,
        /// Print genre info (BPM range, tips) to stderr
        #[arg(long)]
        info: bool,
    },
    /// List all available genres
    List,
}

#[derive(Subcommand)]
enum SnapshotCmd {
    /// Save current session mix state
    Save { name: String },
    /// Restore a saved snapshot (matches tracks by name)
    Restore { name: String },
    /// List saved snapshots
    List,
    /// Delete a saved snapshot
    Delete { name: String },
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Start the daemon (persistent Ableton connection + incremental updates)
    Start,
    /// Stop the daemon
    Stop,
    /// Show daemon status
    Status,
}

#[derive(Subcommand)]
enum LoomCmd {
    /// Create a new weave plan
    New {
        /// Duration in bars
        #[arg(long)]
        duration: u32,
        /// Duration in minutes (default 20)
        #[arg(long)]
        minutes: Option<f64>,
    },
    /// Add or update a strand (named voice with lifecycle)
    Strand {
        /// Strand name (maps to a track name)
        name: String,
        /// When the strand enters (0.0-1.0, proportion of weave)
        #[arg(long)]
        enter: f64,
        /// Attack duration (proportion, default 0.1)
        #[arg(long)]
        attack: Option<f64>,
        /// Sustain duration (proportion, default 0.7)
        #[arg(long)]
        sustain: Option<f64>,
        /// Release duration (proportion, default 0.1)
        #[arg(long)]
        release: Option<f64>,
    },
    /// Set a parameter trajectory on a strand
    Trajectory {
        /// Strand name
        strand: String,
        /// Parameter name
        param: String,
        /// Trajectory spec: constant:0.5, ramp:0.2,0.8, drunk:lo,hi,step, sine:lo,hi,cycles, follow:strand,param,lag
        spec: String,
    },
    /// Add a crossing (interaction between strands)
    Crossing {
        /// Crossing kind: handoff, crossfade, entrain, diverge, converge, echo
        kind: String,
        /// Strand names involved
        #[arg(num_args = 1..)]
        strands: Vec<String>,
        /// When the crossing occurs (0.0-1.0)
        #[arg(long)]
        at: f64,
        /// Duration of crossing (proportion, default 0.05)
        #[arg(long)]
        duration: Option<f64>,
    },
    /// Add an arc movement (coarse structure)
    Arc {
        /// Movement name
        name: String,
        /// Start position (0.0-1.0)
        #[arg(long)]
        from: f64,
        /// End position (0.0-1.0)
        #[arg(long)]
        to: f64,
        /// Energy trajectory spec (default: ramp proportional to position)
        #[arg(long)]
        energy: Option<String>,
    },
    /// Start the weave (begin tracking time)
    Start,
    /// Show current weave status (JSON)
    Status,
    /// Show the full weave plan
    Show,
    /// End the active weave (archives it)
    End,
    /// Export the raw weave JSON
    Export,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        // ── Session ──
        Cmd::Tempo { bpm } => session::tempo(bpm),
        Cmd::Play => session::play(),
        Cmd::Stop => session::stop(),
        Cmd::Undo => session::undo(),
        Cmd::Redo => session::redo(),

        // ── Tracks ──
        Cmd::Track { name, simpler, instrument, audio } => {
            track::create(&name, simpler.as_deref(), instrument.as_deref(), audio)
        }
        Cmd::DeleteTrack { name } => track::delete(&name),
        Cmd::Effect { track: t, name } => track::effect(&t, &name),
        Cmd::Tracks => track::list(),

        // ── Mixing ──
        Cmd::Mix { track: t, vol, pan, mute, solo } => mix::mix(&t, vol, pan, mute, solo),
        Cmd::Send { track: t, return_track, level } => mix::send(&t, &return_track, level),

        // ── Device ──
        Cmd::Device { track: t, device, params } => device::set_params(&t, device, &params),

        // ── Inspection ──
        Cmd::Status => inspect::status(),
        Cmd::Inspect { target } => {
            if target.contains(':') {
                inspect::inspect_clip(&target)
            } else {
                inspect::inspect_track(&target)
            }
        }
        Cmd::Params { track: t, device_idx } => inspect::params(&t, device_idx),
        Cmd::Returns => inspect::returns(),
        Cmd::Read { target } => inspect::read(&target),
        Cmd::Meters => inspect::meters(),
        Cmd::Spectrum { json, continuous, device } => {
            spectrum::spectrum(json, continuous, device.as_deref())
        }

        // ── Pattern Generators ──
        Cmd::Scale { root, mode, octaves, duration, velocity } => {
            generate::scale(&root, &mode, octaves, duration, velocity)
        }
        Cmd::Euclidean { hits, steps, pitch, duration, rotation, velocity } => {
            generate::euclidean(hits, steps, pitch, duration, rotation, velocity)
        }
        Cmd::Chord { spec, duration, velocity } => generate::chord(&spec, duration, velocity),
        Cmd::Notes { notation } => generate::notes(&notation),

        // ── Pattern Transforms ──
        Cmd::Transpose { semitones } => transform::transpose(semitones),
        Cmd::Rotate { n } => transform::rotate(n),
        Cmd::Retrograde => transform::retrograde(),
        Cmd::Invert { axis } => transform::invert(axis),
        Cmd::Stretch { factor } => transform::stretch(factor),
        Cmd::Degrade { probability, seed } => transform::degrade(probability, seed),
        Cmd::Thin { n } => transform::thin(n),
        Cmd::Humanize { timing, velocity, seed } => transform::humanize(timing, velocity, seed),
        Cmd::Swing { amount } => transform::swing(amount),
        Cmd::Overlap { amount } => transform::overlap(amount),
        Cmd::Crescendo { start_vel, end_vel } => transform::crescendo(start_vel, end_vel),
        Cmd::Quantize { root, mode } => transform::quantize(&root, &mode),
        Cmd::Repeat { times } => transform::repeat(times),
        Cmd::Velocities { pattern } => transform::velocities(&pattern),
        Cmd::Concat { second } => transform::concat(&second),
        Cmd::Stack { second } => transform::stack(&second),
        Cmd::Morph { second, amount, seed } => transform::morph(&second, amount, seed),
        Cmd::Weave { voice, balance, seed, transpose } => {
            transform::weave(&voice, balance, seed, transpose)
        }

        // ── Curve Generators ──
        Cmd::Curve { cmd } => match cmd {
            CurveCmd::Sine { cycle, range, bars, phase, resolution } => {
                curve::sine(cycle, [range[0], range[1]], bars, phase, resolution)
            }
            CurveCmd::Drunk { range, bars, step, gravity, seed, resolution } => {
                curve::drunk([range[0], range[1]], bars, step, gravity, seed, resolution)
            }
            CurveCmd::Envelope { attack, sustain, release, bars, range, resolution } => {
                curve::envelope(attack, sustain, release, bars, [range[0], range[1]], resolution)
            }
            CurveCmd::Ramp { start, end, bars, resolution } => {
                curve::ramp(start, end, bars, resolution)
            }
            CurveCmd::Stochastic { range, bars, density, smoothing, seed, resolution } => {
                curve::stochastic([range[0], range[1]], bars, density, smoothing, seed, resolution)
            }
        },

        // ── Curve Transforms ──
        Cmd::CurveInvert => curve_transform::invert(),
        Cmd::CurveReverse => curve_transform::reverse(),
        Cmd::CurveScale { min, max } => curve_transform::scale(min, max),
        Cmd::CurveOffset { amount } => curve_transform::offset(amount),
        Cmd::CurveBlend { second, mix } => curve_transform::blend(&second, mix),

        // ── Harmony ──
        Cmd::Harmony { cmd } => match cmd {
            HarmonyCmd::Walk { chord, steps, dist, avoid_revisit, seed } => {
                harmony::walk(&chord, steps, dist, avoid_revisit, seed)
            }
            HarmonyCmd::Path { start, end, smoothest } => {
                harmony::path(&start, &end, smoothest)
            }
            HarmonyCmd::Orbit { chord, ops, max_iter } => {
                harmony::orbit(&chord, &ops, max_iter)
            }
            HarmonyCmd::Neighbors { chord, dist } => {
                harmony::neighbors(&chord, dist)
            }
        },

        // ── Voicing ──
        Cmd::Voice { cmd } => match cmd {
            VoiceCmd::Bloom { range } => voice::bloom([range[0], range[1]], 0.02),
            VoiceCmd::Open { range } => voice::open([range[0], range[1]]),
            VoiceCmd::Wide { range } => voice::wide([range[0], range[1]]),
            VoiceCmd::Led { range } => voice::led([range[0], range[1]]),
        },

        // ── Generative ──
        Cmd::Markov { root, mode, steps, duration, velocity, seed } => {
            generative::markov(&root, &mode, steps, duration, velocity, seed)
        }
        Cmd::Accumulate { seed, transforms, iterations, velocity } => {
            generative::accumulate(&seed, &transforms, iterations, velocity)
        }

        // ── Genre & Rhythm ──
        Cmd::Genre { cmd } => match cmd {
            GenreCmd::Drums { name, tempo, info } => genre::drums(&name, tempo, info),
            GenreCmd::List => genre::list(),
        },
        Cmd::Rhythm { name, pitch, velocity, duration } => {
            genre::rhythm(&name, pitch, velocity, duration)
        }

        // ── Bridges ──
        Cmd::ToCurve { feature, bars, resolution } => bridge::to_curve(&feature, bars, resolution),
        Cmd::ToPattern { duration, velocity } => bridge::to_pattern(duration, velocity),

        // ── Save / Load ──
        Cmd::Save { name } => stash::save(&name),
        Cmd::Load { name } => stash::load(&name),

        // ── Real-time ──
        Cmd::Walk { track: t, param, from, to, seconds, device, drunk, sine, range, step, cycle, seed } => {
            walk::walk(&t, &param, from, to, seconds, device, drunk, sine, range, step, cycle, seed)
        }

        // ── Sinks ──
        Cmd::Write { target, length, name } => sink::write(&target, length, name.as_deref()),
        Cmd::Automate { target, param, device, resolution } => {
            sink::automate(&target, &param, device, resolution)
        }

        // ── Daemon ──
        Cmd::Daemon { cmd } => match cmd {
            DaemonCmd::Start => daemon::start(),
            DaemonCmd::Stop => daemon::stop(),
            DaemonCmd::Status => daemon::status(),
        },

        // ── Scenes ──
        Cmd::Scene { idx, name } => scene::scene(idx, name.as_deref()),
        Cmd::Fire { idx } => scene::fire(idx),
        Cmd::StopAll => scene::stop_all(),

        // ── Snapshots ──
        Cmd::Snapshot { cmd } => match cmd {
            SnapshotCmd::Save { name } => snapshot::save(&name),
            SnapshotCmd::Restore { name } => snapshot::restore(&name),
            SnapshotCmd::List => snapshot::list(),
            SnapshotCmd::Delete { name } => snapshot::delete(&name),
        },

        // ── Loom ──
        Cmd::Loom { cmd } => match cmd {
            LoomCmd::New { duration, minutes } => loom::new(duration, minutes),
            LoomCmd::Strand { name, enter, attack, sustain, release } => {
                loom::strand(&name, enter, attack, sustain, release)
            }
            LoomCmd::Trajectory { strand, param, spec } => {
                loom::trajectory(&strand, &param, &spec)
            }
            LoomCmd::Crossing { kind, strands, at, duration } => {
                loom::crossing(&kind, &strands, at, duration)
            }
            LoomCmd::Arc { name, from, to, energy } => {
                loom::arc(&name, from, to, energy.as_deref())
            }
            LoomCmd::Start => loom::start(),
            LoomCmd::Status => loom::status(),
            LoomCmd::Show => loom::show(),
            LoomCmd::End => loom::end(),
            LoomCmd::Export => loom::export(),
        },
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
