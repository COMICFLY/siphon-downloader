use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

pub type TaskId = Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolKind {
    Http,
    Hls,
    BitTorrent,
    Ed2k,
    Custom(String),
}

impl ProtocolKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Http => "http",
            Self::Hls => "hls",
            Self::BitTorrent => "bittorrent",
            Self::Ed2k => "ed2k",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "http" => Self::Http,
            "hls" => Self::Hls,
            "bittorrent" => Self::BitTorrent,
            "ed2k" => Self::Ed2k,
            other => Self::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Probing,
    Downloading,
    Paused,
    Verifying,
    Completed,
    Error,
    LinkExpired,
}

impl TaskStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Probing | Self::Downloading | Self::Verifying)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Probing => "probing",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Error => "error",
            Self::LinkExpired => "link_expired",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "probing" => Self::Probing,
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "verifying" => Self::Verifying,
            "completed" => Self::Completed,
            "link_expired" => Self::LinkExpired,
            _ => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    Video,
    Archive,
    Software,
    Document,
    DiskImage,
    Audio,
    Image,
    Other,
}

impl FileCategory {
    pub fn from_filename_and_mime(filename: &str, mime: Option<&str>) -> Self {
        let lower = filename.to_ascii_lowercase();
        let ext = lower
            .rsplit('.')
            .next()
            .filter(|e| *e != lower.as_str())
            .unwrap_or("");

        match ext {
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "m4v" | "ts" | "m3u8" => {
                return Self::Video;
            }
            "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" | "zst" | "tgz" => {
                return Self::Archive;
            }
            "exe" | "msi" | "msix" | "appx" | "dmg" | "deb" | "rpm" | "apk" | "pkg" => {
                return Self::Software;
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "epub"
            | "csv" => {
                return Self::Document;
            }
            "iso" | "img" | "vhd" | "vhdx" | "vmdk" | "qcow2" => {
                return Self::DiskImage;
            }
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" => {
                return Self::Audio;
            }
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" | "avif" | "ico" => {
                return Self::Image;
            }
            _ => {}
        }

        if let Some(m) = mime {
            let m = m.to_ascii_lowercase();
            if m.starts_with("video/") {
                return Self::Video;
            }
            if m.starts_with("audio/") {
                return Self::Audio;
            }
            if m.starts_with("image/") {
                return Self::Image;
            }
            if m.contains("zip") || m.contains("compressed") || m.contains("tar") {
                return Self::Archive;
            }
            if m.contains("pdf") || m.starts_with("text/") {
                return Self::Document;
            }
        }

        Self::Other
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Archive => "archive",
            Self::Software => "software",
            Self::Document => "document",
            Self::DiskImage => "disk_image",
            Self::Audio => "audio",
            Self::Image => "image",
            Self::Other => "other",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "video" => Self::Video,
            "archive" => Self::Archive,
            "software" => Self::Software,
            "document" => Self::Document,
            "disk_image" => Self::DiskImage,
            "audio" => Self::Audio,
            "image" => Self::Image,
            _ => Self::Other,
        }
    }

    pub fn folder_slug(self) -> &'static str {
        match self {
            Self::Video => "Videos",
            Self::Archive => "Archives",
            Self::Software => "Software",
            Self::Document => "Documents",
            Self::DiskImage => "Images",
            Self::Audio => "Audio",
            Self::Image => "Pictures",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    Pending,
    Active,
    Retrying,
    Done,
    Error,
}

impl SegmentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Retrying => "retrying",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "active" => Self::Active,
            "retrying" => Self::Retrying,
            "done" => Self::Done,
            _ => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentInfo {
    pub index: u32,
    pub start_offset: u64,
    pub current_offset: u64,
    /// Inclusive end offset. For single-stream unknown-size downloads, `u64::MAX`.
    pub end_offset: u64,
    pub state: SegmentState,
    pub speed_bps: u64,
}

impl SegmentInfo {
    pub fn total_bytes(&self) -> Option<u64> {
        if self.end_offset == u64::MAX || self.end_offset < self.start_offset {
            None
        } else {
            Some(self.end_offset - self.start_offset + 1)
        }
    }

    pub fn downloaded_bytes(&self) -> u64 {
        self.current_offset.saturating_sub(self.start_offset)
    }

    pub fn remaining_bytes(&self) -> u64 {
        if self.end_offset == u64::MAX {
            u64::MAX
        } else {
            self.end_offset
                .saturating_add(1)
                .saturating_sub(self.current_offset)
        }
    }

