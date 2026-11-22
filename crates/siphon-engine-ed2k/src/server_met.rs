use crate::link::parse_ed2k_server_link;
use siphon_core::Ed2kServerEntry;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// Parse standard binary eMule/eDonkey `server.met` format.
///
/// Format:
/// - 1 byte version (`0x0E` or `0xE0`)
/// - 4 bytes LE server count
/// - Per server: 4 bytes IPv4, 2 bytes LE port, 4 bytes LE tag count, followed by tagged records
pub fn parse_server_met_bytes(data: &[u8]) -> Result<Vec<Ed2kServerEntry>, String> {
    if data.len() < 5 {
        return Err("server.met 数据长度不足".into());
    }
    let version = data[0];
    if version != 0x0E && version != 0xE0 {
        return Err(format!("不支持的 server.met 文件头标识: 0x{version:02X}"));
    }

    let count = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
    let mut pos = 5usize;
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for _ in 0..count.min(2000) {
        if pos + 10 > data.len() {
            break;
        }
        let ip = Ipv4Addr::new(data[pos], data[pos + 1], data[pos + 2], data[pos + 3]);
        pos += 4;
        let port = u16::from_le_bytes([data[pos], data[pos + 1]]);
        pos += 2;
        let tag_count = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;

        let mut server_name = None;

        for _ in 0..tag_count.min(128) {
            if pos >= data.len() {
                break;
            }
            let raw_type = data[pos];
            pos += 1;
            let short_tag = (raw_type & 0x80) != 0;
            let tag_type = raw_type & 0x7F;

            // Read tag name (1-byte special ID for short format or length-prefixed name for classic format)
            let special_id = if short_tag {
                if pos >= data.len() {
                    break;
                }
                let id = data[pos];
                pos += 1;
                Some(id)
            } else {
                if pos + 2 > data.len() {
                    break;
                }
                let name_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                if pos + name_len > data.len() {
                    break;
                }
                let id = if name_len == 1 { Some(data[pos]) } else { None };
                pos += name_len;
                id
            };

            // Decode tag value by type
            match tag_type {
                0x01 => {
                    // Hash16
                    if pos + 16 > data.len() {
                        break;
                    }
                    pos += 16;
                }
                0x02 => {
                    // String (u16 LE length + bytes)
                    if pos + 2 > data.len() {
                        break;
                    }
                    let slen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                    pos += 2;
                    if pos + slen > data.len() {
                        break;
                    }
                    if special_id == Some(0x01) {
                        let s = String::from_utf8_lossy(&data[pos..pos + slen])
                            .trim()
                            .to_string();
                        if !s.is_empty() {
                            server_name = Some(s);
                        }
                    }
                    pos += slen;
                }
                0x03 | 0x04 => {
                    // uint32 / float32
                    if pos + 4 > data.len() {
                        break;
                    }
                    pos += 4;
                }
                0x05 | 0x09 => {
                    // bool / uint8
                    if pos + 1 > data.len() {
                        break;
                    }
                    pos += 1;
                }
                0x08 => {
                    // uint16
                    if pos + 2 > data.len() {
                        break;
                    }
                    pos += 2;
                }
                0x0B => {
                    // uint64
                    if pos + 8 > data.len() {
                        break;
                    }
                    pos += 8;
                }
                0x07 => {
                    // Blob (u32 LE length)
                    if pos + 4 > data.len() {
                        break;
                    }
                    let blen =
                        u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                            as usize;
                    pos += 4;
                    if pos + blen > data.len() {
                        break;
                    }
                    pos += blen;
                }
                t if (0x11..=0x20).contains(&t) => {
                    // Compact inline string (length = t - 0x10)
                    let slen = (t - 0x10) as usize;
                    if pos + slen > data.len() {
                        break;
                    }
                    if special_id == Some(0x01) {
                        let s = String::from_utf8_lossy(&data[pos..pos + slen])
                            .trim()
                            .to_string();
                        if !s.is_empty() {
                            server_name = Some(s);
                        }
                    }
                    pos += slen;
                }
                _ => break,
            }
        }

        let host = ip.to_string();
        if port > 0 && !ip.is_unspecified() && seen.insert((host.clone(), port)) {
            out.push(Ed2kServerEntry {
                name: server_name.unwrap_or_else(|| format!("eD2k {host}:{port}")),
                host,
                port,
                enabled: true,
            });
        }
    }

    Ok(out)
}

