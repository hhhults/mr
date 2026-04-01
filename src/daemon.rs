//! Persistent daemon for Ableton communication.
//!
//! Holds a single Session connection, accepts commands over a Unix socket,
//! and supports concurrent clients. Provides both:
//! - Semantic handlers (write, automate) with clip cache optimization
//! - Generic OSC proxy (query, send) for all other operations

use std::collections::HashMap;
use std::f64::consts::PI;
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fs, process, thread};

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use ableton::{Arg, Session};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::events::EventBroadcaster;
use crate::json::MrNote;
use crate::state::{SessionState, StateEvent};

// ─── Socket paths ───────────────────────────────────────────────────────────

fn socket_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mr")
}

pub fn socket_path() -> PathBuf {
    socket_dir().join("mr.sock")
}

fn pid_path() -> PathBuf {
    socket_dir().join("mr.pid")
}

// ─── Protocol ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub cmd: String,
    // Semantic command fields
    #[serde(default)]
    pub track: String,
    #[serde(default)]
    pub slot: i32,
    #[serde(default)]
    pub length: f64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub param: String,
    #[serde(default)]
    pub device: i32,
    #[serde(default)]
    pub resolution: f64,
    #[serde(default)]
    pub notes: Vec<MrNote>,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    // Walk fields
    #[serde(default)]
    pub walk_mode: String,
    #[serde(default)]
    pub walk_from: Option<f64>,
    #[serde(default)]
    pub walk_to: Option<f64>,
    #[serde(default)]
    pub walk_range: Option<[f64; 2]>,
    #[serde(default)]
    pub walk_step: f64,
    #[serde(default)]
    pub walk_cycle: f64,
    #[serde(default)]
    pub walk_seconds: f64,
    #[serde(default)]
    pub walk_seed: u64,
    // Mix fields
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub pan: Option<f64>,
    #[serde(default)]
    pub mute: Option<bool>,
    #[serde(default)]
    pub solo: Option<bool>,
    // Device param fields
    #[serde(default)]
    pub params_kv: Vec<[String; 2]>, // key:value pairs for device params
    // Send fields
    #[serde(default)]
    pub return_name: String,
    #[serde(default)]
    pub level: f64,
    // Track create fields
    #[serde(default)]
    pub instrument: String,
    #[serde(default)]
    pub simpler: String,
    #[serde(default)]
    pub audio: bool,
    // Scene fields
    #[serde(default)]
    pub scene_idx: Option<i32>,
    // Proxy fields
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub osc_args: Vec<Arg>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    // Batch query fields
    #[serde(default)]
    pub queries: Vec<BatchQueryEntry>,
}

