use crate::config::{BtConfig, Ed2kConfig};
use crate::error::Result;
use crate::limiter::HierarchicalLimiter;
use crate::model::{
    BtLiveTelemetry, DownloadTask, ProbeMetadata, ProtocolKind, SegmentInfo, TaskLogEntry,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum EngineUpdate {
    Probed(ProbeMetadata),
    Progress {
        downloaded_bytes: u64,
        total_size: Option<u64>,
        speed_bps: u64,
        eta_secs: Option<u64>,
        segments: Vec<SegmentInfo>,
    },
    BtTelemetry(BtLiveTelemetry),
    Verifying,
    Log(TaskLogEntry),
}

#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    pub total_bytes: u64,
    pub verified_hash: Option<String>,
}

pub struct DownloadContext {
    pub task: DownloadTask,
    pub cancel: CancellationToken,
    pub limiter: HierarchicalLimiter,
    pub updates_tx: mpsc::UnboundedSender<EngineUpdate>,
    pub min_split_size_bytes: u64,
    pub proxy_url: Option<String>,
    pub user_agent: String,
    pub live_max_connections: Arc<AtomicU32>,
    pub bt_config: BtConfig,
    pub ed2k_config: Ed2kConfig,
}

/// Extensible protocol engine trait.
///
/// Phase 1 implements `HttpEngine` (`ProtocolKind::Http`).
/// Future phases plug `HlsEngine` and `BitTorrentEngine` directly into this same trait.
#[async_trait]
pub trait ProtocolHandler: Send + Sync {
    fn kind(&self) -> ProtocolKind;

    fn supports_url(&self, url: &str) -> bool;

    async fn probe(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        proxy_url: Option<&str>,
        user_agent: &str,
    ) -> Result<ProbeMetadata>;

    async fn download(&self, ctx: DownloadContext) -> Result<DownloadOutcome>;
}
