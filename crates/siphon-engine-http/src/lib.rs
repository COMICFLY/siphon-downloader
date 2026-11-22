pub mod file_io;
pub mod probe;
pub mod splitter;
pub mod updater;
pub mod verifier;
pub mod worker;

pub use updater::{
    GITHUB_RELEASES_URL, GITHUB_REPO_URL, GITHUB_USER_URL, UpdateInfo, check_for_updates,
};

use async_trait::async_trait;
use siphon_core::{
    DownloadContext, DownloadOutcome, ProbeMetadata, ProtocolHandler, ProtocolKind, Result,
};
use std::collections::BTreeMap;

/// Pure-Rust HTTP/HTTPS dynamic multi-segment download engine.
#[derive(Debug, Default)]
pub struct HttpEngine;

impl HttpEngine {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProtocolHandler for HttpEngine {
    fn kind(&self) -> ProtocolKind {
        ProtocolKind::Http
    }

    fn supports_url(&self, url: &str) -> bool {
        let lower = url.trim().to_ascii_lowercase();
        lower.starts_with("http://") || lower.starts_with("https://")
    }

    async fn probe(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        proxy_url: Option<&str>,
        user_agent: &str,
    ) -> Result<ProbeMetadata> {
        probe::probe_http(url, headers, proxy_url, user_agent).await
    }

