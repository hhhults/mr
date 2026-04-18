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

pub fn mosaic_feedback(corpus: &str, cols: u32, rows: u32) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "mosaic_feedback",
        "corpus": corpus,
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

// ─── Lerp ──────────────────────────────────────────────────────────────────

/// Smoothly interpolate a parameter to a target value over duration seconds.
pub fn set_lerp(name: &str, value: f64, duration: f64) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "set_param_lerp",
        "name": name,
        "value": value,
        "duration": duration,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("viz lerp {name} → {value:.3} over {duration:.1}s — {resp}");
    Ok(())
}

// ─── Triangle Stamp ────────────────────────────────────────────────────────

/// Tessellate a target image into textured triangles, render for N frames.
pub fn triangle_stamp(path: &str, density: usize, edge_bias: f64, frames: u32) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "triangle_stamp",
        "path": path,
        "density": density,
        "edge_bias": edge_bias,
        "frames": frames,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

/// Tessellate a target image into textured triangles (persistent).
pub fn triangle_mosaic_textured(path: &str, density: usize, edge_bias: f64) -> Result<()> {
    let json = serde_json::to_string(&serde_json::json!({
        "cmd": "triangle_mosaic_textured",
        "path": path,
        "density": density,
        "edge_bias": edge_bias,
    }))
    .unwrap();
    let resp = send_command(&json)?;
    eprintln!("{resp}");
    Ok(())
}

// ─── Random Transport ──────────────────────────────────────────────────────

fn pick_random_images(corpus_name: &str, n: usize) -> Result<Vec<PathBuf>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let corpus_dir = PathBuf::from(&home)
        .join("musictools")
        .join("visual-corpus")
        .join(corpus_name);

    if !corpus_dir.exists() {
        return Err(Error::Other(format!(
            "corpus dir not found: {}",
            corpus_dir.display()
        )));
    }

    let images: Vec<PathBuf> = std::fs::read_dir(&corpus_dir)
        .map_err(|e| Error::Other(format!("read dir: {e}")))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("jpg" | "jpeg" | "png")
            )
        })
        .collect();

    if images.len() < n {
        return Err(Error::Other(format!(
            "corpus '{}' has fewer than {} images",
            corpus_name, n
        )));
    }

    use rand::seq::IndexedRandom;
    let mut rng = rand::rng();
    Ok(images.choose_multiple(&mut rng, n).cloned().collect())
}

/// Pick 2 random images from a corpus directory and start mosaic transport.
pub fn random_transport(corpus_name: &str, cols: u32, rows: u32) -> Result<()> {
    let chosen = pick_random_images(corpus_name, 2)?;

    eprintln!(
        "random-transport: {} → {}",
        chosen[0].file_name().unwrap().to_string_lossy(),
        chosen[1].file_name().unwrap().to_string_lossy()
    );

    mosaic_transport(
        &chosen[0].to_string_lossy(),
        &chosen[1].to_string_lossy(),
        corpus_name,
        cols,
        rows,
    )
}

/// Pick a random image and show it as a textured triangle mosaic.
pub fn random_triangle(corpus_name: &str, density: usize, edge_bias: f64, frames: u32) -> Result<()> {
    let chosen = pick_random_images(corpus_name, 1)?;

    eprintln!(
        "random-triangle: {}",
        chosen[0].file_name().unwrap().to_string_lossy()
    );

    if frames > 0 {
        triangle_stamp(&chosen[0].to_string_lossy(), density, edge_bias, frames)
    } else {
        triangle_mosaic_textured(&chosen[0].to_string_lossy(), density, edge_bias)
    }
}
