use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_DOWNLOADED_HISTORY_PER_FEED: usize = 500;

fn default_true() -> bool {
    true
}

fn default_refresh_interval_mins() -> u32 {
    15
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RssLinkKind {
    Magnet,
    Torrent,
    Ed2k,
    #[default]
    Http,
}

impl RssLinkKind {
    pub fn from_url(url: &str) -> Self {
        let low = url.trim().to_ascii_lowercase();
        if low.starts_with("magnet:?") {
            Self::Magnet
        } else if low.starts_with("ed2k://") {
            Self::Ed2k
        } else if low.ends_with(".torrent") || low.contains(".torrent?") {
            Self::Torrent
        } else {
            Self::Http
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Magnet => "MAGNET",
            Self::Torrent => "BT",
            Self::Ed2k => "eD2k",
            Self::Http => "HTTP",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssItem {
    pub guid: String,
    pub title: String,
    pub download_url: String,
    pub page_url: Option<String>,
    pub published_at: Option<String>,
    pub size_bytes: Option<u64>,
    pub kind: RssLinkKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RssFeed {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Automatically download newly discovered items that satisfy `include_filter` / `exclude_filter`.
    pub auto_download: bool,
    /// Include keywords (space/comma = AND, `|` = OR) or regex wrapped in `/.../` (case-insensitive).
    pub include_filter: String,
    /// Exclude keywords (space/comma = AND, `|` = OR) or regex wrapped in `/.../` (case-insensitive).
    pub exclude_filter: String,
    /// Optional per-feed download directory; `None` falls back to default save dir / rules.
    pub save_dir: Option<PathBuf>,
    /// Per-feed refresh interval in minutes (`0` = inherit global `default_refresh_interval_mins`).
    pub refresh_interval_mins: u32,
    pub last_refreshed_at: Option<String>,
    pub last_error: Option<String>,
    /// Deduplication history of downloaded / marked-read item GUIDs (newest last, capped at 500).
    pub downloaded_ids: Vec<String>,
}

impl Default for RssFeed {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            url: String::new(),
            enabled: true,
            auto_download: false,
            include_filter: String::new(),
            exclude_filter: String::new(),
            save_dir: None,
            refresh_interval_mins: 0,
            last_refreshed_at: None,
            last_error: None,
            downloaded_ids: Vec::new(),
        }
    }
}

impl RssFeed {
    pub fn effective_interval_mins(&self, global_default: u32) -> u32 {
        let raw = if self.refresh_interval_mins > 0 {
            self.refresh_interval_mins
        } else {
            global_default
        };
        raw.clamp(5, 1440)
    }

    pub fn display_name(&self) -> &str {
        let trimmed = self.name.trim();
        if !trimmed.is_empty() {
            trimmed
        } else {
            self.url.trim()
        }
    }

    pub fn has_downloaded(&self, guid: &str) -> bool {
        self.downloaded_ids.iter().any(|id| id == guid)
    }

    pub fn mark_downloaded(&mut self, guid: impl Into<String>) {
        let g = guid.into();
        if g.is_empty() || self.has_downloaded(&g) {
            return;
        }
        self.downloaded_ids.push(g);
        if self.downloaded_ids.len() > MAX_DOWNLOADED_HISTORY_PER_FEED {
            let excess = self.downloaded_ids.len() - MAX_DOWNLOADED_HISTORY_PER_FEED;
            self.downloaded_ids.drain(0..excess);
        }
    }

    /// Evaluate `include_filter` and `exclude_filter` against an RSS item's title.
    pub fn matches_title(&self, title: &str) -> bool {
        let inc = self.include_filter.trim();
        if !inc.is_empty() && !matches_filter_expr(title, inc) {
            return false;
        }
        let exc = self.exclude_filter.trim();
        if !exc.is_empty() && matches_filter_expr(title, exc) {
            return false;
        }
        true
    }
}

/// Evaluate a user filter expression against `haystack` (case-insensitive):
/// - `/pattern/` or `re:pattern`: regular expression match
/// - Otherwise: `|` separates OR groups; whitespace or `,` / `，` inside a group are AND terms.
pub fn matches_filter_expr(haystack: &str, expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return true;
    }

    if let Some(pat) = extract_regex_pattern(trimmed) {
        return RegexBuilder::new(pat)
            .case_insensitive(true)
            .build()
            .is_ok_and(|re| re.is_match(haystack));
    }

    let hay_low = haystack.to_lowercase();
    let mut any_group = false;
    for group in trimmed.split('|') {
        let tokens: Vec<String> = group
            .split(|c: char| c.is_whitespace() || c == ',' || c == '，' || c == ';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        if tokens.is_empty() {
            continue;
        }
        any_group = true;
        if tokens.iter().all(|tok| hay_low.contains(tok)) {
            return true;
        }
    }
    !any_group
}

fn extract_regex_pattern(expr: &str) -> Option<&str> {
    if let Some(rest) = expr.strip_prefix("re:") {
        let r = rest.trim();
        if !r.is_empty() {
            return Some(r);
        }
    }
    if expr.len() >= 3 && expr.starts_with('/') {
        if let Some(inner) = expr.strip_prefix('/').and_then(|s| s.strip_suffix("/i")) {
            if !inner.is_empty() {
                return Some(inner);
            }
        }
        if let Some(inner) = expr.strip_prefix('/').and_then(|s| s.strip_suffix('/')) {
            if !inner.is_empty() {
                return Some(inner);
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RssConfig {
    /// Master switch for automatic periodic RSS feed polling.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Global default refresh interval in minutes (clamped 5..=1440).
    #[serde(default = "default_refresh_interval_mins")]
    pub default_refresh_interval_mins: u32,
    pub feeds: Vec<RssFeed>,
}

impl Default for RssConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_refresh_interval_mins: 15,
            feeds: Vec::new(),
        }
    }
}

/// On-disk cache of fetched RSS items (stored in `<data_dir>/rss_cache.json` to keep `config.toml` clean).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RssCache {
    pub items_by_feed: HashMap<Uuid, Vec<RssItem>>,
}

impl RssCache {
    pub fn cache_path(data_dir: &Path) -> PathBuf {
        data_dir.join("rss_cache.json")
    }

    pub fn load_or_default(data_dir: &Path) -> Self {
        let path = Self::cache_path(data_dir);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let json = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(Self::cache_path(data_dir), json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_supports_and_or_keywords_exclude_and_regex() {
        let feed = RssFeed {
            include_filter: "1080p 简日 | 1080p CHS".to_string(),
            exclude_filter: "720p | 繁日".to_string(),
            ..Default::default()
        };
        assert!(feed.matches_title("[Sub] Anime Title - 01 [1080p][简日内嵌].mkv"));
        assert!(feed.matches_title("[Sub] Anime Title - 02 [1080P][CHS].mp4"));
        assert!(!feed.matches_title("[Sub] Anime Title - 02 [720p][简日].mkv"));
        assert!(!feed.matches_title("[Sub] Anime Title - 02 [1080p][繁日].mkv"));
        assert!(!feed.matches_title("[Sub] Anime Title - 02 [1080p][RAW].mkv"));

        let regex_feed = RssFeed {
            include_filter: r"/S02E\d{2}.*1080p/i".to_string(),
            exclude_filter: "/HEVC/".to_string(),
            ..Default::default()
        };
        assert!(regex_feed.matches_title("Show.S02E05.1080p.WEB-DL.x264"));
        assert!(!regex_feed.matches_title("Show.S02E05.1080p.HEVC"));
        assert!(!regex_feed.matches_title("Show.S01E05.1080p.WEB-DL"));
    }

    #[test]
    fn feed_dedup_history_caps_and_prevents_duplicates() {
        let mut feed = RssFeed::default();
        for i in 0..520 {
            feed.mark_downloaded(format!("guid-{i}"));
        }
        assert_eq!(feed.downloaded_ids.len(), MAX_DOWNLOADED_HISTORY_PER_FEED);
        assert!(!feed.has_downloaded("guid-0"));
        assert!(feed.has_downloaded("guid-519"));
        feed.mark_downloaded("guid-519");
        assert_eq!(feed.downloaded_ids.len(), MAX_DOWNLOADED_HISTORY_PER_FEED);
    }

    #[test]
    fn rss_cache_roundtrips_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let feed_id = Uuid::new_v4();
        let mut cache = RssCache::default();
        cache.items_by_feed.insert(
            feed_id,
            vec![RssItem {
                guid: "ep-01".into(),
                title: "Episode 01 [1080p]".into(),
                download_url: "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
                    .into(),
                page_url: Some("https://example.com/ep-01".into()),
                published_at: Some("2026-11-24 20:00".into()),
                size_bytes: Some(1_048_576),
                kind: RssLinkKind::Magnet,
            }],
        );
        cache.save(tmp.path()).unwrap();
        let loaded = RssCache::load_or_default(tmp.path());
        assert_eq!(loaded.items_by_feed.get(&feed_id).unwrap().len(), 1);
        assert_eq!(
            loaded.items_by_feed.get(&feed_id).unwrap()[0].kind,
            RssLinkKind::Magnet
        );
    }
}