/// Download a `server.met` (binary) or plain-text eD2k server list from `url`.
pub async fn sync_servers_from_met(
    url: &str,
    proxy_url: Option<&str>,
) -> Result<Vec<Ed2kServerEntry>, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("请输入 server.met 订阅地址".into());
    }

    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(format!("Siphon/{}", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = proxy_url
        && !proxy.trim().is_empty()
        && let Ok(p) = reqwest::Proxy::all(proxy.trim())
    {
        builder = builder.proxy(p);
    }
    let client = builder
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;

    let resp = client
        .get(trimmed)
        .send()
        .await
        .map_err(|e| format!("拉取 server.met 失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("server.met 返回 HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取 server.met 响应失败: {e}"))?;

    // Try binary server.met first, then fallback to plain-text `ed2k://|server|...` / `host:port` lines
    if let Ok(list) = parse_server_met_bytes(&bytes)
        && !list.is_empty()
    {
        return Ok(list);
    }

    let text = String::from_utf8_lossy(&bytes);
    let mut parsed = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        if let Some(entry) = parse_ed2k_server_link(line)
            && seen.insert((entry.host.clone(), entry.port))
        {
            parsed.push(entry);
        }
    }
    if parsed.is_empty() {
        Err("未能从订阅内容解析出有效的 eD2k 服务器条目".into())
    } else {
        Ok(parsed)
    }
}

/// Build a standard eD2k `OP_LOGINREQUEST` (`0xE3` protocol, opcode `0x01`) packet.
pub fn build_login_packet(nickname: &str, tcp_port: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(64);
    // Opcode: OP_LOGINREQUEST = 0x01
    payload.push(0x01);
    // 16-byte user hash (eMule v4 marker: byte 5 = 0x0E, byte 14 = 0x6F)
    let mut user_hash = [0x42u8; 16];
    user_hash[5] = 0x0E;
    user_hash[14] = 0x6F;
    payload.extend_from_slice(&user_hash);
    // Client ID (0 = requesting new ID)
    payload.extend_from_slice(&0u32.to_le_bytes());
    // Client TCP port
    payload.extend_from_slice(&tcp_port.to_le_bytes());
    // Tag count = 2 (Name + Version)
    payload.extend_from_slice(&2u32.to_le_bytes());

    // Tag 1: CT_NAME (type 0x02 string, name len 1, id 0x01)
    let nick_bytes = nickname.as_bytes();
    let nick_len = nick_bytes.len().min(64) as u16;
    payload.push(0x02);
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.push(0x01);
    payload.extend_from_slice(&nick_len.to_le_bytes());
    payload.extend_from_slice(&nick_bytes[..nick_len as usize]);

    // Tag 2: CT_VERSION (type 0x03 uint32, name len 1, id 0x11, value 0x3C)
    payload.push(0x03);
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.push(0x11);
    payload.extend_from_slice(&0x3Cu32.to_le_bytes());

    let mut packet = Vec::with_capacity(5 + payload.len());
    packet.push(0xE3); // OP_EDONKEYHEADER
    packet.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    packet.extend_from_slice(&payload);
    packet
}

/// Build an eD2k `OP_GETSOURCES` (`0x19`) query packet for a 16-byte MD4 file hash & file size.
pub fn build_get_sources_packet(md4_bytes: &[u8; 16], file_size: u64) -> Vec<u8> {
    let mut payload = Vec::with_capacity(25);
    payload.push(0x19); // OP_GETSOURCES
    payload.extend_from_slice(md4_bytes);
    payload.extend_from_slice(&(file_size.min(u32::MAX as u64) as u32).to_le_bytes());

    let mut packet = Vec::with_capacity(5 + payload.len());
    packet.push(0xE3);
    packet.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    packet.extend_from_slice(&payload);
    packet
}

/// Perform a real TCP connectivity + latency test against an eD2k server (`host:port`).
pub async fn test_ed2k_server(host: &str, port: u16, nickname: &str, tcp_port: u16) -> Result<u64, String> {
    let addr = format!("{host}:{port}");
    let start = Instant::now();
    let mut stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&addr))
        .await
        .map_err(|_| "连接超时 (5s)".to_string())?
        .map_err(|e| format!("无法连接: {e}"))?;

    let login = build_login_packet(nickname, tcp_port);
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.write_all(&login)).await;
    let latency_ms = start.elapsed().as_millis().max(1) as u64;
    Ok(latency_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::get;
    use tokio::net::TcpListener;

    fn sample_binary_server_met() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0x0E); // standard header
        buf.extend_from_slice(&2u32.to_le_bytes()); // 2 servers

        // Server 1: 45.82.80.155:5687, Name = "eMule Security"
        buf.extend_from_slice(&[45, 82, 80, 155]);
        buf.extend_from_slice(&5687u16.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes()); // 2 tags
        // Tag 1: name string
        let name1 = b"eMule Security";
        buf.push(0x02);
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.push(0x01);
        buf.extend_from_slice(&(name1.len() as u16).to_le_bytes());
        buf.extend_from_slice(name1);
        // Tag 2: dummy uint32 users count
        buf.push(0x03);
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.push(0x87);
        buf.extend_from_slice(&12345u32.to_le_bytes());

        // Server 2: 145.239.2.134:4661, short-format name tag = "GrupoTS"
        buf.extend_from_slice(&[145, 239, 2, 134]);
        buf.extend_from_slice(&4661u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        let name2 = b"GrupoTS";
        buf.push(0x80 | 0x02); // short string tag
        buf.push(0x01); // special_id = name
        buf.extend_from_slice(&(name2.len() as u16).to_le_bytes());
        buf.extend_from_slice(name2);

        buf
    }

    #[test]
    fn parses_binary_server_met_with_classic_and_short_tags() {
        let met = sample_binary_server_met();
        let servers = parse_server_met_bytes(&met).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "eMule Security");
        assert_eq!(servers[0].host, "45.82.80.155");
        assert_eq!(servers[0].port, 5687);
        assert_eq!(servers[1].name, "GrupoTS");
        assert_eq!(servers[1].host, "145.239.2.134");
        assert_eq!(servers[1].port, 4661);
    }

    #[tokio::test]
    async fn sync_servers_from_local_server_met_endpoint() {
        let met = sample_binary_server_met();
        let app = Router::new().route("/server.met", get(move || async move { met.clone() }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let servers = sync_servers_from_met(&format!("http://{addr}/server.met"), None)
            .await
            .unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "eMule Security");
    }

    #[tokio::test]
    async fn tcp_server_test_measures_latency_and_sends_ed2k_login_packet() {
        use tokio::io::AsyncReadExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut hdr = [0u8; 6];
            sock.read_exact(&mut hdr).await.unwrap();
            assert_eq!(hdr[0], 0xE3); // OP_EDONKEYHEADER
            assert_eq!(hdr[5], 0x01); // OP_LOGINREQUEST
        });

        let ms = test_ed2k_server("127.0.0.1", addr.port(), "SiphonTest", 4662)
            .await
            .unwrap();
        assert!(ms >= 1);
        server_task.await.unwrap();
    }
}