impl Default for DaemonRequest {
    fn default() -> Self {
        DaemonRequest {
            cmd: String::new(),
            track: String::new(),
            slot: 0,
            length: 0.0,
            name: String::new(),
            param: String::new(),
            device: 0,
            resolution: 0.25,
            notes: Vec::new(),
            points: Vec::new(),
            walk_mode: String::new(),
            walk_from: None,
            walk_to: None,
            walk_range: None,
            walk_step: 0.05,
            walk_cycle: 4.0,
            walk_seconds: 8.0,
            walk_seed: 42,
            volume: None,
            pan: None,
            mute: None,
            solo: None,
            params_kv: Vec::new(),
            return_name: String::new(),
            level: 0.0,
            instrument: String::new(),
            simpler: String::new(),
            audio: false,
            scene_idx: None,
            queries: Vec::new(),
            address: String::new(),
            osc_args: Vec::new(),
            timeout_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueryEntry {
    pub address: String,
    #[serde(default)]
    pub args: Vec<Arg>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Vec<Arg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<Vec<Arg>>>,
}

impl DaemonResponse {
    fn ok(message: impl Into<String>) -> Self {
        DaemonResponse {
            ok: true,
            message: Some(message.into()),
            error: None,
            update_kind: None,
            result: None,
            results: None,
        }
    }

    fn ok_with_kind(message: impl Into<String>, kind: impl Into<String>) -> Self {
        DaemonResponse {
            ok: true,
            message: Some(message.into()),
            error: None,
            update_kind: Some(kind.into()),
            result: None,
            results: None,
        }
    }

    fn ok_with_result(result: Vec<Arg>) -> Self {
        DaemonResponse {
            ok: true,
            message: None,
            error: None,
            update_kind: None,
            result: Some(result),
            results: None,
        }
    }

    fn ok_with_results(results: Vec<Vec<Arg>>) -> Self {
        DaemonResponse {
            ok: true,
            message: None,
            error: None,
            update_kind: None,
            result: None,
            results: Some(results),
        }
    }

    fn err(error: impl Into<String>) -> Self {
        DaemonResponse {
            ok: false,
            message: None,
            error: Some(error.into()),
            update_kind: None,
            result: None,
            results: None,
        }
    }
}

// ─── Clip state cache ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ClipCacheEntry {
    length: f64,
    note_hash: u64,
}

fn hash_notes(notes: &[MrNote]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for n in notes {
        h ^= n.pitch as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= (n.start * 1000.0) as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= (n.duration * 1000.0) as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= n.velocity as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ─── Shared daemon state ────────────────────────────────────────────────────

struct SharedState {
    session: Session,
    track_cache: Mutex<HashMap<String, i32>>,
    clip_cache: Mutex<HashMap<(i32, i32), ClipCacheEntry>>,
    state: Arc<Mutex<SessionState>>,
    broadcaster: Arc<EventBroadcaster>,
    /// Active walk cancel flags, keyed by walk_id.
    active_walks: Mutex<HashMap<String, Arc<AtomicBool>>>,
    walk_counter: Mutex<u64>,
    /// Dedup: recent broadcast timestamps keyed by "track_idx:property".
    recent_broadcasts: Mutex<HashMap<String, Instant>>,
    /// Suppress listener callbacks until initial value dump has drained.
    listeners_ready: AtomicBool,
    /// Cached param names: (track_idx, device_idx, param_idx) → name.
    param_name_cache: Mutex<HashMap<(i32, i32, i32), String>>,
}

fn refresh_track_cache(state: &SharedState) -> Result<()> {
    let names = state.session.track_names()?;
    let mut cache = state.track_cache.lock().unwrap();
    cache.clear();
    for (i, name) in names.iter().enumerate() {
        cache.insert(name.to_lowercase(), i as i32);
    }
    Ok(())
}

fn resolve_track(state: &SharedState, name: &str) -> Result<i32> {
    let key = name.to_lowercase();
    {
        let cache = state.track_cache.lock().unwrap();
        if let Some(&idx) = cache.get(&key) {
            return Ok(idx);
        }
    }
    refresh_track_cache(state)?;
    state
        .track_cache
        .lock()
        .unwrap()
        .get(&key)
        .copied()
        .ok_or_else(|| Error::TrackNotFound(name.to_string()))
}

// ─── State polling (detect GUI changes) ─────────────────────────────────────

fn poll_ableton_state(state: &SharedState) {
    // Check tempo
    if let Ok(tempo) = state.session.get_tempo() {
        let tempo = tempo as f64;
        let mut s = state.state.lock().unwrap();
        if (s.tempo - tempo).abs() > 0.01 {
            s.tempo = tempo;
            drop(s);
            state
                .broadcaster
                .broadcast(&StateEvent::TempoChanged { tempo });
        }
    }

    // Check playing state
    if let Ok(playing) = state.session.is_playing() {
        let mut s = state.state.lock().unwrap();
        if s.playing != playing {
            s.playing = playing;
            drop(s);
            state
                .broadcaster
                .broadcast(&StateEvent::TransportChanged { playing });
        }
    }

    // Check track count (detect track add/remove in GUI)
    if let Ok(names) = state.session.track_names() {
        let mut s = state.state.lock().unwrap();
        if names.len() != s.tracks.len() {
            // Track count changed — refresh track cache and state
            drop(s);
            let _ = refresh_track_cache(state);
            let new_state = SessionState::from_session(&state.session);
            *state.state.lock().unwrap() = new_state;
        } else {
            // Check for renamed tracks
            for (i, name) in names.iter().enumerate() {
                if let Some(track) = s.tracks.get_mut(i) {
                    if &track.name != name {
                        track.name = name.clone();
                    }
                }
            }
        }
    }
}

// ─── Request handlers ───────────────────────────────────────────────────────

fn handle_request(state: &Arc<SharedState>, req: &DaemonRequest) -> DaemonResponse {
    match req.cmd.as_str() {
        "query" => handle_proxy_query(state, req),
        "batch_query" => handle_batch_query(state, req),
        "send" => handle_proxy_send(state, req),
        "write" => handle_write(state, req),
        "automate" => handle_automate(state, req),
        "status" => {
            let tracks = state.track_cache.lock().unwrap().len();
            let clips = state.clip_cache.lock().unwrap().len();
            let subs = state.broadcaster.subscriber_count();
            DaemonResponse::ok(format!(
                "daemon running — {} tracks, {} clips, {} subscribers",
                tracks, clips, subs
            ))
        }
        "refresh" => match refresh_track_cache(state) {
            Ok(()) => {
                let n = state.track_cache.lock().unwrap().len();
                DaemonResponse::ok(format!("refreshed — {} tracks", n))
            }
            Err(e) => DaemonResponse::err(e.to_string()),
        },
        "mix" => handle_mix(state, req),
        "send_level" => handle_send_level(state, req),
        "device_param" => handle_device_param(state, req),
        "track_create" => handle_track_create(state, req),
        "track_delete" => handle_track_delete(state, req),
        "effect_load" => handle_effect_load(state, req),
        "scene_fire" => handle_scene_fire(state, req),
        "walk" => handle_walk(state, req),
        "walk_stop" => handle_walk_stop(state, req),
        "walk_stop_all" => handle_walk_stop_all(state),
        "shutdown" => DaemonResponse::ok("shutting down"),
        _ => DaemonResponse::err(format!("unknown command: {}", req.cmd)),
    }
}

fn handle_proxy_query(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let timeout = req
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(2));
    match state
        .session
        .osc()
        .query_timeout(&req.address, &req.osc_args, timeout)
    {
        Ok(result) => DaemonResponse::ok_with_result(result),
        Err(e) => DaemonResponse::err(e.to_string()),
    }
}

fn handle_batch_query(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let timeout = req
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(2));
    let queries: Vec<(String, Vec<Arg>)> = req
        .queries
        .iter()
        .map(|q| (q.address.clone(), q.args.clone()))
        .collect();
    match state.session.osc().batch_query_timeout(&queries, timeout) {
        Ok(results) => DaemonResponse::ok_with_results(results),
        Err(e) => DaemonResponse::err(e.to_string()),
    }
}

fn handle_proxy_send(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    match state.session.osc().send(&req.address, &req.osc_args) {
        Ok(()) => {
            // Intercept known mutations and emit events
            if req.address == "/live/song/set/tempo" {
                if let Some(tempo) = req.osc_args.first().and_then(|a| a.as_f64()) {
                    state.state.lock().unwrap().apply_tempo(tempo);
                    state.broadcaster.broadcast(&StateEvent::TempoChanged { tempo });
                }
            } else if req.address == "/live/song/start_playing" {
                state.state.lock().unwrap().playing = true;
                state.broadcaster.broadcast(&StateEvent::TransportChanged { playing: true });
            } else if req.address == "/live/song/stop_playing" {
                state.state.lock().unwrap().playing = false;
                state.broadcaster.broadcast(&StateEvent::TransportChanged { playing: false });
            }
            DaemonResponse::ok("sent")
        }
        Err(e) => DaemonResponse::err(e.to_string()),
    }
}

fn handle_write(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };

    let track = state.session.track(track_idx);
    let new_hash = hash_notes(&req.notes);
    let cache_key = (track_idx, req.slot);

    let update_kind = {
        let cache = state.clip_cache.lock().unwrap();
        if let Some(cached) = cache.get(&cache_key) {
            if cached.note_hash == new_hash && (cached.length - req.length).abs() < 0.01 {
                return DaemonResponse::ok_with_kind(
                    format!("{}:{} unchanged", req.track, req.slot),
                    "no-op",
                );
            } else if (cached.length - req.length).abs() < 0.01 {
                "notes-only"
            } else {
                "recreate"
            }
        } else {
            "create"
        }
    };

    let result: std::result::Result<&str, String> = match update_kind {
        "notes-only" => {
            let clip = track.clip(req.slot);
            if let Err(e) = clip.clear_notes() {
                return DaemonResponse::err(format!("clear_notes: {}", e));
            }
            let ableton_notes: Vec<ableton::Note> = req
                .notes
                .iter()
                .map(|n| {
                    ableton::Note::new(n.pitch, n.start as f32, n.duration as f32, n.velocity)
                })
                .collect();
            for chunk in ableton_notes.chunks(80) {
                if let Err(e) = clip.add_notes(chunk) {
                    return DaemonResponse::err(format!("add_notes: {}", e));
                }
            }
            Ok("notes-only")
        }
        _ => {
            if track.has_clip(req.slot).unwrap_or(false) {
                let _ = track.delete_clip(req.slot);
                thread::sleep(Duration::from_millis(50));
            }
            let clip = match track.create_clip(req.slot, req.length as f32) {
                Ok(c) => c,
                Err(e) => return DaemonResponse::err(format!("create_clip: {}", e)),
            };
            thread::sleep(Duration::from_millis(100));
            if !req.name.is_empty() {
                let _ = clip.set_name(&req.name);
            }
            let _ = clip.set_looping(true);
            let ableton_notes: Vec<ableton::Note> = req
                .notes
                .iter()
                .map(|n| {
                    ableton::Note::new(n.pitch, n.start as f32, n.duration as f32, n.velocity)
                })
                .collect();
            for chunk in ableton_notes.chunks(80) {
                if let Err(e) = clip.add_notes(chunk) {
                    return DaemonResponse::err(format!("add_notes: {}", e));
                }
            }
            let _ = clip.fire();
            Ok(update_kind)
        }
    };

    match result {
        Ok(kind) => {
            state.clip_cache.lock().unwrap().insert(
                cache_key,
                ClipCacheEntry {
                    length: req.length,
                    note_hash: new_hash,
                },
            );

            // Update session state
            state.state.lock().unwrap().apply_clip_write(
                track_idx,
                req.slot,
                req.notes.clone(),
                req.length,
                &req.name,
            );

            // Emit event
            state.broadcaster.broadcast(&StateEvent::ClipWritten {
                track: req.track.clone(),
                track_idx,
                slot: req.slot,
                notes: req.notes.clone(),
                length: req.length,
            });

            DaemonResponse::ok_with_kind(
                format!(
                    "wrote {} notes to {}:{} ({:.1} beats)",
                    req.notes.len(),
                    req.track,
                    req.slot,
                    req.length
                ),
                kind,
            )
        }
        Err(e) => DaemonResponse::err(e.to_string()),
    }
}

fn handle_automate(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };

