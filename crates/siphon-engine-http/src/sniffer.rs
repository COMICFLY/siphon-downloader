use futures_util::StreamExt;
use percent_encoding::percent_decode_str;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, RANGE};
use siphon_core::{infer_filename_from_raw_url, sanitize_filename};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use url::Url;

const MAX_HTML_SNIFF_BYTES: usize = 4 * 1024 * 1024;
const MAX_SNIFFED_ITEMS: usize = 200;

/// A downloadable media stream, file, or P2P link extracted by Siphon's webpage sniffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffedWebItem {
    pub url: String,
    pub filename: String,
    pub kind: String,
    pub label: String,
    pub mime: Option<String>,
    pub file_size: Option<u64>,
}

fn build_http_client(
    proxy_url: Option<&str>,
    user_agent: &str,
    timeout: Duration,
) -> std::result::Result<reqwest::Client, String> {
    let ua = if user_agent.trim().is_empty() {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36"
    } else {
        user_agent.trim()
    };
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(8))
        .user_agent(ua);
    if let Some(proxy) = proxy_url
        && !proxy.trim().is_empty()
        && let Ok(p) = reqwest::Proxy::all(proxy.trim())
    {
        builder = builder.proxy(p);
    }
    builder
        .build()
        .map_err(|e| format!("初始化 HTTP 嗅探客户端失败: {e}"))
}

/// Fetch a webpage URL and extract downloadable streams (`.m3u8`/`.mpd`), video/audio media,
/// direct file links, images, `magnet:?`, `.torrent`, and `ed2k://` resources.
pub async fn sniff_webpage_url(
    page_url: &str,
    proxy_url: Option<&str>,
    user_agent: &str,
) -> std::result::Result<(String, Vec<SniffedWebItem>), String> {
    let trimmed = page_url.trim();
    let parsed_input = Url::parse(trimmed).map_err(|_| "请输入有效的 http:// 或 https:// 网页链接".to_string())?;
    if !matches!(parsed_input.scheme(), "http" | "https") {
        return Err("网页嗅探仅支持 http:// 或 https:// 地址".to_string());
    }

    let client = build_http_client(proxy_url, user_agent, Duration::from_secs(15))?;
    let resp = client
        .get(parsed_input.clone())
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await
        .map_err(|e| format!("无法访问目标网页: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("目标网页返回状态码 HTTP {}", resp.status().as_u16()));
    }

    let final_url = resp.url().clone();
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase());
    let content_length = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());

    // If the pasted URL itself points directly to a media stream or downloadable file, return it directly.
    if let Some(ct) = content_type.as_deref() {
        let is_textual_page = ct.contains("html")
            || ct.contains("xhtml")
            || ct.contains("xml")
            || ct.contains("json")
            || ct.starts_with("text/plain");
        if !is_textual_page {
            let (kind, label) = classify_by_mime_and_url(ct, final_url.as_str());
            let filename = infer_sniff_filename(final_url.as_str(), &kind);
            return Ok((
                filename.clone(),
                vec![SniffedWebItem {
                    url: final_url.to_string(),
                    filename,
                    kind,
                    label,
                    mime: Some(ct.to_string()),
                    file_size: content_length,
                }],
            ));
        }
    }

    // Stream-read up to MAX_HTML_SNIFF_BYTES so huge responses never exhaust memory.
    let mut bytes = Vec::with_capacity(64 * 1024);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取网页内容失败: {e}"))?;
        let remaining = MAX_HTML_SNIFF_BYTES.saturating_sub(bytes.len());
        if remaining == 0 {
            break;
        }
        let take = chunk.len().min(remaining);
        bytes.extend_from_slice(&chunk[..take]);
        if bytes.len() >= MAX_HTML_SNIFF_BYTES {
            break;
        }
    }

    let html = String::from_utf8_lossy(&bytes);
    let (title, items) = extract_resources_from_html(&final_url, &html);
    Ok((title, items))
}

/// Concurrently probe unknown file sizes (`HEAD` / `GET Range: bytes=0-0`) for sniffed URLs.
pub async fn probe_urls_sizes(
    urls: Vec<String>,
    headers: BTreeMap<String, String>,
    proxy_url: Option<String>,
    user_agent: String,
) -> Vec<(String, Option<u64>)> {
    let client = match build_http_client(proxy_url.as_deref(), &user_agent, Duration::from_secs(7)) {
        Ok(c) => c,
        Err(_) => return urls.into_iter().map(|u| (u, None)).collect(),
    };

    let mut base_headers = HeaderMap::new();
    for (k, v) in &headers {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            base_headers.insert(name, val);
        }
    }
    let base_headers = Arc::new(base_headers);
    let sem = Arc::new(Semaphore::new(6));
    let mut handles = Vec::with_capacity(urls.len());

    for url in urls {
        let client = client.clone();
        let base_headers = Arc::clone(&base_headers);
        let sem = Arc::clone(&sem);
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let size = probe_single_size(&client, &url, &base_headers).await;
            (url, size)
        }));
    }

    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(pair) = h.await {
            out.push(pair);
        }
    }
    out
}

