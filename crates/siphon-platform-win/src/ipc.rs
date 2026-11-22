use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub const SIPHON_PIPE_NAME: &str = r"\\.\pipe\siphon-ipc";
pub const DEFAULT_BROWSER_BRIDGE_PORT: u16 = 16800;

/// Rich download metadata captured from Chrome/Edge by the Siphon Browser Extension.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowserCapturePayload {
    pub url: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// When `true`, skip the capture confirmation popup and start downloading immediately in the background.
    #[serde(default)]
    pub silent: bool,
}

impl BrowserCapturePayload {
    /// Merge `referrer`, `cookie`, and `user_agent` into a unified HTTP header map.
    pub fn merged_headers(&self) -> BTreeMap<String, String> {
        let mut map = self.headers.clone();
        if let Some(ref r) = self.referrer
            && !r.trim().is_empty()
        {
            map.entry("Referer".to_string())
                .or_insert_with(|| r.trim().to_string());
        }
        if let Some(ref c) = self.cookie
            && !c.trim().is_empty()
        {
            map.entry("Cookie".to_string())
                .or_insert_with(|| c.trim().to_string());
        }
        if let Some(ref ua) = self.user_agent
            && !ua.trim().is_empty()
        {
            map.entry("User-Agent".to_string())
                .or_insert_with(|| ua.trim().to_string());
        }
        map
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcCommand {
    ShowWindow,
    AddDownload {
        url: String,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        from_browser: bool,
    },
    BrowserCapture(BrowserCapturePayload),
    Ping,
}

/// If another Siphon instance is already running, send `ShowWindow` (or `AddDownload`)
/// over Named Pipe and return `true`.
#[cfg(windows)]
pub fn try_notify_existing_instance(initial_url: Option<String>) -> bool {
    use std::fs::OpenOptions;
    use std::io::Write;

    let Ok(mut pipe) = OpenOptions::new().read(true).write(true).open(SIPHON_PIPE_NAME) else {
        return false;
    };

    let cmd = match initial_url {
        Some(url) => IpcCommand::AddDownload {
            url,
            filename: None,
            headers: BTreeMap::new(),
            from_browser: false,
        },
        None => IpcCommand::ShowWindow,
    };

    if let Ok(mut json) = serde_json::to_vec(&cmd) {
        json.push(b'\n');
        let _ = pipe.write_all(&json);
        let _ = pipe.flush();
    }
    true
}

#[cfg(not(windows))]
pub fn try_notify_existing_instance(_initial_url: Option<String>) -> bool {
    false
}

/// Spawn the Windows Named Pipe IPC server accepting commands from secondary instances
/// and `siphon-native-host` (Chrome/Edge Native Messaging).
#[cfg(windows)]
pub fn spawn_ipc_server(tx: mpsc::UnboundedSender<IpcCommand>) {
    use tokio::net::windows::named_pipe::ServerOptions;

    tokio::spawn(async move {
        let mut first = true;
        loop {
            let mut opts = ServerOptions::new();
            if first {
                opts.first_pipe_instance(true);
                first = false;
            }
            let server = match opts.create(SIPHON_PIPE_NAME) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "创建 Siphon Named Pipe IPC 失败");
                    break;
                }
            };

            if server.connect().await.is_err() {
                continue;
            }

            let tx_conn = tx.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(server);
                let mut line = String::new();
                if let Ok(n) = reader.read_line(&mut line).await
                    && n > 0
                {
                    let reply = match serde_json::from_str::<IpcCommand>(line.trim()) {
                        Ok(cmd) => {
                            let _ = tx_conn.send(cmd);
                            r#"{"ok":true}"#
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "忽略非法 IPC 消息");
                            r#"{"ok":false,"error":"invalid_json"}"#
                        }
                    };
                    let mut inner = reader.into_inner();
                    let _ = inner.write_all(reply.as_bytes()).await;
                    let _ = inner.flush().await;
                }
            });
        }
    });
}

