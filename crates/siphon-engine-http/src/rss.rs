use chrono::{DateTime, Local};
use siphon_core::{RssItem, RssLinkKind};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone)]
pub struct FetchedRssFeed {
    pub feed_title: Option<String>,
    pub items: Vec<RssItem>,
}

/// Fetch and parse an RSS 2.0 or Atom 1.0 feed URL (respecting optional proxy & custom User-Agent).
pub async fn fetch_rss_feed(
    feed_url: &str,
    proxy_url: Option<&str>,
    user_agent: &str,
) -> std::result::Result<FetchedRssFeed, String> {
    let trimmed = feed_url.trim();
    let parsed_base = Url::parse(trimmed).map_err(|e| format!("无效的 RSS 地址：{e}"))?;
    if !matches!(parsed_base.scheme(), "http" | "https") {
        return Err("RSS 地址仅支持 http:// 或 https://".to_string());
    }

    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(8))
        .user_agent(if user_agent.trim().is_empty() {
            format!("Siphon/{}", env!("CARGO_PKG_VERSION"))
        } else {
            user_agent.to_string()
        });
    if let Some(proxy) = proxy_url
        && !proxy.trim().is_empty()
    {
        if let Ok(p) = reqwest::Proxy::all(proxy.trim()) {
            builder = builder.proxy(p);
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败：{e}"))?;

    let resp = client
        .get(trimmed)
        .header(
            reqwest::header::ACCEPT,
            "application/rss+xml, application/atom+xml, application/xml, text/xml;q=0.9, */*;q=0.8",
        )
        .send()
        .await
        .map_err(|e| format!("拉取 RSS 失败：{e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("RSS 服务器返回 HTTP {}", status.as_u16()));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("读取 RSS 响应正文失败：{e}"))?;

    let fetched = parse_rss_or_atom(&body, Some(&parsed_base))?;
    Ok(fetched)
}

/// Parse RSS 2.0 or Atom 1.0 XML into structured `FetchedRssFeed`.
pub fn parse_rss_or_atom(
    xml: &str,
    base_url: Option<&Url>,
) -> std::result::Result<FetchedRssFeed, String> {
    let cleaned = xml.trim_start_matches('\u{feff}').trim();
    if cleaned.is_empty() {
        return Err("RSS 响应内容为空".to_string());
    }

    let item_blocks = extract_tag_blocks(cleaned, "item");
    if !item_blocks.is_empty() {
        return Ok(parse_rss2(cleaned, &item_blocks, base_url));
    }

    let entry_blocks = extract_tag_blocks(cleaned, "entry");
    if !entry_blocks.is_empty() {
        return Ok(parse_atom(cleaned, &entry_blocks, base_url));
    }

    // Valid empty RSS/Atom feed check
    let low = cleaned.to_ascii_lowercase();
    if low.contains("<rss") || low.contains("<channel") || low.contains("<feed") {
        let feed_title = extract_first_tag_text(cleaned, "title")
            .map(|t| clean_text(&t))
            .filter(|t| !t.is_empty());
        return Ok(FetchedRssFeed {
            feed_title,
            items: Vec::new(),
        });
    }

    Err("未能识别有效的 RSS 2.0 或 Atom 1.0 XML 格式".to_string())
}

fn parse_rss2(xml: &str, item_blocks: &[&str], base_url: Option<&Url>) -> FetchedRssFeed {
    // Channel header sits before the first `<item`
    let header_slice = find_tag_open_index(xml, "item")
        .map(|pos| &xml[..pos])
        .unwrap_or(xml);
    let feed_title = extract_first_tag_text(header_slice, "title")
        .map(|t| clean_text(&t))
        .filter(|t| !t.is_empty());

    let mut items = Vec::with_capacity(item_blocks.len());
    for block in item_blocks {
        let title = extract_first_tag_text(block, "title")
            .map(|t| clean_text(&t))
            .unwrap_or_default();
        let guid_raw = extract_first_tag_text(block, "guid")
            .map(|t| clean_text(&t))
            .filter(|s| !s.is_empty());
        let link_raw = extract_first_tag_text(block, "link")
            .map(|t| clean_text(&t))
            .filter(|s| !s.is_empty());
        let pub_date = extract_first_tag_text(block, "pubDate")
            .or_else(|| extract_first_tag_text(block, "dc:date"))
            .or_else(|| extract_first_tag_text(block, "published"))
            .map(|d| format_rss_date(&clean_text(&d)))
            .filter(|s| !s.is_empty());

        let (enc_url, enc_len) = extract_enclosure(block);
        let magnet_tag = extract_first_tag_text(block, "magnetURI")
            .or_else(|| extract_first_tag_text(block, "torrent:magnetURI"))
            .map(|s| clean_text(&s))
            .filter(|s| s.to_ascii_lowercase().starts_with("magnet:?"));
        let nyaa_hash = extract_first_tag_text(block, "nyaa:infoHash")
            .or_else(|| extract_first_tag_text(block, "infoHash"))
            .map(|s| clean_text(&s))
            .filter(|s| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()));
        let torznab_magnet = extract_torznab_attr(block, "magneturl");
        let torznab_size = extract_torznab_attr(block, "size").and_then(|s| s.parse::<u64>().ok());
        let content_len = extract_first_tag_text(block, "contentLength")
            .or_else(|| extract_first_tag_text(block, "torrent:contentLength"))
            .and_then(|s| clean_text(&s).parse::<u64>().ok());
        let nyaa_size = extract_first_tag_text(block, "nyaa:size")
            .and_then(|s| parse_human_size(&clean_text(&s)));

        let description = extract_first_tag_text(block, "description")
            .or_else(|| extract_first_tag_text(block, "content:encoded"))
            .map(|s| unescape_xml(&strip_cdata(&s)))
            .unwrap_or_default();

        // Pick best downloadable URL with P2P / enclosure priority
        let mut download_url = enc_url
            .or(magnet_tag)
            .or(torznab_magnet)
            .or_else(|| {
                nyaa_hash.map(|h| format!("magnet:?xt=urn:btih:{h}"))
            })
            .or_else(|| extract_download_link_from_text(&description))
            .or_else(|| link_raw.clone())
            .unwrap_or_default();

        download_url = resolve_maybe_relative(&download_url, base_url);
        if download_url.is_empty() {
            continue;
        }

        let page_url = link_raw
            .map(|u| resolve_maybe_relative(&u, base_url))
            .filter(|u| {
                let low = u.to_ascii_lowercase();
                (low.starts_with("http://") || low.starts_with("https://"))
                    && !low.ends_with(".torrent")
            });

        let kind = RssLinkKind::from_url(&download_url);
        let guid = guid_raw.unwrap_or_else(|| download_url.clone());
        let size_bytes = enc_len
            .filter(|&n| n > 0)
            .or(content_len.filter(|&n| n > 0))
            .or(torznab_size.filter(|&n| n > 0))
            .or(nyaa_size.filter(|&n| n > 0));

        let final_title = if title.is_empty() {
            download_url.clone()
        } else {
            title
        };

        items.push(RssItem {
            guid,
            title: final_title,
            download_url,
            page_url,
            published_at: pub_date,
            size_bytes,
            kind,
        });
    }

    FetchedRssFeed { feed_title, items }
}

fn parse_atom(xml: &str, entry_blocks: &[&str], base_url: Option<&Url>) -> FetchedRssFeed {
    let header_slice = find_tag_open_index(xml, "entry")
        .map(|pos| &xml[..pos])
        .unwrap_or(xml);
    let feed_title = extract_first_tag_text(header_slice, "title")
        .map(|t| clean_text(&t))
        .filter(|t| !t.is_empty());

    let mut items = Vec::with_capacity(entry_blocks.len());
    for block in entry_blocks {
        let title = extract_first_tag_text(block, "title")
            .map(|t| clean_text(&t))
            .unwrap_or_default();
        let id_raw = extract_first_tag_text(block, "id")
            .map(|t| clean_text(&t))
            .filter(|s| !s.is_empty());
        let pub_date = extract_first_tag_text(block, "published")
            .or_else(|| extract_first_tag_text(block, "updated"))
            .map(|d| format_rss_date(&clean_text(&d)))
            .filter(|s| !s.is_empty());

        let (enc_href, enc_len, alt_href) = extract_atom_links(block);
        let content = extract_first_tag_text(block, "content")
            .or_else(|| extract_first_tag_text(block, "summary"))
            .map(|s| unescape_xml(&strip_cdata(&s)))
            .unwrap_or_default();

        let mut download_url = enc_href
            .or_else(|| extract_download_link_from_text(&content))
            .or_else(|| alt_href.clone())
            .unwrap_or_default();
        download_url = resolve_maybe_relative(&download_url, base_url);
        if download_url.is_empty() {
            continue;
        }

        let page_url = alt_href
            .map(|u| resolve_maybe_relative(&u, base_url))
            .filter(|u| {
                let low = u.to_ascii_lowercase();
                low.starts_with("http://") || low.starts_with("https://")
            });

        let kind = RssLinkKind::from_url(&download_url);
        let guid = id_raw.unwrap_or_else(|| download_url.clone());
        let final_title = if title.is_empty() {
            download_url.clone()
        } else {
            title
        };

        items.push(RssItem {
            guid,
            title: final_title,
            download_url,
            page_url,
            published_at: pub_date,
            size_bytes: enc_len.filter(|&n| n > 0),
            kind,
        });
    }

    FetchedRssFeed { feed_title, items }
}

fn resolve_maybe_relative(raw: &str, base_url: Option<&Url>) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let low = trimmed.to_ascii_lowercase();
    if low.starts_with("http://")
        || low.starts_with("https://")
        || low.starts_with("magnet:?")
        || low.starts_with("ed2k://")
    {
        return trimmed.to_string();
    }
    if let Some(base) = base_url
        && let Ok(joined) = base.join(trimmed)
    {
        return joined.to_string();
    }
    trimmed.to_string()
}

fn find_tag_open_index(xml: &str, tag: &str) -> Option<usize> {
    let low = xml.to_ascii_lowercase();
    let needle = format!("<{}", tag.to_ascii_lowercase());
    let mut search_from = 0;
    while let Some(rel) = low[search_from..].find(&needle) {
        let pos = search_from + rel;
        let after = pos + needle.len();
        if let Some(ch) = low[after..].chars().next()
            && (ch == '>' || ch.is_ascii_whitespace() || ch == '/')
        {
            return Some(pos);
        }
        search_from = after;
    }
    None
}

fn extract_tag_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let low = xml.to_ascii_lowercase();
    let open_needle = format!("<{}", tag.to_ascii_lowercase());
    let close_needle = format!("</{}>", tag.to_ascii_lowercase());
    let mut out = Vec::new();
    let mut pos = 0;

    while let Some(rel) = low[pos..].find(&open_needle) {
        let start = pos + rel;
        let after_name = start + open_needle.len();
        let valid_boundary = low[after_name..]
            .chars()
            .next()
            .is_some_and(|c| c == '>' || c.is_ascii_whitespace());
        if !valid_boundary {
            pos = after_name;
            continue;
        }
        let Some(open_end_rel) = xml[after_name..].find('>') else {
            break;
        };
        let content_start = after_name + open_end_rel + 1;
        // Skip self-closing tags
        if xml[..content_start - 1].ends_with('/') {
            pos = content_start;
            continue;
        }
        let Some(close_rel) = low[content_start..].find(&close_needle) else {
            break;
        };
        let content_end = content_start + close_rel;
        out.push(&xml[content_start..content_end]);
        pos = content_end + close_needle.len();
    }

    out
}

