use iced::{Point, window};
use siphon_core::{
    AppConfig, AutomationRule, BtEncryptionMode, DownloadTask, ExternalPlugin, FileCategory,
    Language, ListDensity, ProbePreview, ProtocolKind, QueueCompleteAction, TaskEvent, TaskId,
    TaskSource, TaskStatus, ThemeMode,
};
use siphon_engine_bt::{BtPortCheckReport, BtSessionStatusSnapshot};
use siphon_engine_http::{FetchedRssFeed, SniffedWebItem, UpdateInfo};
use siphon_platform_win::{
    DetectedBrowser, DropEvent, FileIconBitmap, FileIconKey, IpcCommand, TrayAction, WinHookEvent,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavSection {
    All,
    Downloading,
    Completed,
    Paused,
    Torrents,
    Category(FileCategory),
    BitTorrent,
    Ed2k,
    Rss,
    Scheduler,
    Rules,
    Plugins,
    Settings,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TorrentFilter {
    #[default]
    All,
    Seeding,
    Downloading,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskQuickFilter {
    #[default]
    All,
    Downloading,
    Seeding,
    Paused,
    Completed,
}

impl TaskQuickFilter {
    pub fn matches(self, status: TaskStatus) -> bool {
        match self {
            Self::All => true,
            Self::Downloading => status.is_active(),
            Self::Seeding => status == TaskStatus::Seeding,
            Self::Paused => matches!(
                status,
                TaskStatus::Paused
                    | TaskStatus::Queued
                    | TaskStatus::Error
                    | TaskStatus::LinkExpired
            ),
            Self::Completed => status == TaskStatus::Completed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskProtocolFilter {
    #[default]
    All,
    Http,
    BitTorrent,
    Hls,
    Ed2k,
}

impl TaskProtocolFilter {
    pub fn matches(self, protocol: &ProtocolKind) -> bool {
        match self {
            Self::All => true,
            Self::Http => *protocol == ProtocolKind::Http,
            Self::BitTorrent => *protocol == ProtocolKind::BitTorrent,
            Self::Hls => *protocol == ProtocolKind::Hls,
            Self::Ed2k => *protocol == ProtocolKind::Ed2k,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskSortBy {
    /// Default: Progress (quantized to whole %) + Request/Created time.
    #[default]
    ProgressAndTime,
    Progress,
    CreatedAt,
    Size,
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskListFilterState {
    pub quick_filter: TaskQuickFilter,
    pub protocol_filter: TaskProtocolFilter,
    pub sort_by: TaskSortBy,
    /// `true` = Descending (higher progress / newer request / larger size first).
    pub sort_desc: bool,
    /// Keep actively downloading tasks pinned at the top regardless of sort column.
    pub pin_downloading: bool,
}

impl Default for TaskListFilterState {
    fn default() -> Self {
        Self {
            quick_filter: TaskQuickFilter::All,
            protocol_filter: TaskProtocolFilter::All,
            sort_by: TaskSortBy::ProgressAndTime,
            sort_desc: true,
            pin_downloading: true,
        }
    }
}

fn task_pin_rank(status: TaskStatus) -> u8 {
    match status {
        TaskStatus::Downloading | TaskStatus::Probing | TaskStatus::Verifying => 0,
        TaskStatus::Queued => 1,
        TaskStatus::Paused | TaskStatus::LinkExpired | TaskStatus::Error => 2,
        TaskStatus::Seeding => 3,
        TaskStatus::Completed => 4,
    }
}

fn quantized_progress_pct(task: &DownloadTask) -> u32 {
    if task.status.is_finished() {
        100
    } else {
        (task.progress_ratio().clamp(0.0, 1.0) * 100.0).floor() as u32
    }
}

pub fn sort_tasks(tasks: &mut [&DownloadTask], filter: TaskListFilterState) {
    tasks.sort_by(|a, b| {
        if filter.pin_downloading {
            let tier_cmp = match filter.sort_by {
                // Default mode keeps full intuitive tier ordering: Downloading -> Queued -> Paused -> Seeding -> Completed
                TaskSortBy::ProgressAndTime => {
                    task_pin_rank(a.status).cmp(&task_pin_rank(b.status))
                }
                // Explicit column sorts pin active downloads to top while sorting everything else purely by chosen column
                _ => {
                    let a_active = if a.status.is_active() { 0u8 } else { 1u8 };
                    let b_active = if b.status.is_active() { 0u8 } else { 1u8 };
                    a_active.cmp(&b_active)
                }
            };
            if tier_cmp != std::cmp::Ordering::Equal {
                return tier_cmp;
            }
        }

        let primary = match filter.sort_by {
            TaskSortBy::ProgressAndTime => quantized_progress_pct(a)
                .cmp(&quantized_progress_pct(b))
                .then_with(|| a.created_at.cmp(&b.created_at)),
            TaskSortBy::Progress => quantized_progress_pct(a)
                .cmp(&quantized_progress_pct(b))
                .then_with(|| a.created_at.cmp(&b.created_at)),
            TaskSortBy::CreatedAt => a.created_at.cmp(&b.created_at),
            TaskSortBy::Size => {
                let sa = a.total_size.unwrap_or(a.downloaded_bytes);
                let sb = b.total_size.unwrap_or(b.downloaded_bytes);
                sa.cmp(&sb).then_with(|| a.created_at.cmp(&b.created_at))
            }
            TaskSortBy::Name => a
                .filename
                .to_ascii_lowercase()
                .cmp(&b.filename.to_ascii_lowercase())
                .then_with(|| a.created_at.cmp(&b.created_at)),
        };

        let ordered = if filter.sort_desc {
            match filter.sort_by {
                // For Name, "default/desc" means A->Z (natural), toggled means Z->A
                TaskSortBy::Name => primary,
                _ => primary.reverse(),
            }
        } else {
            match filter.sort_by {
                TaskSortBy::Name => primary.reverse(),
                _ => primary,
            }
        };

        ordered.then_with(|| a.id.cmp(&b.id))
    });
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TorrentSummary {
    pub total: usize,
    pub seeding: usize,
    pub downloading: usize,
    pub active_or_queued: usize,
    pub paused_or_error: usize,
    pub stopped: usize,
    pub completed_idle: usize,
    pub total_upload_bps: u64,
    pub total_uploaded_bytes: u64,
    pub bt_enabled: bool,
    pub seed_after_complete: bool,
}

#[derive(Debug, Clone)]
pub enum ServerTestStatus {
    Testing,
    Online(u64),
    Offline(String),
}

#[derive(Debug, Clone, Default)]
pub enum UpdateCheckStatus {
    #[default]
    Idle,
    Checking,
    UpToDate(String),
    UpdateAvailable(UpdateInfo),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTarget {
    QuickBar,
    ModalUrl,
    ModalFilename,
    ModalSaveDir,
    ModalSpeedLimit,
    ModalSha256,
    ModalBatchUrls,
    ModalBatchFilter,
    ReplaceUrl,
    RuleName,
    RuleExtensions,
    RuleDomain,
    RulePriority,
    RuleSaveDir,
    RuleConnections,
    SettingsSaveDir,
    SettingsGlobalSpeed,
    SettingsProxy,
    SettingsUserAgent,
    SettingsAccentHex,
    SettingsPopupAccentHex,
    SchedStartTime,
    SchedStopTime,
    SchedSpeedStart,
    SchedSpeedEnd,
    SchedSpeedKbps,
    SchedScript,
    BtListenPort,
    BtMaxPeers,
    BtConnectTimeout,
    BtRwTimeout,
    BtNewTracker,
    BtNewSubscribeUrl,
    Ed2kNickname,
    Ed2kTcpPort,
    Ed2kUdpPort,
    Ed2kMaxSources,
    Ed2kUploadLimit,
    Ed2kServerMetUrl,
    Ed2kQuickServerLink,
    Ed2kNewServerName,
    Ed2kNewServerHost,
    Ed2kNewServerPort,
    CaptureFilename,
    CaptureSaveDir,
    SniffPageUrl,
    SniffSearch,
    SniffSaveDir,
    P2pSourceInput,
    P2pEd2kInput,
    P2pSearch,
    P2pSaveDir,
    RssUrl,
    RssName,
    RssInclude,
    RssExclude,
    RssSaveDir,
    RssSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAction {
    /// "稍后下载" — add task in Paused state (`auto_start: false`).
    Later,
    /// "后台下载" — start downloading silently in background without opening a progress detail popup.
    Background,
    /// "开始下载" — start downloading immediately AND open the standalone download detail popup window.
    StartWithDialog,
}

#[derive(Debug, Clone)]
pub struct DownloadPopupState {
    pub window_id: window::Id,
    pub window_hwnd: Option<u64>,
    pub task_id: TaskId,
    pub auto_close_on_complete: bool,
}

#[derive(Debug, Clone)]
pub struct CapturePopupState {
    pub window_id: Option<window::Id>,
    pub window_hwnd: Option<u64>,
    pub source: TaskSource,
    pub url: String,
    pub filename: String,
    pub save_dir: String,
    pub headers: BTreeMap<String, String>,
    pub connections: u32,
    pub speed_limit_bps: u64,
    pub known_size: Option<u64>,
    pub supports_range: Option<bool>,
    pub matched_rule_name: Option<String>,
    pub probing: bool,
    pub probe_error: Option<String>,
    pub dont_ask_again: bool,
    pub filename_edited_by_user: bool,
    pub save_dir_edited_by_user: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SniffFilterKind {
    #[default]
    All,
    Hls,
    Video,
    Audio,
    File,
    P2p,
    Image,
}

impl SniffFilterKind {
    pub fn matches(self, kind: &str) -> bool {
        let k = kind.to_ascii_lowercase();
        match self {
            Self::All => true,
            Self::Hls => k == "hls",
            Self::Video => k == "video",
            Self::Audio => k == "audio",
            Self::File => k == "file" || k.is_empty(),
            Self::P2p => matches!(k.as_str(), "magnet" | "torrent" | "ed2k"),
            Self::Image => k == "image",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SniffEntry {
    pub id: usize,
    pub url: String,
    pub filename: String,
    pub kind: String,
    pub label: String,
    pub mime: Option<String>,
    pub file_size: Option<u64>,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct SniffPopupState {
    pub window_id: Option<window::Id>,
    pub window_hwnd: Option<u64>,
    pub page_title: String,
    pub page_url: String,
    pub page_url_input: String,
    pub sniffing_page: bool,
    pub probing_sizes: bool,
    pub sniff_error: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub items: Vec<SniffEntry>,
    pub next_entry_id: usize,
    pub filter: SniffFilterKind,
    pub search: String,
    pub save_dir: String,
    pub connections: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum P2pSelectTab {
    #[default]
    Torrent,
    Ed2k,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum P2pFileFilter {
    #[default]
    All,
    Video,
    Audio,
    ArchiveIso,
    DocImage,
    Other,
}

impl P2pFileFilter {
    pub fn matches_filename(self, path: &str) -> bool {
        if self == Self::All {
            return true;
        }
        let cat = FileCategory::from_filename_and_mime(path, None);
        match self {
            Self::All => true,
            Self::Video => cat == FileCategory::Video,
            Self::Audio => cat == FileCategory::Audio,
            Self::ArchiveIso => matches!(
                cat,
                FileCategory::Archive | FileCategory::DiskImage | FileCategory::Software
            ),
            Self::DocImage => matches!(cat, FileCategory::Document | FileCategory::Image),
            Self::Other => cat == FileCategory::Other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TorrentSelectItem {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct Ed2kSelectItem {
    pub id: usize,
    pub raw_url: String,
    pub filename: String,
    pub file_size: u64,
    pub md4_hash: String,
    pub aich_hash: Option<String>,
    pub web_sources: usize,
    pub peer_sources: usize,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct P2pSelectPopupState {
    pub window_id: Option<window::Id>,
    pub window_hwnd: Option<u64>,
    pub tab: P2pSelectTab,
    pub source_input: String,
    pub normalized_bt_url: String,
    pub torrent_name: String,
    pub info_hash: String,
    pub inspecting_bt: bool,
    pub bt_error: Option<String>,
    pub bt_files: Vec<TorrentSelectItem>,
    pub ed2k_input: String,
    pub ed2k_error: Option<String>,
    pub ed2k_items: Vec<Ed2kSelectItem>,
    pub next_ed2k_id: usize,
    pub filter: P2pFileFilter,
    pub search: String,
    pub save_dir: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ContextMenuState {
    pub target: InputTarget,
    pub position: Point,
}

#[derive(Debug, Clone, Default)]
pub struct NewTaskModalState {
    pub open: bool,
    pub batch_mode: bool,
    pub url: String,
    pub batch_text: String,
    pub batch_filter: String,
    pub filename: String,
    pub save_dir: String,
    pub connections: u32,
    pub speed_limit_kbps: String,
    pub sha256: String,
    pub probing: bool,
    pub probe_error: Option<String>,
    pub preview: Option<ProbePreview>,
}

#[derive(Debug, Clone)]
pub struct DeleteConfirmState {
    pub task_id: TaskId,
    pub filename: String,
    pub file_path: PathBuf,
    pub delete_files: bool,
    pub dont_ask_again: bool,
}

#[derive(Debug, Clone)]
pub struct ExtInstallModalState {
    pub browsers: Vec<DetectedBrowser>,
    pub selected_browser_id: String,
    pub ext_dir: PathBuf,
    pub deployed: bool,
    pub path_copied: bool,
    pub browser_launched: bool,
    pub error_msg: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RuleDraft {
    pub editing_id: Option<Uuid>,
    pub name: String,
    pub extensions: String,
    pub domain_glob: String,
    pub priority: String,
    pub save_dir_template: String,
    pub connections: String,
}

#[derive(Debug, Clone, Default)]
pub struct RssDraft {
    pub editing_id: Option<Uuid>,
    pub url: String,
    pub name: String,
    pub include_filter: String,
    pub exclude_filter: String,
    pub save_dir: String,
    pub auto_download: bool,
    pub refresh_interval_mins: u32,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Window & platform lifecycle
    WindowOpened(window::Id),
    WindowRawId(u64),
    WindowCloseRequested(window::Id),
    WindowClosed(window::Id),
    WindowResized(window::Id, iced::Size),
    WindowFocused(window::Id),
    DragWindow,
    MinimizeWindow,
    ToggleMaximizeWindow,
    CloseWindow,
    FileDropped(PathBuf),
    CursorMoved(window::Id, Point),
    ShortcutNewTask,
    ShortcutFocusQuickBar,
    ShortcutEscape,

    // Standalone Browser / Clipboard Capture Popup Window
    CaptureWindowOpened(window::Id),
    CaptureWindowRawId(u64),
    CaptureDragWindow,
    CaptureFilenameChanged(String),
    CaptureSaveDirChanged(String),
    CaptureBrowseDir,
    CaptureDirPicked(Option<PathBuf>),
    CaptureToggleDontAsk(bool),
    CaptureProbeFinished(String, std::result::Result<ProbePreview, String>),
    CaptureSubmit(CaptureAction),
    CaptureStartedWithDialog(std::result::Result<DownloadTask, String>),
    CaptureCancel,

    // Standalone Browser Resource Sniff & Selective Download Window
    SniffWindowOpened(window::Id),
    SniffWindowRawId(u64),
    SniffDragWindow,
    SniffClose,
    SniffPageUrlChanged(String),
    SniffFetchPageUrl,
    SniffFetchPageFinished(std::result::Result<(String, String, Vec<SniffedWebItem>), String>),
    SniffSetFilter(SniffFilterKind),
    SniffSearchChanged(String),
    SniffToggleItem(usize),
    SniffSelectAllVisible(bool),
    SniffInvertSelection,
    SniffProbeSizes,
    SniffProbeSizesFinished(Vec<(String, Option<u64>)>),
    SniffRemoveItem(usize),
    SniffClearAll,
    SniffSaveDirChanged(String),
    SniffBrowseDir,
    SniffDirPicked(Option<PathBuf>),
    SniffConnectionsChanged(f32),
    SniffDownloadSingle(usize),
    SniffDownloadSelected(bool),

    // Standalone BitTorrent (.torrent / Magnet) & eD2k Selective Download Window
    P2pWindowOpened(window::Id),
    P2pWindowRawId(u64),
    P2pDragWindow,
    P2pClose,
    P2pSetTab(P2pSelectTab),
    P2pSourceInputChanged(String),
    P2pPickTorrentFile,
    P2pTorrentFilePicked(Option<PathBuf>),
    P2pInspectTorrent,
    P2pInspectTorrentFinished(std::result::Result<siphon_engine_bt::TorrentInspectInfo, String>),
    P2pToggleBtFile(usize),
    P2pBtSelectLargeOnly,
    P2pSubmitBtDirect(bool),
    P2pSubmitBtSelected(bool),
    P2pEd2kInputChanged(String),
    P2pParseEd2kInput,
    P2pImportEd2kClipboard,
    P2pToggleEd2kItem(usize),
    P2pRemoveEd2kItem(usize),
    P2pClearEd2kItems,
    P2pDownloadSingleEd2k(usize),
    P2pSubmitEd2kSelected(bool),
    P2pSetFilter(P2pFileFilter),
    P2pSearchChanged(String),
    P2pSelectAllVisible(bool),
    P2pInvertSelection,
    P2pSaveDirChanged(String),
    P2pBrowseDir,
    P2pDirPicked(Option<PathBuf>),

    // Standalone Per-Task Download Progress Detail Popup Window
    OpenTaskDetailPopup(TaskId),
    DownloadPopupRawId(window::Id, u64),
    DownloadPopupDrag(window::Id),
    DownloadPopupToggleAutoClose(window::Id, bool),
    DownloadPopupClose(window::Id),
    DownloadPopupCancelTask(window::Id, TaskId),
    DownloadPopupOpenFile(window::Id, TaskId),

    // Paste & right-click context menu
    FocusInputTarget(InputTarget),
    PasteIntoActiveInput,
    OpenInputContextMenu(InputTarget),
    CloseContextMenu,
    ContextPaste(InputTarget),
    ContextPasteAndGo(InputTarget),
    ContextCopy(InputTarget),
    ContextClear(InputTarget),

    // Background event streams
    EngineEvent(TaskEvent),
    TrayEvent(TrayAction),
    IpcEvent(IpcCommand),
    DropEvent(DropEvent),
    WinHook(WinHookEvent),
    FileIconsLoaded(Vec<(FileIconKey, Option<FileIconBitmap>)>),
    ClipboardUrlDetected(String),
    DismissClipboardBanner,
    ClipboardQuickDownload,
    ClipboardOpenModal,

    // Navigation & quick bar
    SelectNav(NavSection),
    SetTorrentFilter(TorrentFilter),
    SetTaskQuickFilter(TaskQuickFilter),
    SetTaskProtocolFilter(TaskProtocolFilter),
    SetTaskSortBy(TaskSortBy),
    ToggleTaskSortOrder,
    TogglePinDownloading(bool),
    ResetTaskFilters,
    ResumeAllTorrents,
    PauseAllTorrents,
    StartAllSeeding,
    StopAllSeeding,
    BatchSeedingFinished(std::result::Result<String, String>),
    QuickBarChanged(String),
    QuickBarSubmit,
    CycleTheme,

    // Task operations
    SelectTask(TaskId),
    CloseInspector,
    PauseTask(TaskId),
    ResumeTask(TaskId),
    PauseAll,
    ResumeAll,
    RequestDeleteTask(TaskId),
    DeleteConfirmToggleFiles(bool),
    DeleteConfirmToggleDontAsk(bool),
    DeleteConfirmAccept,
    DeleteConfirmCancel,
    RemoveTask(TaskId, bool),
    OpenTaskFile(TaskId),
    RevealTaskInExplorer(TaskId),
    AdjustTaskConnections(TaskId, i32),
    SetTaskSpeedLimit(TaskId, u64),
    ReplaceUrlInputChanged(String),
    ToggleReplaceUrlBox,
    SubmitReplaceUrl(TaskId),
    TaskOpFinished(std::result::Result<(), String>),

    // New Task modal
    OpenNewTaskModal(Option<String>),
    CloseNewTaskModal,
    ModalUrlChanged(String),
    ModalFilenameChanged(String),
    ModalSaveDirChanged(String),
    ModalBrowseDir,
    ModalDirPicked(Option<PathBuf>),
    ModalConnectionsChanged(f32),
    ModalSpeedLimitChanged(String),
    ModalSha256Changed(String),
    ModalProbeUrl,
    ModalProbeFinished(std::result::Result<ProbePreview, String>),
    ModalSubmit(bool),
    ModalToggleBatchMode(bool),
    ModalBatchTextChanged(String),
    ModalBatchFilterChanged(String),
    ModalBatchPasteClipboard,
    ModalPickTorrentFile,
    ModalTorrentFilePicked(Option<PathBuf>),
    ModalBatchSubmit(bool),

    // Scheduler & Queue Complete Actions
    SchedToggleStart(bool),
    SchedStartTimeChanged(String),
    SchedToggleStop(bool),
    SchedStopTimeChanged(String),
    SchedToggleSpeed(bool),
    SchedSpeedStartChanged(String),
    SchedSpeedEndChanged(String),
    SchedSpeedKbpsChanged(String),
    SchedSetCompleteAction(QueueCompleteAction),
    SchedScriptChanged(String),
    SchedToggleOneShot(bool),
    SchedSave,
    SchedulerTick,

    // BitTorrent & Custom Tracker dedicated workspace
    BtToggleEnabled(bool),
    BtToggleSeedAfterComplete(bool),
    BtSetMaxSeedRatio(f64),
    BtToggleDht(bool),
    BtToggleLsd(bool),
    BtToggleUpnp(bool),
    BtToggleIpv6DualStack(bool),
    BtSetEncryptionMode(BtEncryptionMode),
    BtToggleDisableTrackers(bool),
    BtToggleAutoAppend(bool),
    BtToggleAutoSyncOnStart(bool),
    BtListenPortChanged(String),
    BtMaxPeersChanged(String),
    BtConnectTimeoutChanged(String),
    BtRwTimeoutChanged(String),
    BtNewTrackerChanged(String),
    BtAddTracker,
    BtPasteTrackersClipboard,
    BtRemoveCustomTracker(usize),
    BtClearCustomTrackers,
    BtNewSubscribeUrlChanged(String),
    BtAddSubscribeUrl,
    BtRemoveSubscribeUrl(usize),
    BtResetDefaultSubscribeUrls,
    BtSyncTrackers,
    BtSyncTrackersFinished(std::result::Result<(Vec<String>, usize, usize), String>),
    BtClearSyncedTrackers,
    BtSave,
    BtSessionStatusUpdated(BtSessionStatusSnapshot),
    BtCheckPort,
    BtPortCheckFinished(BtPortCheckReport),
    CopyTextToClipboard(String),

    // eD2k dedicated workspace
    Ed2kToggleEnabled(bool),
    Ed2kNicknameChanged(String),
    Ed2kTcpPortChanged(String),
    Ed2kUdpPortChanged(String),
    Ed2kToggleKad(bool),
    Ed2kToggleObfuscation(bool),
    Ed2kMaxSourcesChanged(String),
    Ed2kUploadLimitChanged(String),
    Ed2kToggleVerifyMd4(bool),
    Ed2kServerMetUrlChanged(String),
    Ed2kSyncServerMet,
    Ed2kSyncServerMetFinished(std::result::Result<Vec<siphon_core::Ed2kServerEntry>, String>),
    Ed2kQuickServerLinkChanged(String),
    Ed2kImportQuickServerLink,
    Ed2kNewServerNameChanged(String),
    Ed2kNewServerHostChanged(String),
    Ed2kNewServerPortChanged(String),
    Ed2kAddServer,
    Ed2kToggleServerEnabled(usize),
    Ed2kRemoveServer(usize),
    Ed2kResetDefaultServers,
    Ed2kTestServer(usize),
    Ed2kTestServerFinished(String, std::result::Result<u64, String>),
    Ed2kSave,

    // Rules workspace
    RuleDraftName(String),
    RuleDraftExtensions(String),
    RuleDraftDomain(String),
    RuleDraftPriority(String),
    RuleDraftSaveDir(String),
    RuleDraftBrowseDir,
    RuleDraftDirPicked(Option<PathBuf>),
    RuleDraftConnections(String),
    RuleEditStart(Uuid),
    RuleEditCancel,
    RuleAddSubmit,
    RuleToggleEnabled(Uuid),
    RuleDelete(Uuid),
    RulesLoaded(Vec<AutomationRule>),

    // RSS subscriptions & auto-download workspace
    RssToggleGlobalEnabled(bool),
    RssSetDefaultInterval(u32),
    RssDraftUrl(String),
    RssDraftName(String),
    RssDraftInclude(String),
    RssDraftExclude(String),
    RssDraftSaveDir(String),
    RssDraftBrowseDir,
    RssDraftDirPicked(Option<PathBuf>),
    RssDraftToggleAutoDownload(bool),
    RssDraftSetInterval(u32),
    RssDraftSubmit,
    RssEditStart(Uuid),
    RssEditCancel,
    RssSelectFeed(Option<Uuid>),
    RssToggleFeedEnabled(Uuid, bool),
    RssToggleFeedAutoDownload(Uuid, bool),
    RssDeleteFeed(Uuid),
    RssRefreshFeed(Uuid),
    RssRefreshAll,
    RssRefreshFinished(Uuid, std::result::Result<FetchedRssFeed, String>),
    RssSearchChanged(String),
    RssToggleMatchOnly(bool),
    RssDownloadItem(Uuid, String),
    RssDownloadAllMatching(Option<Uuid>),
    RssMarkAllRead(Option<Uuid>),

    // Plugins workspace
    ReloadPlugins,
    PluginsLoaded(Vec<ExternalPlugin>),
    TogglePlugin(String, bool),
    OpenPluginsFolder,
    InstallExternalPlugin,
    ExternalPluginDirPicked(Option<PathBuf>),

    // Settings workspace
    SetThemeMode(ThemeMode),
    SetAccentPreset(String),
    SetAccentHexInput(String),
    ApplyAccentHex,
    SetDownloadPopupAccentPreset(String),
    SetDownloadPopupAccentHexInput(String),
    ApplyDownloadPopupAccentHex,
    ToggleMica(bool),
    SetDensity(ListDensity),
    SetLanguage(Language),
    SetDefaultSaveDirInput(String),
    BrowseDefaultSaveDir,
    DefaultSaveDirPicked(Option<PathBuf>),
    SetDefaultConnections(f32),
    SetMaxConcurrentTasks(f32),
    SetMinSplitMiB(u64),
    SetGlobalSpeedLimitInput(String),
    ApplyGlobalSpeedLimit,
    SetProxyUrlInput(String),
    SetUserAgentInput(String),
    ToggleClipboardMonitor(bool),
    ToggleMinimizeToTray(bool),
    ToggleAutostart(bool),
    ToggleConfirmOnDelete(bool),
    SetDeleteFilesByDefault(bool),
    ToggleBrowserIntegration(bool),
    ToggleBrowserCapturePopup(bool),
    ToggleClipboardCapturePopup(bool),
    InstallBrowserExtension,
    CloseExtInstallModal,
    ExtInstallSelectBrowser(String),
    ExtInstallCopyPath,
    ExtInstallRevealFolder,
    ExtInstallLaunchSelectedBrowser,
    OpenBrowserExtensionFolder,
    ExportBrowserExtensionZip,
    BrowserExtensionZipSaved(Option<PathBuf>),
    OpenDataDir,
    ChangeDataDir,
    DataDirPicked(Option<PathBuf>),
    CheckForUpdates,
    UpdateCheckFinished(std::result::Result<UpdateInfo, String>),
    OpenExternalUrl(String),
    DownloadUpdateInstaller(String),
    StartAutoUpdate(String),
    AutoUpdateTaskStarted(std::result::Result<DownloadTask, String>),
    SaveSettings,
    SettingsSaved(std::result::Result<AppConfig, String>),

    // Toast notification
    ClearToast,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use std::collections::BTreeMap;

    fn sample_task(
        name: &str,
        status: TaskStatus,
        downloaded: u64,
        total: u64,
        age_secs: i64,
    ) -> DownloadTask {
        let now = Utc::now();
        DownloadTask {
            id: Uuid::new_v4(),
            original_url: format!("https://example.com/{name}"),
            resolved_url: format!("https://example.com/{name}"),
            filename: name.to_string(),
            save_dir: PathBuf::from("."),
            protocol: ProtocolKind::Http,
            category: FileCategory::Other,
            status,
            content_type: None,
            matched_rule: None,
            total_size: Some(total),
            downloaded_bytes: downloaded,
            speed_bps: 0,
            eta_secs: None,
            supports_range: true,
            etag: None,
            last_modified: None,
            max_connections: 8,
            speed_limit_bps: 0,
            headers: BTreeMap::new(),
            segments: Vec::new(),
            checksum: None,
            selected_files: None,
            source: TaskSource::Manual,
            error_message: None,
            created_at: now - Duration::seconds(age_secs),
            completed_at: status.is_finished().then_some(now),
            speed_history: Vec::new(),
            logs: Vec::new(),
            bt_telemetry: None,
        }
    }

    #[test]
    fn default_sort_pins_downloading_and_orders_by_progress_then_request_time() {
        let done_new = sample_task("done_new.zip", TaskStatus::Completed, 100, 100, 10);
        let done_old = sample_task("done_old.zip", TaskStatus::Completed, 100, 100, 300);
        let dl_low = sample_task("dl_low.iso", TaskStatus::Downloading, 25, 100, 5);
        let dl_high_old = sample_task("dl_high_old.iso", TaskStatus::Downloading, 80, 100, 60);
        let dl_high_new = sample_task("dl_high_new.iso", TaskStatus::Downloading, 80, 100, 20);
        let paused_mid = sample_task("paused.bin", TaskStatus::Paused, 50, 100, 15);
        let seeding = sample_task("seed.mkv", TaskStatus::Seeding, 100, 100, 120);

        let all = [
            &done_old,
            &seeding,
            &dl_low,
            &done_new,
            &paused_mid,
            &dl_high_old,
            &dl_high_new,
        ];
        let mut sorted = all.to_vec();
        sort_tasks(&mut sorted, TaskListFilterState::default());

        let names: Vec<&str> = sorted.iter().map(|t| t.filename.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "dl_high_new.iso",
                "dl_high_old.iso",
                "dl_low.iso",
                "paused.bin",
                "seed.mkv",
                "done_new.zip",
                "done_old.zip",
            ]
        );
    }

    #[test]
    fn unpinned_created_at_sort_respects_ascending_and_descending() {
        let t_old = sample_task("old.bin", TaskStatus::Downloading, 10, 100, 500);
        let t_new = sample_task("new.bin", TaskStatus::Completed, 100, 100, 10);
        let mut sorted = vec![&t_old, &t_new];

        let filter = TaskListFilterState {
            sort_by: TaskSortBy::CreatedAt,
            sort_desc: true,
            pin_downloading: false,
            ..Default::default()
        };
        sort_tasks(&mut sorted, filter);
        assert_eq!(sorted[0].filename, "new.bin");

        let asc_filter = TaskListFilterState {
            sort_desc: false,
            ..filter
        };
        sort_tasks(&mut sorted, asc_filter);
        assert_eq!(sorted[0].filename, "old.bin");
    }
}
