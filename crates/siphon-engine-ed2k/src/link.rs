use percent_encoding::percent_decode_str;
use siphon_core::{Ed2kServerEntry, Result, SiphonError, sanitize_filename};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kFileLink {
    pub filename: String,
    pub file_size: u64,
    /// Uppercase 32-character hexadecimal MD4 root hash.
    pub md4_hash: String,
    /// Optional AICH root hash (`h=...`).
    pub aich_hash: Option<String>,
    /// Optional HTTP/HTTPS web-seed URLs (`s=http://...`).
    pub web_sources: Vec<String>,
    /// Optional explicit peer endpoints (`sources,ip:port,...`).
    pub peer_sources: Vec<(String, u16)>,
}

pub fn is_ed2k_file_link(raw: &str) -> bool {
    raw.trim().to_ascii_lowercase().starts_with("ed2k://|file|")
}

/// Parse an `ed2k://|file|<name>|<size>|<md4_hash>|[options]|/` link.
pub fn parse_ed2k_file_link(raw: &str) -> Result<Ed2kFileLink> {
    let trimmed = raw.trim();
    if !is_ed2k_file_link(trimmed) {
        return Err(SiphonError::InvalidUrl(format!(
            "无效的 eD2k 文件链接: {trimmed}"
        )));
    }

    let parts: Vec<&str> = trimmed.split('|').collect();
    if parts.len() < 5 {
        return Err(SiphonError::InvalidUrl(
            "eD2k 文件链接缺少必要字段（需要文件名、大小、MD4 哈希）".into(),
        ));
    }

    let decoded_name = percent_decode_str(parts[2]).decode_utf8_lossy();
    let filename = sanitize_filename(&decoded_name);

    let file_size = parts[3]
        .trim()
        .parse::<u64>()
        .map_err(|_| SiphonError::InvalidUrl(format!("无效的 eD2k 文件大小: {}", parts[3])))?;

    let md4_raw = parts[4].trim().to_ascii_uppercase();
    if md4_raw.len() != 32 || !md4_raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SiphonError::InvalidUrl(format!(
            "无效的 eD2k MD4 哈希（应为 32 位十六进制）: {}",
            parts[4]
        )));
    }

    let mut aich_hash = None;
    let mut web_sources = Vec::new();
    let mut peer_sources = Vec::new();

    for extra in &parts[5..] {
        let token = extra.trim();
        if token.is_empty() || token == "/" {
            continue;
        }
        if let Some(rest) = token.strip_prefix("h=").or_else(|| token.strip_prefix("H=")) {
            if !rest.trim().is_empty() {
                aich_hash = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = token.strip_prefix("s=").or_else(|| token.strip_prefix("S=")) {
            let url = rest.trim();
            if url.starts_with("http://") || url.starts_with("https://") {
                web_sources.push(url.to_string());
            }
        } else if let Some(rest) = token
            .strip_prefix("sources,")
            .or_else(|| token.strip_prefix("SOURCES,"))
        {
            for ep in rest.split(',') {
                if let Some((host, port_s)) = ep.trim().rsplit_once(':')
                    && let Ok(port) = port_s.parse::<u16>()
                    && !host.is_empty()
                    && port > 0
                {
                    peer_sources.push((host.to_string(), port));
                }
            }
        }
    }

    Ok(Ed2kFileLink {
        filename,
        file_size,
        md4_hash: md4_raw,
        aich_hash,
        web_sources,
        peer_sources,
    })
}

/// Extract and parse all valid `ed2k://|file|...|/` links from arbitrary multi-line text,
/// clipboard content, or forum snippets, deduplicated by MD4 hash.
pub fn extract_ed2k_file_links(raw_text: &str) -> Vec<(String, Ed2kFileLink)> {
    let mut results = Vec::new();
    let mut seen_hashes = std::collections::HashSet::new();
    let lower = raw_text.to_ascii_lowercase();
    let needle = "ed2k://|file|";
    let mut cursor = 0usize;

    while let Some(rel) = lower[cursor..].find(needle) {
        let start = cursor + rel;
        let rest = &raw_text[start..];
        let end_offset = rest
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`' | ')' | ']' | '}')
            })
            .unwrap_or(rest.len());
        let mut candidate = rest[..end_offset].trim();
        if let Some(slash_pos) = candidate.rfind("|/") {
            candidate = &candidate[..slash_pos + 2];
        }
        if let Ok(parsed) = parse_ed2k_file_link(candidate)
            && seen_hashes.insert(parsed.md4_hash.clone())
        {
            results.push((candidate.to_string(), parsed));
        }
        cursor = start + end_offset.max(needle.len());
        if cursor >= raw_text.len() {
            break;
        }
    }

    results
}

/// Parse an `ed2k://|server|<host>|<port>|/` link or plain `host:port` string into an `Ed2kServerEntry`.
pub fn parse_ed2k_server_link(raw: &str) -> Option<Ed2kServerEntry> {
    let trimmed = raw.trim();
    if trimmed.to_ascii_lowercase().starts_with("ed2k://|server|") {
        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.len() >= 4 {
            let host = parts[2].trim();
            let port = parts[3].trim().parse::<u16>().ok()?;
            if !host.is_empty() && port > 0 {
                return Some(Ed2kServerEntry {
                    name: format!("eD2k {host}:{port}"),
                    host: host.to_string(),
                    port,
                    enabled: true,
                });
            }
        }
        return None;
    }

    if let Some((host, port_s)) = trimmed.rsplit_once(':')
        && let Ok(port) = port_s.trim().parse::<u16>()
        && !host.trim().is_empty()
        && !host.contains('/')
        && port > 0
    {
        let h = host.trim();
        return Some(Ed2kServerEntry {
            name: format!("eD2k {h}:{port}"),
            host: h.to_string(),
            port,
            enabled: true,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_ed2k_file_link_with_web_seed_aich_and_peers() {
        let raw = "ed2k://|file|Ubuntu%2024.04.iso|1048576|31d6cfe0d16ae931b73c59d7e0c089c0|h=ABCDEF123456|s=https://mirror.example.com/ubuntu.iso|sources,10.0.0.1:4662,10.0.0.2:4665|/";
        let parsed = parse_ed2k_file_link(raw).unwrap();
        assert_eq!(parsed.filename, "Ubuntu 24.04.iso");
        assert_eq!(parsed.file_size, 1_048_576);
        assert_eq!(parsed.md4_hash, "31D6CFE0D16AE931B73C59D7E0C089C0");
        assert_eq!(parsed.aich_hash.as_deref(), Some("ABCDEF123456"));
        assert_eq!(
            parsed.web_sources,
            vec!["https://mirror.example.com/ubuntu.iso"]
        );
        assert_eq!(
            parsed.peer_sources,
            vec![("10.0.0.1".to_string(), 4662), ("10.0.0.2".to_string(), 4665)]
        );
    }

    #[test]
    fn parses_ed2k_server_link_and_host_port() {
        let s1 = parse_ed2k_server_link("ed2k://|server|45.82.80.155|5687|/").unwrap();
        assert_eq!(s1.host, "45.82.80.155");
        assert_eq!(s1.port, 5687);

        let s2 = parse_ed2k_server_link("145.239.2.134:4661").unwrap();
        assert_eq!(s2.host, "145.239.2.134");
        assert_eq!(s2.port, 4661);
    }

    #[test]
    fn extracts_multiple_ed2k_file_links_from_mixed_text_and_deduplicates() {
        let blob = r#"
            第一集: ed2k://|file|Show.EP01.mkv|104857600|31D6CFE0D16AE931B73C59D7E0C089C0|/
            重复链接: ed2k://|file|Show.EP01.dup.mkv|104857600|31d6cfe0d16ae931b73c59d7e0c089c0|/
            第二集: "ed2k://|file|Show.EP02.mkv|209715200|0123456789ABCDEF0123456789ABCDEF|s=https://cdn.example/ep2.mkv|/"
            无效链接: ed2k://|file|Broken.mkv|abc|short|/
        "#;
        let links = extract_ed2k_file_links(blob);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].1.filename, "Show.EP01.mkv");
        assert_eq!(links[0].1.file_size, 104_857_600);
        assert_eq!(links[1].1.filename, "Show.EP02.mkv");
        assert_eq!(links[1].1.web_sources, vec!["https://cdn.example/ep2.mkv"]);
    }
}