fn extract_first_tag_text(xml: &str, tag: &str) -> Option<String> {
    extract_tag_blocks(xml, tag)
        .into_iter()
        .next()
        .map(str::to_string)
}

fn extract_enclosure(block: &str) -> (Option<String>, Option<u64>) {
    let low = block.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(rel) = low[pos..].find("<enclosure") {
        let start = pos + rel + "<enclosure".len();
        let Some(end_rel) = block[start..].find('>') else {
            break;
        };
        let attrs = &block[start..start + end_rel];
        let url = extract_attr(attrs, "url")
            .map(|s| unescape_xml(&s).trim().to_string())
            .filter(|s| !s.is_empty());
        let len = extract_attr(attrs, "length").and_then(|s| s.trim().parse::<u64>().ok());
        if url.is_some() {
            return (url, len);
        }
        pos = start + end_rel + 1;
    }
    (None, None)
}

fn extract_atom_links(block: &str) -> (Option<String>, Option<u64>, Option<String>) {
    let low = block.to_ascii_lowercase();
    let mut pos = 0;
    let mut enclosure_href: Option<String> = None;
    let mut enclosure_len: Option<u64> = None;
    let mut alt_href: Option<String> = None;

    while let Some(rel) = low[pos..].find("<link") {
        let start = pos + rel + "<link".len();
        let valid = low[start..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_whitespace() || c == '>');
        if !valid {
            pos = start;
            continue;
        }
        let Some(end_rel) = block[start..].find('>') else {
            break;
        };
        let attrs = &block[start..start + end_rel];
        if let Some(href) = extract_attr(attrs, "href")
            .map(|s| unescape_xml(&s).trim().to_string())
            .filter(|s| !s.is_empty())
        {
            let rel_attr = extract_attr(attrs, "rel")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_default();
            let type_attr = extract_attr(attrs, "type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_default();
            let is_enclosure = rel_attr == "enclosure"
                || type_attr.contains("bittorrent")
                || href.to_ascii_lowercase().starts_with("magnet:?")
                || href.to_ascii_lowercase().ends_with(".torrent");
            if is_enclosure && enclosure_href.is_none() {
                enclosure_href = Some(href);
                enclosure_len =
                    extract_attr(attrs, "length").and_then(|s| s.trim().parse::<u64>().ok());
            } else if alt_href.is_none() && (rel_attr.is_empty() || rel_attr == "alternate") {
                alt_href = Some(href);
            }
        }
        pos = start + end_rel + 1;
    }
    (enclosure_href, enclosure_len, alt_href)
}

fn extract_torznab_attr(block: &str, wanted_name: &str) -> Option<String> {
    let low = block.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(rel) = low[pos..].find("<torznab:attr") {
        let start = pos + rel + "<torznab:attr".len();
        let Some(end_rel) = block[start..].find('>') else {
            break;
        };
        let attrs = &block[start..start + end_rel];
        if let Some(name) = extract_attr(attrs, "name")
            && name.eq_ignore_ascii_case(wanted_name)
            && let Some(val) = extract_attr(attrs, "value")
        {
            let cleaned = unescape_xml(&val).trim().to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
        pos = start + end_rel + 1;
    }
    None
}

fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let low = attrs.to_ascii_lowercase();
    let name_low = name.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(rel) = low[pos..].find(&name_low) {
        let idx = pos + rel;
        let left_ok = idx == 0
            || attrs[..idx]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_whitespace() || c == '/');
        let after_name = idx + name_low.len();
        if !left_ok {
            pos = after_name;
            continue;
        }
        let rest = attrs[after_name..].trim_start();
        let Some(after_eq) = rest.strip_prefix('=') else {
            pos = after_name;
            continue;
        };
        let val_part = after_eq.trim_start();
        if let Some(quote) = val_part.chars().next().filter(|&c| c == '"' || c == '\'') {
            let inner = &val_part[1..];
            if let Some(end_q) = inner.find(quote) {
                return Some(inner[..end_q].to_string());
            }
        } else {
            let end = val_part
                .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
                .unwrap_or(val_part.len());
            if end > 0 {
                return Some(val_part[..end].to_string());
            }
        }
        pos = after_name;
    }
    None
}