async fn probe_single_size(
    client: &reqwest::Client,
    raw_url: &str,
    base_headers: &HeaderMap,
) -> Option<u64> {
    let lower = raw_url.trim().to_ascii_lowercase();
    if lower.starts_with("ed2k://|file|") {
        return parse_ed2k_file_size(raw_url);
    }
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return None;
    }

    // 1) Try fast HEAD
    if let Ok(resp) = client
        .head(raw_url)
        .headers(base_headers.clone())
        .send()
        .await
        && resp.status().is_success()
        && let Some(len) = parse_content_length(resp.headers())
        && len > 0
    {
        return Some(len);
    }

    // 2) Fallback: single-byte Range GET (works on CDNs/servers that disallow HEAD)
    let mut range_headers = base_headers.clone();
    range_headers.insert(RANGE, HeaderValue::from_static("bytes=0-0"));
    if let Ok(resp) = client.get(raw_url).headers(range_headers).send().await
        && (resp.status().is_success() || resp.status().as_u16() == 206)
    {
        if let Some(total) = parse_content_range_total(resp.headers())
            && total > 0
        {
            return Some(total);
        }
        if resp.status().as_u16() == 200
            && let Some(len) = parse_content_length(resp.headers())
            && len > 0
        {
            return Some(len);
        }
    }

    None
}

fn parse_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

fn parse_content_range_total(headers: &HeaderMap) -> Option<u64> {
    let val = headers.get(CONTENT_RANGE)?.to_str().ok()?;
    let (_, total_str) = val.rsplit_once('/')?;
    total_str.trim().parse::<u64>().ok()
}

fn parse_ed2k_file_size(raw: &str) -> Option<u64> {
    let parts: Vec<&str> = raw.split('|').collect();
    if parts.len() >= 5 && parts[1].eq_ignore_ascii_case("file") {
        parts[3].trim().parse::<u64>().ok()
    } else {
        None
    }
}

/// Pure HTML/text parser that extracts streams, media, direct files, images, and P2P links.
pub fn extract_resources_from_html(base_url: &Url, html: &str) -> (String, Vec<SniffedWebItem>) {
    let title = extract_html_title(html).unwrap_or_else(|| {
        base_url
            .host_str()
            .unwrap_or("网页嗅探结果")
            .to_string()
    });

    let mut seen = HashSet::new();
    let mut items = Vec::new();

    let mut push_candidate = |raw: &str, force_kind: Option<(&str, &str)>| {
        if items.len() >= MAX_SNIFFED_ITEMS {
            return;
        }
        let cleaned = decode_html_entities(raw.trim());
        let cleaned = cleaned.trim_matches(|c: char| matches!(c, '"' | '\'' | ' ' | '\t' | '\r' | '\n'));
        if cleaned.is_empty()
            || cleaned.starts_with('#')
            || cleaned.starts_with("javascript:")
            || cleaned.starts_with("data:")
            || cleaned.starts_with("mailto:")
            || cleaned.starts_with("blob:")
        {
            return;
        }

        let low = cleaned.to_ascii_lowercase();
        let resolved = if low.starts_with("magnet:?") || low.starts_with("ed2k://|file|") {
            cleaned.to_string()
        } else if low.starts_with("http://") || low.starts_with("https://") {
            cleaned.to_string()
        } else if cleaned.starts_with("//") {
            format!("{}:{}", base_url.scheme(), cleaned)
        } else if let Ok(joined) = base_url.join(cleaned) {
            if matches!(joined.scheme(), "http" | "https") {
                joined.to_string()
            } else {
                return;
            }
        } else {
            return;
        };

        let classified = if let Some((k, lbl)) = force_kind {
            // Let more specific stream/P2P classifications override generic tag hints
            classify_url_candidate(&resolved).unwrap_or_else(|| (k.to_string(), lbl.to_string()))
        } else {
            match classify_url_candidate(&resolved) {
                Some(pair) => pair,
                None => return,
            }
        };

        if !seen.insert(resolved.clone()) {
            return;
        }

        let (kind, label) = classified;
        let file_size = if kind == "ed2k" {
            parse_ed2k_file_size(&resolved)
        } else {
            None
        };
        let filename = infer_sniff_filename(&resolved, &kind);
        items.push(SniffedWebItem {
            url: resolved,
            filename,
            kind,
            label,
            mime: None,
            file_size,
        });
    };

    // 1. Scan HTML tag attributes: <video>, <audio>, <source>, <a>, <img>, <embed>, <object>
    for (tag, attr_name, attr_val) in scan_html_tag_attrs(html) {
        let is_media_tag = matches!(tag.as_str(), "video" | "audio" | "source" | "embed" | "object");
        let is_src_attr = matches!(
            attr_name.as_str(),
            "src" | "href" | "data-src" | "data-url" | "data-original" | "data"
        );
        if !is_src_attr {
            continue;
        }
        if is_media_tag {
            let hint = if tag == "audio" {
                Some(("audio", "网页音频"))
            } else {
                Some(("video", "网页视频"))
            };
            push_candidate(&attr_val, hint);
        } else if tag == "img" {
            if let Some((kind, _)) = classify_url_candidate(&attr_val)
                && kind != "image"
            {
                push_candidate(&attr_val, None);
            } else {
                push_candidate(&attr_val, Some(("image", "网页图片")));
            }
        } else {
            push_candidate(&attr_val, None);
        }
    }

    // 2. Scan raw & JSON-escaped text (catches hidden <script> player configs like `https:\/\/cdn.x\/play.m3u8`)
    let unescaped = html
        .replace("\\/", "/")
        .replace("\\u002F", "/")
        .replace("\\u002f", "/")
        .replace("\\u0026", "&")
        .replace("&amp;", "&");

    for url_candidate in scan_inline_urls(&unescaped) {
        push_candidate(&url_candidate, None);
    }

    items.sort_by_key(|it| kind_priority(&it.kind));
    (title, items)
}

