//! DaemonTransport — routes OSC operations through the mr daemon's Unix socket.
//!
//! Implements `ableton::Transport`, so a `Session` backed by this transport
//! sends all OSC through the daemon instead of directly to Ableton.
//!
//! Uses pipelined request/response multiplexing: each request carries a
//! `request_id` that the daemon echoes back. A background reader thread
//! routes responses to the correct caller via oneshot channels.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ableton::{Arg, Transport};

use crate::daemon::{socket_path, BatchQueryEntry, DaemonRequest, DaemonResponse};

type PendingMap = HashMap<u64, std::sync::mpsc::Sender<DaemonResponse>>;

/// OSC transport that routes through the mr daemon.
/// Supports concurrent requests via request_id multiplexing.
pub struct DaemonTransport {
    writer: Mutex<UnixStream>,
    next_id: AtomicU64,
    pending: Arc<Mutex<PendingMap>>,
}

impl DaemonTransport {
    /// Connect to a running daemon.
    pub fn connect() -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path())?;
        let reader_stream = stream.try_clone()?;
        let writer = stream;

        let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        std::thread::Builder::new()
            .name("daemon-reader".into())
            .spawn(move || {
                Self::reader_loop(reader_stream, &pending_clone);
            })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        Ok(Self {
            writer: Mutex::new(writer),
            next_id: AtomicU64::new(1),
            pending,
        })
    }

    fn reader_loop(stream: UnixStream, pending: &Mutex<PendingMap>) {
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let resp: DaemonResponse = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let id = resp.request_id;
            let mut map = pending.lock().unwrap();
            if let Some(tx) = map.remove(&id) {
                let _ = tx.send(resp);
            } else if id == 0 {
                // Fallback for daemons that don't echo request_id:
                // route to the oldest pending request (FIFO)
                if let Some(&min_id) = map.keys().min() {
                    if let Some(tx) = map.remove(&min_id) {
                        let _ = tx.send(resp);
                    }
                }
            }
        }
        // Socket closed — wake all pending callers with error
        let mut map = pending.lock().unwrap();
        for (_, tx) in map.drain() {
            let _ = tx.send(DaemonResponse {
                ok: false,
                message: None,
                error: Some("daemon connection closed".into()),
                update_kind: None,
                result: None,
                results: None,
                request_id: 0,
            });
        }
    }

    fn request(&self, req: &mut DaemonRequest) -> ableton::Result<DaemonResponse> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        req.request_id = id;

        // Register response channel before sending
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let json = serde_json::to_string(req)
            .map_err(|e| ableton::Error::Ableton(format!("serialize: {}", e)))?;

        let send_result = {
            let mut writer = self.writer.lock().unwrap();
            writer
                .write_all(json.as_bytes())
                .and_then(|_| writer.write_all(b"\n"))
                .and_then(|_| writer.flush())
        };

        if let Err(e) = send_result {
            self.pending.lock().unwrap().remove(&id);
            return Err(e.into());
        }

        rx.recv().map_err(|_| {
            ableton::Error::Ableton("daemon response channel closed".into())
        })
    }
}

impl Transport for DaemonTransport {
    fn send(&self, address: &str, args: &[Arg]) -> ableton::Result<()> {
        let mut req = DaemonRequest {
            cmd: "send".into(),
            address: address.to_string(),
            osc_args: args.to_vec(),
            ..Default::default()
        };
        let resp = self.request(&mut req)?;
        if resp.ok {
            Ok(())
        } else {
            Err(ableton::Error::Ableton(
                resp.error.unwrap_or_else(|| "daemon send failed".into()),
            ))
        }
    }

    fn query_timeout(
        &self,
        address: &str,
        args: &[Arg],
        timeout: Duration,
    ) -> ableton::Result<Vec<Arg>> {
        let mut req = DaemonRequest {
            cmd: "query".into(),
            address: address.to_string(),
            osc_args: args.to_vec(),
            timeout_ms: Some(timeout.as_millis() as u64),
            ..Default::default()
        };
        let resp = self.request(&mut req)?;
        if resp.ok {
            Ok(resp.result.unwrap_or_default())
        } else {
            let msg = resp.error.unwrap_or_default();
            if msg.contains("timed out") {
                Err(ableton::Error::Timeout {
                    address: address.to_string(),
                })
            } else {
                Err(ableton::Error::Ableton(msg))
            }
        }
    }

    fn batch_query_timeout(
        &self,
        queries: &[(String, Vec<Arg>)],
        timeout: Duration,
    ) -> ableton::Result<Vec<Vec<Arg>>> {
        let mut req = DaemonRequest {
            cmd: "batch_query".into(),
            queries: queries
                .iter()
                .map(|(addr, args)| BatchQueryEntry {
                    address: addr.clone(),
                    args: args.clone(),
                })
                .collect(),
            timeout_ms: Some(timeout.as_millis() as u64),
            ..Default::default()
        };
        let resp = self.request(&mut req)?;
        if resp.ok {
            Ok(resp.results.unwrap_or_default())
        } else {
            Err(ableton::Error::Ableton(
                resp.error.unwrap_or_else(|| "batch query failed".into()),
            ))
        }
    }
}