fn extract_download_link_from_text(text: &str) -> Option<String> {
    // 1. Prefer magnet:? links
    let low = text.to_ascii_lowercase();
    if let Some(pos) = low.find("magnet:?") {
        let slice = &text[pos..];
        let end = slice
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
            .unwrap_or(slice.len());
        let cand = unescape_xml(&slice[..end]);
        if cand.len() > 16 {
            return Some(cand);
        }
    }
    // 2. Check ed2k://|file|...
    if let Some(pos) = low.find("ed2k://|file|") {
        let slice = &text[pos..];
        let end = slice
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
            .unwrap_or(slice.len());
        let cand = unescape_xml(&slice[..end]);
        if cand.len() > 20 {
            return Some(cand);
        }
    }
    // 3. Check direct .torrent URLs
    let mut scan = 0;
    while let Some(rel) = low[scan..].find("http") {
        let pos = scan + rel;
        let slice = &text[pos..];
        let end = slice
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
            .unwrap_or(slice.len());
        let cand = unescape_xml(&slice[..end]);
        let cand_low = cand.to_ascii_lowercase();
        if (cand_low.starts_with("http://") || cand_low.starts_with("https://"))
            && (cand_low.ends_with(".torrent") || cand_low.contains(".torrent?"))
        {
            return Some(cand);
        }
        scan = pos + 4;
    }
    None
}

