use percent_encoding::percent_decode_str;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
    HeaderMap, HeaderName, HeaderValue, LAST_MODIFIED, RANGE,
};
use reqwest::{Client, StatusCode};
use siphon_core::{
    FileCategory, ProbeMetadata, Result, SiphonError, infer_filename_from_url, sanitize_filename,
};
use std::collections::BTreeMap;
use std::time::Duration;
use url::Url;

pub fn build_client(proxy_url: Option<&str>, user_agent: &str) -> Result<Client> {
    // Force HTTP/1.1 (`http1_only`) so multi-threaded segment workers open N independent
    // parallel TCP connections instead of multiplexing all range streams over a single HTTP/2
    // TCP socket (which gets bottlenecked by single-connection TCP CWND & H2 stream window).
    let mut builder = Client::builder()
        .user_agent(user_agent)
        .http1_only()
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
        .pool_max_idle_per_host(64);

    if let Some(proxy_str) = proxy_url.map(str::trim).filter(|s| !s.is_empty()) {
        let proxy = reqwest::Proxy::all(proxy_str)
            .map_err(|e| SiphonError::Network(format!("代理配置无效 ({proxy_str}): {e}")))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| SiphonError::Network(format!("创建 HTTP 客户端失败: {e}")))
}

pub fn build_header_map(headers: &BTreeMap<String, String>) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            map.insert(name, val);
        }
    }
    map
}

pub async fn probe_http(
    raw_url: &str,
    headers: &BTreeMap<String, String>,
    proxy_url: Option<&str>,
    user_agent: &str,
) -> Result<ProbeMetadata> {
    let client = build_client(proxy_url, user_agent)?;
    let custom_headers = build_header_map(headers);

    // 1) Probe with `GET Range: bytes=0-0` first in a single round-trip:
    //    - Works on pre-signed S3 / GitHub Release GET URLs where HEAD returns 403/405
    //    - Immediately resolves redirects, Content-Range total size, Accept-Ranges, and Content-Disposition
    let mut range_headers = custom_headers.clone();
    range_headers.insert(RANGE, HeaderValue::from_static("bytes=0-0"));

    if let Ok(resp) = client.get(raw_url).headers(range_headers).send().await {
        let status = resp.status();
        if matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::GONE
        ) {
            return Err(SiphonError::LinkExpired(status.as_u16()));
        }
        if status.is_success() || status == StatusCode::RANGE_NOT_SATISFIABLE {
            if let Some(meta) = extract_metadata(resp.url(), resp.headers(), status) {
                return Ok(meta);
            }
        }
    }

    // 2) Fallback to HEAD only if `GET Range: bytes=0-0` did not succeed
    let resp = client
        .head(raw_url)
        .headers(custom_headers)
        .send()
        .await
        .map_err(|e| SiphonError::Network(e.to_string()))?;

    let status = resp.status();
    if matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::GONE
    ) {
        return Err(SiphonError::LinkExpired(status.as_u16()));
    }
    if !status.is_success() {
        return Err(SiphonError::Network(format!(
            "服务器返回异常状态码 HTTP {}",
            status.as_u16()
        )));
    }

    extract_metadata(resp.url(), resp.headers(), status)
        .ok_or_else(|| SiphonError::Network("无法探测目标文件元数据".to_string()))
}

fn extract_metadata(
    final_url: &Url,
    headers: &HeaderMap,
    status: StatusCode,
) -> Option<ProbeMetadata> {
    let content_range_total = headers
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_range_total);

    let header_len = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());

    let (supports_range, content_length) = if status == StatusCode::PARTIAL_CONTENT {
        (true, content_range_total.or(header_len))
    } else {
        let accepts = headers
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);
        (accepts, content_range_total.or(header_len))
    };

    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());

    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let last_modified = headers
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let disposition_name = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename);

    let url_name = percent_decode_str(&infer_filename_from_url(final_url))
        .decode_utf8_lossy()
        .to_string();

    let filename = sanitize_filename(disposition_name.as_deref().unwrap_or(&url_name));
    let suggested_category =
        FileCategory::from_filename_and_mime(&filename, content_type.as_deref());

    Some(ProbeMetadata {
        final_url: final_url.to_string(),
        filename,
        content_length,
        content_type,
        supports_range,
        etag,
        last_modified,
        suggested_category,
        resolved_headers: BTreeMap::new(),
        plugin_note: None,
    })
}

pub fn parse_content_range_total(header: &str) -> Option<u64> {
    // Examples: "bytes 0-0/12345" or "bytes 0-1023/*"
    let (_, total_part) = header.rsplit_once('/')?;
    let trimmed = total_part.trim();
    if trimmed == "*" {
        None
    } else {
        trimmed.parse::<u64>().ok()
    }
}

pub fn parse_content_disposition_filename(header: &str) -> Option<String> {
    // 1) Prefer RFC 5987 `filename*=UTF-8''...`
    for part in header.split(';') {
        let trimmed = part.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("filename*=") {
            let val = &trimmed[10..].trim().trim_matches('"');
            if let Some((_, encoded)) = val.split_once("''") {
                let decoded = percent_decode_str(encoded).decode_utf8_lossy().to_string();
                if !decoded.trim().is_empty() {
                    return Some(decoded);
                }
            }
        }
    }

    // 2) Fallback to `filename="..."`
    for part in header.split(';') {
        let trimmed = part.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("filename=") {
            let val = trimmed[9..].trim().trim_matches('"');
            if !val.is_empty() {
                let decoded = percent_decode_str(val).decode_utf8_lossy().to_string();
                return Some(decoded);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc5987_utf8_content_disposition() {
        let h = "attachment; filename=\"fallback.zip\"; filename*=UTF-8''%E6%B5%8B%E8%AF%95%E6%96%87%E4%BB%B6.zip";
        assert_eq!(
            parse_content_disposition_filename(h),
            Some("测试文件.zip".to_string())
        );
    }

    #[test]
    fn parses_content_range_total() {
        assert_eq!(parse_content_range_total("bytes 0-0/1048576"), Some(1048576));
        assert_eq!(parse_content_range_total("bytes 0-0/*"), None);
    }
}
