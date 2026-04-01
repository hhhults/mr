use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot connect to Ableton Live (is AbletonOSC running?): {0}")]
    Connection(String),

    #[error("track \"{0}\" not found")]
    TrackNotFound(String),

    #[error("return track \"{0}\" not found")]
    ReturnTrackNotFound(String),

    #[error("no clip in {track}:{slot}")]
    NoClip { track: String, slot: i32 },

    #[error("unknown note name: \"{0}\"")]
    BadNote(String),

    #[error("unknown scale mode: \"{0}\"\navailable: major, minor, dorian, phrygian, lydian, mixolydian, pentatonic, minor_pentatonic, blues, whole_tone, chromatic, harmonic_minor, melodic_minor")]
    BadScale(String),

    #[error("unknown chord quality: \"{0}\"\navailable: maj, min, dim, aug, 5, sus2, sus4, maj7, m7, 7, 6, m6, maj9, m9, 9, maj11, m11, 11, maj13, m13, 13, add9, add11, 7#9, 7b9, 7#11")]
    BadChord(String),

    #[error("invalid target \"{0}\" — expected \"track:slot\" (e.g. \"pad:0\")")]
    BadTarget(String),

    #[error("parameter \"{0}\" not found on device")]
    ParamNotFound(String),

    #[error("{command} expects {expected} input, got {got}")]
    TypeMismatch {
        command: String,
        expected: String,
        got: String,
    },

    #[error("no data on stdin — pipe a generator to this command\nexample: mr scale C4 dorian | mr {0}")]
    NoInput(String),

    #[error("invalid key:value pair: \"{0}\"")]
    BadKeyValue(String),

    #[error("{0}")]
    Ableton(#[from] ableton::Error),

    #[error("{0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Corpus(#[from] corpus::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
