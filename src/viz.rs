//! `mr viz` — control the visual engine via its command socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crate::error::{Error, Result};

fn viz_socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".mr").join("viz.sock")
}

fn send_command(json: &str) -> Result<String> {
    let path = viz_socket_path();
    if !path.exists() {
        return Err(Error::Other(
            "viz socket not found — is mr-viz running?".into(),
        ));
    }
    let mut stream =
        UnixStream::connect(&path).map_err(|e| Error::Other(format!("connect: {e}")))?;
    stream
        .write_all(format!("{json}\n").as_bytes())
        .map_err(|e| Error::Other(format!("write: {e}")))?;
    stream
        .flush()
        .map_err(|e| Error::Other(format!("flush: {e}")))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| Error::Other(format!("read: {e}")))?;
    Ok(line.trim().to_string())
}

/// Set a visual parameter.
pub fn set(name: &str, value: f64) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "set_param",
        "name": name,
        "value": value,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("viz set {name} = {value:.3} → {resp}");
    Ok(())
}

/// Get a visual parameter value.
pub fn get(name: &str) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "get_param",
        "name": name,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

/// List all visual parameters.
pub fn list() -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "list_params",
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

// ─── Corpus ─────────────────────────────────────────────────────────────────

pub fn corpus_load(name: &str) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "load_corpus",
        "name": name,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn corpus_unload(name: &str) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "unload_corpus",
        "name": name,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn corpus_list() -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "list_corpora",
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

// ─── Voice ──────────────────────────────────────────────────────────────────

pub fn voice_add(name: &str, corpus: &str) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "voice_add",
        "name": name,
        "corpus": corpus,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn voice_remove(name: &str) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "voice_remove",
        "name": name,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn voice_pattern(name: &str, atoms: &[f64], duration: f64) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "voice_pattern",
        "name": name,
        "atoms": atoms,
        "duration": duration,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn voice_param(name: &str, param: &str, signal: &str) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "voice_param",
        "name": name,
        "param": param,
        "signal": signal,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn voice_list() -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "list_voices",
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

// ─── Transport ──────────────────────────────────────────────────────────────

pub fn play() -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({ "cmd": "viz_play" })).unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn stop() -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({ "cmd": "viz_stop" })).unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn set_tempo(bpm: f64) -> Result<()> {
    let json =
        serde_json::to_string(&serde_json::json!({ "cmd": "viz_tempo", "bpm": bpm })).unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn mosaic_transport(
    path_a: &str,
    path_b: &str,
    corpus: &str,
    cols: u32,
    rows: u32,
) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "mosaic_transport",
        "corpus": corpus,
        "path_a": path_a,
        "path_b": path_b,
        "cols": cols,
        "rows": rows,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn transport_speed(speed: f64) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "transport_speed",
        "speed": speed,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn mosaic(path: &str, corpus: &str, cols: u32, rows: u32) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "mosaic",
        "corpus": corpus,
        "path": path,
        "cols": cols,
        "rows": rows,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn sync(enabled: &str) -> Result<()> {
    let on = matches!(enabled.to_lowercase().as_str(), "on" | "true" | "1" | "yes");
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "viz_sync",
        "enabled": on,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

// ─── Frame Pipe ─────────────────────────────────────────────────────────────

pub fn pipe_enable(frame_skip: u32) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "pipe_enable",
        "frame_skip": frame_skip
    })).unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn pipe_disable() -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({ "cmd": "pipe_disable" })).unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

pub fn pipe_status() -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({ "cmd": "pipe_status" })).unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}