fn kind_priority(kind: &str) -> u8 {
    match kind {
        "hls" => 0,
        "video" => 1,
        "audio" => 2,
        "magnet" | "torrent" | "ed2k" => 3,
        "file" => 4,
        "image" => 5,
        _ => 6,
    }
}

fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start_tag = lower.find("<title")?;
    let after_open = html[start_tag..].find('>')? + start_tag + 1;
    let end_rel = lower[after_open..].find("</title>")?;
    let raw = &html[after_open..after_open + end_rel];
    let decoded = decode_html_entities(raw);
    let collapsed: String = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// Lightweight, panic-free HTML tag/attribute scanner (no regex dependency required).
fn scan_html_tag_attrs(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= len || bytes[i] == b'/' || bytes[i] == b'!' || bytes[i] == b'?' {
            continue;
        }
        let tag_start = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
            i += 1;
        }
        if i == tag_start {
            continue;
        }
        let tag = html[tag_start..i].to_ascii_lowercase();
        let interested = matches!(
            tag.as_str(),
            "a" | "video" | "audio" | "source" | "img" | "embed" | "object" | "link"
        );

        // Parse attributes until '>'
        while i < len && bytes[i] != b'>' {
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= len || bytes[i] == b'>' {
                break;
            }
            if bytes[i] == b'/' {
                i += 1;
                continue;
            }
            let attr_start = i;
            while i < len
                && !bytes[i].is_ascii_whitespace()
                && bytes[i] != b'='
                && bytes[i] != b'>'
            {
                i += 1;
            }
            let attr_name = html[attr_start..i].to_ascii_lowercase();
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let mut attr_val = String::new();
            if i < len && bytes[i] == b'=' {
                i += 1;
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < len && (bytes[i] == b'"' || bytes[i] == b'\'') {
                    let quote = bytes[i];
                    i += 1;
                    let val_start = i;
                    while i < len && bytes[i] != quote {
                        i += 1;
                    }
                    attr_val = html[val_start..i].to_string();
                    if i < len {
                        i += 1;
                    }
                } else {
                    let val_start = i;
                    while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                        i += 1;
                    }
                    attr_val = html[val_start..i].to_string();
                }
            }

            if interested && !attr_name.is_empty() && !attr_val.is_empty() {
                results.push((tag.clone(), attr_name, attr_val));
            }
        }
    }

    results
}

