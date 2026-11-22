//! Siphon Chrome/Edge Native Messaging bridge with automatic desktop client launcher.
//!
//! Reads length-prefixed UTF-8 JSON frames from stdin, ensures Siphon (`siphon.exe`) is
//! running (auto-launching it detached if closed), forwards the JSON command to Siphon's
//! Windows Named Pipe (`\\.\pipe\siphon-ipc`), and writes a length-prefixed JSON reply
//! back to stdout.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const PIPE_NAME: &str = r"\\.\pipe\siphon-ipc";

#[derive(Debug, Serialize, Deserialize)]
struct BridgeReply {
    ok: bool,
    launched: bool,
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
    if len == 0 || len > 4 * 1024 * 1024 {
        return;
    }

    let mut payload = vec![0u8; len];
    if stdin.read_exact(&mut payload).await.is_err() {
        return;
    }

    let reply = forward_or_launch(&payload, PIPE_NAME).await;
    if let Ok(bytes) = serde_json::to_vec(&reply) {
        let out_len = (bytes.len() as u32).to_le_bytes();
        let _ = stdout.write_all(&out_len).await;
        let _ = stdout.write_all(&bytes).await;
        let _ = stdout.flush().await;
    }
}

/// Decide whether auto-launched `siphon.exe` should boot with `--minimized` (tray + standalone popup)
/// or show the full main window immediately.
fn should_launch_minimized(payload: &[u8]) -> bool {
    let Ok(val) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return false;
    };
    matches!(
        val.get("type").and_then(|v| v.as_str()),
        Some("browser_capture" | "browser_sniff")
    )
}

/// Normalize optional convenience `"launch"` command into `IpcCommand::ShowWindow`.
fn normalize_forward_payload(payload: &[u8]) -> Vec<u8> {
    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(payload)
        && val.get("type").and_then(|v| v.as_str()) == Some("launch")
    {
        return br#"{"type":"show_window"}"#.to_vec();
    }
    payload.to_vec()
}

fn locate_siphon_exe() -> Option<PathBuf> {
    // 1. Primary: `siphon.exe` right beside `siphon-native-host.exe`
    if let Ok(self_exe) = std::env::current_exe()
        && let Some(dir) = self_exe.parent()
    {
        let sibling = dir.join("siphon.exe");
        if sibling.is_file() {
            return Some(sibling);
        }
    }

    // 2. Fallback: standard per-user & system install paths
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("Programs").join("Siphon").join("siphon.exe");
        if p.is_file() {
            return Some(p);
        }
    }
    for env_key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(pf) = std::env::var(env_key) {
            let p = PathBuf::from(pf).join("Siphon").join("siphon.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(windows)]
fn spawn_siphon_detached(minimized: bool) -> std::io::Result<PathBuf> {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let exe = locate_siphon_exe().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "未找到 siphon.exe 主程序，请确认已安装 Siphon Downloader",
        )
    })?;

    let mut cmd = std::process::Command::new(&exe);
    if minimized {
        cmd.arg("--minimized");
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    cmd.spawn()?;
    Ok(exe)
}

#[cfg(windows)]
async fn write_to_named_pipe(pipe_name: &str, payload: &[u8]) -> Result<String, std::io::Error> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let mut client = ClientOptions::new().open(pipe_name)?;
    let mut framed = Vec::with_capacity(payload.len() + 1);
    framed.extend_from_slice(payload);
    framed.push(b'\n');
    client.write_all(&framed).await?;
    client.flush().await?;
    let mut resp = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(800), client.read_to_end(&mut resp)).await;
    Ok(String::from_utf8_lossy(&resp).trim().to_string())
}

#[cfg(windows)]
async fn forward_or_launch(payload: &[u8], pipe_name: &str) -> BridgeReply {
    let forward_bytes = normalize_forward_payload(payload);

    // 1. Fast path: Siphon is already running
    if let Ok(resp) = write_to_named_pipe(pipe_name, &forward_bytes).await {
        return BridgeReply {
            ok: true,
            launched: false,
            message: resp,
        };
    }

    // 2. Siphon is not running: launch detached & wait for Named Pipe IPC to come online
    let minimized = should_launch_minimized(payload);
    if let Err(e) = spawn_siphon_detached(minimized) {
        return BridgeReply {
            ok: false,
            launched: false,
            message: format!("无法启动 Siphon 客户端: {e}"),
        };
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        tokio::time::sleep(Duration::from_millis(120)).await;
        if let Ok(resp) = write_to_named_pipe(pipe_name, &forward_bytes).await {
            return BridgeReply {
                ok: true,
                launched: true,
                message: resp,
            };
        }
        if tokio::time::Instant::now() >= deadline {
            return BridgeReply {
                ok: false,
                launched: true,
                message: "已拉起 Siphon 客户端，但等待 IPC 就绪超时".to_string(),
            };
        }
    }
}

#[cfg(not(windows))]
async fn forward_or_launch(_payload: &[u8], _pipe_name: &str) -> BridgeReply {
    BridgeReply {
        ok: false,
        launched: false,
        message: "Named Pipe bridge is Windows-only".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimized_launch_policy_matches_popup_commands() {
        assert!(should_launch_minimized(
            br#"{"type":"browser_sniff","items":[]}"#
        ));
        assert!(should_launch_minimized(
            br#"{"type":"browser_capture","url":"https://example.com/a.zip"}"#
        ));
        assert!(!should_launch_minimized(br#"{"type":"show_window"}"#));
        assert!(!should_launch_minimized(br#"{"type":"launch"}"#));
    }

    #[test]
    fn launch_command_normalizes_to_show_window() {
        let out = normalize_forward_payload(br#"{"type":"launch"}"#);
        assert_eq!(out, br#"{"type":"show_window"}"#);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn forwards_payload_over_named_pipe() {
        use tokio::io::AsyncBufReadExt;
        use tokio::net::windows::named_pipe::ServerOptions;

        let pipe = format!(r"\\.\pipe\siphon-native-host-test-{}", std::process::id());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .unwrap();

        let server_task = tokio::spawn(async move {
            server.connect().await.unwrap();
            let mut reader = tokio::io::BufReader::new(server);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let mut inner = reader.into_inner();
            inner.write_all(b"{\"ok\":true}\n").await.unwrap();
            inner.flush().await.unwrap();
            line
        });

        let payload = br#"{"type":"browser_sniff","page_title":"Test","items":[]}"#;
        let reply = forward_or_launch(payload, &pipe).await;
        assert!(reply.ok);
        assert!(!reply.launched);
        let received = server_task.await.unwrap();
        assert!(received.contains("browser_sniff"));
    }
}
