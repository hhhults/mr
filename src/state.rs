//! Session state model — the daemon's view of what's in Ableton.
//!
//! Populated on startup by querying Ableton, updated as mutations flow through.
//! This is the canonical state that subscribers (visualizer, etc.) consume.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::json::MrNote;

/// Full session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub tempo: f64,
    pub playing: bool,
    pub tracks: Vec<TrackState>,
    pub returns: Vec<ReturnState>,
}

/// A track's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackState {
    pub name: String,
    pub index: i32,
    pub volume: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub devices: Vec<DeviceState>,
    pub clips: HashMap<i32, ClipState>,
    pub sends: Vec<f64>,
}

/// A clip's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipState {
    pub name: String,
    pub length: f64,
    pub notes: Vec<MrNote>,
}

/// A device's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub name: String,
    pub params: Vec<ParamState>,
}

/// A device parameter's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamState {
    pub name: String,
    pub value: f64,
}

/// A return track's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnState {
    pub name: String,
    pub index: i32,
    pub volume: f64,
    pub pan: f64,
}

/// Events emitted when state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum StateEvent {
    #[serde(rename = "tempo_changed")]
    TempoChanged { tempo: f64 },
    #[serde(rename = "transport_changed")]
    TransportChanged { playing: bool },
    #[serde(rename = "track_created")]
    TrackCreated { index: i32, name: String },
    #[serde(rename = "track_deleted")]
    TrackDeleted { index: i32 },
    #[serde(rename = "clip_written")]
    ClipWritten {
        track: String,
        track_idx: i32,
        slot: i32,
        notes: Vec<MrNote>,
        length: f64,
    },
    #[serde(rename = "clip_automated")]
    ClipAutomated {
        track: String,
        track_idx: i32,
        slot: i32,
        param: String,
        device: i32,
        points: Vec<[f64; 2]>,
    },
    #[serde(rename = "param_changed")]
    ParamChanged {
        track: String,
        track_idx: i32,
        device: i32,
        param: String,
        value: f64,
    },
    #[serde(rename = "mix_changed")]
    MixChanged {
        track: String,
        track_idx: i32,
        volume: Option<f64>,
        pan: Option<f64>,
        mute: Option<bool>,
        solo: Option<bool>,
    },
    #[serde(rename = "send_changed")]
    SendChanged {
        track: String,
        track_idx: i32,
        return_idx: i32,
        level: f64,
    },
    #[serde(rename = "effect_loaded")]
    EffectLoaded {
        track: String,
        track_idx: i32,
        effect: String,
    },
    #[serde(rename = "scene_fired")]
    SceneFired { index: i32 },
}

impl SessionState {
    /// Build initial state by querying Ableton.
    pub fn from_session(session: &ableton::Session) -> Self {
        let tempo = session.get_tempo().unwrap_or(120.0) as f64;
        let playing = session.is_playing().unwrap_or(false);

        let track_names = session.track_names().unwrap_or_default();
        let tracks: Vec<TrackState> = track_names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let track = session.track(i as i32);
                let volume = track.get_volume().unwrap_or(0.85) as f64;
                let pan = track.get_panning().unwrap_or(0.0) as f64;
                let mute = track.get_mute().unwrap_or(false);
                let solo = track.get_solo().unwrap_or(false);

                TrackState {
                    name: name.clone(),
                    index: i as i32,
                    volume,
                    pan,
                    mute,
                    solo,
                    devices: Vec::new(), // populated lazily
                    clips: HashMap::new(),
                    sends: Vec::new(),
                }
            })
            .collect();

        let return_names = session.return_track_names().unwrap_or_default();
        let returns: Vec<ReturnState> = return_names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let rt = session.return_track(i as i32);
                ReturnState {
                    name: name.clone(),
                    index: i as i32,
                    volume: rt.get_volume().unwrap_or(0.85) as f64,
                    pan: rt.get_panning().unwrap_or(0.0) as f64,
                }
            })
            .collect();

        SessionState {
            tempo,
            playing,
            tracks,
            returns,
        }
    }

    /// Update state from a clip write event.
    pub fn apply_clip_write(
        &mut self,
        track_idx: i32,
        slot: i32,
        notes: Vec<MrNote>,
        length: f64,
        name: &str,
    ) {
        if let Some(track) = self.tracks.get_mut(track_idx as usize) {
            track.clips.insert(
                slot,
                ClipState {
                    name: name.to_string(),
                    length,
                    notes,
                },
            );
        }
    }

    /// Update state from a tempo change.
    pub fn apply_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
    }

    /// Update state from a mix change.
    pub fn apply_mix(
        &mut self,
        track_idx: i32,
        volume: Option<f64>,
        pan: Option<f64>,
        mute: Option<bool>,
        solo: Option<bool>,
    ) {
        if let Some(track) = self.tracks.get_mut(track_idx as usize) {
            if let Some(v) = volume {
                track.volume = v;
            }
            if let Some(p) = pan {
                track.pan = p;
            }
            if let Some(m) = mute {
                track.mute = m;
            }
            if let Some(s) = solo {
                track.solo = s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_event_serialization() {
        let event = StateEvent::ClipWritten {
            track: "pad".into(),
            track_idx: 0,
            slot: 0,
            notes: vec![MrNote {
                pitch: 60,
                start: 0.0,
                duration: 1.0,
                velocity: 100,
            }],
            length: 16.0,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("clip_written"));
        assert!(json.contains("pad"));

        let parsed: StateEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            StateEvent::ClipWritten { track, slot, .. } => {
                assert_eq!(track, "pad");
                assert_eq!(slot, 0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_apply_clip_write() {
        let mut state = SessionState {
            tempo: 120.0,
            playing: false,
            tracks: vec![TrackState {
                name: "pad".into(),
                index: 0,
                volume: 0.85,
                pan: 0.0,
                mute: false,
                solo: false,
                devices: vec![],
                clips: HashMap::new(),
                sends: vec![],
            }],
            returns: vec![],
        };

        let notes = vec![MrNote {
            pitch: 60,
            start: 0.0,
            duration: 1.0,
            velocity: 100,
        }];
        state.apply_clip_write(0, 0, notes, 16.0, "Melody");
        assert!(state.tracks[0].clips.contains_key(&0));
        assert_eq!(state.tracks[0].clips[&0].notes.len(), 1);
    }
}
