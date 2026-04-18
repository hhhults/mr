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
                ..Default::default()
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
                ..Default::default()
        }];
        state.apply_clip_write(0, 0, notes, 16.0, "Melody");
        assert!(state.tracks[0].clips.contains_key(&0));
        assert_eq!(state.tracks[0].clips[&0].notes.len(), 1);
    }

    fn make_state() -> SessionState {
        SessionState {
            tempo: 120.0,
            playing: false,
            tracks: vec![
                TrackState {
                    name: "pad".into(),
                    index: 0,
                    volume: 0.85,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    devices: vec![],
                    clips: HashMap::new(),
                    sends: vec![],
                },
                TrackState {
                    name: "bass".into(),
                    index: 1,
                    volume: 0.7,
                    pan: -0.1,
                    mute: false,
                    solo: false,
                    devices: vec![],
                    clips: HashMap::new(),
                    sends: vec![],
                },
            ],
            returns: vec![ReturnState {
                name: "Reverb".into(),
                index: 0,
                volume: 0.8,
                pan: 0.0,
            }],
        }
    }

    #[test]
    fn test_apply_tempo() {
        let mut state = make_state();
        assert_eq!(state.tempo, 120.0);
        state.apply_tempo(140.0);
        assert_eq!(state.tempo, 140.0);
    }

    #[test]
    fn test_apply_mix_volume() {
        let mut state = make_state();
        state.apply_mix(0, Some(0.5), None, None, None);
        assert!((state.tracks[0].volume - 0.5).abs() < 1e-10);
        // Other fields unchanged
        assert!((state.tracks[0].pan - 0.0).abs() < 1e-10);
        assert!(!state.tracks[0].mute);
    }

    #[test]
    fn test_apply_mix_all_fields() {
        let mut state = make_state();
        state.apply_mix(1, Some(0.9), Some(0.3), Some(true), Some(true));
        assert!((state.tracks[1].volume - 0.9).abs() < 1e-10);
        assert!((state.tracks[1].pan - 0.3).abs() < 1e-10);
        assert!(state.tracks[1].mute);
        assert!(state.tracks[1].solo);
    }

    #[test]
    fn test_apply_mix_out_of_range_track() {
        let mut state = make_state();
        // Should not panic on invalid track index
        state.apply_mix(99, Some(0.5), None, None, None);
        // Tracks unchanged
        assert!((state.tracks[0].volume - 0.85).abs() < 1e-10);
    }

    #[test]
    fn test_apply_clip_write_out_of_range() {
        let mut state = make_state();
        // Should not panic
        state.apply_clip_write(99, 0, vec![], 4.0, "Ghost");
        assert!(state.tracks[0].clips.is_empty());
    }

    #[test]
    fn test_apply_clip_write_overwrites() {
        let mut state = make_state();
        let notes1 = vec![MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100, ..Default::default() }];
        let notes2 = vec![
            MrNote { pitch: 64, start: 0.0, duration: 1.0, velocity: 80, ..Default::default() },
            MrNote { pitch: 67, start: 1.0, duration: 1.0, velocity: 80, ..Default::default() },
        ];
        state.apply_clip_write(0, 0, notes1, 4.0, "V1");
        assert_eq!(state.tracks[0].clips[&0].notes.len(), 1);
        state.apply_clip_write(0, 0, notes2, 8.0, "V2");
        assert_eq!(state.tracks[0].clips[&0].notes.len(), 2);
        assert_eq!(state.tracks[0].clips[&0].name, "V2");
    }

    #[test]
    fn test_state_event_all_variants_serde() {
        let events: Vec<StateEvent> = vec![
            StateEvent::TempoChanged { tempo: 130.0 },
            StateEvent::TransportChanged { playing: true },
            StateEvent::TrackCreated { index: 0, name: "kick".into() },
            StateEvent::TrackDeleted { index: 0 },
            StateEvent::ClipAutomated {
                track: "pad".into(), track_idx: 0, slot: 0,
                param: "Filter Freq".into(), device: 0,
                points: vec![[0.0, 0.5], [4.0, 0.8]],
            },
            StateEvent::ParamChanged {
                track: "pad".into(), track_idx: 0, device: 0,
                param: "Volume".into(), value: 0.7,
            },
            StateEvent::MixChanged {
                track: "pad".into(), track_idx: 0,
                volume: Some(0.8), pan: None, mute: None, solo: None,
            },
            StateEvent::SendChanged {
                track: "pad".into(), track_idx: 0, return_idx: 0, level: 0.5,
            },
            StateEvent::EffectLoaded {
                track: "pad".into(), track_idx: 0, effect: "Reverb".into(),
            },
            StateEvent::SceneFired { index: 2 },
        ];
        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let parsed: StateEvent = serde_json::from_str(&json).unwrap();
            // Roundtrip should produce valid JSON
            let json2 = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, json2);
        }
    }
}