#[cfg(not(windows))]
pub fn spawn_ipc_server(_tx: mpsc::UnboundedSender<IpcCommand>) {}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Option<(String, Vec<u8>)> {
    const MAX_REQ_BYTES: usize = 512 * 1024;
    let mut data = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];

    let (hdr_len, sep_len) = loop {
        if let Some(pos) = find_header_end(&data) {
            break pos;
        }
        if data.len() >= MAX_REQ_BYTES {
            return None;
        }
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut tmp))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            if data.is_empty() {
                return None;
            }
            break (data.len(), 0);
        }
        data.extend_from_slice(&tmp[..n]);
    };

    let headers_str = String::from_utf8_lossy(&data[..hdr_len]).into_owned();
    let body_start = hdr_len + sep_len;

    let mut content_length: usize = 0;
    let mut expects_continue = false;
    for line in headers_str.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim();
            let val = v.trim();
            if key.eq_ignore_ascii_case("content-length") {
                content_length = val.parse::<usize>().unwrap_or(0).min(MAX_REQ_BYTES);
            } else if key.eq_ignore_ascii_case("expect")
                && val.eq_ignore_ascii_case("100-continue")
            {
                expects_continue = true;
            }
        }
    }

    if expects_continue && data.len() < body_start + content_length {
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await;
        let _ = stream.flush().await;
    }

    let target_total = body_start.saturating_add(content_length);
    while data.len() < target_total && data.len() < MAX_REQ_BYTES {
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut tmp))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&tmp[..n]);
    }

    let body_end = data.len().min(target_total);
    let body = if body_start <= body_end {
        data[body_start..body_end].to_vec()
    } else {
        Vec::new()
    };
    Some((headers_str, body))
}

fn find_header_end(buf: &[u8]) -> Option<(usize, usize)> {
    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some((i, 4));
    }
    if let Some(i) = buf.windows(2).position(|w| w == b"\n\n") {
        return Some((i, 2));
    }
    None
}

