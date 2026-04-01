//! Event subscription system — push state changes to connected subscribers.
//!
//! The daemon listens on a second Unix socket (`~/.mr/events.sock`).
//! When a subscriber connects, it receives:
//! 1. A snapshot of the current session state
//! 2. A stream of state change events as they happen

use std::io::Write as IoWrite;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, thread};

use serde::Serialize;

use crate::state::{SessionState, StateEvent};

fn events_socket_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home).join(".mr").join("events.sock")
}

/// Message sent to subscribers.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum SubscriberMessage<'a> {
    #[serde(rename = "snapshot")]
    Snapshot { state: &'a SessionState },
    #[serde(rename = "event")]
    Event { event: &'a StateEvent },
}

/// Manages subscriber connections and broadcasts events.
pub struct EventBroadcaster {
    subscribers: Arc<Mutex<Vec<UnixStream>>>,
}

impl EventBroadcaster {
    /// Start the event listener on a background thread.
    /// Returns a broadcaster handle for pushing events.
    pub fn start(state: Arc<Mutex<SessionState>>) -> Arc<Self> {
        let broadcaster = Arc::new(Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
        });

        let sock = events_socket_path();
        if sock.exists() {
            let _ = fs::remove_file(&sock);
        }

        let subs = broadcaster.subscribers.clone();
        let state = state.clone();

        thread::Builder::new()
            .name("event-listener".into())
            .spawn(move || {
                let listener = match UnixListener::bind(&sock) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("events: failed to bind {}: {}", sock.display(), e);
                        return;
                    }
                };
                eprintln!("events: listening on {}", sock.display());

                for stream in listener.incoming() {
                    match stream {
                        Ok(mut stream) => {
                            // Send snapshot on connect
                            let snapshot = state.lock().unwrap().clone();
                            let msg = SubscriberMessage::Snapshot { state: &snapshot };
                            if let Ok(json) = serde_json::to_string(&msg) {
                                let _ = stream.write_all(json.as_bytes());
                                let _ = stream.write_all(b"\n");
                                let _ = stream.flush();
                            }

                            // Set a short write timeout so we don't block on slow subscribers
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));

                            subs.lock().unwrap().push(stream);
                            eprintln!("events: subscriber connected");
                        }
                        Err(e) => {
                            eprintln!("events: accept error: {}", e);
                        }
                    }
                }
            })
            .expect("failed to spawn event listener thread");

        broadcaster
    }

    /// Broadcast an event to all connected subscribers.
    /// Disconnected subscribers are removed.
    pub fn broadcast(&self, event: &StateEvent) {
        let msg = SubscriberMessage::Event { event };
        let json = match serde_json::to_string(&msg) {
            Ok(j) => j,
            Err(_) => return,
        };
        let line = format!("{}\n", json);

        let mut subs = self.subscribers.lock().unwrap();
        subs.retain(|mut stream| {
            stream.write_all(line.as_bytes()).is_ok() && stream.flush().is_ok()
        });
    }

    /// Number of connected subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.lock().unwrap().len()
    }

    /// Cleanup the socket file.
    pub fn shutdown(&self) {
        let _ = fs::remove_file(events_socket_path());
    }
}
