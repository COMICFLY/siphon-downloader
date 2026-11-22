use thiserror::Error;

#[derive(Debug, Error)]
pub enum SiphonError {
    #[error("无效的 URL: {0}")]
    InvalidUrl(String),

    #[error("不支持的下载协议: {0}")]
    UnsupportedProtocol(String),

    #[error("任务不存在: {0}")]
    TaskNotFound(String),

    #[error("网络请求错误: {0}")]
    Network(String),

    #[error("远端文件已变更（ETag / Last-Modified 不匹配），请重新下载")]
    RemoteFileChanged,

    #[error("下载链接已过期或拒绝访问 (HTTP {0})，可替换新链接续传")]
    LinkExpired(u16),

    #[error("磁盘 I/O 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("配置序列化/解析错误: {0}")]
    Config(String),

    #[error("JSON 解析错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("校验和不匹配: 期望 {expected}，实际 {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("插件执行错误 ({plugin}): {message}")]
    Plugin { plugin: String, message: String },

    #[error("任务已取消或暂停")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, SiphonError>;
