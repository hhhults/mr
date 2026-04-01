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
