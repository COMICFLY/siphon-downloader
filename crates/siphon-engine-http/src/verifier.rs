use md5::Md5;
use sha2::{Digest, Sha256};
use siphon_core::{ChecksumSpec, HashAlgorithm, Result, SiphonError};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

pub async fn verify_file_checksum(path: PathBuf, spec: ChecksumSpec) -> Result<String> {
    tokio::task::spawn_blocking(move || compute_and_check(&path, &spec))
        .await
        .map_err(|e| SiphonError::Network(format!("校验任务异常: {e}")))?
}

fn compute_and_check(path: &PathBuf, spec: &ChecksumSpec) -> Result<String> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 256 * 1024];

    let actual = match spec.algorithm {
        HashAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            hex::encode(hasher.finalize())
        }
        HashAlgorithm::Md5 => {
            let mut hasher = Md5::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            hex::encode(hasher.finalize())
        }
        HashAlgorithm::Blake3 => {
            let mut hasher = blake3::Hasher::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            hasher.finalize().to_hex().to_string()
        }
    };

    let expected = spec.expected.trim().to_ascii_lowercase();
    if !expected.is_empty() && actual != expected {
        return Err(SiphonError::ChecksumMismatch { expected, actual });
    }
    Ok(actual)
}