/// Extract embedded `http(s)://`, `magnet:?`, and `ed2k://|file|` URLs from arbitrary script/JSON text.
fn scan_inline_urls(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let lower = text.to_ascii_lowercase();
    let prefixes = ["https://", "http://", "magnet:?", "ed2k://|file|"];

    for prefix in prefixes {
        let mut search_from = 0usize;
        while let Some(rel) = lower[search_from..].find(prefix) {
            let start = search_from + rel;
            let rest = &text[start..];
            let end_offset = rest
                .find(|c: char| {
                    c.is_whitespace()
                        || matches!(c, '"' | '\'' | '<' | '>' | '`' | ')' | ']' | '}' | '\\')
                })
                .unwrap_or(rest.len());
            let raw = rest[..end_offset].trim_end_matches(['.', ',', ';']);
            if raw.len() > prefix.len() + 3 {
                found.push(raw.to_string());
            }
            search_from = start + end_offset.max(prefix.len());
            if search_from >= text.len() || found.len() >= 1500 {
                break;
            }
        }
    }
    found
}

fn classify_by_mime_and_url(mime: &str, url: &str) -> (String, String) {
    if mime.contains("mpegurl") || mime.contains("dash+xml") {
        return ("hls".to_string(), "流媒体清单".to_string());
    }
    if mime.starts_with("video/") {
        return ("video".to_string(), "直链视频".to_string());
    }
    if mime.starts_with("audio/") {
        return ("audio".to_string(), "直链音频".to_string());
    }
    if mime.starts_with("image/") {
        return ("image".to_string(), "图片资源".to_string());
    }
    if mime.contains("bittorrent") {
        return ("torrent".to_string(), "BT 种子".to_string());
    }
    classify_url_candidate(url).unwrap_or_else(|| ("file".to_string(), "直链文件".to_string()))
}

fn classify_url_candidate(url: &str) -> Option<(String, String)> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("magnet:?") {
        return Some(("magnet".to_string(), "磁力链接".to_string()));
    }
    if lower.starts_with("ed2k://|file|") {
        return Some(("ed2k".to_string(), "eD2k 电驴".to_string()));
    }
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let path_lower = parsed.path().to_ascii_lowercase();
    let query_lower = parsed.query().unwrap_or("").to_ascii_lowercase();

    if path_lower.ends_with(".m3u8") || path_lower.ends_with(".m3u") {
        return Some(("hls".to_string(), "HLS 播放列表".to_string()));
    }
    if path_lower.ends_with(".mpd") {
        return Some(("hls".to_string(), "DASH 媒体流".to_string()));
    }
    if path_lower.ends_with(".torrent") {
        return Some(("torrent".to_string(), "BT 种子".to_string()));
    }

    let ext = path_lower
        .rsplit('/')
        .next()
        .and_then(|seg| seg.rsplit_once('.'))
        .map(|(_, e)| e)
        .unwrap_or("");

    if matches!(
        ext,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "flv" | "f4v" | "m4v" | "m4s" | "wmv" | "ts"
    ) {
        return Some(("video".to_string(), format!("{} 视频", ext.to_ascii_uppercase())));
    }
    if matches!(ext, "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus") {
        return Some(("audio".to_string(), format!("{} 音频", ext.to_ascii_uppercase())));
    }
    if matches!(
        ext,
        "zip"
            | "rar"
            | "7z"
            | "tar"
            | "gz"
            | "xz"
            | "zst"
            | "bz2"
            | "iso"
            | "img"
            | "exe"
            | "msi"
            | "dmg"
            | "apk"
            | "deb"
            | "rpm"
            | "pdf"
            | "epub"
            | "docx"
            | "xlsx"
            | "pptx"
            | "bin"
    ) {
        return Some(("file".to_string(), format!("{} 文件", ext.to_ascii_uppercase())));
    }
    if matches!(ext, "jpg" | "jpeg" | "png" | "webp" | "gif" | "avif" | "bmp") {
        // Ignore tiny favicon/sprite noise by filename convention
        if path_lower.contains("favicon") || path_lower.contains("sprite") {
            return None;
        }
        return Some(("image".to_string(), format!("{} 图片", ext.to_ascii_uppercase())));
    }

    // Query hints commonly used by CDN media players without explicit path extensions
    if query_lower.contains("mime_type=video")
        || query_lower.contains("mime=video")
        || query_lower.contains("format=mp4")
    {
        return Some(("video".to_string(), "视频流".to_string()));
    }
    if query_lower.contains("type=m3u8") || query_lower.contains("format=m3u8") {
        return Some(("hls".to_string(), "HLS 播放列表".to_string()));
    }
    if query_lower.contains("mime_type=audio") || query_lower.contains("mime=audio") {
        return Some(("audio".to_string(), "音频流".to_string()));
    }

    None
}