    let track = state.session.track(track_idx);
    let device = track.device(req.device);

    let param_names = match device.parameter_names() {
        Ok(n) => n,
        Err(e) => return DaemonResponse::err(format!("parameter_names: {}", e)),
    };
    let param_idx = param_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(&req.param))
        .or_else(|| {
            param_names
                .iter()
                .position(|n| n.to_lowercase().contains(&req.param.to_lowercase()))
        });
    let param_idx = match param_idx {
        Some(i) => i as i32,
        None => return DaemonResponse::err(format!("parameter \"{}\" not found", req.param)),
    };

    let clip = track.clip(req.slot);
    let smooth_points: Vec<(f32, f32)> = req.points.iter().map(|p| (p[0] as f32, p[1] as f32)).collect();
    let res = if req.resolution > 0.0 {
        req.resolution as f32
    } else {
        0.25
    };

    if let Err(e) = clip.automate_smooth(req.device, param_idx, &smooth_points, res) {
        return DaemonResponse::err(format!("automate_smooth: {}", e));
    }

    // Emit event
    state.broadcaster.broadcast(&StateEvent::ClipAutomated {
        track: req.track.clone(),
        track_idx,
        slot: req.slot,
        param: req.param.clone(),
        device: req.device,
        points: req.points.clone(),
    });

    DaemonResponse::ok_with_kind(
        format!(
            "automated {} on {}:{} ({} points)",
            req.param,
            req.track,
            req.slot,
            req.points.len()
        ),
        "automation",
    )
}

// ─── Semantic handlers: mix, device, track, effect, scene ────────────────────

fn handle_mix(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };
    let track = state.session.track(track_idx);
    let mut changes = Vec::new();

    if let Some(v) = req.volume {
        if let Err(e) = track.set_volume(v as f32) {
            return DaemonResponse::err(format!("set_volume: {e}"));
        }
        changes.push(format!("vol:{v:.2}"));
    }
    if let Some(p) = req.pan {
        if let Err(e) = track.set_panning(p as f32) {
            return DaemonResponse::err(format!("set_panning: {e}"));
        }
        changes.push(format!("pan:{p:.2}"));
    }
    if let Some(m) = req.mute {
        // Explicit set: mute=true → mute on, mute=false → mute off
        if let Err(e) = track.set_mute(m) {
            return DaemonResponse::err(format!("set_mute: {e}"));
        }
        changes.push(format!("mute:{m}"));
    }
    if let Some(s) = req.solo {
        if let Err(e) = track.set_solo(s) {
            return DaemonResponse::err(format!("set_solo: {e}"));
        }
        changes.push(format!("solo:{s}"));
    }

    // Read back actual values for event
    let vol = req.volume.or_else(|| track.get_volume().ok().map(|v| v as f64));
    let pan_val = req.pan.or_else(|| track.get_panning().ok().map(|v| v as f64));
    let mute_val = if req.mute.is_some() {
        track.get_mute().ok()
    } else {
        None
    };
    let solo_val = if req.solo.is_some() {
        track.get_solo().ok()
    } else {
        None
    };

    // Update state
    state.state.lock().unwrap().apply_mix(
        track_idx,
        req.volume,
        req.pan,
        mute_val,
        solo_val,
    );

    // Broadcast + record for dedup against listener callbacks
    if req.volume.is_some() { record_broadcast(state, &format!("{track_idx}:volume")); }
    if req.pan.is_some() { record_broadcast(state, &format!("{track_idx}:panning")); }
    if mute_val.is_some() { record_broadcast(state, &format!("{track_idx}:mute")); }
    if solo_val.is_some() { record_broadcast(state, &format!("{track_idx}:solo")); }
    state.broadcaster.broadcast(&StateEvent::MixChanged {
        track: req.track.clone(),
        track_idx,
        volume: req.volume,
        pan: req.pan,
        mute: mute_val,
        solo: solo_val,
    });

    DaemonResponse::ok(format!("mix {} → {}", req.track, changes.join(" ")))
}

