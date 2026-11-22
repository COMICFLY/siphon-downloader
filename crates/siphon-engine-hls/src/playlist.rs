use siphon_core::{Result, SiphonError};
use url::Url;

#[derive(Debug, Clone)]
pub struct HlsKeySpec {
    pub uri: String,
    /// Optional explicit 16-byte IV from `IV=0x...`; when `None`, RFC 8216 uses big-endian media sequence.
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, Clone)]
pub struct HlsSegment {
    pub index: usize,
    pub sequence: u64,
    pub url: String,
    pub duration_secs: f64,
    pub key: Option<HlsKeySpec>,
}

#[derive(Debug, Clone)]
pub struct HlsMediaPlaylist {
    pub init_map_url: Option<String>,
    pub segments: Vec<HlsSegment>,
    pub total_duration_secs: f64,
    pub is_fmp4: bool,
}

#[derive(Debug, Clone)]
pub enum ParsedPlaylist {
    Master { best_variant_url: String, variants_count: usize },
    Media(HlsMediaPlaylist),
}

pub fn is_m3u8_url(raw: &str) -> bool {
    let Ok(u) = Url::parse(raw.trim()) else {
        return false;
    };
    let scheme = u.scheme();
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    u.path().to_ascii_lowercase().ends_with(".m3u8")
}

/// Parse an M3U8 playlist (either Master Playlist with `#EXT-X-STREAM-INF` or Media Playlist
/// with `#EXTINF`, `#EXT-X-MAP`, `#EXT-X-KEY`, `#EXT-X-MEDIA-SEQUENCE`).
pub fn parse_m3u8(base_url: &Url, content: &str) -> Result<ParsedPlaylist> {
    let lines: Vec<&str> = content.lines().map(str::trim).collect();

    // Check if Master Playlist (`#EXT-X-STREAM-INF`)
    let mut best_bw: u64 = 0;
    let mut best_url: Option<String> = None;
    let mut variants = 0usize;
    let mut pending_bw: Option<u64> = None;

    for line in &lines {
        if line.is_empty() {
            continue;
        }
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let bw = parse_attr(attrs, "BANDWIDTH")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            pending_bw = Some(bw);
        } else if !line.starts_with('#') && let Some(bw) = pending_bw.take() {
            let resolved = resolve_url(base_url, line)?;
            variants += 1;
            if best_url.is_none() || bw >= best_bw {
                best_bw = bw;
                best_url = Some(resolved);
            }
        }
    }

    if let Some(best_variant_url) = best_url {
        return Ok(ParsedPlaylist::Master {
            best_variant_url,
            variants_count: variants,
        });
    }

    // Parse Media Playlist
    let mut media_seq: u64 = 0;
    let mut current_key: Option<HlsKeySpec> = None;
    let mut init_map_url: Option<String> = None;
    let mut pending_dur: Option<f64> = None;
    let mut segments = Vec::new();
    let mut total_duration_secs = 0.0f64;

    for line in &lines {
        if line.is_empty() {
            continue;
        }
        if let Some(seq_str) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            if let Ok(s) = seq_str.trim().parse::<u64>() {
                media_seq = s;
            }
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some(uri) = parse_attr(attrs, "URI") {
                init_map_url = Some(resolve_url(base_url, &uri)?);
            }
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            let method = parse_attr(attrs, "METHOD").unwrap_or_default();
            if method.eq_ignore_ascii_case("NONE") {
                current_key = None;
            } else if method.eq_ignore_ascii_case("AES-128") {
                let uri = parse_attr(attrs, "URI")
                    .ok_or_else(|| SiphonError::Network("HLS AES-128 密钥缺少 URI".to_string()))?;
                let resolved_uri = resolve_url(base_url, &uri)?;
                let iv = parse_attr(attrs, "IV").and_then(|s| parse_hex_iv(&s));
                current_key = Some(HlsKeySpec {
                    uri: resolved_uri,
                    iv,
                });
            }
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let dur_str = rest.split(',').next().unwrap_or("0").trim();
            pending_dur = Some(dur_str.parse::<f64>().unwrap_or(0.0).max(0.0));
        } else if !line.starts_with('#') {
            let resolved = resolve_url(base_url, line)?;
            let dur = pending_dur.take().unwrap_or(0.0);
            let idx = segments.len();
            total_duration_secs += dur;
            segments.push(HlsSegment {
                index: idx,
                sequence: media_seq + idx as u64,
                url: resolved,
                duration_secs: dur,
                key: current_key.clone(),
            });
        }
    }

    if segments.is_empty() {
        return Err(SiphonError::Network(
            "M3U8 播放列表中未找到任何可下载媒体分片".to_string(),
        ));
    }

    let is_fmp4 = init_map_url.is_some()
        || segments
            .first()
            .is_some_and(|s| s.url.to_ascii_lowercase().contains(".m4s"));

    Ok(ParsedPlaylist::Media(HlsMediaPlaylist {
        init_map_url,
        segments,
        total_duration_secs,
        is_fmp4,
    }))
}

fn resolve_url(base: &Url, rel: &str) -> Result<String> {
    base.join(rel)
        .map(|u| u.to_string())
        .map_err(|e| SiphonError::Network(format!("解析 HLS 相对地址失败 ({rel}): {e}")))
}

fn parse_attr(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    while !rest.is_empty() {
        let rest_trim = rest.trim_start_matches([',', ' ']);
        if rest_trim.is_empty() {
            break;
        }
        let Some((k, after_eq)) = rest_trim.split_once('=') else {
            break;
        };
        let (val, remainder) = if let Some(stripped) = after_eq.strip_prefix('"') {
            if let Some((quoted, after_q)) = stripped.split_once('"') {
                (quoted, after_q)
            } else {
                (stripped, "")
            }
        } else if let Some((unquoted, after_c)) = after_eq.split_once(',') {
            (unquoted, after_c)
        } else {
            (after_eq, "")
        };
        if k.trim().eq_ignore_ascii_case(key) {
            return Some(val.trim().to_string());
        }
        rest = remainder;
    }
    None
}

fn parse_hex_iv(raw: &str) -> Option<[u8; 16]> {
    let clean = raw
        .trim()
        .strip_prefix("0x")
        .or_else(|| raw.trim().strip_prefix("0X"))
        .unwrap_or(raw.trim());
    let padded = if clean.len() < 32 {
        format!("{:0>32}", clean)
    } else {
        clean[..32].to_string()
    };
    let bytes = hex::decode(padded).ok()?;
    if bytes.len() == 16 {
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&bytes);
        Some(iv)
    } else {
        None
    }
}
