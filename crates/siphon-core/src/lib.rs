pub mod batch;
pub mod config;
pub mod error;
pub mod limiter;
pub mod manager;
pub mod model;
pub mod plugin;
pub mod protocol;
pub mod rules;
pub mod storage;

pub use batch::{BatchExpandResult, MAX_BATCH_URLS, expand_batch_input};
pub use config::{
    AppConfig, BtConfig, Ed2kConfig, Ed2kServerEntry, Language, ListDensity, QueueCompleteAction,
    ScheduleConfig, ThemeMode, default_ed2k_servers, default_tracker_subscribe_urls,
    is_time_in_window, parse_hhmm, parse_tracker_list,
};
pub use error::{Result, SiphonError};
pub use limiter::{HierarchicalLimiter, TokenBucket};
pub use manager::{
    DownloadManager, deduplicate_filename, infer_filename_from_raw_url, infer_filename_from_url,
    sanitize_filename,
};
pub use model::{
    ChecksumSpec, DownloadTask, FileCategory, HashAlgorithm, NewTaskRequest, ProbeMetadata,
    ProbePreview, ProtocolKind, SegmentInfo, SegmentState, SpeedSample, TaskEvent, TaskId,
    TaskLogEntry, TaskSource, TaskStatus, format_bytes, format_eta, format_speed,
};
pub use plugin::{
    ExternalPlugin, ExtraTaskSpec, LinkResolver, PluginManifest, PluginRegistry, PostDownloadInput,
    PostDownloadOutput, PostProcessor, ResolveInput, ResolveOutput,
};
pub use protocol::{DownloadContext, DownloadOutcome, EngineUpdate, ProtocolHandler};
pub use rules::{AutomationRule, RuleAction, RuleCondition, RuleEngine, RuleEvalInput, RuleMatchResult};
pub use storage::TaskStore;