fn handle_send_level(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };

    // Resolve return track
    let return_names = state.session.return_track_names().unwrap_or_default();
    let return_idx = return_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(&req.return_name))
        .or_else(|| {
            return_names
                .iter()
                .position(|n| n.to_lowercase().contains(&req.return_name.to_lowercase()))
        });
    let return_idx = match return_idx {
        Some(i) => i as i32,
        None => return DaemonResponse::err(format!("return track \"{}\" not found", req.return_name)),
    };

    let track = state.session.track(track_idx);
    if let Err(e) = track.set_send(return_idx, req.level as f32) {
        return DaemonResponse::err(format!("set_send: {e}"));
    }

    state.broadcaster.broadcast(&StateEvent::SendChanged {
        track: req.track.clone(),
        track_idx,
        return_idx,
        level: req.level,
    });

    DaemonResponse::ok(format!(
        "send {} → {} level:{:.2}",
        req.track, req.return_name, req.level
    ))
}

fn handle_device_param(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };
    let track = state.session.track(track_idx);
    let device = track.device(req.device);

    let param_names = match device.parameter_names() {
        Ok(n) => n,
        Err(e) => return DaemonResponse::err(format!("parameter_names: {e}")),
    };

    let mut changes = Vec::new();

    for kv in &req.params_kv {
        let key = &kv[0];
        let value: f32 = match kv[1].parse() {
            Ok(v) => v,
            Err(_) => return DaemonResponse::err(format!("bad value: {}", kv[1])),
        };

        let param_idx = param_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(key))
            .or_else(|| {
                param_names
                    .iter()
                    .position(|n| n.to_lowercase().contains(&key.to_lowercase()))
            });
        let param_idx = match param_idx {
            Some(i) => i,
            None => return DaemonResponse::err(format!("parameter \"{}\" not found", key)),
        };

        if let Err(e) = device.set_param(param_idx as i32, value) {
            return DaemonResponse::err(format!("set_param: {e}"));
        }

        record_broadcast(state, &format!("{track_idx}:d{}:p{param_idx}", req.device));
        state.broadcaster.broadcast(&StateEvent::ParamChanged {
            track: req.track.clone(),
            track_idx,
            device: req.device,
            param: param_names[param_idx].clone(),
            value: value as f64,
        });

        changes.push(format!("{}:{:.3}", param_names[param_idx], value));
    }

    DaemonResponse::ok(format!("device {} → {}", req.track, changes.join(" ")))
}

fn handle_track_create(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track = if req.audio {
        match state.session.create_audio_track(-1) {
            Ok(t) => t,
            Err(e) => return DaemonResponse::err(format!("create_audio_track: {e}")),
        }
    } else {
        match state.session.create_midi_track(-1) {
            Ok(t) => t,
            Err(e) => return DaemonResponse::err(format!("create_midi_track: {e}")),
        }
    };
    thread::sleep(Duration::from_millis(150));

    if let Err(e) = track.set_name(&req.name) {
        return DaemonResponse::err(format!("set_name: {e}"));
    }

    let track_idx = track.track_idx;

    if !req.instrument.is_empty() {
        if let Err(e) = state.session.load_instrument(track_idx, &req.instrument) {
            return DaemonResponse::err(format!("load_instrument: {e}"));
        }
        thread::sleep(Duration::from_millis(500));
    }

    if !req.simpler.is_empty() {
        // Stage sample
        let src = std::path::PathBuf::from(&req.simpler);
        if src.exists() {
            let dest_dir = dirs::home_dir()
                .unwrap_or_default()
                .join("Music/Ableton/User Library/Samples/Imported");
            let _ = std::fs::create_dir_all(&dest_dir);
            let filename = src.file_name().unwrap_or_default().to_string_lossy().to_string();
            let dest = dest_dir.join(&filename);
            let _ = std::fs::copy(&src, &dest);
            thread::sleep(Duration::from_millis(300));
            let _ = track.load_sample(&filename);
            thread::sleep(Duration::from_millis(300));
        }
    }

    // Refresh track cache
    let _ = refresh_track_cache(state);

    // Register listeners for the new track
    register_track_listeners(state, track_idx);

    // Broadcast
    state.broadcaster.broadcast(&StateEvent::TrackCreated {
        index: track_idx,
        name: req.name.clone(),
    });

    let label = if !req.instrument.is_empty() {
        format!("track \"{}\" [#{}] ← {}", req.name, track_idx, req.instrument)
    } else if !req.simpler.is_empty() {
        format!("track \"{}\" [#{}] ← simpler", req.name, track_idx)
    } else {
        format!("track \"{}\" [#{}]", req.name, track_idx)
    };
    DaemonResponse::ok(label)
}

fn handle_track_delete(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };
    if let Err(e) = state.session.delete_track(track_idx) {
        return DaemonResponse::err(format!("delete_track: {e}"));
    }
    let _ = refresh_track_cache(state);
    state.broadcaster.broadcast(&StateEvent::TrackDeleted { index: track_idx });
    DaemonResponse::ok(format!("deleted track \"{}\"", req.track))
}

