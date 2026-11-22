//! Siphon Chrome/Edge Native Messaging bridge (Phase 3 ready).
//!
//! Reads length-prefixed UTF-8 JSON frames from stdin, forwards them to Siphon's
//! Windows Named Pipe (`\\.\pipe\siphon-ipc`), and writes JSON responses back to stdout.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const PIPE_NAME: &str = r"\\.\pipe\siphon-ipc";

#[derive(Debug, Serialize, Deserialize)]
struct BridgeReply {
    ok: bool,
    message: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let mut len_buf = [0u8; 4];
    if stdin.read_exact(&mut len_buf).await.is_err() {
        return;
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 1024 * 1024 {
        return;
    }

    let mut payload = vec![0u8; len];
    if stdin.read_exact(&mut payload).await.is_err() {
        return;
    }

    let reply = forward_to_pipe(&payload).await;
    if let Ok(bytes) = serde_json::to_vec(&reply) {
        let out_len = (bytes.len() as u32).to_le_bytes();
        let _ = stdout.write_all(&out_len).await;
        let _ = stdout.write_all(&bytes).await;
        let _ = stdout.flush().await;
    }
}

#[cfg(windows)]
async fn forward_to_pipe(payload: &[u8]) -> BridgeReply {
    use tokio::net::windows::named_pipe::ClientOptions;

    match ClientOptions::new().open(PIPE_NAME) {
        Ok(mut client) => {
            let mut framed = Vec::with_capacity(payload.len() + 1);
            framed.extend_from_slice(payload);
            framed.push(b'\n');
            if let Err(e) = client.write_all(&framed).await {
                return BridgeReply {
                    ok: false,
                    message: format!("IPC write failed: {e}"),
                };
            }
            let _ = client.flush().await;
            let mut resp = Vec::new();
            let _ = client.read_to_end(&mut resp).await;
            BridgeReply {
                ok: true,
                message: String::from_utf8_lossy(&resp).trim().to_string(),
            }
        }
        Err(e) => BridgeReply {
            ok: false,
            message: format!("Siphon is not running or IPC unavailable: {e}"),
        },
    }
}

#[cfg(not(windows))]
async fn forward_to_pipe(_payload: &[u8]) -> BridgeReply {
    BridgeReply {
        ok: false,
        message: "Named Pipe bridge is Windows-only".to_string(),
    }
}