fn strip_cdata(raw: &str) -> String {
    let t = raw.trim();
    if let Some(inner) = t
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
    {
        return inner.to_string();
    }
    t.replace("<![CDATA[", "").replace("]]>", "")
}

fn clean_text(raw: &str) -> String {
    unescape_xml(&strip_cdata(raw))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn unescape_xml(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let entity = &after[..semi];
            let decoded = match entity {
                "amp" => Some("&".to_string()),
                "lt" => Some("<".to_string()),
                "gt" => Some(">".to_string()),
                "quot" => Some("\"".to_string()),
                "apos" => Some("'".to_string()),
                "nbsp" => Some(" ".to_string()),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                }
                _ if entity.starts_with('#') => entity[1..]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string()),
                _ => None,
            };
            if let Some(rep) = decoded {
                out.push_str(&rep);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn format_rss_date(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(trimmed) {
        return dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string();
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string();
    }
    trimmed.chars().take(24).collect()
}

fn parse_human_size(raw: &str) -> Option<u64> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let val: f64 = parts[0].parse().ok()?;
    let unit = parts
        .get(1)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let mult = match unit.as_str() {
        "kib" | "kb" | "k" => 1024.0,
        "mib" | "mb" | "m" => 1024.0 * 1024.0,
        "gib" | "gb" | "g" => 1024.0 * 1024.0 * 1024.0,
        "tib" | "tb" | "t" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    Some((val * mult).max(0.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::get;
    use tokio::net::TcpListener;

    #[test]
    fn parses_rss2_with_cdata_enclosure_and_nyaa_hash() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0" xmlns:nyaa="https://nyaa.si/xmlns/nyaa">
          <channel>
            <title><![CDATA[Mikan Project - 动漫订阅]]></title>
            <item>
              <title><![CDATA[[Lilith-Raws] Example Show - 01 [Baha][WEB-DL][1080p][简日].mp4]]></title>
              <guid>https://mikanani.me/Home/Episode/abc123</guid>
              <link>https://mikanani.me/Home/Episode/abc123</link>
              <pubDate>Tue, 24 Nov 2026 12:30:00 +0800</pubDate>
              <enclosure url="https://mikanani.me/Download/abc123.torrent" length="524288000" type="application/x-bittorrent" />
            </item>
            <item>
              <title>[SubsPlease] Another Show - 02 (1080p)</title>
              <guid>nyaa-02</guid>
              <link>https://nyaa.si/view/123456</link>
              <nyaa:infoHash>0123456789abcdef0123456789abcdef01234567</nyaa:infoHash>
              <nyaa:size>1.5 GiB</nyaa:size>
            </item>
          </channel>
        </rss>"#;

        let parsed = parse_rss_or_atom(xml, None).unwrap();
        assert_eq!(
            parsed.feed_title.as_deref(),
            Some("Mikan Project - 动漫订阅")
        );
        assert_eq!(parsed.items.len(), 2);

        assert_eq!(parsed.items[0].kind, RssLinkKind::Torrent);
        assert_eq!(
            parsed.items[0].download_url,
            "https://mikanani.me/Download/abc123.torrent"
        );
        assert_eq!(parsed.items[0].size_bytes, Some(524_288_000));
        assert!(parsed.items[0].published_at.is_some());

        assert_eq!(parsed.items[1].kind, RssLinkKind::Magnet);
        assert_eq!(
            parsed.items[1].download_url,
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(parsed.items[1].size_bytes, Some(1_610_612_736));
    }

    #[test]
    fn parses_atom_feed_with_enclosure_and_relative_url() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <title>Siphon Releases</title>
          <entry>
            <title>Siphon v0.9.0 &amp; Patch</title>
            <id>tag:siphon:v0.9.0</id>
            <updated>2026-11-24T12:00:00Z</updated>
            <link rel="alternate" href="/releases/v0.9.0" />
            <link rel="enclosure" href="/files/Siphon-0.9.0.exe" length="10485760" />
          </entry>
        </feed>"#;

        let base = Url::parse("https://releases.example.com/atom.xml").unwrap();
        let parsed = parse_rss_or_atom(xml, Some(&base)).unwrap();
        assert_eq!(parsed.feed_title.as_deref(), Some("Siphon Releases"));
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title, "Siphon v0.9.0 & Patch");
        assert_eq!(
            parsed.items[0].download_url,
            "https://releases.example.com/files/Siphon-0.9.0.exe"
        );
        assert_eq!(
            parsed.items[0].page_url.as_deref(),
            Some("https://releases.example.com/releases/v0.9.0")
        );
        assert_eq!(parsed.items[0].size_bytes, Some(10_485_760));
        assert_eq!(parsed.items[0].kind, RssLinkKind::Http);
    }

    #[tokio::test]
    async fn fetches_local_http_rss_feed() {
        let xml = r#"<?xml version="1.0"?>
        <rss version="2.0"><channel><title>Local Test Feed</title>
          <item>
            <title>Sample Distro ISO</title>
            <guid>iso-1</guid>
            <enclosure url="/distro.iso" length="4096" type="application/octet-stream"/>
          </item>
        </channel></rss>"#;

        let app = Router::new().route("/feed.xml", get(move || async move { xml }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let fetched = fetch_rss_feed(&format!("http://{addr}/feed.xml"), None, "SiphonTest")
            .await
            .unwrap();
        assert_eq!(fetched.feed_title.as_deref(), Some("Local Test Feed"));
        assert_eq!(fetched.items.len(), 1);
        assert_eq!(fetched.items[0].download_url, format!("http://{addr}/distro.iso"));
    }
}
