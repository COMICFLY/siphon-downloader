pub mod downloader;
pub mod playlist;

use async_trait::async_trait;
use siphon_core::{
    DownloadContext, DownloadOutcome, ProbeMetadata, ProtocolHandler, ProtocolKind, Result,
};
use std::collections::BTreeMap;

/// Pure-Rust HLS (`.m3u8`) streaming downloader supporting Master/Media playlists,
/// AES-128-CBC decryption, parallel segment fetching, pause/resume, and ordered merging.
#[derive(Debug, Default)]
pub struct HlsEngine;

impl HlsEngine {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProtocolHandler for HlsEngine {
    fn kind(&self) -> ProtocolKind {
        ProtocolKind::Hls
    }

    fn supports_url(&self, url: &str) -> bool {
        playlist::is_m3u8_url(url)
    }

    async fn probe(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        proxy_url: Option<&str>,
        user_agent: &str,
    ) -> Result<ProbeMetadata> {
        downloader::probe_hls(url, headers, proxy_url, user_agent).await
    }

    async fn download(&self, ctx: DownloadContext) -> Result<DownloadOutcome> {
        downloader::run_hls_download(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::Aes128;
    use axum::Router;
    use axum::extract::Path as AxumPath;
    use axum::routing::get;
    use cbc::cipher::block_padding::Pkcs7;
    use cbc::cipher::{BlockEncryptMut, KeyIvInit};
    use siphon_core::{DownloadManager, NewTaskRequest, TaskSource, TaskStatus};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::net::TcpListener;

    type Aes128CbcEnc = cbc::Encryptor<Aes128>;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn master_playlist_aes128_hls_downloads_and_merges_byte_identical() {
        let key: [u8; 16] = *b"0123456789abcdef";
        let plain_chunks: Vec<Vec<u8>> = (0..6)
            .map(|i| vec![b'A' + i as u8; 4096 + i * 37])
            .collect();
        let expected_full: Vec<u8> = plain_chunks.iter().flatten().copied().collect();

        let enc_chunks: Arc<Vec<Vec<u8>>> = Arc::new(
            plain_chunks
                .iter()
                .enumerate()
                .map(|(idx, plain)| {
                    let mut iv = [0u8; 16];
                    iv[8..16].copy_from_slice(&(10u64 + idx as u64).to_be_bytes());
                    let enc = Aes128CbcEnc::new_from_slices(&key, &iv).unwrap();
                    let mut buf = vec![0u8; plain.len() + 16];
                    buf[..plain.len()].copy_from_slice(plain);
                    let ct = enc.encrypt_padded_mut::<Pkcs7>(&mut buf, plain.len()).unwrap();
                    ct.to_vec()
                })
                .collect(),
        );

        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=4500000\nhigh.m3u8\n";
        let mut media = String::from(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-MEDIA-SEQUENCE:10\n#EXT-X-KEY:METHOD=AES-128,URI=\"enc.key\"\n",
        );
        for i in 0..6 {
            media.push_str(&format!("#EXTINF:4.0,\nseg/{i}.ts\n"));
        }
        media.push_str("#EXT-X-ENDLIST\n");

        let enc_for_route = Arc::clone(&enc_chunks);
        let app = Router::new()
            .route("/video/master.m3u8", get(move || async move { master }))
            .route("/video/high.m3u8", get(move || async move { media.clone() }))
            .route("/video/enc.key", get(move || async move { key.to_vec() }))
            .route(
                "/video/seg/{name}",
                get(move |AxumPath(name): AxumPath<String>| {
                    let chunks = Arc::clone(&enc_for_route);
                    async move {
                        let idx: usize = name.trim_end_matches(".ts").parse().unwrap_or(0);
                        chunks[idx].clone()
                    }
                }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(HlsEngine::new())],
        )
        .unwrap();

        let task = mgr
            .add_task(NewTaskRequest::quick(
                format!("http://{addr}/video/master.m3u8"),
                TaskSource::Browser,
            ))
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let cur = mgr.get_task(task.id).await.unwrap();
            if cur.status == TaskStatus::Completed {
                let bytes = std::fs::read(cur.target_path()).unwrap();
                assert_eq!(bytes, expected_full);
                break;
            }
            if cur.status == TaskStatus::Error {
                panic!("HLS download failed: {:?}", cur.error_message);
            }
            assert!(tokio::time::Instant::now() < deadline, "HLS timed out");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}