    pub fn progress_ratio(&self) -> f32 {
        match self.total_bytes() {
            Some(0) => 1.0,
            Some(total) => (self.downloaded_bytes() as f64 / total as f64).clamp(0.0, 1.0) as f32,
            None => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpeedSample {
    pub timestamp_ms: i64,
    pub bytes_per_sec: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Sha256,
    Md5,
    Blake3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumSpec {
    pub algorithm: HashAlgorithm,
    pub expected: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSource {
    Manual,
    QuickBar,
    Clipboard,
    DragDrop,
    Browser,
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: TaskId,
    pub protocol: ProtocolKind,
    pub original_url: String,
    pub resolved_url: String,
    pub filename: String,
    pub save_dir: PathBuf,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    pub status: TaskStatus,
    pub supports_range: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub max_connections: u32,
    /// Per-task speed limit in bytes/sec (`0` = unlimited).
    pub speed_limit_bps: u64,
    pub category: FileCategory,
    pub matched_rule: Option<String>,
    pub checksum: Option<ChecksumSpec>,
    pub source: TaskSource,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub segments: Vec<SegmentInfo>,
    pub speed_history: Vec<SpeedSample>,
    pub logs: Vec<TaskLogEntry>,
}

impl DownloadTask {
    pub fn target_path(&self) -> PathBuf {
        self.save_dir.join(&self.filename)
    }

    pub fn part_path(&self) -> PathBuf {
        self.save_dir.join(format!("{}.siphon.part", self.filename))
    }

    pub fn domain(&self) -> String {
        let low = self.original_url.trim().to_ascii_lowercase();
        if low.starts_with("ed2k://") {
            return "ed2k".to_string();
        }
        if low.starts_with("magnet:?") {
            return "magnet".to_string();
        }
        url::Url::parse(&self.resolved_url)
            .or_else(|_| url::Url::parse(&self.original_url))
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "direct".to_string())
    }

    pub fn progress_ratio(&self) -> f32 {
        match self.total_size {
            Some(0) => {
                if self.status == TaskStatus::Completed {
                    1.0
                } else {
                    0.0
                }
            }
            Some(total) => (self.downloaded_bytes as f64 / total as f64).clamp(0.0, 1.0) as f32,
            None => {
                if self.status == TaskStatus::Completed {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    pub fn push_log(&mut self, level: impl Into<String>, message: impl Into<String>) {
        self.logs.push(TaskLogEntry {
            timestamp: Utc::now(),
            level: level.into(),
            message: message.into(),
        });
        if self.logs.len() > 80 {
            let drain = self.logs.len() - 80;
            self.logs.drain(0..drain);
        }
    }

    pub fn push_speed_sample(&mut self, speed_bps: u64) {
        self.speed_history.push(SpeedSample {
            timestamp_ms: Utc::now().timestamp_millis(),
            bytes_per_sec: speed_bps,
        });
        if self.speed_history.len() > 120 {
            let drain = self.speed_history.len() - 120;
            self.speed_history.drain(0..drain);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeMetadata {
    pub final_url: String,
    pub filename: String,
    pub content_length: Option<u64>,
    pub content_type: Option<String>,
    pub supports_range: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub suggested_category: FileCategory,
    pub resolved_headers: BTreeMap<String, String>,
    pub plugin_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbePreview {
    pub metadata: ProbeMetadata,
    pub suggested_save_dir: PathBuf,
    pub suggested_connections: u32,
    pub suggested_speed_limit_bps: u64,
    pub matched_rule_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTaskRequest {
    pub url: String,
    pub filename: Option<String>,
    pub save_dir: Option<PathBuf>,
    pub max_connections: Option<u32>,
    pub speed_limit_bps: Option<u64>,
    pub headers: BTreeMap<String, String>,
    pub checksum: Option<ChecksumSpec>,
    pub source: TaskSource,
    pub auto_start: bool,
}

impl NewTaskRequest {
    pub fn quick(url: impl Into<String>, source: TaskSource) -> Self {
        Self {
            url: url.into(),
            filename: None,
            save_dir: None,
            max_connections: None,
            speed_limit_bps: None,
            headers: BTreeMap::new(),
            checksum: None,
            source,
            auto_start: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TaskEvent {
    TaskAdded(DownloadTask),
    TaskUpdated(DownloadTask),
    TaskProgress {
        task_id: TaskId,
        downloaded_bytes: u64,
        total_size: Option<u64>,
        speed_bps: u64,
        eta_secs: Option<u64>,
        segments: Vec<SegmentInfo>,
    },
    TaskCompleted {
        task_id: TaskId,
        filename: String,
        target_path: PathBuf,
    },
    TaskFailed {
        task_id: TaskId,
        error: String,
        link_expired: bool,
    },
    TaskRemoved(TaskId),
    GlobalSpeedTick {
        total_speed_bps: u64,
        active_count: usize,
    },
    LogEntry {
        task_id: TaskId,
        entry: TaskLogEntry,
    },
}

/// Format byte sizes cleanly for UI display.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_speed(bps: u64) -> String {
    if bps == 0 {
        "0 B/s".to_string()
    } else {
        format!("{}/s", format_bytes(bps))
    }
}

pub fn format_eta(secs: Option<u64>) -> String {
    match secs {
        None => "--".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m {:02}s", s / 60, s % 60),
        Some(s) => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
    }
}