/// Spawn a lightweight local loopback HTTP server (`127.0.0.1:16800`) so the Chrome/Edge
/// browser extension can send intercepted downloads (with cookies, Referer, filename, size)
/// with zero Windows registry configuration.
pub fn spawn_browser_http_bridge(
    port: u16,
    enabled_flag: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<IpcCommand>,
) {
    let bind_port = if port == 0 {
        DEFAULT_BROWSER_BRIDGE_PORT
    } else {
        port
    };

    tokio::spawn(async move {
        let addr = format!("127.0.0.1:{bind_port}");
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => {
                tracing::info!("Siphon 浏览器捕获本地桥接已启动: http://{addr}");
                l
            }
            Err(e) => {
                tracing::warn!(error = %e, "无法绑定浏览器捕获端口 {addr}");
                return;
            }
        };

        loop {
            let Ok((mut stream, _peer)) = listener.accept().await else {
                continue;
            };
            let tx_conn = tx.clone();
            let flag = Arc::clone(&enabled_flag);

            tokio::spawn(async move {
                let Some((headers_str, body_bytes)) = read_http_request(&mut stream).await else {
                    return;
                };
                let first_line = headers_str.lines().next().unwrap_or("");
                let mut parts = first_line.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("");

                if method.eq_ignore_ascii_case("OPTIONS") {
                    let resp = concat!(
                        "HTTP/1.1 204 No Content\r\n",
                        "Access-Control-Allow-Origin: *\r\n",
                        "Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n",
                        "Access-Control-Allow-Headers: Content-Type, X-Siphon-Client\r\n",
                        "Access-Control-Allow-Private-Network: true\r\n",
                        "Content-Length: 0\r\n",
                        "Connection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.flush().await;
                    return;
                }

                let is_enabled = flag.load(Ordering::Relaxed);

                let (status, body) = if method.eq_ignore_ascii_case("GET")
                    && (path == "/api/ping" || path == "/ping")
                {
                    let _ = tx_conn.send(IpcCommand::Ping);
                    (
                        "200 OK",
                        format!(
                            r#"{{"ok":true,"app":"Siphon","enabled":{}}}"#,
                            if is_enabled { "true" } else { "false" }
                        ),
                    )
                } else if method.eq_ignore_ascii_case("POST")
                    && (path == "/api/capture" || path == "/capture")
                {
                    if !is_enabled {
                        (
                            "200 OK",
                            r#"{"ok":false,"captured":false,"reason":"disabled"}"#.to_string(),
                        )
                    } else {
                        let json_str = String::from_utf8_lossy(&body_bytes);
                        let json_body = json_str.trim_matches(char::from(0)).trim();

                        match serde_json::from_str::<BrowserCapturePayload>(json_body) {
                            Ok(payload)
                                if payload.url.starts_with("http://")
                                    || payload.url.starts_with("https://") =>
                            {
                                let _ = tx_conn.send(IpcCommand::BrowserCapture(payload));
                                ("200 OK", r#"{"ok":true,"captured":true}"#.to_string())
                            }
                            _ => (
                                "400 Bad Request",
                                r#"{"ok":false,"captured":false,"reason":"invalid_payload"}"#
                                    .to_string(),
                            ),
                        }
                    }
                } else {
                    ("404 Not Found", r#"{"ok":false}"#.to_string())
                };

                let http_resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Private-Network: true\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(http_resp.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;

    #[test]
    fn merged_headers_folds_referrer_cookie_and_user_agent() {
        let payload = BrowserCapturePayload {
            url: "https://example.com/archive.zip".to_string(),
            filename: Some("archive.zip".to_string()),
            referrer: Some("https://example.com/page".to_string()),
            cookie: Some("sid=123; token=abc".to_string()),
            user_agent: Some("Mozilla/5.0".to_string()),
            mime: None,
            file_size: Some(1024),
            headers: BTreeMap::new(),
            silent: false,
        };
        let h = payload.merged_headers();
        assert_eq!(h.get("Referer").map(String::as_str), Some("https://example.com/page"));
        assert_eq!(h.get("Cookie").map(String::as_str), Some("sid=123; token=abc"));
        assert_eq!(h.get("User-Agent").map(String::as_str), Some("Mozilla/5.0"));
    }

    #[tokio::test]
    async fn local_http_bridge_accepts_browser_capture_post() {
        let enabled = Arc::new(AtomicBool::new(true));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let port = 26819u16;
        spawn_browser_http_bridge(port, Arc::clone(&enabled), tx);
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;

        let body = r#"{"url":"https://example.com/setup.exe","filename":"setup.exe","referrer":"https://example.com/","file_size":4096}"#;
        let req = format!(
            "POST /api/capture HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut client = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("connect bridge");
        client.write_all(req.as_bytes()).await.expect("write post");
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.expect("read response");
        let resp_str = String::from_utf8_lossy(&resp);
        assert!(resp_str.contains(r#""captured":true"#));

        let cmd = rx.recv().await.expect("ipc command received");
        match cmd {
            IpcCommand::BrowserCapture(p) => {
                assert_eq!(p.url, "https://example.com/setup.exe");
                assert_eq!(p.filename.as_deref(), Some("setup.exe"));
                assert_eq!(p.file_size, Some(4096));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn local_http_bridge_accepts_split_tcp_post_like_chromium_fetch() {
        let enabled = Arc::new(AtomicBool::new(true));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let port = 26820u16;
        spawn_browser_http_bridge(port, Arc::clone(&enabled), tx);
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;

        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("connect to local browser bridge");
        let body = r#"{"url":"https://release-assets.githubusercontent.com/app.exe","filename":"app.exe","file_size":4096000}"#;
        let headers = format!(
            "POST /api/capture HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        // Send headers first, pause, then send JSON body (exact Chromium fetch behavior!)
        stream.write_all(headers.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        stream.write_all(body.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();

        let mut resp = Vec::new();
        stream.read_to_end(&mut resp).await.unwrap();
        let resp_str = String::from_utf8_lossy(&resp);
        assert!(resp_str.contains("200 OK"), "unexpected response: {resp_str}");
        assert!(resp_str.contains(r#""captured":true"#));

        let cmd = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for BrowserCapture")
            .expect("channel closed");
        match cmd {
            IpcCommand::BrowserCapture(p) => {
                assert_eq!(p.url, "https://release-assets.githubusercontent.com/app.exe");
                assert_eq!(p.filename.as_deref(), Some("app.exe"));
            }
            other => panic!("expected BrowserCapture, got {other:?}"),
        }
    }
}
