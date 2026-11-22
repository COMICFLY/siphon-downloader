use iced::{Point, window};
use siphon_core::{
    AppConfig, AutomationRule, DownloadTask, ExternalPlugin, FileCategory, Language, ListDensity,
    ProbePreview, TaskEvent, TaskId, TaskSource, ThemeMode,
};
use siphon_engine_http::UpdateInfo;
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
    Category(FileCategory),
    Rules,
    Plugins,
    Settings,
    About,
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
    CaptureFilename,
    CaptureSaveDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAction {
    /// "稍后下载" — add task in Paused state (`auto_start: false`).
    Later,
    /// "后台下载" — start downloading silently in background without opening a progress detail popup.
    Background,
    /// "开始下载" — start downloading immediately AND open the standalone IDM-style green download detail popup window.
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

#[derive(Debug, Clone, Copy)]
pub struct ContextMenuState {
    pub target: InputTarget,
    pub position: Point,
}

#[derive(Debug, Clone, Default)]
pub struct NewTaskModalState {
    pub open: bool,
    pub url: String,
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

    // Standalone IDM-style Browser / Clipboard Capture Popup Window
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

    // Standalone IDM-style Per-Task Download Progress Detail Popup Window (Green Theme)
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
    SaveSettings,
    SettingsSaved(std::result::Result<AppConfig, String>),

    // Toast notification
    ClearToast,
}
