//! DaemonTransport — routes OSC operations through the mr daemon's Unix socket.
//!
//! Implements `ableton::Transport`, so a `Session` backed by this transport
//! sends all OSC through the daemon instead of directly to Ableton.

use std::io::{BufRead, BufReader, Write as IoWrite};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::Duration;

use ableton::{Arg, Transport};

use crate::daemon::{socket_path, BatchQueryEntry, DaemonRequest, DaemonResponse};

struct DaemonConn {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

/// OSC transport that routes through the mr daemon.
pub struct DaemonTransport {
    conn: Mutex<DaemonConn>,
}

impl DaemonTransport {
    /// Connect to a running daemon.
    pub fn connect() -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path())?;
        let writer = stream.try_clone()?;
        Ok(Self {
            conn: Mutex::new(DaemonConn {
                reader: BufReader::new(stream),
                writer,
            }),
        })
    }

    fn request(&self, req: &DaemonRequest) -> ableton::Result<DaemonResponse> {
        let mut conn = self.conn.lock().unwrap();
        let json = serde_json::to_string(req)
            .map_err(|e| ableton::Error::Ableton(format!("serialize: {}", e)))?;
        conn.writer.write_all(json.as_bytes())?;
        conn.writer.write_all(b"\n")?;
        conn.writer.flush()?;

        let mut line = String::new();
        conn.reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            return Err(ableton::Error::Ableton("empty response from daemon".into()));
        }
        serde_json::from_str(&line)
            .map_err(|e| ableton::Error::Ableton(format!("deserialize: {}", e)))
    }
}

impl Transport for DaemonTransport {
    fn send(&self, address: &str, args: &[Arg]) -> ableton::Result<()> {
        let req = DaemonRequest {
            cmd: "send".into(),
            address: address.to_string(),
            osc_args: args.to_vec(),
            ..Default::default()
        };
        let resp = self.request(&req)?;
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
        let req = DaemonRequest {
            cmd: "query".into(),
            address: address.to_string(),
            osc_args: args.to_vec(),
            timeout_ms: Some(timeout.as_millis() as u64),
            ..Default::default()
        };
        let resp = self.request(&req)?;
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
        let req = DaemonRequest {
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
        let resp = self.request(&req)?;
        if resp.ok {
            Ok(resp.results.unwrap_or_default())
        } else {
            Err(ableton::Error::Ableton(
                resp.error.unwrap_or_else(|| "batch query failed".into()),
            ))
        }
    }
}