fn handle_effect_load(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };
    if let Err(e) = state.session.load_effect(track_idx, &req.name) {
        return DaemonResponse::err(format!("load_effect: {e}"));
    }
    thread::sleep(Duration::from_millis(300));
    state.broadcaster.broadcast(&StateEvent::EffectLoaded {
        track: req.track.clone(),
        track_idx,
        effect: req.name.clone(),
    });
    DaemonResponse::ok(format!("effect \"{}\" → {}", req.name, req.track))
}

fn handle_scene_fire(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    match req.scene_idx {
        Some(idx) => {
            if let Err(e) = state.session.fire_scene(idx) {
                return DaemonResponse::err(format!("fire_scene: {e}"));
            }
            state.broadcaster.broadcast(&StateEvent::SceneFired { index: idx });
            DaemonResponse::ok(format!("fired scene {}", idx))
        }
        None => {
            if let Err(e) = state.session.play() {
                return DaemonResponse::err(format!("play: {e}"));
            }
            state.state.lock().unwrap().playing = true;
            state.broadcaster.broadcast(&StateEvent::TransportChanged { playing: true });
            DaemonResponse::ok("▶ playing")
        }
    }
}

// ─── Connection handling ────────────────────────────────────────────────────

fn handle_walk(state: &Arc<SharedState>, req: &DaemonRequest) -> DaemonResponse {
    let track_idx = match resolve_track(state, &req.track) {
        Ok(i) => i,
        Err(e) => return DaemonResponse::err(e.to_string()),
    };

    let track = state.session.track(track_idx);
    let device = track.device(req.device);

    // Resolve param
    let param_names = match device.parameter_names() {
        Ok(n) => n,
        Err(e) => return DaemonResponse::err(format!("parameter_names: {}", e)),
    };
    let param_idx = param_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(&req.param))
        .or_else(|| {
            param_names
                .iter()
                .position(|n| n.to_lowercase().contains(&req.param.to_lowercase()))
        });
    let param_idx = match param_idx {
        Some(i) => i as i32,
        None => return DaemonResponse::err(format!("parameter \"{}\" not found", req.param)),
    };
    let resolved_name = param_names[param_idx as usize].clone();

    // Generate walk ID
    let walk_id = {
        let mut counter = state.walk_counter.lock().unwrap();
        *counter += 1;
        format!("w{}", *counter)
    };

    // Create cancel flag
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .active_walks
        .lock()
        .unwrap()
        .insert(walk_id.clone(), cancel.clone());

    // Read current value
    let current = device.get_param(param_idx).unwrap_or(0.5) as f64;

    // Clone what the walk thread needs
    let osc = state.session.osc().clone();
    let broadcaster = state.broadcaster.clone();
    let state_ref = state.clone();
    let walk_id_clone = walk_id.clone();
    let track_name = req.track.clone();
    let device_idx = req.device;
    let mode = req.walk_mode.clone();
    let seconds = req.walk_seconds;
    let walk_from = req.walk_from;
    let walk_to = req.walk_to;
    let range = req.walk_range;
    let step = req.walk_step;
    let cycle = req.walk_cycle;
    let seed = req.walk_seed;

    thread::spawn(move || {
        let total_ticks = (seconds / 0.033).ceil() as usize;
        let start_time = Instant::now();

        let set_param = |val: f64| {
            let addr = "/live/device/set/parameter/value";
            let args = vec![
                Arg::Int(track_idx),
                Arg::Int(device_idx),
                Arg::Int(param_idx),
                Arg::Float(val as f32),
            ];
            let _ = osc.send(addr, &args);
            broadcaster.broadcast(&StateEvent::ParamChanged {
                track: track_name.clone(),
                track_idx,
                device: device_idx,
                param: resolved_name.clone(),
                value: val,
            });
        };

        match mode.as_str() {
            "drunk" => {
                let (vmin, vmax) = range.unwrap_or([0.0, 1.0]).into();
                let mut rng = SmallRng::seed_from_u64(seed);
                let mut val = current.clamp(vmin, vmax);

                for i in 0..total_ticks {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let elapsed = start_time.elapsed().as_secs_f64();
                    if elapsed >= seconds {
                        break;
                    }
                    set_param(val);
                    val += rng.random::<f64>() * step * 2.0 - step;
                    val = val.clamp(vmin, vmax);
                    let target = Duration::from_secs_f64((i + 1) as f64 * 0.033);
                    let now = start_time.elapsed();
                    if target > now {
                        thread::sleep(target - now);
                    }
                }
            }
            "sine" => {
                let (vmin, vmax) = range.unwrap_or([0.0, 1.0]).into();
                for i in 0..total_ticks {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let elapsed = start_time.elapsed().as_secs_f64();
                    if elapsed >= seconds {
                        break;
                    }
                    let t = elapsed / cycle;
                    let n = 0.5 + 0.5 * (2.0 * PI * t).sin();
                    let val = vmin + n * (vmax - vmin);
                    set_param(val);
                    let target = Duration::from_secs_f64((i + 1) as f64 * 0.033);
                    let now = start_time.elapsed();
                    if target > now {
                        thread::sleep(target - now);
                    }
                }
            }
            _ => {
                // Ramp (default)
                let v_from = walk_from.unwrap_or(current);
                let v_to = walk_to.unwrap_or(1.0);
                for i in 0..total_ticks {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let elapsed = start_time.elapsed().as_secs_f64();
                    if elapsed >= seconds {
                        break;
                    }
                    let frac = (elapsed / seconds).clamp(0.0, 1.0);
                    let val = v_from + frac * (v_to - v_from);
                    set_param(val);
                    let target = Duration::from_secs_f64((i + 1) as f64 * 0.033);
                    let now = start_time.elapsed();
                    if target > now {
                        thread::sleep(target - now);
                    }
                }
                // Hit exact target
                if !cancel.load(Ordering::Relaxed) {
                    set_param(walk_to.unwrap_or(1.0));
                }
            }
        }

        // Cleanup
        state_ref.active_walks.lock().unwrap().remove(&walk_id_clone);
    });

    DaemonResponse {
        ok: true,
        message: Some(format!("walk started: {}", walk_id)),
        error: None,
        update_kind: None,
        result: None,
        results: None,
    }
}