fn infer_sniff_filename(url: &str, kind: &str) -> String {
    let parsed = Url::parse(url).ok();
    let raw = infer_filename_from_raw_url(url, parsed.as_ref());
    let decoded = percent_decode_str(&raw).decode_utf8_lossy().to_string();
    let clean = sanitize_filename(&decoded);
    if !clean.is_empty() && clean != "download.bin" {
        return clean;
    }
    match kind {
        "hls" => "stream.m3u8".to_string(),
        "video" => "video.mp4".to_string(),
        "audio" => "audio.mp3".to_string(),
        "image" => "image.jpg".to_string(),
        _ => "resource.bin".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::http::{HeaderMap as AxumHeaders, StatusCode, header};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use tokio::net::TcpListener;

    #[test]
    fn extracts_hls_video_p2p_files_and_escaped_script_streams() {
        let base = Url::parse("https://media.example.com/watch/123").unwrap();
        let html = r#"
            <!doctype html>
            <html>
            <head><title>Siphon 演示视频页 &amp; 资源</title></head>
            <body>
                <video src="/cdn/trailer.mp4"></video>
                <a href="https://files.example.com/Siphon-Setup.exe">Download Installer</a>
                <a href="magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Ubuntu.iso">Magnet</a>
                <a href="ed2k://|file|Demo.mkv|52428800|31D6CFE0D16AE931B73C59D7E0C089C0|/">eD2k</a>
                <script>
                    window.__PLAY__ = {"hls":"https:\/\/stream.example.com\/live\/master.m3u8?token=abc"};
                </script>
            </body>
            </html>
        "#;

        let (title, items) = extract_resources_from_html(&base, html);
        assert_eq!(title, "Siphon 演示视频页 & 资源");
        assert!(items.iter().any(|i| i.kind == "hls" && i.url.contains("master.m3u8?token=abc")));
        assert!(items.iter().any(|i| i.kind == "video" && i.url == "https://media.example.com/cdn/trailer.mp4"));
        assert!(items.iter().any(|i| i.kind == "file" && i.filename == "Siphon-Setup.exe"));
        assert!(items.iter().any(|i| i.kind == "magnet"));
        assert!(items.iter().any(|i| i.kind == "ed2k" && i.file_size == Some(52_428_800)));
        // HLS should sort first by priority
        assert_eq!(items[0].kind, "hls");
    }

    #[tokio::test]
    async fn sniffs_local_webpage_and_probes_unknown_file_sizes() {
        let app = Router::new()
            .route(
                "/page",
                get(|| async {
                    let mut h = AxumHeaders::new();
                    h.insert(header::CONTENT_TYPE, "text/html; charset=utf-8".parse().unwrap());
                    (
                        h,
                        r#"<html><head><title>测试资源页</title></head><body>
                           <a href="/files/demo.zip">zip</a>
                           <video><source src="/media/clip.mp4"></video>
                        </body></html>"#,
                    )
                }),
            )
            .route(
                "/files/demo.zip",
                get(|| async {
                    let payload = vec![0x5Au8; 4096];
                    (StatusCode::OK, payload).into_response()
                }),
            )
            .route(
                "/media/clip.mp4",
                get(|req_headers: AxumHeaders| async move {
                    if req_headers.contains_key(header::RANGE) {
                        let mut h = AxumHeaders::new();
                        h.insert(header::CONTENT_RANGE, "bytes 0-0/1048576".parse().unwrap());
                        h.insert(header::CONTENT_LENGTH, "1".parse().unwrap());
                        (StatusCode::PARTIAL_CONTENT, h, vec![0u8; 1]).into_response()
                    } else {
                        StatusCode::METHOD_NOT_ALLOWED.into_response()
                    }
                }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let (title, items) = sniff_webpage_url(&format!("http://{addr}/page"), None, "SiphonTest/1.0")
            .await
            .unwrap();
        assert_eq!(title, "测试资源页");
        assert_eq!(items.len(), 2);

        let urls: Vec<String> = items.iter().map(|i| i.url.clone()).collect();
        let probed = probe_urls_sizes(urls, BTreeMap::new(), None, "SiphonTest/1.0".into()).await;
        let map: BTreeMap<String, Option<u64>> = probed.into_iter().collect();

        assert_eq!(
            map.get(&format!("http://{addr}/files/demo.zip")).copied().flatten(),
            Some(4096)
        );
        assert_eq!(
            map.get(&format!("http://{addr}/media/clip.mp4")).copied().flatten(),
            Some(1_048_576)
        );
    }
}
