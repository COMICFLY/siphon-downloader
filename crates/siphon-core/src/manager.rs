use crate::config::AppConfig;
use crate::error::{Result, SiphonError};
use crate::limiter::{HierarchicalLimiter, TokenBucket};
use crate::model::{
    DownloadTask, FileCategory, NewTaskRequest, ProbePreview, ProtocolKind, SegmentState,
    TaskEvent, TaskId, TaskSource, TaskStatus,
};
use crate::plugin::{PluginRegistry, PostDownloadInput, ResolveInput};
use crate::protocol::{DownloadContext, EngineUpdate, ProtocolHandler};
use crate::rules::{AutomationRule, RuleEngine, RuleEvalInput};
use crate::storage::TaskStore;
use chrono::Utc;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

struct ActiveHandle {
    cancel: CancellationToken,
    limiter: Arc<TokenBucket>,
    live_connections: Arc<AtomicU32>,
}

pub struct DownloadManager {
    data_dir: PathBuf,
    store: Arc<TaskStore>,
    config: RwLock<AppConfig>,
    rules: RwLock<RuleEngine>,
    plugins: RwLock<PluginRegistry>,
    protocols: Vec<Arc<dyn ProtocolHandler>>,
    global_limiter: Arc<TokenBucket>,
    tasks: RwLock<HashMap<TaskId, DownloadTask>>,
    active: RwLock<HashMap<TaskId, ActiveHandle>>,
    events_tx: broadcast::Sender<TaskEvent>,
}

impl DownloadManager {
    pub fn new(
        data_dir: PathBuf,
        protocols: Vec<Arc<dyn ProtocolHandler>>,
        extra_plugin_dirs: Vec<PathBuf>,
    ) -> Result<Arc<Self>> {
        let config = AppConfig::load_or_default(&data_dir)?;
        let store = Arc::new(TaskStore::open(&AppConfig::db_path(&data_dir))?);
        let recovered = store.recover_interrupted_tasks()?;
        if recovered > 0 {
            tracing::info!(recovered, "已从上次中断恢复任务状态为已暂停");
        }

        let loaded_tasks = store.load_all_tasks()?;
        let mut task_map = HashMap::with_capacity(loaded_tasks.len());
        for t in loaded_tasks {
            task_map.insert(t.id, t);
        }

        let rules_vec = store.load_or_seed_rules()?;
        let mut plugin_dirs = vec![AppConfig::plugins_dir(&data_dir)];
        plugin_dirs.extend(extra_plugin_dirs);
        let plugins = PluginRegistry::new(plugin_dirs);

        let global_limiter = TokenBucket::new(config.global_speed_limit_bps);
        let (events_tx, _) = broadcast::channel(512);

        let mgr = Arc::new(Self {
            data_dir,
            store,
            config: RwLock::new(config),
            rules: RwLock::new(RuleEngine::new(rules_vec)),
            plugins: RwLock::new(plugins),
            protocols,
            global_limiter,
            tasks: RwLock::new(task_map),
            active: RwLock::new(HashMap::new()),
            events_tx,
        });

        Self::spawn_global_speed_ticker(Arc::clone(&mgr));
        Ok(mgr)
    }

    pub fn new_in_memory(
        data_dir: PathBuf,
        protocols: Vec<Arc<dyn ProtocolHandler>>,
    ) -> Result<Arc<Self>> {
        let mut config = AppConfig::default();
        config.default_save_dir = data_dir.join("downloads");
        std::fs::create_dir_all(&config.default_save_dir)?;
        let store = Arc::new(TaskStore::open_in_memory()?);
        let rules_vec = store.load_or_seed_rules()?;
        let global_limiter = TokenBucket::new(0);
        let (events_tx, _) = broadcast::channel(256);

        Ok(Arc::new(Self {
            data_dir,
            store,
            config: RwLock::new(config),
            rules: RwLock::new(RuleEngine::new(rules_vec)),
            plugins: RwLock::new(PluginRegistry::default()),
            protocols,
            global_limiter,
            tasks: RwLock::new(HashMap::new()),
            active: RwLock::new(HashMap::new()),
            events_tx,
        }))
    }