fn handle_walk_stop(state: &SharedState, req: &DaemonRequest) -> DaemonResponse {
    let walks = state.active_walks.lock().unwrap();
    if let Some(cancel) = walks.get(&req.name) {
        cancel.store(true, Ordering::Relaxed);
        DaemonResponse::ok(format!("stopping walk {}", req.name))
    } else {
        DaemonResponse::err(format!("no active walk: {}", req.name))
    }
}

fn handle_walk_stop_all(state: &SharedState) -> DaemonResponse {
    let walks = state.active_walks.lock().unwrap();
    let count = walks.len();
    for cancel in walks.values() {
        cancel.store(true, Ordering::Relaxed);
    }
    DaemonResponse::ok(format!("stopping {} walks", count))
}

// ─── Connection handling ────────────────────────────────────────────────────

fn handle_connection(state: &Arc<SharedState>, shutdown: &AtomicBool, stream: UnixStream) {
    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: DaemonRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = DaemonResponse::err(format!("bad request: {}", e));
                let _ = write_response(&stream, &resp);
                break;
            }
        };

        let is_shutdown = req.cmd == "shutdown";
        let resp = handle_request(state, &req);
        let _ = write_response(&stream, &resp);

        if is_shutdown {
            shutdown.store(true, Ordering::Relaxed);
            break;
        }
    }
}

