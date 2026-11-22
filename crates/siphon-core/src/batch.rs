use std::collections::HashSet;

pub const MAX_BATCH_URLS: usize = 1000;

#[derive(Debug, Clone, Default)]
pub struct BatchExpandResult {
    pub urls: Vec<String>,
    pub truncated: bool,
}

/// Expand multi-line links and wildcard patterns such as:
/// - `https://example.com/ep_[001-024].mp4`
/// - `https://example.com/part_[1-10:2].zip`
/// - `https://example.com/vol_[a-d].pdf`
/// - Multiple wildcards in one URL (Cartesian product)
/// - Multi-line pasted text containing `http://`, `https://`, or `magnet:?` links
///
/// Optional comma/space-separated `filter_expr` retains only URLs matching at least one token
/// (case-insensitive substring / extension match).
pub fn expand_batch_input(raw: &str, filter_expr: &str) -> BatchExpandResult {
    let filters: Vec<String> = filter_expr
        .split([',', ';', ' '])
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    let mut truncated = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // If the line contains whitespace-separated tokens (e.g. copied text), extract URL tokens;
        // otherwise treat the entire line as a candidate URL / wildcard template.
        let candidates: Vec<&str> = if is_supported_scheme(trimmed) && !trimmed.contains(char::is_whitespace) {
            vec![trimmed]
        } else {
            trimmed
                .split_whitespace()
                .map(|tok| tok.trim_matches(|c: char| matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | ',' | ';')))
                .filter(|tok| is_supported_scheme(tok))
                .collect()
        };

        for cand in candidates {
            let expanded = expand_single_template(cand, MAX_BATCH_URLS.saturating_sub(urls.len()) + 1);
            for url in expanded {
                if !filters.is_empty() {
                    let lower = url.to_ascii_lowercase();
                    if !filters.iter().any(|f| lower.contains(f)) {
                        continue;
                    }
                }
                if seen.insert(url.clone()) {
                    if urls.len() >= MAX_BATCH_URLS {
                        truncated = true;
                        return BatchExpandResult { urls, truncated };
                    }
                    urls.push(url);
                }
            }
        }
    }

    BatchExpandResult { urls, truncated }
}

fn is_supported_scheme(s: &str) -> bool {
    let low = s.to_ascii_lowercase();
    low.starts_with("http://") || low.starts_with("https://") || low.starts_with("magnet:?")
}

fn expand_single_template(tpl: &str, limit: usize) -> Vec<String> {
    let mut current = vec![tpl.to_string()];

    // Repeatedly expand the first valid `[start-end]` or `[start-end:step]` token found.
    while let Some((span_start, span_end, values)) = find_first_wildcard(&current[0]) {
        let mut next = Vec::new();
        for item in &current {
            if span_end <= item.len() {
                let prefix = &item[..span_start];
                let suffix = &item[span_end..];
                for val in &values {
                    if next.len() >= limit {
                        break;
                    }
                    next.push(format!("{prefix}{val}{suffix}"));
                }
            }
            if next.len() >= limit {
                break;
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }

    current
}

fn find_first_wildcard(s: &str) -> Option<(usize, usize, Vec<String>)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(rel_close) = s[i + 1..].find(']') {
                let close = i + 1 + rel_close;
                let inner = &s[i + 1..close];
                if let Some(vals) = parse_wildcard_spec(inner) {
                    return Some((i, close + 1, vals));
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    None
}

fn parse_wildcard_spec(spec: &str) -> Option<Vec<String>> {
    let (range_part, step) = match spec.split_once(':') {
        Some((r, st)) => {
            let parsed = st.trim().parse::<usize>().ok()?;
            if parsed == 0 {
                return None;
            }
            (r.trim(), parsed)
        }
        None => (spec.trim(), 1usize),
    };

    let (start_s, end_s) = range_part.split_once('-')?;
    let start_s = start_s.trim();
    let end_s = end_s.trim();
    if start_s.is_empty() || end_s.is_empty() {
        return None;
    }

    // 1) Numeric range: e.g. `001-024` or `1-10:2`
    if start_s.chars().all(|c| c.is_ascii_digit()) && end_s.chars().all(|c| c.is_ascii_digit()) {
        let start_n = start_s.parse::<u64>().ok()?;
        let end_n = end_s.parse::<u64>().ok()?;
        let pad_width = if start_s.starts_with('0') && start_s.len() > 1 {
            start_s.len()
        } else if end_s.starts_with('0') && end_s.len() > 1 {
            end_s.len()
        } else {
            0
        };
        let mut out = Vec::new();
        if start_n <= end_n {
            let mut cur = start_n;
            while cur <= end_n && out.len() <= MAX_BATCH_URLS {
                if pad_width > 0 {
                    out.push(format!("{cur:0pad_width$}"));
                } else {
                    out.push(cur.to_string());
                }
                match cur.checked_add(step as u64) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
        } else {
            let mut cur = start_n;
            while cur >= end_n && out.len() <= MAX_BATCH_URLS {
                if pad_width > 0 {
                    out.push(format!("{cur:0pad_width$}"));
                } else {
                    out.push(cur.to_string());
                }
                if cur < step as u64 {
                    break;
                }
                cur -= step as u64;
            }
        }
        return Some(out);
    }

    // 2) Single ASCII letter range: e.g. `a-f` or `A-Z`
    if start_s.len() == 1 && end_s.len() == 1 {
        let a = start_s.as_bytes()[0];
        let b = end_s.as_bytes()[0];
        let same_case = (a.is_ascii_lowercase() && b.is_ascii_lowercase())
            || (a.is_ascii_uppercase() && b.is_ascii_uppercase());
        if same_case && a <= b {
            let mut out = Vec::new();
            let mut cur = a;
            while cur <= b {
                out.push((cur as char).to_string());
                match cur.checked_add(step as u8) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            return Some(out);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_zero_padded_numeric_wildcard_and_step() {
        let r = expand_batch_input("https://cdn.example.com/ep_[001-005:2].mp4", "");
        assert_eq!(
            r.urls,
            vec![
                "https://cdn.example.com/ep_001.mp4",
                "https://cdn.example.com/ep_003.mp4",
                "https://cdn.example.com/ep_005.mp4",
            ]
        );
        assert!(!r.truncated);
    }

    #[test]
    fn expands_multi_wildcard_cartesian_and_letters() {
        let r = expand_batch_input("https://cdn.example.com/s[01-02]_[a-b].zip", "");
        assert_eq!(
            r.urls,
            vec![
                "https://cdn.example.com/s01_a.zip",
                "https://cdn.example.com/s01_b.zip",
                "https://cdn.example.com/s02_a.zip",
                "https://cdn.example.com/s02_b.zip",
            ]
        );
    }

    #[test]
    fn filters_multiline_mixed_text_and_supports_magnet() {
        let input = r#"
            # Comment line
            Download: https://a.com/movie.mp4
            Doc: https://a.com/readme.pdf
            magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=test
        "#;
        let r = expand_batch_input(input, ".mp4, magnet:");
        assert_eq!(r.urls.len(), 2);
        assert_eq!(r.urls[0], "https://a.com/movie.mp4");
        assert!(r.urls[1].starts_with("magnet:?"));
    }
}