    fn spawn_global_speed_ticker(mgr: Arc<Self>) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(500));
                loop {
                    ticker.tick().await;
                    let (total_speed_bps, active_count) = {
                        let tasks = mgr.tasks.read().await;
                        let mut speed = 0u64;
                        let mut count = 0usize;
                        for t in tasks.values() {
                            if t.status.is_active() {
                                count += 1;
                                speed = speed.saturating_add(t.speed_bps);
                            }
                        }
                        (speed, count)
                    };
                    let _ = mgr.events_tx.send(TaskEvent::GlobalSpeedTick {
                        total_speed_bps,
                        active_count,
                    });
                }
            });
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.events_tx.subscribe()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub async fn config(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    pub async fn update_config(&self, mut new_cfg: AppConfig) -> Result<()> {
        new_cfg.default_connections = new_cfg.default_connections.clamp(1, 32);
        new_cfg.max_concurrent_tasks = new_cfg.max_concurrent_tasks.clamp(1, 16);
        new_cfg.min_split_size_bytes = new_cfg.min_split_size_bytes.max(128 * 1024);
        self.global_limiter
            .set_limit(new_cfg.global_speed_limit_bps);
        new_cfg.save(&self.data_dir)?;
        *self.config.write().await = new_cfg;
        Ok(())
    }

    pub async fn update_global_speed_limit(&self, limit_bps: u64) -> Result<()> {
        self.global_limiter.set_limit(limit_bps);
        let mut cfg = self.config.write().await;
        cfg.global_speed_limit_bps = limit_bps;
        cfg.save(&self.data_dir)?;
        Ok(())
    }

    pub async fn list_tasks(&self) -> Vec<DownloadTask> {
        let mut list: Vec<DownloadTask> = self.tasks.read().await.values().cloned().collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub async fn get_task(&self, id: TaskId) -> Option<DownloadTask> {
        self.tasks.read().await.get(&id).cloned()
    }

    pub async fn list_rules(&self) -> Vec<AutomationRule> {
        self.rules.read().await.rules().to_vec()
    }

    pub async fn save_rules(&self, rules: Vec<AutomationRule>) -> Result<()> {
        self.store.save_rules(&rules)?;
        self.rules.write().await.set_rules(rules);
        Ok(())
    }

    pub async fn list_plugins(&self) -> Vec<crate::plugin::ExternalPlugin> {
        self.plugins.read().await.external_plugins().to_vec()
    }

    pub async fn reload_plugins(&self) -> Vec<crate::plugin::ExternalPlugin> {
        let mut plugins = self.plugins.write().await;
        plugins.reload();
        plugins.external_plugins().to_vec()
    }

    pub async fn set_plugin_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.plugins.write().await.set_plugin_enabled(name, enabled)
    }

    fn find_protocol(&self, url: &str) -> Result<Arc<dyn ProtocolHandler>> {
        self.protocols
            .iter()
            .find(|p| p.supports_url(url))
            .cloned()
            .ok_or_else(|| SiphonError::UnsupportedProtocol(url.to_string()))
    }

    fn normalize_input_url(raw: &str) -> Result<(String, Option<Url>)> {
        let trimmed = raw.trim().trim_matches('"');
        if trimmed.to_ascii_lowercase().starts_with("ed2k://|") {
            return Ok((trimmed.to_string(), None));
        }
        if let Ok(u) = Url::parse(trimmed) {
            return Ok((trimmed.to_string(), Some(u)));
        }
        // Allow local `.torrent` file paths cleanly by turning them into `file://` URLs
        let p = Path::new(trimmed);
        if p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("torrent"))
            && let Ok(u) = Url::from_file_path(p)
        {
            return Ok((u.to_string(), Some(u)));
        }
        Err(SiphonError::InvalidUrl(trimmed.to_string()))
    }

    /// Run plugins + protocol probe + rule engine preview without creating a task.
    pub async fn probe_url(
        &self,
        raw_url: &str,
        headers: &BTreeMap<String, String>,
        source: TaskSource,
    ) -> Result<ProbePreview> {
        let (cleaned, _) = Self::normalize_input_url(raw_url)?;

        let resolved = {
            let plugins = self.plugins.read().await;
            plugins
                .run_resolve(&ResolveInput {
                    url: cleaned,
                    headers: headers.clone(),
                })
                .await
        };

        let protocol = self.find_protocol(&resolved.url)?;
        let cfg = self.config.read().await.clone();
        let mut meta = protocol
            .probe(
                &resolved.url,
                &resolved.headers,
                cfg.proxy_url.as_deref(),
                &cfg.user_agent,
            )
            .await?;

        if let Some(custom_name) = resolved.filename
            && !custom_name.trim().is_empty()
        {
            meta.filename = sanitize_filename(&custom_name);
        }
        meta.resolved_headers = resolved.headers;
        meta.plugin_note = resolved.note;

        let domain = Url::parse(&meta.final_url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "direct".to_string());

        let rule_match = {
            let engine = self.rules.read().await;
            engine.evaluate(&RuleEvalInput {
                url: &meta.final_url,
                domain: &domain,
                filename: &meta.filename,
                mime: meta.content_type.as_deref(),
                size_bytes: meta.content_length,
                category: meta.suggested_category,
                source,
                base_save_dir: &cfg.default_save_dir,
            })
        };

        Ok(ProbePreview {
            suggested_save_dir: rule_match
                .save_dir
                .unwrap_or_else(|| cfg.default_save_dir.clone()),
            suggested_connections: rule_match
                .connections_override
                .unwrap_or(cfg.default_connections),
            suggested_speed_limit_bps: rule_match.speed_limit_bps.unwrap_or(0),
            matched_rule_name: rule_match.matched_rules.first().cloned(),
            metadata: meta,
        })
    }

    /// Create a new download task, apply plugins & rules, persist to SQLite, and optionally queue/start.
    pub async fn add_task(self: &Arc<Self>, req: NewTaskRequest) -> Result<DownloadTask> {
        let (task, extra_tasks, should_start) = self.add_task_inner(req, true).await?;
        for extra in extra_tasks {
            let sub_req = NewTaskRequest {
                url: extra.url,
                filename: extra.filename,
                save_dir: Some(task.save_dir.clone()),
                max_connections: Some(task.max_connections),
                speed_limit_bps: Some(task.speed_limit_bps),
                headers: extra.headers,
                checksum: None,
                selected_files: None,
                source: TaskSource::Plugin,
                auto_start: should_start,
            };
            if let Err(e) = self.add_task_inner(sub_req, false).await {
                tracing::warn!(error = %e, "添加插件展开的子任务失败");
            }
        }
        Ok(task)
    }

    async fn add_task_inner(
        self: &Arc<Self>,
        req: NewTaskRequest,
        run_resolve_plugins: bool,
    ) -> Result<(DownloadTask, Vec<crate::plugin::ExtraTaskSpec>, bool)> {
        let (cleaned_url, parsed) = Self::normalize_input_url(&req.url)?;

        let resolved = if run_resolve_plugins {
            let plugins = self.plugins.read().await;
            plugins
                .run_resolve(&ResolveInput {
                    url: cleaned_url.clone(),
                    headers: req.headers.clone(),
                })
                .await
        } else {
            crate::plugin::ResolveOutput {
                url: cleaned_url.clone(),
                headers: req.headers.clone(),
                filename: None,
                extra_tasks: Vec::new(),
                note: None,
            }
        };

        let protocol = self.find_protocol(&resolved.url)?;
        let cfg = self.config.read().await.clone();

        let fallback_name = req
            .filename
            .clone()
            .or_else(|| resolved.filename.clone())
            .unwrap_or_else(|| infer_filename_from_raw_url(&resolved.url, parsed.as_ref()));
        let initial_filename = sanitize_filename(&fallback_name);
        let initial_category = FileCategory::from_filename_and_mime(&initial_filename, None);
        let domain = Url::parse(&resolved.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "direct".to_string());

        let rule_match = {
            let engine = self.rules.read().await;
            engine.evaluate(&RuleEvalInput {
                url: &resolved.url,
                domain: &domain,
                filename: &initial_filename,
                mime: None,
                size_bytes: None,
                category: initial_category,
                source: req.source,
                base_save_dir: &cfg.default_save_dir,
            })
        };

        let save_dir = req
            .save_dir
            .clone()
            .or(rule_match.save_dir)
            .unwrap_or_else(|| cfg.default_save_dir.clone());
        std::fs::create_dir_all(&save_dir)?;

        let unique_filename = deduplicate_filename(&save_dir, &initial_filename);
        let connections = req
            .max_connections
            .or(rule_match.connections_override)
            .unwrap_or(cfg.default_connections)
            .clamp(1, 32);
        let speed_limit = req
            .speed_limit_bps
            .or(rule_match.speed_limit_bps)
            .unwrap_or(0);
        let should_start = req.auto_start && rule_match.auto_start.unwrap_or(true);

        let mut task = DownloadTask {
            id: Uuid::new_v4(),
            protocol: protocol.kind(),
            original_url: cleaned_url,
            resolved_url: resolved.url,
            filename: unique_filename,
            save_dir,
            total_size: None,
            downloaded_bytes: 0,
            speed_bps: 0,
            eta_secs: None,
            status: if should_start {
                TaskStatus::Queued
            } else {
                TaskStatus::Paused
            },
            supports_range: false,
            etag: None,
            last_modified: None,
            content_type: None,
            headers: resolved.headers,
            max_connections: connections,
            speed_limit_bps: speed_limit,
            category: rule_match.category_override.unwrap_or(initial_category),
            matched_rule: rule_match.matched_rules.first().cloned(),
            checksum: req.checksum,
            selected_files: req.selected_files,
            source: req.source,
            error_message: None,
            created_at: Utc::now(),
            completed_at: None,
            segments: Vec::new(),
            speed_history: Vec::new(),
            logs: Vec::new(),
            bt_telemetry: None,
        };

        if let Some(note) = resolved.note {
            task.push_log("INFO", format!("插件解析: {note}"));
        }
        if let Some(rule_name) = &task.matched_rule {
            task.push_log("INFO", format!("命中自动化规则: {rule_name}"));
        }
        task.push_log(
            "INFO",
            format!("任务已创建，目标文件: {}", task.target_path().display()),
        );

        self.store.upsert_task(&task)?;
        for entry in &task.logs {
            let _ = self.store.append_log(task.id, entry);
        }
        self.tasks.write().await.insert(task.id, task.clone());
        let _ = self.events_tx.send(TaskEvent::TaskAdded(task.clone()));

        if should_start {
            self.pump_queue().await;
        }

        Ok((task, resolved.extra_tasks, should_start))
    }

    pub async fn start_task(self: &Arc<Self>, id: TaskId) -> Result<()> {
        {
            let mut tasks = self.tasks.write().await;
            let task = tasks
                .get_mut(&id)
                .ok_or_else(|| SiphonError::TaskNotFound(id.to_string()))?;
            if task.status.is_active()
                || task.status == TaskStatus::Seeding
                || (task.status == TaskStatus::Completed
                    && task.protocol != ProtocolKind::BitTorrent)
            {
                return Ok(());
            }
            let restarting_seed =
                task.status == TaskStatus::Completed && task.protocol == ProtocolKind::BitTorrent;
            if restarting_seed && !self.config.read().await.bt.enabled {
                return Err(SiphonError::Config(
                    "BitTorrent 引擎当前已关闭，请先在「BT / Tracker」页开启开关".to_string(),
                ));
            }
            task.status = TaskStatus::Queued;
            task.error_message = None;
            task.push_log(
                "INFO",
                if restarting_seed {
                    "已加入队列（校验后继续做种）"
                } else {
                    "已加入下载队列"
                },
            );
            self.store.upsert_task(task)?;
            let _ = self.events_tx.send(TaskEvent::TaskUpdated(task.clone()));
        }
        self.pump_queue().await;
        Ok(())
    }

    pub async fn pause_task(self: &Arc<Self>, id: TaskId) -> Result<()> {
        if let Some(handle) = self.active.write().await.remove(&id) {
            handle.cancel.cancel();
        }
        {
            let mut tasks = self.tasks.write().await;
            if let Some(task) = tasks.get_mut(&id)
                && task.status != TaskStatus::Completed
            {
                let was_seeding = task.status == TaskStatus::Seeding;
                task.status = if was_seeding {
                    TaskStatus::Completed
                } else {
                    TaskStatus::Paused
                };
                task.speed_bps = 0;
                task.eta_secs = if was_seeding { Some(0) } else { None };
                if !was_seeding {
                    for seg in &mut task.segments {
                        if matches!(seg.state, SegmentState::Active | SegmentState::Retrying) {
                            seg.state = SegmentState::Pending;
                            seg.speed_bps = 0;
                        }
                    }
                }
                if let Some(tel) = &mut task.bt_telemetry {
                    tel.is_seeding = false;
                    tel.upload_speed_bps = 0;
                    tel.live_peers = 0;
                    tel.phase = if was_seeding {
                        "已完成".to_string()
                    } else {
                        "已暂停".to_string()
                    };
                }
                task.push_log(
                    "INFO",
                    if was_seeding {
                        "已手动停止做种"
                    } else {
                        "任务已暂停"
                    },
                );
                self.store.upsert_task(task)?;
                let _ = self.events_tx.send(TaskEvent::TaskUpdated(task.clone()));
            }
        }
        self.pump_queue().await;
        Ok(())
    }

    pub async fn pause_all(self: &Arc<Self>) {
        let ids: Vec<TaskId> = {
            let tasks = self.tasks.read().await;
            tasks
                .values()
                .filter(|t| t.status.is_active() || t.status == TaskStatus::Queued)
                .map(|t| t.id)
                .collect()
        };
        for id in ids {
            let _ = self.pause_task(id).await;
        }
    }

    pub async fn resume_all(self: &Arc<Self>) {
        let ids: Vec<TaskId> = {
            let tasks = self.tasks.read().await;
            tasks
                .values()
                .filter(|t| matches!(t.status, TaskStatus::Paused | TaskStatus::Error))
                .map(|t| t.id)
                .collect()
        };
        for id in ids {
            let _ = self.start_task(id).await;
        }
    }

    /// Pause all active or queued BitTorrent download tasks (leaving active seeders untouched).
    pub async fn pause_all_torrents(self: &Arc<Self>) -> usize {
        let ids: Vec<TaskId> = {
            let tasks = self.tasks.read().await;
            tasks
                .values()
                .filter(|t| {
                    t.protocol == ProtocolKind::BitTorrent
                        && (t.status.is_active() || t.status == TaskStatus::Queued)
                })
                .map(|t| t.id)
                .collect()
        };
        let mut paused = 0usize;
        for id in ids {
            if self.pause_task(id).await.is_ok() {
                paused += 1;
            }
        }
        paused
    }

    /// Resume all paused or errored BitTorrent download tasks.
    pub async fn resume_all_torrents(self: &Arc<Self>) -> Result<usize> {
        if !self.config.read().await.bt.enabled {
            return Err(SiphonError::Config(
                "BitTorrent 引擎当前已关闭，请先开启引擎开关".to_string(),
            ));
        }
        let ids: Vec<TaskId> = {
            let tasks = self.tasks.read().await;
            tasks
                .values()
                .filter(|t| {
                    t.protocol == ProtocolKind::BitTorrent
                        && matches!(t.status, TaskStatus::Paused | TaskStatus::Error)
                })
                .map(|t| t.id)
                .collect()
        };
        let mut resumed = 0usize;
        for id in ids {
            if self.start_task(id).await.is_ok() {
                resumed += 1;
            }
        }
        Ok(resumed)
    }

    /// Start seeding all completed BitTorrent tasks that are currently stopped.
    pub async fn start_all_seeding(self: &Arc<Self>) -> Result<usize> {
        if !self.config.read().await.bt.enabled {
            return Err(SiphonError::Config(
                "BitTorrent 引擎当前已关闭，请先开启引擎开关".to_string(),
            ));
        }
        let ids: Vec<TaskId> = {
            let tasks = self.tasks.read().await;
            tasks
                .values()
                .filter(|t| {
                    t.protocol == ProtocolKind::BitTorrent && t.status == TaskStatus::Completed
                })
                .map(|t| t.id)
                .collect()
        };
        let mut started = 0usize;
        for id in ids {
            if self.start_task(id).await.is_ok() {
                started += 1;
            }
        }
        Ok(started)
    }

    /// Stop seeding all currently seeding BitTorrent tasks (transitioning them to Completed).
    pub async fn stop_all_seeding(self: &Arc<Self>) -> usize {
        let ids: Vec<TaskId> = {
            let tasks = self.tasks.read().await;
            tasks
                .values()
                .filter(|t| {
                    t.protocol == ProtocolKind::BitTorrent && t.status == TaskStatus::Seeding
                })
                .map(|t| t.id)
                .collect()
        };
        let mut stopped = 0usize;
        for id in ids {
            if self.pause_task(id).await.is_ok() {
                stopped += 1;
            }
        }
        stopped
    }

    pub async fn remove_task(self: &Arc<Self>, id: TaskId, delete_files: bool) -> Result<()> {
        let was_active = if let Some(handle) = self.active.write().await.remove(&id) {
            handle.cancel.cancel();
            true
        } else {
            false
        };
        let removed = self.tasks.write().await.remove(&id);
        self.store.delete_task(id)?;
        if let Some(task) = removed
            && delete_files
        {
            let part = task.part_path();
            if part.exists() {
                if was_active {
                    // Give cancelled Windows worker handles a brief moment to close before unlinking
                    for _ in 0..8 {
                        if std::fs::remove_file(&part).is_ok() || !part.exists() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                } else {
                    let _ = std::fs::remove_file(&part);
                }
            }
            if task.status.is_finished() {
                let target = task.target_path();
                if target.exists() {
                    let _ = std::fs::remove_file(&target);
                }
            }
        }
        let _ = self.events_tx.send(TaskEvent::TaskRemoved(id));
        self.pump_queue().await;
        Ok(())
    }

    /// Replace an expired direct link while preserving already downloaded segments!
    pub async fn replace_task_url(
        self: &Arc<Self>,
        id: TaskId,
        new_url: String,
        auto_resume: bool,
    ) -> Result<()> {
        let cleaned = new_url.trim().to_string();
        let _ = Url::parse(&cleaned).map_err(|_| SiphonError::InvalidUrl(cleaned.clone()))?;

        if let Some(handle) = self.active.write().await.remove(&id) {
            handle.cancel.cancel();
        }

        {
            let mut tasks = self.tasks.write().await;
            let task = tasks
                .get_mut(&id)
                .ok_or_else(|| SiphonError::TaskNotFound(id.to_string()))?;
            task.resolved_url = cleaned;
            task.error_message = None;
            task.status = TaskStatus::Paused;
            task.push_log("INFO", "已替换下载直链，保留断点进度");
            self.store.upsert_task(task)?;
            let _ = self.events_tx.send(TaskEvent::TaskUpdated(task.clone()));
        }

        if auto_resume {
            self.start_task(id).await?;
        }
        Ok(())
    }

    pub async fn update_task_speed_limit(&self, id: TaskId, limit_bps: u64) -> Result<()> {
        if let Some(handle) = self.active.read().await.get(&id) {
            handle.limiter.set_limit(limit_bps);
        }
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(&id) {
            task.speed_limit_bps = limit_bps;
            self.store.upsert_task(task)?;
            let _ = self.events_tx.send(TaskEvent::TaskUpdated(task.clone()));
        }
        Ok(())
    }

    pub async fn update_task_connections(&self, id: TaskId, connections: u32) -> Result<()> {
        let clamped = connections.clamp(1, 32);
        if let Some(handle) = self.active.read().await.get(&id) {
            handle.live_connections.store(clamped, Ordering::Relaxed);
        }
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(&id) {
            task.max_connections = clamped;
            self.store.upsert_task(task)?;
            let _ = self.events_tx.send(TaskEvent::TaskUpdated(task.clone()));
        }
        Ok(())
    }

    fn pump_queue(
        self: &Arc<Self>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let max_concurrent = self.config.read().await.max_concurrent_tasks;
            loop {
                let active_count = {
                    let tasks = self.tasks.read().await;
                    tasks.values().filter(|t| t.status.is_active()).count()
                };
                if active_count >= max_concurrent {
                    break;
                }
                let next_id = {
                    let tasks = self.tasks.read().await;
                    tasks
                        .values()
                        .filter(|t| t.status == TaskStatus::Queued)
                        .min_by_key(|t| t.created_at)
                        .map(|t| t.id)
                };
                let Some(id) = next_id else {
                    break;
                };
                self.spawn_task_runner(id).await;
            }
        })
    }

    async fn spawn_task_runner(self: &Arc<Self>, id: TaskId) {
        let (task_snapshot, cfg) = {
            let mut tasks = self.tasks.write().await;
            let Some(task) = tasks.get_mut(&id) else {
                return;
            };
            if task.status != TaskStatus::Queued {
                return;
            }
            task.status = TaskStatus::Probing;
            task.error_message = None;
            let _ = self.store.upsert_task(task);
            let _ = self.events_tx.send(TaskEvent::TaskUpdated(task.clone()));
            (task.clone(), self.config.read().await.clone())
        };

        let protocol = match self.find_protocol(&task_snapshot.resolved_url) {
            Ok(p) => p,
            Err(e) => {
                self.mark_task_failed(id, e).await;
                return;
            }
        };

        let cancel = CancellationToken::new();
        let task_limiter = TokenBucket::new(task_snapshot.speed_limit_bps);
        let live_connections = Arc::new(AtomicU32::new(task_snapshot.max_connections));

        self.active.write().await.insert(
            id,
            ActiveHandle {
                cancel: cancel.clone(),
                limiter: Arc::clone(&task_limiter),
                live_connections: Arc::clone(&live_connections),
            },
        );

        let mut bt_config = cfg.bt.clone();
        if task_snapshot.protocol == ProtocolKind::BitTorrent
            && task_snapshot.completed_at.is_some()
        {
            // Explicit "start / resume seeding" on an already-completed torrent should seed
            // even when global auto-seeding after download is disabled.
            bt_config.seed_after_complete = true;
        }

        let (updates_tx, mut updates_rx) = mpsc::unbounded_channel();
        let ctx = DownloadContext {
            task: task_snapshot.clone(),
            cancel: cancel.clone(),
            limiter: HierarchicalLimiter::new(Arc::clone(&self.global_limiter), task_limiter),
            updates_tx,
            min_split_size_bytes: cfg.min_split_size_bytes,
            proxy_url: cfg.proxy_url.clone(),
            user_agent: cfg.user_agent.clone(),
            live_max_connections: live_connections,
            bt_config,
            ed2k_config: cfg.ed2k.clone(),
        };

        let mgr_updates = Arc::clone(self);
        let update_forwarder = tokio::spawn(async move {
            let mut last_db_checkpoint = Instant::now();
            while let Some(update) = updates_rx.recv().await {
                match update {
                    EngineUpdate::Probed(meta) => {
                        let mut tasks = mgr_updates.tasks.write().await;
                        if let Some(t) = tasks.get_mut(&id) {
                            t.resolved_url = meta.final_url;
                            // Adopt the exact deduplicated filename finalized by the protocol engine
                            if t.segments.is_empty() && !meta.filename.is_empty() {
                                t.filename = meta.filename;
                            }
                            t.total_size = meta.content_length;
                            t.supports_range = meta.supports_range;
                            t.etag = meta.etag;
                            t.last_modified = meta.last_modified;
                            t.content_type = meta.content_type;
                            t.category = FileCategory::from_filename_and_mime(
                                &t.filename,
                                t.content_type.as_deref(),
                            );
                            t.status = TaskStatus::Downloading;
                            let _ = mgr_updates.store.upsert_task(t);
                            let _ = mgr_updates.events_tx.send(TaskEvent::TaskUpdated(t.clone()));
                        }
                    }
                    EngineUpdate::Progress {
                        downloaded_bytes,
                        total_size,
                        speed_bps,
                        eta_secs,
                        segments,
                    } => {
                        let mut tasks = mgr_updates.tasks.write().await;
                        if let Some(t) = tasks.get_mut(&id) {
                            if t.status == TaskStatus::Probing {
                                t.status = TaskStatus::Downloading;
                            }
                            t.downloaded_bytes = downloaded_bytes;
                            if total_size.is_some() {
                                t.total_size = total_size;
                            }
                            t.speed_bps = speed_bps;
                            t.eta_secs = eta_secs;
                            t.segments = segments.clone();
                            t.push_speed_sample(speed_bps);

                            if last_db_checkpoint.elapsed() >= Duration::from_millis(600) {
                                let _ = mgr_updates.store.checkpoint_progress(
                                    id,
                                    downloaded_bytes,
                                    t.total_size,
                                    &segments,
                                );
                                last_db_checkpoint = Instant::now();
                            }
                        }
                        drop(tasks);
                        let _ = mgr_updates.events_tx.send(TaskEvent::TaskProgress {
                            task_id: id,
                            downloaded_bytes,
                            total_size,
                            speed_bps,
                            eta_secs,
                            segments,
                        });
                    }
                    EngineUpdate::BtTelemetry(telemetry) => {
                        let mut tasks = mgr_updates.tasks.write().await;
                        if let Some(t) = tasks.get_mut(&id) {
                            t.bt_telemetry = Some(telemetry.clone());
                        }
                        drop(tasks);
                        let _ = mgr_updates.events_tx.send(TaskEvent::BtTelemetry {
                            task_id: id,
                            telemetry,
                        });
                    }
                    EngineUpdate::Seeding { total_bytes } => {
                        let completed_event = {
                            let mut tasks = mgr_updates.tasks.write().await;
                            if let Some(t) = tasks.get_mut(&id) {
                                let first_completion = t.completed_at.is_none();
                                t.status = TaskStatus::Seeding;
                                t.downloaded_bytes = total_bytes;
                                if t.total_size.is_none() {
                                    t.total_size = Some(total_bytes);
                                }
                                t.speed_bps = 0;
                                t.eta_secs = Some(0);
                                if first_completion {
                                    t.completed_at = Some(Utc::now());
                                }
                                for seg in &mut t.segments {
                                    seg.state = SegmentState::Done;
                                    seg.speed_bps = 0;
                                }
                                let _ = mgr_updates.store.upsert_task(t);
                                let _ = mgr_updates.events_tx.send(TaskEvent::TaskUpdated(t.clone()));
                                if first_completion {
                                    Some((t.filename.clone(), t.target_path()))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        };
                        if let Some((filename, target_path)) = completed_event {
                            let _ = mgr_updates.events_tx.send(TaskEvent::TaskCompleted {
                                task_id: id,
                                filename,
                                target_path,
                            });
                        }
                        mgr_updates.pump_queue().await;
                    }
                    EngineUpdate::Verifying => {
                        let mut tasks = mgr_updates.tasks.write().await;
                        if let Some(t) = tasks.get_mut(&id) {
                            t.status = TaskStatus::Verifying;
                            t.speed_bps = 0;
                            let _ = mgr_updates.events_tx.send(TaskEvent::TaskUpdated(t.clone()));
                        }
                    }
                    EngineUpdate::Log(entry) => {
                        let mut tasks = mgr_updates.tasks.write().await;
                        if let Some(t) = tasks.get_mut(&id) {
                            t.logs.push(entry.clone());
                            if t.logs.len() > 80 {
                                let drain = t.logs.len() - 80;
                                t.logs.drain(0..drain);
                            }
                        }
                        drop(tasks);
                        let _ = mgr_updates.store.append_log(id, &entry);
                        let _ = mgr_updates.events_tx.send(TaskEvent::LogEntry {
                            task_id: id,
                            entry,
                        });
                    }
                }
            }
        });

        let mgr = Arc::clone(self);
        tokio::spawn(async move {
            let outcome = protocol.download(ctx).await;
            let _ = update_forwarder.await;
            mgr.active.write().await.remove(&id);

            match outcome {
                Ok(done) => {
                    let post_input = {
                        let mut tasks = mgr.tasks.write().await;
                        if let Some(t) = tasks.get_mut(&id) {
                            t.status = TaskStatus::Completed;
                            t.downloaded_bytes = done.total_bytes;
                            if t.total_size.is_none() {
                                t.total_size = Some(done.total_bytes);
                            }
                            t.speed_bps = 0;
                            t.eta_secs = Some(0);
                            t.completed_at = Some(Utc::now());
                            for seg in &mut t.segments {
                                seg.state = SegmentState::Done;
                                seg.speed_bps = 0;
                            }
                            t.push_log(
                                "INFO",
                                format!("下载完成，已保存至 {}", t.target_path().display()),
                            );
                            let _ = mgr.store.upsert_task(t);
                            let _ = mgr.events_tx.send(TaskEvent::TaskUpdated(t.clone()));
                            let _ = mgr.events_tx.send(TaskEvent::TaskCompleted {
                                task_id: id,
                                filename: t.filename.clone(),
                                target_path: t.target_path(),
                            });
                            Some(PostDownloadInput {
                                task_id: t.id.to_string(),
                                url: t.resolved_url.clone(),
                                file_path: t.target_path().to_string_lossy().to_string(),
                                filename: t.filename.clone(),
                                total_bytes: done.total_bytes,
                                verified_hash: done.verified_hash,
                            })
                        } else {
                            None
                        }
                    };

                    if let Some(input) = post_input {
                        let notes = {
                            let plugins = mgr.plugins.read().await;
                            plugins.run_post_download(&input).await
                        };
                        if !notes.is_empty() {
                            let mut tasks = mgr.tasks.write().await;
                            if let Some(t) = tasks.get_mut(&id) {
                                for note in notes {
                                    t.push_log("INFO", note);
                                }
                                let _ = mgr.events_tx.send(TaskEvent::TaskUpdated(t.clone()));
                            }
                        }
                    }
                }
                Err(SiphonError::Cancelled) => {
                    // Pause/removal already updated task state; persist latest segment checkpoint
                    let tasks = mgr.tasks.read().await;
                    if let Some(t) = tasks.get(&id) {
                        let _ = mgr.store.upsert_task(t);
                    }
                }
                Err(err) => {
                    mgr.mark_task_failed(id, err).await;
                }
            }

            mgr.pump_queue().await;
        });
    }

    async fn mark_task_failed(&self, id: TaskId, err: SiphonError) {
        let link_expired = matches!(err, SiphonError::LinkExpired(_));
        let msg = err.to_string();
        let mut tasks = self.tasks.write().await;
        if let Some(t) = tasks.get_mut(&id) {
            t.status = if link_expired {
                TaskStatus::LinkExpired
            } else {
                TaskStatus::Error
            };
            t.speed_bps = 0;
            t.eta_secs = None;
            t.error_message = Some(msg.clone());
            t.push_log("ERROR", msg.clone());
            let _ = self.store.upsert_task(t);
            let _ = self.events_tx.send(TaskEvent::TaskUpdated(t.clone()));
        }
        drop(tasks);
        let _ = self.events_tx.send(TaskEvent::TaskFailed {
            task_id: id,
            error: msg,
            link_expired,
        });
    }
}

pub fn infer_filename_from_raw_url(raw: &str, parsed: Option<&Url>) -> String {
    let trimmed = raw.trim();
    if trimmed.to_ascii_lowercase().starts_with("ed2k://|file|") {
        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.len() >= 5 && !parts[2].trim().is_empty() {
            let decoded = percent_encoding::percent_decode_str(parts[2]).decode_utf8_lossy();
            let clean = decoded.trim();
            if !clean.is_empty() {
                return clean.to_string();
            }
        }
        return format!("ed2k_{}.bin", Utc::now().format("%Y%m%d_%H%M%S"));
    }
    if let Some(u) = parsed {
        return infer_filename_from_url(u);
    }
    if let Ok(u) = Url::parse(trimmed) {
        return infer_filename_from_url(&u);
    }
    format!("download_{}.bin", Utc::now().format("%Y%m%d_%H%M%S"))
}

pub fn infer_filename_from_url(url: &Url) -> String {
    if url.scheme().eq_ignore_ascii_case("magnet") {
        let mut dn = None;
        let mut hash = None;
        for (k, v) in url.query_pairs() {
            if k == "dn" && !v.trim().is_empty() {
                dn = Some(v.trim().to_string());
            } else if k == "xt"
                && let Some(h) = v.rsplit(':').next()
                && !h.is_empty()
            {
                hash = Some(h.to_string());
            }
        }
        if let Some(name) = dn {
            return name;
        }
        if let Some(h) = hash {
            return format!("torrent_{}", &h[..h.len().min(12)]);
        }
        return format!("magnet_{}", Utc::now().format("%Y%m%d_%H%M%S"));
    }

    if let Some(mut segs) = url.path_segments()
        && let Some(last) = segs.next_back()
        && !last.is_empty()
    {
        if last.to_ascii_lowercase().ends_with(".m3u8") {
            let stem = last.trim_end_matches(|c| c == '8' || c == 'u' || c == '3' || c == 'm' || c == 'M' || c == 'U' || c == '.');
            let base = if stem.is_empty() || stem.eq_ignore_ascii_case("index") || stem.eq_ignore_ascii_case("playlist") {
                format!("stream_{}", Utc::now().format("%Y%m%d_%H%M%S"))
            } else {
                stem.to_string()
            };
            return format!("{base}.ts");
        }
        if last.to_ascii_lowercase().ends_with(".torrent") {
            return last
                .trim_end_matches(".torrent")
                .trim_end_matches(".TORRENT")
                .to_string();
        }
        return last.to_string();
    }
    let host = url.host_str().unwrap_or("download");
    format!("{host}_{}.bin", Utc::now().format("%Y%m%d_%H%M%S"))
}

pub fn sanitize_filename(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    let mut out: String = trimmed
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while out.ends_with(['.', ' ']) {
        out.pop();
    }
    if out.is_empty() {
        format!("siphon_{}.bin", Utc::now().format("%Y%m%d_%H%M%S"))
    } else {
        out
    }
}

pub fn deduplicate_filename(dir: &Path, filename: &str) -> String {
    let candidate = dir.join(filename);
    let candidate_part = dir.join(format!("{filename}.siphon.part"));
    if !candidate.exists() && !candidate_part.exists() {
        return filename.to_string();
    }

    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| filename.to_string());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for i in 1..10_000 {
        let next = format!("{stem} ({i}){ext}");
        if !dir.join(&next).exists() && !dir.join(format!("{next}.siphon.part")).exists() {
            return next;
        }
    }
    format!("{stem}_{}{ext}", Uuid::new_v4().simple())
}