fn write_response(mut stream: &UnixStream, resp: &DaemonResponse) -> Result<()> {
    let json = serde_json::to_string(resp)?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

// ─── AbletonOSC push listeners ──────────────────────────────────────────────

/// Register AbletonOSC listeners for track properties and song-level state.
/// AbletonOSC will push value changes back as OSC messages on the recv port.
fn register_ableton_listeners(state: &SharedState, track_count: usize) {
    let osc = state.session.osc();

    // Song-level listeners
    let _ = osc.send("/live/song/start_listen/tempo", &[]);
    let _ = osc.send("/live/song/start_listen/is_playing", &[]);

    // Per-track listeners: volume, panning, mute, solo
    for i in 0..track_count {
        let idx = Arg::Int(i as i32);
        let _ = osc.send("/live/track/start_listen/volume", &[idx.clone()]);
        let _ = osc.send("/live/track/start_listen/panning", &[idx.clone()]);
        let _ = osc.send("/live/track/start_listen/mute", &[idx.clone()]);
        let _ = osc.send("/live/track/start_listen/solo", &[idx.clone()]);
    }

    // Per-track device parameter listeners (device 0 = instrument)
    // Also populate the param name cache.
    {
        let mut cache = state.param_name_cache.lock().unwrap();
        for i in 0..track_count {
            let track = state.session.track(i as i32);
            let device = track.device(0);
            if let Ok(names) = device.parameter_names() {
                let param_count = names.len().min(64);
                for p in 0..param_count {
                    cache.insert((i as i32, 0, p as i32), names[p].clone());
                    let _ = osc.send(
                        "/live/device/start_listen/parameter/value",
                        &[Arg::Int(i as i32), Arg::Int(0), Arg::Int(p as i32)],
                    );
                }
            }
        }
    }

    eprintln!("listeners: registered for {} tracks", track_count);

    // Wait for initial value dump to drain, then enable listener dispatch
    thread::sleep(Duration::from_millis(500));
    state.listeners_ready.store(true, Ordering::Relaxed);
    eprintln!("listeners: ready");
}

/// Register listeners for a single track (used when tracks are added at runtime).
fn register_track_listeners(state: &SharedState, track_idx: i32) {
    let osc = state.session.osc();
    let idx = Arg::Int(track_idx);
    let _ = osc.send("/live/track/start_listen/volume", &[idx.clone()]);
    let _ = osc.send("/live/track/start_listen/panning", &[idx.clone()]);
    let _ = osc.send("/live/track/start_listen/mute", &[idx.clone()]);
    let _ = osc.send("/live/track/start_listen/solo", &[idx.clone()]);

    // Device 0 params
    let track = state.session.track(track_idx);
    let device = track.device(0);
    if let Ok(names) = device.parameter_names() {
        let mut cache = state.param_name_cache.lock().unwrap();
        for (p, name) in names.iter().enumerate().take(64) {
            cache.insert((track_idx, 0, p as i32), name.clone());
            let _ = osc.send(
                "/live/device/start_listen/parameter/value",
                &[Arg::Int(track_idx), Arg::Int(0), Arg::Int(p as i32)],
            );
        }
    }
}

/// Spawn the listener thread that receives AbletonOSC push callbacks
/// and translates them into StateEvents.
fn start_listener_thread(state: &Arc<SharedState>, shutdown: &Arc<AtomicBool>) {
    let rx = match state.session.osc().register_listener("/live/") {
        Some(rx) => rx,
        None => {
            eprintln!("listeners: transport doesn't support listeners");
            return;
        }
    };

    let state = state.clone();
    let shutdown = shutdown.clone();

    thread::Builder::new()
        .name("ableton-listener".into())
        .spawn(move || {
            // Track names by index (for events)
            let get_track_name = |idx: i32| -> String {
                let s = state.state.lock().unwrap();
                s.tracks
                    .get(idx as usize)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| format!("track-{idx}"))
            };

            loop {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                // Use recv_timeout so we can check shutdown periodically
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok((addr, args)) => {
                        dispatch_listener_callback(&state, &get_track_name, &addr, &args);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("failed to spawn listener thread");
}

/// Check if we recently broadcast the same event (dedup against semantic handlers).
fn should_suppress(state: &SharedState, key: &str) -> bool {
    let recent = state.recent_broadcasts.lock().unwrap();
    if let Some(t) = recent.get(key) {
        t.elapsed() < Duration::from_millis(100)
    } else {
        false
    }
}

/// Record a broadcast for dedup purposes.
fn record_broadcast(state: &SharedState, key: &str) {
    state
        .recent_broadcasts
        .lock()
        .unwrap()
        .insert(key.to_string(), Instant::now());
}

/// Translate an AbletonOSC push callback into a StateEvent and broadcast it.
fn dispatch_listener_callback(
    state: &SharedState,
    get_track_name: &dyn Fn(i32) -> String,
    addr: &str,
    args: &[Arg],
) {
    // Gate: suppress until initial value dump has drained
    if !state.listeners_ready.load(Ordering::Relaxed) {
        return;
    }
    // Track property callbacks: /live/track/get/<prop> <track_idx> <value>
    if let Some(prop) = addr.strip_prefix("/live/track/get/") {
        let track_idx = match args.first().and_then(|a| a.as_i32()) {
            Some(i) => i,
            None => return,
        };
        let track = get_track_name(track_idx);

        let dedup_key = format!("{track_idx}:{prop}");
        if should_suppress(state, &dedup_key) {
            return;
        }

        match prop {
            "volume" => {
                if let Some(v) = args.get(1).and_then(|a| a.as_f64()) {
                    state.state.lock().unwrap().apply_mix(track_idx, Some(v), None, None, None);
                    state.broadcaster.broadcast(&StateEvent::MixChanged {
                        track, track_idx, volume: Some(v), pan: None, mute: None, solo: None,
                    });
                }
            }
            "panning" => {
                if let Some(v) = args.get(1).and_then(|a| a.as_f64()) {
                    state.state.lock().unwrap().apply_mix(track_idx, None, Some(v), None, None);
                    state.broadcaster.broadcast(&StateEvent::MixChanged {
                        track, track_idx, volume: None, pan: Some(v), mute: None, solo: None,
                    });
                }
            }
            "mute" => {
                if let Some(v) = args.get(1).and_then(|a| a.as_bool()) {
                    state.state.lock().unwrap().apply_mix(track_idx, None, None, Some(v), None);
                    state.broadcaster.broadcast(&StateEvent::MixChanged {
                        track, track_idx, volume: None, pan: None, mute: Some(v), solo: None,
                    });
                }
            }
            "solo" => {
                if let Some(v) = args.get(1).and_then(|a| a.as_bool()) {
                    state.state.lock().unwrap().apply_mix(track_idx, None, None, None, Some(v));
                    state.broadcaster.broadcast(&StateEvent::MixChanged {
                        track, track_idx, volume: None, pan: None, mute: None, solo: Some(v),
                    });
                }
            }
            _ => {}
        }
        return;
    }

    // Device parameter callbacks: /live/device/get/parameter/value <track> <device> <param> <value>
    if addr == "/live/device/get/parameter/value" {
        if args.len() >= 4 {
            let track_idx = args[0].as_i32().unwrap_or(0);
            let device_idx = args[1].as_i32().unwrap_or(0);
            let param_idx = args[2].as_i32().unwrap_or(0);
            let value = args[3].as_f64().unwrap_or(0.0);

            let dedup_key = format!("{track_idx}:d{device_idx}:p{param_idx}");
            if should_suppress(state, &dedup_key) {
                return;
            }

            let track = get_track_name(track_idx);

            // Look up param name from cache, fall back to index
            let param_name = state
                .param_name_cache
                .lock()
                .unwrap()
                .get(&(track_idx, device_idx, param_idx))
                .cloned()
                .unwrap_or_else(|| format!("param-{param_idx}"));

            state.broadcaster.broadcast(&StateEvent::ParamChanged {
                track,
                track_idx,
                device: device_idx,
                param: param_name,
                value,
            });
        }
        return;
    }

    // Ignore the value_string callback (we only need numeric values)
    if addr == "/live/device/get/parameter/value_string" {
        return;
    }

    // Song-level callbacks
    if addr == "/live/song/get/tempo" {
        if let Some(tempo) = args.first().and_then(|a| a.as_f64()) {
            state.state.lock().unwrap().apply_tempo(tempo);
            state.broadcaster.broadcast(&StateEvent::TempoChanged { tempo });
        }
        return;
    }
    if addr == "/live/song/get/is_playing" {
        if let Some(playing) = args.first().and_then(|a| a.as_bool()) {
            state.state.lock().unwrap().playing = playing;
            state.broadcaster.broadcast(&StateEvent::TransportChanged { playing });
        }
    }
}

// ─── Daemon server ──────────────────────────────────────────────────────────

/// Start the daemon. Blocks the current process.
pub fn run_daemon() -> Result<()> {
    let session = Session::connect().map_err(|e| Error::Connection(e.to_string()))?;

    // Start with empty state — will be populated asynchronously
    let session_state = Arc::new(Mutex::new(SessionState {
        tempo: 120.0,
        playing: false,
        tracks: Vec::new(),
        returns: Vec::new(),
    }));

    // Start event broadcaster
    let broadcaster = EventBroadcaster::start(session_state.clone());

    let state = Arc::new(SharedState {
        session,
        track_cache: Mutex::new(HashMap::new()),
        clip_cache: Mutex::new(HashMap::new()),
        state: session_state,
        broadcaster,
        active_walks: Mutex::new(HashMap::new()),
        walk_counter: Mutex::new(0),
        recent_broadcasts: Mutex::new(HashMap::new()),
        listeners_ready: AtomicBool::new(false),
        param_name_cache: Mutex::new(HashMap::new()),
    });

    // Start listener immediately so the parent process sees us as running
    let sock = socket_path();
    let dir = socket_dir();
    fs::create_dir_all(&dir)?;

    if sock.exists() {
        fs::remove_file(&sock)?;
    }

    let listener = UnixListener::bind(&sock)?;
    eprintln!("mr daemon listening on {}", sock.display());

    fs::write(pid_path(), process::id().to_string())?;

    let shutdown = Arc::new(AtomicBool::new(false));

    // Warm caches, sync state, then register Ableton listeners
    {
        let state = state.clone();
        thread::spawn(move || {
            if let Err(e) = refresh_track_cache(&state) {
                eprintln!("daemon: track cache: {}", e);
            }
            let session_state = SessionState::from_session(&state.session);
            eprintln!(
                "state synced: {} tracks, {} returns, {:.0} BPM",
                session_state.tracks.len(),
                session_state.returns.len(),
                session_state.tempo
            );
            let track_count = session_state.tracks.len();
            *state.state.lock().unwrap() = session_state;

            // Register AbletonOSC push listeners for all known tracks
            register_ableton_listeners(&state, track_count);
        });
    }

    // Spawn listener thread to receive AbletonOSC push callbacks
    {
        let state = state.clone();
        let shutdown = shutdown.clone();
        start_listener_thread(&state, &shutdown);
    }

    // Periodic state poll — detect Ableton GUI changes every 3 seconds
    {
        let state = state.clone();
        let shutdown = shutdown.clone();
        thread::Builder::new()
            .name("state-poll".into())
            .spawn(move || {
                while !shutdown.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(3));
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    poll_ableton_state(&state);
                }
            })
            .expect("failed to spawn state poll thread");
    }

    listener.set_nonblocking(false)?;

    for stream in listener.incoming() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(stream) => {
                let state = state.clone();
                let shutdown = shutdown.clone();
                thread::spawn(move || {
                    handle_connection(&state, &shutdown, stream);
                });
            }
            Err(e) => {
                eprintln!("daemon: connection error: {}", e);
            }
        }
    }

    let _ = fs::remove_file(&sock);
    let _ = fs::remove_file(pid_path());
    state.broadcaster.shutdown();
    eprintln!("mr daemon stopped");
    Ok(())
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Check if the daemon is running.
pub fn is_running() -> bool {
    socket_path().exists() && UnixStream::connect(socket_path()).is_ok()
}

/// Send a request to the daemon and get the response.
pub fn send_request(req: &DaemonRequest) -> Result<DaemonResponse> {
    let stream = UnixStream::connect(socket_path())
        .map_err(|e| Error::Other(format!("cannot connect to daemon: {}", e)))?;

    let json = serde_json::to_string(req)?;
    let mut writer = &stream;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    // Shutdown the write half so daemon knows we're done
    stream.shutdown(std::net::Shutdown::Write)?;

    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let resp: DaemonResponse = serde_json::from_str(&line)?;
        return Ok(resp);
    }

    Err(Error::Other("no response from daemon".into()))
}