    async fn download(&self, ctx: DownloadContext) -> Result<DownloadOutcome> {
        worker::run_http_download(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::Response;
    use axum::routing::get;
    use sha2::{Digest, Sha256};
    use siphon_core::{
        ChecksumSpec, DownloadManager, HashAlgorithm, NewTaskRequest, SiphonError, TaskSource,
        TaskStatus,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct MockState {
        data: Arc<Vec<u8>>,
        etag: Arc<std::sync::Mutex<String>>,
        allow_range: Arc<AtomicBool>,
        slow_first_segment: Arc<AtomicBool>,
        request_counter: Arc<AtomicUsize>,
    }

    async fn serve_file(
        State(state): State<MockState>,
        headers: HeaderMap,
    ) -> Response<Body> {
        let req_num = state.request_counter.fetch_add(1, Ordering::SeqCst);
        let total = state.data.len();
        let etag = state.etag.lock().unwrap().clone();
        let allow_range = state.allow_range.load(Ordering::SeqCst);

        if let Some(range_val) = headers.get(header::RANGE).and_then(|v| v.to_str().ok())
            && allow_range
            && let Some(spec) = range_val.strip_prefix("bytes=")
            && let Some((s, e)) = spec.split_once('-')
        {
            // Check If-Range mismatch
            if let Some(if_range) = headers.get(header::IF_RANGE).and_then(|v| v.to_str().ok())
                && if_range != etag
            {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_LENGTH, total)
                    .header(header::ETAG, &etag)
                    .body(Body::from(state.data.as_ref().clone()))
                    .unwrap();
            }

            let start: usize = s.parse().unwrap_or(0);
            let end: usize = e.parse::<usize>().unwrap_or(total - 1).min(total - 1);
            let slice = state.data[start..=end].to_vec();

            // Optionally throttle chunk 0 stream to force dynamic work stealing!
            let slow_this = state.slow_first_segment.load(Ordering::SeqCst)
                && start == 0
                && end > 0
                && req_num >= 1;

            let body = if slow_this {
                let stream = futures_util::stream::unfold(
                    (slice, 0usize),
                    |(buf, pos)| async move {
                        if pos >= buf.len() {
                            return None;
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                        let next = (pos + 4096).min(buf.len());
                        Some((Ok::<_, std::io::Error>(buf[pos..next].to_vec()), (buf, next)))
                    },
                );
                Body::from_stream(stream)
            } else {
                Body::from(slice)
            };

            return Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
                .header(header::CONTENT_LENGTH, end - start + 1)
                .header(header::ETAG, &etag)
                .header(
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"payload.bin\"",
                )
                .body(body)
                .unwrap();
        }

        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::ETAG, &etag)
            .header(
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"payload.bin\"",
            );
        if allow_range {
            builder = builder
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, total);
        }
        builder
            .body(Body::from(state.data.as_ref().clone()))
            .unwrap()
    }

    async fn serve_forbidden() -> StatusCode {
        StatusCode::FORBIDDEN
    }

    async fn spawn_mock_server(
        size: usize,
        allow_range: bool,
        slow_first_segment: bool,
    ) -> (String, MockState) {
        let mut data = vec![0u8; size];
        for (i, b) in data.iter_mut().enumerate() {
            *b = ((i * 31 + 17) % 251) as u8;
        }
        let state = MockState {
            data: Arc::new(data),
            etag: Arc::new(std::sync::Mutex::new("\"etag-v1\"".to_string())),
            allow_range: Arc::new(AtomicBool::new(allow_range)),
            slow_first_segment: Arc::new(AtomicBool::new(slow_first_segment)),
            request_counter: Arc::new(AtomicUsize::new(0)),
        };

        let app = Router::new()
            .route("/file", get(serve_file).head(serve_file))
            .route("/expired", get(serve_forbidden).head(serve_forbidden))
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        (format!("http://{addr}"), state)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multi_segment_download_with_dynamic_stealing_and_sha256_verification() {
        let (base_url, state) = spawn_mock_server(512 * 1024, true, true).await;
        let expected_hash = hex::encode(Sha256::digest(state.data.as_ref()));

        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(HttpEngine::new())],
        )
        .unwrap();

        // Lower min_split_size for test so 512 KiB splits into multiple segments & steals!
        let mut cfg = mgr.config().await;
        cfg.min_split_size_bytes = 64 * 1024;
        cfg.default_connections = 4;
        mgr.update_config(cfg).await.unwrap();

        let mut req = NewTaskRequest::quick(format!("{base_url}/file"), TaskSource::Manual);
        req.max_connections = Some(4);
        req.checksum = Some(ChecksumSpec {
            algorithm: HashAlgorithm::Sha256,
            expected: expected_hash.clone(),
        });

        let task = mgr.add_task(req).await.unwrap();

        // Wait for completion
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            let current = mgr.get_task(task.id).await.unwrap();
            if current.status == TaskStatus::Completed {
                let bytes = std::fs::read(current.target_path()).unwrap();
                assert_eq!(bytes.len(), state.data.len());
                assert_eq!(hex::encode(Sha256::digest(&bytes)), expected_hash);
                // Slow segment 0 should have triggered dynamic stealing -> > 4 segments!
                assert!(
                    current.segments.len() > 4,
                    "expected dynamic work stealing to create > 4 segments, got {}",
                    current.segments.len()
                );
                break;
            }
            if matches!(current.status, TaskStatus::Error | TaskStatus::LinkExpired) {
                panic!("download failed unexpectedly: {:?}", current.error_message);
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "download timed out"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn non_range_server_falls_back_cleanly_to_single_stream() {
        let (base_url, state) = spawn_mock_server(128 * 1024, false, false).await;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(HttpEngine::new())],
        )
        .unwrap();

        let task = mgr
            .add_task(NewTaskRequest::quick(
                format!("{base_url}/file"),
                TaskSource::QuickBar,
            ))
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let current = mgr.get_task(task.id).await.unwrap();
            if current.status == TaskStatus::Completed {
                assert!(!current.supports_range);
                let bytes = std::fs::read(current.target_path()).unwrap();
                assert_eq!(bytes, *state.data);
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pause_and_resume_completes_byte_identical_file() {
        let (base_url, state) = spawn_mock_server(256 * 1024, true, true).await;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(HttpEngine::new())],
        )
        .unwrap();

        let mut cfg = mgr.config().await;
        cfg.min_split_size_bytes = 64 * 1024;
        mgr.update_config(cfg).await.unwrap();

        let task = mgr
            .add_task(NewTaskRequest::quick(
                format!("{base_url}/file"),
                TaskSource::Manual,
            ))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(120)).await;
        mgr.pause_task(task.id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;

        let paused = mgr.get_task(task.id).await.unwrap();
        assert!(
            matches!(paused.status, TaskStatus::Paused | TaskStatus::Completed),
            "expected paused or completed, got {:?}",
            paused.status
        );

        if paused.status == TaskStatus::Paused {
            state.slow_first_segment.store(false, Ordering::SeqCst);
            mgr.start_task(task.id).await.unwrap();
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let current = mgr.get_task(task.id).await.unwrap();
            if current.status == TaskStatus::Completed {
                let bytes = std::fs::read(current.target_path()).unwrap();
                assert_eq!(bytes, *state.data);
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn forbidden_url_reports_link_expired() {
        let (base_url, _) = spawn_mock_server(1024, true, false).await;
        let engine = HttpEngine::new();
        let err = engine
            .probe(
                &format!("{base_url}/expired"),
                &BTreeMap::new(),
                None,
                "SiphonTest/0.1",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, SiphonError::LinkExpired(403)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remote_etag_change_on_resume_triggers_safety_error() {
        let (base_url, state) = spawn_mock_server(256 * 1024, true, true).await;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(HttpEngine::new())],
        )
        .unwrap();

        let mut cfg = mgr.config().await;
        cfg.min_split_size_bytes = 64 * 1024;
        mgr.update_config(cfg).await.unwrap();

        let task = mgr
            .add_task(NewTaskRequest::quick(
                format!("{base_url}/file"),
                TaskSource::Manual,
            ))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(120)).await;
        mgr.pause_task(task.id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;

        let paused = mgr.get_task(task.id).await.unwrap();
        if paused.status == TaskStatus::Paused && !paused.segments.is_empty() {
            // Mutate remote ETag before resuming
            *state.etag.lock().unwrap() = "\"etag-v2-changed\"".to_string();
            mgr.start_task(task.id).await.unwrap();

            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let cur = mgr.get_task(task.id).await.unwrap();
                if cur.status == TaskStatus::Error {
                    assert!(
                        cur.error_message
                            .as_deref()
                            .unwrap_or("")
                            .contains("ETag"),
                        "expected ETag change error, got {:?}",
                        cur.error_message
                    );
                    break;
                }
                assert!(tokio::time::Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replace_expired_link_recovers_and_completes_download() {
        let (base_url, state) = spawn_mock_server(128 * 1024, true, false).await;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(HttpEngine::new())],
        )
        .unwrap();

        let task = mgr
            .add_task(NewTaskRequest::quick(
                format!("{base_url}/expired"),
                TaskSource::Manual,
            ))
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let cur = mgr.get_task(task.id).await.unwrap();
            if cur.status == TaskStatus::LinkExpired {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }

        // Now replace the expired URL with a valid mirror URL and auto-resume!
        mgr.replace_task_url(task.id, format!("{base_url}/file"), true)
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let cur = mgr.get_task(task.id).await.unwrap();
            if cur.status == TaskStatus::Completed {
                let bytes = std::fs::read(cur.target_path()).unwrap();
                assert_eq!(bytes, *state.data);
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}