/// Start the daemon in the background.
pub fn start() -> Result<()> {
    if is_running() {
        eprintln!("daemon already running");
        return Ok(());
    }

    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            return Err(Error::Other("fork failed".into()));
        }
        if pid > 0 {
            thread::sleep(Duration::from_millis(500));
            if is_running() {
                eprintln!("daemon started (pid {})", pid);
            } else {
                eprintln!("daemon failed to start");
            }
            return Ok(());
        }
        libc::setsid();
    }

    if let Err(e) = run_daemon() {
        eprintln!("daemon error: {}", e);
        process::exit(1);
    }
    process::exit(0);
}

/// Stop the daemon.
pub fn stop() -> Result<()> {
    if !is_running() {
        eprintln!("daemon not running");
        return Ok(());
    }

    let resp = send_request(&DaemonRequest {
        cmd: "shutdown".into(),
        ..Default::default()
    })?;

    if resp.ok {
        eprintln!("daemon stopped");
        thread::sleep(Duration::from_millis(100));
        let _ = fs::remove_file(socket_path());
    } else {
        eprintln!("shutdown failed: {}", resp.error.unwrap_or_default());
    }
    Ok(())
}

/// Subscribe to the event stream (for debugging).
pub fn stream_events() -> Result<()> {
    use std::io::{BufRead, BufReader};

    let events_path = socket_dir().join("events.sock");
    if !events_path.exists() {
        return Err(Error::Other(
            "events socket not found — is the daemon running?".into(),
        ));
    }

    let stream = UnixStream::connect(&events_path)
        .map_err(|e| Error::Other(format!("cannot connect to events socket: {}", e)))?;

    eprintln!("connected to event stream (ctrl-c to stop)");

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        match line {
            Ok(l) => println!("{}", l),
            Err(_) => break,
        }
    }
    Ok(())
}

/// Query daemon status.
pub fn status() -> Result<()> {
    if !is_running() {
        println!("daemon: not running");
        return Ok(());
    }

    let resp = send_request(&DaemonRequest {
        cmd: "status".into(),
        ..Default::default()
    })?;

    println!("daemon: {}", resp.message.unwrap_or_else(|| "running".into()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_notes_deterministic() {
        let notes = vec![
            MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 },
            MrNote { pitch: 64, start: 1.0, duration: 1.0, velocity: 80 },
        ];
        assert_eq!(hash_notes(&notes), hash_notes(&notes));
    }

    #[test]
    fn test_hash_notes_different() {
        let notes1 = vec![MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 }];
        let notes2 = vec![MrNote { pitch: 61, start: 0.0, duration: 1.0, velocity: 100 }];
        assert_ne!(hash_notes(&notes1), hash_notes(&notes2));
    }

    #[test]
    fn test_socket_path() {
        assert!(socket_path().to_string_lossy().contains("mr.sock"));
    }

    #[test]
    fn test_daemon_request_roundtrip() {
        let req = DaemonRequest {
            cmd: "write".into(),
            track: "pad".into(),
            slot: 0,
            length: 16.0,
            name: "Melody".into(),
            notes: vec![MrNote { pitch: 60, start: 0.0, duration: 1.0, velocity: 100 }],
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cmd, "write");
        assert_eq!(parsed.track, "pad");
        assert_eq!(parsed.notes.len(), 1);
    }

    #[test]
    fn test_proxy_request_roundtrip() {
        let req = DaemonRequest {
            cmd: "query".into(),
            address: "/live/song/get/tempo".into(),
            osc_args: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cmd, "query");
        assert_eq!(parsed.address, "/live/song/get/tempo");
    }

    #[test]
    fn test_daemon_response_serialization() {
        let resp = DaemonResponse::ok_with_kind("wrote 5 notes", "notes-only");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("notes-only"));

        let resp = DaemonResponse::ok_with_result(vec![Arg::Float(120.0)]);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("120.0"));
        assert!(json.contains("result"));
    }
}
